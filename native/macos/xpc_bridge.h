#pragma once
#include <stdint.h>
#include <stddef.h>

void *tr_xpc_open(uint32_t slot, uint32_t timeout_ms, int *input, int *output, char *error, size_t error_size);
int tr_xpc_usage(void *lease, uint64_t *rss, uint64_t *footprint, int *pid);
int tr_xpc_suspend_for_test(void *lease);
int tr_xpc_inject_growth_for_test(void *lease);
int tr_xpc_stop(void *lease);
void tr_xpc_release(void *lease);
