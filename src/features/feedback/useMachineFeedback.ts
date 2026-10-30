import { useEffect, useMemo, useState } from "react";
import { feedbackMachineBatches, feedbackReadsEnabled } from "./model";
import { useFeedbackStore } from "./store";

export function useMachineFeedback(machineIds: unknown[], active: boolean) {
  const status = useFeedbackStore((state) => state.status);
  const visibilityRevision = useFeedbackStore((state) => state.visibilityRevision);
  useFeedbackStore((state) => state.cacheRevision);
  const [loading, setLoading] = useState(false);
  const [unavailable, setUnavailable] = useState(false);
  const [, setClock] = useState(0);
  const [retry, setRetry] = useState(0);
  const requestKey = JSON.stringify(feedbackMachineBatches(machineIds).flat().sort());
  const ids = useMemo<string[]>(() => JSON.parse(requestKey) as string[], [requestKey]);
  const enabled = feedbackReadsEnabled(status);

  useEffect(() => {
    setLoading(false);
    setUnavailable(false);
    if (!active || !enabled || ids.length === 0) return;
    let cancelled = false;
    let running = false;
    let retryAfter = 0;
    async function load() {
      if (running || document.visibilityState === "hidden") return;
      setClock((clock) => clock + 1);
      if (Date.now() < retryAfter || ids.every((id) => useFeedbackStore.getState().summary(id))) return;
      running = true;
      setLoading(true);
      try {
        await useFeedbackStore.getState().summaries(ids);
        if (!cancelled) setUnavailable(false);
      } catch {
        retryAfter = Date.now() + 60_000;
        if (!cancelled) setUnavailable(true);
      }
      finally {
        running = false;
        if (!cancelled) setLoading(false);
      }
    }
    void load();
    // Reads check expiry; this timer also removes expired values from an otherwise idle screen.
    const interval = window.setInterval(() => { if (!cancelled) void load(); }, 5_000);
    const foreground = () => { if (document.visibilityState === "visible") void load(); };
    document.addEventListener("visibilitychange", foreground);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
      document.removeEventListener("visibilitychange", foreground);
    };
  }, [active, enabled, ids, visibilityRevision, retry]);

  return {
    enabled, loading: active && enabled && loading, unavailable: active && enabled && unavailable,
    summary: useFeedbackStore.getState().summary,
    retry: () => setRetry((value) => value + 1),
  };
}
