import type {
  FeedbackJitter,
  FeedbackJitterGroup,
  FeedbackJitterStatistics,
  FeedbackOptionalProfile,
  FeedbackPendingControl,
  FeedbackProgramDocument,
  FeedbackProgramStatus,
  FeedbackReviewDraft,
  MachineFeedbackReview,
  MachineFeedbackReviewsPage,
  MachineFeedbackSummary,
} from "./types";

export const FEEDBACK_CACHE_TTL_MS = 60_000;
export const FEEDBACK_BATCH_SIZE = 100;
export const FEEDBACK_COMMENT_LIMIT = 2_000;
export const FEEDBACK_METRIC_VERSION = "rtt_jitter_ewma_window60_report1s_v1";
const MAX_PROVIDER_ID = "9223372036854775807";

export function disabledFeedbackStatus(message: string | null = null): FeedbackProgramStatus {
  return {
    schemaVersion: 1, enabled: false, enrollmentStatus: "not_enrolled",
    installationId: null, userId: null, credentialRef: null, deletionCodeRef: null,
    consentEpoch: 0, privacyVersion: null, termsVersion: null, consentedAt: null,
    optionalContextEnabled: false, pendingControlAction: null,
    configured: false, supported: false, statusMessage: message, pendingSessions: 0,
  };
}

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown> : null;
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function count(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function ratio(value: unknown): value is number {
  return finite(value) && value >= 0 && value <= 1;
}

function timestamp(value: unknown): string | null {
  return text(value) && Number.isFinite(Date.parse(value as string)) ? value as string : null;
}

export function feedbackAge(value: string | null | undefined, now = Date.now()): string {
  const time = value ? Date.parse(value) : NaN;
  if (!Number.isFinite(time) || time > now + 60_000) return "age unavailable";
  const minutes = Math.max(0, Math.floor((now - time) / 60_000));
  if (minutes < 1) return "less than a minute ago";
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? "" : "s"} ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.floor(hours / 24);
  return `${days} day${days === 1 ? "" : "s"} ago`;
}

export function providerMachineId(value: unknown): string | null {
  if (typeof value !== "string" || !/^[1-9]\d{0,18}$/.test(value)) return null;
  if (value.length === MAX_PROVIDER_ID.length && value > MAX_PROVIDER_ID) return null;
  return value;
}

/** The inspected Rust offer mapper emits machineId from Vast machine_id, never host_id/id. */
export function feedbackOfferMachineId(offer: { providerMachineId?: unknown; machineId?: unknown }): string | null {
  const explicit = providerMachineId(offer.providerMachineId);
  const native = providerMachineId(offer.machineId);
  if (explicit && native && explicit !== native) return null;
  return offer.providerMachineId !== undefined ? explicit : native;
}

/** Legacy rental IDs are numbers; reject anything JS can no longer represent exactly. */
export function safeProviderInstanceId(value: unknown): string | null {
  if (typeof value === "string") return providerMachineId(value);
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0 ? String(value) : null;
}

export function feedbackMachineBatches(values: unknown[]): string[][] {
  const ids = [...new Set(values.map(providerMachineId).filter((id): id is string => id !== null))];
  const batches: string[][] = [];
  for (let offset = 0; offset < ids.length; offset += FEEDBACK_BATCH_SIZE) batches.push(ids.slice(offset, offset + FEEDBACK_BATCH_SIZE));
  return batches;
}

function uuid(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
}

function pendingControl(value: unknown): FeedbackPendingControl | null {
  const raw = record(value);
  if (!raw || (raw.kind !== "consent" && raw.kind !== "delete") || !uuid(raw.requestId) ||
      !count(raw.expectedEpoch) || typeof raw.enabled !== "boolean" ||
      typeof raw.optionalContextEnabled !== "boolean" ||
      !(raw.afterAck === undefined || raw.afterAck === null || raw.afterAck === "consent" || raw.afterAck === "delete")) return null;
  return { kind: raw.kind, requestId: raw.requestId as string, expectedEpoch: raw.expectedEpoch,
    enabled: raw.enabled, optionalContextEnabled: raw.optionalContextEnabled,
    afterAck: raw.afterAck as FeedbackPendingControl["afterAck"] };
}

