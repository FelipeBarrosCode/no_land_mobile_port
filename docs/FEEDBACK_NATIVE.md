# Feedback native adapter — iOS mobile port

## Scope and validation boundary

Implemented on the existing `feat/feedback-program-ios` branch, based on `808df8c8a96a692c58c3dc77a60bccd8540cff53`. The complete API implementation plan, coordination contract, current OpenAPI/domain contract, mobile bootstrap, atomic state writer, Keychain-backed state namespace, native video callbacks, network reporter, provider orchestration, and destruction/reconciliation paths were inspected.

This is **source implementation, not platform/runtime certification**. Per the user's instruction, no Cargo check/test/build, native app compilation, Xcode, Gradle, Tauri build, deployment, provider rental, or real contribution submission was performed. Rust source parsing/formatting and source/diff review are permitted. Added Rust tests are intentionally unrun. No commits or pushes were made.

Only native-owned paths and this document were edited. Frontend `src/**` belongs to the frontend agent. The pre-existing README modification, deletion of `src-tauri/binaries/ssh-runtime/README.txt`, and untracked Apple review response files are preserved.

## Entry points and ownership

- `src-tauri/src/lib.rs`: actual mobile entry point; creates `AppContext`, starts the feedback adapter beside the native Moonlight manager, and registers all eleven commands.
- `src-tauri/src/commands/feedback.rs`: exact cross-client command names, camelCase Tauri arguments, object responses, bounded decimal-string/UUID inputs. No frontend HTTP or credential storage is needed.
- `src-tauri/src/models/feedback.rs`: fail-closed consent state, allowlisted wire DTOs, lifecycle order, independent review revisions, public projections and source tests.
- `src-tauri/src/services/feedback/`: centralized collection/delivery permission, protected enrollment/recovery, consent/deletion controls, bounded cumulative spool, lifecycle hooks, metric accumulator, and restart/foreground delivery.
- `src-tauri/apple/NolandPlatform.m`: device-bound feedback Keychain namespace, file-protection/backup exclusion checks, synchronous pre-suspension callbacks and foreground wake-up. `build.rs` links Security.framework on iOS.
- `network-contracts/src/feedback.rs`, `network-agent/src/{session.rs,telemetry/window.rs,transport/websocket.rs}`: additive measurement evidence from the actual existing RTT gauge. Operational thresholds, EWMA calculation and probe/report cadence are unchanged.

## Public configuration and default-off behavior

The only endpoint setting is public compile-time `NOLAND_FEEDBACK_API_URL`, consumed with `option_env!`. No production hostname, operator/contact, administrative key, provider key, or universal bearer credential is supplied. Use an HTTPS base URL, without userinfo, query, fragment or a trailing `/v1`; the adapter appends `/v1`. Debug builds permit HTTP only on loopback for explicit development configuration. Redirects are disabled so credentials cannot follow an alternate origin.

An absent/invalid URL prevents enrollment and collection. Initial `feedbackProgram` state is schema 1 and entirely disabled, with no installation ID. A partial/malformed/future feedback object is not repaired by the general permissive migration. Unsupported/malformed root state versions also disable feedback. The existing schema version is 4; the change is additive, not an assumed migration from another repository. The existing state writer still atomically preserves unrelated root fields such as `moonligConf`. Reloading state from disk also reconciles the native permission gate.

Status is local-only. Default-off startup/foreground does not enroll, fetch notices, enrich searches, generate a contribution identity, create a spool, or send feedback HTTP. Explicit notices/enrollment and durable withdrawal/deletion/receipt work are narrow exceptions. Operational streaming/network monitoring remains independent.

Collection checks consent/enrollment/epoch, credentials, endpoint binding, platform support, background state and explicit cached service pause. Queue insertion and immediate request delivery have additional native checks. A timeout is not interpreted as a collection pause. Scope synchronization pauses contribution delivery while basic consented observations can remain local.

## Protected enrollment and controls

The provider/general-state vault is `noland-connect.state-secrets.v1.mobile-port`. Feedback deliberately has its own namespace:

- Service: `noland-connect.feedback.mobile-port.v1`
- Account: `installation`
- Accessibility: `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`
- Synchronization: `kSecAttrSynchronizable = NO`, default app access group, no shared/cloud identity

The protected, versioned blob contains the installation identity, endpoint binding, scoped bearer, deletion-only code, temporary enrollment proof/request, protected enrollment acknowledgment, cached publication policy, stop latch and deletion receipt capability. Secret-bearing Rust types do not implement Debug. Keychain writes must succeed and pass read-back verification before enrollment HTTP. There is no plaintext fallback. State JSON contains references, not secrets. No MAC, device fingerprint, provider account, Moonlight identity or user agent is used for installation/platform identity.

