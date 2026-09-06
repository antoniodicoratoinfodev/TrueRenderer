#import "signing.h"
#include "xpc_bridge.h"
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <mach/mach.h>
#include <signal.h>
#include <stdatomic.h>
#include <unistd.h>
#include <time.h>

// libproc SPI from the installed SDK, resolved explicitly. No kill(pid) fallback.
// This adapter is internal R0; OS/API support remains a release qualification gate.
typedef int (*TrSignalAudit)(audit_token_t *, int);
static atomic_bool poisoned[2];

@interface TRXpcLease : NSObject {
@public
    xpc_connection_t peer;
    mach_port_t task;
    audit_token_t token;
    BOOL authenticated;
    BOOL stopped;
    uint32_t slot;
    pid_t pid;
    TrSignalAudit signalAudit;
}
@end
@implementation TRXpcLease
- (void)dealloc {
    if (authenticated && !stopped && signalAudit) (void)signalAudit(&token, SIGKILL);
    if (peer) xpc_connection_cancel(peer);
    if (task) mach_port_deallocate(mach_task_self(), task);
}
@end

static void describe(char *error, size_t size, const char *text) {
    if (error && size) snprintf(error, size, "%s", text);
}
static xpc_object_t message(const char *operation, uint64_t identity) {
    xpc_object_t request = xpc_dictionary_create(NULL, NULL, 0);
    xpc_dictionary_set_uint64(request, "version", 1);
    xpc_dictionary_set_uint64(request, "id", identity);
    xpc_dictionary_set_string(request, "operation", operation);
    return request;
}
static xpc_object_t exchange(xpc_connection_t peer, xpc_object_t request, uint32_t timeout) {
    dispatch_semaphore_t ready = dispatch_semaphore_create(0);
    __block xpc_object_t response = NULL;
    xpc_connection_send_message_with_reply(peer, request,
        dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^(xpc_object_t value) {
            response = value;
            dispatch_semaphore_signal(ready);
        });
    if (dispatch_semaphore_wait(ready, dispatch_time(DISPATCH_TIME_NOW, (int64_t)timeout * NSEC_PER_MSEC)) != 0)
        return NULL;
    return response;
}
static BOOL valid_reply(xpc_object_t reply, uint64_t identity, size_t count) {
    if (!reply || xpc_get_type(reply) != XPC_TYPE_DICTIONARY || xpc_dictionary_get_count(reply) != count) return NO;
    for (const char **key = (const char *[]){"version", "id", NULL}; *key; ++key) {
        xpc_object_t value = xpc_dictionary_get_value(reply, *key);
        if (!value || xpc_get_type(value) != XPC_TYPE_UINT64) return NO;
    }
    return xpc_dictionary_get_uint64(reply, "version") == 1 && xpc_dictionary_get_uint64(reply, "id") == identity;
}