export function normalizeFeedbackStatus(value: unknown): FeedbackProgramStatus {
  const raw = record(value);
  const statuses = ["not_enrolled", "enrolling", "enrolled", "withdrawn", "deletion_pending", "deleted"];
  if (!raw || raw.schemaVersion !== 1 || !statuses.includes(String(raw.enrollmentStatus)) ||
      typeof raw.enabled !== "boolean" || typeof raw.optionalContextEnabled !== "boolean" ||
      !count(raw.consentEpoch) || typeof raw.configured !== "boolean" || typeof raw.supported !== "boolean" ||
      !(raw.pendingControlAction === null || pendingControl(raw.pendingControlAction))) {
    return disabledFeedbackStatus("Feedback is off: native consent state is missing, invalid, or from an unsupported version.");
  }
  const capabilities = record(raw.capabilities);
  const status: FeedbackProgramStatus = {
    ...disabledFeedbackStatus(),
    enrollmentStatus: raw.enrollmentStatus as FeedbackProgramStatus["enrollmentStatus"],
    installationId: text(raw.installationId), userId: text(raw.userId),
    credentialRef: text(raw.credentialRef), deletionCodeRef: text(raw.deletionCodeRef),
    consentEpoch: raw.consentEpoch, privacyVersion: text(raw.privacyVersion), termsVersion: text(raw.termsVersion),
    consentedAt: text(raw.consentedAt), optionalContextEnabled: raw.optionalContextEnabled,
    pendingControlAction: pendingControl(raw.pendingControlAction), configured: raw.configured, supported: raw.supported,
    statusMessage: text(raw.statusMessage), pendingSessions: count(raw.pendingSessions) ? raw.pendingSessions : 0,
    deletionReceiptId: text(raw.deletionReceiptId), deletionStatus: text(raw.deletionStatus),
    capabilities: capabilities ? {
      setupLifecycle: capabilities.setupLifecycle === true,
      nativeReceivedFrameEvidence: capabilities.nativeReceivedFrameEvidence === true,
      jitter: text(capabilities.jitter), jitterMetricVersion: text(capabilities.jitterMetricVersion),
      jitterUnavailableReason: text(capabilities.jitterUnavailableReason),
      backgroundDeliveryGuaranteed: capabilities.backgroundDeliveryGuaranteed === true,
    } : undefined,
  };
  status.enabled = raw.enabled && status.configured && status.supported && status.enrollmentStatus === "enrolled" &&
    status.consentEpoch > 0 && Boolean(uuid(status.userId) && uuid(status.installationId) && status.credentialRef && status.deletionCodeRef && status.privacyVersion &&
      status.termsVersion && status.consentedAt && Number.isFinite(Date.parse(status.consentedAt)));
  return status;
}

export function feedbackReadsEnabled(status: FeedbackProgramStatus): boolean {
  return status.schemaVersion === 1 && status.enabled && status.configured && status.supported &&
    status.enrollmentStatus === "enrolled" && status.consentEpoch > 0 && !status.pendingControlAction;
}

export function feedbackViewerKey(status: FeedbackProgramStatus, visibilityRevision: number): string | null {
  if (!feedbackReadsEnabled(status)) return null;
  return JSON.stringify([status.userId, status.installationId, status.consentEpoch,
    status.privacyVersion, status.termsVersion, FEEDBACK_METRIC_VERSION, visibilityRevision]);
}

export function safePolicyUrl(value: unknown): string | null {
  if (typeof value !== "string") return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" && !url.username && !url.password ? url.href : null;
  } catch { return null; }
}

export function normalizeProgramDocument(value: unknown): FeedbackProgramDocument | null {
  const raw = record(value);
  if (!raw || raw.schemaVersion !== 1 || typeof raw.available !== "boolean" ||
      typeof raw.collectionPaused !== "boolean" || !text(raw.privacyVersion) || !text(raw.termsVersion) ||
      !safePolicyUrl(raw.privacyUrl) || !safePolicyUrl(raw.termsUrl) || !safePolicyUrl(raw.deletionUrl) ||
      !text(raw.contact) || !text(raw.disclosure) || !Array.isArray(raw.metrics)) return null;
  return {
    schemaVersion: 1, available: raw.available, collectionPaused: raw.collectionPaused,
    privacyVersion: raw.privacyVersion as string, termsVersion: raw.termsVersion as string,
    privacyUrl: raw.privacyUrl as string, termsUrl: raw.termsUrl as string,
    deletionUrl: raw.deletionUrl as string, contact: raw.contact as string,
    disclosure: raw.disclosure as string, metrics: raw.metrics,
  };
}

