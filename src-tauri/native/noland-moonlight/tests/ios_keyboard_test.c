#include "noland_keyboard_ios.h"
#include <assert.h>
#include <stdio.h>

int main(void) {
  bool shift;
  // Regression: typing 'r' must use R's virtual key, never the host's
  // Ctrl+Shift+U Unicode composition shortcut.
  assert(nl_ios_ascii_key('r', &shift) == 0x52 && !shift);
  assert(nl_ios_ascii_key('R', &shift) == 0x52 && shift);
  assert(nl_ios_ascii_key('u', &shift) == 0x55 && !shift);
  assert(nl_ios_ascii_key('@', &shift) == 0x32 && shift);
  assert(nl_ios_ascii_key('"', &shift) == 0xDE && shift);
  assert(nl_ios_ascii_key('\'', &shift) == 0xDE && !shift);
  assert(nl_ios_ascii_key('\n', &shift) == 0x0D && !shift);
  assert(nl_ios_ascii_key('\t', &shift) == 0x09 && !shift);
  for (int ch = 32; ch < 127; ch++) assert(nl_ios_ascii_key(ch, &shift) != 0);
  assert(nl_ios_ascii_key(0xC3, &shift) == 0); // UTF-8 remains a text event
  puts("iOS ASCII keyboard mapping passed");
}
