# No Land iOS Mobile Port Plan

## 1. Purpose

This document defines the implementation plan for porting the current No Land desktop application in this repository to iOS.

The port will preserve the current product workflows, API contracts, state model, and visual identity while adapting the interface and platform integrations for iPhone and iPad. The existing project at `/Users/felipebarros/Code/no_land_mobile` is the reference for iOS environment setup, Tauri mobile commands, Apple project configuration, native bridges, signing, and deployment. It is not the source of truth for current product behavior because this repository contains newer desktop features and interfaces.

## 2. Goals

1. Deliver an iOS application using Tauri 2, React, TypeScript, Rust, and the existing No Land design system.
2. Port the current user-facing interfaces to touch-friendly, responsive iPhone and iPad layouts.
3. Keep TypeScript/Rust request and response contracts aligned rather than creating an unrelated mobile API.
4. Preserve cloud orchestration features that can run safely in an iOS sandbox.
5. Replace desktop-only integrations with iOS-native equivalents where required.
6. Reuse the proven environment variables, build commands, Apple signing model, and CI patterns from `no_land_mobile`.
7. Keep the desktop-origin implementation understandable by isolating platform-specific code behind explicit interfaces.

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
  localTerminal: boolean;
  filePicker: boolean;
  nativeShare: boolean;
  selfUpdate: boolean;
  nativeClipboard: boolean;
}
```

The UI should hide, replace, or explain unavailable actions based on this response. Unsupported operations must return stable typed errors rather than panic or silently do nothing.

### 6.3 State and secrets

- Keep non-secret state in the Tauri app data directory.
- Move Vast keys, SSH credentials, OAuth refresh tokens, tunnel material, and recovery keys to secure storage.
- Persist opaque references in app state rather than secret values.
- Perform state migration transactionally and preserve rollback behavior.
- Define an import/export interchange format that never exports secrets unless the user explicitly chooses an encrypted recovery operation.
- Test upgrade from the current desktop-origin state schema and from the older mobile reference schema.

### 6.4 Networking

- Continue using `reqwest` and pure-Rust SSH/network logic where iOS permits it.
- Replace local `wg`/`wg-quick` commands with a Network Extension packet tunnel.
- Store tunnel configuration in a shared Keychain access group and expose only a stable reference to Rust/state.
- Use an app group shared by the main app and packet-tunnel extension.
- Reconcile VPN state after foreground resume, network changes, extension termination, and app updates.
- Declare local-network and Bonjour usage only when embedded streaming/discovery needs them.

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
| WireGuard | Replace | Network Extension + WireGuardKit + shared Keychain/app group |
| Tailscale/provider flows | Adapt | Universal/deep links and OAuth callback handling; no desktop app launching assumptions |
| Moonlight pairing | Port | Reuse command contracts; use iOS-native stream surface and lifecycle |
| Embedded streaming | Native port | Metal/UIKit renderer, GameController/touch input, AVAudioSession |
| Microphone forwarding | Native port | iOS permission, capture, interruption recovery, mute/status APIs |
| Clipboard sync | Adapt/defer | Use `UIPasteboard` with explicit user action and iOS privacy expectations |
| Remote terminal | Defer or replace | `portable-pty` cannot be local; optionally build a direct SSH terminal with touch keyboard UX |
| File upload | Adapt | UIDocumentPicker/security-scoped access; stream selected files without assuming permanent paths |
| Shared storage | Adapt | Security-scoped files, secure provider credentials, background/suspension-safe operation state |
| Auto shutdown | Reuse | Remote agent feature; remove desktop-local service assumptions |
| Display controls | Reuse | Remote settings UI with mobile presets and validation |
| Diagnostics export | Adapt | Native share sheet with redaction |
| Desktop updater | Remove on iOS | App Store/TestFlight handles application updates |
| Exit/relaunch | Remove on iOS | iOS lifecycle owns process termination |
| Sleep prevention | Replace | `UIApplication.isIdleTimerDisabled` while streaming only |
| Multi-window stream | Replace | Single in-app full-screen route for phase 1 |

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
- Hide desktop updater, process, binary-path, and local-tool settings on iOS.
- Clearly label settings that configure the remote Linux host rather than the phone.

#### Launch library and stream

- Use two-column cards on larger phones/tablets and one column on compact widths.
- Enter a dedicated full-screen landscape-friendly stream route.
- Auto-hide stream controls but retain a discoverable gesture/button to restore them.
- Provide touch/controller input mode, keyboard invocation, microphone toggle, network quality, and disconnect controls.
- Handle the home indicator and notches without covering controls.

#### Terminal and file transfer

- Do not ship the current desktop terminal unchanged.
- For phase 1, hide terminal if a safe direct-SSH terminal is not ready.
- Use the native document picker for upload; do not expose desktop filesystem paths.
- Show transfer progress and a resumable/unknown-outcome state when iOS suspends the app.

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

Fail CI if a frontend wrapper references an unregistered command or if a registered product command has no disposition.

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

Create explicit features such as:

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

Port the reference `native:bootstrap` approach, but ensure all dependency source directories are ignored and reproducibly fetched at pinned revisions. Record:

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
- Node 20.x and npm 10.x, matching the reference baseline until deliberately upgraded;
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
- Gate unavailable terminal, updater, desktop file, and desktop window actions.
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

### Phase 5 — Managed WireGuard

- Port the reference Network Extension structure and pin WireGuardKit.
- Add shared app/Keychain groups and packet-tunnel entitlement.
- Implement install/start/status/reconnect/stop/remove and recovery states.
- Reconcile tunnel state on launch and resume.
- Test install, upgrade, revoked permission, duplicate profile, network switch, and extension crash paths.

**Exit criteria:** a provisioned instance can establish and recover its managed tunnel on a signed device.

### Phase 6 — Native streaming and input

- Build native dependencies for device and simulator.
- Attach the iOS native video surface and Metal renderer.
- Integrate playback audio, microphone forwarding, touch, software keyboard, and GameController.
- Add stream lifecycle, keep-awake, interruption, and reconnect handling.
- Port statistics/network warning overlays to mobile-safe controls.

**Exit criteria:** pair, launch, interact, reconnect, and disconnect on physical iPhone/iPad with measured stability.

### Phase 7 — Storage and optional advanced tools

- Port security-scoped file upload and shared-storage flows.
- Validate behavior across suspension and unknown remote outcomes.
- Decide whether to implement a direct-SSH mobile terminal or keep it deferred.
- Add encrypted recovery/import/export workflows.

**Exit criteria:** supported transfers are safe, resumable/reconcilable, and do not depend on desktop paths.

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
| Native streaming scope dominates the port | Ship control-plane UI first; make streaming a gated milestone |
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
- All shipped frontend backend calls map to registered, tested commands.
- Unsupported desktop features are hidden or clearly explained, not broken.
- Onboarding, offers, instance lifecycle, provisioning, VPN, pairing, streaming, and disconnect complete on a physical device.
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
8. **PR 8+: Native streaming** — renderer, audio, input, microphone, network transitions, and performance hardening.

Each PR should leave desktop behavior intact unless the repository explicitly changes to iOS-only. Platform-specific removals must be guarded by target configuration, not implemented as unconditional deletion of working desktop code.
