import type { MachineFeedbackSummary } from "./types";
import { displayReliability, feedbackAge, providerMachineId } from "./model";

export function MachineFeedbackMetrics({ vastReliability, machineId, summary, enabled, loading, unavailable, onReviews }: {
  vastReliability: number;
  machineId: string | null | undefined;
  summary: MachineFeedbackSummary | undefined;
  enabled: boolean;
  loading: boolean;
  unavailable: boolean;
  onReviews: () => void;
}) {
  const display = displayReliability(vastReliability, enabled ? summary : null);
  const validMachine = providerMachineId(machineId);
  const jitter = summary?.jitter;
  const count = summary?.reviews?.commentCount;
  const explanation = !validMachine ? "No native machine ID is available; no history is inferred from an offer or host." : unavailable && !summary
    ? "Community feedback is unavailable. Vast reliability and Rent remain available. Open reviews to retry."
    : "Only retained, valid reported rentals count. Setup means ready and paired before destruction; abandoned setups can count as failures. This differs from Vast uptime/health reliability.";
  return <div className="mt-3 grid min-w-0 grid-cols-1 gap-3 border-t border-[#3e4270] pt-3 text-base text-[#c6dbf4] min-[360px]:grid-cols-2">
    <div title={explanation} className="min-w-0">
      <p>Reliability <strong>{Number.isFinite(display.ratio) ? `${(display.ratio * 100).toFixed(1)}%` : "N/A"}</strong></p>
      <p className="text-sm text-[#9ad9ff]">{display.source}{enabled && summary ? ` · ${display.eligibleSessions} eligible session${display.eligibleSessions === 1 ? "" : "s"}` : ""}</p>
      {enabled && summary && display.source === "Vast" && <p className="text-sm text-[#9ec4df]">{display.eligibleSessions < 2 ? "No Land: fewer than 2 outcomes" : "No valid No Land ratio"}</p>}
      {enabled && summary && display.smallSample && <p className="text-sm text-[#ffd78a]">Small sample</p>}
      {enabled && summary && <p className="text-sm text-[#9ec4df]">{summary.reliability.lastObservedAt
        ? <>Latest setup outcome: <time dateTime={summary.reliability.lastObservedAt} title={new Date(summary.reliability.lastObservedAt).toLocaleString()}>{feedbackAge(summary.reliability.lastObservedAt)}</time></>
        : "Setup observation age unavailable"}</p>}
      {enabled && !summary && <p className="text-sm text-[#9ec4df]">{!validMachine ? "Machine history N/A" : unavailable ? "Community unavailable" : loading ? "Loading community data…" : "Community N/A"}</p>}
    </div>
    {enabled && <div className="min-w-0" title={jitter ? "Sample-weighted RTT jitter EWMA over full 60-second windows, 1-second reports, all routes. Reflects the client and network path as well as the machine, not just host quality." : "No valid comparable jitter observations. Short streams and unsupported measurements remain N/A, not zero."}>
      <p>Jitter <strong>{jitter ? `${jitter.meanMs.toFixed(2)} ms` : "N/A"}</strong></p>
      {jitter ? <p className="text-sm text-[#9ec4df]">No Land · {jitter.sampleCount} observations · {jitter.contributingSessions} session{jitter.contributingSessions === 1 ? "" : "s"} · all routes</p> : <p className="text-sm text-[#9ec4df]">No valid observations</p>}
    </div>}
    {enabled && summary && <p className="col-span-full text-sm text-[#9ec4df]">Summary updated {summary.asOf
      ? <time dateTime={summary.asOf} title={new Date(summary.asOf).toLocaleString()}>{feedbackAge(summary.asOf)}</time>
      : "at an unavailable time"}. Updates are not new observations.</p>}
    {enabled && <button
      type="button"
      className="col-span-full flex min-h-11 min-w-0 items-center justify-between gap-2 rounded border border-[#495188] px-3 py-2 text-left text-neon-cyan focus-visible:outline focus-visible:outline-2 focus-visible:outline-neon-cyan disabled:opacity-60"
      disabled={!validMachine}
      aria-label={`View machine reviews${count !== undefined ? `, ${count} comments` : ", comments unavailable"}`}
      title={explanation}
      onClick={(event) => { event.preventDefault(); event.stopPropagation(); onReviews(); }}
      onKeyDown={(event) => event.stopPropagation()}
    >
      <span className="flex min-w-0 items-center gap-2"><svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" className="size-5 shrink-0"><path d="M4 4h16v12H9l-5 4V4Z" /><path d="M8 8h8M8 12h6" /></svg>Reviews</span>
      <span className="text-sm">{count === undefined ? "N/A" : count === 0 ? "N/A · 0 comments" : `${count} comment${count === 1 ? "" : "s"}`}</span>
    </button>}
  </div>;
}
