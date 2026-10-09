# iOS streaming implementation reference

The iOS media implementation was reconciled against the GPL-3.0 Moonlight iOS repository requested for this port:

- Repository: <https://github.com/moonlight-stream/moonlight-ios>
- Reviewed commit: `02dc9780496eeeac6d01c8bbdccb8b6fe71ef28a`
- Review date: 2026-10-08

The reference informed these platform behaviors while No Land retains its own Rust/Tauri contracts, stream settings, telemetry, connection manager, and UI:

- `VideoDecoderRenderer.m`: pull-frame draining on `CADisplayLink`, H.264/HEVC parameter-set handling, Annex-B to length-prefixed sample conversion, `AVSampleBufferDisplayLayer`, decoder recovery, HDR metadata, and IDR requests after lifecycle interruptions.
- `StreamView.m`, `RelativeTouchHandler.m`, `AbsoluteTouchHandler.m`, and `KeyboardSupport.m`: aspect-correct pointer coordinates, relative/absolute touch modes, scrolling, mouse buttons, full hardware-key mapping, and input release when focus is lost.
- `ControllerSupport.m` and `HapticContext.m`: GameController discovery, stable player slots, arrival/state reports, mouse events, LED output, and localized controller haptics.
- `Connection.m`: bounded Opus decode/playback queues and the separation between Moonlight audio playback and connection callbacks.

No Land uses `AVAudioEngine` instead of Moonlight iOS's SDL playback path, because desktop SDL is not shipped in the iOS app. Opus 1.5.2 and Mbed TLS 3.6.4 are source-built from pinned upstream commits by CMake. AV1 remains unavailable because the current No Land client capability contract explicitly reports `supports_av1: false`; H.264 and HEVC are the negotiated iOS formats. This avoids advertising Moonlight iOS's FFmpeg-assisted AV1 path without shipping and validating that dependency.

Moonlight iOS is GPL-3.0, compatible with this repository's GPL-3.0-only license. This document pins the reviewed upstream revision so future updates can be reconciled explicitly.

## Native stream controls (2026-10-09)

`noland_stream_controls_ios.m` implements No Land's requested in-stream drawer.
Swipe in from the left edge, tap with three fingers on the video, or use the
44-point menu button. The drawer can be dragged closed or dismissed via its
scrim/Resume action. It scrolls within safe areas on smaller displays.

- **Trackpad:** relative movement, tap left-click, hold then drag, two-finger
  scroll/right-click.
- **Direct touch:** absolute position with press-on-touch and dragging; hold still
  for right-click.
- **Click-to-use:** absolute positioning without a held button while moving;
  tapping clicks at the tapped video position, holding begins a drag. Coordinates
  respect the rendered video's aspect ratio and letterboxing.
- **On-screen controller:** two analog sticks, D-pad (including diagonals),
  ABXY, shoulders, digital LT/RT, L3/R3, Select/Start. Uses the same Moonlight
  controller arrival/state API as physical controllers and allocates a free
  controller slot. Unoccupied screen regions continue to accept pointer input.
- **Keyboard / Return to Noland:** available within the drawer. The keyboard has
  a Done toolbar and hides the overlay while visible. ASCII characters use
  virtual key pairs; non-ASCII text uses UTF-8. Hardware keys handled by Moonlight
  are not also delivered to UIKit's text insertion path.
- **Performance overlay:** top-right safe-area positioning with 9-point monospaced
  text, 10-point lines, abbreviated metrics, and a text-sized translucent background
  capped at 245 points wide. Full underlying statistics remain unchanged.

Menu gestures cancel video touches without generating clicks. Pointer timers,
held keys, sticks, and buttons are released on cancellation, focus loss, dismissal
and stream teardown. Delayed click releases use the owning live view/runtime,
not a global event that could reach a later session.

The upstream iOS reference uses three-finger input for the keyboard. The drawer
gesture here is the user's requested No Land adaptation, not a claim that the
pinned upstream version has an identical menu.

Verified: native iOS streaming compile, signed iPhone build/export, and the
standalone `tests/ios_keyboard_test.c` regression (all printable ASCII, letter
case/modifiers, punctuation, Return/Tab, and non-ASCII fallback). Gestures,
multi-touch gamepad behavior and keyboard results still need live stream testing
on the installed device build.
