# No Land iOS Mobile Port Plan

## 1. Purpose

This document defines the implementation plan for porting the current No Land desktop application in this repository to iOS.

The port will preserve the current product workflows, API contracts, state model, and visual identity while adapting the interface and platform integrations for iPhone and iPad. The existing project at `/Users/felipebarros/Code/no_land_mobile` is the reference for iOS environment setup, Tauri mobile commands, Apple project configuration, native bridges, signing, and deployment. It is not the source of truth for current product behavior because this repository contains newer desktop features and interfaces.

**Completion requirement: full behavioral parity with the original application.** Every existing user workflow must have a working iOS implementation. Intermediate milestones are development checkpoints, not a reduced-feature definition of the finished port. A hidden control, unsupported-command error, mock response, or compiling shell does not satisfy parity.

Implementation is in progress; see [the implementation status](docs/ios-port-status.md) for verified work and remaining blockers. The desktop baseline is the source snapshot in commit `b49160a` of this repository. Compare subsequent changes to that baseline and explicitly reconcile upstream changes before claiming parity with a newer original version.

Behavioral parity means preserving user outcomes, remote effects, options, progress, failure handling, and recovery. Platform mechanisms may differ: an SSH library replaces a local `ssh` process, an in-app stream view replaces a separate desktop window, and App Store/TestFlight replaces desktop self-updating. If iOS cannot reproduce an outcome, record it as a blocking parity gap with evidence and a proposed equivalent; do not silently remove the feature or mark it complete. Scope reductions require an explicit user decision.

## 2. Goals

1. Deliver an iOS application using Tauri 2, React, TypeScript, Rust, and the existing No Land design system.
2. Port the current user-facing interfaces to touch-friendly, responsive iPhone and iPad layouts.
3. Keep TypeScript/Rust request and response contracts aligned rather than creating an unrelated mobile API.
4. Preserve every existing cloud orchestration and remote-management workflow through iOS-compatible implementations.
5. Replace desktop-only integrations with iOS-native equivalents where required.
6. Adapt the environment variables, build commands, Apple signing model, and CI patterns from `no_land_mobile`, validating them against the current dependencies.
7. Keep the desktop-origin implementation understandable by isolating platform-specific code behind explicit interfaces.
8. Port direct WireGuard and Cloudflare TURN, terminal, clipboard, uploads, shared storage, streaming, microphone, and all advanced settings as mandatory parts of the final deliverable.
9. Preserve existing algorithms, remote protocols, settings defaults, and rollout gates; change only what the mobile platform requires.

## 3. Non-goals

- Android support in this phase.
- A full visual redesign or a separate product identity.
- Shipping desktop-only sidecars, shell tools, PTYs, or installers inside iOS.
- Blindly copying the older `no_land_mobile` frontend or backend over newer code.
- Committing generated Xcode build output, archives, `.ipa` files, `.xcappdata`, local provisioning profiles, certificates, API keys, or developer-specific Xcode state.
- Modifying or deleting either `FelipeBarrosCode/no_land` or the local `no_land_mobile` reference project.

## 4. Source-of-truth policy

| Concern | Source of truth |
| --- | --- |
| Current screens and user workflows | `no_land_mobile_port/src/` |
| Current backend behavior and commands | `no_land_mobile_port/src-tauri/src/` |
| Current shared and remote agents | `network-*`, `state-agent`, `vm-cloud-mic-agent` in this repository |
| iOS Tauri setup and npm commands | `no_land_mobile/package.json` |
| Mobile Tauri configuration | `no_land_mobile/src-tauri/tauri.mobile.conf.json` |
| Tauri mobile Rust entry point | `no_land_mobile/src-tauri/src/lib.rs` and thin `main.rs` |
| Apple bridge patterns | `no_land_mobile/src-tauri/gen/apple/Sources/` |
| Network Extension pattern | `no_land_mobile/src-tauri/gen/apple/Sources/NolandPacketTunnel/` |
| Apple permissions and entitlements | `no_land_mobile/src-tauri/gen/apple/project.yml` |
| iOS CI and signing variables | `no_land_mobile/.github/workflows/` and `docs/mobile-store-cicd.md` |

When behavior differs, retain the newer behavior from this repository and port only the platform technique from the reference.

Do not replace current connection, storage, or streaming implementations wholesale with the older reference. In particular, a direct-only WireGuardKit integration cannot replace the current direct/Cloudflare TURN connection manager. Features found only in the reference are implementation examples, not automatic additions to the product scope. Shared Linux agents remain remote programs; adapt their client interfaces without moving their execution onto iOS or redesigning their protocols.

## 5. Current-state findings

### 5.1 Frontend

The current app has mobile-relevant screens for:

- onboarding and tutorial;
- dashboard and rented instances;
- offer/server selection;
- provisioning and post-WireGuard setup;
- launch library;
- embedded Moonlight stream controls;
- display, microphone, networking, storage, shutdown, and notification settings;
- shared-storage import/export/sync;
- remote terminal and file upload.

The frontend already uses responsive Tailwind breakpoints and dynamic viewport units in several places, but it remains desktop-oriented in navigation density, modal size, hover behavior, multi-column cards, terminal interaction, window management, and streaming controls.

### 5.2 Frontend-to-backend interface

`src/lib/backend.ts` is the TypeScript facade over approximately 135 Tauri commands. `src/lib/types.ts` mirrors the Rust models and `src/store/appStore.ts` coordinates long-running workflows and event subscriptions.

The port must preserve this layering:

```text
React screen
  -> Zustand action
  -> src/lib/backend.ts typed function
  -> Tauri invoke/event
  -> Rust command
  -> service/platform adapter
```

No screen should call ad hoc Swift, HTTP, or Tauri commands directly when a typed backend wrapper can be used.

### 5.3 Rust/Tauri structure

