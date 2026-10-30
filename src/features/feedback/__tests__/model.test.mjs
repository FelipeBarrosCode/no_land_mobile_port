import test from "node:test";
import assert from "node:assert/strict";
import {
  disabledFeedbackStatus, normalizeFeedbackStatus, feedbackReadsEnabled, feedbackViewerKey,
  providerMachineId, feedbackOfferMachineId, safeProviderInstanceId, feedbackMachineBatches, feedbackAge,
  normalizeMachineSummary, displayReliability, normalizeProgramDocument, safePolicyUrl,
  optionalFeedbackProfile, normalizeReviewDraft, normalizeReviewsPage,
  FeedbackCache, stageFeedbackThenDestroy, FEEDBACK_CACHE_TTL_MS, FEEDBACK_METRIC_VERSION,
} from "../model.ts";

export const validStatus = () => ({
  ...disabledFeedbackStatus(), enabled: true, configured: true, supported: true,
  enrollmentStatus: "enrolled", userId: "11111111-1111-4111-8111-111111111111",
  installationId: "22222222-2222-4222-8222-222222222222", credentialRef: "native-ref",
  deletionCodeRef: "native-deletion-ref", consentEpoch: 1, privacyVersion: "1", termsVersion: "1",
  consentedAt: "2026-10-26T12:00:00Z",
});

export const summary = (id = "123", successes = 0, failures = 2) => ({
  providerMachineId: id,
  reliability: { source: "no_land", ratio: successes + failures >= 2 ? successes / (successes + failures) : null,
    eligibleSessions: successes + failures, successfulSessions: successes, failedSessions: failures,
    basis: "ready_and_paired_before_destroy", scope: "retained_history" },
  jitter: null, reviews: { commentCount: 0, ratingCount: 0, averageScore: null },
  asOf: "2026-10-26T12:00:00Z",
});

test("missing, malformed, future, incomplete and unsupported consent fail closed", () => {
  for (const value of [undefined, null, {}, { enabled: true }, { ...validStatus(), schemaVersion: 2 },
    { ...validStatus(), userId: null }, { ...validStatus(), userId: "bad" },
    { ...validStatus(), credentialRef: null }, { ...validStatus(), consentEpoch: NaN },
    { ...validStatus(), enrollmentStatus: "future" }, { ...validStatus(), pendingControlAction: "delete" },
    { ...validStatus(), pendingControlAction: { kind: "consent", requestId: "33333333-3333-4333-8333-333333333333",
      expectedEpoch: 1, enabled: true, optionalContextEnabled: false, afterAck: "unknown-future-control" } },
    { ...validStatus(), supported: false }, { ...validStatus(), configured: false }]) {
    assert.equal(feedbackReadsEnabled(normalizeFeedbackStatus(value)), false);
  }
  assert.equal(feedbackReadsEnabled(normalizeFeedbackStatus(validStatus())), true);
  assert.equal(disabledFeedbackStatus().optionalContextEnabled, false);
  const pending = normalizeFeedbackStatus({ ...validStatus(), pendingControlAction: {
    kind: "consent", requestId: "33333333-3333-4333-8333-333333333333", expectedEpoch: 1,
    enabled: false, optionalContextEnabled: false,
  } });
  assert.equal(pending.pendingControlAction.kind, "consent");
  assert.equal(feedbackReadsEnabled(pending), false);
  const queuedDelete = normalizeFeedbackStatus({ ...pending, pendingControlAction: { ...pending.pendingControlAction, afterAck: "delete" } });
  assert.equal(queuedDelete.pendingControlAction.afterAck, "delete");
  assert.equal(feedbackReadsEnabled(queuedDelete), false);
});

test("machine IDs never traverse JS numbers and must fit positive PostgreSQL bigint", () => {
  assert.equal(providerMachineId("9007199254740993"), "9007199254740993");
  assert.equal(providerMachineId("9223372036854775807"), "9223372036854775807");
  for (const value of [9007199254740993, 123, null, "0", "01", "-1", "1.5", "1e3", " 123", "9223372036854775808"]) assert.equal(providerMachineId(value), null);
  assert.equal(safeProviderInstanceId(Number.MAX_SAFE_INTEGER), "9007199254740991");
  assert.equal(safeProviderInstanceId(Number.MAX_SAFE_INTEGER + 1), null);
});

test("native machineId maps losslessly; conflicting explicit IDs and provider metadata never invent a join", () => {
  const offer = { id: 123, hostId: 456, hostLabel: "Host-789", machineId: "9007199254740993" };
  assert.equal(feedbackOfferMachineId(offer), "9007199254740993");
  assert.equal(feedbackOfferMachineId({ ...offer, providerMachineId: "9007199254740993" }), "9007199254740993");
  assert.equal(feedbackOfferMachineId({ ...offer, providerMachineId: "999" }), null);
  assert.equal(feedbackOfferMachineId({ ...offer, providerMachineId: null }), null);
  assert.equal(feedbackOfferMachineId({ id: 123, hostId: 456, hostLabel: "Host-789" }), null);
  assert.equal(feedbackOfferMachineId({ ...offer, machineId: 9007199254740993 }), null);
});

