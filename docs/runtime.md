# Modern Linux Runtime — mobile preview

iOS controls the same optional **remote Linux** runtime as the No Land desktop
client. Ubuntu 24.04, AppImages and native ELF files execute on the rented x86-64
Linux PC, **never on the iPhone/iPad**. No downloaded Linux executable, rootfs or
compatibility engine is loaded into the iOS process or bundled as an app sidecar.

## Usage

Open an instance's launch library → **Modern Linux Runtime**:

1. Enable the remote companion (optional, independent of normal desktop access).
2. Review measured download/free-disk sizes **on the remote PC**, then approve
   installation. Consent is tied to the reviewed artifact SHA256.
3. Enter the absolute path of a trusted Linux executable/AppImage on that PC.
   There is intentionally no iOS file picker: device `file://` URLs and sandbox
   document paths do not identify remote executables.
4. Launch the registered app through its runtime library card. Per-app shortcut
   buttons create shortcuts on the **remote desktop**, not the iOS Home Screen.
5. Use diagnostics, cancel/retry or rollback as needed. Native and Steam/Proton
   routes stay unchanged. Runtime errors never silently retry on the parent OS
   or fall back to PRoot.

The touch layout stacks controls on narrow screens, uses 44px minimum touch
targets and 16px path input text to avoid iOS focus zoom. Status polling starts
only while expanded, pauses when the app is hidden, ignores stale responses after
closing, and refreshes on return. Authorized installs and launched games are
supervised remotely and are not cancelled merely because iOS suspends the client
or the SSH request ends. The reviewed download plan is cleared on visibility
changes. A companion upload interrupted before installation may need **Enable**
again; background execution of phone-side upload work is not guaranteed.

## Shared signed release setup

This client reuses the engine, artifact builder and native GTK companion from
[`FelipeBarrosCode/no_land`](https://github.com/FelipeBarrosCode/no_land), built by
its `runtime-artifacts.yml` workflow on Ubuntu 22.04. The mobile application only
downloads a pinned, size-limited Linux companion to memory, verifies SHA256, and
transfers it over the existing in-process SSH adapter to the remote PC. It does
not execute downloaded code on iOS. The large base image download, signature
verification, extraction, launch and supervision all take place on the remote PC.

Configure these four **public repository variables** for iOS CI/App Store builds,
or export them before building locally:

```text
NOLAND_RUNTIME_AGENT_URL=https://…/<immutable-tag>/noland-runtime-linux-x86_64
NOLAND_RUNTIME_AGENT_SHA256=<64-hex SHA256 from the verified release>
NOLAND_RUNTIME_MANIFEST_URL=https://…/<immutable-tag>/manifest.json
NOLAND_RUNTIME_PUBLIC_KEY=<64-hex Ed25519 public key verified out-of-band>
```

Never include a private signing key in the mobile repository/application. Do not
use mutable `latest` URLs. Without all four valid pins, enabling stays disabled;
an already-installed compatible companion can still be managed. No new iOS
entitlements, background modes, local shell/PTY, public services or ports are
introduced. App Store review remains a release process, not a guarantee made by
this implementation.

The remote parent must already have `zenity`, `bwrap`, `xauth`, `curl`, `sudo`,
`visudo`, `timeout`, `ldconfig`, `ldd`, `sha256sum`, matching injected NVIDIA
graphics/display components, the existing No Land X11 session and audio endpoint.
Enabling requires root/passwordless administrative SSH. It installs only the
pinned companion, constrained credential helper/sudoers entry, release metadata
and desktop shortcuts; it does not run apt, change libc/drivers, or restart
Xorg/Sunshine.

## Security and remaining gates

Sharing trusted X11 is **not a security sandbox** for arbitrary downloads. The
remote engine refreshes private Xauthority, keeps per-app saves outside images,
checks namespaces/glibc/hardware rendering/audio and refuses unsupported native
backends. Credentials/driver bindings/images/caches are excluded from updated
shared-storage attribution rules. iOS CI rebuilds existing remote-agent bundles
from source; ensure those rebuilt bundles are deployed before relying on the
exclusions. Automatic runtime-app shared-storage restoration remains unfinished;
manually back up remote registry and `app-data`, never private sessions or
machine-specific bindings.

Same preview limits as desktop: NVIDIA x86-64/Bubblewrap only; no PRoot, 32-bit,
Steam nesting, Windows compatibility or anti-cheat workaround. Lichii's original
assertion is **unvalidated**, not claimed fixed.

Before shipping, publish/configure the signed runtime release and test on actual
iPhone/iPad plus a Vast.ai parent: background/suspension, lost network/reconnect,
interrupted enable/install, streaming GPU/audio/controllers, AppImages, rollback,
small-screen keyboard/safe areas and unchanged native/Steam launches. An iOS SDK,
codesigning and device validation are still required to ship an IPA.

## Checks

```sh
npm run test:runtime
npm run build
npm run i18n:check
npm run check:ios
```

Runtime tests cover consent metadata, remote paths, polling lifecycle, late
results/errors, command registration/SSH boundaries and all nine locales. The
repository-wide localization check currently also reports unrelated pre-existing
hardcoded text in dashboard, feedback, server-picker and mobile-menu components;
those files are not changed by this feature.

Local validation: the frontend build, runtime tests and discovery tests pass.
The iOS native cross-check is blocked by the missing iPhone Simulator SDK on this
machine. A fallback macOS library check also encounters existing feedback-code
errors (private retry methods, a non-Send command future and a milestone borrow
conflict). Those unrelated files are unchanged; a successful iOS/IPA build is
not claimed.