The current Tauri startup and command registration live in `src-tauri/src/main.rs`. Tauri mobile requires a library entry point decorated with:

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() { /* builder */ }
```

The reference project demonstrates the required structure: a thin binary `main.rs` calling `noland_connect::run()` and the application builder in `lib.rs`.

### 5.4 Desktop-only dependencies and behavior

The following require isolation or replacement before an iOS build can succeed:

- `portable-pty` and the local terminal implementation;
- `arboard` desktop clipboard integration;
- `ctrlc` process signal handling;
- desktop `keyring` feature selection;
- local `std::process::Command` calls for `ssh`, `wg`, `wg-quick`, WireGuard apps, shell scripts, sleep inhibition, and system inspection;
- packaged executables and sidecars (`noland-net-helper`, `noland-mic-sender`);
- updater and process plugins intended for desktop restart/exit flows;
- separate desktop stream windows and native mouse capture;
- SDL/GStreamer/macOS-specific native build assumptions;
- desktop file paths and unrestricted filesystem access;
- Windows/macOS/Linux setup checks presented as mobile requirements.

The remote machine may still execute Linux commands through a pure-Rust SSH implementation. The restriction is on launching local desktop executables from the iOS app.

### 5.5 Useful iOS implementations in the reference project

The reference contains patterns that should be ported and reconciled with current contracts:

- `#[cfg(target_os = "ios")]` Rust platform adapter functions;
- Swift-to-Rust C ABI bridge;
- secure storage through the Tauri secure keystore and Apple Keychain;
- Network Extension packet tunnel with `WireGuardKit`;
- security-scoped document access;
- native OAuth handoff and callback URL scheme;
- iOS share sheet for diagnostics;
- `AVAudioSession`, microphone permission, stream lifecycle, and keep-awake handling;
- iOS native Moonlight surface integration;
- application groups and shared Keychain access groups;
- local-network and Bonjour declarations;
- game controller declarations;
- simulator and signed-device CI flows.

## 6. Target architecture

### 6.1 One product contract, explicit platform adapters

Keep shared domain logic and serialized contracts platform-neutral. Put operating-system behavior behind narrow interfaces.

Recommended boundaries:

```text
src-tauri/src/
  lib.rs                         # mobile entry point and command registration
  platform/
    mod.rs                       # platform traits and shared DTOs
    desktop.rs                   # existing desktop implementations
    ios.rs                       # Rust side of Apple bridge
  services/
    ...                          # shared orchestration/domain behavior

src-tauri/apple/                 # source-controlled Apple customization templates
  Sources/
  NolandPacketTunnel/
  NolandTunnelShared/
  project.patch.yml or scripts/
```

Generated `src-tauri/gen/apple/` content should remain ignored. If Tauri regeneration overwrites Apple customizations, maintain deterministic source templates and a script that reapplies them after `tauri ios init`.

### 6.2 Platform capability model

Add a typed capability response so the UI does not infer support from the user agent:

```ts
interface ClientCapabilities {
  platform: "ios" | "desktop";
  embeddedStreaming: boolean;
  managedVpn: boolean;
  remoteTerminal: boolean;
  filePicker: boolean;
  nativeShare: boolean;
  selfUpdate: boolean;
  nativeClipboard: boolean;
}
```

Use capabilities to select the appropriate platform implementation and explain runtime conditions such as denied permissions or unavailable hardware. During development, incomplete features may return explicit typed errors, but must remain open parity tasks. Capability flags cannot be used to declare required features out of scope. In the completed app, every baseline workflow must be reachable through its iOS equivalent.

### 6.3 State and secrets

- Keep non-secret state in the Tauri app data directory.
- Move Vast keys, SSH credentials, OAuth refresh tokens, tunnel material, and recovery keys to secure storage.
- Persist opaque references in app state rather than secret values.
- Perform state migration transactionally and preserve rollback behavior.
- Define an import/export interchange format that never exports secrets unless the user explicitly chooses an encrypted recovery operation.
- Test upgrade from the current desktop-origin state schema and from the older mobile reference schema.

### 6.4 Networking

- Continue using `reqwest` and pure-Rust SSH/network logic where iOS permits it.
- Replace local `wg`/`wg-quick` commands and privileged helper operations with a Network Extension packet tunnel, retaining the current connection-manager contract.
- Store tunnel configuration in a shared Keychain access group and expose only a stable reference to Rust/state.
- Use an app group shared by the main app and packet-tunnel extension.
- Reconcile VPN state after foreground resume, network changes, extension termination, and app updates.
- Declare local-network and Bonjour usage only when embedded streaming/discovery needs them.

#### Direct and Cloudflare TURN parity

Sources: `docs/networking/connection/README.md`, `docs/networking/connection/transitions.md`, `network-contracts/`, `src-tauri/src/services/connection_manager.rs`, and `src-tauri/src/services/cloudflare_turn.rs`.

- Preserve one logical WireGuard tunnel with host target `10.77.0.1`; `direct` and `cloudflare_turn` are alternative outer transports, not separate VPN products.
- Preserve Cloudflare settings read/save/test/clear, secure token read-back, short-lived credential generation, expiry, and host-owned TURN allocation/bridge behavior. The host receives temporary credentials, never the long-lived Cloudflare token.
- Provide an iOS tunnel adapter equivalent to the desktop GotaTun/helper operations: runtime identity/status, peer endpoint and MTU mutation/read-back, traffic/handshake observation, and rollback. Validate WireGuardKit support before selecting it; implement any missing bridge operations explicitly.
- Preserve preference versus active transport, manual Direct/TURN selection, Auto selection, repair, transition events, and diagnostic metrics. A saved preference must never be displayed as an active validated transport.
- Preserve transition order: validate target, apply endpoint, validate tunnel, commit, or roll back. Commit requires runtime identity/read-back, fresh traffic or handshake, host reachability, host control RPC, and Sunshine validation.
- Preserve serialization of mutations, allocation-before-transition lock ordering, client/host revisions, transition IDs, allocation generations, host profile leases, and recovery from interrupted commits.
- Keep direct connectivity sufficient for provisioning; TURN failures remain warnings when direct works. Preserve existing feature-gate defaults and the `gaming-v1` evaluator's sampling, three-win threshold, and 90-second dwell policy.
- Keep active-allocation maintenance distinct from optional quality-driven switching, including existing pinned-stream mutation rules. Define which work runs in the extension and which is reconciled by the app after resume; do not assume JavaScript or app-process timers run while suspended.
- Test both transports, failed switches and rollback, expired credentials/allocations, unavailable relay, Wi-Fi/cellular changes, extension restart, and stream continuity/recovery on a physical device.

### 6.5 Streaming

Treat streaming as a dedicated workstream rather than assuming the desktop native library will compile unchanged.

- Port or adopt the reference iOS `UIView`/Metal surface attachment.
- Compile `moonlight-common-c`, ENet, Opus, and required crypto dependencies for device and simulator architectures.
- Replace SDL/macOS renderer and input code with UIKit, Metal/MetalKit, GameController, and iOS audio implementations.
- Route touch, keyboard, and game-controller input through the existing Moonlight command model.
- Use one full-screen in-app stream view; do not create a desktop-style second Tauri window.
- Coordinate `AVAudioSession`, microphone forwarding, interruptions, route changes, backgrounding, and idle timer state.
- Define reconnect behavior for Wi-Fi/cellular transitions and managed-tunnel changes.

## 7. Feature disposition matrix

