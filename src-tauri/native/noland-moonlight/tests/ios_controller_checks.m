// Exercise the production controller manager and stream view against a recording
// transport. GCController snapshots supply real GameController input profiles.
#import "../src/noland_video_renderer_ios.m"

static BOOL transportReady;
static unsigned arrivals, states;
static uint16_t lastSlot, lastMask;
static NolandVirtualGamepadState lastState;

nl_result_t nl_send_controller_arrival(nl_runtime_t* runtime, uint8_t slot, uint16_t mask,
    uint8_t type, uint32_t buttons, uint16_t capabilities) {
  if (!transportReady) return NL_RESULT_NOT_READY;
  arrivals++;
  return NL_RESULT_OK;
}
nl_result_t nl_send_controller(nl_runtime_t* runtime, uint16_t slot, uint16_t mask,
    uint32_t buttons, uint8_t lt, uint8_t rt, int16_t lx, int16_t ly, int16_t rx, int16_t ry) {
  NSCAssert(transportReady, @"State must wait for successful arrival");
  states++; lastSlot = slot; lastMask = mask;
  lastState = (NolandVirtualGamepadState){.buttons=buttons, .leftTrigger=lt, .rightTrigger=rt,
    .leftX=lx, .leftY=ly, .rightX=rx, .rightY=ry};
  return NL_RESULT_OK;
}
// Non-controller transport/video operations are outside this harness.
bool nl_ios_input_capture_active(void) { return true; }
void noland_ios_stream_dismiss(void) {}
nl_result_t nl_release_all_input(nl_runtime_t* r) { return NL_RESULT_OK; }
nl_result_t nl_send_relative_mouse(nl_runtime_t* r, int16_t x, int16_t y) { return NL_RESULT_OK; }
nl_result_t nl_send_absolute_mouse(nl_runtime_t* r, int16_t x, int16_t y, int16_t w, int16_t h) { return NL_RESULT_OK; }
nl_result_t nl_send_mouse_button(nl_runtime_t* r, uint8_t b, bool p) { return NL_RESULT_OK; }
nl_result_t nl_send_vertical_scroll(nl_runtime_t* r, int16_t a, bool h) { return NL_RESULT_OK; }
nl_result_t nl_send_horizontal_scroll(nl_runtime_t* r, int16_t a, bool h) { return NL_RESULT_OK; }
nl_result_t nl_send_keyboard(nl_runtime_t* r, uint16_t k, bool p, uint8_t m) { return NL_RESULT_OK; }
nl_result_t nl_send_utf8_text(nl_runtime_t* r, const char* t, uint32_t l) { return NL_RESULT_OK; }
int nl_video_renderer_submit_frame(nl_video_renderer_t* r, const void* d, const nl_video_frame_metadata_t* m) { return DR_OK; }
int LiSendControllerBatteryEvent(uint8_t n, uint8_t s, uint8_t p) { return 0; }
int LiSendControllerMotionEvent(uint8_t n, uint8_t t, float x, float y, float z) { return 0; }
int LiGetPendingVideoFrames(void) { return 0; }
bool LiPollNextVideoFrame(VIDEO_FRAME_HANDLE* h, PDECODE_UNIT* d) { return false; }
void LiCompleteVideoFrame(VIDEO_FRAME_HANDLE h, int s) {}
bool LiGetCurrentHostDisplayHdrMode(void) { return false; }
bool LiGetHdrMetadata(PSS_HDR_METADATA m) { return false; }
void LiRequestIdrFrame(void) {}