An explicit accepted enrollment creates a UUID and 32 random bytes encoded as canonical unpadded base64url, plus a stable request UUID. The exact choices/proof are persisted before the enrollment POST. Lost responses retry only that proof-bound enrollment for 24 hours; installation ID alone never recovers credentials. A response already captured in protected storage can finish the local acknowledgment without requiring another server recovery. Protected acknowledgment metadata also repairs a crash between writing recovered credentials and persisting the consent-control epoch. Unknown/expired enrollment without an acknowledgment remains off and retains its proof rather than silently registering a second identity.

Withdrawal/deletion set the local stop gate immediately, before waiting on command/network locks. A protected stop latch supplements the existing atomic state writer, including when normal state persistence fails. Unsent contributions/drafts are cleared, while credentials and minimal durable controls remain for authenticated offline synchronization. Controls carry a stable request ID and expected epoch; acknowledged scope changes keep the same rental/initial epoch and advance the current snapshot epoch/revision. No optional profile is copied into session snapshots. The temporary original enrollment profile/proof is erased once recovery authentication/epoch are durable, and the server consent control removes retained optional context/profile when scope is withdrawn.

A queued scope request is not rewritten after possible transmission; a later withdrawal/deletion is sequenced after its acknowledgment. Permanent HTTP rejections suspend automatic delivery and remain visible; explicit user control retries are possible. Transient connectivity/5xx/429 retries use bounded exponential backoff and Retry-After. Successful enrollment does not join already-existing rentals. Rejoining is blocked until pending controls resolve and never revives old snapshots.

Deletion uses authenticated DELETE `/me`, with the protected deletion-only recovery POST as a fallback for a lost/revoked participant credential. The receipt and separate status capability are persisted before local credential cleanup. Status polling uses that capability, never a participant credential. Receipt ID/status may cross the command seam; bearer, deletion code, recovery proof and receipt capability do not. A deliberate new enrollment after completed deletion explicitly deletes/replaces the old protected namespace; orphaned Keychain items never automatically enroll a reinstalled/recovered app.

## Rental identity, lifecycle and journal

Vast `machine_id` is retained as a canonical positive signed-bigint decimal string in `VastOffer`, `OfferCandidate`, `InstanceState` and `ProvisionedServerState`. Offer IDs, provider host IDs, rental IDs and contribution UUIDs remain separate. There is no `hosting_id`, host label, endpoint, GPU or offer-ID fallback for physical identity. Missing machine identity remains unattributed; a conflicting provider identity suspends delivery.

The rental list additionally exposes exact `providerInstanceId: string` and `machineId: string | null`. Native feedback uploads convert the original native integers directly to decimal strings, never through JavaScript Number. Frontends should prefer this exact rental string. Existing numeric provider-control APIs are unchanged; an older UI that only supplies a numeric rental ID must reject unsafe integers rather than stringify a rounded ID. The frontend's current safe-integer guard does this, so very large rental IDs can hide the optional review until that UI adopts `providerInstanceId`.

Before a real create request, consented new rentals get a durable pending intent. A local observer at the Vast accepted-contract boundary preserves the real rental ID **before** the subsequent detail GET can fail. Later verified details can finish an unattributed intent using only that exact persisted rental ID. Existing-instance/resume/fallback paths never create a new contribution session. A completely lost create response with no known contract ID stays unattributed; this adapter does not guess a matching old rental or issue an automatic paid retry.

Each rental has a stable session UUID, initial/current consent epochs, cumulative snapshot/revision, persisted milestone sequence, acknowledgment watermarks and independent review draft/revision. Per-rental state references point to `feedback/outbox-v1.json#<session UUID>`. The journal is bound to its installation; another installation cannot replay the old file. It survives removal of ordinary rental state.

- `InstanceReady` records actual readiness from the existing provider readiness path.
- Explicit verified pairing, generic native pairing and automatic verified host pairing record paired evidence; connection alone does not.
- Both prerequisites establish setup success once; reconnects do not create another vote.
- Native `NL_EVENT_VIDEO_FRAME` originates in `nl_video_frame_processor` after a real decode unit reaches the renderer submission path. It is **received/submitted evidence, not proof of display presentation**. Generation checks reject stale callbacks. A bounded first-frame inbox prevents frame-then-stop watch coalescing from erasing eligibility, and the latest frame carries monotonic time for the active-stream gate.
- Eligibility and pre-teardown capture read this native evidence directly. Pairing or `NL_EVENT_CONNECTED` is never substituted for a frame. A frame with missing setup observations marks observation incomplete instead of inventing earlier ready/paired evidence.
- Local checkpoint/capture happens before stream, microphone, monitor or tunnel teardown. Backup and provider deletion still run through their established paths and never wait for feedback HTTP.
- Only a positive provider deletion response finalizes destruction and permits queued review delivery. A failed backup, failed DELETE, click, timeout, stopped stream or failed list request is not destruction.
- Process gaps and suspension mark unresolved active observations incomplete while preserving established success. Verified provider-list absence is conservatively incomplete/pending, not a guessed destruction time or failure. Remote/state-agent deletion without explicit terminal evidence therefore cannot manufacture a failure or publish a review.

