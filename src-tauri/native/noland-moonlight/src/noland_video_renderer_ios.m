// iOS media/input backend for Noland Connect.
// Architecture and platform behavior reconciled against Moonlight iOS
// 02dc9780496eeeac6d01c8bbdccb8b6fe71ef28a (GPL-3.0).
#import <UIKit/UIKit.h>
#import <AVFoundation/AVFoundation.h>
#import <CoreMedia/CoreMedia.h>
#import <QuartzCore/QuartzCore.h>
#import <GameController/GameController.h>
#import <CoreHaptics/CoreHaptics.h>
#import <dispatch/dispatch.h>

#include "noland_video_renderer.h"
#import "noland_stream_controls_ios.h"
#include "noland_keyboard_ios.h"
#include "Limelight.h"

#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <math.h>

@interface NolandSampleDisplayLayer : AVSampleBufferDisplayLayer
@end
@implementation NolandSampleDisplayLayer
- (void)layoutSublayers { [super layoutSublayers]; }
@end

extern bool nl_ios_input_capture_active(void);
extern void noland_ios_stream_dismiss(void);

@class NolandKeyboardResponder;

@interface NolandStreamView : UIView {
  CGPoint _touchLocation;
  CGPoint _originalLocation;
  BOOL _touchMoved;
  BOOL _dragging;
  BOOL _absoluteRightClick;
  NSUInteger _peakTouchCount;
  NSTimer* _dragTimer;
  NSTimer* _clickTimer;
  uint8_t _pendingClickButton;
  BOOL _suppressTouchSequence;
  BOOL _keyboardVisible;
  BOOL _gamepadOwnsTouches;
  NSMutableSet<NSNumber*>* _hardwareKeys;
  NSMutableSet<UITouch*>* _surfaceTouches;
}
@property (nonatomic, assign) nl_video_renderer_t* renderer;
@property (nonatomic, strong) NolandStreamControls* controls;
@property (nonatomic, strong) NSMutableArray<id>* inputObservers;
@property (nonatomic, strong) NolandKeyboardResponder* keyboardResponder;
- (void)prepareForRemoval;
- (void)cancelPointerInput;
- (void)insertText:(NSString*)text;
- (void)deleteBackward;
- (void)hideKeyboard;
@end

// The video view must not itself conform to UIKeyInput: UIKit may activate a
// text responder on a touch and summon the keyboard over gamepad controls.
// Only the explicit Keyboard action can authorize this separate responder.
@interface NolandKeyboardResponder : UIView <UIKeyInput>
@property(nonatomic, weak) NolandStreamView* owner;
@property(nonatomic, assign) BOOL requested;
@end
@implementation NolandKeyboardResponder
- (BOOL)canBecomeFirstResponder { return self.requested; }
- (BOOL)hasText { return YES; }
- (void)insertText:(NSString*)text { [self.owner insertText:text]; }
- (void)deleteBackward { [self.owner deleteBackward]; }
- (UITextAutocorrectionType)autocorrectionType { return UITextAutocorrectionTypeNo; }
- (UITextAutocapitalizationType)autocapitalizationType { return UITextAutocapitalizationTypeNone; }
- (UITextSpellCheckingType)spellCheckingType { return UITextSpellCheckingTypeNo; }
- (UIEditingInteractionConfiguration)editingInteractionConfiguration { return UIEditingInteractionConfigurationNone; }
- (UIView*)inputAccessoryView {
  UIToolbar* toolbar = [[UIToolbar alloc] initWithFrame:CGRectMake(0, 0, self.owner.bounds.size.width, 44)];
  toolbar.items = @[[[UIBarButtonItem alloc] initWithBarButtonSystemItem:UIBarButtonSystemItemFlexibleSpace target:nil action:nil],
    [[UIBarButtonItem alloc] initWithBarButtonSystemItem:UIBarButtonSystemItemDone target:self.owner action:@selector(hideKeyboard)]];
  return toolbar;
}
- (void)pressesBegan:(NSSet<UIPress*>*)presses withEvent:(UIPressesEvent*)event { [self.owner pressesBegan:presses withEvent:event]; }
- (void)pressesEnded:(NSSet<UIPress*>*)presses withEvent:(UIPressesEvent*)event { [self.owner pressesEnded:presses withEvent:event]; }
- (void)pressesCancelled:(NSSet<UIPress*>*)presses withEvent:(UIPressesEvent*)event { [self.owner pressesCancelled:presses withEvent:event]; }
@end

@interface NolandControllerInput : NSObject
@property (nonatomic, assign) nl_video_renderer_t* renderer;
@property (nonatomic, strong) NSMutableArray<id>* observers;
@property (nonatomic, strong) NSMapTable<GCController*, NSNumber*>* controllerNumbers;
@property (nonatomic, strong) NSMutableSet<NSNumber*>* announcedControllers;
@property (nonatomic, assign) float accumulatedMouseX;
@property (nonatomic, assign) float accumulatedMouseY;
@property (nonatomic, strong) NSMutableDictionary<NSNumber*, NSArray*>* hapticMotors;
@property (nonatomic, strong) NSMutableDictionary<NSString*, NSTimer*>* motionTimers;
@property (atomic, assign) BOOL inputSuspended;
@property (nonatomic, assign) NSUInteger virtualNumber;
@property (nonatomic, assign) BOOL virtualAnnounced;
- (void)start;
- (void)stop;
- (void)announceControllers;
- (void)sendVirtualGamepad:(NolandVirtualGamepadState)state enabled:(BOOL)enabled;
@end

@interface NolandHapticMotor : NSObject
@property (nonatomic, strong) CHHapticEngine* engine;
@property (nonatomic, strong) id<CHHapticPatternPlayer> player;
@property (nonatomic, assign) BOOL playing;
- (instancetype)initWithController:(GCController*)controller locality:(GCHapticsLocality)locality;
- (void)setAmplitude:(uint16_t)amplitude;
- (void)stop;
@end

@implementation NolandHapticMotor
- (instancetype)initWithController:(GCController*)controller locality:(GCHapticsLocality)locality {
  self = [super init];
  if (self == nil || controller.haptics == nil || ![controller.haptics.supportedLocalities containsObject:locality]) return nil;
  self.engine = [controller.haptics createEngineWithLocality:locality];
  if (![self.engine startAndReturnError:nil]) return nil;
  __weak NolandHapticMotor* weakSelf = self;
  self.engine.stoppedHandler = ^(CHHapticEngineStoppedReason reason) {
    (void)reason;
    NolandHapticMotor* motor = weakSelf;
    motor.player = nil;
    motor.playing = NO;
  };
  self.engine.resetHandler = ^{
    NolandHapticMotor* motor = weakSelf;
    motor.player = nil;
    motor.playing = NO;
    [motor.engine startAndReturnError:nil];
  };
  return self;
}
- (void)setAmplitude:(uint16_t)amplitude {
  if (self.engine == nil) return;
  if (amplitude == 0) {
    if (self.playing) [self.player stopAtTime:CHHapticTimeImmediate error:nil];
    self.playing = NO;
    return;
  }
  if (self.player == nil) {
    CHHapticEventParameter* intensity = [[CHHapticEventParameter alloc]
        initWithParameterID:CHHapticEventParameterIDHapticIntensity value:1.0f];
    CHHapticEvent* event = [[CHHapticEvent alloc]
        initWithEventType:CHHapticEventTypeHapticContinuous
        parameters:@[intensity] relativeTime:0 duration:GCHapticDurationInfinite];
    CHHapticPattern* pattern = [[CHHapticPattern alloc] initWithEvents:@[event] parameters:@[] error:nil];
    self.player = [self.engine createPlayerWithPattern:pattern error:nil];
  }
  CHHapticDynamicParameter* value = [[CHHapticDynamicParameter alloc]
      initWithParameterID:CHHapticDynamicParameterIDHapticIntensityControl
      value:amplitude / 65535.0f relativeTime:0];
  [self.player sendParameters:@[value] atTime:CHHapticTimeImmediate error:nil];
  if (!self.playing) self.playing = [self.player startAtTime:CHHapticTimeImmediate error:nil];
}
- (void)stop {
  [self.player cancelAndReturnError:nil];
  [self.engine stopWithCompletionHandler:nil];
  self.player = nil; self.engine = nil; self.playing = NO;
}
@end

static __weak NolandControllerInput* g_controller_input;

typedef struct nl_ios_video_context nl_ios_video_context_t;

@interface NolandDisplayLinkProxy : NSObject
@property (nonatomic, assign) nl_video_renderer_t* renderer;
- (void)onDisplayLink:(CADisplayLink*)link;
@end

typedef struct nl_ios_video_context {
  __unsafe_unretained UIView* view;
  __strong UIView* owned_view;
  __strong AVSampleBufferDisplayLayer* layer;
  CMVideoFormatDescriptionRef format_description;
  uint8_t* sps;
  size_t sps_len;
  uint8_t* pps;
  size_t pps_len;
  uint8_t* vps;
  size_t vps_len;
  int video_format;
  int width;
  int height;
  int redraw_rate;
  __strong CADisplayLink* display_link;
  __strong NolandDisplayLinkProxy* display_link_proxy;
  __strong NolandControllerInput* controller_input;
  __strong NSMutableArray<id>* lifecycle_observers;
} nl_ios_video_context_t;

static nl_runtime_t* nl_ios_runtime(nl_video_renderer_t* renderer) {
  return renderer ? (nl_runtime_t*)renderer->frame_processor_user_data : NULL;
}

static uint8_t nl_ios_modifiers(UIKeyModifierFlags flags) {
  uint8_t value = 0;
  if ((flags & UIKeyModifierShift) != 0) value |= MODIFIER_SHIFT;
  if ((flags & UIKeyModifierControl) != 0) value |= MODIFIER_CTRL;
  if ((flags & UIKeyModifierAlternate) != 0) value |= MODIFIER_ALT;
  if ((flags & UIKeyModifierCommand) != 0) value |= MODIFIER_META;
  return value;
}