export function optionalFeedbackProfile(enabled: boolean, profile: FeedbackOptionalProfile): FeedbackOptionalProfile {
  if (!enabled) return {};
  const result: FeedbackOptionalProfile = {};
  for (const key of ["country", "region", "connectionCategory", "bandwidthTier"] as const) {
    const value = profile[key]?.trim();
    if (value) result[key] = value;
  }
  return result;
}

function normalizeJitterStatistics(value: unknown): FeedbackJitterStatistics | null {
  const raw = record(value);
  if (!raw || raw.metricVersion !== FEEDBACK_METRIC_VERSION ||
        (raw.nominalReportIntervalMs !== undefined && raw.nominalReportIntervalMs !== 1000) ||
        (raw.completeness !== undefined && raw.completeness !== "full_window") ||
        (raw.thresholdVersion !== undefined && raw.thresholdVersion !== "noland_jitter_bands_v1") ||
        (raw.highThresholdMs !== undefined && raw.highThresholdMs !== 7) ||
        (raw.severeThresholdMs !== undefined && raw.severeThresholdMs !== 15) ||
      !finite(raw.meanMs) || raw.meanMs < 0 || !finite(raw.minMs) || raw.minMs < 0 ||
      !finite(raw.maxMs) || raw.maxMs < raw.minMs || raw.meanMs < raw.minMs || raw.meanMs > raw.maxMs ||
      !count(raw.sampleCount) || raw.sampleCount < 1 || !count(raw.contributingSessions) || raw.contributingSessions < 1 ||
      raw.contributingSessions > raw.sampleCount ||
      (ratio(raw.highSampleRatio) && ratio(raw.severeSampleRatio) && raw.severeSampleRatio > raw.highSampleRatio)) return null;
  return {
    meanMs: raw.meanMs, minMs: raw.minMs, maxMs: raw.maxMs, sampleCount: raw.sampleCount,
    contributingSessions: raw.contributingSessions,
    highSampleRatio: ratio(raw.highSampleRatio) ? raw.highSampleRatio : null,
    severeSampleRatio: ratio(raw.severeSampleRatio) ? raw.severeSampleRatio : null,
    metricVersion: raw.metricVersion,
    firstObservedAt: timestamp(raw.firstObservedAt), lastObservedAt: timestamp(raw.lastObservedAt),
  };
}

function normalizeJitter(value: unknown): FeedbackJitter | null {
  const stats = normalizeJitterStatistics(value);
  return stats && record(value)?.scope === "all_routes" ? { ...stats, scope: "all_routes" } : null;
}

function normalizeJitterGroups(value: unknown): FeedbackJitterGroup[] {
  if (!Array.isArray(value) || value.length > 3) return [];
  const groups: FeedbackJitterGroup[] = [];
  for (const entry of value) {
    const raw = record(entry);
    const stats = normalizeJitterStatistics(raw);
    if (!raw || !stats || raw.scope !== "route" ||
        (raw.routeMode !== "direct" && raw.routeMode !== "relay" && raw.routeMode !== "mixed") ||
        groups.some((group) => group.routeMode === raw.routeMode)) continue;
    groups.push({ ...stats, scope: "route", routeMode: raw.routeMode });
  }
  return groups;
}

export function normalizeMachineSummary(value: unknown): MachineFeedbackSummary | null {
  const raw = record(value);
  const id = providerMachineId(raw?.providerMachineId);
  const reliability = record(raw?.reliability);
  if (!raw || !id || !reliability || reliability.source !== "no_land" ||
      !count(reliability.eligibleSessions) || !count(reliability.successfulSessions) || !count(reliability.failedSessions)) return null;
  const validOutcomeCounts = reliability.successfulSessions + reliability.failedSessions === reliability.eligibleSessions;
  const validRatio = validOutcomeCounts && reliability.eligibleSessions >= 2 && ratio(reliability.ratio) &&
    Math.abs(reliability.ratio - reliability.successfulSessions / reliability.eligibleSessions) < 0.000001 &&
    reliability.basis === "ready_and_paired_before_destroy" && reliability.scope === "retained_history";
  const reviews = record(raw.reviews);
  const validReviews = reviews && count(reviews.commentCount) && count(reviews.ratingCount);
  return {
    providerMachineId: id,
    reliability: {
      source: "no_land", ratio: validRatio ? reliability.ratio as number : null,
      eligibleSessions: reliability.eligibleSessions, successfulSessions: reliability.successfulSessions,
      failedSessions: reliability.failedSessions, basis: text(reliability.basis) ?? "unknown", scope: text(reliability.scope) ?? "unknown",
      firstObservedAt: timestamp(reliability.firstObservedAt), lastObservedAt: timestamp(reliability.lastObservedAt),
    },
    jitter: normalizeJitter(raw.jitter),
    jitterGroups: normalizeJitterGroups(raw.jitterGroups),
    reviews: validReviews ? {
      commentCount: reviews.commentCount as number, ratingCount: reviews.ratingCount as number,
      averageScore: (reviews.ratingCount as number) > 0 && finite(reviews.averageScore) &&
        reviews.averageScore >= 1 && reviews.averageScore <= 10 ? reviews.averageScore : null,
    } : null,
    asOf: timestamp(raw.asOf),
  };
}

