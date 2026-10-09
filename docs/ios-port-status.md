# iOS port implementation status

## Current checkpoint

The port now produces a signed iOS device application with its embedded packet-tunnel extension. It has not yet been installed over the reused app identity or validated against a live remote stream, so full behavior parity remains the acceptance requirement in `IOS_MOBILE_PORT_PLAN.md`.

The user selected reuse of the reference Apple identity: `noland.main.app`, signing team `U66WLT4SP6`. Installing this identity can replace the reference app. No app has been installed or uploaded during this work.

## Implemented

- Tauri library/mobile entry point, thin desktop entry point, iOS configuration, mobile capabilities, and npm commands. Desktop updater/process plugins are target-gated; iOS updates use the App Store/TestFlight mechanism.
- Xcode project generation with `npm run tauri:ios:init`. Generated project output stays ignored.
- Xcode 27 compatibility through the reference project's vendored `swift-rs` 1.0.8, including its documented Swift export fix and licenses. The Cargo lockfile selects the patch.
- Xcode 27 final linking requires `rustup component add llvm-tools`; the patched `swift-rs` uses `llvm-objcopy` from that component to restore global visibility for Tauri and plugin `@_cdecl` exports.
- Safe-area layout, touch-friendly form controls, modal bounds, zoom-enabled viewport, reduced-motion handling, and iOS guards for desktop exit/update hooks.
- Pure-Rust SSH transport in `src-tauri/crates/noland-ssh`: public-key authentication, pinned host keys, concurrent stdin/output, exit status, deadlines, remote PTYs, and streamed recursive SFTP uploads. Commands are not automatically replayed after dispatch.
- iOS `RemoteExec` adapter preserving method names, terminal metadata, output/closed events, sudo behavior, and exact upload destinations. A dedicated runtime keeps terminal connections alive independently of blocking command calls.
- SSH key generation/validation without `ssh-keygen`. iOS private keys are stored in Keychain; the existing private-key path contains an opaque reference. Legacy key files are replaced only after Keychain write/read-back validation. This does not yet implement migration from the older reference app's separate secure-reference schema.
- UIKit clipboard read/write using explicit existing clipboard actions, including Unicode, embedded NUL, the existing size limit, and main-thread dispatch.
- UIKit idle-timer control and Darwin interface MTU detection without local executables.
- A maintained Apple packet-tunnel project template and deterministic `npm run configure:ios` step. It adds the `noland.main.app.PacketTunnel` extension, WireGuardKit, shared Keychain/app-group entitlements, required usage descriptions, and the pinned WireGuard Go bridge build.
- An iOS Network Extension adapter for the current WireGuard service contract: install/start/reconnect/stop/remove, protected configuration storage, exact runtime identity/fingerprint/endpoint/MTU read-back, handshake and traffic counters, and serialized endpoint/MTU mutation messages. Mutations verify the expected fingerprint, update WireGuardKit, persist to shared Keychain, and roll the running adapter back when secure persistence fails. Before first protection Rust journals and rolls back its local configuration around those mutations; after successful activation the local file is replaced with an opaque Keychain reference and subsequent reads resolve through the protected bridge. This is the client-side foundation for both Direct and Cloudflare TURN endpoints; live transition validation remains untested.
- An in-app native stream presentation container replacing desktop window setters on iOS, with native controls, a localized Return to stream action, clipboard actions, disconnect, and live FPS/decode summaries.
- A linked native iOS media stack modeled against Moonlight iOS commit `02dc9780496eeeac6d01c8bbdccb8b6fe71ef28a`: H.264/HEVC `AVSampleBufferDisplayLayer` rendering, Annex-B conversion, IDR recovery, HDR metadata, CADisplayLink pacing, bounded Opus/AVAudioEngine playback, lifecycle/audio interruption recovery, absolute and relative touch, hardware mouse/scroll, expanded keyboard mapping, GameController input and haptics, native statistics overlay, and native microphone RTP/RTCP forwarding. See `docs/ios-streaming-reference.md`.
- iOS microphone commands now use native AVAudioSession/AudioUnit capture and Opus rather than the desktop GStreamer sidecar, while retaining the existing host-agent/session contracts, mute, bitrate update, metrics, reconnect, and cleanup flows.
- Mobile resources retain the remote-agent source trees needed by provisioning; desktop sidecar executables are excluded.
- A baseline removal guard for all 135 original registered commands and 123 exported frontend functions. This checks names, not runtime behavior or serialized shape parity.