| Feature | iOS disposition | Required work |
| --- | --- | --- |
| Onboarding/tutorial | Port | Mobile form layout, keyboard avoidance, secure credential storage, external-link handling |
| Vast account/API | Reuse | Keep typed HTTP client; verify ATS/TLS and lifecycle cancellation |
| Location | Adapt | Prefer Core Location bridge with consent; retain IP/manual fallback |
| Offer search and selection | Port | Touch cards, compact filters, bottom-sheet details |
| Instance create/pause/destroy | Reuse | Confirm idempotency and foreground-resume reconciliation |
| Provisioning | Reuse/adapt | Keep remote SSH orchestration; remove local executable assumptions and support app suspension |
| Dashboard | Port | Mobile navigation, single-column hierarchy, pull-to-refresh or explicit refresh |
| Notifications | Adapt | Request iOS permission contextually; verify foreground/background behavior |
| WireGuard | Replace platform adapter | Network Extension with equivalent endpoint/MTU control, runtime identity, shared Keychain/app group, and tunnel recovery |
| Cloudflare TURN and transport selection | Mandatory port | Preserve credentials, allocation lifecycle, Direct/TURN/Auto preferences, validation, rollback, maintenance, metrics, and rollout gates |
| Storage provider authentication | Adapt | Preserve current provider profiles, static credentials, OAuth, connection tests, activation, and disconnect; use native callback handling |
| Moonlight pairing | Port | Reuse command contracts; use iOS-native stream surface and lifecycle |
| Embedded streaming | Native port | Metal/UIKit renderer, GameController/touch input, AVAudioSession |
| Microphone forwarding | Native port | iOS permission, capture, interruption recovery, mute/status APIs |
| Clipboard sync | Mandatory port | Preserve both directions with `UIPasteboard`/native paste UI and explicit user action |
| Remote terminal | Mandatory replacement | Direct SSH remote PTY with open/write/resize/close, output/exit events, touch keyboard, and reconnect handling |
| File upload | Adapt | UIDocumentPicker/security-scoped access; stream selected files without assuming permanent paths |
| Shared storage | Adapt | Security-scoped files, secure provider credentials, background/suspension-safe operation state |
| Auto shutdown | Reuse | Remote agent feature; remove desktop-local service assumptions |
| Display controls | Reuse | Remote settings UI with mobile presets and validation |
| Sunshine, latency, and stream preferences | Reuse/adapt | Preserve configuration/read-back/reset, EDID, host latency, codec/quality options, and supported device-specific choices |
| Artwork and IGDB integration | Reuse | Preserve credential settings, artwork lookup/cache, and launch-job progress/errors |
| Network/performance overlays | Port | Preserve metrics, warning thresholds, preferences, and visibility controls on the mobile stream surface |
| Localization, tutorials, sound, and help | Port | Preserve all existing locale choices and assistance flows with touch-accessible controls |
| Diagnostics export | Adapt | Native share sheet with redaction |
| Application updates | iOS equivalent | App Store/TestFlight delivery, version/help UI, and state preservation across upgrade |
| Exit/relaunch workflow | iOS equivalent | Explicit disconnect/cleanup and state restoration; iOS owns process termination |
| Sleep prevention | Replace | `UIApplication.isIdleTimerDisabled` while streaming only |
| Multi-window stream | Replace presentation | Single in-app full-screen route with equivalent start/stop, controls, and return-to-dashboard behavior |

Every row is required for final parity. Hardware-dependent options must preserve their meaning and expose accurate device support; do not advertise unsupported codecs or input devices as working.

## 8. UI and interaction migration

### 8.1 Global shell

1. Add safe-area CSS variables and apply `env(safe-area-inset-*)` to top-level screens, overlays, and stream controls.
2. Use `100dvh`, not fixed desktop heights, and test Safari/WebView keyboard viewport resizing.
3. Replace desktop side navigation with a compact tab bar or top-level mobile navigation.
4. Preserve deep-linkable app routes so state restoration can return to dashboard, provisioning, settings, or streaming.
5. Support iPhone portrait for management flows and landscape for streaming; support both orientations on iPad.
6. Add a visible offline/reconnecting state that survives app foreground/background transitions.

### 8.2 Touch and accessibility rules

- Minimum interactive target: 44 by 44 points.
- No feature may depend on hover, right-click, or mouse capture.
- Keep focus-visible support for keyboard/iPad users.
- Every icon-only control needs an accessible label.
- Respect reduced motion and avoid relying on CRT/glitch animation to communicate state.
- Verify Dynamic Type behavior or define an accessible bounded scaling policy.
- Maintain color contrast when neon colors appear on glass/CRT surfaces.
- Use haptic feedback sparingly for destructive confirmations, pairing completion, and critical failures.

### 8.3 Screen-by-screen plan

#### Onboarding

- Convert the centered desktop card into a scroll-safe mobile form.
- Keep validation next to fields and move primary action above the keyboard when needed.
- Open legal/help links through the iOS-safe opener.
- Ask for notification, local network, location, and microphone permissions only when the related feature is used.

#### Dashboard

- Prioritize active instance, connection status, and the primary Play/Resume action.
- Collapse secondary health details into expandable sections.
- Replace dense two/four-column data blocks with horizontal metric rows or one/two-column cards.
- Move destructive instance actions into a confirmation action sheet.

#### Server picker

- Use a full-screen mobile route or sheet rather than a wide desktop modal.
- Put essential filters first; move advanced filters into a collapsible sheet.
- Keep pricing, GPU, VRAM, reliability, region, and storage readable without horizontal scrolling.
- Keep selection sticky at the bottom with safe-area padding.

#### Provisioning

- Present the current stage and user-required action before logs.
- Collapse logs by default and allow copy/share through native actions.
- Persist operation identifiers and reconcile status after the app resumes.
- Never imply cancellation succeeded until the backend confirms whether a remote side effect occurred.

#### Settings

- Replace the desktop left rail with grouped navigation or nested routes.
- Keep connection, credentials, streaming, storage, notifications, and diagnostics as separate sections.
- Replace desktop updater, process, binary-path, and local-tool settings with their iOS equivalents while preserving the workflows they support.
- Preserve every current remote configuration section, including Cloudflare TURN, transport preferences/repair, shared-storage providers, IGDB, automatic backup/shutdown, and host latency settings.
- Clearly label settings that configure the remote Linux host rather than the phone.

#### Launch library and stream

- Use two-column cards on larger phones/tablets and one column on compact widths.
- Enter a dedicated full-screen landscape-friendly stream route.
- Auto-hide stream controls but retain a discoverable gesture/button to restore them.
- Provide touch/controller input mode, keyboard invocation, microphone toggle, network quality, and disconnect controls.
- Handle the home indicator and notches without covering controls.

