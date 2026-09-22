// Recipe TR-libraw-linear-v1 over LibRaw. Nothing else in the project talks to
// LibRaw, so every parameter that shapes the pixels is visible here.
//
// The file is opened from the buffer the broker granted, never by name: §5.5
// requires that the library not reopen a path of its own choosing, and
// open_buffer is what satisfies that.
#include "tr_libraw.h"

#include <cstdio>
#include <cstring>
#include <string>
#include <cmath>
#include <exception>
#include <memory>

#include "libraw/libraw.h"

namespace {

void message(char *error, size_t size, const char *text) {
    if (size) {
        std::snprintf(error, size, "%s", text ? text : "LibRaw: errore");
    }
}

void copy(char *destination, size_t size, const char *text) {
    std::snprintf(destination, size, "%s", text ? text : "");
}

/// Every value the recipe fixes. Called before unpack so that identification
/// and development see the same configuration.
///
/// A LibRaw default is never relied upon: one that changed between releases
/// would change the pixels without changing this file, and the provenance
/// would keep claiming the old recipe.
void apply_recipe(LibRaw &raw, uint32_t variant = 0) {
    libraw_output_params_t &p = raw.imgdata.params;
    // Linear light. The working space is linear Rec.2020 and applies its own
    // transfer curve; a display gamma here would be applied twice.
    p.gamm[0] = 1.0;
    p.gamm[1] = 1.0;
    p.no_auto_bright = 1;
    p.bright = 1.0f;
    // 16 bits: the sensor is 12 to 14, and 8 would throw away what RAW is for.
    p.output_bps = 16;
    // 1 = sRGB primaries, a named space with a stated definition. §7 asks to
    // try the named-space contract first and to fall back to camera space only
    // if a measured requirement fails.
    p.output_color = 1;
    // White balance as the camera recorded it, with the camera's own table as
    // the fallback. Never the automatic grey-world guess.
    p.use_camera_wb = 1;
    p.use_auto_wb = 0;
    p.use_camera_matrix = 1;
    // 0 = clip. Recovery modes reconstruct highlights by interpolation, which
    // is a creative decision this pipeline does not take on the user's behalf.
    p.highlight = 0;
    // Historical bilinear recipe is retained byte-for-byte for comparison.
    p.user_qual = 0;
    // No creative or corrective stages at all.
    p.user_flip = -1;
    p.med_passes = 0;
    p.threshold = 0.0f;
    p.four_color_rgb = 0;
    p.use_fuji_rotate = 0;
    p.exp_correc = 0;
    p.aber[0] = p.aber[1] = p.aber[2] = p.aber[3] = 1.0;
    // No auxiliary profile or file is opened by path.
    p.output_profile = nullptr;
    p.camera_profile = nullptr;
    p.bad_pixels = nullptr;
    p.dark_frame = nullptr;
    if (variant == 1) {
        p.user_qual = 3; // AHD, core LibRaw, no optional demosaic packs.
        p.output_color = 8; // Rec.2020/D65, still a bounded integer output.
        p.highlight = 1; // unclip scale; no reconstruction
        p.adjust_maximum_thr = 0.0f;
        raw.imgdata.rawparams.options |= LIBRAW_RAWOPTIONS_CAMERAWB_FALLBACK_TO_DAYLIGHT;
    }
}

void describe(LibRaw &raw, TRRawInfo *info) {
    std::memset(info, 0, sizeof(*info));
    info->bits = 16;
    copy(info->version, sizeof(info->version), LibRaw::version());
    std::string camera = std::string(raw.imgdata.idata.make) + " " +
                         raw.imgdata.idata.model;
    copy(info->camera, sizeof(info->camera), camera.c_str());
    copy(info->color, sizeof(info->color), raw.imgdata.params.output_color == 8 ? "Rec.2020 primaries, linear, uint16" : "sRGB primaries, linear");
    // Anything other than a three or four colour Bayer mosaic needs a demosaic
    // this recipe has not qualified. X-Trans identifies itself this way.
    const auto &id = raw.imgdata.idata;
    // Linear DNG is already developed RGB: there is no CFA to demosaic.
    // Qualify only this writer's RGB contract, not arbitrary non-Bayer RAW.
    const bool linear_dng = id.dng_version && id.filters == 0 && id.colors == 3 &&
        !std::strcmp(id.make, "TrueRenderer") &&
        !std::strcmp(id.model, "Linear Rec2020 v1");
    info->non_bayer = linear_dng ? 2 : (id.filters == 9 || id.filters == 0);
}

/// Open from the granted bytes and identify. Shared by both entry points so
/// they cannot drift apart.
int open_and_identify(LibRaw &raw, const uint8_t *bytes, size_t length,
                      char *error, size_t error_size, uint32_t variant = 0) {
    apply_recipe(raw, variant);
    int status = raw.open_buffer(const_cast<uint8_t *>(bytes), length);
    if (status != LIBRAW_SUCCESS) {
        message(error, error_size, libraw_strerror(status));
        return 1;
    }
    if (raw.imgdata.idata.raw_count < 1) {
        message(error, error_size, "Nessuna immagine RAW nel file");
        return 1;
    }
    if (variant == 1) {
        for (int c = 0; c < 3; ++c) {
            if (!std::isfinite(raw.imgdata.color.cam_mul[c]) || raw.imgdata.color.cam_mul[c] <= 0) {
                message(error, error_size, "WB as-shot assente: AHD non usa fallback automatici");
                return 1;
            }
        }
    }
    return 0;
}

} // namespace

