#import <Foundation/Foundation.h>
#import <Security/SecTask.h>
#include <xpc/xpc.h>
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <mach/mach.h>
#include <poll.h>
#include <spawn.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>
#include "common.h"

static BOOL denied(int error) { return error == EPERM || error == EACCES; }

static void result(xpc_object_t reply, const char *name, int error) {
    xpc_object_t item = xpc_dictionary_create(NULL, NULL, 0);
    xpc_dictionary_set_int64(item, "errno", error);
    xpc_dictionary_set_bool(item, "denied", denied(error));
    xpc_dictionary_set_value(reply, name, item);
}

static int try_open(const char *path, int flags) {
    int fd = open(path, flags | O_CLOEXEC | O_NOFOLLOW, 0600);
    int error = fd < 0 ? errno : 0;
    if (fd >= 0) close(fd);
    return error;
}

static int try_connect(uint16_t port) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return errno;
    (void)fcntl(fd, F_SETFL, O_NONBLOCK);
    struct sockaddr_in address = { .sin_len = sizeof(address), .sin_family = AF_INET,
        .sin_port = htons(port), .sin_addr.s_addr = htonl(INADDR_LOOPBACK) };
    int error = connect(fd, (const struct sockaddr *)&address, sizeof(address)) < 0 ? errno : 0;
    if (error == EINPROGRESS) {
        struct pollfd pfd = { .fd = fd, .events = POLLOUT };
        int ready = poll(&pfd, 1, 1000);
        if (ready > 0) {
            socklen_t length = sizeof(error);
            if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &length) < 0) error = errno;
        } else error = ready == 0 ? ETIMEDOUT : errno;
    }
    close(fd);
    return error;
}

static int try_listen(void) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return errno;
    struct sockaddr_in address = { .sin_len = sizeof(address), .sin_family = AF_INET,
        .sin_addr.s_addr = htonl(INADDR_LOOPBACK) };
    int error = bind(fd, (const struct sockaddr *)&address, sizeof(address)) < 0 ? errno : 0;
    if (error == 0 && listen(fd, 1) < 0) error = errno;
    close(fd);
    return error;
}

static uint64_t resident_bytes(void) {
    mach_task_basic_info_data_t info = {0};
    mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
    if (task_info(mach_task_self(), MACH_TASK_BASIC_INFO, (task_info_t)&info, &count) != KERN_SUCCESS)
        return 0;
    return info.resident_size;
}

static void inspect(xpc_object_t event, xpc_object_t reply) {
    const char *read_path = xpc_dictionary_get_string(event, "read_path");
    const char *create_path = xpc_dictionary_get_string(event, "create_path");
    result(reply, "external_read", try_open(read_path, O_RDONLY));
    result(reply, "external_write", try_open(read_path, O_WRONLY));
    result(reply, "external_create", try_open(create_path, O_WRONLY | O_CREAT | O_EXCL));
    result(reply, "network_connect", try_connect((uint16_t)xpc_dictionary_get_uint64(event, "port")));
    result(reply, "network_listen", try_listen());

    int fd = xpc_dictionary_dup_fd(event, "source");
    char bytes[sizeof(TR_PROBE_SOURCE)] = {0};
    ssize_t length = fd < 0 ? -1 : pread(fd, bytes, sizeof(bytes), 0);
    BOOL source_ok = length == (ssize_t)strlen(TR_PROBE_SOURCE)
        && memcmp(bytes, TR_PROBE_SOURCE, strlen(TR_PROBE_SOURCE)) == 0;
    xpc_dictionary_set_bool(reply, "fd_read_matches", source_ok);
    errno = 0;
    ssize_t written = fd < 0 ? -1 : pwrite(fd, "!", 1, 0);
    int write_error = fd < 0 ? EBADF : (written < 0 ? errno : 0);
    xpc_dictionary_set_int64(reply, "fd_write_errno", write_error);
    xpc_dictionary_set_bool(reply, "fd_write_denied", written < 0 && write_error == EBADF);
    if (fd >= 0) close(fd);

    // Only /usr/bin/true with a fixed, empty-of-secrets environment is attempted.
    char *const arguments[] = { "/usr/bin/true", NULL };
    char *const environment[] = { "PATH=/usr/bin:/bin", NULL };
    pid_t child = -1;
    int spawn_error = posix_spawn(&child, arguments[0], NULL, NULL, arguments, environment);
    int child_status = -1;
    if (spawn_error == 0) {
        while (waitpid(child, &child_status, 0) < 0 && errno == EINTR) {}
    }
    result(reply, "child_spawn", spawn_error);
    xpc_dictionary_set_int64(reply, "child_status", child_status);
    if (TR_PROBE_LIMIT_CHILDREN) {
        struct rlimit observed = {0};
        BOOL limited = getrlimit(RLIMIT_NPROC, &observed) == 0
            && observed.rlim_cur == 0 && observed.rlim_max == 0;
        struct rlimit raised = { .rlim_cur = 1, .rlim_max = 1 };
        int raised_result = setrlimit(RLIMIT_NPROC, &raised);
        int raised_error = raised_result == 0 ? 0 : errno;
        xpc_dictionary_set_int64(reply, "nproc_raise_errno", raised_error);
        xpc_dictionary_set_bool(reply, "nproc_hard_zero", limited);
        xpc_dictionary_set_bool(reply, "nproc_policy_checks_passed",
            limited && spawn_error == EAGAIN && raised_result < 0 && raised_error == EPERM);
    }

    // A small bounded observation, not an OOM test or a kernel memory cap.
    uint64_t before = resident_bytes();
    size_t size = 16 * 1024 * 1024;
    volatile unsigned char *memory = malloc(size);
    if (memory) for (size_t i = 0; i < size; i += 4096) memory[i] = 0x7f;
    xpc_dictionary_set_uint64(reply, "rss_before_bytes", before);
    xpc_dictionary_set_uint64(reply, "rss_touched_bytes", resident_bytes());
    xpc_dictionary_set_uint64(reply, "allocation_bytes", memory ? size : 0);
    free((void *)memory);
    xpc_dictionary_set_bool(reply, "hard_memory_cap_demonstrated", false);
}