## Verification

Passed:

- `npm run build` (TypeScript and Vite; existing large-chunk warning).
- `npm run i18n:check` (1,167 keys across nine locale bundles).
- `npm run check:ios:contracts` (135 baseline commands, 123 baseline frontend exports).
- `npm run test:ssh` (six tests with a real loopback SSH/SFTP protocol server):
  - full-duplex input/output larger than an SSH flow-control window;
  - stderr and exit status arriving after EOF;
  - deadlines without command replay;
  - changed host keys and rejected client keys;
  - PTY negotiation, terminal input, resize, and exit;
  - recursive uploads, empty directories, binary contents, and quoted/Unicode paths;
  - UTF-8 split across arbitrary packet boundaries.
- `cargo check --locked --manifest-path src-tauri/Cargo.toml -p noland-ssh --target aarch64-apple-ios`.
- `npm run check:ios` now passes for the complete Rust library on the arm64 iOS simulator target, including the Network Extension adapter boundary. It intentionally skips native-media linking and is not an app build.
- A non-skipping arm64 iOS Simulator Cargo build now compiles and links the source-built native media archive, Moonlight common C, ENet, Opus, Mbed TLS, UIKit renderer/input, AVAudioEngine output, and native microphone implementation.
- `npm run check:ios:streaming` independently configures and builds that native stack in Release mode for the arm64 iOS Simulator SDK.
- `npm run tauri:ios:build -- --debug --target aarch64 --ci --export-method debugging` completes Xcode compilation, final application linking, development signing, embedded-extension validation, and export for the configured Apple team.
- The exported development IPA contains the signed `NolandConnectMobile.app` and embedded `NolandPacketTunnel.appex`; generated archives and IPAs remain ignored and are not committed.
- Code-signature inspection confirms the main app and extension share the expected app group and Keychain group, and the extension carries the `packet-tunnel-provider` entitlement.
- The `NolandPacketTunnel` extension, WireGuardKit Swift package, and pinned WireGuard Go archive compile for arm64 iOS Simulator through the dedicated `NolandPacketTunnelOnly` scheme. The app-side VPN bridge also passes standalone Swift type-checking against that module.
- Desktop Rust library check with native media and absent bundle artifacts excluded (not a linked desktop build):

  ```sh
  NOLAND_SKIP_NATIVE_BUILD=1 \
  TAURI_CONFIG='{"bundle":{"externalBin":[],"resources":[],"macOS":{"frameworks":[]}}}' \
  cargo check --locked --manifest-path src-tauri/Cargo.toml --lib
  ```

The installed Xcode has the iOS 27 SDK but only an iOS 26.3 simulator runtime, so Tauri's simulator wrapper refuses to launch until a matching runtime is installed. The native simulator library and packet-tunnel target compile independently.

Not run: app installation (which would replace the reference app because the identity is reused), physical-device behavior tests, Keychain migration tests on-device, live SSH provisioning against a real workstation, live game streaming, or TestFlight delivery.

## Remaining work

1. Complete VPN lifecycle reconciliation after extension termination, app suspension, and network changes, then test Direct/TURN transitions, failed mutation rollback, allocation expiry, protected-reference migration, and traffic continuity on a signed physical device.
2. Implement document-picker/security-scoped access, OAuth handoff, diagnostics sharing, mobile health checks, and app lifecycle reconciliation. Remove remaining local process assumptions from iOS paths.
3. Complete migration and secure persistence for other state secrets and the older mobile-reference schema; test upgrades before installing over that app identity.
4. Exercise every feature in the plan on simulator and device, including real H.264/HEVC streams, touch/mouse/keyboard/controllers, haptics, audio route changes, microphone interruptions, and all failure/recovery cases. Add signed CI/TestFlight delivery only after those requirements pass.

No required feature is considered complete solely because its frontend control or Tauri command remains present.
