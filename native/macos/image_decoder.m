#import "image_decoder.h"
#import <Foundation/Foundation.h>
#import <CoreImage/CoreImage.h>
#import <ImageIO/ImageIO.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#include <math.h>

@interface TRImage : NSObject
@property(strong) NSData *data;
@property(strong) CIImage *image;
@property uint32_t width;
@property uint32_t height;
@end
@implementation TRImage
@end
static void message(char *error, size_t size, NSString *text) {
    if (size) snprintf(error,size,"%s",text.UTF8String ?: "Decoder di sistema: errore");
}
static BOOL extent_ok(CGSize size, uint64_t limit) {
    return isfinite(size.width) && isfinite(size.height) && size.width > 0 && size.height > 0
        && size.width <= 32768 && size.height <= 32768 && size.width*size.height <= (double)limit;
}
void *tr_image_open(const uint8_t *bytes, size_t length, uint64_t max_pixels,
                    TRImageInfo *info, char *error, size_t error_size) {
    @autoreleasepool { @try {
        if (!bytes || !length || !info) return NULL;
        memset(info,0,sizeof(*info));
        TRImage *result = [TRImage new];
        // Input bytes are kept alive by the synchronous Rust decode call.
        result.data = [NSData dataWithBytesNoCopy:(void *)bytes length:length freeWhenDone:NO];
        CGImageSourceRef source = CGImageSourceCreateWithData((__bridge CFDataRef)result.data,
            (__bridge CFDictionaryRef)@{(__bridge NSString *)kCGImageSourceShouldCache:@NO});
        if (!source) { message(error,error_size,@"Formato non riconosciuto da ImageIO"); return NULL; }
        NSString *typeName = (__bridge NSString *)CGImageSourceGetType(source);
        UTType *type = typeName ? [UTType typeWithIdentifier:typeName] : nil;
        size_t frames = CGImageSourceGetCount(source);
        NSDictionary *properties = CFBridgingRelease(CGImageSourceCopyPropertiesAtIndex(source,0,NULL));
        NSNumber *width = properties[(__bridge NSString *)kCGImagePropertyPixelWidth];
        NSNumber *height = properties[(__bridge NSString *)kCGImagePropertyPixelHeight];
        NSNumber *bits = properties[(__bridge NSString *)kCGImagePropertyDepth];
        uint32_t orientation = [properties[(__bridge NSString *)kCGImagePropertyOrientation] unsignedIntValue];
        if (orientation < 1 || orientation > 8) orientation = 1;
        BOOL raw = type && [type conformsToType:UTTypeRAWImage];
        BOOL allowed = raw || [type conformsToType:UTTypeJPEG] || [type conformsToType:UTTypePNG]
            || [type conformsToType:UTTypeTIFF] || [type conformsToType:UTTypeGIF]
            || [type conformsToType:UTTypeBMP] || [type conformsToType:UTTypeHEIC]
            || [typeName isEqualToString:@"org.webmproject.webp"] || [typeName isEqualToString:@"public.heif"];
        // RAW files without an embedded preview can omit the ImageIO bitmap dimensions.
        // Their full-resolution dimensions are checked below via CIRAWFilter.nativeSize.
        if (!allowed || !frames || frames > 256 || (!raw && !extent_ok(CGSizeMake(width.doubleValue,height.doubleValue),max_pixels))) {
            CFRelease(source); message(error,error_size,@"Formato, numero di pagine/fotogrammi o dimensioni non supportati (max 64 Mi pixel, 32768 per lato)"); return NULL;
        }
        info->frames=(uint32_t)frames; info->raw=raw; info->orientation=orientation;
        info->bits=bits ? bits.unsignedIntValue : 0;
        NSString *format = raw ? @"RAW" : ([type conformsToType:UTTypeJPEG] ? @"JPEG" :
            [type conformsToType:UTTypePNG] ? @"PNG" : [type conformsToType:UTTypeTIFF] ? @"TIFF" :
            [type conformsToType:UTTypeGIF] ? @"GIF" : [type conformsToType:UTTypeBMP] ? @"BMP" :
            [typeName containsString:@"webp"] ? @"WebP" : @"HEIF/HEIC");
        if (raw) {
            CIRAWFilter *filter = [CIRAWFilter filterWithImageData:result.data identifierHint:typeName];
            if (!filter || !extent_ok(filter.nativeSize,max_pixels)) {
                CFRelease(source); message(error,error_size,@"RAW non supportato dal decoder Apple installato, oppure oltre quota"); return NULL;
            }
            filter.draftModeEnabled=NO; filter.scaleFactor=1.; filter.orientation=(CGImagePropertyOrientation)orientation;
            // Named recipe: native WB/baseline exposure, no optional sharpening/tone boost.
            filter.exposure=0.; filter.boostAmount=0.; filter.gamutMappingEnabled=NO;
            filter.lensCorrectionEnabled=NO;
            if (filter.sharpnessSupported) filter.sharpnessAmount=0.;
            if (filter.contrastSupported) filter.contrastAmount=0.;
            if (filter.detailSupported) filter.detailAmount=0.;
            if (filter.luminanceNoiseReductionSupported) filter.luminanceNoiseReductionAmount=0.;
            if (filter.colorNoiseReductionSupported) filter.colorNoiseReductionAmount=0.;
            if (filter.moireReductionSupported) filter.moireReductionAmount=0.;
            if (filter.localToneMapSupported) filter.localToneMapAmount=0.;
            result.image=filter.outputImage;
            snprintf(info->decoder,sizeof(info->decoder),"Apple RAW %s · full · TR-linear-v1",filter.decoderVersion.UTF8String ?: "OS");
            snprintf(info->input_color,sizeof(info->input_color),"RAW: WB/metadati Apple; baseline %.3f EV; boost/NR/sharpen off",filter.baselineExposure);
            // Sensor depth is not always reported. Zero means unknown, never an invented 16.
        } else {
            CGImageRef bitmap=CGImageSourceCreateImageAtIndex(source,0,(__bridge CFDictionaryRef)@{
                (__bridge NSString *)kCGImageSourceShouldAllowFloat:@YES,
                (__bridge NSString *)kCGImageSourceShouldCacheImmediately:@NO});
            if (!bitmap) { CFRelease(source); message(error,error_size,@"ImageIO non riesce a decodificare il file"); return NULL; }
            info->bits=(uint32_t)CGImageGetBitsPerComponent(bitmap);
            result.image=[[CIImage imageWithCGImage:bitmap] imageByApplyingOrientation:(int)orientation];
            NSString *profile=properties[(__bridge NSString *)kCGImagePropertyProfileName];
            snprintf(info->decoder,sizeof(info->decoder),"Apple ImageIO / ColorSync · %s",NSProcessInfo.processInfo.operatingSystemVersionString.UTF8String);
            snprintf(info->input_color,sizeof(info->input_color),"%s",(profile ? [@"Profilo ImageIO: " stringByAppendingString:profile] : @"Profilo ImageIO implicito/assunto; non assegnato dall'utente").UTF8String);
            CGImageRelease(bitmap);
        }
        CFRelease(source);
        if (!result.image || !extent_ok(result.image.extent.size,max_pixels)) {
            message(error,error_size,@"Il decoder non ha prodotto un'immagine completa valida"); return NULL;
        }
        CGRect extent=result.image.extent;
        if (extent.origin.x != 0 || extent.origin.y != 0)
            result.image=[result.image imageByApplyingTransform:CGAffineTransformMakeTranslation(-extent.origin.x,-extent.origin.y)];
        result.width=info->width=(uint32_t)extent.size.width;
        result.height=info->height=(uint32_t)extent.size.height;
        snprintf(info->format,sizeof(info->format),"%s",format.UTF8String);
        return (__bridge_retained void *)result;
    } @catch (NSException *exception) { message(error,error_size,exception.reason); return NULL; } }
}
int tr_image_render(void *handle, float *pixels, size_t count, char *error, size_t error_size) {
    @autoreleasepool { @try {
        TRImage *image=(__bridge TRImage *)handle;
        if (!image || !pixels || count != (size_t)image.width*image.height) return 1;
        CGColorSpaceRef working=CGColorSpaceCreateWithName(kCGColorSpaceExtendedLinearITUR_2020);
        if (!working) { message(error,error_size,@"Spazio lineare Rec.2020 non disponibile"); return 1; }
        CIContext *context=[CIContext contextWithOptions:@{kCIContextUseSoftwareRenderer:@YES,
            kCIContextCacheIntermediates:@NO, kCIContextWorkingFormat:@(kCIFormatRGBAf),
            kCIContextWorkingColorSpace:(__bridge id)working, kCIContextOutputPremultiplied:@YES}];
        [context render:image.image toBitmap:pixels rowBytes:(size_t)image.width*16
            bounds:CGRectMake(0,0,image.width,image.height) format:kCIFormatRGBAf colorSpace:working];
        [context clearCaches]; CFRelease(working);
        return 0;
    } @catch (NSException *exception) { message(error,error_size,exception.reason); return 1; } }
}
void tr_image_close(void *handle) { if (handle) { id owner=CFBridgingRelease(handle); (void)owner; } }