static uint16_t nl_ios_virtual_key(UIKeyboardHIDUsage usage) {
  if (usage >= UIKeyboardHIDUsageKeyboardA && usage <= UIKeyboardHIDUsageKeyboardZ)
    return 0x41 + (uint16_t)(usage - UIKeyboardHIDUsageKeyboardA);
  if (usage >= UIKeyboardHIDUsageKeyboard1 && usage <= UIKeyboardHIDUsageKeyboard9)
    return 0x31 + (uint16_t)(usage - UIKeyboardHIDUsageKeyboard1);
  if (usage >= UIKeyboardHIDUsageKeypad1 && usage <= UIKeyboardHIDUsageKeypad9)
    return 0x61 + (uint16_t)(usage - UIKeyboardHIDUsageKeypad1);
  if (usage >= UIKeyboardHIDUsageKeyboardF1 && usage <= UIKeyboardHIDUsageKeyboardF12)
    return 0x70 + (uint16_t)(usage - UIKeyboardHIDUsageKeyboardF1);
  if (usage >= UIKeyboardHIDUsageKeyboardF13 && usage <= UIKeyboardHIDUsageKeyboardF24)
    return 0x7C + (uint16_t)(usage - UIKeyboardHIDUsageKeyboardF13);
  switch (usage) {
    case UIKeyboardHIDUsageKeyboard0: return 0x30;
    case UIKeyboardHIDUsageKeypad0: return 0x60;
    case UIKeyboardHIDUsageKeyboardReturnOrEnter: return 0x0D;
    case UIKeyboardHIDUsageKeyboardEscape: return 0x1B;
    case UIKeyboardHIDUsageKeyboardDeleteOrBackspace: return 0x08;
    case UIKeyboardHIDUsageKeyboardTab: return 0x09;
    case UIKeyboardHIDUsageKeyboardSpacebar: return 0x20;
    case UIKeyboardHIDUsageKeyboardHyphen: return 0xBD;
    case UIKeyboardHIDUsageKeyboardEqualSign: return 0xBB;
    case UIKeyboardHIDUsageKeyboardOpenBracket: return 0xDB;
    case UIKeyboardHIDUsageKeyboardCloseBracket: return 0xDD;
    case UIKeyboardHIDUsageKeyboardBackslash: return 0xDC;
    case UIKeyboardHIDUsageKeyboardSemicolon: return 0xBA;
    case UIKeyboardHIDUsageKeyboardQuote: return 0xDE;
    case UIKeyboardHIDUsageKeyboardGraveAccentAndTilde: return 0xC0;
    case UIKeyboardHIDUsageKeyboardComma: return 0xBC;
    case UIKeyboardHIDUsageKeyboardPeriod: return 0xBE;
    case UIKeyboardHIDUsageKeyboardSlash: return 0xBF;
    case UIKeyboardHIDUsageKeyboardCapsLock: return 0x14;
    case UIKeyboardHIDUsageKeyboardPrintScreen: return 0x2A;
    case UIKeyboardHIDUsageKeyboardScrollLock: return 0x91;
    case UIKeyboardHIDUsageKeyboardPause: return 0x13;
    case UIKeyboardHIDUsageKeyboardInsert: return 0x2D;
    case UIKeyboardHIDUsageKeyboardRightArrow: return 0x27;
    case UIKeyboardHIDUsageKeyboardLeftArrow: return 0x25;
    case UIKeyboardHIDUsageKeyboardDownArrow: return 0x28;
    case UIKeyboardHIDUsageKeyboardUpArrow: return 0x26;
    case UIKeyboardHIDUsageKeyboardHome: return 0x24;
    case UIKeyboardHIDUsageKeyboardEnd: return 0x23;
    case UIKeyboardHIDUsageKeyboardPageUp: return 0x21;
    case UIKeyboardHIDUsageKeyboardPageDown: return 0x22;
    case UIKeyboardHIDUsageKeyboardDeleteForward: return 0x2E;
    case UIKeyboardHIDUsageKeypadNumLock: return 0x90;
    case UIKeyboardHIDUsageKeypadSlash: return 0x6F;
    case UIKeyboardHIDUsageKeypadAsterisk: return 0x6A;
    case UIKeyboardHIDUsageKeypadHyphen: return 0x6D;
    case UIKeyboardHIDUsageKeypadPlus: return 0x6B;
    case UIKeyboardHIDUsageKeypadEnter: return 0x0D;
    case UIKeyboardHIDUsageKeypadPeriod: return 0x6E;
    case UIKeyboardHIDUsageKeyboardLeftControl: return 0xA2;
    case UIKeyboardHIDUsageKeyboardLeftShift: return 0xA0;
    case UIKeyboardHIDUsageKeyboardLeftAlt: return 0xA4;
    case UIKeyboardHIDUsageKeyboardLeftGUI: return 0x5B;
    case UIKeyboardHIDUsageKeyboardRightControl: return 0xA3;
    case UIKeyboardHIDUsageKeyboardRightShift: return 0xA1;
    case UIKeyboardHIDUsageKeyboardRightAlt: return 0xA5;
    case UIKeyboardHIDUsageKeyboardRightGUI: return 0x5C;
    default: return 0;
  }
}