#### Terminal and file transfer

- Keep the xterm interface and implement remote PTY allocation over library-based SSH, preserving session IDs, output/exit events, write, resize, and close semantics.
- Add touch selection, copy/paste, software-keyboard resize, and an extra-key row for Escape, Tab, arrows, and Ctrl combinations. Verify hardware-keyboard input too.
- Use the native document picker for upload; do not expose desktop filesystem paths.
- Preserve multiple-file/directory upload behavior through document/directory selection and security-scoped access, remote folder browsing, and error/progress reporting. Prove provider-specific folder access; a picker that handles only one file is not full upload parity.
- Show transfer progress and a resumable/unknown-outcome state when iOS suspends the app.
- Validate bidirectional remote clipboard sync from both terminal/stream contexts wherever the baseline exposes it.

## 9. API and contract migration

### 9.1 Build a command inventory first

Create `docs/ios-command-matrix.md` generated or checked from:

- functions exported by `src/lib/backend.ts`;
- commands in `tauri::generate_handler!`;
- Rust functions marked `#[tauri::command]`;
- event names consumed with `listen(...)`;
- TypeScript DTOs and Rust serialized models.

For each command record:

- feature owner;
- request and response type;
- side effects;
- whether it invokes a local process/filesystem/native window;
- whether it is safe on iOS;
- replacement adapter;
- expected error codes;
- resume/idempotency behavior;
- test status.

Track all 135 baseline registered commands, including input/debug/health commands and functions called outside `src/lib/backend.ts`. Inventory backend producers and frontend consumers for events, along with polling, persistence, defaults, and UI-only behaviors. For every baseline command, assign an iOS implementation and acceptance scenario; if several commands map to one mobile adapter, document each mapping explicitly.

Use statuses `not_started`, `in_progress`, `implemented_unverified`, `verified`, and `blocked`. Start each item as `not_started`. Mark `verified` only with simulator/device evidence appropriate to that behavior. A removed command needs a documented iOS equivalent for the same outcome, not just a removed caller. Every blocked or unverified mandatory item prevents final parity sign-off.

Fail CI if a frontend call references an unregistered command, a baseline command disappears without an explicit equivalent mapping, or a product command/event has no disposition. Registration checks establish coverage only; behavioral tests establish parity.

### 9.2 Contract rules

- Keep `camelCase` serialization consistent across Rust and TypeScript.
- Prefer additive schema changes with serde defaults.
- Use stable error codes plus user-readable messages.
- Attach operation IDs to long-running mutations.
- Model `queued`, `running`, `waiting_user`, `suspended`, `reconciling`, `outcome_unknown`, `succeeded`, `failed`, and `cancelled` explicitly.
- Re-subscribe to events and fetch authoritative state whenever the app returns to foreground.
- Do not rely on an event being delivered while iOS suspends the WebView.

### 9.3 Transport decision

The production iOS app should use in-process Tauri commands for local orchestration. The optional HTTP adapter may remain for frontend-only development, but it must not become a second divergent product API.

If HTTP development mode is retained:

- keep command-to-route mapping generated or tested against the same DTOs;
- bind only to loopback by default;
- never expose secrets in logs;
- document `VITE_NOLAND_API_BASE` as development-only.

## 10. Rust and library migration

### 10.1 Restructure startup

1. Move modules and `tauri::Builder` setup from `src-tauri/src/main.rs` to `src-tauri/src/lib.rs`.
2. Add `#[cfg_attr(mobile, tauri::mobile_entry_point)]` to `run()`.
3. Reduce `main.rs` to `noland_connect::run()`.
4. Keep navigation policy tests and startup diagnostics in library modules.
5. Register plugins conditionally: updater/process only on desktop; mobile-safe plugins on iOS.

### 10.2 Cargo feature and target cleanup

Use target-specific modules/dependencies and introduce explicit features only where needed, such as:

- `desktop-runtime`;
- `ios-runtime`;
- `embedded-streaming`;
- `host-control-plane` for desktop testing if still useful.

Move dependencies into target-specific sections where possible. At minimum audit:

- `portable-pty` -> desktop only;
- `arboard` -> desktop only;
- `ctrlc` -> desktop/CLI only;
- `keyring` -> replace or configure Apple-safe secure storage;
- `raw-window-handle` -> only where required by renderer integration;
- process/updater plugins -> desktop only;
- GStreamer/SDL dependencies -> remove from iOS path or replace with Apple frameworks;
- sidecar preparation -> desktop packaging only.

Run `cargo check` for both `aarch64-apple-ios` and `aarch64-apple-ios-sim` early, before UI migration is considered complete.

### 10.3 Build script

Refactor `src-tauri/build.rs` so target detection has a dedicated iOS branch. The current desktop flow prepares GStreamer, sidecars, CMake libraries, macOS Objective-C files, and desktop link libraries before it has an iOS implementation.

The iOS branch must:

- avoid desktop sidecar and GStreamer packaging;
- compile only iOS-compatible Moonlight sources;
- link UIKit, Metal, MetalKit, QuartzCore, AVFoundation, AudioToolbox, GameController, Security, WebKit, and NetworkExtension as required;
- build device and simulator architecture variants;
- use pinned native dependency revisions;
- emit bindings deterministically;
- fail with a focused message when Xcode/native prerequisites are missing.

### 10.4 Native dependency pinning

Adapt the reference `native:bootstrap` approach for newly fetched dependencies. Preserve the existing vendored Moonlight/ENet sources and local changes; replacing them with upstream revisions could discard current behavior. Record:

- repository URL;
- exact commit;
- license;
- patches applied;
- supported Apple architectures;
- update procedure and smoke tests.

Do not commit nested dependency `.git` directories or unreviewed binary outputs.

## 11. Apple platform integration

### 11.1 App identity decision gate

Before generating the final Xcode project, decide whether this port replaces the existing `noland.main.app` App Store identity.

- If it replaces the reference mobile app, reuse the registered bundle ID and compatible signing assets.
- If it is a separate app, choose a new reverse-DNS identifier and create matching App ID, extension ID, app group, Keychain group, provisioning profiles, and App Store Connect record.

Never partially rename identifiers: main app, packet-tunnel extension, URL scheme, app group, Keychain group, provisioning profile, and CI variables must remain consistent.

### 11.2 Minimum iOS configuration

Use the reference as the baseline:

- iOS deployment target: 15.0 unless dependency testing requires a higher version;
- arm64 device support and arm64 simulator support;
- portrait management UI plus landscape streaming;
- local-network usage description;
- Bonjour `_nvstream._tcp` when discovery is enabled;
- microphone usage description;
- app URL scheme for OAuth callbacks;
- game-controller declarations;
- non-exempt encryption declaration reviewed against actual cryptography/export use;
- packet-tunnel Network Extension entitlement;
- shared app group and Keychain access group;
- privacy manifest updated for all used required-reason APIs.

