// UIKit integration checks for the real overlay, run inside iOS Simulator.
#import <UIKit/UIKit.h>
#import "noland_stream_controls_ios.h"
#include "Limelight.h"

@interface NolandStreamControls (Harness)
- (void)requestKeyboard;
@end

static UIView* findLabel(UIView* root, NSString* label) {
  if ([root.accessibilityLabel isEqualToString:label]) return root;
  for (UIView* child in root.subviews) {
    UIView* result = findLabel(child, label);
    if (result) return result;
  }
  return nil;
}

@interface RoutingTouch : UITouch
@property(nonatomic, strong) UIView* target;
@property(nonatomic, assign) CGPoint point;
@end
@implementation RoutingTouch
- (UIView*)view { return self.target; }
- (CGPoint)locationInView:(UIView*)view { return self.point; }
@end

@interface HarnessController : UIViewController
@end
@implementation HarnessController
- (void)viewDidAppear:(BOOL)animated {
  [super viewDidAppear:animated];
  [UIView setAnimationsEnabled:NO];
  self.view.backgroundColor = UIColor.darkGrayColor;
  [NSUserDefaults.standardUserDefaults setBool:NO forKey:@"noland.stream.gamepad"];
  NolandStreamControls* controls = [[NolandStreamControls alloc] initWithFrame:self.view.bounds];
  [self.view addSubview:controls];
  [controls installGesturesOnView:self.view];
  [controls layoutIfNeeded];
  __block NolandVirtualGamepadState state = {0};
  __block NSUInteger keyboardRequests = 0;
  controls.gamepadChanged = ^(BOOL enabled, NolandVirtualGamepadState value) { state = value; };
  controls.keyboardRequested = ^{ keyboardRequests++; };

  UIButton* a = (UIButton*)findLabel(controls, @"A");
  UIButton* b = (UIButton*)findLabel(controls, @"B");
  UISwitch* toggle = (UISwitch*)findLabel(controls, @"On-screen controller");
  // findLabel can match the label before the switch; find the switch directly.
  NSMutableArray* queue = [NSMutableArray arrayWithObject:controls];
  while (queue.count) {
    UIView* next = queue.lastObject; [queue removeLastObject];
    if ([next isKindOfClass:UISwitch.class]) { toggle = (UISwitch*)next; break; }
    [queue addObjectsFromArray:next.subviews];
  }
  NSCAssert(a && b && [toggle isKindOfClass:UISwitch.class], @"Controller widgets must exist");
  CGPoint aPoint = [a convertPoint:CGPointMake(CGRectGetMidX(a.bounds), CGRectGetMidY(a.bounds)) toView:self.view];
  NSCAssert([self.view hitTest:aPoint withEvent:nil] == self.view, @"Hidden controller must pass through");
  [controls toggleMenu];
  NSCAssert(controls.menuVisible, @"Menu must open");
  toggle.on = YES;
  [toggle sendActionsForControlEvents:UIControlEventValueChanged];
  [controls toggleMenu];
  [controls layoutIfNeeded];
  NSCAssert(controls.gamepadEnabled && !controls.menuVisible, @"Controller must enable independently of the drawer");
  aPoint = [a convertPoint:CGPointMake(CGRectGetMidX(a.bounds), CGRectGetMidY(a.bounds)) toView:self.view];
  NSCAssert([self.view hitTest:aPoint withEvent:nil] == a, @"Visible A button must own its touch");
  [a sendActionsForControlEvents:UIControlEventTouchDown];
  [b sendActionsForControlEvents:UIControlEventTouchDown];
  NSCAssert((state.buttons & (A_FLAG | B_FLAG)) == (A_FLAG | B_FLAG), @"Multiple buttons must be independently held");
  [a sendActionsForControlEvents:UIControlEventTouchUpInside];
  NSCAssert(!(state.buttons & A_FLAG) && (state.buttons & B_FLAG), @"Releasing A must preserve B");
  RoutingTouch* touch = [RoutingTouch new]; touch.target = a; touch.point = CGPointMake(10, 100);
  for (UIGestureRecognizer* gesture in self.view.gestureRecognizers) {
    NSCAssert(![controls gestureRecognizer:gesture shouldReceiveTouch:touch], @"Menu gestures must not steal gamepad touches");
  }
  NSCAssert(keyboardRequests == 0, @"Gamepad input must never request the keyboard");
  [controls requestKeyboard];
  NSCAssert(keyboardRequests == 1, @"Three-finger keyboard action must request the keyboard without opening the drawer");
  NSCAssert(!controls.menuVisible, @"Keyboard action must not open the drawer");
  [controls toggleMenu];
  NSCAssert(state.buttons == 0 && state.leftTrigger == 0 && state.leftX == 0, @"Opening drawer releases held controls");
  UIView* hit = [self.view hitTest:aPoint withEvent:nil];
  NSCAssert(hit != a && hit != self.view, @"Open menu must intercept game touches");
  [controls detach];
  NSCAssert(self.view.gestureRecognizers.count == 0, @"Detach removes stream gestures");
  NSLog(@"NOLAND_UI_ROUTING_PASS");
  exit(0);
}
@end

@interface HarnessDelegate : UIResponder <UIApplicationDelegate>
@property(nonatomic, strong) UIWindow* window;
@end
@implementation HarnessDelegate
- (BOOL)application:(UIApplication*)app didFinishLaunchingWithOptions:(NSDictionary*)options {
  self.window = [[UIWindow alloc] initWithFrame:UIScreen.mainScreen.bounds];
  self.window.rootViewController = [HarnessController new];
  [self.window makeKeyAndVisible];
  return YES;
}
@end
int main(int argc, char** argv) {
  @autoreleasepool { return UIApplicationMain(argc, argv, nil, NSStringFromClass(HarnessDelegate.class)); }
}