@implementation NolandStreamView
- (instancetype)initWithFrame:(CGRect)frame {
  self = [super initWithFrame:frame];
  if (self != nil) {
    self.multipleTouchEnabled = YES;
    _hardwareKeys = [NSMutableSet set];
    _surfaceTouches = [NSMutableSet set];
    self.inputObservers = [NSMutableArray array];
    self.keyboardResponder = [[NolandKeyboardResponder alloc] initWithFrame:CGRectMake(-2, -2, 1, 1)];
    self.keyboardResponder.owner = self;
    [self addSubview:self.keyboardResponder];

    UILabel* statistics = [[UILabel alloc] initWithFrame:CGRectZero];
    statistics.tag = 1003;
    statistics.numberOfLines = 0;
    statistics.font = [UIFont monospacedSystemFontOfSize:9 weight:UIFontWeightMedium];
    statistics.textColor = UIColor.whiteColor;
    statistics.backgroundColor = [UIColor colorWithWhite:0 alpha:0.45];
    statistics.layer.cornerRadius = 4;
    statistics.layer.masksToBounds = YES;
    statistics.hidden = YES;
    statistics.isAccessibilityElement = YES;
    statistics.accessibilityLabel = @"Stream statistics";
    [self addSubview:statistics];

    self.controls = [[NolandStreamControls alloc] initWithFrame:self.bounds];
    [self addSubview:self.controls];
    [self.controls installGesturesOnView:self];
    __weak NolandStreamView* weakSelf = self;
    self.controls.menuChanged = ^(BOOL visible) {
      NolandStreamView* view = weakSelf;
      if (!view) return;
      [view cancelPointerInput];
      g_controller_input.inputSuspended = visible;
      if (visible) {
        [view hideKeyboard];
        nl_runtime_t* runtime = nl_ios_runtime(view.renderer);
        if (runtime) nl_release_all_input(runtime);
        [view->_hardwareKeys removeAllObjects];
      } else [view.controls refreshGamepad];
    };
    self.controls.modeChanged = ^{ [weakSelf cancelPointerInput]; };
    self.controls.keyboardRequested = ^{ [weakSelf showKeyboard]; };
    self.controls.dashboardRequested = ^{ [weakSelf closeStream]; };
    self.controls.gamepadChanged = ^(BOOL enabled, NolandVirtualGamepadState state) {
      // Controller mode owns finger input, including contacts outside a button.
      // Clear any pointer sequence left over from switching modes.
      NolandStreamView* view = weakSelf;
      if (!view) return;
      if (enabled && !view->_gamepadOwnsTouches) [view cancelPointerInput];
      view->_gamepadOwnsTouches = enabled;
      [g_controller_input sendVirtualGamepad:state enabled:enabled];
    };
    id show = [NSNotificationCenter.defaultCenter addObserverForName:UIKeyboardWillShowNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification* note) {
      NolandStreamView* view = weakSelf;
      if (!view || !view.keyboardResponder.isFirstResponder) return;
      view->_keyboardVisible = YES;
      [view cancelPointerInput];
      [view.controls releaseControls];
      view.controls.hidden = YES;
    }];
    id hide = [NSNotificationCenter.defaultCenter addObserverForName:UIKeyboardWillHideNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification* note) {
      NolandStreamView* view = weakSelf;
      if (!view) return;
      view->_keyboardVisible = NO;
      view.controls.hidden = NO;
      [view cancelPointerInput];
    }];
    id inactive = [NSNotificationCenter.defaultCenter addObserverForName:UIApplicationWillResignActiveNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification* note) {
      NolandStreamView* view = weakSelf;
      if (!view) return;
      [view cancelPointerInput];
      [view.controls releaseControls];
      nl_runtime_t* runtime = nl_ios_runtime(view.renderer);
      if (runtime) nl_release_all_input(runtime);
      [view->_hardwareKeys removeAllObjects];
      g_controller_input.inputSuspended = YES;
    }];
    id active = [NSNotificationCenter.defaultCenter addObserverForName:UIApplicationDidBecomeActiveNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification* note) {
      NolandStreamView* view = weakSelf;
      g_controller_input.inputSuspended = view.controls.menuVisible || !view.window;
      [view.controls refreshGamepad];
    }];
    [self.inputObservers addObjectsFromArray:@[show, hide, inactive, active]];

    if (@available(iOS 13.4, *)) {
      UIPanGestureRecognizer* discrete = [[UIPanGestureRecognizer alloc] initWithTarget:self action:@selector(mouseWheelDiscrete:)];
      discrete.maximumNumberOfTouches = 0;
      discrete.allowedScrollTypesMask = UIScrollTypeMaskDiscrete;
      discrete.allowedTouchTypes = @[@(UITouchTypeIndirectPointer)];
      [self addGestureRecognizer:discrete];
      UIPanGestureRecognizer* continuous = [[UIPanGestureRecognizer alloc] initWithTarget:self action:@selector(mouseWheelContinuous:)];
      continuous.maximumNumberOfTouches = 0;
      continuous.allowedScrollTypesMask = UIScrollTypeMaskContinuous;
      continuous.allowedTouchTypes = @[@(UITouchTypeIndirectPointer)];
      [self addGestureRecognizer:continuous];
    }
  }
  return self;
}
- (void)layoutSubviews {
  [super layoutSubviews];
  for (CALayer* layer in self.layer.sublayers) {
    if ([layer isKindOfClass:[AVSampleBufferDisplayLayer class]]) layer.frame = self.bounds;
  }
  self.controls.frame = self.bounds;
  UILabel* statistics = (UILabel*)[self viewWithTag:1003];
  CGFloat maxWidth = MIN(245.0, self.bounds.size.width * 0.4);
  CGSize textSize = [statistics sizeThatFits:CGSizeMake(maxWidth, CGFLOAT_MAX)];
  CGFloat width = ceil(MIN(maxWidth, textSize.width));
  statistics.frame = CGRectMake(CGRectGetMaxX(self.bounds) - self.safeAreaInsets.right - width - 6.0,
                                self.safeAreaInsets.top + 6.0, width, ceil(textSize.height));
}
- (void)closeStream {
  [self cancelPointerInput];
  [self.controls releaseControls];
  [self hideKeyboard];
  [self resignFirstResponder];
  g_controller_input.inputSuspended = YES;
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  if (runtime != NULL) nl_release_all_input(runtime);
  noland_ios_stream_dismiss();
}
- (void)showKeyboard {
  [self cancelPointerInput];
  [self.controls releaseControls];
  self.keyboardResponder.requested = YES;
  if (![self.keyboardResponder becomeFirstResponder]) self.keyboardResponder.requested = NO;
}
- (void)hideKeyboard {
  self.keyboardResponder.requested = NO;
  [self.keyboardResponder resignFirstResponder];
  if (self.window) [self becomeFirstResponder]; // hardware keys, never a software keyboard
}
- (UIEditingInteractionConfiguration)editingInteractionConfiguration { return UIEditingInteractionConfigurationNone; }
- (void)mouseWheelDiscrete:(UIPanGestureRecognizer*)gesture API_AVAILABLE(ios(13.4)) {
  if (self.controls.menuVisible || _keyboardVisible) return;
  if (gesture.state != UIGestureRecognizerStateChanged) return;
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  CGPoint translation = [gesture translationInView:self];
  [gesture setTranslation:CGPointZero inView:self];
  if (runtime != NULL) {
    if (fabs(translation.y) >= 1.0) nl_send_vertical_scroll(runtime, (int16_t)-translation.y, false);
    if (fabs(translation.x) >= 1.0) nl_send_horizontal_scroll(runtime, (int16_t)translation.x, false);
  }
}
- (void)mouseWheelContinuous:(UIPanGestureRecognizer*)gesture API_AVAILABLE(ios(13.4)) {
  if (self.controls.menuVisible || _keyboardVisible) return;
  if (gesture.state != UIGestureRecognizerStateChanged) return;
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  CGPoint translation = [gesture translationInView:self];
  [gesture setTranslation:CGPointZero inView:self];
  if (runtime != NULL) {
    nl_send_vertical_scroll(runtime, (int16_t)lrint(-translation.y * 10.0), true);
    nl_send_horizontal_scroll(runtime, (int16_t)lrint(translation.x * 10.0), true);
  }
}
- (BOOL)canBecomeFirstResponder { return YES; }
- (void)insertText:(NSString*)text {
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  if (runtime != NULL && text.length > 0) {
    // Match Moonlight's keyboard strategy: normal keys for ASCII, UTF-8 only
    // when needed. Linux hosts may implement UTF-8 via Ctrl+Shift+U composition,
    // which is unsuitable for ordinary key presses in games/terminal apps.
    NSData* ascii = [text dataUsingEncoding:NSASCIIStringEncoding allowLossyConversion:NO];
    if (ascii) {
      const uint8_t* chars = ascii.bytes;
      for (NSUInteger i = 0; i < ascii.length; i++) {
        bool shift;
        uint16_t key = nl_ios_ascii_key(chars[i], &shift);
        if (!key) continue;
        BOOL pressShift = shift && ![_hardwareKeys containsObject:@(0xA0)] && ![_hardwareKeys containsObject:@(0xA1)];
        if (pressShift) nl_send_keyboard(runtime, 0xA0, true, MODIFIER_SHIFT);
        nl_send_keyboard(runtime, key, true, shift ? MODIFIER_SHIFT : 0);
        nl_send_keyboard(runtime, key, false, shift ? MODIFIER_SHIFT : 0);
        if (pressShift) nl_send_keyboard(runtime, 0xA0, false, 0);
      }
    } else {
      const char* utf8 = text.UTF8String;
      nl_send_utf8_text(runtime, utf8, (uint32_t)strlen(utf8));
    }
  }
}
- (void)deleteBackward {
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  if (runtime != NULL) {
    nl_send_keyboard(runtime, 0x08, true, 0);
    nl_send_keyboard(runtime, 0x08, false, 0);
  }
}
- (void)sendAbsolutePosition:(CGPoint)point {
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  if (runtime == NULL || self.bounds.size.width <= 0 || self.bounds.size.height <= 0) return;
  CGFloat streamWidth = self.renderer != NULL && self.renderer->width > 0 ? self.renderer->width : self.bounds.size.width;
  CGFloat streamHeight = self.renderer != NULL && self.renderer->height > 0 ? self.renderer->height : self.bounds.size.height;
  CGFloat aspect = streamWidth / MAX(1.0, streamHeight);
  CGSize videoSize = self.bounds.size.width > self.bounds.size.height * aspect
      ? CGSizeMake(self.bounds.size.height * aspect, self.bounds.size.height)
      : CGSizeMake(self.bounds.size.width, self.bounds.size.width / aspect);
  CGPoint origin = CGPointMake((self.bounds.size.width - videoSize.width) / 2.0,
                               (self.bounds.size.height - videoSize.height) / 2.0);
  CGFloat videoX = fmin(videoSize.width, fmax(0, point.x - origin.x));
  CGFloat videoY = fmin(videoSize.height, fmax(0, point.y - origin.y));
  nl_send_absolute_mouse(runtime,
    (int16_t)lrint(videoX), (int16_t)lrint(videoY),
    (int16_t)MIN(INT16_MAX, lrint(videoSize.width)),
    (int16_t)MIN(INT16_MAX, lrint(videoSize.height)));
}
- (void)beginDragTimer {
  [_dragTimer invalidate];
  __weak NolandStreamView* weakSelf = self;
  _dragTimer = [NSTimer scheduledTimerWithTimeInterval:0.650 repeats:NO block:^(NSTimer* timer) {
    (void)timer;
    NolandStreamView* view = weakSelf;
    if (!view || view->_touchMoved || view->_suppressTouchSequence || view->_surfaceTouches.count != 1 || view.controls.menuVisible || view->_keyboardVisible) return;
    nl_runtime_t* runtime = nl_ios_runtime(view.renderer);
    if (runtime == NULL) return;
    if (view.controls.touchMode != NolandTouchModeDirect) {
      view->_dragging = YES;
      nl_send_mouse_button(runtime, BUTTON_LEFT, true);
    } else {
      view->_absoluteRightClick = YES;
      nl_send_mouse_button(runtime, BUTTON_LEFT, false);
      nl_send_mouse_button(runtime, BUTTON_RIGHT, true);
    }
  }];
}
- (BOOL)confirmedMove:(CGPoint)point {
  return hypot(point.x - _originalLocation.x, point.y - _originalLocation.y) >= 5.0;
}
- (void)touchesBegan:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event {
  if (!nl_ios_input_capture_active() || self.controls.menuVisible || self.controls.gamepadEnabled || _keyboardVisible) return;
  if (_surfaceTouches.count == 0) {
    [self finishPendingClick];
    _touchMoved = NO; _suppressTouchSequence = NO; _peakTouchCount = 0;
  }
  for (UITouch* touch in touches) {
    // Exclude gamepad, drawer, keyboard, and hardware mouse touches.
    if (touch.view == self && touch.type != UITouchTypeIndirectPointer) [_surfaceTouches addObject:touch];
  }
  NSArray<UITouch*>* all = _surfaceTouches.allObjects;
  _peakTouchCount = MAX(_peakTouchCount, all.count);
  if (all.count >= 3) { [self cancelPointerInput]; return; }
  if (all.count == 1) {
    _originalLocation = _touchLocation = [all[0] locationInView:self];
    if (self.controls.touchMode != NolandTouchModeTrackpad) {
      [self sendAbsolutePosition:_touchLocation];
    }
    if (self.controls.touchMode == NolandTouchModeDirect) {
      nl_send_mouse_button(nl_ios_runtime(self.renderer), BUTTON_LEFT, true);
    }
    [self beginDragTimer];
  } else if (all.count == 2) {
    [_dragTimer invalidate]; _dragTimer = nil;
    nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
    if (runtime) { nl_send_mouse_button(runtime, BUTTON_LEFT, false); nl_send_mouse_button(runtime, BUTTON_RIGHT, false); }
    _dragging = NO; _absoluteRightClick = NO;
    CGPoint first = [all[0] locationInView:self];
    CGPoint second = [all[1] locationInView:self];
    _originalLocation = _touchLocation = CGPointMake((first.x + second.x) / 2, (first.y + second.y) / 2);
  }
  (void)touches;
}
- (void)touchesMoved:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event {
  if (!nl_ios_input_capture_active() || self.controls.menuVisible || self.controls.gamepadEnabled || _keyboardVisible || _suppressTouchSequence) return;
  NSArray<UITouch*>* all = _surfaceTouches.allObjects;
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  if (runtime == NULL) return;
  if (_peakTouchCount > 1 && all.count < 2) return;
  if (self.controls.touchMode != NolandTouchModeTrackpad && all.count == 1) {
    CGPoint point = [all[0] locationInView:self];
    [self sendAbsolutePosition:point];
    if ([self confirmedMove:point]) { _touchMoved = YES; [_dragTimer invalidate]; }
  } else if (all.count == 1) {
    CGPoint point = [all[0] locationInView:self];
    int dx = (int)lrint((point.x - _touchLocation.x) * (1280.0 / MAX(1.0, self.bounds.size.width)));
    int dy = (int)lrint((point.y - _touchLocation.y) * (720.0 / MAX(1.0, self.bounds.size.height)));
    if (dx != 0 || dy != 0) nl_send_relative_mouse(runtime, (int16_t)dx, (int16_t)dy);
    _touchLocation = point;
    if ([self confirmedMove:point]) { _touchMoved = YES; [_dragTimer invalidate]; }
  } else if (all.count == 2) {
    CGPoint first = [all[0] locationInView:self];
    CGPoint second = [all[1] locationInView:self];
    CGPoint average = CGPointMake((first.x + second.x) / 2, (first.y + second.y) / 2);
    nl_send_vertical_scroll(runtime, (int16_t)lrint((average.y - _touchLocation.y) * 10.0), true);
    if ([self confirmedMove:average]) _touchMoved = YES;
    _touchLocation = average;
  }
  (void)touches;
}
- (void)touchesEnded:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event {
  [_dragTimer invalidate]; _dragTimer = nil;
  BOOL hadTouches = _surfaceTouches.count > 0;
  for (UITouch* touch in touches) [_surfaceTouches removeObject:touch];
  if (!hadTouches) return;
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  if (runtime == NULL || _suppressTouchSequence || self.controls.menuVisible || self.controls.gamepadEnabled || _keyboardVisible || !nl_ios_input_capture_active()) {
    [self cancelPointerInput]; return;
  }
  if (self.controls.touchMode == NolandTouchModeDirect) {
    nl_send_mouse_button(runtime, BUTTON_LEFT, false);
    if (_absoluteRightClick) nl_send_mouse_button(runtime, BUTTON_RIGHT, false);
  } else if (_dragging) {
    nl_send_mouse_button(runtime, BUTTON_LEFT, false);
    _touchMoved = YES;
  } else if (!_touchMoved && _surfaceTouches.count == 0 && _peakTouchCount < 3) {
    if (self.controls.touchMode == NolandTouchModeClickToUse && _peakTouchCount == 1)
      [self sendAbsolutePosition:[[touches anyObject] locationInView:self]];
    uint8_t button = _peakTouchCount >= 2 ? BUTTON_RIGHT : BUTTON_LEFT;
    nl_send_mouse_button(runtime, button, true);
    _pendingClickButton = button;
    __weak NolandStreamView* weakSelf = self;
    _clickTimer = [NSTimer scheduledTimerWithTimeInterval:0.05 repeats:NO block:^(NSTimer* timer) { [weakSelf finishPendingClick]; }];
  }
  _dragging = NO; _absoluteRightClick = NO;
  if (_surfaceTouches.count == 0) _peakTouchCount = 0;
}
- (void)touchesCancelled:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event {
  // Cancellation (menu gesture/background/keyboard) is never a click.
  [self cancelPointerInput];
}
- (void)finishPendingClick {
  [_clickTimer invalidate]; _clickTimer = nil;
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  if (runtime && _pendingClickButton) nl_send_mouse_button(runtime, _pendingClickButton, false);
  _pendingClickButton = 0;
}
- (void)cancelPointerInput {
  [_dragTimer invalidate]; _dragTimer = nil;
  [self finishPendingClick];
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  if (runtime) { nl_send_mouse_button(runtime, BUTTON_LEFT, false); nl_send_mouse_button(runtime, BUTTON_RIGHT, false); }
  [_surfaceTouches removeAllObjects];
  _dragging = NO; _absoluteRightClick = NO; _touchMoved = YES; _suppressTouchSequence = YES; _peakTouchCount = 0;
}
- (void)pressesBegan:(NSSet<UIPress*>*)presses withEvent:(UIPressesEvent*)event {
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  NSMutableSet* unhandled = [presses mutableCopy];
  for (UIPress* press in presses) {
    if (press.key == nil || runtime == NULL || self.controls.menuVisible) continue;
    uint16_t key = nl_ios_virtual_key(press.key.keyCode);
    if (key != 0) {
      if (![_hardwareKeys containsObject:@(key)]) nl_send_keyboard(runtime, key, true, nl_ios_modifiers(press.key.modifierFlags));
      [_hardwareKeys addObject:@(key)];
      [unhandled removeObject:press];
    }
  }
  if (unhandled.count) [super pressesBegan:unhandled withEvent:event];
}
- (void)pressesEnded:(NSSet<UIPress*>*)presses withEvent:(UIPressesEvent*)event {
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  NSMutableSet* unhandled = [presses mutableCopy];
  for (UIPress* press in presses) {
    if (press.key == nil || runtime == NULL) continue;
    uint16_t key = nl_ios_virtual_key(press.key.keyCode);
    if (key != 0) {
      nl_send_keyboard(runtime, key, false, nl_ios_modifiers(press.key.modifierFlags));
      [_hardwareKeys removeObject:@(key)];
      [unhandled removeObject:press];
    }
  }
  if (unhandled.count) [super pressesEnded:unhandled withEvent:event];
}
- (void)pressesCancelled:(NSSet<UIPress*>*)presses withEvent:(UIPressesEvent*)event {
  [self pressesEnded:presses withEvent:event];
}
- (void)prepareForRemoval {
  [self cancelPointerInput];
  [self.controls releaseControls];
  [self hideKeyboard];
  [self resignFirstResponder];
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  if (runtime != NULL) nl_release_all_input(runtime);
  self.renderer = NULL;
}
- (void)didMoveToWindow {
  [super didMoveToWindow];
  if (!self.window) {
    [self hideKeyboard];
    [self cancelPointerInput]; [self.controls releaseControls];
    [_hardwareKeys removeAllObjects];
  }
  g_controller_input.inputSuspended = !self.window || self.controls.menuVisible;
  if (self.window) { [self becomeFirstResponder]; [self.controls refreshGamepad]; }
}
- (void)dealloc {
  [_dragTimer invalidate]; [_clickTimer invalidate];
  for (id observer in self.inputObservers) [NSNotificationCenter.defaultCenter removeObserver:observer];
}
@end