### 11.3 Generated project policy

Do not commit:

- `src-tauri/gen/apple/build/`;
- `src-tauri/gen/apple/Externals/`;
- `.xcarchive`, `.ipa`, `.xcuserstate`, `xcuserdata`;
- `*.mobileprovision`, `.p12`, or `.p8` credentials;
- local `.xcappdata` containers.

Commit source templates and deterministic patch/config scripts. CI must be able to initialize a clean checkout and reproduce the project without local Xcode state.

## 12. Environment and commands

### 12.1 Required local toolchain

- macOS with a supported Xcode version and accepted license;
- Xcode command-line tools;
- Node 22.12+ within the Node 22 LTS line, matching the current repository's Node 22 CI and Vite 7 requirements; pin npm consistently with the lockfile. The reference's older Node 20.x declaration must not override current dependency requirements;
- Rust toolchain pinned by `rust-toolchain.toml`;
- Rust targets `aarch64-apple-ios`, `aarch64-apple-ios-sim`, and optionally `x86_64-apple-ios` only if still needed;
- CocoaPods if required by the generated project/native packages;
- XcodeGen if the selected deterministic Apple project workflow uses `project.yml`.

### 12.2 Planned npm commands

Add iOS-only commands first; do not make Android part of the default mobile command:

```json
{
  "native:bootstrap": "bash scripts/bootstrap-native-deps.sh",
  "tauri:ios:init": "tauri ios init --ci --config src-tauri/tauri.mobile.conf.json",
  "tauri:ios:dev": "tauri ios dev --config src-tauri/tauri.mobile.conf.json",
  "tauri:ios:build": "tauri ios build --config src-tauri/tauri.mobile.conf.json",
  "tauri:dev": "npm run tauri:ios:dev",
  "tauri:build": "npm run tauri:ios:build"
}
```

Keep separate desktop commands if desktop builds remain supported in this repository, for example `tauri:desktop:dev` and `tauri:desktop:build`.

### 12.3 Runtime/configuration variables

Retain and document current shared configuration where it is meaningful on iOS:

- `NOLAND_TEMPLATE_HASH`
- `NOLAND_MIN_RELIABILITY`
- `NOLAND_OFFERS_SEARCH_LIMIT`
- `NOLAND_VAST_BASE_URL`
- `NOLAND_INSTANCE_SSH_USER`
- `NOLAND_AUDIO_TARGET_USER`
- `NOLAND_AUDIO_PROFILE`
- `NOLAND_AUDIO_FORCE_SINK_OVERRIDE`
- `NOLAND_AUDIO_SINK_OVERRIDE`
- `NOLAND_SUNSHINE_BIND_ADDRESS`
- `NOLAND_SUNSHINE_CSRF_ALLOWED_ORIGINS`
- `NOLAND_WIREGUARD_CLIENT_LISTEN_PORT`
- `NOLAND_WIREGUARD_QOS_MODE`
- `NOLAND_WIREGUARD_QOS_BANDWIDTH_MBIT`
- `NOLAND_WIREGUARD_QOS_DIFFSERV`
- `NOLAND_WIREGUARD_DSCP_ENABLED`

Development/build variables:

- `VITE_NOLAND_API_BASE` for frontend-only HTTP development;
- `NOLAND_SOURCE_REVISION` embedded by CI for diagnostics;
- `RUST_LOG` and `RUST_BACKTRACE` for development schemes;
- `APPLE_DEVELOPMENT_TEAM` or `IOS_DEVELOPMENT_TEAM` for generated project signing configuration;
- native build variables only where the iOS build script explicitly supports them.

Environment variables are not an appropriate production secret store on iOS. User credentials must be entered through the app or securely provisioned and stored in Keychain-backed storage.

Also preserve `NOLAND_ENABLE_VERIFIED_TURN_SWITCHING` and `NOLAND_ENABLE_AUTOMATIC_TRANSPORT_SELECTION` from the current connection manager. Keep their existing defaults and eligibility semantics. Shell exports on the Mac are not automatically runtime variables inside an installed iOS app: distinguish build-time values, development scheme variables, and shipped runtime settings. Where needed, inject non-secret build configuration explicitly or use persisted settings, and verify the effective values on device. Keep Cloudflare credentials in the existing settings/secure-store workflow, not in `VITE_*` variables.

### 12.4 GitHub iOS secrets and variables

Following the reference CI naming:

Secrets:

- `IOS_DISTRIBUTION_CERT_BASE64`
- `IOS_DISTRIBUTION_CERT_PASSWORD`
- `IOS_PROVISIONING_PROFILE_BASE64`
- `APP_STORE_CONNECT_ISSUER_ID`
- `APP_STORE_CONNECT_KEY_ID`
- `APP_STORE_CONNECT_API_KEY_P8`

Variables:

- `IOS_DEVELOPMENT_TEAM`
- final app bundle identifier if workflows allow an override;
- packet-tunnel bundle identifier and app group only if they are not derived from one canonical identifier.

Use GitHub Environments such as `development-store` and `production-store`, with approval protection for production. Never print secret values or decoded provisioning content.

## 13. CI/CD plan

### Pull-request validation

On a pinned macOS runner:

1. install Node and Rust Apple targets;
2. run `npm ci`;
3. run TypeScript/frontend build and unit tests;
4. run shared Rust/domain tests;
5. bootstrap pinned native dependencies;
6. run host-side control-plane tests where supported;
7. initialize the iOS scaffold from a clean checkout;
8. apply deterministic Apple project customization;
9. build an unsigned iOS simulator app;
10. run command-contract and generated-project cleanliness checks;
11. upload build logs on failure, without credentials.

### TestFlight deployment

1. validate required environment secrets before expensive build steps;
2. create an ephemeral Keychain;
3. import the distribution certificate;
4. install the matching provisioning profiles for the app and packet-tunnel extension;
5. build/archive with a monotonically increasing `CFBundleVersion`;
6. export an `.ipa` with the correct export options;
7. upload through the App Store Connect API key;
8. retain the `.ipa` as a restricted CI artifact for debugging;
9. clean the temporary Keychain and decoded files.

App Store production release remains a manual approval step until TestFlight quality and compliance are stable.

## 14. Implementation phases

### Phase 0 — Baseline and decisions

- Confirm final bundle identity and whether the existing App Store identity is reused.
- Pin Node, npm, Rust, Xcode runner, and iOS deployment target.
- Add an architecture decision record for Tauri mobile + native Apple bridges.
- Generate the command/event/platform compatibility matrix.
- Capture baseline desktop screenshots and critical flow tests.

