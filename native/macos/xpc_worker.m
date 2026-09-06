#import <Foundation/Foundation.h>
#import <Security/SecTask.h>
#include <xpc/xpc.h>
#include <mach/mach.h>
#include <fcntl.h>
#include <stdatomic.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <unistd.h>

extern int tr_worker_serve_fds(int input, int output);
static atomic_bool occupied;
static atomic_bool started;

static BOOL valid(xpc_object_t request, const char *operation, uint64_t id, size_t count) {
    if (xpc_dictionary_get_count(request) != count) return NO;
    xpc_object_t version = xpc_dictionary_get_value(request, "version");
    xpc_object_t identity = xpc_dictionary_get_value(request, "id");
    xpc_object_t name = xpc_dictionary_get_value(request, "operation");
    return version && xpc_get_type(version) == XPC_TYPE_UINT64 && xpc_uint64_get_value(version) == 1
        && identity && xpc_get_type(identity) == XPC_TYPE_UINT64 && xpc_uint64_get_value(identity) == id
        && name && xpc_get_type(name) == XPC_TYPE_STRING && xpc_string_get_length(name) <= 8
        && strcmp(xpc_string_get_string_ptr(name), operation) == 0;
}
static BOOL pipe_mode(int fd, int mode) {
    struct stat info = {0};
    int flags = fcntl(fd, F_GETFL);
    return fd >= 0 && flags >= 0 && (flags & O_ACCMODE) == mode
        && fstat(fd, &info) == 0 && S_ISFIFO(info.st_mode);
}
static void accept_peer(xpc_connection_t peer) {
    if (atomic_exchange(&occupied, true)) { xpc_connection_cancel(peer); return; }
    // Private embedded XPC namespace + identity. Developer ID signer is a release gate.
    if (xpc_connection_set_peer_code_signing_requirement(peer, "identifier \"it.truerenderer.prototype\"") != 0) {
        xpc_connection_cancel(peer); _exit(78);
    }
    __block BOOL helloSent = NO;
    xpc_connection_set_event_handler(peer, ^(xpc_object_t request) {
        if (xpc_get_type(request) == XPC_TYPE_ERROR) {
#ifdef TR_XPC_FAULT_INJECTION
            return; // Qualification variant deliberately ignores cooperative cancellation.
#else
            _exit(0);
#endif
        }
        if (xpc_get_type(request) != XPC_TYPE_DICTIONARY) _exit(65);
        xpc_connection_t remote = xpc_dictionary_get_remote_connection(request);
        xpc_object_t reply = xpc_dictionary_create_reply(request);
        if (!reply) _exit(65);
        xpc_dictionary_set_uint64(reply, "version", 1);
        if (!helloSent && valid(request, "hello", 1, 3)) {
            xpc_dictionary_set_uint64(reply, "id", 1);
            xpc_dictionary_set_mach_send(reply, "task", mach_task_self());
            xpc_connection_send_message(remote, reply);
            helloSent = YES;
        } else if (helloSent && !atomic_load(&started) && valid(request, "start", 2, 5)) {
            int input = xpc_dictionary_dup_fd(request, "input");
            int output = xpc_dictionary_dup_fd(request, "output");
            if (!pipe_mode(input, O_RDONLY) || !pipe_mode(output, O_WRONLY) || input == output) {
                if (input >= 0) close(input); if (output >= 0) close(output); _exit(65);
            }
            atomic_store(&started, true);
            xpc_dictionary_set_uint64(reply, "id", 2);
            xpc_connection_send_message(remote, reply);
            dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
                // Rust consumes both duplicate FDs; no paths enter the codec.
                int result = tr_worker_serve_fds(input, output);
                _exit(result);
            });
        }
#ifdef TR_XPC_FAULT_INJECTION
        else if (atomic_load(&started) && valid(request, "grow", 3, 3)) {
            xpc_dictionary_set_uint64(reply, "id", 3);
            xpc_connection_send_message(remote, reply);
            dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
                const size_t chunk = 1024 * 1024, size = 512 * chunk;
                volatile unsigned char *memory = malloc(size);
                if (!memory) _exit(74);
                for (size_t base = 0; base < size; base += chunk) {
                    for (size_t index = base; index < base + chunk; index += 4096) memory[index] = 0x7f;
                    usleep(2000);
                }
                for (;;) sleep(1); // Only the broker/OS can end this bounded 512 MiB fault.
            });
        }
#endif
        else _exit(65);
    });
    xpc_connection_resume(peer);
}
int main(void) {
    struct rlimit children = {0, 0}, core = {0, 0}, descriptors = {128, 128}, cpu = {60, 60};
    if (setrlimit(RLIMIT_NPROC, &children) != 0 || setrlimit(RLIMIT_CORE, &core) != 0
        || setrlimit(RLIMIT_NOFILE, &descriptors) != 0 || setrlimit(RLIMIT_CPU, &cpu) != 0) return 78;
    SecTaskRef task = SecTaskCreateFromSelf(NULL);
    CFTypeRef enabled = task ? SecTaskCopyValueForEntitlement(task, CFSTR("com.apple.security.app-sandbox"), NULL) : NULL;
    BOOL sandboxed = enabled && CFEqual(enabled, kCFBooleanTrue);
    if (enabled) CFRelease(enabled); if (task) CFRelease(task);
    if (!sandboxed) return 78;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 10 * NSEC_PER_SEC), dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{
        if (!atomic_load(&started)) _exit(70);
    });
    xpc_main(accept_peer);
}