static int16_t nl_ios_axis(float value) {
  return (int16_t)lrintf(fminf(1.0f, fmaxf(-1.0f, value)) * 32767.0f);
}

@implementation NolandControllerInput
- (NSUInteger)nextControllerNumber {
  bool occupied[16] = { false };
  if (self.virtualNumber < 16) occupied[self.virtualNumber] = true;
  for (NSNumber* value in self.controllerNumbers.objectEnumerator) {
    if (value.unsignedIntegerValue < 16) occupied[value.unsignedIntegerValue] = true;
  }
  for (NSUInteger index = 0; index < 16; index++) if (!occupied[index]) return index;
  return NSNotFound;
}
- (uint16_t)activeMask {
  uint16_t mask = self.virtualNumber < 16 ? (uint16_t)(1u << self.virtualNumber) : 0;
  for (NSNumber* value in self.controllerNumbers.objectEnumerator) {
    NSUInteger number = value.unsignedIntegerValue;
    if (number < 16) mask |= (uint16_t)(1u << number);
  }
  return mask;
}
- (void)bindController:(GCController*)controller {
  GCExtendedGamepad* pad = controller.extendedGamepad;
  if (pad == nil) return;
  NSNumber* existing = [self.controllerNumbers objectForKey:controller];
  NSUInteger number = existing != nil ? existing.unsignedIntegerValue : [self nextControllerNumber];
  if (number == NSNotFound) return;
  [self.controllerNumbers setObject:@(number) forKey:controller];
  if (@available(iOS 14.0, *)) {
    NSArray* motors = @[
      [[NolandHapticMotor alloc] initWithController:controller locality:GCHapticsLocalityLeftHandle] ?: NSNull.null,
      [[NolandHapticMotor alloc] initWithController:controller locality:GCHapticsLocalityRightHandle] ?: NSNull.null,
      [[NolandHapticMotor alloc] initWithController:controller locality:GCHapticsLocalityLeftTrigger] ?: NSNull.null,
      [[NolandHapticMotor alloc] initWithController:controller locality:GCHapticsLocalityRightTrigger] ?: NSNull.null,
    ];
    self.hapticMotors[@(number)] = motors;
  }
  __weak NolandControllerInput* weakSelf = self;
  pad.valueChangedHandler = ^(GCExtendedGamepad* gamepad, GCControllerElement* element) {
    (void)element;
    NolandControllerInput* strongSelf = weakSelf;
    nl_runtime_t* runtime = nl_ios_runtime(strongSelf.renderer);
    if (runtime == NULL || strongSelf.inputSuspended) return;
    uint32_t buttons = 0;
    if (gamepad.buttonA.isPressed) buttons |= A_FLAG;
    if (gamepad.buttonB.isPressed) buttons |= B_FLAG;
    if (gamepad.buttonX.isPressed) buttons |= X_FLAG;
    if (gamepad.buttonY.isPressed) buttons |= Y_FLAG;
    if (gamepad.dpad.up.isPressed) buttons |= UP_FLAG;
    if (gamepad.dpad.down.isPressed) buttons |= DOWN_FLAG;
    if (gamepad.dpad.left.isPressed) buttons |= LEFT_FLAG;
    if (gamepad.dpad.right.isPressed) buttons |= RIGHT_FLAG;
    if (gamepad.leftShoulder.isPressed) buttons |= LB_FLAG;
    if (gamepad.rightShoulder.isPressed) buttons |= RB_FLAG;
    if (gamepad.buttonMenu.isPressed) buttons |= PLAY_FLAG;
    if (gamepad.buttonOptions != nil && gamepad.buttonOptions.isPressed) buttons |= BACK_FLAG;
    if (gamepad.buttonHome != nil && gamepad.buttonHome.isPressed) buttons |= SPECIAL_FLAG;
    if (gamepad.leftThumbstickButton != nil && gamepad.leftThumbstickButton.isPressed) buttons |= LS_CLK_FLAG;
    if (gamepad.rightThumbstickButton != nil && gamepad.rightThumbstickButton.isPressed) buttons |= RS_CLK_FLAG;
    NSNumber* assigned = [strongSelf.controllerNumbers objectForKey:controller];
    if (assigned == nil) return;
    NSUInteger currentNumber = assigned.unsignedIntegerValue;
    uint16_t mask = [strongSelf activeMask];
    if (![strongSelf.announcedControllers containsObject:assigned]) {
      uint32_t supported = A_FLAG | B_FLAG | X_FLAG | Y_FLAG | UP_FLAG | DOWN_FLAG |
          LEFT_FLAG | RIGHT_FLAG | LB_FLAG | RB_FLAG | PLAY_FLAG | BACK_FLAG |
          LS_CLK_FLAG | RS_CLK_FLAG;
      if (gamepad.buttonHome != nil) supported |= SPECIAL_FLAG;
      uint8_t controllerType = LI_CTYPE_XBOX;
      uint16_t capabilities = 0;
      if (@available(iOS 14.0, *)) {
        if ([gamepad isKindOfClass:[GCDualShockGamepad class]] ||
            [gamepad isKindOfClass:[GCDualSenseGamepad class]]) controllerType = LI_CTYPE_PS;
        if (controller.motion.hasGravityAndUserAcceleration) capabilities |= LI_CCAP_ACCEL;
        if (controller.motion.hasRotationRate) capabilities |= LI_CCAP_GYRO;
        if (controller.light != nil) capabilities |= LI_CCAP_RGB_LED;
        if (controller.battery != nil) capabilities |= LI_CCAP_BATTERY_STATE;
        if ([controller.haptics.supportedLocalities containsObject:GCHapticsLocalityHandles])
          capabilities |= LI_CCAP_RUMBLE;
        if ([controller.haptics.supportedLocalities containsObject:GCHapticsLocalityTriggers])
          capabilities |= LI_CCAP_TRIGGER_RUMBLE;
      }
      if (nl_send_controller_arrival(runtime, (uint8_t)currentNumber, mask, controllerType,
                                     supported, capabilities) == NL_RESULT_OK) {
        [strongSelf.announcedControllers addObject:assigned];
        if (controller.battery != nil) {
          uint8_t state = LI_BATTERY_STATE_UNKNOWN;
          switch (controller.battery.batteryState) {
            case GCDeviceBatteryStateCharging: state = LI_BATTERY_STATE_CHARGING; break;
            case GCDeviceBatteryStateDischarging: state = LI_BATTERY_STATE_DISCHARGING; break;
            case GCDeviceBatteryStateFull: state = LI_BATTERY_STATE_FULL; break;
            default: break;
          }
          LiSendControllerBatteryEvent((uint8_t)currentNumber, state,
              (uint8_t)lrintf(fminf(1.0f, fmaxf(0.0f, controller.battery.batteryLevel)) * 100.0f));
        }
      }
    }
    nl_send_controller(runtime, (uint16_t)currentNumber, mask, buttons,
      (uint8_t)lrintf(gamepad.leftTrigger.value * 255.0f),
      (uint8_t)lrintf(gamepad.rightTrigger.value * 255.0f),
      nl_ios_axis(gamepad.leftThumbstick.xAxis.value),
      nl_ios_axis(gamepad.leftThumbstick.yAxis.value),
      nl_ios_axis(gamepad.rightThumbstick.xAxis.value),
      nl_ios_axis(gamepad.rightThumbstick.yAxis.value));
  };
}
- (void)bindMouse:(GCMouse*)mouse API_AVAILABLE(ios(14.0)) {
  __weak NolandControllerInput* weakSelf = self;
  mouse.mouseInput.mouseMovedHandler = ^(GCMouseInput* input, float deltaX, float deltaY) {
    (void)input;
    NolandControllerInput* strongSelf = weakSelf;
    nl_runtime_t* runtime = nl_ios_runtime(strongSelf.renderer);
    if (runtime == NULL || strongSelf.inputSuspended) return;
    strongSelf.accumulatedMouseX += deltaX / 1.25f;
    strongSelf.accumulatedMouseY += -deltaY / 1.25f;
    int16_t x = (int16_t)strongSelf.accumulatedMouseX;
    int16_t y = (int16_t)strongSelf.accumulatedMouseY;
    if (x != 0 || y != 0) {
      nl_send_relative_mouse(runtime, x, y);
      strongSelf.accumulatedMouseX -= x;
      strongSelf.accumulatedMouseY -= y;
    }
  };
#define BIND_MOUSE_BUTTON(input, code) \
  input.pressedChangedHandler = ^(GCControllerButtonInput* button, float value, BOOL pressed) { \
    (void)button; (void)value; \
    nl_runtime_t* runtime = nl_ios_runtime(weakSelf.renderer); \
    if (runtime != NULL && !weakSelf.inputSuspended) nl_send_mouse_button(runtime, code, pressed); \
  }
  BIND_MOUSE_BUTTON(mouse.mouseInput.leftButton, BUTTON_LEFT);
  BIND_MOUSE_BUTTON(mouse.mouseInput.middleButton, BUTTON_MIDDLE);
  BIND_MOUSE_BUTTON(mouse.mouseInput.rightButton, BUTTON_RIGHT);
  if (mouse.mouseInput.auxiliaryButtons.count > 0)
    BIND_MOUSE_BUTTON(mouse.mouseInput.auxiliaryButtons[0], BUTTON_X1);
  if (mouse.mouseInput.auxiliaryButtons.count > 1)
    BIND_MOUSE_BUTTON(mouse.mouseInput.auxiliaryButtons[1], BUTTON_X2);
#undef BIND_MOUSE_BUTTON
}
- (void)start {
  self.virtualNumber = NSNotFound;
  self.observers = [NSMutableArray array];
  self.controllerNumbers = [NSMapTable weakToStrongObjectsMapTable];
  self.announcedControllers = [NSMutableSet set];
  self.hapticMotors = [NSMutableDictionary dictionary];
  self.motionTimers = [NSMutableDictionary dictionary];
  g_controller_input = self;
  [GCController.controllers enumerateObjectsUsingBlock:^(GCController* controller, NSUInteger index, BOOL* stop) {
    (void)index; (void)stop; [self bindController:controller];
  }];
  __weak NolandControllerInput* weakSelf = self;
  id connected = [NSNotificationCenter.defaultCenter addObserverForName:GCControllerDidConnectNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification* note) {
    GCController* controller = note.object;
    [weakSelf bindController:controller];
  }];
  [self.observers addObject:connected];
  id disconnected = [NSNotificationCenter.defaultCenter addObserverForName:GCControllerDidDisconnectNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification* note) {
    NolandControllerInput* strongSelf = weakSelf;
    GCController* controller = note.object;
    NSNumber* assigned = [strongSelf.controllerNumbers objectForKey:controller];
    if (assigned == nil) return;
    [strongSelf.controllerNumbers removeObjectForKey:controller];
    [strongSelf.announcedControllers removeObject:assigned];
    for (id motor in strongSelf.hapticMotors[assigned])
      if ([motor isKindOfClass:[NolandHapticMotor class]]) [motor stop];
    [strongSelf.hapticMotors removeObjectForKey:assigned];
    NSString* motionPrefix = [NSString stringWithFormat:@"%@:", assigned];
    for (NSString* key in strongSelf.motionTimers.allKeys) {
      if ([key hasPrefix:motionPrefix]) {
        [strongSelf.motionTimers[key] invalidate];
        [strongSelf.motionTimers removeObjectForKey:key];
      }
    }
    nl_runtime_t* runtime = nl_ios_runtime(strongSelf.renderer);
    if (runtime != NULL) {
      nl_send_controller(runtime, assigned.unsignedShortValue, [strongSelf activeMask], 0, 0, 0, 0, 0, 0, 0);
    }
  }];
  [self.observers addObject:disconnected];
  if (@available(iOS 14.0, *)) {
    for (GCMouse* mouse in GCMouse.mice) [self bindMouse:mouse];
    id mouseConnected = [NSNotificationCenter.defaultCenter addObserverForName:GCMouseDidConnectNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification* note) {
      [weakSelf bindMouse:note.object];
    }];
    [self.observers addObject:mouseConnected];
  }
}
- (void)announceControllers {
  for (GCController* controller in self.controllerNumbers.keyEnumerator) {
    GCExtendedGamepad* gamepad = controller.extendedGamepad;
    if (gamepad != nil && gamepad.valueChangedHandler != nil)
      gamepad.valueChangedHandler(gamepad, gamepad.buttonA);
  }
}
- (void)sendVirtualGamepad:(NolandVirtualGamepadState)state enabled:(BOOL)enabled {
  nl_runtime_t* runtime = nl_ios_runtime(self.renderer);
  if (!runtime) return;
  if (!enabled) {
    NSUInteger number = self.virtualNumber;
    self.virtualNumber = NSNotFound; self.virtualAnnounced = NO;
    if (number < 16) nl_send_controller(runtime, (uint16_t)number, [self activeMask], 0, 0, 0, 0, 0, 0, 0);
    return;
  }
  if (self.virtualNumber == NSNotFound) self.virtualNumber = [self nextControllerNumber];
  if (self.virtualNumber == NSNotFound) return;
  uint16_t mask = [self activeMask];
  if (!self.virtualAnnounced) {
    uint32_t buttons = A_FLAG | B_FLAG | X_FLAG | Y_FLAG | UP_FLAG | DOWN_FLAG | LEFT_FLAG | RIGHT_FLAG |
        LB_FLAG | RB_FLAG | PLAY_FLAG | BACK_FLAG | LS_CLK_FLAG | RS_CLK_FLAG;
    self.virtualAnnounced = nl_send_controller_arrival(runtime, (uint8_t)self.virtualNumber, mask, LI_CTYPE_XBOX, buttons, 0) == NL_RESULT_OK;
  }
  if (self.inputSuspended) state = (NolandVirtualGamepadState){0};
  nl_send_controller(runtime, (uint16_t)self.virtualNumber, mask, state.buttons,
      state.leftTrigger, state.rightTrigger, state.leftX, state.leftY, state.rightX, state.rightY);
}
- (void)stop {
  [self sendVirtualGamepad:(NolandVirtualGamepadState){0} enabled:NO];
  for (id observer in self.observers) [NSNotificationCenter.defaultCenter removeObserver:observer];
  for (GCController* controller in GCController.controllers) controller.extendedGamepad.valueChangedHandler = nil;
  if (@available(iOS 14.0, *)) {
    for (GCMouse* mouse in GCMouse.mice) {
      mouse.mouseInput.mouseMovedHandler = nil;
      mouse.mouseInput.leftButton.pressedChangedHandler = nil;
      mouse.mouseInput.middleButton.pressedChangedHandler = nil;
      mouse.mouseInput.rightButton.pressedChangedHandler = nil;
      for (GCControllerButtonInput* button in mouse.mouseInput.auxiliaryButtons)
        button.pressedChangedHandler = nil;
    }
  }
  [self.observers removeAllObjects];
  for (NSArray* motors in self.hapticMotors.allValues)
    for (id motor in motors)
      if ([motor isKindOfClass:[NolandHapticMotor class]]) [motor stop];
  [self.hapticMotors removeAllObjects];
  for (NSTimer* timer in self.motionTimers.allValues) [timer invalidate];
  [self.motionTimers removeAllObjects];
  [self.controllerNumbers removeAllObjects];
  [self.announcedControllers removeAllObjects];
  if (g_controller_input == self) g_controller_input = nil;
}
@end