extern "C" int tr_libraw_probe(const uint8_t *bytes, size_t length, uint32_t variant,
                               TRRawInfo *info, char *error,
                               size_t error_size) try {
    if (!bytes || !length || !info || variant > 1) {
        message(error, error_size, "Argomenti non validi");
        return 1;
    }
    // LibRaw exceeds the stack available on macOS XPC dispatch threads.
    // Keep its lifetime scoped and exception-safe without consuming that stack.
    auto owner = std::make_unique<LibRaw>();
    LibRaw &raw = *owner;
    if (open_and_identify(raw, bytes, length, error, error_size, variant)) {
        return 1;
    }
    describe(raw, info);
    // `iwidth` and `iheight` are the final dimensions after cropping and
    // rotation, and only this call fills them without unpacking the mosaic.
    // Reading them straight after identify would give the pre-rotation values
    // and the caller would size its buffer for a differently shaped image.
    int adjusted = raw.adjust_sizes_info_only();
    if (adjusted != LIBRAW_SUCCESS) {
        message(error, error_size, libraw_strerror(adjusted));
        return 1;
    }
    info->width = raw.imgdata.sizes.iwidth;
    info->height = raw.imgdata.sizes.iheight;
    if (info->width == 0 || info->height == 0) {
        message(error, error_size, "Dimensioni sviluppate non dichiarate");
        return 1;
    }
    return 0;
} catch (const std::exception &e) { message(error, error_size, e.what()); return 1; }
  catch (...) { message(error, error_size, "Eccezione nel probe LibRaw"); return 1; }

extern "C" int tr_libraw_develop(const uint8_t *bytes, size_t length, uint32_t variant,
                                 uint16_t *samples, size_t count,
                                 TRRawInfo *info, char *error,
                                 size_t error_size) try {
    if (!bytes || !length || !samples || !info || variant > 1) {
        message(error, error_size, "Argomenti non validi");
        return 1;
    }
    // LibRaw exceeds the stack available on macOS XPC dispatch threads.
    // Keep its lifetime scoped and exception-safe without consuming that stack.
    auto owner = std::make_unique<LibRaw>();
    LibRaw &raw = *owner;
    if (open_and_identify(raw, bytes, length, error, error_size, variant)) {
        return 1;
    }
    describe(raw, info);
    if (info->non_bayer == 1) {
        message(error, error_size,
                "CFA non Bayer: il demosaicing di questa ricetta non e' qualificato");
        return 1;
    }
    int status = raw.unpack();
    if (status != LIBRAW_SUCCESS) {
        message(error, error_size, libraw_strerror(status));
        return 1;
    }
    if (variant == 1) {
        for (int c = 0; c < 3; ++c) {
            if (!std::isfinite(raw.imgdata.color.cam_mul[c]) || raw.imgdata.color.cam_mul[c] <= 0) {
                message(error, error_size, "WB as-shot assente dopo unpack: nessun fallback");
                return 1;
            }
        }
    }
    status = raw.dcraw_process();
    if (status != LIBRAW_SUCCESS) {
        message(error, error_size, libraw_strerror(status));
        return 1;
    }
    libraw_processed_image_t *image = raw.dcraw_make_mem_image(&status);
    if (!image) {
        message(error, error_size, libraw_strerror(status));
        return 1;
    }
    // Everything below must release the image before returning.
    int result = 0;
    const size_t produced =
        static_cast<size_t>(image->width) * image->height * image->colors;
    if (image->type != LIBRAW_IMAGE_BITMAP || image->colors != 3 ||
        image->bits != 16) {
        message(error, error_size,
                "LibRaw ha prodotto un formato diverso da quello richiesto");
        result = 1;
    } else if (produced != count) {
        char detail[256];
        std::snprintf(detail, sizeof(detail),
                      "Raster %ux%u da %zu campioni, il probe ne aveva previsti %zu",
                      image->width, image->height, produced, count);
        message(error, error_size, detail);
        result = 1;
    } else {
        std::memcpy(samples, image->data, produced * sizeof(uint16_t));
        info->width = image->width;
        info->height = image->height;
    }
    LibRaw::dcraw_clear_mem(image);
    return result;
} catch (const std::exception &e) { message(error, error_size, e.what()); return 1; }
  catch (...) { message(error, error_size, "Eccezione nello sviluppo LibRaw"); return 1; }

