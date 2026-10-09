#import "noland_stream_controls_ios.h"
#include "Limelight.h"
#include <math.h>

static NSString* const TouchModeKey = @"noland.stream.touchMode";
static NSString* const GamepadKey = @"noland.stream.gamepad";

@interface NolandPassthroughView : UIView
@end
@implementation NolandPassthroughView
- (UIView*)hitTest:(CGPoint)point withEvent:(UIEvent*)event {
  UIView* hit = [super hitTest:point withEvent:event];
  return hit == self ? nil : hit;
}
@end

@interface NolandTouchStick : UIControl
@property(nonatomic, strong) UIView* knob;
@property(nonatomic, copy) void (^changed)(float x, float y);
- (void)reset;
@end
@implementation NolandTouchStick
- (instancetype)initWithFrame:(CGRect)frame {
  if ((self = [super initWithFrame:frame])) {
    self.backgroundColor = [UIColor colorWithWhite:0.1 alpha:0.3];
    self.layer.borderColor = [UIColor colorWithWhite:1 alpha:0.6].CGColor;
    self.layer.borderWidth = 1.5;
    self.knob = [UIView new];
    self.knob.userInteractionEnabled = NO;
    self.knob.backgroundColor = [UIColor colorWithWhite:1 alpha:0.4];
    [self addSubview:self.knob];
    self.isAccessibilityElement = YES;
    self.accessibilityTraits = UIAccessibilityTraitAllowsDirectInteraction;
  }
  return self;
}
- (void)layoutSubviews {
  [super layoutSubviews];
  self.layer.cornerRadius = self.bounds.size.width / 2;
  CGFloat size = self.bounds.size.width * 0.4;
  self.knob.bounds = CGRectMake(0, 0, size, size);
  self.knob.layer.cornerRadius = size / 2;
  if (!self.tracking) self.knob.center = CGPointMake(CGRectGetMidX(self.bounds), CGRectGetMidY(self.bounds));
}
- (void)updateTouch:(UITouch*)touch {
  CGPoint point = [touch locationInView:self];
  CGFloat radius = MAX(1, self.bounds.size.width * 0.32);
  CGFloat x = (point.x - CGRectGetMidX(self.bounds)) / radius;
  CGFloat y = (CGRectGetMidY(self.bounds) - point.y) / radius;
  CGFloat length = hypot(x, y);
  if (length > 1) { x /= length; y /= length; }
  self.knob.center = CGPointMake(CGRectGetMidX(self.bounds) + x * radius, CGRectGetMidY(self.bounds) - y * radius);
  if (self.changed) self.changed(fabs(x) < 0.12 ? 0 : x, fabs(y) < 0.12 ? 0 : y);
}
- (BOOL)beginTracking:(UITouch*)touch withEvent:(UIEvent*)event { [self updateTouch:touch]; return YES; }
- (BOOL)continueTracking:(UITouch*)touch withEvent:(UIEvent*)event { [self updateTouch:touch]; return YES; }
- (void)endTracking:(UITouch*)touch withEvent:(UIEvent*)event { [self reset]; }
- (void)cancelTrackingWithEvent:(UIEvent*)event { [super cancelTrackingWithEvent:event]; [self reset]; }
- (void)reset {
  self.knob.center = CGPointMake(CGRectGetMidX(self.bounds), CGRectGetMidY(self.bounds));
  if (self.changed) self.changed(0, 0);
}
@end

@interface NolandStreamControls () {
  NolandVirtualGamepadState _pad;
  CGFloat _drawerProgress;
  CGFloat _panStart;
}
@property(nonatomic, readwrite) NolandTouchMode touchMode;
@property(nonatomic, readwrite) BOOL menuVisible;
@property(nonatomic, readwrite) BOOL gamepadEnabled;
@property(nonatomic, strong) UIButton* menuButton;
@property(nonatomic, strong) UIButton* scrim;
@property(nonatomic, strong) UIScrollView* drawer;
@property(nonatomic, strong) UIStackView* menuStack;
@property(nonatomic, strong) UILabel* hint;
@property(nonatomic, strong) NolandPassthroughView* padView;
@property(nonatomic, strong) NolandTouchStick* leftStick;
@property(nonatomic, strong) NolandTouchStick* rightStick;
@property(nonatomic, strong) NolandTouchStick* dpad;
@property(nonatomic, strong) NSMutableArray<UIButton*>* padButtons;
@property(nonatomic, strong) UIPanGestureRecognizer* edgePan;
@property(nonatomic, strong) UITapGestureRecognizer* threeFingerTap;
@property(nonatomic, weak) UIView* gestureView;
@end