test("offers deduplicate physical machines into batches of at most 100", () => {
  const ids = Array.from({ length: 215 }, (_, index) => String(index + 1));
  const batches = feedbackMachineBatches([...ids, ...ids, 216, "0", undefined]);
  assert.deepEqual(batches.map((batch) => batch.length), [100, 100, 15]);
  assert.deepEqual(batches.flat(), ids);
});

test("display keeps Vast at zero/one outcomes, replaces at two, including a true 0%", () => {
  for (const value of [summary("1", 0, 0), summary("1", 1, 0)]) assert.equal(displayReliability(0.98, normalizeMachineSummary(value)).source, "Vast");
  assert.deepEqual(displayReliability(0.98, normalizeMachineSummary(summary())), { ratio: 0, source: "No Land", eligibleSessions: 2, smallSample: true });
  assert.equal(displayReliability(0.98, normalizeMachineSummary(summary("1", 1, 1))).ratio, 0.5);
  const malformed = summary(); malformed.reliability.ratio = 0.5;
  assert.equal(displayReliability(0.98, normalizeMachineSummary(malformed)).source, "Vast");
  const wrongCounts = summary(); wrongCounts.reliability.failedSessions = 1;
  assert.equal(displayReliability(0.98, normalizeMachineSummary(wrongCounts)).source, "Vast");
});

test("null, unsupported, partial and invalid jitter remain N/A; measured zero remains zero", () => {
  const measured = { meanMs: 0, minMs: 0, maxMs: 0, sampleCount: 60, contributingSessions: 1,
    highSampleRatio: 0, severeSampleRatio: 0, metricVersion: FEEDBACK_METRIC_VERSION, scope: "all_routes" };
  assert.equal(normalizeMachineSummary({ ...summary(), jitter: measured }).jitter.meanMs, 0);
  for (const jitter of [null, { ...measured, sampleCount: 0 }, { ...measured, meanMs: NaN },
    { ...measured, metricVersion: "rtt_not_jitter" }, { ...measured, completeness: "partial_window" },
    { ...measured, nominalReportIntervalMs: 200 }, { ...measured, highThresholdMs: 3 },
    { ...measured, severeThresholdMs: 10 }, { ...measured, contributingSessions: 61 },
    { ...measured, highSampleRatio: 0, severeSampleRatio: 0.5 }]) assert.equal(normalizeMachineSummary({ ...summary(), jitter }).jitter, null);
});

test("observation dates and compatible route groups survive separately from summary generation time", () => {
  const stats = { meanMs: 0, minMs: 0, maxMs: 0, sampleCount: 60, contributingSessions: 1,
    highSampleRatio: 0, severeSampleRatio: 0, metricVersion: FEEDBACK_METRIC_VERSION,
    firstObservedAt: "2026-09-25T12:00:00Z", lastObservedAt: "2026-09-26T12:00:00Z" };
  const input = summary();
  input.reliability.firstObservedAt = stats.firstObservedAt;
  input.reliability.lastObservedAt = stats.lastObservedAt;
  const result = normalizeMachineSummary({ ...input, jitter: { ...stats, scope: "all_routes" }, jitterGroups: [
    { ...stats, scope: "route", routeMode: "direct" },
    { ...stats, scope: "route", routeMode: "relay", metricVersion: "unsupported" },
    { ...stats, scope: "route", routeMode: "mixed" },
  ] });
  assert.equal(result.reliability.lastObservedAt, stats.lastObservedAt);
  assert.equal(result.jitter.lastObservedAt, stats.lastObservedAt);
  assert.notEqual(result.asOf, result.jitter.lastObservedAt);
  assert.deepEqual(result.jitterGroups.map((group) => group.routeMode), ["direct", "mixed"]);
  assert.equal(result.jitterGroups[0].meanMs, 0);
  assert.equal(feedbackAge("2026-10-26T12:00:00Z", Date.parse("2026-10-26T12:01:00Z")), "1 minute ago");
  assert.equal(feedbackAge("2026-10-25T12:00:00Z", Date.parse("2026-10-26T12:00:00Z")), "1 day ago");
  assert.equal(feedbackAge(null), "age unavailable");
  assert.equal(feedbackAge("invalid"), "age unavailable");
  assert.equal(feedbackAge("2027-01-01", Date.parse("2026-10-26")), "age unavailable");
});

test("enrollment requires actual published HTTPS notices; optional profile is allowlisted and blank-valid", () => {
  const document = { schemaVersion: 1, available: true, collectionPaused: false, privacyVersion: "1", termsVersion: "1",
    privacyUrl: "https://example.invalid/privacy", termsUrl: "https://example.invalid/terms", deletionUrl: "https://example.invalid/delete",
    contact: "Published operator contact", disclosure: "Published disclosure", metrics: [] };
  assert.ok(normalizeProgramDocument(document));
  assert.equal(normalizeProgramDocument({ ...document, privacyUrl: "javascript:alert(1)" }), null);
  assert.equal(safePolicyUrl("https://user:secret@example.invalid/"), null);
  assert.equal(normalizeProgramDocument({ ...document, contact: "" }), null);
  assert.deepEqual(optionalFeedbackProfile(false, { country: "CA" }), {});
  assert.deepEqual(optionalFeedbackProfile(true, { country: "", region: "   " }), {});
  assert.deepEqual(optionalFeedbackProfile(true, { country: " CA ", secret: "must not pass" }), { country: "CA" });
});