static void runControllerChecks(UIWindow* window) {
  nl_video_renderer_t renderer = {0};
  renderer.frame_processor_user_data = &renderer; // recording transport token
  NolandStreamView* view = [[NolandStreamView alloc] initWithFrame:window.bounds];
  [window addSubview:view];
  nl_surface_descriptor_t surface = {.surface_type=NL_SURFACE_IOS_UIVIEW,
    .window_handle=(__bridge void*)view};
  renderer.surface = surface; renderer.surface_attached = true;
  nl_video_renderer_platform_attach_surface(&renderer, &surface);
  NolandControllerInput* input = view.controllerInput;
  NSCAssert(input && view.renderer == &renderer, @"Surface must own its input manager");

  NolandVirtualGamepadState osc = {.buttons=A_FLAG, .leftX=16000, .leftTrigger=180};
  [input sendVirtualGamepad:osc enabled:YES];
  NSCAssert(states == 0 && !input.virtualAnnounced, @"Not-ready arrival must not send state");
  [input beginStream];
  transportReady = YES;
  [input.arrivalRetryTimer fire];
  NSCAssert(arrivals == 1 && states == 1 && lastSlot == 0 && lastMask == 1 && lastState.buttons == A_FLAG,
    @"OSC must work alone and retry initial arrival without a physical controller");

  GCController* physical = [GCController controllerWithExtendedGamepad];
  [input bindController:physical];
  [physical.extendedGamepad.buttonB setValue:1];
  [physical.extendedGamepad.leftThumbstick.xAxis setValue:-0.8];
  [input sendPhysicalController:physical];
  NSCAssert(lastState.buttons == (A_FLAG|B_FLAG) && lastState.leftX < -25000 && lastState.leftTrigger == 180,
    @"Physical and OSC inputs must merge without releasing either source");
  [input sendVirtualGamepad:(NolandVirtualGamepadState){0} enabled:NO];
  NSCAssert(lastMask == 1 && lastState.buttons == B_FLAG && lastState.leftX < -25000,
    @"Disabling OSC must preserve held physical input");
  [physical.extendedGamepad.buttonX setValue:1];
  physical.extendedGamepad.valueChangedHandler(physical.extendedGamepad, physical.extendedGamepad.buttonX);
  NSCAssert(lastState.buttons == (B_FLAG|X_FLAG), @"Physical callback must work with OSC off");
  [input sendVirtualGamepad:osc enabled:YES];
  [physical.extendedGamepad.buttonB setValue:0];
  [physical.extendedGamepad.buttonX setValue:0];
  [input sendPhysicalController:physical];
  NSCAssert(lastState.buttons == A_FLAG, @"Releasing physical buttons must preserve OSC input");

  [view.controls toggleMenu];
  NSCAssert(input.inputSuspended, @"Drawer suspends input");
  [view.controls toggleMenu];
  NSCAssert(!input.inputSuspended && lastMask == 1, @"Closing drawer resumes physical input even with OSC off");

  [input sendVirtualGamepad:osc enabled:YES];
  [NSNotificationCenter.defaultCenter postNotificationName:GCControllerDidDisconnectNotification object:physical];
  NSCAssert(lastMask == 1 && lastState.buttons == A_FLAG && !physical.extendedGamepad.valueChangedHandler,
    @"Unplugging physical controller must leave OSC working independently");
  [NSNotificationCenter.defaultCenter postNotificationName:GCControllerDidConnectNotification object:physical];
  NSCAssert(physical.extendedGamepad.valueChangedHandler != nil, @"Hot reconnect must reinstall callbacks");

  nl_video_renderer_platform_cleanup(&renderer);
  NSCAssert(!physical.extendedGamepad.valueChangedHandler && !view.controllerInput,
    @"Teardown removes callbacks and view ownership");
  // Match generic cleanup: descriptor survives; next setup/start must rebuild
  // the same stream surface instead of an unbound fallback view.
  nl_video_renderer_platform_setup(&renderer, VIDEO_FORMAT_H264, 1280, 720, 60);
  renderer.frame_processor_user_data = &renderer;
  nl_video_renderer_platform_start(&renderer);
  NSCAssert(view.controllerInput && view.controllerInput != input && view.renderer == &renderer,
    @"Restart must restore manager on the original surface");
  [view.controllerInput bindController:physical];
  [physical.extendedGamepad.buttonY setValue:1];
  [view.controllerInput sendPhysicalController:physical];
  NSCAssert(lastState.buttons & Y_FLAG, @"Physical controller must work in the next session");
  [view.controllerInput sendVirtualGamepad:osc enabled:YES];
  NSCAssert((lastState.buttons & (A_FLAG|Y_FLAG)) == (A_FLAG|Y_FLAG), @"OSC also works after restart");
  nl_video_renderer_platform_cleanup(&renderer);
  [view removeFromSuperview];
  NSLog(@"NOLAND_CONTROLLER_LIFECYCLE_PASS");
}