void *tr_xpc_open(uint32_t slot, uint32_t timeout_ms, int *input, int *output, char *error, size_t error_size) {
    @autoreleasepool {
        if (slot > 1 || !input || !output || timeout_ms == 0 || timeout_ms > 12000) return NULL;
        *input = -1; *output = -1;
        if (atomic_load(&poisoned[slot])) {
            describe(error, error_size, "Isolato XPC non recuperabile: riavviare l'app"); return NULL;
        }
        TRXpcLease *lease = [TRXpcLease new];
        lease->slot = slot;
        lease->signalAudit = (TrSignalAudit)dlsym(RTLD_DEFAULT, "proc_signal_with_audittoken");
        if (!lease->signalAudit) {
            describe(error, error_size, "Controllo audit-token non disponibile su questo macOS"); return NULL;
        }
        NSString *identifier = [NSString stringWithFormat:@"it.truerenderer.prototype.decoder.%u", slot];
        NSURL *service = [tr_executable_bundle() URLByAppendingPathComponent:
            [NSString stringWithFormat:@"Contents/XPCServices/Decoder%u.xpc", slot]];
        NSString *requirement = tr_code_requirement(service, identifier);
        if (!requirement) {
            describe(error, error_size, "Firma del servizio XPC assente o non valida"); return NULL;
        }
        lease->peer = xpc_connection_create(identifier.UTF8String, NULL);
        if (xpc_connection_set_peer_code_signing_requirement(lease->peer, requirement.UTF8String) != 0) {
            // A connection must be resumed before normal ARC disposal.
            xpc_connection_set_event_handler(lease->peer, ^(xpc_object_t event) { (void)event; });
            xpc_connection_resume(lease->peer);
            describe(error, error_size, "Requisito di firma XPC non valido"); return NULL;
        }
        xpc_connection_set_event_handler(lease->peer, ^(xpc_object_t event) { (void)event; });
        xpc_connection_resume(lease->peer);
        uint64_t began = clock_gettime_nsec_np(CLOCK_MONOTONIC);
        xpc_object_t hello = exchange(lease->peer, message("hello", 1), timeout_ms);
        if (!valid_reply(hello, 1, 3)) {
            describe(error, error_size, "Handshake XPC rifiutato, scaduto o con firma inattesa"); return NULL;
        }
        lease->task = xpc_dictionary_copy_mach_send(hello, "task");
        mach_msg_type_number_t count = TASK_AUDIT_TOKEN_COUNT;
        if (!lease->task || pid_for_task(lease->task, &lease->pid) != KERN_SUCCESS
            || lease->pid <= 0 || lease->pid == getpid() || lease->pid != xpc_connection_get_pid(lease->peer)
            || task_info(lease->task, TASK_AUDIT_TOKEN, (task_info_t)&lease->token, &count) != KERN_SUCCESS) {
            describe(error, error_size, "Riferimento del kernel non corrisponde al servizio XPC"); return NULL;
        }
        lease->authenticated = YES;
        int source[2] = {-1, -1}, pixels[2] = {-1, -1};
        if (pipe(source) != 0 || pipe(pixels) != 0) {
            for (int i = 0; i < 2; ++i) { if (source[i] >= 0) close(source[i]); if (pixels[i] >= 0) close(pixels[i]); }
            describe(error, error_size, "Creazione pipe XPC non riuscita"); return NULL;
        }
        for (int i = 0; i < 2; ++i) { (void)fcntl(source[i], F_SETFD, FD_CLOEXEC); (void)fcntl(pixels[i], F_SETFD, FD_CLOEXEC); }
        xpc_object_t start = message("start", 2);
        xpc_dictionary_set_fd(start, "input", source[0]);
        xpc_dictionary_set_fd(start, "output", pixels[1]);
        close(source[0]); close(pixels[1]);
        uint64_t elapsed = (clock_gettime_nsec_np(CLOCK_MONOTONIC) - began) / NSEC_PER_MSEC;
        uint32_t remaining = elapsed >= timeout_ms ? 1 : timeout_ms - (uint32_t)elapsed;
        xpc_object_t started = exchange(lease->peer, start, remaining);
        if (!valid_reply(started, 2, 2)) {
            close(source[1]); close(pixels[0]);
            describe(error, error_size, "Attivazione decoder XPC non riuscita"); return NULL;
        }
        *input = source[1]; *output = pixels[0];
        return (__bridge_retained void *)lease;
    }
}

int tr_xpc_usage(void *pointer, uint64_t *rss, uint64_t *footprint, int *pid) {
    TRXpcLease *lease = (__bridge TRXpcLease *)pointer;
    if (!lease || !lease->authenticated || lease->stopped) return -1;
    task_vm_info_data_t info = {0};
    mach_msg_type_number_t count = TASK_VM_INFO_COUNT;
    kern_return_t status = task_info(lease->task, TASK_VM_INFO, (task_info_t)&info, &count);
    if (status != KERN_SUCCESS) return (int)status;
    if (rss) *rss = info.resident_size;
    if (footprint) *footprint = info.phys_footprint;
    if (pid) *pid = lease->pid;
    return 0;
}
int tr_xpc_suspend_for_test(void *pointer) {
    TRXpcLease *lease = (__bridge TRXpcLease *)pointer;
    return lease && lease->authenticated && !lease->stopped ? task_suspend(lease->task) : KERN_FAILURE;
}
int tr_xpc_inject_growth_for_test(void *pointer) {
    TRXpcLease *lease = (__bridge TRXpcLease *)pointer;
    if (!lease || !lease->authenticated || lease->stopped) return -1;
    xpc_object_t reply = exchange(lease->peer, message("grow", 3), 1000);
    return valid_reply(reply, 3, 2) ? 0 : -1;
}
int tr_xpc_stop(void *pointer) {
    TRXpcLease *lease = (__bridge TRXpcLease *)pointer;
    if (!lease || lease->stopped) return 0;
    int status = lease->authenticated ? lease->signalAudit(&lease->token, SIGKILL) : 0;
    xpc_connection_cancel(lease->peer);
    if (status != 0 && status != ESRCH) {
        atomic_store(&poisoned[lease->slot], true);
        return status;
    }
    // A successful signal is a request; confirm the retained task has actually gone away.
    mach_task_basic_info_data_t info = {0};
    for (int attempt = 0; attempt < 100; ++attempt) {
        mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
        if (task_info(lease->task, MACH_TASK_BASIC_INFO, (task_info_t)&info, &count) != KERN_SUCCESS) {
            lease->stopped = YES;
            return 0;
        }
        usleep(5000);
    }
    atomic_store(&poisoned[lease->slot], true);
    return ETIMEDOUT;
}
void tr_xpc_release(void *pointer) {
    if (pointer) { TRXpcLease *lease = (__bridge_transfer TRXpcLease *)pointer; (void)lease; }
}