test("drafts start empty, accept rating-only/comment-only, reject invalid scores and >2k text", () => {
  assert.equal(normalizeReviewDraft(null, "  "), null);
  assert.deepEqual(normalizeReviewDraft(1, ""), { score: 1, comment: null });
  assert.deepEqual(normalizeReviewDraft(null, "  text  "), { score: null, comment: "text" });
  for (const score of [0, 11, 1.5, NaN]) assert.equal(normalizeReviewDraft(score, "valid comment"), null);
  assert.equal(normalizeReviewDraft(10, "x".repeat(2001)), null);
  assert.ok(normalizeReviewDraft(10, "x".repeat(2000)));
});

test("public reviews remain plain text, preserve pagination and whitelist coarse hardware", () => {
  const page = normalizeReviewsPage({ reviews: [{ reviewId: "33333333-3333-4333-8333-333333333333", score: 7,
    comment: "<script>alert('text only')</script>", publishedDate: "2026-10-26",
    userId: "do not expose", providerInstanceId: "private", hardwareSnapshot: { gpuName: "GPU", rentedGpuCount: 1, country: "CA", ip: "private" } }], nextCursor: "opaque-cursor" });
  assert.equal(page.reviews[0].comment, "<script>alert('text only')</script>");
  assert.equal(page.nextCursor, "opaque-cursor");
  assert.equal("userId" in page.reviews[0], false);
  assert.equal("ip" in page.reviews[0].hardwareSnapshot, false);
  assert.throws(() => normalizeReviewsPage({ reviews: [], nextCursor: 5 }));
});

test("60-second cache expires on reads and viewer/epoch/block context never shares entries", () => {
  const cache = new FeedbackCache();
  const key = feedbackViewerKey(normalizeFeedbackStatus(validStatus()), 0);
  cache.set(key, summary(), 100);
  assert.ok(cache.get(key, 100 + FEEDBACK_CACHE_TTL_MS - 1));
  assert.equal(cache.get(key, 100 + FEEDBACK_CACHE_TTL_MS), undefined);
  assert.notEqual(key, feedbackViewerKey(normalizeFeedbackStatus({ ...validStatus(), consentEpoch: 2 }), 0));
  assert.notEqual(key, feedbackViewerKey(normalizeFeedbackStatus(validStatus()), 1));
  assert.equal(feedbackViewerKey(disabledFeedbackStatus(), 0), null);
  cache.set(key, summary()); cache.clear(); assert.equal(cache.get(key), undefined);
});

test("draft persistence precedes destruction, absent action does not stage, failures and hung persistence cannot veto destroy", async () => {
  const draft = { score: 8, comment: null };
  const events = [];
  await stageFeedbackThenDestroy(draft, async () => { events.push("stage"); }, async () => { events.push("destroy"); }, () => events.push("warn"));
  assert.deepEqual(events, ["stage", "destroy"]);
  events.length = 0;
  await stageFeedbackThenDestroy(null, async () => events.push("stage"), async () => events.push("destroy"), () => events.push("warn"));
  assert.deepEqual(events, ["destroy"]);
  events.length = 0;
  const cancellation = { score: null, comment: null };
  await stageFeedbackThenDestroy(cancellation, async (value) => {
    assert.deepEqual(value, cancellation);
    await new Promise((resolve) => setTimeout(resolve, 5));
    events.push("cleared");
  }, async () => events.push("destroy"), () => events.push("warn"));
  assert.deepEqual(events, ["cleared", "destroy"]);
  events.length = 0;
  await stageFeedbackThenDestroy(cancellation, async () => { throw new Error("disk full"); }, async () => events.push("destroy"), () => events.push("warn"));
  assert.deepEqual(events, ["warn", "destroy"]);
  events.length = 0;
  await stageFeedbackThenDestroy(cancellation, () => new Promise(() => {}), async () => events.push("destroy"), () => events.push("warn"), 5);
  assert.deepEqual(events, ["warn", "destroy"]);
  events.length = 0;
  await stageFeedbackThenDestroy(draft, async () => { throw new Error("disk full"); }, async () => events.push("destroy"), () => events.push("warn"));
  assert.deepEqual(events, ["warn", "destroy"]);
  events.length = 0;
  await stageFeedbackThenDestroy(draft, () => new Promise(() => {}), async () => events.push("destroy"), () => events.push("warn"), 5);
  assert.deepEqual(events, ["warn", "destroy"]);
  await assert.rejects(() => stageFeedbackThenDestroy(draft, async () => {}, async () => { throw new Error("backup failed"); }, () => {}), /backup failed/);
});