namespace {
int mosaic_info(LibRaw &raw, TRMosaicInfo *info, char *error, size_t size) {
    const auto &id = raw.imgdata.idata;
    const auto &s = raw.imgdata.sizes;
    const auto &c = raw.imgdata.color;
    // Model allowlist is a scientific scope boundary, not a security boundary.
    const bool nikon = !std::strcmp(id.make, "Nikon") && !id.dng_version &&
        (!std::strcmp(id.model, "D750") || !std::strcmp(id.model, "D40"));
    const bool fixture = !std::strcmp(id.make, "TrueRenderer") && !std::strcmp(id.model, "Synthetic DNG") && id.dng_version;
    if ((!nikon && !fixture) || id.colors != 3 || id.filters <= 1000 ||
        s.width < 8 || s.height < 8 || size_t(s.width) * s.height > 67108864 ||
        (s.flip != 0 && s.flip != 3 && s.flip != 5 && s.flip != 6)) {
        message(error, size, "TrueRenderer sperimentale: solo Nikon D750/D40 Bayer e DNG sintetico TrueRenderer; formato/orientamento non supportato");
        return 1;
    }
    if ((c.cblack[4] || c.cblack[5]) &&
        (c.cblack[4] < 1 || c.cblack[4] > 2 || c.cblack[5] < 1 || c.cblack[5] > 2)) {
        message(error, size, "Mappa nero non supportata dalla ricetta fp32"); return 1;
    }
    std::memset(info, 0, sizeof(*info));
    info->width = s.width; info->height = s.height; info->flip = s.flip;
    info->white = float(c.maximum);
    copy(info->camera, sizeof(info->camera), (std::string(id.make) + " " + id.model).c_str());
    int histogram[3] = {};
    for (int phase = 0; phase < 4; ++phase) {
        const int y = phase / 2, x = phase % 2;
        const int color = raw.COLOR(y, x);
        if (color < 0 || color > 3) { message(error, size, "CFA non RGB"); return 1; }
        const int rgb = color == 3 ? 1 : color;
        info->cfa[phase] = rgb; histogram[rgb]++;
        info->black[phase] = float(c.black) + float(c.cblack[color]);
        if (c.cblack[4] && c.cblack[5])
            info->black[phase] += float(c.cblack[6 + y % c.cblack[4] * c.cblack[5] + x % c.cblack[5]]);
        float wb = c.cam_mul[color];
        if (color == 3 && wb == 0) wb = c.cam_mul[1];
        if (!std::isfinite(wb) || wb <= 0 || !std::isfinite(c.cam_mul[1]) || c.cam_mul[1] <= 0 ||
            info->white <= info->black[phase]) {
            message(error, size, "WB as-shot o livelli nero/bianco non validi: nessun fallback"); return 1;
        }
        info->wb[phase] = wb / c.cam_mul[1];
    }
    if (histogram[0] != 1 || histogram[1] != 2 || histogram[2] != 1 ||
        info->cfa[0] == info->cfa[1] || info->cfa[0] == info->cfa[2]) {
        message(error, size, "Pattern diverso da Bayer RGB 2x2"); return 1;
    }
    for (int row = 0; row < 3; ++row) {
        if (std::fabs(c.rgb_cam[row][3]) > 1e-6f) {
            message(error, size, "Matrice a quattro colori non supportata"); return 1;
        }
        float sum = 0;
        for (int col = 0; col < 3; ++col) {
            const float value = c.rgb_cam[row][col];
            if (!std::isfinite(value) || std::fabs(value) > 16.f) {
                message(error, size, "Matrice camera non valida"); return 1;
            }
            info->camera_to_srgb[row * 3 + col] = value; sum += value;
        }
        if (std::fabs(sum - 1.f) > .05f) {
            message(error, size, "Matrice camera non normalizzata"); return 1;
        }
    }
    return 0;
}
}

