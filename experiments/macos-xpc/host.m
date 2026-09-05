#import <Foundation/Foundation.h>
#include <xpc/xpc.h>
#include <fcntl.h>
#include <time.h>
#include <unistd.h>
#include "common.h"

static xpc_connection_t connection(void) {
    xpc_connection_t peer = xpc_connection_create(TR_PROBE_SERVICE, NULL);
    if (xpc_connection_set_peer_code_signing_requirement(peer, "identifier \"" TR_PROBE_SERVICE "\"") != 0) {
        fprintf(stderr, "Invalid XPC peer requirement\n");
        exit(3);
    }
    xpc_connection_set_event_handler(peer, ^(xpc_object_t event) { (void)event; });
    xpc_connection_resume(peer);
    return peer;
}

static xpc_object_t request(const char *operation, uint64_t identity) {
    xpc_object_t message = xpc_dictionary_create(NULL, NULL, 0);
    xpc_dictionary_set_uint64(message, "version", TR_PROBE_VERSION);
    xpc_dictionary_set_uint64(message, "id", identity);
    xpc_dictionary_set_string(message, "operation", operation);
    return message;
}

static xpc_object_t exchange(xpc_connection_t peer, xpc_object_t message) {
    dispatch_semaphore_t ready = dispatch_semaphore_create(0);
    __block xpc_object_t response = NULL;
    xpc_connection_send_message_with_reply(peer, message,
        dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^(xpc_object_t reply) {
            response = reply;
            dispatch_semaphore_signal(ready);
        });
    if (dispatch_semaphore_wait(ready, dispatch_time(DISPATCH_TIME_NOW, 15 * NSEC_PER_SEC)) != 0) {
        xpc_connection_cancel(peer);
        return NULL;
    }
    return response;
}

// Copy only a tiny, bounded tree of expected primitive types; never log source data.
static id copy_value(xpc_object_t value, unsigned depth) {
    if (!value || depth > 2) return nil;
    xpc_type_t type = xpc_get_type(value);
    if (type == XPC_TYPE_BOOL) return @(xpc_bool_get_value(value));
    if (type == XPC_TYPE_INT64) return @(xpc_int64_get_value(value));
    if (type == XPC_TYPE_UINT64) return @(xpc_uint64_get_value(value));
    if (type != XPC_TYPE_DICTIONARY || xpc_dictionary_get_count(value) > 40) return nil;
    NSMutableDictionary *dictionary = [NSMutableDictionary dictionary];
    __block BOOL valid = YES;
    xpc_dictionary_apply(value, ^bool(const char *key, xpc_object_t child) {
        if (strlen(key) > 64) { valid = NO; return false; }
        id copy = copy_value(child, depth + 1);
        NSString *name = [NSString stringWithUTF8String:key];
        if (!copy || !name) { valid = NO; return false; }
        dictionary[name] = copy;
        return true;
    });
    return valid ? dictionary : nil;
}

static NSDictionary *checked_reply(xpc_object_t reply, uint64_t identity) {
    if (!reply || xpc_get_type(reply) != XPC_TYPE_DICTIONARY
        || xpc_dictionary_get_uint64(reply, "version") != TR_PROBE_VERSION
        || xpc_dictionary_get_uint64(reply, "id") != identity) return nil;
    return copy_value(reply, 0);
}