static BOOL valid_request(xpc_object_t event) {
    xpc_object_t version = xpc_dictionary_get_value(event, "version");
    xpc_object_t request = xpc_dictionary_get_value(event, "id");
    xpc_object_t operation = xpc_dictionary_get_value(event, "operation");
    if (!version || xpc_get_type(version) != XPC_TYPE_UINT64 || xpc_uint64_get_value(version) != TR_PROBE_VERSION
        || !request || xpc_get_type(request) != XPC_TYPE_UINT64 || xpc_uint64_get_value(request) == 0
        || !operation || xpc_get_type(operation) != XPC_TYPE_STRING || xpc_string_get_length(operation) > 16)
        return NO;
    const char *name = xpc_string_get_string_ptr(operation);
    if (strcmp(name, "ping") == 0 || strcmp(name, "crash") == 0)
        return xpc_dictionary_get_count(event) == 3;
    if (strcmp(name, "inspect") != 0 || xpc_dictionary_get_count(event) != 7) return NO;
    for (const char **key = (const char *[]){"read_path", "create_path", NULL}; *key; ++key) {
        xpc_object_t path = xpc_dictionary_get_value(event, *key);
        if (!path || xpc_get_type(path) != XPC_TYPE_STRING || xpc_string_get_length(path) == 0
            || xpc_string_get_length(path) >= PATH_MAX) return NO;
    }
    xpc_object_t source = xpc_dictionary_get_value(event, "source");
    xpc_object_t port = xpc_dictionary_get_value(event, "port");
    return source && xpc_get_type(source) == XPC_TYPE_FD && port && xpc_get_type(port) == XPC_TYPE_UINT64
        && xpc_uint64_get_value(port) > 0 && xpc_uint64_get_value(port) <= 65535;
}

static void peer_handler(xpc_connection_t peer) {
    // Probe-only ad hoc identity: a production build also needs a pinned signer.
    if (xpc_connection_set_peer_code_signing_requirement(peer, "identifier \"" TR_PROBE_HOST "\"") != 0) {
        xpc_connection_cancel(peer);
        return;
    }
    xpc_connection_set_event_handler(peer, ^(xpc_object_t event) {
        @autoreleasepool {
            if (xpc_get_type(event) != XPC_TYPE_DICTIONARY) return;
            xpc_connection_t remote = xpc_dictionary_get_remote_connection(event);
            if (!valid_request(event)) {
                xpc_connection_cancel(remote);
                return;
            }
            const char *operation = xpc_dictionary_get_string(event, "operation");
            if (strcmp(operation, "crash") == 0) _exit(75);
            xpc_object_t reply = xpc_dictionary_create_reply(event);
            if (!reply) return;
            xpc_dictionary_set_uint64(reply, "version", TR_PROBE_VERSION);
            xpc_dictionary_set_uint64(reply, "id", xpc_dictionary_get_uint64(event, "id"));
            xpc_dictionary_set_int64(reply, "pid", getpid());
            SecTaskRef task = SecTaskCreateFromSelf(NULL);
            CFTypeRef enabled = task ? SecTaskCopyValueForEntitlement(task, CFSTR("com.apple.security.app-sandbox"), NULL) : NULL;
            xpc_dictionary_set_bool(reply, "app_sandbox_entitlement", enabled && CFEqual(enabled, kCFBooleanTrue));
            if (enabled) CFRelease(enabled);
            if (task) CFRelease(task);
            if (strcmp(operation, "inspect") == 0) inspect(event, reply);
            xpc_connection_send_message(remote, reply);
        }
    });
    xpc_connection_resume(peer);
}

int main(void) {
    if (TR_PROBE_LIMIT_CHILDREN) {
        // These limits belong only to this service process, never to the shell or host.
        struct rlimit children = { .rlim_cur = 0, .rlim_max = 0 };
        if (setrlimit(RLIMIT_NPROC, &children) != 0) {
            perror("cannot install worker child-process limit");
            return 78;
        }
    }
    xpc_main(peer_handler);
}