extern "C" int tr_mosaic_probe(const uint8_t *bytes, size_t length,
    TRMosaicInfo *info, char *error, size_t size) try {
    if (!bytes || !length || !info) return 1;
    // LibRaw exceeds the stack available on macOS XPC dispatch threads.
    // Keep its lifetime scoped and exception-safe without consuming that stack.
    auto owner = std::make_unique<LibRaw>();
    LibRaw &raw = *owner;
    if (open_and_identify(raw, bytes, length, error, size)) return 1;
    return mosaic_info(raw, info, error, size);
} catch (const std::exception &e) { message(error, size, e.what()); return 1; }
  catch (...) { message(error, size, "Eccezione nel probe mosaico"); return 1; }

extern "C" int tr_mosaic_color_matrix(const uint8_t *bytes, size_t length,
    float *matrix, char *error, size_t size) try {
    if (!bytes || !length || !matrix) return 1;
    auto raw = std::make_unique<LibRaw>();
    TRMosaicInfo info;
    if (open_and_identify(*raw, bytes, length, error, size) ||
        mosaic_info(*raw, &info, error, size)) return 1;
    for (int y = 0; y < 3; ++y)
        for (int x = 0; x < 3; ++x) {
            float value;
            if (raw->imgdata.idata.dng_version) {
                const auto &calibration = raw->imgdata.color.dng_color[0];
                if (calibration.illuminant != 21) {
                    message(error, size, "DNG: calibrazione D65 originale assente"); return 1;
                }
                value = calibration.colormatrix[y][x];
            } else {
                value = raw->imgdata.color.cam_xyz[y][x];
            }
            if (!std::isfinite(value) || std::fabs(value) > 16.f) return 1;
            matrix[y * 3 + x] = value;
        }
    return 0;
} catch (...) { message(error, size, "Matrice DNG non disponibile"); return 1; }

extern "C" int tr_mosaic_read(const uint8_t *bytes, size_t length,
    uint16_t *samples, size_t count, TRMosaicInfo *info, char *error, size_t size) try {
    if (!bytes || !length || !samples || !info) return 1;
    // LibRaw exceeds the stack available on macOS XPC dispatch threads.
    // Keep its lifetime scoped and exception-safe without consuming that stack.
    auto owner = std::make_unique<LibRaw>();
    LibRaw &raw = *owner;
    if (open_and_identify(raw, bytes, length, error, size) || mosaic_info(raw, info, error, size)) return 1;
    int status = raw.unpack();
    if (status == LIBRAW_SUCCESS) status = raw.raw2image();
    if (status != LIBRAW_SUCCESS) { message(error, size, libraw_strerror(status)); return 1; }
    if (mosaic_info(raw, info, error, size)) return 1;
    if (!raw.imgdata.rawdata.raw_image || !raw.imgdata.image ||
        raw.imgdata.sizes.iwidth != info->width || raw.imgdata.sizes.iheight != info->height ||
        size_t(info->width) * info->height != count) {
        message(error, size, "Raster mosaico diverso dal probe"); return 1;
    }
    // raw2image copies/crops the unpacked sensor plane. No subtract_black,
    // scale_colors, dcraw_process, gamma, colour conversion or orientation.
    for (uint32_t y = 0; y < info->height; ++y)
        for (uint32_t x = 0; x < info->width; ++x)
            samples[size_t(y) * info->width + x] = raw.imgdata.image[size_t(y) * info->width + x][raw.COLOR(y, x)];
    return 0;
} catch (const std::exception &e) { message(error, size, e.what()); return 1; }
  catch (...) { message(error, size, "Eccezione nella lettura mosaico"); return 1; }
