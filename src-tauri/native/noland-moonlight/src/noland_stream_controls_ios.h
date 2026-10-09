#import <UIKit/UIKit.h>
#include <stdint.h>

typedef NS_ENUM(NSInteger, NolandTouchMode) {
  NolandTouchModeTrackpad = 1,
  NolandTouchModeDirect = 2,
  NolandTouchModeClickToUse = 3,
};

typedef struct {
  uint32_t buttons;
  uint8_t leftTrigger, rightTrigger;
  int16_t leftX, leftY, rightX, rightY;
} NolandVirtualGamepadState;

// The empty area passes touches through to the video. Only visible controls
// and the open drawer consume input. All callbacks execute on the main thread.
@interface NolandStreamControls : UIView <UIGestureRecognizerDelegate>
@property(nonatomic, readonly) NolandTouchMode touchMode;
@property(nonatomic, readonly) BOOL menuVisible;
@property(nonatomic, readonly) BOOL gamepadEnabled;
@property(nonatomic, copy) void (^menuChanged)(BOOL visible);
@property(nonatomic, copy) void (^modeChanged)(void);
@property(nonatomic, copy) void (^gamepadChanged)(BOOL enabled, NolandVirtualGamepadState state);
@property(nonatomic, copy) void (^keyboardRequested)(void);
@property(nonatomic, copy) void (^dashboardRequested)(void);
- (void)installGesturesOnView:(UIView*)view;
- (void)toggleMenu;
- (void)releaseControls;
- (void)refreshGamepad;
- (void)detach;
@end
