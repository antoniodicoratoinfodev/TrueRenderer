#import <Foundation/Foundation.h>
#import <dispatch/dispatch.h>
#include <stdatomic.h>

// Process-lifetime notification source; no polling allocation or OS mutation.
static atomic_int tr_pressure = -1;
static dispatch_source_t tr_pressure_source;
int tr_memory_pressure_level(void) {
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        tr_pressure_source = dispatch_source_create(DISPATCH_SOURCE_TYPE_MEMORYPRESSURE, 0,
            DISPATCH_MEMORYPRESSURE_NORMAL | DISPATCH_MEMORYPRESSURE_WARN | DISPATCH_MEMORYPRESSURE_CRITICAL,
            dispatch_get_global_queue(QOS_CLASS_UTILITY, 0));
        if (!tr_pressure_source) return;
        atomic_store_explicit(&tr_pressure, 0, memory_order_release);
        dispatch_source_set_event_handler(tr_pressure_source, ^{
            unsigned long flags = dispatch_source_get_data(tr_pressure_source);
            int level = flags & DISPATCH_MEMORYPRESSURE_CRITICAL ? 2 : flags & DISPATCH_MEMORYPRESSURE_WARN ? 1 : 0;
            atomic_store_explicit(&tr_pressure, level, memory_order_release);
        });
        dispatch_resume(tr_pressure_source);
    });
    return atomic_load_explicit(&tr_pressure, memory_order_acquire);
}