@implementation NolandStreamControls
- (UIButton*)button:(NSString*)title action:(SEL)action {
  UIButton* button = [UIButton buttonWithType:UIButtonTypeSystem];
  [button setTitle:NSLocalizedString(title, nil) forState:UIControlStateNormal];
  button.tintColor = UIColor.whiteColor;
  button.backgroundColor = [UIColor colorWithWhite:0.15 alpha:0.8];
  button.layer.cornerRadius = 10;
  button.titleLabel.font = [UIFont systemFontOfSize:15 weight:UIFontWeightSemibold];
  [button addTarget:self action:action forControlEvents:UIControlEventTouchUpInside];
  return button;
}
- (instancetype)initWithFrame:(CGRect)frame {
  if (!(self = [super initWithFrame:frame])) return nil;
  NSInteger stored = [NSUserDefaults.standardUserDefaults integerForKey:TouchModeKey];
  self.touchMode = stored >= 1 && stored <= 3 ? stored : NolandTouchModeTrackpad;
  self.gamepadEnabled = [NSUserDefaults.standardUserDefaults boolForKey:GamepadKey];
  self.padButtons = [NSMutableArray array];
  self.padView = [NolandPassthroughView new];
  self.padView.multipleTouchEnabled = YES;
  [self addSubview:self.padView];
  __weak NolandStreamControls* weakSelf = self;
  self.leftStick = [NolandTouchStick new];
  self.leftStick.accessibilityLabel = NSLocalizedString(@"Left stick", nil);
  self.leftStick.changed = ^(float x, float y) {
    NolandStreamControls* view = weakSelf;
    if (!view) return;
    view->_pad.leftX = (int16_t)lrintf(x * 32767); view->_pad.leftY = (int16_t)lrintf(y * 32767);
    [view refreshGamepad];
  };
  self.rightStick = [NolandTouchStick new];
  self.rightStick.accessibilityLabel = NSLocalizedString(@"Right stick", nil);
  self.rightStick.changed = ^(float x, float y) {
    NolandStreamControls* view = weakSelf;
    if (!view) return;
    view->_pad.rightX = (int16_t)lrintf(x * 32767); view->_pad.rightY = (int16_t)lrintf(y * 32767);
    [view refreshGamepad];
  };
  self.dpad = [NolandTouchStick new];
  self.dpad.accessibilityLabel = NSLocalizedString(@"Directional pad", nil);
  self.dpad.changed = ^(float x, float y) {
    NolandStreamControls* view = weakSelf;
    if (!view) return;
    view->_pad.buttons &= ~(UP_FLAG | DOWN_FLAG | LEFT_FLAG | RIGHT_FLAG);
    if (y > 0.35) view->_pad.buttons |= UP_FLAG;
    if (y < -0.35) view->_pad.buttons |= DOWN_FLAG;
    if (x > 0.35) view->_pad.buttons |= RIGHT_FLAG;
    if (x < -0.35) view->_pad.buttons |= LEFT_FLAG;
    [view refreshGamepad];
  };
  for (UIView* stick in @[self.leftStick, self.rightStick, self.dpad]) [self.padView addSubview:stick];
  NSArray* titles = @[@"A", @"B", @"X", @"Y", @"LB", @"RB", @"LT", @"RT", @"L3", @"R3", @"Select", @"Start"];
  uint32_t flags[] = {A_FLAG, B_FLAG, X_FLAG, Y_FLAG, LB_FLAG, RB_FLAG, 0, 0, LS_CLK_FLAG, RS_CLK_FLAG, BACK_FLAG, PLAY_FLAG};
  for (NSUInteger i = 0; i < titles.count; i++) {
    UIButton* button = [self button:titles[i] action:@selector(ignoreTap)];
    button.exclusiveTouch = NO;
    button.tag = flags[i];
    button.accessibilityLabel = titles[i];
    button.accessibilityIdentifier = [NSString stringWithFormat:@"stream.pad.%@", titles[i]];
    [button addTarget:self action:@selector(padDown:) forControlEvents:UIControlEventTouchDown | UIControlEventTouchDragEnter];
    [button addTarget:self action:@selector(padUp:) forControlEvents:UIControlEventTouchUpInside | UIControlEventTouchUpOutside | UIControlEventTouchCancel | UIControlEventTouchDragExit];
    [self.padButtons addObject:button];
    [self.padView addSubview:button];
  }
  self.scrim = [UIButton new];
  self.scrim.backgroundColor = [UIColor colorWithWhite:0 alpha:0.5];
  self.scrim.accessibilityLabel = NSLocalizedString(@"Close stream menu", nil);
  [self.scrim addTarget:self action:@selector(closeMenu) forControlEvents:UIControlEventTouchUpInside];
  [self addSubview:self.scrim];
  self.drawer = [UIScrollView new];
  self.drawer.backgroundColor = [UIColor colorWithRed:0.055 green:0.075 blue:0.12 alpha:0.98];
  self.drawer.accessibilityViewIsModal = YES;
  [self addSubview:self.drawer];
  UIPanGestureRecognizer* closePan = [[UIPanGestureRecognizer alloc] initWithTarget:self action:@selector(dragDrawer:)];
  closePan.delegate = self;
  [self.drawer addGestureRecognizer:closePan];
  self.menuStack = [UIStackView new];
  self.menuStack.axis = UILayoutConstraintAxisVertical;
  self.menuStack.spacing = 12;
  [self.drawer addSubview:self.menuStack];
  UILabel* title = [UILabel new];
  title.text = NSLocalizedString(@"Stream controls", nil);
  title.textColor = UIColor.whiteColor;
  title.font = [UIFont systemFontOfSize:23 weight:UIFontWeightBold];
  [self.menuStack addArrangedSubview:title];
  for (NSUInteger i = 0; i < 3; i++) {
    UIButton* mode = [self button:@[@"Trackpad", @"Direct touch", @"Click-to-use"][i] action:@selector(selectMode:)];
    mode.tag = i + 1;
    [mode.heightAnchor constraintEqualToConstant:44].active = YES;
    [self.menuStack addArrangedSubview:mode];
  }
  self.hint = [UILabel new];
  self.hint.textColor = [UIColor colorWithWhite:0.8 alpha:1];
  self.hint.font = [UIFont systemFontOfSize:13];
  self.hint.numberOfLines = 0;
  [self.menuStack addArrangedSubview:self.hint];
  UIStackView* gamepadRow = [UIStackView new];
  gamepadRow.spacing = 8;
  UILabel* label = [UILabel new]; label.text = NSLocalizedString(@"On-screen controller", nil); label.textColor = UIColor.whiteColor;
  label.font = [UIFont systemFontOfSize:14]; label.numberOfLines = 0;
  UISwitch* gamepadSwitch = [UISwitch new]; gamepadSwitch.on = self.gamepadEnabled;
  gamepadSwitch.accessibilityLabel = label.text;
  [gamepadSwitch addTarget:self action:@selector(toggleGamepad:) forControlEvents:UIControlEventValueChanged];
  [gamepadRow addArrangedSubview:label]; [gamepadRow addArrangedSubview:gamepadSwitch];
  [self.menuStack addArrangedSubview:gamepadRow];
  for (UIButton* button in @[[self button:@"Keyboard" action:@selector(keyboard)],
                              [self button:@"Return to Noland" action:@selector(dashboard)],
                              [self button:@"Resume stream" action:@selector(closeMenu)]]) {
    [button.heightAnchor constraintEqualToConstant:44].active = YES;
    [self.menuStack addArrangedSubview:button];
  }
  self.menuButton = [self button:@"☰" action:@selector(toggleMenu)];
  self.menuButton.accessibilityLabel = NSLocalizedString(@"Open stream menu", nil);
  self.menuButton.accessibilityHint = NSLocalizedString(@"Also opens with three fingers or a swipe from the left edge", nil);
  [self addSubview:self.menuButton];
  [self updateModeLabels];
  [self setDrawerProgress:0];
  return self;
}
- (void)ignoreTap {}
- (UIView*)hitTest:(CGPoint)point withEvent:(UIEvent*)event {
  if (self.hidden || !self.userInteractionEnabled || self.alpha < 0.01 || ![self pointInside:point withEvent:event]) return nil;
  // Establish priority explicitly instead of allowing transparent layers or
  // the text/video responder to acquire a touch intended for a gamepad button.
  NSArray<UIView*>* targets = self.menuVisible ? @[self.drawer, self.scrim]
      : self.gamepadEnabled ? @[self.menuButton, self.padView] : @[self.menuButton];
  for (UIView* target in targets) {
    UIView* hit = [target hitTest:[self convertPoint:point toView:target] withEvent:event];
    if (hit) return hit;
  }
  return nil;
}
- (void)installGesturesOnView:(UIView*)view {
  self.gestureView = view;
  // Accept the left safe-area strip too, so a swipe need not begin on the exact
  // physical edge (which competes with system gestures/notch exclusion).
  self.edgePan = [[UIPanGestureRecognizer alloc] initWithTarget:self action:@selector(dragDrawer:)];
  self.edgePan.maximumNumberOfTouches = 1;
  self.edgePan.delegate = self;
  self.edgePan.delaysTouchesBegan = YES;
  self.edgePan.allowedTouchTypes = @[@(UITouchTypeDirect)];
  [view addGestureRecognizer:self.edgePan];
  self.threeFingerTap = [[UITapGestureRecognizer alloc] initWithTarget:self action:@selector(toggleMenu)];
  self.threeFingerTap.numberOfTouchesRequired = 3;
  self.threeFingerTap.delegate = self;
  self.threeFingerTap.delaysTouchesBegan = YES;
  self.threeFingerTap.allowedTouchTypes = @[@(UITouchTypeDirect)];
  [view addGestureRecognizer:self.threeFingerTap];
}
- (BOOL)gestureRecognizer:(UIGestureRecognizer*)gesture shouldReceiveTouch:(UITouch*)touch {
  // Gamepad and menu touches never participate in stream menu gestures.
  if (gesture == self.edgePan || gesture == self.threeFingerTap) {
    if ([touch.view isDescendantOfView:self]) return NO;
    if (gesture == self.edgePan)
      return !self.menuVisible && [touch locationInView:self].x <= MAX(44, self.safeAreaInsets.left + 24);
    return YES;
  }
  for (UIView* view = touch.view; view && view != self.drawer; view = view.superview)
    if ([view isKindOfClass:UIControl.class]) return NO;
  return YES;
}
- (BOOL)gestureRecognizerShouldBegin:(UIGestureRecognizer*)gesture {
  if ([gesture isKindOfClass:UIPanGestureRecognizer.class]) {
    CGPoint velocity = [(UIPanGestureRecognizer*)gesture velocityInView:self];
    if (gesture == self.edgePan) return velocity.x > fabs(velocity.y);
    return fabs(velocity.x) > fabs(velocity.y);
  }
  return YES;
}
- (void)setDrawerProgress:(CGFloat)progress {
  _drawerProgress = MIN(1, MAX(0, progress));
  CGFloat width = MIN(340, self.bounds.size.width * 0.8);
  self.drawer.frame = CGRectMake(-width * (1 - _drawerProgress), 0, width, self.bounds.size.height);
  self.scrim.alpha = _drawerProgress;
  self.scrim.hidden = !self.menuVisible;
  self.drawer.hidden = !self.menuVisible;
  self.menuButton.hidden = self.menuVisible;
  self.padView.hidden = !self.gamepadEnabled || self.menuVisible;
}
- (void)setMenuOpen:(BOOL)open animated:(BOOL)animated {
  [self releaseControls];
  self.menuVisible = open;
  if (self.menuChanged) self.menuChanged(open);
  // Keep views visible through the closing animation to avoid a jump.
  self.drawer.hidden = NO; self.scrim.hidden = NO;
  CGFloat width = MIN(340, self.bounds.size.width * 0.8);
  _drawerProgress = open ? 1 : 0;
  [UIView animateWithDuration:animated && !UIAccessibilityIsReduceMotionEnabled() ? 0.2 : 0 animations:^{
    self.drawer.frame = CGRectMake(open ? 0 : -width, 0, width, self.bounds.size.height);
    self.scrim.alpha = open ? 1 : 0;
  } completion:^(BOOL finished) {
    if (!finished) return;
    [self setDrawerProgress:self->_drawerProgress];
    UIAccessibilityPostNotification(UIAccessibilityScreenChangedNotification, open ? self.menuStack : self.menuButton);
  }];
}
- (void)toggleMenu { [self setMenuOpen:!self.menuVisible animated:YES]; }
- (void)closeMenu { [self setMenuOpen:NO animated:YES]; }
- (void)dragDrawer:(UIPanGestureRecognizer*)gesture {
  CGFloat width = MIN(340, self.bounds.size.width * 0.8);
  if (gesture.state == UIGestureRecognizerStateBegan) {
    _panStart = self.menuVisible ? 1 : 0;
    [self releaseControls]; self.menuVisible = YES;
    if (self.menuChanged) self.menuChanged(YES);
  }
  if (gesture.state == UIGestureRecognizerStateBegan || gesture.state == UIGestureRecognizerStateChanged)
    [self setDrawerProgress:_panStart + [gesture translationInView:self].x / MAX(width, 1)];
  if (gesture.state == UIGestureRecognizerStateEnded) {
    CGFloat speed = [gesture velocityInView:self].x;
    [self setMenuOpen:fabs(speed) > 300 ? speed > 0 : _drawerProgress > 0.4 animated:YES];
  } else if (gesture.state == UIGestureRecognizerStateCancelled) {
    [self setMenuOpen:_panStart > 0 animated:YES];
  }
}
- (void)selectMode:(UIButton*)button {
  [self releaseControls];
  self.touchMode = button.tag;
  [NSUserDefaults.standardUserDefaults setInteger:self.touchMode forKey:TouchModeKey];
  [self updateModeLabels];
  if (self.modeChanged) self.modeChanged();
}
- (void)updateModeLabels {
  for (UIView* view in self.menuStack.arrangedSubviews) {
    if ([view isKindOfClass:UIButton.class] && view.tag >= 1 && view.tag <= 3) {
      view.backgroundColor = view.tag == self.touchMode
          ? [UIColor colorWithRed:0.08 green:0.36 blue:0.5 alpha:1] : [UIColor colorWithWhite:0.15 alpha:0.8];
      view.accessibilityTraits = UIAccessibilityTraitButton | (view.tag == self.touchMode ? UIAccessibilityTraitSelected : 0);
    }
  }
  self.hint.text = NSLocalizedString(self.touchMode == NolandTouchModeTrackpad
    ? @"Slide to move the cursor. Tap to click, hold to drag. Two fingers scroll or right-click."
    : self.touchMode == NolandTouchModeDirect
    ? @"Touch to press at that position, then move to drag. Hold still to right-click."
    : @"Tap to move the cursor there and click. Slide to position without clicking; hold to drag. Two-finger tap right-clicks.", nil);
  [self setNeedsLayout];
}
- (void)toggleGamepad:(UISwitch*)sender {
  [self releaseControls]; self.gamepadEnabled = sender.on;
  [NSUserDefaults.standardUserDefaults setBool:sender.on forKey:GamepadKey];
  [self refreshGamepad]; [self setNeedsLayout];
}
- (void)padDown:(UIButton*)sender { [self updateButton:sender pressed:YES]; }
- (void)padUp:(UIButton*)sender { [self updateButton:sender pressed:NO]; }
- (void)updateButton:(UIButton*)button pressed:(BOOL)pressed {
  if ([button.currentTitle isEqualToString:@"LT"]) _pad.leftTrigger = pressed ? 255 : 0;
  else if ([button.currentTitle isEqualToString:@"RT"]) _pad.rightTrigger = pressed ? 255 : 0;
  else if (pressed) _pad.buttons |= (uint32_t)button.tag;
  else _pad.buttons &= ~(uint32_t)button.tag;
  button.alpha = pressed ? 1 : 0.65;
  [self refreshGamepad];
}
- (void)refreshGamepad { if (self.gamepadChanged) self.gamepadChanged(self.gamepadEnabled, _pad); }
- (void)releaseControls {
  _pad = (NolandVirtualGamepadState){0};
  for (UIButton* button in self.padButtons) { [button cancelTrackingWithEvent:nil]; button.highlighted = NO; button.alpha = 0.65; }
  for (NolandTouchStick* stick in @[self.leftStick, self.rightStick, self.dpad]) [stick cancelTrackingWithEvent:nil];
  [self refreshGamepad];
}
- (void)keyboard { [self closeMenu]; if (self.keyboardRequested) self.keyboardRequested(); }
- (void)dashboard { [self closeMenu]; if (self.dashboardRequested) self.dashboardRequested(); }
- (void)layoutSubviews {
  [super layoutSubviews];
  UIEdgeInsets safe = self.safeAreaInsets;
  self.scrim.frame = self.bounds; self.padView.frame = self.bounds;
  self.menuButton.frame = CGRectMake(safe.left + 8, safe.top + 8, 44, 44);
  [self setDrawerProgress:_drawerProgress];
  CGFloat inset = MAX(16, safe.left);
  CGFloat contentWidth = self.drawer.bounds.size.width - inset - 16;
  CGSize size = [self.menuStack systemLayoutSizeFittingSize:CGSizeMake(contentWidth, 0)
      withHorizontalFittingPriority:UILayoutPriorityRequired verticalFittingPriority:UILayoutPriorityFittingSizeLevel];
  self.menuStack.frame = CGRectMake(inset, safe.top + 16, contentWidth, size.height);
  self.drawer.contentSize = CGSizeMake(self.drawer.bounds.size.width, size.height + safe.top + safe.bottom + 32);
  CGFloat left = safe.left + 12, right = self.bounds.size.width - safe.right - 12;
  CGFloat bottom = self.bounds.size.height - safe.bottom - 12;
  CGFloat stickSize = MIN(108, MAX(80, (bottom - safe.top) * 0.28));
  CGFloat face = 46, gap = 48;
  self.leftStick.frame = CGRectMake(left, bottom - stickSize, stickSize, stickSize);
  self.rightStick.frame = CGRectMake(right - stickSize - 145, bottom - stickSize, stickSize, stickSize);
  self.dpad.frame = CGRectMake(left + stickSize + 12, bottom - 94, 94, 94);
  CGPoint center = CGPointMake(right - 70, bottom - 74);
  CGPoint offsets[] = {{0,gap}, {gap,0}, {-gap,0}, {0,-gap}};
  for (NSUInteger i = 0; i < 4; i++) self.padButtons[i].frame = CGRectMake(center.x + offsets[i].x - face/2, center.y + offsets[i].y - face/2, face, face);
  CGFloat top = MAX(safe.top + 62, bottom - stickSize - 64);
  self.padButtons[4].frame = CGRectMake(left, top, 54, 44);
  self.padButtons[6].frame = CGRectMake(left + 60, top, 54, 44);
  self.padButtons[5].frame = CGRectMake(right - 54, top, 54, 44);
  self.padButtons[7].frame = CGRectMake(right - 114, top, 54, 44);
  self.padButtons[8].frame = CGRectMake(left + stickSize + 12, top, 44, 44);
  self.padButtons[9].frame = CGRectMake(right - stickSize - 145, top, 44, 44);
  CGFloat middle = (left + right) / 2;
  self.padButtons[10].frame = CGRectMake(middle - 66, safe.top + 12, 60, 44);
  self.padButtons[11].frame = CGRectMake(middle + 6, safe.top + 12, 60, 44);
}
- (void)detach {
  [self releaseControls];
  [self.gestureView removeGestureRecognizer:self.edgePan];
  [self.gestureView removeGestureRecognizer:self.threeFingerTap];
  self.gestureView = nil;
}
@end
