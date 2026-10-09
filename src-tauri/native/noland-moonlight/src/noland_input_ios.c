#include "noland_moonlight.h"
#include <stdatomic.h>

static atomic_bool g_capture_active = false;
static atomic_int g_capture_mode = 0;

// UIKit touch, pointer, keyboard, and GameController events are installed by
// the iOS render view. These retain the cross-platform input lifecycle ABI.
int nl_desktop_input_install(const nl_surface_descriptor_t* surface) {
  return surface != NULL && surface->surface_type == NL_SURFACE_IOS_UIVIEW ? 0 : -1;
}

void nl_desktop_input_uninstall(void) {
  atomic_store(&g_capture_active, false);
  atomic_store(&g_capture_mode, 0);
}

int nl_desktop_input_set_capture_active(bool active, int mode) {
  atomic_store(&g_capture_active, active);
  atomic_store(&g_capture_mode, active ? mode : 0);
  return 0;
}

bool nl_ios_input_capture_active(void) {
  return atomic_load(&g_capture_active);
}

int nl_ios_input_capture_mode(void) {
  return atomic_load(&g_capture_mode);
}