static void nl_ios_set_haptic(uint16_t controller_number, NSUInteger motor_index, uint16_t amplitude) {
  dispatch_async(dispatch_get_main_queue(), ^{
    NSArray* motors = g_controller_input.hapticMotors[@(controller_number)];
    if (motor_index >= motors.count) return;
    id motor = motors[motor_index];
    if ([motor isKindOfClass:[NolandHapticMotor class]]) [motor setAmplitude:amplitude];
  });
}

void nl_ios_controller_rumble(uint16_t controller_number, uint16_t low_frequency, uint16_t high_frequency) {
  nl_ios_set_haptic(controller_number, 0, low_frequency);
  nl_ios_set_haptic(controller_number, 1, high_frequency);
}

void nl_ios_controller_rumble_triggers(uint16_t controller_number, uint16_t left, uint16_t right) {
  nl_ios_set_haptic(controller_number, 2, left);
  nl_ios_set_haptic(controller_number, 3, right);
}

void nl_ios_controller_set_led(uint16_t controller_number, uint8_t r, uint8_t g, uint8_t b) {
  dispatch_async(dispatch_get_main_queue(), ^{
    for (GCController* controller in g_controller_input.controllerNumbers.keyEnumerator) {
      if ([g_controller_input.controllerNumbers objectForKey:controller].unsignedShortValue != controller_number) continue;
      if (@available(iOS 14.0, *))
        controller.light.color = [[GCColor alloc] initWithRed:r / 255.0f green:g / 255.0f blue:b / 255.0f];
      break;
    }
  });
}

