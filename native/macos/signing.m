#import "signing.h"
#import <Security/Security.h>
#include <mach-o/dyld.h>

NSURL *tr_executable_bundle(void) {
    uint32_t size = 0;
    (void)_NSGetExecutablePath(NULL, &size);
    if (size == 0 || size > 32768) return nil;
    char *path = malloc(size);
    if (!path) return nil;
    NSURL *bundle = nil;
    if (_NSGetExecutablePath(path, &size) == 0) {
        NSString *text = [[NSFileManager defaultManager] stringWithFileSystemRepresentation:path length:strlen(path)];
        NSURL *executable = [NSURL fileURLWithPath:text];
        bundle = executable.URLByDeletingLastPathComponent.URLByDeletingLastPathComponent.URLByDeletingLastPathComponent;
    }
    free(path);
    return bundle;
}

NSString *tr_code_requirement(NSURL *bundle, NSString *identifier) {
    SecStaticCodeRef code = NULL;
    SecRequirementRef identity = NULL;
    CFDictionaryRef raw = NULL;
    NSString *identityText = [NSString stringWithFormat:@"identifier \"%@\"", identifier];
    OSStatus status = SecRequirementCreateWithString((__bridge CFStringRef)identityText, kSecCSDefaultFlags, &identity);
    if (status == errSecSuccess)
        status = SecStaticCodeCreateWithPath((__bridge CFURLRef)bundle, kSecCSDefaultFlags, &code);
    if (status == errSecSuccess)
        status = SecStaticCodeCheckValidity(code, kSecCSStrictValidate | kSecCSCheckAllArchitectures, identity);
    if (status == errSecSuccess)
        status = SecCodeCopySigningInformation(code, kSecCSSigningInformation, &raw);
    NSString *requirement = nil;
    if (status == errSecSuccess && raw) {
        NSDictionary *information = (__bridge NSDictionary *)raw;
        NSData *hash = information[(__bridge NSString *)kSecCodeInfoUnique];
        if ([hash isKindOfClass:NSData.class] && (hash.length == 20 || hash.length == 32)) {
            NSMutableString *hex = [NSMutableString string];
            const unsigned char *bytes = hash.bytes;
            for (NSUInteger i = 0; i < hash.length; ++i) [hex appendFormat:@"%02x", bytes[i]];
            requirement = [NSString stringWithFormat:@"%@ and cdhash H\"%@\"", identityText, hex];
        }
    }
    if (raw) CFRelease(raw);
    if (code) CFRelease(code);
    if (identity) CFRelease(identity);
    return requirement;
}
