import { create } from "zustand";
import { feedbackNative } from "./api";
import {
  disabledFeedbackStatus, FeedbackCache, feedbackMachineBatches, feedbackReadsEnabled,
  feedbackViewerKey, normalizeMachineSummary, providerMachineId,
} from "./model";
import type {
  FeedbackEnrollmentRequest, FeedbackProgramDocument, FeedbackProgramStatus,
  MachineFeedbackReviewsPage, MachineFeedbackSummary,
} from "./types";

const summaryCache = new FeedbackCache<MachineFeedbackSummary>();

const summaryRequests = new Map<string, Promise<void>>();
const reviewRequests = new Map<string, Promise<MachineFeedbackReviewsPage>>();

interface FeedbackStore {
  status: FeedbackProgramStatus;
  initialized: boolean;
  busy: boolean;
  visibilityRevision: number;
  cacheRevision: number;
  notification: string | null;
  refreshStatus: () => Promise<FeedbackProgramStatus>;
  acceptStatus: (status: FeedbackProgramStatus) => void;
  loadNotices: () => Promise<FeedbackProgramDocument>;
  enroll: (request: FeedbackEnrollmentRequest) => Promise<void>;
  consent: (enabled: boolean, optionalContextEnabled: boolean) => Promise<void>;
  deleteContributions: () => Promise<void>;
  invalidate: () => void;
  notify: (message: string | null) => void;
  summaries: (machineIds: unknown[]) => Promise<void>;
  summary: (machineId: string) => MachineFeedbackSummary | undefined;
  reviews: (machineId: string, cursor: string | null) => Promise<MachineFeedbackReviewsPage>;
  report: (reviewId: string, reason: string) => Promise<void>;
  blockAuthor: (reviewId: string) => Promise<void>;
}