void nl_ios_controller_set_motion(uint16_t controller_number, uint8_t motion_type, uint16_t report_rate_hz) {
  dispatch_async(dispatch_get_main_queue(), ^{
    NSString* key = [NSString stringWithFormat:@"%u:%u", controller_number, motion_type];
    [g_controller_input.motionTimers[key] invalidate];
    [g_controller_input.motionTimers removeObjectForKey:key];
    GCController* selected = nil;
    for (GCController* controller in g_controller_input.controllerNumbers.keyEnumerator) {
      if ([g_controller_input.controllerNumbers objectForKey:controller].unsignedShortValue == controller_number) {
        selected = controller; break;
      }
    }
    if (selected == nil || selected.motion == nil) return;
    if (report_rate_hz == 0) {
      if (g_controller_input.motionTimers.count == 0 && selected.motion.sensorsRequireManualActivation)
        selected.motion.sensorsActive = NO;
      return;
    }
    if (selected.motion.sensorsRequireManualActivation) selected.motion.sensorsActive = YES;
    NSTimer* timer = [NSTimer scheduledTimerWithTimeInterval:1.0 / report_rate_hz repeats:YES block:^(NSTimer* timer) {
      (void)timer;
      if (motion_type == LI_MOTION_TYPE_ACCEL && selected.motion.hasGravityAndUserAcceleration) {
        GCAcceleration value = selected.motion.acceleration;
        LiSendControllerMotionEvent((uint8_t)controller_number, LI_MOTION_TYPE_ACCEL,
                                    value.x * -9.80665f, value.y * -9.80665f, value.z * -9.80665f);
      } else if (motion_type == LI_MOTION_TYPE_GYRO && selected.motion.hasRotationRate) {
        GCRotationRate value = selected.motion.rotationRate;
        LiSendControllerMotionEvent((uint8_t)controller_number, LI_MOTION_TYPE_GYRO,
                                    value.x * 57.2957795f, value.z * 57.2957795f, value.y * -57.2957795f);
      }
    }];
    g_controller_input.motionTimers[key] = timer;
  });
}

static void nl_ios_start_lifecycle_observers(nl_video_renderer_t* renderer, nl_ios_video_context_t* ctx) {
  if (ctx == NULL || ctx->lifecycle_observers != nil) return;
  ctx->lifecycle_observers = [NSMutableArray array];
  NSNotificationCenter* center = NSNotificationCenter.defaultCenter;
  id resign = [center addObserverForName:UIApplicationWillResignActiveNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification* note) {
    (void)note;
    nl_runtime_t* runtime = nl_ios_runtime(renderer);
    if (runtime != NULL) nl_release_all_input(runtime);
    ctx->display_link.paused = YES;
  }];
  [ctx->lifecycle_observers addObject:resign];
  id foreground = [center addObserverForName:UIApplicationDidBecomeActiveNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification* note) {
    (void)note;
    [ctx->layer flush];
    ctx->display_link.paused = NO;
    LiRequestIdrFrame();
  }];
  [ctx->lifecycle_observers addObject:foreground];
}

static void nl_ios_stop_lifecycle_observers(nl_ios_video_context_t* ctx) {
  if (ctx == NULL || ctx->lifecycle_observers == nil) return;
  for (id observer in ctx->lifecycle_observers) {
    [NSNotificationCenter.defaultCenter removeObserver:observer];
  }
  [ctx->lifecycle_observers removeAllObjects];
  ctx->lifecycle_observers = nil;
}

static void nl_ios_run_on_main_sync(dispatch_block_t block) {
  if ([NSThread isMainThread]) {
    block();
  } else {
    dispatch_sync(dispatch_get_main_queue(), block);
  }
}

static nl_ios_video_context_t* nl_ios_context(nl_video_renderer_t* renderer) {
  return renderer ? (nl_ios_video_context_t*)renderer->platform_context : NULL;
}

static nl_ios_video_context_t* nl_ios_ensure_context(nl_video_renderer_t* renderer) {
  nl_ios_video_context_t* ctx = nl_ios_context(renderer);
  if (ctx != NULL) return ctx;
  ctx = calloc(1, sizeof(*ctx));
  if (ctx != NULL) renderer->platform_context = ctx;
  return ctx;
}

static UIWindow* nl_ios_active_window(void) {
  UIApplication* application = UIApplication.sharedApplication;
  if (@available(iOS 13.0, *)) {
    for (UIScene* scene in application.connectedScenes) {
      if (![scene isKindOfClass:[UIWindowScene class]]) continue;
      UIWindowScene* window_scene = (UIWindowScene*)scene;
      if (window_scene.activationState != UISceneActivationStateForegroundActive) continue;
      for (UIWindow* window in window_scene.windows) {
        if (window.isKeyWindow) return window;
      }
      for (UIWindow* window in window_scene.windows) {
        if (!window.hidden) return window;
      }
    }
  }
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
  return application.keyWindow;
#pragma clang diagnostic pop
}

static CGRect nl_ios_stream_frame(UIWindow* window) {
  CGRect frame = window.bounds;
  CGFloat controls = 84.0 + window.safeAreaInsets.bottom;
  frame.size.height = fmax(1.0, frame.size.height - controls);
  return frame;
}

static UIView* nl_ios_resolve_render_view(nl_ios_video_context_t* ctx) {
  if (ctx == NULL) return nil;
  if (ctx->view != nil) return ctx->view;

  UIWindow* window = nl_ios_active_window();
  if (window == nil) return nil;

  UIView* target = ctx->owned_view;
  if (target == nil) {
    NolandStreamView* stream_view = [[NolandStreamView alloc] initWithFrame:nl_ios_stream_frame(window)];
    stream_view.multipleTouchEnabled = YES;
    target = stream_view;
    target.backgroundColor = UIColor.blackColor;
    target.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
    [window addSubview:target];
    ctx->owned_view = target;
  } else if (target.superview != window) {
    target.frame = nl_ios_stream_frame(window);
    [window addSubview:target];
  } else {
    target.frame = nl_ios_stream_frame(window);
  }

  ctx->view = target;
  return target;
}

static void nl_ios_free_parameter_set(uint8_t** bytes, size_t* length) {
  if (bytes != NULL && *bytes != NULL) {
    free(*bytes);
    *bytes = NULL;
  }
  if (length != NULL) *length = 0;
}

static void nl_ios_reset_format_description(nl_ios_video_context_t* ctx) {
  if (ctx == NULL) return;
  if (ctx->format_description != NULL) {
    CFRelease(ctx->format_description);
    ctx->format_description = NULL;
  }
}

static void nl_ios_store_parameter_set(uint8_t** dst, size_t* dst_len, const uint8_t* src, size_t src_len) {
  if (dst == NULL || dst_len == NULL || src == NULL || src_len == 0) return;
  uint8_t* next = malloc(src_len);
  if (next == NULL) return;
  memcpy(next, src, src_len);
  nl_ios_free_parameter_set(dst, dst_len);
  *dst = next;
  *dst_len = src_len;
}

static bool nl_ios_strip_annexb_start_code(const uint8_t* data, size_t length,
                                           const uint8_t** payload, size_t* payload_len) {
  size_t offset = 0;
  if (data == NULL || payload == NULL || payload_len == NULL || length == 0) return false;

  if (length >= 4 && data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x00 && data[3] == 0x01) {
    offset = 4;
  } else if (length >= 3 && data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x01) {
    offset = 3;
  }

  if (offset >= length) return false;
  *payload = data + offset;
  *payload_len = length - offset;
  return true;
}

static bool nl_ios_find_next_annexb_nal(const uint8_t* data, size_t length, size_t* cursor,
                                        const uint8_t** nal, size_t* nal_len) {
  size_t i;
  size_t start = SIZE_MAX;
  if (data == NULL || cursor == NULL || nal == NULL || nal_len == NULL) return false;

  for (i = *cursor; i + 3 < length; ++i) {
    if (i + 4 <= length && data[i] == 0x00 && data[i + 1] == 0x00 && data[i + 2] == 0x00 && data[i + 3] == 0x01) {
      start = i + 4;
      break;
    }
    if (data[i] == 0x00 && data[i + 1] == 0x00 && data[i + 2] == 0x01) {
      start = i + 3;
      break;
    }
  }

  if (start == SIZE_MAX || start >= length) return false;

  size_t end = length;
  for (i = start; i + 2 < length; ++i) {
    if (i + 4 <= length && data[i] == 0x00 && data[i + 1] == 0x00 && data[i + 2] == 0x00 && data[i + 3] == 0x01) {
      end = i;
      break;
    }
    if (data[i] == 0x00 && data[i + 1] == 0x00 && data[i + 2] == 0x01) {
      end = i;
      break;
    }
  }

  *nal = data + start;
  *nal_len = end - start;
  *cursor = end;
  return *nal_len > 0;
}

static CFDictionaryRef nl_ios_copy_hevc_extensions(void) {
  if (!LiGetCurrentHostDisplayHdrMode()) return NULL;
  NSMutableDictionary* extensions = [NSMutableDictionary dictionary];
  extensions[(__bridge NSString*)kCMFormatDescriptionExtension_ColorPrimaries] =
      (__bridge NSString*)kCMFormatDescriptionColorPrimaries_ITU_R_2020;
  extensions[(__bridge NSString*)kCMFormatDescriptionExtension_TransferFunction] =
      (__bridge NSString*)kCMFormatDescriptionTransferFunction_SMPTE_ST_2084_PQ;
  extensions[(__bridge NSString*)kCMFormatDescriptionExtension_YCbCrMatrix] =
      (__bridge NSString*)kCMFormatDescriptionYCbCrMatrix_ITU_R_2020;

  SS_HDR_METADATA metadata;
  if (LiGetHdrMetadata(&metadata)) {
    if (metadata.displayPrimaries[0].x != 0 && metadata.maxDisplayLuminance != 0) {
      struct {
        vector_ushort2 primaries[3];
        vector_ushort2 white_point;
        uint32_t luminance_max;
        uint32_t luminance_min;
      } __attribute__((packed, aligned(4))) mdcv;
      mdcv.primaries[0] = (vector_ushort2){__builtin_bswap16(metadata.displayPrimaries[1].x), __builtin_bswap16(metadata.displayPrimaries[1].y)};
      mdcv.primaries[1] = (vector_ushort2){__builtin_bswap16(metadata.displayPrimaries[2].x), __builtin_bswap16(metadata.displayPrimaries[2].y)};
      mdcv.primaries[2] = (vector_ushort2){__builtin_bswap16(metadata.displayPrimaries[0].x), __builtin_bswap16(metadata.displayPrimaries[0].y)};
      mdcv.white_point = (vector_ushort2){__builtin_bswap16(metadata.whitePoint.x), __builtin_bswap16(metadata.whitePoint.y)};
      mdcv.luminance_max = __builtin_bswap32((uint32_t)metadata.maxDisplayLuminance * 10000U);
      mdcv.luminance_min = __builtin_bswap32(metadata.minDisplayLuminance);
      extensions[(__bridge NSString*)kCMFormatDescriptionExtension_MasteringDisplayColorVolume] =
          [NSData dataWithBytes:&mdcv length:sizeof(mdcv)];
    }
    if (metadata.maxContentLightLevel != 0 && metadata.maxFrameAverageLightLevel != 0) {
      uint16_t clli[2] = {
        __builtin_bswap16(metadata.maxContentLightLevel),
        __builtin_bswap16(metadata.maxFrameAverageLightLevel),
      };
      extensions[(__bridge NSString*)kCMFormatDescriptionExtension_ContentLightLevelInfo] =
          [NSData dataWithBytes:clli length:sizeof(clli)];
    }
  }
  return CFBridgingRetain(extensions);
}