A separately revisioned review is staged durably only for an enrolled tracked rental with native frame evidence. Score must be an integer 1–10; trimmed blank comments become null; text is bounded to 2,000 UTF-16 units and rejects controls except newline/tab. A nonempty submission requires at least one value. Coordinated `feedback_stage_review` null/null instead durably replaces the draft with a local cancellation tombstone at the next independent review revision. Session delivery/ack precedes review delivery, and review publication is never driven by an older telemetry retry. The API can additionally hold reviews for moderation: native acceptance after confirmed destruction does not promise immediate public visibility.

## Bounded durable delivery

The spool reuses the existing fsync/atomic replacement utility. Its directory is private (0700), is checked for iOS file protection, and is excluded from backup; the final spool file is also protected/checked. It contains allowlisted feedback data only, not whole application/provider responses, network endpoints, raw measurements, credentials, error logs or files.

Bounds are 100 combined sessions/intents, 5 MiB and 30 days from original rental observation (or pending-intent creation), not refreshed by retries. One latest cumulative snapshot and one independent draft per rental are retained. Acknowledged terminal entries retire before admission is refused. Capacity/storage trouble stops new feedback admission and reports status without blocking rental operations. Expiration/drop does not submit a synthetic failure or undo an established server outcome. Control requests live in the ordinary atomic app state outside telemetry eviction and are retained until acknowledged (or explicitly suspended after a permanent error).

Raw probes are never written by feedback. Fresh gauges aggregate in memory, checkpoint about every 30 seconds, and upload at most on approximately 60-second active ticks plus finalization/foreground/control kicks. Dirty cumulative data is durably checkpointed before sending. Each accepted revision retires only its acknowledgment watermark; a newer cumulative snapshot remains pending. Bounded batches skip already-acknowledged records so early rentals cannot starve later sessions.

UIKit will-resign/background callbacks checkpoint synchronously before returning and reset active metric tracking. Foreground retries pending controls/outbox and rechecks protected permission for enabled state. Startup also resumes delivery. No background delivery entitlement, continuous background execution, or background completion guarantee is claimed.

## Jitter — canonical source or N/A

The actual iOS Rust stream path starts the existing UDP/WebSocket `NetworkMonitor` against the prepared host. It can use the network-agent's existing `last60Seconds.jitterMs`: successive received RTT differences, EWMA update divided by 16, reset to zero for each rolling 60-second calculation. The metric remains `rtt_jitter_ewma_window60_report1s_v1`, report cadence 1,000 ms, thresholds `noland_jitter_bands_v1` (>7 ms high, >15 ms severe).

For a consented tracked active stream only, the narrow internal report contract supplies a random report ID, processed highest sample sequence, highest received sequence, received RTT count, window age, source age and metric version. The agent echoes processed evidence rather than the client's newest send timestamp. Probe sequences remain monotonic across internal WebSocket reconnects. Legacy agents missing this evidence continue operational monitoring but contribute **no jitter**. The existing agent deployment revision is bumped to 6 for normal future provisioning; no remote deployment was run here.

Acceptance requires:

1. Current native permission and the correct tracked rental/monitor.
2. Recent actual native frame activity, not a merely connected or historically used stream.
3. A matching locally sent batch ID and processed watermark, consumed once and younger than three seconds.
4. Finite nonnegative jitter, at least two received RTTs, a newly advancing received watermark, and matching counts from the actual statistics response.
5. A full 60-second source window **and** a full 60-second continuously observed active warmup. Gaps/restarts/suspension reset warmup. Short streams are legitimately N/A.
6. Fixed report cadence; duplicate heartbeats, loss-only/stale reports and the operational connection-loss synthetic zero are never accumulated.

Route comes from the existing rental transport state at report time: direct, Cloudflare TURN relay, or unknown/mixed. Route changes keep the same rental and mark the following rolling window mixed for 60 seconds. Bucket dimensions include metric, cadence, route, completeness and threshold version. Cumulative sum/count/min/max/high/severe/active coverage are bounded; no jitter percentile is invented and no average-of-session-averages is used. Monotonic time determines coverage; clock corrections do not regress observation timestamp metadata.

