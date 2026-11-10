import { translate } from "../../lib/i18n";
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
      <p>{translate("generated.71d0acd465564246")} <strong>{Number.isFinite(display.ratio) ? `${(display.ratio * 100).toFixed(1)}%` : translate("feedback.e2f79e5b60330bba")}</strong></p>
      <p className="text-sm text-[#9ad9ff]">{display.source}{enabled && summary ? ` · ${display.eligibleSessions} eligible session${display.eligibleSessions === 1 ? "" : translate("generated.043a718774c572bd")}` : ""}</p>
      {enabled && summary && display.source === "Vast" && <p className="text-sm text-[#9ec4df]">{display.eligibleSessions < 2 ? translate("feedback.9a049522eacc07bb") : translate("feedback.fb4fb32c41e6711f")}</p>}
      {enabled && summary && display.smallSample && <p className="text-sm text-[#ffd78a]">{translate("feedback.b951a6a92be4f1e7")}</p>}
      {enabled && summary && <p className="text-sm text-[#9ec4df]">{summary.reliability.lastObservedAt
        ? <>{translate("feedback.fbc9a84e9564ec0a")} <time dateTime={summary.reliability.lastObservedAt} title={new Date(summary.reliability.lastObservedAt).toLocaleString()}>{feedbackAge(summary.reliability.lastObservedAt)}</time></>
        : translate("feedback.d377b78c3131156e")}</p>}
      {enabled && !summary && <p className="text-sm text-[#9ec4df]">{!validMachine ? translate("feedback.5847671291b9b381") : unavailable ? translate("feedback.f544fc0d9bccb3a0") : loading ? translate("feedback.1fc9a555b9f844b0") : translate("feedback.83c2394cae63dbce")}</p>}
    </div>
    {enabled && <div className="min-w-0" title={jitter ? translate("feedback.faf3aab250181725") : translate("feedback.0b306b724b27fcc3")}>
      <p>{translate("generated.91fbe4c855fbe075")} <strong>{jitter ? `${jitter.meanMs.toFixed(2)} ms` : translate("feedback.e2f79e5b60330bba")}</strong></p>
      {jitter ? <p className="text-sm text-[#9ec4df]">{translate("feedback.19f0c53a08f60727")} {jitter.sampleCount} {translate("feedback.90906734c10cad44")} {jitter.contributingSessions} {translate("feedback.3f3af1ecebbd1410")}{jitter.contributingSessions === 1 ? "" : translate("generated.043a718774c572bd")} {translate("feedback.7e998d89d2271f45")}</p> : <p className="text-sm text-[#9ec4df]">{translate("feedback.6cba6bc2f5eb6105")}</p>}
    </div>}
    {enabled && summary && <p className="col-span-full text-sm text-[#9ec4df]">{translate("feedback.0661145fa5a4bb38")} {summary.asOf
      ? <time dateTime={summary.asOf} title={new Date(summary.asOf).toLocaleString()}>{feedbackAge(summary.asOf)}</time>
      : translate("feedback.1e2845b93d3f6298")}{translate("feedback.2c9303412393817d")}</p>}
    {enabled && <button
      type="button"
      className="col-span-full flex min-h-11 min-w-0 items-center justify-between gap-2 rounded border border-[#495188] px-3 py-2 text-left text-neon-cyan focus-visible:outline focus-visible:outline-2 focus-visible:outline-neon-cyan disabled:opacity-60"
      disabled={!validMachine}
      aria-label={translate("feedback.52d8d8cf6edf0da3", { value0: count !== undefined ? `, ${count} comments` : translate("feedback.commentsUnavailable") })}
      title={explanation}
      onClick={(event) => { event.preventDefault(); event.stopPropagation(); onReviews(); }}
      onKeyDown={(event) => event.stopPropagation()}
    >
      <span className="flex min-w-0 items-center gap-2"><svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" className="size-5 shrink-0"><path d="M4 4h16v12H9l-5 4V4Z" /><path d="M8 8h8M8 12h6" /></svg>{translate("feedback.84cb7871b741c32e")}</span>
      <span className="text-sm">{count === undefined ? translate("feedback.e2f79e5b60330bba") : count === 0 ? translate("feedback.b15c7d09194b2fb7") : `${count} comment${count === 1 ? "" : translate("generated.043a718774c572bd")}`}</span>
    </button>}
  </div>;
}