static bool nl_ios_update_format_description(nl_ios_video_context_t* ctx) {
  CMVideoFormatDescriptionRef fd = NULL;
  OSStatus status;
  if (ctx == NULL) return false;

  if (ctx->video_format & VIDEO_FORMAT_MASK_H264) {
    if (ctx->sps == NULL || ctx->pps == NULL || ctx->sps_len == 0 || ctx->pps_len == 0) return false;
    const uint8_t* sets[2] = { ctx->sps, ctx->pps };
    size_t sizes[2] = { ctx->sps_len, ctx->pps_len };
    status = CMVideoFormatDescriptionCreateFromH264ParameterSets(kCFAllocatorDefault, 2, sets, sizes, 4, &fd);
  } else if (ctx->video_format & VIDEO_FORMAT_MASK_H265) {
    const uint8_t* sets[3];
    size_t sizes[3];
    size_t count = 0;

    if (ctx->vps != NULL && ctx->vps_len > 0) {
      sets[count] = ctx->vps;
      sizes[count] = ctx->vps_len;
      count++;
    }
    if (ctx->sps != NULL && ctx->sps_len > 0) {
      sets[count] = ctx->sps;
      sizes[count] = ctx->sps_len;
      count++;
    }
    if (ctx->pps != NULL && ctx->pps_len > 0) {
      sets[count] = ctx->pps;
      sizes[count] = ctx->pps_len;
      count++;
    }

    if (count == 0) return false;
    CFDictionaryRef extensions = nl_ios_copy_hevc_extensions();
    status = CMVideoFormatDescriptionCreateFromHEVCParameterSets(kCFAllocatorDefault, count, sets, sizes, 4, extensions, &fd);
    if (extensions != NULL) CFRelease(extensions);
  } else {
    return false;
  }

  if (status != noErr || fd == NULL) return false;
  nl_ios_reset_format_description(ctx);
  ctx->format_description = fd;
  return true;
}

static void nl_ios_collect_parameter_sets(nl_ios_video_context_t* ctx, const DECODE_UNIT* du) {
  const LENTRY* entry;
  if (ctx == NULL || du == NULL) return;

  for (entry = du->bufferList; entry != NULL; entry = entry->next) {
    const uint8_t* payload = NULL;
    size_t payload_len = 0;
    if (!nl_ios_strip_annexb_start_code((const uint8_t*)entry->data, (size_t)entry->length, &payload, &payload_len)) continue;

    if (entry->bufferType == BUFFER_TYPE_SPS) {
      nl_ios_store_parameter_set(&ctx->sps, &ctx->sps_len, payload, payload_len);
    } else if (entry->bufferType == BUFFER_TYPE_PPS) {
      nl_ios_store_parameter_set(&ctx->pps, &ctx->pps_len, payload, payload_len);
    } else if (entry->bufferType == BUFFER_TYPE_VPS) {
      nl_ios_store_parameter_set(&ctx->vps, &ctx->vps_len, payload, payload_len);
    }
  }

  nl_ios_update_format_description(ctx);
}

static uint8_t* nl_ios_build_avcc_sample(const DECODE_UNIT* du, size_t* out_len) {
  size_t annexb_len = 0;
  size_t avcc_len = 0;
  uint8_t* annexb = NULL;
  uint8_t* avcc = NULL;
  uint8_t* cursor = NULL;
  const LENTRY* entry;

  if (out_len != NULL) *out_len = 0;
  if (du == NULL || out_len == NULL) return NULL;

  for (entry = du->bufferList; entry != NULL; entry = entry->next) {
    if (entry->bufferType == BUFFER_TYPE_PICDATA) annexb_len += (size_t)entry->length;
  }
  if (annexb_len == 0) return NULL;

  annexb = malloc(annexb_len);
  if (annexb == NULL) return NULL;

  cursor = annexb;
  for (entry = du->bufferList; entry != NULL; entry = entry->next) {
    if (entry->bufferType == BUFFER_TYPE_PICDATA) {
      memcpy(cursor, entry->data, (size_t)entry->length);
      cursor += (size_t)entry->length;
    }
  }

  size_t c = 0;
  while (c < annexb_len) {
    const uint8_t* nal = NULL;
    size_t nal_len = 0;
    if (!nl_ios_find_next_annexb_nal(annexb, annexb_len, &c, &nal, &nal_len)) break;
    avcc_len += 4 + nal_len;
  }

  if (avcc_len == 0) {
    free(annexb);
    return NULL;
  }

  avcc = malloc(avcc_len);
  if (avcc == NULL) {
    free(annexb);
    return NULL;
  }

  c = 0;
  size_t write_offset = 0;
  while (c < annexb_len) {
    const uint8_t* nal = NULL;
    size_t nal_len = 0;
    if (!nl_ios_find_next_annexb_nal(annexb, annexb_len, &c, &nal, &nal_len)) break;

    uint32_t be_len = CFSwapInt32HostToBig((uint32_t)nal_len);
    memcpy(avcc + write_offset, &be_len, 4);
    write_offset += 4;
    memcpy(avcc + write_offset, nal, nal_len);
    write_offset += nal_len;
  }

  free(annexb);
  *out_len = avcc_len;
  return avcc;
}

static CMSampleBufferRef nl_ios_create_sample_buffer(nl_ios_video_context_t* ctx, const DECODE_UNIT* du) {
  if (ctx == NULL || du == NULL || ctx->format_description == NULL) return NULL;

  size_t avcc_len = 0;
  uint8_t* avcc = nl_ios_build_avcc_sample(du, &avcc_len);
  if (avcc == NULL || avcc_len == 0) {
    free(avcc);
    return NULL;
  }

  CMBlockBufferRef block_buffer = NULL;
  OSStatus status = CMBlockBufferCreateWithMemoryBlock(
    kCFAllocatorDefault,
    NULL,
    avcc_len,
    kCFAllocatorDefault,
    NULL,
    0,
    avcc_len,
    0,
    &block_buffer
  );
  if (status != noErr || block_buffer == NULL) {
    free(avcc);
    return NULL;
  }

  status = CMBlockBufferReplaceDataBytes(avcc, block_buffer, 0, avcc_len);
  free(avcc);
  if (status != noErr) {
    CFRelease(block_buffer);
    return NULL;
  }

  CMSampleTimingInfo timing = {
    .duration = ctx->redraw_rate > 0 ? CMTimeMake(1, ctx->redraw_rate) : kCMTimeInvalid,
    .presentationTimeStamp = CMTimeMake((int64_t)du->presentationTimeUs, 1000000),
    .decodeTimeStamp = kCMTimeInvalid,
  };
  size_t sample_size = avcc_len;
  CMSampleBufferRef sample_buffer = NULL;
  status = CMSampleBufferCreateReady(
    kCFAllocatorDefault,
    block_buffer,
    ctx->format_description,
    1,
    1,
    &timing,
    1,
    &sample_size,
    &sample_buffer
  );
  CFRelease(block_buffer);
  if (status != noErr || sample_buffer == NULL) return NULL;

  CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(sample_buffer, YES);
  if (attachments != NULL && CFArrayGetCount(attachments) > 0) {
    CFMutableDictionaryRef attachment = (CFMutableDictionaryRef)CFArrayGetValueAtIndex(attachments, 0);
    if (attachment != NULL) {
      CFDictionarySetValue(attachment, kCMSampleAttachmentKey_DisplayImmediately, kCFBooleanTrue);
      if (du->frameType == FRAME_TYPE_IDR) {
        CFDictionarySetValue(attachment, kCMSampleAttachmentKey_NotSync, kCFBooleanFalse);
      }
    }
  }

  return sample_buffer;
}

static void nl_ios_recover_display_layer(nl_ios_video_context_t* ctx) {
  UIView* view = nl_ios_resolve_render_view(ctx);
  if (ctx == NULL || view == nil) return;

  if (ctx->layer != nil) {
    [ctx->layer flushAndRemoveImage];
    [ctx->layer removeFromSuperlayer];
    ctx->layer = nil;
  }

  ctx->layer = [NolandSampleDisplayLayer layer];
  ctx->layer.videoGravity = AVLayerVideoGravityResizeAspect;
  ctx->layer.backgroundColor = UIColor.blackColor.CGColor;
  ctx->layer.needsDisplayOnBoundsChange = YES;
  ctx->layer.frame = view.bounds;
  [view.layer insertSublayer:ctx->layer atIndex:0];
  nl_ios_reset_format_description(ctx);
}

static void nl_ios_drain_video_frames(nl_video_renderer_t* renderer) {
  if (renderer == NULL || renderer->platform_context == NULL) return;

  nl_ios_video_context_t* ctx = (nl_ios_video_context_t*)renderer->platform_context;
  if (ctx->layer == nil) return;

  VIDEO_FRAME_HANDLE handle;
  PDECODE_UNIT du;
  while (LiPollNextVideoFrame(&handle, &du)) {
    nl_video_frame_metadata_t meta;
    memset(&meta, 0, sizeof(meta));
    meta.frame_number = du->frameNumber;
    meta.frame_type = du->frameType;
    meta.full_length = du->fullLength;
    meta.host_processing_latency = du->frameHostProcessingLatency;
    meta.receive_time_us = du->receiveTimeUs;
    meta.enqueue_time_us = du->enqueueTimeUs;
    meta.presentation_time_us = du->presentationTimeUs;
    meta.rtp_timestamp = du->rtpTimestamp;
    meta.hdr_active = du->hdrActive ? 1 : 0;
    meta.colorspace = du->colorspace;

    int result = renderer->frame_processor != NULL
      ? renderer->frame_processor(renderer->frame_processor_user_data, du, &meta)
      : nl_video_renderer_submit_frame(renderer, du, &meta);

    LiCompleteVideoFrame(handle, result);

    if (renderer->latency_config.pacing_mode != NL_PACING_MODE_OFF &&
        ctx->redraw_rate > 0 && LiGetPendingVideoFrames() == 1) {
      break;
    }
  }
}

