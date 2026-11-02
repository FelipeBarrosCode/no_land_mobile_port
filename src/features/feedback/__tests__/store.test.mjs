import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { disabledFeedbackStatus, normalizeFeedbackStatus } from "../model.ts";

// Node's native type stripping runs the real TS store without tsc, a bundler, or extra dependencies.
const nativeMock = `export function invokeSafe(command, args) { return globalThis.__feedbackInvoke(command, args); }`;
registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.endsWith("/lib/tauri")) return { url: `data:text/javascript,${encodeURIComponent(nativeMock)}`, shortCircuit: true };
    if (specifier.startsWith(".") && context.parentURL?.startsWith("file:")) {
      const candidate = new URL(`${specifier}.ts`, context.parentURL);
      if (existsSync(fileURLToPath(candidate))) return { url: candidate.href, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
});
const { useFeedbackStore } = await import("../store.ts");
const { feedbackNative } = await import("../api.ts");
const store = useFeedbackStore.getState;
const enabled = () => normalizeFeedbackStatus({
  ...disabledFeedbackStatus(), enabled: true, configured: true, supported: true, enrollmentStatus: "enrolled",
  userId: "11111111-1111-4111-8111-111111111111", installationId: "22222222-2222-4222-8222-222222222222",
  credentialRef: "native-ref", deletionCodeRef: "native-deletion-ref", consentEpoch: 1,
  privacyVersion: "1", termsVersion: "1", consentedAt: "2026-10-26T12:00:00Z",
});
const summary = (id) => ({ providerMachineId: id, reliability: { source: "no_land", ratio: 0, eligibleSessions: 2,
  successfulSessions: 0, failedSessions: 2, basis: "ready_and_paired_before_destroy", scope: "retained_history" },
  jitter: null, reviews: { commentCount: 0, ratingCount: 0, averageScore: null }, asOf: "2026-10-26T12:00:00Z" });
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
let calls = [];
function reset(status = disabledFeedbackStatus()) {
  calls = [];
  store().invalidate(); store().acceptStatus(status);
  globalThis.__feedbackInvoke = async (command, args) => { calls.push([command, args]);
    if (command === "feedback_program_status") return status;
    if (command === "feedback_machine_summaries") return { summaries: args.machineIds.map(summary) };
    if (command === "feedback_machine_reviews") return { reviews: [], nextCursor: null };
    return status;
  };
}

test("disabled startup only invokes local status; reads/moderation never invoke native HTTP commands", async () => {
  reset();
  await store().refreshStatus();
  await assert.rejects(() => store().summaries(["1"]));
  await assert.rejects(() => store().reviews("1", null));
  await assert.rejects(() => store().report("review", "spam"));
  await assert.rejects(() => store().blockAuthor("review"));
  assert.deepEqual(calls.map(([command]) => command), ["feedback_program_status"]);
});

test("actual summary requests batch/deduplicate IDs, cache values and deduplicate overlapping in-flight reads", async () => {
  reset(enabled());
  const ids = Array.from({ length: 205 }, (_, index) => String(index + 1));
  await store().summaries([...ids, ...ids, 999, undefined]);
  assert.deepEqual(calls.map(([, args]) => args.machineIds.length), [100, 100, 5]);
  await store().summaries(ids); assert.equal(calls.length, 3);
  assert.equal(store().summary("1").reliability.ratio, 0);
  reset(enabled());
  const pending = deferred();
  globalThis.__feedbackInvoke = (command, args) => { calls.push([command, args]); return pending.promise; };
  const first = store().summaries(["1", "2"]);
  const second = store().summaries(["2"]);
  assert.equal(calls.length, 1);
  pending.resolve({ summaries: [summary("1"), summary("2")] });
  await Promise.all([first, second]);
});

test("switching authenticated viewers clears filtered counts and never shares cached machine responses", async () => {
  reset(enabled());
  await store().summaries(["1"]);
  const nextViewer = { ...enabled(), userId: "44444444-4444-4444-8444-444444444444",
    installationId: "55555555-5555-4555-8555-555555555555" };
  store().acceptStatus(nextViewer);
  assert.equal(store().summary("1"), undefined);
  await store().summaries(["1"]);
  assert.equal(calls.filter(([command]) => command === "feedback_machine_summaries").length, 2);
});

test("expired summaries disappear on reads and native read errors leave no invented community values", async () => {
  reset(enabled());
  const realNow = Date.now;
  let now = realNow();
  Date.now = () => now;
  try {
    await store().summaries(["1"]);
    now += 60_000;
    assert.equal(store().summary("1"), undefined);
    globalThis.__feedbackInvoke = async () => { throw new Error("native timeout"); };
    await assert.rejects(() => store().summaries(["1"]), /native timeout/);
    assert.equal(store().summary("1"), undefined);
  } finally { Date.now = realNow; }
});