export const useFeedbackStore = create<FeedbackStore>((set, get) => {
  let statusRequest: Promise<FeedbackProgramStatus> | null = null;
  let controlInFlight = false;
  let generation = 0;
  let controlGeneration = 0;
  const viewer = () => feedbackViewerKey(get().status, get().visibilityRevision);
  const requireViewer = () => {
    const key = viewer();
    if (!key || controlInFlight) throw new Error("Feedback is off or a consent change is pending.");
    return key;
  };
  const clear = () => {
    generation += 1;
    summaryCache.clear();

    summaryRequests.clear();
    reviewRequests.clear();
    set((state) => ({ visibilityRevision: state.visibilityRevision + 1, cacheRevision: state.cacheRevision + 1 }));
  };
  const control = async (action: () => Promise<FeedbackProgramStatus>, stopLocally: boolean) => {
    if (controlInFlight) throw new Error("A feedback control change is already in progress.");
    controlInFlight = true;
    controlGeneration += 1;
    set({ busy: true });
    if (stopLocally) set((state) => ({ status: { ...state.status, enabled: false } }));
    clear();
    try {
      get().acceptStatus(await action());
    } catch {
      // A lost control response never re-enables reads. Native retains durable retry/authentication state.
      set((state) => ({ status: { ...state.status, enabled: false } }));
      throw new Error("Feedback change could not be confirmed. Check native status; renting and streaming are unaffected.");
    } finally {
      controlInFlight = false;
      set({ busy: false });
    }
  };
  return {
    status: disabledFeedbackStatus(), initialized: false, busy: false,
    visibilityRevision: 0, cacheRevision: 0, notification: null,
    refreshStatus: () => {
      if (statusRequest) return statusRequest;
      if (controlInFlight) return Promise.resolve(get().status);
      const startedControlGeneration = controlGeneration;
      statusRequest = feedbackNative.status().then((status) => {
        if (!controlInFlight && startedControlGeneration === controlGeneration) get().acceptStatus(status);
        return get().status;
      }).catch(() => {
        const status = disabledFeedbackStatus("Native feedback service is unavailable. Feedback is off; renting and streaming still work.");
        if (!controlInFlight && startedControlGeneration === controlGeneration) get().acceptStatus(status);
        return get().status;
      }).finally(() => { statusRequest = null; });
      return statusRequest;
    },
    acceptStatus: (status) => {
      const previous = get().status;
      if (feedbackViewerKey(previous, 0) !== feedbackViewerKey(status, 0) ||
          JSON.stringify(previous.pendingControlAction) !== JSON.stringify(status.pendingControlAction) || previous.pendingSessions !== status.pendingSessions ||
          previous.enrollmentStatus !== status.enrollmentStatus || previous.optionalContextEnabled !== status.optionalContextEnabled) clear();
      set({ status, initialized: true });
    },
    loadNotices: () => feedbackNative.notices(),
    enroll: (request) => control(() => feedbackNative.enroll(request), false),
    consent: (enabled, optionalContextEnabled) => control(() => feedbackNative.consent(enabled, optionalContextEnabled), !enabled),
    deleteContributions: () => control(() => feedbackNative.deleteContributions(), true),
    invalidate: clear,
    notify: (notification) => set({ notification }),
    summary: (machineId) => {
      const key = viewer();
      return key ? summaryCache.get(JSON.stringify([key, machineId])) : undefined;
    },
    summaries: async (values) => {
      const key = requireViewer();
      const startedGeneration = generation;
      const waiting = new Set<Promise<void>>();
      const missing = feedbackMachineBatches(values).flat().filter((id) => {
        const cacheKey = JSON.stringify([key, id]);
        if (summaryCache.get(cacheKey)) return false;
        const pending = summaryRequests.get(cacheKey);
        if (pending) { waiting.add(pending); return false; }
        return true;
      });
      for (const ids of feedbackMachineBatches(missing)) {
        if (viewer() !== key || generation !== startedGeneration || controlInFlight) return;
        const pending: Promise<void> = feedbackNative.summaries(ids).then((response) => {
            if (viewer() !== key || generation !== startedGeneration || controlInFlight) return;
            if (!Array.isArray(response?.summaries)) throw new Error("Machine summaries are unavailable.");
            const requested = new Set(ids);
            const received = new Set<string>();
            for (const raw of response.summaries) {
              const summary = normalizeMachineSummary(raw);
              if (!summary || !requested.has(summary.providerMachineId)) continue;
              received.add(summary.providerMachineId);
              summaryCache.set(JSON.stringify([key, summary.providerMachineId]), summary);
            }
            set((state) => ({ cacheRevision: state.cacheRevision + 1 }));
            if (received.size !== requested.size) throw new Error("Some machine summaries are unavailable.");
          }).finally(() => {
            for (const id of ids) {
              const cacheKey = JSON.stringify([key, id]);
              if (summaryRequests.get(cacheKey) === pending) summaryRequests.delete(cacheKey);
            }
          });
        for (const id of ids) summaryRequests.set(JSON.stringify([key, id]), pending);
        waiting.add(pending);
      }
      await Promise.all(waiting);
    },
    reviews: async (machineId, cursor) => {
      const key = requireViewer();
      if (!providerMachineId(machineId)) throw new Error("A native machine identifier is required.");
      const startedGeneration = generation;
      const cacheKey = JSON.stringify([key, machineId, cursor]);

      const existing = reviewRequests.get(cacheKey);
      if (existing) return existing;
      const pending = feedbackNative.reviews(machineId, cursor).then((page) => {
        if (viewer() !== key || generation !== startedGeneration || controlInFlight) throw new Error("Feedback visibility changed. Refresh the reviews.");

        return page;
      }).finally(() => { if (reviewRequests.get(cacheKey) === pending) reviewRequests.delete(cacheKey); });
      reviewRequests.set(cacheKey, pending);
      return pending;
    },
    report: async (reviewId, reason) => {
      requireViewer();
      try { await feedbackNative.report(reviewId, reason); } finally { clear(); }
    },
    blockAuthor: async (reviewId) => {
      requireViewer();
      try { await feedbackNative.blockAuthor(reviewId); } finally { clear(); }
    },
  };
});

export function feedbackIsEnabled(): boolean {
  return feedbackReadsEnabled(useFeedbackStore.getState().status);
}