export function displayReliability(vastRatio: number, summary: MachineFeedbackSummary | null | undefined) {
  const eligible = summary?.reliability.eligibleSessions ?? 0;
  const communityRatio = summary?.reliability.ratio;
  const community = eligible >= 2 && communityRatio !== null && communityRatio !== undefined && ratio(communityRatio);
  return { ratio: community ? communityRatio : vastRatio, source: community ? "No Land" : "Vast",
    eligibleSessions: eligible, smallSample: eligible > 0 && eligible < 10 };
}

export function normalizeReviewsPage(value: unknown): MachineFeedbackReviewsPage {
  const raw = record(value);
  if (!raw || !Array.isArray(raw.reviews) || raw.reviews.length > 50 ||
      !(raw.nextCursor === null || typeof raw.nextCursor === "string")) throw new Error("Machine reviews are unavailable. Please retry.");
  const reviews: MachineFeedbackReview[] = [];
  for (const value of raw.reviews) {
    const review = record(value);
    if (!review || !uuid(review.reviewId) || !text(review.publishedDate) ||
        !(review.score === null || (count(review.score) && review.score >= 1 && review.score <= 10)) ||
        !(review.comment === null || (typeof review.comment === "string" && review.comment.length <= FEEDBACK_COMMENT_LIMIT))) continue;
    const hardware = record(review.hardwareSnapshot);
    reviews.push({
      reviewId: review.reviewId as string, score: review.score as number | null,
      comment: text(review.comment), publishedDate: (review.publishedDate as string).slice(0, 10),
      hardwareSnapshot: hardware ? {
        gpuName: text(hardware.gpuName) ?? undefined,
        rentedGpuCount: count(hardware.rentedGpuCount) && hardware.rentedGpuCount > 0 ? hardware.rentedGpuCount : undefined,
        country: text(hardware.country) ?? undefined, region: text(hardware.region) ?? undefined,
      } : undefined,
    });
  }
  return { reviews, nextCursor: text(raw.nextCursor) };
}

export function normalizeReviewDraft(score: number | null, comment: string): FeedbackReviewDraft | null {
  if (score !== null && (!Number.isInteger(score) || score < 1 || score > 10)) return null;
  if (comment.length > FEEDBACK_COMMENT_LIMIT) return null;
  const normalizedComment = comment.trim() || null;
  return score !== null || normalizedComment !== null ? { score, comment: normalizedComment } : null;
}

/** Small bounded memory-only cache. Expiry is checked on reads, not just on a timer. */
export class FeedbackCache<T> {
  private entries = new Map<string, { expiresAt: number; value: T }>();
  get(key: string, now = Date.now()): T | undefined {
    const entry = this.entries.get(key);
    if (!entry) return undefined;
    if (entry.expiresAt <= now) { this.entries.delete(key); return undefined; }
    return entry.value;
  }
  set(key: string, value: T, now = Date.now()): void {
    this.entries.delete(key);
    this.entries.set(key, { value, expiresAt: now + FEEDBACK_CACHE_TTL_MS });
    if (this.entries.size > 500) this.entries.delete(this.entries.keys().next().value as string);
  }
  clear(): void { this.entries.clear(); }
}

/** No feedback error may veto the established backup/provider destruction path. */
export async function stageFeedbackThenDestroy(
  draft: FeedbackReviewDraft | null,
  stage: (draft: FeedbackReviewDraft) => Promise<unknown>,
  destroy: () => Promise<void>,
  onPersistenceFailure: () => void,
  persistenceTimeoutMs = 3_000,
): Promise<void> {
  if (draft) {
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        stage(draft),
        new Promise<never>((_, reject) => {
          timeout = setTimeout(() => reject(new Error("Feedback persistence timed out")), persistenceTimeoutMs);
        }),
      ]);
    } catch { onPersistenceFailure(); }
    finally { if (timeout !== undefined) clearTimeout(timeout); }
  }
  await destroy();
}
