import { invokeSafe } from "../../lib/tauri";
import { normalizeFeedbackStatus, normalizeProgramDocument, normalizeReviewsPage } from "./model";
import type { FeedbackEnrollmentRequest, FeedbackReviewDraft } from "./types";

// The native service alone owns authorization, transport, persistence and final privacy gates.
export const feedbackNative = {
  async status() {
    return normalizeFeedbackStatus(await invokeSafe<unknown>("feedback_program_status"));
  },
  async notices() {
    const document = normalizeProgramDocument(await invokeSafe<unknown>("feedback_program_notices"));
    if (!document) throw new Error("Published feedback notices are unavailable or incomplete. Enrollment is off.");
    return document;
  },
  async enroll(request: FeedbackEnrollmentRequest) {
    return normalizeFeedbackStatus(await invokeSafe<unknown>("feedback_program_enroll", { request }));
  },
  async consent(enabled: boolean, optionalContextEnabled: boolean) {
    return normalizeFeedbackStatus(await invokeSafe<unknown>("feedback_program_consent", { enabled, optionalContextEnabled }));
  },
  async deleteContributions() {
    return normalizeFeedbackStatus(await invokeSafe<unknown>("feedback_program_delete"));
  },
  summaries(machineIds: string[]) {
    return invokeSafe<{ summaries: unknown[] }>("feedback_machine_summaries", { machineIds });
  },
  async reviews(machineId: string, cursor: string | null) {
    return normalizeReviewsPage(await invokeSafe<unknown>("feedback_machine_reviews", { machineId, cursor, limit: 20 }));
  },
  report(reviewId: string, reason: string) {
    return invokeSafe<unknown>("feedback_review_report", { reviewId, reason });
  },
  blockAuthor(reviewId: string) {
    return invokeSafe<unknown>("feedback_review_block_author", { reviewId });
  },
  async eligible(providerInstanceId: string) {
    const result = await invokeSafe<{ eligible: boolean }>("feedback_review_eligibility", { providerInstanceId });
    return result?.eligible === true;
  },
  stageReview(providerInstanceId: string, draft: FeedbackReviewDraft) {
    return invokeSafe<unknown>("feedback_stage_review", { providerInstanceId, score: draft.score, comment: draft.comment });
  },
};
