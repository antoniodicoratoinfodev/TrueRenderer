// Narrow C surface over LibRaw. The Rust side sees these four functions and
// nothing else, so the recipe stays in one readable place and the build needs
// no binding generator. Mirrors the shape of native/macos/image_decoder.h.
#ifndef TR_LIBRAW_H
#define TR_LIBRAW_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uint32_t width;
    uint32_t height;
    /// Bits per sample the recipe asked LibRaw to produce; always 16 today.
    uint32_t bits;
    /// Non-zero when the sensor is not a Bayer CFA, X-Trans above all. Those
    /// need a demosaic this recipe has not qualified, so they are refused.
    uint32_t non_bayer;
    /// LibRaw's own version string, for provenance.
    char version[64];
    /// Camera as identified by LibRaw, for provenance.
    char camera[128];
    /// Named output colour space the recipe selected, for provenance.
    char color[64];
} TRRawInfo;

/// Recipe TR-LIBRAW-LINEAR-V1, fixed and versioned.
///
/// Linear gamma, 16 bits, a named output space, camera white balance, no
/// automatic brightening, no highlight recovery, and the bilinear demosaic,
/// retained as the historical comparison recipe. Every
/// value is set explicitly: a LibRaw default that changed between versions
/// would otherwise change the pixels without changing this file.
#define TR_LIBRAW_RECIPE "TR-libraw-linear-v1"

/// Read metadata only. Never unpacks or develops.
///
/// Returns 0 on success. On failure `error` receives a message and the return
/// value is non-zero.
int tr_libraw_probe(const uint8_t *bytes, size_t length, uint32_t variant, TRRawInfo *info,
                    char *error, size_t error_size);

/// Develop the mosaic under the recipe and write interleaved RGB samples.
///
/// `samples` must hold `width * height * 3` values of 16 bits. The caller sizes
/// it from a previous `tr_libraw_probe` on the same bytes; a mismatch is
/// refused rather than truncated.
int tr_libraw_develop(const uint8_t *bytes, size_t length, uint32_t variant, uint16_t *samples,
                      size_t count, TRRawInfo *info, char *error,
                      size_t error_size);

// Unrotated, unscaled Bayer data. All arrays indexed by 2x2 CFA phase except
// camera_to_srgb (row-major 3x3). Metadata is re-read after unpack/raw2image.
typedef struct {
    uint32_t width, height, flip;
    uint32_t cfa[4];
    float black[4], white, wb[4], camera_to_srgb[9];
    char camera[128];
} TRMosaicInfo;
// XYZ -> reference camera calibration from LibRaw's pinned camera table.
// Separate from the white-normalized matrix used by the rendering recipe.
int tr_mosaic_color_matrix(const uint8_t *, size_t, float *, char *, size_t);
int tr_mosaic_probe(const uint8_t *, size_t, TRMosaicInfo *, char *, size_t);
int tr_mosaic_read(const uint8_t *, size_t, uint16_t *, size_t, TRMosaicInfo *, char *, size_t);

#ifdef __cplusplus
}
#endif

#endif
