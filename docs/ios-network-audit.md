# iOS network audit — 2026-10-09

## Verdict

**Not yet full iOS parity.** The device passed VPN activation and the initial
WireGuard handshake. The latest failure is MTU tuning parsing the local opaque
Keychain reference as if it were the plaintext configuration.

## Pipeline and evidence

| Stage | Implementation / evidence | Status |
| --- | --- | --- |
| SSH and uploads | `remote_exec_ios.rs`, `noland-ssh`; device completed remote agents, NVIDIA, Sunshine, WireGuard and microphone receiver | Exercised on device; readiness retry semantics still differ from desktop |
| Remote WireGuard setup | `WireGuardService::configure`, shared SSH commands; client route `10.77.0.1/32` | Device completed; desktop helper precondition removed in `2f85c6f` |
| VPN activation | `NolandVPNBridge.swift`, signed app/extension, WireGuardKit | Device passed activation/handshake after entitlement, JSON-key and startup-race fixes |
| Protected config | `read_local_wireguard_configuration`, shared Keychain | MTU, resume and endpoint readers corrected in this audit |
| Connected MTU discovery | `wireguard_mtu.rs`, Darwin ICMP sockets in `ios_network.rs` | Replaces local `ping`; bounded IPv4 DF probes validate size/nonce/sequence/checksum, retain explicit fallback for blocked ICMP; current inner route is IPv4 |
| MTU apply | `set_managed_gotatun_mtu` → provider → WireGuardKit + Keychain | Corrected to mutate live tunnel with local/remote rollback instead of editing the reference file and restarting |
| Runtime telemetry | provider UAPI and Darwin interface MTU | Endpoint/peer/MTU now measured rather than echoed from requested config; counters/handshake already came from UAPI |
| Reachability / Sunshine | TCP/HTTP in `post_wireguard_setup.rs` | Native Rust sockets; complete post-MTU device flow pending |
| Streaming packet sizing | `adaptive_packet_size.rs` | Now queries active iOS provider/config rather than desktop helper status files |
| Network quality | `network_monitor` authenticated UDP + WebSocket telemetry | Shared sockets compile for iOS; live-stream validation pending |
| Direct/TURN | `connection_manager`, `cloudflare_turn`, remote network agent | Host owns TURN allocation/bridge; iOS mutates WireGuard endpoint. Foreground switching/rollback implemented but not device-validated |
| Restart after mutation | provider loads shared Keychain | Fixed stale profile-fingerprint rejection after MTU/endpoint updates; explicit start options still pin fingerprint |
| Multiple profiles | bridge preferences lookup | Fixed object-identity exclusion across preferences loads; current profile excluded by stable reference |

## Outstanding network parity work

1. **Lifecycle / TURN renewal:** `run_connection_maintenance` runs in the app
   process and cannot run reliably while iOS suspends it. WireGuardKit path
   monitoring does not replace TURN allocation renewal or foreground reconciliation
   of interrupted host/client transactions and persisted UI state.
2. **Feature gates:** TURN switching and Auto still use the baseline environment
   gates `NOLAND_ENABLE_VERIFIED_TURN_SWITCHING` and
   `NOLAND_ENABLE_AUTOMATIC_TRANSPORT_SELECTION`. Installed iOS apps do not inherit
   shell environment settings. This audit does not enable these gates.
3. **Concurrent/durable mutations:** fingerprint checks and rollback exist, but
   asynchronous provider update serialization, late callbacks, timeouts, and
   termination during commits need fault-injection coverage and reconciliation.
   WireGuardKit can time out internally applying settings, so live MTU read-back
   matters; successful callback alone is insufficient.
4. **Migration:** older `managed_ios` state/secure references and all per-server
   absolute paths across container relocation are not fully migrated.
5. **SSH readiness retries:** shared `wait_for_ssh_acceptance` expects desktop
   nonzero exit output; iOS typed connection/authentication errors can exit early.
6. **Device acceptance:** MTU, pairing/stream traffic, Direct/TURN/Auto,
   Wi-Fi↔cellular, airplane mode, sleep/resume, extension restart, allocation
   expiry and failed rollback still need end-to-end verification. Local health
   checks and build success are not proof of these outcomes.

## Checks

- `node scripts/check-ios-network.mjs`: actual Darwin ICMP loopback at 576 and
  1440 bytes plus reply validation regression test. Host Darwin execution is
  not proof of iOS sandbox/external ICMP behavior.
- `npm run check:ios`: iOS Rust/UIKit compilation.
- `npm run check:ios:vpn`: Swift packet-extension compilation.
- Signed app build and the on-device MTU retry are separate acceptance steps.