test("a pending consent change with queued deletion gates all summaries and review pages", async () => {
  reset(normalizeFeedbackStatus({ ...enabled(), pendingControlAction: {
    kind: "consent", requestId: "33333333-3333-4333-8333-333333333333", expectedEpoch: 1,
    enabled: true, optionalContextEnabled: false, afterAck: "delete",
  } }));
  await assert.rejects(() => store().summaries(["1"]));
  await assert.rejects(() => store().reviews("1", null));
  assert.equal(calls.length, 0);
});

test("withdrawal disables immediately, clears cached visibility and rejects a stale summary reply", async () => {
  reset(enabled());
  const pending = deferred();
  globalThis.__feedbackInvoke = (command, args) => {
    calls.push([command, args]);
    return command === "feedback_machine_summaries" ? pending.promise : Promise.resolve({ ...enabled(), enabled: false, enrollmentStatus: "withdrawn" });
  };
  const summaries = store().summaries(["1"]);
  const withdrawal = store().consent(false, false);
  assert.equal(store().status.enabled, false);
  assert.equal(store().summary("1"), undefined);
  await withdrawal;
  pending.resolve({ summaries: [summary("1")] }); await summaries;
  assert.equal(store().summary("1"), undefined);
});

test("a pre-control local status reply cannot re-enable after withdrawal", async () => {
  reset(enabled());
  const pending = deferred();
  globalThis.__feedbackInvoke = (command) => command === "feedback_program_status" ? pending.promise : Promise.resolve({ ...enabled(), enabled: false, enrollmentStatus: "withdrawn" });
  const oldStatus = store().refreshStatus();
  await store().consent(false, false);
  pending.resolve(enabled()); await oldStatus;
  assert.equal(store().status.enabled, false);
});

test("deletion uses exact native seam and never restores enabled on a lost response", async () => {
  reset(enabled());
  globalThis.__feedbackInvoke = async (command, args) => { calls.push([command, args]); throw new Error("offline"); };
  await assert.rejects(() => store().deleteContributions());
  assert.equal(store().status.enabled, false);
  assert.equal(calls[0][0], "feedback_program_delete");
  assert.equal(calls[0][1], undefined);
});

test("blocking/reporting invalidates viewer-specific counts and prevents stale pages from crossing visibility changes", async () => {
  reset(enabled()); await store().summaries(["1"]);
  const before = store().visibilityRevision;
  await store().blockAuthor("33333333-3333-4333-8333-333333333333");
  assert.ok(store().visibilityRevision > before);
  assert.equal(store().summary("1"), undefined);
  assert.equal(calls.at(-1)[0], "feedback_review_block_author");
  const pending = deferred();
  globalThis.__feedbackInvoke = () => pending.promise;
  const page = store().reviews("1", null);
  store().invalidate();
  pending.resolve({ reviews: [], nextCursor: null });
  await assert.rejects(() => page, /visibility changed/);
  reset(enabled());
  await store().report("33333333-3333-4333-8333-333333333333", "spam");
  assert.deepEqual(calls[0], ["feedback_review_report", { reviewId: "33333333-3333-4333-8333-333333333333", reason: "spam" }]);
});

test("enrollment sends only explicit scopes/policy/profile, leaving identity and secrets native", async () => {
  reset();
  const request = { privacyVersion: "1", termsVersion: "1", participationAccepted: true, optionalContextEnabled: false, optionalProfile: {} };
  await store().enroll(request);
  assert.deepEqual(calls, [["feedback_program_enroll", { request }]]);
  assert.equal("installationId" in calls[0][1].request, false);
  assert.equal("credential" in calls[0][1].request, false);
});

test("native evidence controls eligibility; pairing-only/malformed evidence is never eligible", async () => {
  reset(enabled());
  for (const result of [{ eligible: false, paired: true }, { paired: true }, null]) {
    globalThis.__feedbackInvoke = async () => result;
    assert.equal(await feedbackNative.eligible("123"), false);
  }
  globalThis.__feedbackInvoke = async () => ({ eligible: true });
  assert.equal(await feedbackNative.eligible("123"), true);
  calls = [];
  globalThis.__feedbackInvoke = async (command, args) => { calls.push([command, args]); return {}; };
  await feedbackNative.stageReview("123", { score: null, comment: "plain text" });
  assert.deepEqual(calls, [["feedback_stage_review", { providerInstanceId: "123", score: null, comment: "plain text" }]]);
});

test("review pagination passes native string machine ID, opaque cursor and bounded limit", async () => {
  reset(enabled());
  await store().reviews("9007199254740993", "opaque");
  assert.deepEqual(calls[0], ["feedback_machine_reviews", { machineId: "9007199254740993", cursor: "opaque", limit: 20 }]);
  reset(enabled());
  await assert.rejects(() => store().reviews("9223372036854775808", null));
  assert.equal(calls.length, 0);
});
