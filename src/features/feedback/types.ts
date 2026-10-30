export type FeedbackEnrollmentStatus =
  | "not_enrolled"
  | "enrolling"
  | "enrolled"
  | "withdrawn"
  | "deletion_pending"
  | "deleted";

export interface FeedbackPendingControl {
  kind: "consent" | "delete";
  requestId: string;
  expectedEpoch: number;
  enabled: boolean;
  optionalContextEnabled: boolean;
  afterAck?: "consent" | "delete" | null;
}

/** Native-owned state: references are not credentials and must never be resolved in JS. */
export interface FeedbackProgramState {
  schemaVersion: 1;
  enabled: boolean;
  enrollmentStatus: FeedbackEnrollmentStatus;
  installationId: string | null;
  userId: string | null;
  credentialRef: string | null;
  deletionCodeRef: string | null;
  consentEpoch: number;
  privacyVersion: string | null;
  termsVersion: string | null;
  consentedAt: string | null;
  optionalContextEnabled: boolean;
  pendingControlAction: FeedbackPendingControl | null;
}

export interface FeedbackCapabilities {
  setupLifecycle: boolean;
  nativeReceivedFrameEvidence: boolean;
  jitter: string | null;
  jitterMetricVersion: string | null;
  jitterUnavailableReason: string | null;
  backgroundDeliveryGuaranteed: boolean;
}

export interface FeedbackProgramStatus extends FeedbackProgramState {
  configured: boolean;
  supported: boolean;
  statusMessage: string | null;
  pendingSessions: number;
  capabilities?: FeedbackCapabilities;
  deletionReceiptId?: string | null;
  deletionStatus?: string | null;
}

export interface FeedbackOptionalProfile {
  country?: string;
  region?: string;
  connectionCategory?: string;
  bandwidthTier?: string;
}

export interface FeedbackEnrollmentRequest {
  privacyVersion: string;
  termsVersion: string;
  participationAccepted: true;
  optionalContextEnabled: boolean;
  optionalProfile: FeedbackOptionalProfile;
}

export interface FeedbackProgramDocument {
  schemaVersion: 1;
  available: boolean;
  collectionPaused: boolean;
  privacyVersion: string;
  termsVersion: string;
  privacyUrl: string;
  termsUrl: string;
  deletionUrl: string;
  contact: string;
  disclosure: string;
  metrics: unknown[];
}

export interface FeedbackReliability {
  source: "no_land";
  ratio: number | null;
  eligibleSessions: number;
  successfulSessions: number;
  failedSessions: number;
  basis: string;
  scope: string;
  firstObservedAt: string | null;
  lastObservedAt: string | null;
}

export interface FeedbackJitterStatistics {
  meanMs: number;
  minMs: number;
  maxMs: number;
  sampleCount: number;
  contributingSessions: number;
  highSampleRatio: number | null;
  severeSampleRatio: number | null;
  metricVersion: string;
  firstObservedAt: string | null;
  lastObservedAt: string | null;
}

export interface FeedbackJitter extends FeedbackJitterStatistics {
  scope: "all_routes";
}

export interface FeedbackJitterGroup extends FeedbackJitterStatistics {
  scope: "route";
  routeMode: "direct" | "relay" | "mixed";
}

export interface FeedbackReviewSummary {
  commentCount: number;
  ratingCount: number;
  averageScore: number | null;
}

export interface MachineFeedbackSummary {
  providerMachineId: string;
  reliability: FeedbackReliability;
  jitter: FeedbackJitter | null;
  jitterGroups: FeedbackJitterGroup[];
  reviews: FeedbackReviewSummary | null;
  asOf: string | null;
}

export interface FeedbackHardwareSnapshot {
  gpuName?: string;
  rentedGpuCount?: number;
  country?: string;
  region?: string;
}

export interface MachineFeedbackReview {
  reviewId: string;
  score: number | null;
  comment: string | null;
  publishedDate: string;
  hardwareSnapshot?: FeedbackHardwareSnapshot;
}

export interface MachineFeedbackReviewsPage {
  reviews: MachineFeedbackReview[];
  nextCursor: string | null;
}

export interface FeedbackReviewDraft {
  score: number | null;
  comment: string | null;
}
