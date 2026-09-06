#import <Foundation/Foundation.h>
#include <xpc/xpc.h>
#include <mach/mach.h>
#include <sys/resource.h>
#include <unistd.h>
#include <os/log.h>
#include <fcntl.h>
#import "../../native/macos/signing.h"

static void accept_peer(xpc_connection_t peer) {
    if (xpc_connection_set_peer_code_signing_requirement(peer, "identifier \"it.truerenderer.controlprobe\"") != 0) {
        xpc_connection_cancel(peer);
        return;
    }
    xpc_connection_set_event_handler(peer, ^(xpc_object_t event) {
        if (xpc_get_type(event) == XPC_TYPE_ERROR) _exit(0);
        if (xpc_get_type(event) != XPC_TYPE_DICTIONARY) return;
        xpc_object_t reply = xpc_dictionary_create_reply(event);
        if (!reply) return;
        mach_port_t control = MACH_PORT_NULL;
        kern_return_t result = mach_port_mod_refs(mach_task_self(), mach_task_self(), MACH_PORT_RIGHT_SEND, 1);
        if (result == KERN_SUCCESS) control = mach_task_self();
        xpc_dictionary_set_int64(reply, "task_port_result", result);
        if (result == KERN_SUCCESS) {
            xpc_dictionary_set_mach_send(reply, "task", control);
            mach_port_deallocate(mach_task_self(), control);
        }
        xpc_connection_send_message(xpc_dictionary_get_remote_connection(event), reply);
    });
    xpc_connection_resume(peer);
}

int main(void) {
    struct rlimit children = {0, 0};
    if (setrlimit(RLIMIT_NPROC, &children) != 0) return 78;
    xpc_main(accept_peer);
}
