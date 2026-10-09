#import <UIKit/UIKit.h>
#import <AVFoundation/AVFoundation.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

// All UIKit access belongs on the main thread. Rust calls from blocking tasks;
// handling main-thread callers too avoids dispatch_sync self-deadlocks.
static void on_main(void (^operation)(void)) {
    if ([NSThread isMainThread]) operation();
    else dispatch_sync(dispatch_get_main_queue(), operation);
}

static char *response(NSDictionary *value) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:value options:0 error:nil];
    if (!data) return NULL;
    char *result = malloc(data.length + 1);
    if (!result) return NULL;
    memcpy(result, data.bytes, data.length);
    result[data.length] = 0;
    return result;
}

char *noland_ios_clipboard_read(void) {
    @autoreleasepool {
        __block NSString *text;
        on_main(^{ text = UIPasteboard.generalPasteboard.string; });
        if (!text) return response(@{@"error": @"Clipboard does not contain readable text, or paste access was denied"});
        return response(@{@"text": text});
    }
}

char *noland_ios_clipboard_write(const unsigned char *bytes, size_t length) {
    @autoreleasepool {
        NSString *text = [[NSString alloc] initWithBytes:bytes length:length encoding:NSUTF8StringEncoding];
        if (!text) return response(@{@"error": @"Clipboard content is not valid UTF-8"});
        on_main(^{ UIPasteboard.generalPasteboard.string = text; });
        return response(@{});
    }
}

void noland_ios_keep_awake(bool active) {
    on_main(^{ UIApplication.sharedApplication.idleTimerDisabled = active; });
}

int noland_ios_detect_main_display(uint32_t *width, uint32_t *height, uint32_t *refresh_hz) {
    if (!width || !height || !refresh_hz) return 0;
    __block CGRect nativeBounds = CGRectZero;
    __block NSInteger maximumFramesPerSecond = 0;
    on_main(^{
        UIScreen *screen = UIScreen.mainScreen;
        nativeBounds = screen.nativeBounds;
        maximumFramesPerSecond = screen.maximumFramesPerSecond;
    });
    uint32_t first = (uint32_t)llround(CGRectGetWidth(nativeBounds));
    uint32_t second = (uint32_t)llround(CGRectGetHeight(nativeBounds));
    if (first == 0 || second == 0) return 0;
    // Management may be portrait, but the native stream surface is landscape.
    *width = MAX(first, second);
    *height = MIN(first, second);
    *refresh_hz = (uint32_t)MAX(maximumFramesPerSecond, 60);
    return 1;
}

int noland_ios_packet_tunnel_available(void) {
    __block int available = 0;
    on_main(^{
        NSURL *plugins = NSBundle.mainBundle.builtInPlugInsURL;
        NSURL *extensionURL = [plugins URLByAppendingPathComponent:@"NolandPacketTunnel.appex"];
        NSBundle *extension = extensionURL ? [NSBundle bundleWithURL:extensionURL] : nil;
        available = [extension.bundleIdentifier isEqualToString:@"noland.main.app.PacketTunnel"] ? 1 : 0;
    });
    return available;
}

int noland_ios_microphone_permission_status(void) {
    __block AVAudioSessionRecordPermission permission = AVAudioSessionRecordPermissionUndetermined;
    on_main(^{ permission = AVAudioSession.sharedInstance.recordPermission; });
    switch (permission) {
        case AVAudioSessionRecordPermissionGranted: return 2;
        case AVAudioSessionRecordPermissionDenied: return 1;
        default: return 0;
    }
}

void noland_ios_response_free(char *value) { free(value); }

// Streaming is a full-screen presentation in the application's existing scene.
// Dismissing it exposes the management UI without destroying the stream surface.
@interface NolandStreamController : UIViewController
@property(nonatomic, strong) UIView *surface;
@end

@implementation NolandStreamController
- (void)loadView {
    self.view = [[UIView alloc] initWithFrame:UIScreen.mainScreen.bounds];
    self.view.backgroundColor = UIColor.blackColor;
    self.surface = [[UIView alloc] initWithFrame:self.view.bounds];
    self.surface.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
    [self.view addSubview:self.surface];
}
- (BOOL)prefersStatusBarHidden { return YES; }
- (BOOL)prefersHomeIndicatorAutoHidden { return YES; }
- (UIInterfaceOrientationMask)supportedInterfaceOrientations {
    return UIDevice.currentDevice.userInterfaceIdiom == UIUserInterfaceIdiomPad
        ? UIInterfaceOrientationMaskAll
        : UIInterfaceOrientationMaskLandscape;
}
- (UIInterfaceOrientation)preferredInterfaceOrientationForPresentation {
    return UIInterfaceOrientationLandscapeRight;
}
@end

static NolandStreamController *streamController;
static __weak UIView *streamRoot;

extern void *nl_ios_stream_view_create(void);

void *noland_ios_stream_surface(void *root_pointer) {
    __block void *surface = NULL;
    on_main(^{
        UIView *root = (__bridge UIView *)root_pointer;
        if (!root.window) return;
        if (!streamController) {
            streamController = [NolandStreamController new];
            streamController.modalPresentationStyle = UIModalPresentationFullScreen;
            [streamController loadViewIfNeeded];
            UIView *nativeSurface = CFBridgingRelease(nl_ios_stream_view_create());
            nativeSurface.frame = streamController.view.bounds;
            nativeSurface.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
            [streamController.surface removeFromSuperview];
            streamController.surface = nativeSurface;
            [streamController.view insertSubview:nativeSurface atIndex:0];
        }
        streamRoot = root;
        surface = (__bridge void *)streamController.surface;
    });
    return surface;
}

// Read-only lookup for overlays. Unlike noland_ios_stream_surface(), this never
// creates a controller after stream teardown or redirects updates to a future
// stream surface.
void *noland_ios_active_stream_surface(void) {
    __block void *surface = NULL;
    on_main(^{
        if (streamController && streamController.presentingViewController) {
            surface = (__bridge void *)streamController.surface;
        }
    });
    return surface;
}

int noland_ios_stream_present(void) {
    __block int result = -1;
    on_main(^{
        if (!streamController || !streamRoot.window) return;
        UIApplication.sharedApplication.idleTimerDisabled = YES;
        if (streamController.presentingViewController) { result = 0; return; }
        UIViewController *presenter = streamRoot.window.rootViewController;
        while (presenter.presentedViewController) presenter = presenter.presentedViewController;
        if (!presenter || presenter.isBeingDismissed) return;
        [presenter presentViewController:streamController animated:NO completion:nil];
        result = 0;
    });
    return result;
}

void noland_ios_stream_dismiss(void) {
    on_main(^{ [streamController dismissViewControllerAnimated:NO completion:nil]; });
}

void noland_ios_stream_close(void) {
    on_main(^{
        NolandStreamController *closingController = streamController;
        void (^restoreManagementUI)(void) = ^{
            // Release the native stream hierarchy only after UIKit has completed
            // dismissal. Releasing it early can leave the presenting web view
            // without interaction after a host-initiated stream shutdown.
            if (streamController == closingController) streamController = nil;
            streamRoot = nil;
            UIApplication.sharedApplication.idleTimerDisabled = NO;
        };
        if (closingController.presentingViewController) {
            [closingController dismissViewControllerAnimated:NO completion:restoreManagementUI];
        } else {
            restoreManagementUI();
        }
    });
}