The iOS native runtime's `estimatedRttVarianceMs`, decode timing and renderer metrics are **not** equivalent and are not substituted. If the actual device/tunnel/agent path cannot provide the proven canonical report contract, its contribution jitter is N/A. This implementation establishes the source path, not real-device comparability certification.

## Public reads and frontend handoff

All participant reads/reports/blocks are centrally gated immediately before HTTP. Summary requests deduplicate at most 100 canonical machine strings; reviews use encoded opaque cursors and pages of 1–50 (default 20). Public responses are reserialized through allowlisted native DTOs, not forwarded wholesale.

The projection preserves contract-permitted `jitterGroups`, reliability/jitter `firstObservedAt`/`lastObservedAt`, coarse observation dates, cadence/completeness/threshold provenance and coverage. Invalid/noncanonical jitter becomes N/A, never a falsely certified comparable value. Reliability counts/definition are checked and a ratio below two eligible sessions remains null. Zero is valid. Private contributor/session/rental IDs and arbitrary extra response fields do not cross the public-review/summary seam. Report/block target an opaque review ID; no author ID is needed.

There is no native shared response cache that could leak one viewer's blocked-review visibility to another. The frontend owns its documented short-lived viewer-scoped cache and invalidation/fallback behavior. The native metadata-drop limitation observed in the frontend handoff has been addressed in the DTO projection; that frontend-owned document was not edited.

## Remaining limitations and release checks

- Native compilation and **all runtime/device tests remain unvalidated**, intentionally. Rustfmt parses syntax; it is not a Rust type checker or linker. iOS FFI/Keychain/file protection/UIKit suspension, app-signing/linking, direct/relay routing, real frames, actual agent delivery, offline recovery, TLS and API interoperability require subsequent authorized validation.
- This repository enables the secure contribution adapter only for `target_os = "ios"`. Desktop/Android builds of this port explicitly report unsupported secure enrollment; other agents own those platform repositories. No desktop, Android, simulator or real-device support certification is claimed here.
- Completely lost provider creation identity, ambiguous external disappearance and missed setup evidence remain unattributed/pending/incomplete. The adapter never guesses a paid rental or retries a paid operation. External deletion without explicit confirmation cannot release a review.
- A later explicit Skip durably replaces a draft retained after failed backup/destruction using the existing null/null command seam (matching desktop semantics). The sender atomically claims the latest action and persists a possible-PUT marker before HTTP, including lost-acknowledgment cases. A never-sent cancellation is acknowledged locally; otherwise it sends DELETE with `X-Review-Revision`, never an empty review PUT. Legacy journals conservatively assume possible prior delivery. Older acknowledgments cannot retire a newer cancellation. Cancellation may remove an already-destroyed rental's review; new submissions still require an undestroyed eligible rental. Consent, credentials and normal provider destruction gates are unchanged.
- A future privacy/terms version change pauses old consent. Silent rejoin under mismatched published versions is rejected; an existing-contributor renewed-policy acceptance protocol is not supplied by the current consent endpoint/seam. Completed deletion plus explicit new enrollment remains available. Do not deploy changed policy versions assuming silent renewal.
- The deletion-only code remains protected; the specified seam has no native reveal/copy UI for external recovery. In-app authenticated/recovery deletion and protected receipt polling are implemented. Designing a safe explicit code-reveal flow is a separate product/API seam decision.
- Persistence/Keychain corruption or missing ownership credentials fails closed rather than using a plaintext fallback or another installation's spool. An expired unrecovered enrollment can require the configured operator's help. If both local ownership state and protected recovery material are lost, the adapter cannot claim to recover ownership.
- Publication/operator information, HTTPS endpoint, moderation staffing, server deletion/retention jobs and store privacy disclosures are deployment/release prerequisites, not invented defaults.

Bounded Skip fix validation: `node --test src/features/feedback/__tests__/*.test.mjs` passed all 33 tests, including cancellation ordering/failure/timeout and native source invariants. `rustfmt --edition 2021` parsed/formatted journal/lifecycle/delivery sources; focused `git diff --check` passed. The added Rust regression test for failed-destroy draft replacement, restart, resubmit and lost-PUT-ack cancellation was not executed; no native compilation/check/test/build was run.

Added unrun source tests cover fail-closed state migration, canonical large IDs, lifecycle ordering/no frame from connection, installation-bound recovery, exact delayed machine attribution, independent review/scope revisions, queue count/byte/age bounds, default-off no-spool behavior, stale/duplicate/loss-only/startup gauges, genuine zero, weighted totals/mixed routes, processed-batch echo/received counts, and public metadata projection without private IDs.
