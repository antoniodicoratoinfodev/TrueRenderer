#import <Foundation/Foundation.h>
#include <xpc/xpc.h>
#include <mach/mach.h>
#include <libproc.h>
#include <signal.h>
#include <unistd.h>
#import "../../native/macos/signing.h"

int main(void) {
    @autoreleasepool {
        NSURL *service = [tr_executable_bundle() URLByAppendingPathComponent:@"Contents/XPCServices/ControlWorker.xpc"];
        NSString *requirement = tr_code_requirement(service, @"it.truerenderer.controlprobe.worker");
        if (!requirement) return 2;
        xpc_connection_t peer = xpc_connection_create("it.truerenderer.controlprobe.worker", NULL);
        if (xpc_connection_set_peer_code_signing_requirement(peer, requirement.UTF8String) != 0) return 2;
        xpc_connection_set_event_handler(peer, ^(xpc_object_t event) { (void)event; });
        xpc_connection_resume(peer);
        dispatch_semaphore_t ready = dispatch_semaphore_create(0);
        __block xpc_object_t reply = NULL;
        xpc_object_t message = xpc_dictionary_create(NULL, NULL, 0);
        xpc_connection_send_message_with_reply(peer, message, dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^(xpc_object_t value) {
            reply = value;
            dispatch_semaphore_signal(ready);
        });
        if (dispatch_semaphore_wait(ready, dispatch_time(DISPATCH_TIME_NOW, 10 * NSEC_PER_SEC)) != 0
            || !reply || xpc_get_type(reply) != XPC_TYPE_DICTIONARY) return 3;
        mach_port_t control = xpc_dictionary_copy_mach_send(reply, "task");
        pid_t pid = -1;
        kern_return_t pidResult = pid_for_task(control, &pid);
        BOOL identity = control != MACH_PORT_NULL && pidResult == KERN_SUCCESS && pid > 0
            && pid != getpid() && pid == xpc_connection_get_pid(peer);
        mach_task_basic_info_data_t usage = {0};
        mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
        kern_return_t measured = identity ? task_info(control, MACH_TASK_BASIC_INFO, (task_info_t)&usage, &count) : KERN_FAILURE;
        kern_return_t suspended = identity ? task_suspend(control) : KERN_FAILURE;
        kern_return_t terminated = identity ? task_terminate(control) : KERN_FAILURE;
        audit_token_t token = {{0}};
        mach_msg_type_number_t tokenCount = TASK_AUDIT_TOKEN_COUNT;
        kern_return_t tokenResult = identity ? task_info(control, TASK_AUDIT_TOKEN, (task_info_t)&token, &tokenCount) : KERN_FAILURE;
        // SDK libproc SPI: qualified for this internal R0 OS only, never a PID-only fallback.
        int signalled = tokenResult == KERN_SUCCESS ? proc_signal_with_audittoken(&token, SIGKILL) : -1;
        if (signalled != 0 && suspended == KERN_SUCCESS) task_resume(control);
        if (signalled == 0) usleep(20000);
        count = MACH_TASK_BASIC_INFO_COUNT;
        kern_return_t after = identity ? task_info(control, MACH_TASK_BASIC_INFO, (task_info_t)&usage, &count) : KERN_FAILURE;
        NSDictionary *report = @{@"application": @"TrueRenderer", @"peer_cdhash_checked": @YES,
            @"control_matches_peer": @(identity), @"pid": @(pid),
            @"port_export_result": @(xpc_dictionary_get_int64(reply, "task_port_result")),
            @"measurement_result": @(measured), @"suspend_result": @(suspended),
            @"terminate_result": @(terminated), @"measurement_after_termination": @(after),
            @"audit_token_result": @(tokenResult), @"audit_signal_result": @(signalled),
            @"passed": @((BOOL)(identity && measured == KERN_SUCCESS && suspended == KERN_SUCCESS
                && signalled == 0 && after != KERN_SUCCESS))};
        NSData *json = [NSJSONSerialization dataWithJSONObject:report options:NSJSONWritingPrettyPrinted | NSJSONWritingSortedKeys error:nil];
        fwrite(json.bytes, 1, json.length, stdout); fputc('\n', stdout);
        if (control) mach_port_deallocate(mach_task_self(), control);
        xpc_connection_cancel(peer);
        return [report[@"passed"] boolValue] ? 0 : 4;
    }
}
