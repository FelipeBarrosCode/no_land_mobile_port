#ifndef NOLAND_KEYBOARD_IOS_H
#define NOLAND_KEYBOARD_IOS_H
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

// US virtual-key mapping used by Moonlight's normal keyboard protocol. Native
// Unicode text injection is reserved for text outside the ASCII mapping.
static inline uint16_t nl_ios_ascii_key(uint8_t ch, bool* shift) {
  *shift = false;
  if (ch >= 'a' && ch <= 'z') return ch - 'a' + 0x41;
  if (ch >= 'A' && ch <= 'Z') { *shift = true; return ch; }
  if (ch >= '0' && ch <= '9') return ch;
  if (ch == '\n' || ch == '\r') return 0x0D;
  if (ch == '\t') return 0x09;
  const char* plain = " `-=[]\\;',./";
  const char* upper = " ~_+{}|:\"<>?";
  const uint16_t keys[] = {0x20,0xC0,0xBD,0xBB,0xDB,0xDD,0xDC,0xBA,0xDE,0xBC,0xBE,0xBF};
  for (size_t j = 0; plain[j]; j++) {
    if (ch == plain[j]) return keys[j];
    if (ch == upper[j]) { *shift = true; return keys[j]; }
  }
  const char* symbols = ")!@#$%^&*(";
  for (size_t j = 0; symbols[j]; j++) {
    if (ch == symbols[j]) { *shift = true; return 0x30 + j; }
  }
  return 0;
}
#endif
