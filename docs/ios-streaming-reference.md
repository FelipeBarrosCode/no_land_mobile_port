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