@implementation NolandDisplayLinkProxy
- (void)onDisplayLink:(CADisplayLink*)link {
  (void)link;
  nl_ios_drain_video_frames(self.renderer);
}
@end

void* nl_ios_stream_view_create(void) {
  NolandStreamView* view = [[NolandStreamView alloc] initWithFrame:CGRectZero];
  view.backgroundColor = UIColor.blackColor;
  view.multipleTouchEnabled = YES;
  return (__bridge_retained void*)view;
}

void noland_performance_overlay_update(void* handle, const char* text) {
  if (handle == NULL) return;
  NSString* value = text != NULL ? [NSString stringWithUTF8String:text] : @"";
  dispatch_async(dispatch_get_main_queue(), ^{
    UIView* view = (__bridge UIView*)handle;
    UILabel* label = (UILabel*)[view viewWithTag:1003];
    if (![label isKindOfClass:[UILabel class]]) return;
    NSMutableParagraphStyle* paragraph = [NSMutableParagraphStyle new];
    paragraph.minimumLineHeight = 10;
    paragraph.maximumLineHeight = 10;
    paragraph.lineSpacing = 0;
    label.attributedText = [[NSAttributedString alloc] initWithString:value attributes:@{
      NSFontAttributeName: label.font,
      NSForegroundColorAttributeName: UIColor.whiteColor,
      NSParagraphStyleAttributeName: paragraph
    }];
    label.accessibilityValue = value;
    label.hidden = value.length == 0;
    [label.superview setNeedsLayout];
  });
}

void nl_video_renderer_platform_set_overlay_text(nl_video_renderer_t* renderer, const char* text) {
  nl_ios_video_context_t* ctx = renderer != NULL ? (nl_ios_video_context_t*)renderer->platform_context : NULL;
  if (ctx != NULL && ctx->view != nil)
    noland_performance_overlay_update((__bridge void*)ctx->view, text);
}

void nl_video_renderer_platform_attach_surface(nl_video_renderer_t* renderer, const nl_surface_descriptor_t* surface) {
  nl_ios_video_context_t* ctx = nl_ios_ensure_context(renderer);
  if (ctx == NULL || surface == NULL || surface->surface_type != NL_SURFACE_IOS_UIVIEW) return;

  nl_ios_run_on_main_sync(^{
    if (ctx->owned_view != nil && surface->window_handle != NULL) {
      [ctx->owned_view removeFromSuperview];
      ctx->owned_view = nil;
    }

    ctx->view = surface->window_handle != NULL ? (__bridge UIView*)surface->window_handle : nil;
    UIView* view = nl_ios_resolve_render_view(ctx);
    if (view == nil) return;
    if ([view isKindOfClass:[NolandStreamView class]]) {
      ((NolandStreamView*)view).renderer = renderer;
    }
    if (ctx->controller_input == nil) {
      ctx->controller_input = [NolandControllerInput new];
      ctx->controller_input.renderer = renderer;
      [ctx->controller_input start];
    }
    if ([view isKindOfClass:NolandStreamView.class]) [((NolandStreamView*)view).controls refreshGamepad];
    nl_ios_start_lifecycle_observers(renderer, ctx);

    if (ctx->layer == nil) {
      ctx->layer = [NolandSampleDisplayLayer layer];
      ctx->layer.videoGravity = AVLayerVideoGravityResizeAspect;
      ctx->layer.backgroundColor = UIColor.blackColor.CGColor;
      ctx->layer.needsDisplayOnBoundsChange = YES;
      ctx->layer.frame = view.bounds;
      [view.layer insertSublayer:ctx->layer atIndex:0];
    } else if (ctx->layer.superlayer != view.layer) {
      ctx->layer.frame = view.bounds;
      [view.layer insertSublayer:ctx->layer atIndex:0];
    }
  });
}

void nl_video_renderer_platform_detach_surface(nl_video_renderer_t* renderer) {
  nl_ios_video_context_t* ctx = nl_ios_context(renderer);
  if (ctx == NULL) return;

  nl_ios_run_on_main_sync(^{
    UIView* view = nl_ios_resolve_render_view(ctx);
    if ([view isKindOfClass:[NolandStreamView class]])
      [(NolandStreamView*)view prepareForRemoval];
    if (ctx->layer != nil) {
      [ctx->layer flushAndRemoveImage];
      [ctx->layer removeFromSuperlayer];
      ctx->layer = nil;
    }
    if (ctx->owned_view != nil) {
      [ctx->owned_view removeFromSuperview];
      ctx->owned_view = nil;
    }
    [ctx->controller_input stop];
    ctx->controller_input = nil;
    nl_ios_stop_lifecycle_observers(ctx);
  });
  ctx->view = nil;
}

int nl_video_renderer_platform_setup(nl_video_renderer_t* renderer, int video_format, int width, int height, int redraw_rate) {
  nl_ios_video_context_t* ctx = nl_ios_ensure_context(renderer);
  if (ctx == NULL) return -1;

  if (ctx->video_format != video_format) {
    nl_ios_free_parameter_set(&ctx->sps, &ctx->sps_len);
    nl_ios_free_parameter_set(&ctx->pps, &ctx->pps_len);
    nl_ios_free_parameter_set(&ctx->vps, &ctx->vps_len);
  }

  ctx->video_format = video_format;
  ctx->width = width;
  ctx->height = height;
  ctx->redraw_rate = redraw_rate;
  nl_ios_reset_format_description(ctx);
  return 0;
}

void nl_video_renderer_platform_start(nl_video_renderer_t* renderer) {
  nl_ios_video_context_t* ctx = nl_ios_context(renderer);
  if (ctx == NULL) return;

  nl_ios_run_on_main_sync(^{
    UIView* view = nl_ios_resolve_render_view(ctx);
    if (view == nil) return;

    if (ctx->layer == nil) {
      ctx->layer = [NolandSampleDisplayLayer layer];
      ctx->layer.videoGravity = AVLayerVideoGravityResizeAspect;
      ctx->layer.backgroundColor = UIColor.blackColor.CGColor;
      ctx->layer.needsDisplayOnBoundsChange = YES;
      ctx->layer.frame = view.bounds;
      [view.layer insertSublayer:ctx->layer atIndex:0];
    } else {
      ctx->layer.frame = view.bounds;
    }

    if (ctx->layer != nil) [ctx->layer flushAndRemoveImage];
    [ctx->controller_input announceControllers];
    if ([view isKindOfClass:NolandStreamView.class]) [((NolandStreamView*)view).controls refreshGamepad];

    if (ctx->display_link != nil) {
      [ctx->display_link invalidate];
      ctx->display_link = nil;
    }

    ctx->display_link_proxy = [NolandDisplayLinkProxy new];
    ctx->display_link_proxy.renderer = renderer;
    ctx->display_link = [CADisplayLink displayLinkWithTarget:ctx->display_link_proxy selector:@selector(onDisplayLink:)];

    if (@available(iOS 15.0, *)) {
      float rate = ctx->redraw_rate > 0 ? (float)ctx->redraw_rate : 60.0f;
      ctx->display_link.preferredFrameRateRange = CAFrameRateRangeMake(rate, rate, rate);
    } else if (ctx->redraw_rate > 0) {
      ctx->display_link.preferredFramesPerSecond = ctx->redraw_rate;
    }

    [ctx->display_link addToRunLoop:[NSRunLoop mainRunLoop] forMode:NSRunLoopCommonModes];
  });
}

void nl_video_renderer_platform_stop(nl_video_renderer_t* renderer) {
  nl_ios_video_context_t* ctx = nl_ios_context(renderer);
  if (ctx == NULL) return;

  nl_ios_run_on_main_sync(^{
    if (ctx->display_link != nil) {
      [ctx->display_link invalidate];
      ctx->display_link = nil;
    }
    ctx->display_link_proxy = nil;
    if (ctx->layer != nil) {
      [ctx->layer flushAndRemoveImage];
    }
  });
}

void nl_video_renderer_platform_cleanup(nl_video_renderer_t* renderer) {
  nl_ios_video_context_t* ctx = nl_ios_context(renderer);
  if (ctx == NULL) return;

  nl_video_renderer_platform_stop(renderer);
  nl_video_renderer_platform_detach_surface(renderer);
  nl_ios_reset_format_description(ctx);
  nl_ios_free_parameter_set(&ctx->sps, &ctx->sps_len);
  nl_ios_free_parameter_set(&ctx->pps, &ctx->pps_len);
  nl_ios_free_parameter_set(&ctx->vps, &ctx->vps_len);
  free(ctx);
  renderer->platform_context = NULL;
}

int nl_video_renderer_platform_submit_frame(nl_video_renderer_t* renderer, const void* raw_du, const nl_video_frame_metadata_t* frame) {
  nl_ios_video_context_t* ctx = nl_ios_context(renderer);
  const DECODE_UNIT* du = (const DECODE_UNIT*)raw_du;
  (void)frame;

  if (ctx == NULL || du == NULL || ctx->layer == nil) return DR_OK;

  if (ctx->layer.status == AVQueuedSampleBufferRenderingStatusFailed) {
    nl_ios_run_on_main_sync(^{
      nl_ios_recover_display_layer(ctx);
    });
    return DR_NEED_IDR;
  }

  if (du->frameType == FRAME_TYPE_IDR) {
    nl_ios_collect_parameter_sets(ctx, du);
  }
  if (ctx->format_description == NULL) return DR_NEED_IDR;

  CMSampleBufferRef sample_buffer = nl_ios_create_sample_buffer(ctx, du);
  if (sample_buffer == NULL) return DR_OK;

  CFRetain(sample_buffer);
  __block bool failed = false;
  nl_ios_run_on_main_sync(^{
    if (ctx->layer == nil) {
      failed = true;
      CFRelease(sample_buffer);
      return;
    }

    if (ctx->layer.status == AVQueuedSampleBufferRenderingStatusFailed) {
      nl_ios_recover_display_layer(ctx);
      failed = true;
      CFRelease(sample_buffer);
      return;
    }

    [ctx->layer enqueueSampleBuffer:sample_buffer];
    CFRelease(sample_buffer);
  });
  CFRelease(sample_buffer);

  return failed ? DR_NEED_IDR : DR_OK;
}
