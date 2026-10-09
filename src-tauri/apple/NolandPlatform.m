#import <UIKit/UIKit.h>
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
    UIButton *controls = [UIButton buttonWithType:UIButtonTypeSystem];
    [controls setTitle:NSLocalizedString(@"Controls", nil) forState:UIControlStateNormal];
    controls.backgroundColor = [UIColor.blackColor colorWithAlphaComponent:0.75];
    controls.tintColor = UIColor.whiteColor;
    controls.layer.cornerRadius = 8;
    controls.translatesAutoresizingMaskIntoConstraints = NO;
    [controls addTarget:self action:@selector(showControls) forControlEvents:UIControlEventTouchUpInside];
    [self.view addSubview:controls];
    [NSLayoutConstraint activateConstraints:@[
        [controls.topAnchor constraintEqualToAnchor:self.view.safeAreaLayoutGuide.topAnchor constant:8],
        [controls.trailingAnchor constraintEqualToAnchor:self.view.safeAreaLayoutGuide.trailingAnchor constant:-8],
        [controls.heightAnchor constraintGreaterThanOrEqualToConstant:44],
        [controls.widthAnchor constraintGreaterThanOrEqualToConstant:100]
    ]];
}
- (void)showControls { [self dismissViewControllerAnimated:NO completion:nil]; }
- (BOOL)prefersStatusBarHidden { return YES; }
- (BOOL)prefersHomeIndicatorAutoHidden { return YES; }
@end

static NolandStreamController *streamController;
static __weak UIView *streamRoot;

void *noland_ios_stream_surface(void *root_pointer) {
    __block void *surface = NULL;
    on_main(^{
        UIView *root = (__bridge UIView *)root_pointer;
        if (!root.window) return;
        if (!streamController) {
            streamController = [NolandStreamController new];
            streamController.modalPresentationStyle = UIModalPresentationFullScreen;
            [streamController loadViewIfNeeded];
        }
        streamRoot = root;
        surface = (__bridge void *)streamController.surface;
    });
    return surface;
}

int noland_ios_stream_present(void) {
    __block int result = -1;
    on_main(^{
        if (!streamController || !streamRoot.window) return;
        if (streamController.presentingViewController) { result = 0; return; }
        UIViewController *presenter = streamRoot.window.rootViewController;
        while (presenter.presentedViewController) presenter = presenter.presentedViewController;
        if (!presenter || presenter.isBeingDismissed) return;
        [presenter presentViewController:streamController animated:NO completion:nil];
        result = 0;
    });
    return result;
}

void noland_ios_stream_close(void) {
    on_main(^{
        [streamController dismissViewControllerAnimated:NO completion:nil];
        streamController = nil;
        streamRoot = nil;
        UIApplication.sharedApplication.idleTimerDisabled = NO;
    });
}