**Exit criteria:** every current command and user-facing feature has an iOS disposition.

All phases below are incremental implementation milestones. Full completion requires every mandatory parity row to be verified; finishing the shell or control-plane milestones is not a finished mobile port.

### Phase 1 — Compile an empty mobile shell

- Split `main.rs`/`lib.rs` and add the mobile entry point.
- Add `tauri.mobile.conf.json` and iOS npm commands.
- Add target-specific Cargo features/dependencies.
- Refactor `build.rs` to avoid all desktop-only work on iOS.
- Initialize a clean iOS scaffold and launch the basic WebView in the simulator.

**Exit criteria:** clean checkout builds and launches on an iOS simulator without desktop sidecars.

### Phase 2 — Shared API/control plane

- Port shared models, state store, Vast client, pure-Rust SSH, offer selection, and remote orchestration.
- Add secure storage and migrate credentials to opaque references.
- Add client capability reporting and typed unsupported-operation errors.
- Add operation reconciliation after foreground resume.

**Exit criteria:** onboarding, offer search, instance lifecycle, and provisioning APIs work without streaming or managed VPN.

### Phase 3 — Mobile interface

- Implement safe areas, mobile shell/navigation, touch sizing, and accessibility.
- Port onboarding, dashboard, server picker, provisioning, settings, and launch library in that order.
- Wire every baseline action to its planned mobile route/adapter, including terminal, files, clipboard, Cloudflare TURN, and streaming. Track incomplete adapters as open work rather than removing their workflows.
- Test compact iPhone, large iPhone, and iPad widths.

**Exit criteria:** all control-plane flows are usable with touch and no horizontal overflow.

### Phase 4 — Apple platform services

- Integrate Keychain-backed secure store.
- Add native OAuth callback handling.
- Add document picker/security-scoped access.
- Add native diagnostics share sheet.
- Add notification permission and lifecycle handling.
- Add Core Location only if required after IP/manual fallback evaluation.

**Exit criteria:** secrets, OAuth, files, notifications, and diagnostics behave correctly on a physical device.

### Phase 5 — Managed WireGuard and Cloudflare TURN

- Adapt the reference Network Extension structure; validate that the chosen WireGuard backend can implement the current tunnel-control contract before pinning it.
- Add shared app/Keychain groups and packet-tunnel entitlement.
- Implement install/start/status/reconnect/stop/remove and recovery states.
- Reconcile tunnel state on launch and resume.
- Test install, upgrade, revoked permission, duplicate profile, network switch, and extension crash paths.
- Port Cloudflare settings, credential lifecycle, host allocation/bridge interoperability, and connection-manager endpoint/MTU transactions through the iOS tunnel adapter.
- Preserve Direct/TURN/Auto selection, repair, evaluation policy, rollout switches, diagnostics, and verified commit/rollback behavior.
- Test TURN allocation renewal and failed transitions, including app suspension and extension recovery, with the current remote network agent.

**Exit criteria:** direct and Cloudflare TURN both establish the same logical tunnel on a signed device; manual/eligible automatic switching, validation, renewal, rollback, and recovery pass parity scenarios. A direct-only tunnel does not complete this phase.

### Phase 6 — Native streaming and input

- Build native dependencies for device and simulator.
- Attach the iOS native video surface and Metal renderer.
- Integrate playback audio, microphone forwarding, touch, software keyboard, and GameController.
- Add stream lifecycle, keep-awake, interruption, and reconnect handling.
- Port statistics/network warning overlays to mobile-safe controls.

**Exit criteria:** pair, launch, interact, reconnect, and disconnect on physical iPhone/iPad with measured stability.

### Phase 7 — Storage, terminal, and clipboard parity

- Port security-scoped file upload and shared-storage flows.
- Validate behavior across suspension and unknown remote outcomes.
- Implement direct-SSH remote terminal with remote PTY allocation, input/output, resize, exit, cleanup, and mobile/hardware keyboard support.
- Implement both directions of remote clipboard sync.
- Preserve provider profiles, static credentials, OAuth, selected-object backup/restore, cancellation, schedules, progress, auto-shutdown coordination, and recovery semantics.
- Verify every existing import/export operation. Any new recovery format must be additive and must not replace current storage workflows.

**Exit criteria:** all baseline storage, file-transfer, terminal, and clipboard scenarios pass on device; none remain deferred or hidden.

### Phase 8 — Release hardening

- Add simulator CI and signed TestFlight workflow.
- Complete privacy manifest, permission strings, export compliance, licenses, and App Store privacy answers.
- Run accessibility, battery, thermal, memory, network transition, and long-session tests.
- Perform state migration and upgrade tests from prior TestFlight builds.
- Complete beta feedback loop and release checklist.

**Exit criteria:** TestFlight build passes the acceptance criteria below and has no high-severity security or data-loss issue.

## 15. Testing strategy

### Automated

- TypeScript compile and frontend unit tests.
- Contract tests for Rust/TypeScript DTO fixtures.
- Command registration/wrapper parity test.
- Rust domain/service unit tests on macOS host.
- `cargo check` for iOS device and simulator targets.
- Simulator build from a clean checkout.
- Reducer/store tests for resume, reconnect, and unknown-outcome states.
- Snapshot or component tests at representative mobile widths.
- Run shared fixtures against desktop and iOS adapters and compare requests, responses, events, state transitions, and remote side effects. Document intentional platform presentation differences.
- Test direct/TURN transport validation, transaction failure/rollback, allocation renewal, and evaluator policy against existing network-contract fixtures.

### Physical-device matrix

- compact iPhone in portrait;
- modern notched/Dynamic Island iPhone;
- iPhone landscape during streaming;
- iPad in portrait and landscape;
- Wi-Fi only, Wi-Fi to cellular transition, temporary offline mode;
- Bluetooth and wired/controller input where supported;
- microphone allowed, denied, revoked, and interrupted;
- VPN permission allowed, denied, profile removed, and extension restarted;
- low-memory/background/foreground transitions;
- TestFlight install and upgrade over a previous build.

### Critical end-to-end flow

1. Fresh install.
2. Complete onboarding and securely save credentials.
3. Find/select an offer or reuse an instance.
4. Start provisioning and background/foreground the app.
5. Recover authoritative operation state.
6. Install/connect the managed tunnel.
7. Pair with Sunshine/Moonlight.
8. Launch a remote application.
9. Stream with touch/controller and optional microphone.
10. Disconnect safely.
11. Pause or destroy the instance with explicit confirmation.
12. Export redacted diagnostics through the share sheet.

