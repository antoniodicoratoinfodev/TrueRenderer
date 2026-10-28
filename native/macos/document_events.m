#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>

// Keep the winit application delegate intact. Register only the document event.
static void (*trDocumentCallback)(const char *) = NULL;
@interface TRDocumentEvents : NSObject
- (void)install;
- (void)open:(NSAppleEventDescriptor *)event reply:(NSAppleEventDescriptor *)reply;
@end
@implementation TRDocumentEvents
- (void)install {
    [[NSAppleEventManager sharedAppleEventManager] setEventHandler:self
        andSelector:@selector(open:reply:) forEventClass:kCoreEventClass andEventID:kAEOpenDocuments];
}
- (void)open:(NSAppleEventDescriptor *)event reply:(NSAppleEventDescriptor *)reply {
    (void)reply;
    NSAppleEventDescriptor *files = [event paramDescriptorForKeyword:keyDirectObject];
    for (NSInteger i = 1; i <= MIN(files.numberOfItems, 32); ++i) {
        NSURL *url = [files descriptorAtIndex:i].fileURLValue;
        if (url.isFileURL && trDocumentCallback) {
            const char *path = url.fileSystemRepresentation;
            if (path) trDocumentCallback(path);
        }
    }
}
@end
void tr_document_events_install(void (*callback)(const char *)) {
    trDocumentCallback = callback;
    static TRDocumentEvents *handler;
    static id observer;
    if (handler) return;
    handler = [TRDocumentEvents new];
    observer = [[NSNotificationCenter defaultCenter]
        addObserverForName:NSApplicationWillFinishLaunchingNotification object:nil queue:nil
        usingBlock:^(NSNotification *notification) { (void)notification; [handler install]; }];
    [handler install];
    (void)observer;
}