static BOOL interrupted(xpc_object_t reply) {
    return reply && xpc_get_type(reply) == XPC_TYPE_ERROR;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 4) {
            fprintf(stderr, "usage: TrueRendererSandboxProbe synthetic-source create-path loopback-port\n");
            return 2;
        }
        char *end = NULL;
        unsigned long port = strtoul(argv[3], &end, 10);
        if (!end || *end || port == 0 || port > 65535) return 2;
        int fd = open(argv[1], O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
        if (fd < 0) { perror("open synthetic source"); return 2; }
        NSDate *start = [NSDate date];
        xpc_connection_t first = connection();
        NSDictionary *initial = checked_reply(exchange(first, request("ping", 1)), 1);
        if (!initial) { fprintf(stderr, "XPC initial ping failed or timed out\n"); close(fd); return 3; }

        xpc_connection_t second = connection();
        NSDictionary *other = checked_reply(exchange(second, request("ping", 2)), 2);
        xpc_object_t inspect = request("inspect", 3);
        xpc_dictionary_set_string(inspect, "read_path", argv[1]);
        xpc_dictionary_set_string(inspect, "create_path", argv[2]);
        xpc_dictionary_set_fd(inspect, "source", fd);
        xpc_dictionary_set_uint64(inspect, "port", port);
        NSDictionary *capabilities = checked_reply(exchange(first, inspect), 3);
        close(fd);
        if (!other || !capabilities) { fprintf(stderr, "XPC probe response invalid\n"); return 3; }

        // A malformed request must close only its connection, then another peer remains usable.
        xpc_connection_t malformed = connection();
        xpc_object_t invalid = request("ping", 4);
        xpc_dictionary_set_uint64(invalid, "version", 999);
        BOOL version_closed = interrupted(exchange(malformed, invalid));
        NSDictionary *after_invalid = checked_reply(exchange(first, request("ping", 5)), 5);

        int64_t old_pid = [initial[@"pid"] longLongValue];
        BOOL crash_contained = interrupted(exchange(first, request("crash", 6)));
        xpc_connection_t recovered = connection();
        NSDictionary *recovery = checked_reply(exchange(recovered, request("ping", 7)), 7);
        BOOL restarted = recovery && [recovery[@"pid"] longLongValue] != old_pid;

        BOOL basic = [capabilities[@"app_sandbox_entitlement"] boolValue]
            && [capabilities[@"external_read"][@"denied"] boolValue]
            && [capabilities[@"external_write"][@"denied"] boolValue]
            && [capabilities[@"external_create"][@"denied"] boolValue]
            && [capabilities[@"network_connect"][@"denied"] boolValue]
            && [capabilities[@"network_listen"][@"denied"] boolValue]
            && [capabilities[@"fd_read_matches"] boolValue]
            && [capabilities[@"fd_write_denied"] boolValue];
        NSDictionary *report = @{
            @"application": @"TrueRenderer", @"experiment": @"macOS XPC capability probe 1",
            @"policy": TR_PROBE_LIMIT_CHILDREN ? @"App Sandbox + RLIMIT_NPROC hard/soft 0" : @"App Sandbox only",
            @"elapsed_seconds": @(-[start timeIntervalSinceNow]),
            @"capabilities": capabilities,
            @"first_connection_pid": initial[@"pid"], @"second_connection_pid": other[@"pid"],
            @"connections_share_process": @((BOOL)([other[@"pid"] longLongValue] == old_pid)),
            @"unknown_version_connection_closed": @(version_closed),
            @"other_connection_survived_invalid_request": @((BOOL)(after_invalid != nil)),
            @"crash_contained": @(crash_contained), @"restarted_after_crash": @(restarted),
            @"recovered_pid": recovery[@"pid"] ?: @0,
            @"basic_files_network_fd_checks_passed": @(basic),
            @"protocol_lifecycle_checks_passed": @((BOOL)(version_closed && after_invalid && crash_contained && restarted)),
            @"full_sandbox_gate_passed": @NO,
            @"scope": @"Synthetic probe only; no decoder integration, memory cap, Windows or production signing qualification"
        };
        NSData *json = [NSJSONSerialization dataWithJSONObject:report options:NSJSONWritingPrettyPrinted | NSJSONWritingSortedKeys error:nil];
        if (!json) return 4;
        fwrite(json.bytes, 1, json.length, stdout);
        fputc('\n', stdout);
        xpc_connection_cancel(first);
        xpc_connection_cancel(second);
        xpc_connection_cancel(malformed);
        xpc_connection_cancel(recovered);
        return 0;
    }
}
