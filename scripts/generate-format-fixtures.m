#import <Foundation/Foundation.h>
#import <CoreGraphics/CoreGraphics.h>
#import <ImageIO/ImageIO.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>

static NSDictionary *save(NSString *folder, NSString *name, CFStringRef type, uint32_t width,uint32_t height,uint32_t bits,uint32_t orientation) {
    size_t component=bits/8, count=(size_t)width*height*4;
    void *bytes=calloc(count,component);
    for(uint32_t y=0;y<height;y++) for(uint32_t x=0;x<width;x++) {
        uint16_t rgba[4]={0,0,0,65535};
        if(y<height/2) rgba[x<width/2 ? 0 : 1]=65535;
        else if(x<width/2) rgba[2]=65535;
        else rgba[0]=rgba[1]=rgba[2]=(uint16_t)(20000+(x+y)%1000);
        for(size_t c=0;c<4;c++) {
            if(bits==16) ((uint16_t*)bytes)[((size_t)y*width+x)*4+c]=rgba[c];
            else ((uint8_t*)bytes)[((size_t)y*width+x)*4+c]=(uint8_t)(rgba[c]>>8);
        }
    }
    CGColorSpaceRef color=CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGDataProviderRef provider=CGDataProviderCreateWithData(NULL,bytes,count*component,NULL);
    CGImageRef image=CGImageCreate(width,height,bits,bits*4,(size_t)width*4*component,color,
        kCGImageAlphaLast|(bits==16?kCGBitmapByteOrder16Little:kCGBitmapByteOrderDefault),provider,NULL,NO,kCGRenderingIntentDefault);
    NSURL *url=[NSURL fileURLWithPath:[folder stringByAppendingPathComponent:name]];
    CGImageDestinationRef destination=CGImageDestinationCreateWithURL((__bridge CFURLRef)url,type,1,NULL);
    BOOL success=NO;
    if(destination&&image) {
        CGImageDestinationAddImage(destination,image,(__bridge CFDictionaryRef)@{
            (__bridge NSString*)kCGImagePropertyOrientation:@(orientation),
            (__bridge NSString*)kCGImageDestinationLossyCompressionQuality:@1.0});
        success=CGImageDestinationFinalize(destination);
    }
    if(destination) CFRelease(destination);if(image) CGImageRelease(image);
    CGDataProviderRelease(provider); CGColorSpaceRelease(color); free(bytes);
    if(!success) {fprintf(stderr,"Cannot generate %s\n",name.UTF8String);return nil;}
    return @{ @"file":name,@"width":@(orientation>=5?height:width),@"height":@(orientation>=5?width:height),@"source_bits":@(bits),@"orientation":@(orientation),@"generator":@"own analytic quadrants via ImageIO destination"};
}
int main(int argc,char **argv) {
    @autoreleasepool {
        if(argc!=2)return 2;
        NSString *folder=@(argv[1]);
        [[NSFileManager defaultManager] createDirectoryAtPath:folder withIntermediateDirectories:YES attributes:nil error:NULL];
        NSMutableArray *cases=[NSMutableArray array];
        NSArray *formats=@[@[@"01-jpeg.jpg",@"public.jpeg"],@[@"02-png.png",@"public.png"],@[@"03-tiff.tiff",@"public.tiff"],@[@"04-gif.gif",@"com.compuserve.gif"],@[@"05-bmp.bmp",@"com.microsoft.bmp"],@[@"06-heic.heic",@"public.heic"]];
        for(NSArray *format in formats) {NSDictionary *item=save(folder,format[0],(__bridge CFStringRef)format[1],48,32,8,1);if(!item)return 1;[cases addObject:item];}
        for(NSString *extension in @[@"png",@"tiff"]) {NSDictionary *item=save(folder,[@"16bit." stringByAppendingString:extension],(__bridge CFStringRef)[@"public." stringByAppendingString:extension],48,32,16,1);if(!item)return 1;[cases addObject:item];}
        for(uint32_t orientation=1;orientation<=8;orientation++) {NSDictionary *item=save(folder,[NSString stringWithFormat:@"orientation-%u.tiff",orientation],CFSTR("public.tiff"),48,32,8,orientation);if(!item)return 1;[cases addObject:item];}
        NSDictionary *large=save(folder,@"12mp-jpeg.jpg",CFSTR("public.jpeg"),4000,3000,8,1);if(!large)return 1;[cases addObject:large];
        NSData *json=[NSJSONSerialization dataWithJSONObject:cases options:NSJSONWritingPrettyPrinted error:NULL];
        return [json writeToFile:[folder stringByAppendingPathComponent:@"manifest.json"] atomically:YES]?0:1;
    }
}
