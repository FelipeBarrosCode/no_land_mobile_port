import { useEffect, useRef } from "react";
import type { PersistedAppState } from "../../lib/types";
import { useFeedbackStore } from "./store";

/** Only local status reads are automatic. Published notices require opening the program UI. */
export function FeedbackRuntime({ appState }: { appState: PersistedAppState | null }) {
  const notification = useFeedbackStore((state) => state.notification);
  const previousState = useRef<string | null>(null);
  const stateKey = JSON.stringify([appState?.feedbackProgram, appState?.orchestrationState,
    appState?.instance.instanceId, appState?.provisionedServers.map((server) => [server.instanceId, server.status, server.embeddedMoonlightPaired])]);

  useEffect(() => {
    if (previousState.current !== null && previousState.current !== stateKey) useFeedbackStore.getState().invalidate();
    previousState.current = stateKey;
    void useFeedbackStore.getState().refreshStatus();
  }, [stateKey]);

  useEffect(() => {
    const refresh = () => {
      if (document.visibilityState === "visible") void useFeedbackStore.getState().refreshStatus();
    };
    const interval = window.setInterval(refresh, 30_000);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      window.clearInterval(interval);
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, []);

  return notification ? <aside role="status" className="fixed bottom-[max(1rem,var(--safe-bottom))] left-3 right-3 z-[120] mx-auto flex max-w-xl items-start gap-3 rounded border border-[#ffd78a] bg-[#171c31] p-4 text-base text-[#ffe0a3] shadow-lg">
    <p className="min-w-0 flex-1 break-words">{notification}</p>
    <button type="button" className="min-h-11 shrink-0 px-2 text-neon-cyan underline" aria-label="Dismiss feedback status" onClick={() => useFeedbackStore.getState().notify(null)}>Dismiss</button>
  </aside> : null;
}
