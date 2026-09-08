#pragma once
#include <stddef.h>
#include <stdint.h>

typedef struct {
    uint32_t width, height, bits, orientation, frames, raw;
    char format[32], decoder[128], input_color[256];
} TRImageInfo;
int tr_image_probe(const uint8_t *bytes, size_t length, uint64_t max_pixels,
                    TRImageInfo *info, char *error, size_t error_size);
void *tr_image_open(const uint8_t *bytes, size_t length, uint64_t max_pixels,
                    TRImageInfo *info, char *error, size_t error_size);
int tr_image_render(void *handle, float *pixels, size_t count, char *error, size_t error_size);
int tr_image_render_backend(void *handle, float *pixels, size_t count, uint32_t metal, char *error, size_t error_size);
void tr_image_close(void *handle);
