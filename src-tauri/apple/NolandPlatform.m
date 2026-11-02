#import <UIKit/UIKit.h>
#import <AVFoundation/AVFoundation.h>
#import <Security/Security.h>
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

// Feedback has a separate, non-synchronizing, device-bound namespace. A reinstall
// never silently enrolls from an orphaned Keychain item; Rust also requires state consent.
static NSMutableDictionary *feedbackKeychainQuery(void) {
    return [@{(__bridge id)kSecClass: (__bridge id)kSecClassGenericPassword,
              (__bridge id)kSecAttrService: @"noland-connect.feedback.mobile-port.v1",
              (__bridge id)kSecAttrAccount: @"installation",
              (__bridge id)kSecAttrSynchronizable: @NO} mutableCopy];
}
char *noland_ios_feedback_keychain_read(void) {
    @autoreleasepool {
        NSMutableDictionary *query = feedbackKeychainQuery();
        query[(__bridge id)kSecReturnData] = @YES;
        query[(__bridge id)kSecMatchLimit] = (__bridge id)kSecMatchLimitOne;
        CFTypeRef result = NULL;
        OSStatus status = SecItemCopyMatching((__bridge CFDictionaryRef)query, &result);
        if (status == errSecItemNotFound) return response(@{});
        if (status != errSecSuccess) return response(@{@"error": @"Protected feedback credentials are unavailable"});
        NSData *data = CFBridgingRelease(result);
        NSString *value = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
        return value ? response(@{@"value": value}) : response(@{@"error": @"Unsupported protected feedback data"});
    }
}
char *noland_ios_feedback_keychain_write(const unsigned char *bytes, size_t length) {
    @autoreleasepool {
        if (!bytes || !length || length > 32768) return response(@{@"error": @"Invalid protected feedback data"});
        NSData *data = [NSData dataWithBytes:bytes length:length];
        NSMutableDictionary *query = feedbackKeychainQuery();
        NSDictionary *attributes = @{(__bridge id)kSecValueData: data,
            (__bridge id)kSecAttrAccessible: (__bridge id)kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly};
        OSStatus status = SecItemUpdate((__bridge CFDictionaryRef)query, (__bridge CFDictionaryRef)attributes);
        if (status == errSecItemNotFound) {
            [query addEntriesFromDictionary:attributes];
            status = SecItemAdd((__bridge CFDictionaryRef)query, NULL);
        }
        if (status != errSecSuccess) return response(@{@"error": @"Could not persist protected feedback data"});
        // Verify persistence before Rust sends the first enrollment/control request.
        NSMutableDictionary *verify = feedbackKeychainQuery();
        verify[(__bridge id)kSecReturnData] = @YES;
        CFTypeRef result = NULL;
        status = SecItemCopyMatching((__bridge CFDictionaryRef)verify, &result);
        NSData *saved = result ? CFBridgingRelease(result) : nil;
        return status == errSecSuccess && [data isEqualToData:saved]
            ? response(@{}) : response(@{@"error": @"Protected feedback read-back failed"});
    }
}
char *noland_ios_feedback_keychain_delete(void) {
    OSStatus status = SecItemDelete((__bridge CFDictionaryRef)feedbackKeychainQuery());
    return status == errSecSuccess || status == errSecItemNotFound
        ? response(@{}) : response(@{@"error": @"Could not remove protected feedback data"});
}

void noland_ios_feedback_lifecycle_install(void (*callback)(bool)) {
    on_main(^{
        NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
        // Will-resign runs before suspension; no background delivery entitlement or
        // asynchronous HTTP completion is assumed. The Rust callback checkpoints synchronously.
        [center addObserverForName:UIApplicationWillResignActiveNotification object:nil queue:NSOperationQueue.mainQueue
            usingBlock:^(NSNotification *note) { callback(false); }];
        [center addObserverForName:UIApplicationDidEnterBackgroundNotification object:nil queue:NSOperationQueue.mainQueue
            usingBlock:^(NSNotification *note) { callback(false); }];
        [center addObserverForName:UIApplicationDidBecomeActiveNotification object:nil queue:NSOperationQueue.mainQueue
            usingBlock:^(NSNotification *note) { callback(true); }];
    });
}

int noland_ios_feedback_protect_spool(const char *path) {
    @autoreleasepool {
        NSString *name = path ? [NSString stringWithUTF8String:path] : nil;
        if (!name) return 0;
        NSURL *url = [NSURL fileURLWithPath:name];
        NSError *error = nil;
        if (![url setResourceValue:@YES forKey:NSURLIsExcludedFromBackupKey error:&error]) return 0;
        if (![[NSFileManager defaultManager] setAttributes:@{NSFileProtectionKey: NSFileProtectionCompleteUntilFirstUserAuthentication}
            ofItemAtPath:name error:&error]) return 0;
        NSDictionary *attributes = [[NSFileManager defaultManager] attributesOfItemAtPath:name error:&error];
        return [attributes[NSFileProtectionKey] isEqual:NSFileProtectionCompleteUntilFirstUserAuthentication] ? 1 : 0;
    }
}

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