Repeat connection/streaming steps over both direct and Cloudflare TURN paths, including manual switching, eligible Auto switching, failed-switch rollback, and allocation renewal. Also validate remote terminal input/output/resize/exit; clipboard send/receive; multi-file/folder upload and folder browsing; provider OAuth/static credentials; selected backup/restore and cancellation; backup schedules and auto-shutdown; Sunshine/display/EDID/latency settings; artwork; notifications; locale selection; and instance reuse after relaunch. One happy-path stream does not establish parity for these other workflows.

## 16. Security, privacy, and App Store review

- Keep credentials out of JSON state, logs, diagnostics, crash text, and frontend storage.
- Redact host secrets, API keys, OAuth tokens, private keys, tunnel keys, and signed URLs.
- Restrict Tauri capabilities to the main window and only required plugins.
- Use allowlisted navigation and open external links outside the app WebView.
- Explain VPN, local network, microphone, notifications, files, and location usage at the point of use.
- Review Apple Network Extension entitlement eligibility before making it a release blocker.
- Review cryptography/export compliance based on WireGuard, SSH, TLS, and bundled crypto libraries; do not copy `ITSAppUsesNonExemptEncryption=false` without confirming it is accurate.
- Add required third-party licenses and notices for Moonlight, WireGuardKit, Opus, ENet, and other native dependencies.
- Define account/data deletion behavior and privacy policy links before App Store submission.

## 17. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| New desktop code does not compile for iOS | Add target-specific features early and keep simulator compile in every PR |
| Native streaming scope dominates the port | Validate control-plane UI incrementally; retain streaming as a mandatory completion milestone |
| Reference VPN port loses current TURN behavior | Implement endpoint/MTU/identity transactions through the iOS adapter and require direct/relay switching and rollback evidence |
| Generated Xcode files drift | Regenerate from clean config and apply deterministic source-controlled patches |
| Network Extension entitlement/signing blocks devices | Validate on a signed development device in Phase 1/2, before polishing UI |
| iOS suspension loses progress events | Persist operation IDs and reconcile state on every foreground transition |
| Secret migration causes data loss | Transactional migration, rollback snapshot, fixture tests, and explicit recovery export |
| Bundle IDs conflict with the existing mobile app | Make identity reuse a Phase 0 decision and derive all child identifiers consistently |
| App Store rejects private API/background behavior | Use public Apple frameworks and document user-visible purpose; avoid unsupported background execution |
| Native dependency supply-chain drift | Pin commits, verify checksums/revisions, document licenses, and build from clean CI |
| Touch UI hides advanced desktop controls | Use progressive disclosure and capability-aware settings rather than removing remote features silently |

## 18. Definition of done for the iOS phase

- The repository builds from a clean checkout for an iOS simulator using documented commands.
- A signed build installs on a physical device and uploads to TestFlight.
- The app contains no required local desktop executable or sidecar.
- All baseline commands, events, settings, and user workflows have verified iOS mappings. The parity register has no mandatory items marked blocked, unverified, deferred, or unsupported.
- Each desktop-specific mechanism has a tested iOS equivalent that preserves its user outcome. Runtime permission/hardware limits are handled accurately, not used to hide incomplete implementations.
- Onboarding, offers, instance lifecycle, provisioning, direct WireGuard, Cloudflare TURN, pairing, streaming, and disconnect complete on a physical device.
- Direct/TURN/Auto preferences, credential/allocation lifecycle, endpoint/MTU validation, commit/rollback, connection repair, and diagnostics preserve the original behavior and defaults.
- Remote terminal, bidirectional clipboard, multi-file/folder uploads, remote folder browsing, all shared-storage provider/backup/restore/schedule flows, and automatic shutdown pass on-device parity scenarios.
- Audio, microphone, supported touch/keyboard/mouse/controller input, host/display/latency settings, overlays, artwork/IGDB, notifications, localization, tutorial/help, and sound preferences are verified against the baseline.
- Secrets are stored in Keychain-backed storage and absent from state/log exports.
- The UI works at compact iPhone and iPad sizes with safe areas, touch targets, keyboard handling, and no horizontal overflow.
- Foreground/background and network transitions reconcile operation and tunnel state correctly.
- Permission denial and revocation produce recoverable user guidance.
- Privacy manifest, usage strings, entitlements, export compliance, and third-party notices are reviewed.
- CI validates frontend, contracts, Rust logic, iOS target compilation, and simulator build.
- Release documentation lists exact environment variables, signing requirements, build commands, and rollback procedure.

## 19. First implementation pull requests

1. **PR 1: Mobile architecture baseline** — entry-point split, mobile config, iOS scripts, target features, clean simulator shell.
2. **PR 2: Contract and capability matrix** — generated command inventory, capability API, stable platform errors, CI parity check.
3. **PR 3: Secure state foundation** — Keychain-backed secret references and transactional migration.
4. **PR 4: Mobile shell and onboarding** — safe areas, navigation, responsive forms, permission timing.
5. **PR 5: Dashboard/offers/provisioning** — touch layouts and foreground reconciliation.
6. **PR 6: Apple document/OAuth/diagnostic bridges** — security-scoped files, callback flow, share sheet.
7. **PR 7: Managed WireGuard** — extension, shared groups, tunnel lifecycle and recovery.
8. **PR 8: Cloudflare TURN and transport parity** — credentials, relay lifecycle, iOS endpoint/MTU transactions, Direct/TURN/Auto, validation, rollback, and repair.
9. **PR 9+: Native streaming** — renderer, audio, input, microphone, network transitions, overlays, and performance hardening.
10. **Subsequent PRs: Storage, terminal, clipboard, and remaining interface parity** — complete every baseline workflow and its mobile-device acceptance scenarios.
11. **Final PRs: Parity verification and release** — close all register items with evidence, validate clean builds and upgrades, and prepare TestFlight delivery.

Each PR should leave desktop behavior intact unless the repository explicitly changes to iOS-only. Platform-specific removals must be guarded by target configuration, not implemented as unconditional deletion of working desktop code.

## 20. Mandatory behavior-parity register

All entries below begin as **not_started**. These are implementation requirements, not claims that the reference implementation has been validated. The per-command inventory in section 9 expands these feature groups before implementation; each row must retain links to code changes and verification evidence as work proceeds.

| Area and source | Required mobile outcome | Acceptance evidence |
| --- | --- | --- |
| Startup/state — `src-tauri/src/main.rs`, `services/state_store.rs`, `src/store/appStore.ts` | Load/save current state, retain identities/preferences/checkpoints, restore pending work and report failures accurately | Fresh install, restart, interrupted write, migration, and upgrade fixtures; device state survives relaunch |
| Onboarding/credentials — `src/features/onboarding/`, `services/ssh_keys.rs`, `services/vast_api.rs` | Same validation, account setup, SSH key generation/upload, credential updates, and tutorial outcomes | Invalid/valid credentials, existing keys, repeated setup, authentication failure and recovery |
| Location/offers/wallet — `services/location.rs`, `services/offer_selector.rs`, `src/features/servers/` | Preserve location choices, countries, pagination, ranking/filtering, pricing, storage selection, and balance reporting | Identical fixture inputs produce the same ranking/selection; phone controls expose all original options |
| Instances/provisioning — `services/orchestration.rs`, `services/instance_manager.rs`, `services/instance_lifecycle.rs` | Create/reuse, readiness checks, stage ordering, stop-after-stage, retry/resume, pause/destroy, and ownership checks | Equivalent remote effects and checkpoints; lost response/resume does not duplicate instance creation |
| SSH/remote operations — `services/remote_exec.rs` and remote setup services | Execute the same remote commands with equivalent stdout/stderr, exit status, authentication and failure behavior through an in-process SSH library | Key/password auth, host identity validation, timeout, dropped connection, remote failure, file transfer, and cleanup tests |
| Direct WireGuard — `services/wireguard.rs`, `services/network_control.rs`, `network-contracts/` | Equivalent tunnel setup/status/verify/reconnect/disconnect, managed identity, endpoint and MTU control | Real device reaches host/control/Sunshine and recovers tunnel; adapter read-back and rollback verified |
| Cloudflare TURN — `services/cloudflare_turn.rs`, `services/connection_manager.rs`, `network-agent/src/turn_*` | Same settings/credential flow, host relay allocation, switching/repair/evaluation and lifecycle semantics | Direct and relay scenarios including failure, expiry, renewal, revisions, metrics, rollback and rollout gates |
| Sunshine/display — `services/sunshine.rs`, `services/post_wireguard_setup.rs`, `services/remote_display.rs` | Configure/verify/retry/reset, pairing readiness, EDID generation, display mode application and service restart | Remote settings and effective display match requested values; invalid options and failed restarts recover correctly |
| Launch library/artwork — `commands/launch_library.rs`, `services/software_artwork.rs`, `src/features/launch-library/` | Same app discovery, launch job tracking, artwork lookup, IGDB credentials, and error states | Compare library results and launch requests; launch succeeds and failed jobs remain inspectable |
| Moonlight identity/pairing — `src-tauri/src/moonlight/application/`, `infrastructure/gamestream/` | Register/refresh/forget hosts, per-instance pipeline state, pair, list apps and persist identities | Pairing success/failure/retry; remembered hosts and certificates remain consistent after relaunch |
| Stream/render/input — `src-tauri/src/moonlight/runtime/`, `platform/`, `src-tauri/src/input/`, `src/features/moonlight/` | Start/disconnect/quit remote app, geometry, preferences, keyboard/mouse/controller input and touch equivalents | Device video/audio and interactive input; orientation/geometry correctness; disconnect versus quit has the original remote effect |
| Stream quality/diagnostics — `moonlight/adaptive_packet_size.rs`, `moonlight/performance.rs`, `network_monitor/` | Preserve adaptive packet-size logic, host latency preferences, performance toggles, metrics and network warnings | Shared policy fixtures plus measured device behavior over direct and relay paths; thresholds and settings match baseline |
| Microphone — `services/mic_passthrough.rs`, `src-tauri/src/mic_client/`, `src/components/ui/MicControls.tsx` | Configure/enable/disable/reconnect/mute/unmute, supported input selection, metrics, remote device recreation and capture lifecycle | Remote receives audio; denied permission, interruption, route change, mute and reconnect maintain accurate state |
| Clipboard — `services/clipboard.rs`, Moonlight clipboard commands | Send and receive clipboard text with the same remote result using iOS paste/copy interaction | Both directions work; denied/empty clipboard and remote failure are surfaced; no fabricated success |
| Terminal — terminal commands in `commands/mod.rs`, `src/features/dashboard/InstanceTerminalModal.tsx` | Interactive remote PTY open/write/resize/close, output/exit events and cleanup | Interactive command, Ctrl-C/Tab/Escape, resize, remote exit, disconnect and repeated open/close on device |
| Upload — upload commands in `commands/mod.rs`, `src/features/dashboard/InstanceUploadModal.tsx` | Remote destination browsing, selected files/directories, content transfer, progress and completion/errors | Multiple files, nested folder, large file, picker cancellation, inaccessible document and interrupted upload |
| Shared storage — `commands/shared_storage.rs`, `services/shared_storage/`, `src/features/shared-storage*/` | All provider/profile/authentication/testing, backup/restore/sync, object selection, progress/cancellation and schedule workflows | Original remote agent contracts and data restored correctly; profile changes, OAuth cancellation, interrupted jobs and recovery tested |
| Auto shutdown — `commands/auto_shutdown.rs`, `services/lifecycle_agent.rs`, `src/features/settings/AutoShutdownSettings.tsx` | Same settings/status and remote backup/shutdown coordination | Remote idle/activity and backup scenarios; phone suspension must not invent a new shutdown policy |
| Notifications/help/localization — `src/lib/*Notifications.ts`, `src/lib/i18n.tsx`, `src/locales/`, `src/prompts/` | Preserve notification preferences/triggers, locale choices, tutorials/help, sound and external-link actions | Permission outcomes, duplicate-event handling, all shipped locales and touch-accessible assistance flows |
| Diagnostics/update lifecycle — `services/diagnostics.rs`, `services/health_check.rs`, `src/lib/updateChecker.ts` | Equivalent actionable health report/export and application-version/update path with state-preserving lifecycle | Native report sharing, accurate platform checks, store update route and install/upgrade recovery on device |

### Evidence and completion rules

- Record baseline expectation, iOS implementation path, status, test/scenario, result, device/OS or simulator target, and revision for each mapped behavior.
- Compare observable outcomes and remote effects, not just whether the app compiles or a button is present.
- Exercise success, failure, interruption, and recovery for each stateful workflow.
- Preserve original gated behavior: disabled-by-default functionality stays disabled by default, while its supported enabled path must still be ported and verified.
- Keep UI rearrangements focused on touch, safe areas, keyboard, orientation, and available space. Preserve original actions, settings, defaults, localized meaning, and business logic.
- Any iOS platform limitation that prevents a required outcome remains a recorded blocker until an equivalent is proven or the user explicitly changes the requirement.
- Mark the full port complete only when the command/event inventory and every required row are verified. Intermediate builds may be used for development testing without being described as a finished port.
