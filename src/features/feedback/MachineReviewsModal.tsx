import { translate } from "../../lib/i18n";
import { useEffect, useRef, useState } from "react";
import { Button } from "../../components/ui/Button";
import type { OfferCandidate } from "../../lib/types";
import { FeedbackDialog } from "./FeedbackDialog";
import { FEEDBACK_CACHE_TTL_MS, feedbackAge, feedbackOfferMachineId, feedbackReadsEnabled } from "./model";
import { useFeedbackStore } from "./store";
import { useMachineFeedback } from "./useMachineFeedback";
import type { MachineFeedbackReview } from "./types";

export function MachineReviewsModal({ offer, onClose }: { offer: OfferCandidate; onClose: () => void }) {
  const machineId = feedbackOfferMachineId(offer);
  const status = useFeedbackStore((state) => state.status);
  const visibilityRevision = useFeedbackStore((state) => state.visibilityRevision);
  const [reviews, setReviews] = useState<(MachineFeedbackReview & { expiresAt: number })[]>([]);
  const [expired, setExpired] = useState(false);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [reporting, setReporting] = useState<string | null>(null);
  const [reason, setReason] = useState("");
  const [blocking, setBlocking] = useState<string | null>(null);
  const [moderating, setModerating] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [reviewVisibility, setReviewVisibility] = useState(-1);
  const [now, setNow] = useState(Date.now);
  const requestSequence = useRef(0);
  const fetchedAt = useRef(0);
  const seenCursors = useRef(new Set<string>());
  const machine = useMachineFeedback([machineId], true);
  const summary = machineId ? machine.summary(machineId) : undefined;
  const enabled = feedbackReadsEnabled(status);

  async function load(cursor: string | null) {
    if (!machineId || !enabled) return;
    const sequence = ++requestSequence.current;
    const startVisibility = useFeedbackStore.getState().visibilityRevision;
    setLoading(true);
    setError(null);
    if (!cursor) { setReviews([]); setNextCursor(null); setLoaded(false); setExpired(false); seenCursors.current.clear(); }
    try {
      const page = await useFeedbackStore.getState().reviews(machineId, cursor);
      if (sequence !== requestSequence.current || startVisibility !== useFeedbackStore.getState().visibilityRevision) return;
      if (!cursor) fetchedAt.current = Date.now();
      if (cursor) seenCursors.current.add(cursor);
      setReviews((previous) => {
        const known = new Set((cursor ? previous : []).map((review) => review.reviewId));
        const additions = page.reviews.filter((review) => {
          if (known.has(review.reviewId)) return false;
          known.add(review.reviewId);
          return true;
        }).map((review) => ({ ...review, expiresAt: Date.now() + FEEDBACK_CACHE_TTL_MS }));
        return [...(cursor ? previous : []), ...additions];
      });
      setNextCursor(page.nextCursor && !seenCursors.current.has(page.nextCursor) ? page.nextCursor : null);
      setReviewVisibility(startVisibility);
      setLoaded(true);
    } catch {
      if (sequence === requestSequence.current) setError(translate("feedback.51ee944410425b8b"));
    } finally { if (sequence === requestSequence.current) setLoading(false); }
  }

  useEffect(() => {
    setReviews([]); setLoaded(false); setNextCursor(null);
    if (enabled) void load(null);
    return () => { requestSequence.current += 1; };
  }, [machineId, enabled, visibilityRevision, refresh]);

  useEffect(() => {
    const expire = () => {
      setNow(Date.now());
      if (fetchedAt.current && Date.now() - fetchedAt.current >= FEEDBACK_CACHE_TTL_MS) {
        setExpired(true);
        setReviews((previous) => {
          const current = previous.filter((review) => review.expiresAt > Date.now());
          return current.length === previous.length ? previous : current;
        });
      }
    };
    const interval = window.setInterval(expire, 1_000);
    document.addEventListener("visibilitychange", expire);
    return () => { window.clearInterval(interval); document.removeEventListener("visibilitychange", expire); };
  }, []);

  async function moderate(action: "report" | "block", reviewId: string) {
    setModerating(true); setError(null); setMessage(null);
    try {
      if (action === "report") await useFeedbackStore.getState().report(reviewId, reason);
      else await useFeedbackStore.getState().blockAuthor(reviewId);
      setReporting(null); setReason(""); setBlocking(null);
      setMessage(action === "report" ? "Report received. Published visibility is controlled by moderation." : "Contributor blocked. Their reviews and rating counts are being refreshed for this viewer; valid shared setup/jitter observations are not removed by a block.");
    } catch { setError(translate("feedback.893d3564e7702077")); }
    finally { setModerating(false); }
  }

  const currentVisibility = enabled && reviewVisibility === visibilityRevision;
  const visibleReviews = currentVisibility ? reviews.filter((review) => review.expiresAt > now) : [];
  const jitter = summary?.jitter;
  return <FeedbackDialog title={translate("feedback.45eaa437b9e41d66")} onClose={onClose}>
    <section className="space-y-2">
      <h3 className="break-words font-display text-sm text-neon-cyan">{offer.gpuName} · {offer.locationLabel}</h3>
      <p>{translate("feedback.28ba2b6df85dac01")} {machineId ?? "N/A"} {translate("feedback.fb5d0f525fafba9e")} {offer.gpuCount} {translate("generated.ea49523d744c40bc")}{offer.gpuCount === 1 ? "" : translate("generated.043a718774c572bd")}{translate("feedback.03e1b90720f2884c")}</p>
      <p>{translate("feedback.9f29530464f730bd")} {summary?.reviews?.averageScore !== null && summary?.reviews?.averageScore !== undefined ? `${summary.reviews.averageScore.toFixed(1)}/10` : translate("feedback.e2f79e5b60330bba")} · {summary?.reviews?.ratingCount ?? "N/A"} {translate("feedback.61ebf9379940820f")} {summary?.reviews?.commentCount ?? "N/A"} {translate("feedback.7bf260802ba1f315")}</p>
      <p className="text-sm">{translate("feedback.ab635c7695fd40c8")}</p>
      {summary?.asOf && <p className="text-sm">{translate("feedback.0661145fa5a4bb38")} {feedbackAge(summary.asOf)} ({new Date(summary.asOf).toLocaleString()}{translate("feedback.2f0a256a844dce1f")}</p>}
      <details className="rounded border border-[#495188] p-3">
        <summary className="min-h-11 cursor-pointer text-neon-cyan">{translate("feedback.a24ae9a773e43a12")}</summary>
        <div className="mt-2 space-y-2">
          <p>{translate("feedback.561aec25c60a283c")} {summary?.reliability.ratio !== null && summary?.reliability.ratio !== undefined ? `${(summary.reliability.ratio * 100).toFixed(1)}%` : translate("feedback.e2f79e5b60330bba")} · {summary?.reliability.eligibleSessions ?? 0} {translate("feedback.76e11336780af857")}{summary?.reliability.successfulSessions ?? 0} {translate("feedback.578c1d3df7db0434")} {summary?.reliability.failedSessions ?? 0} {translate("feedback.55076dc00fb5e5a6")}</p>
          <p>{translate("feedback.2ccb710ad7c5d2c4")}</p>
          <p>{translate("feedback.c6b76330feb1f75e")} {Number.isFinite(offer.reliability) ? `${(offer.reliability * 100).toFixed(1)}%` : translate("feedback.e2f79e5b60330bba")} {translate("feedback.55f299ba8ffac877")}</p>
          <p>{translate("feedback.466d8653f54e43d0")} {summary?.reliability.firstObservedAt ? new Date(summary.reliability.firstObservedAt).toLocaleDateString() : translate("feedback.e2f79e5b60330bba")} – {summary?.reliability.lastObservedAt ? new Date(summary.reliability.lastObservedAt).toLocaleDateString() : translate("feedback.e2f79e5b60330bba")}{translate("feedback.6ad4bf3c5162bc55")} {feedbackAge(summary?.reliability.lastObservedAt)}{translate("feedback.06d550be602bcdd7")}</p>
          {jitter ? <>
            <p>{translate("feedback.a48501efccb55d4c")} {jitter.meanMs.toFixed(2)} {translate("feedback.d1a67143f419833d")} {jitter.minMs.toFixed(2)} {translate("feedback.a8723b9acc64565b")} {jitter.maxMs.toFixed(2)} {translate("feedback.7203edc226706aee")} {jitter.sampleCount} {translate("feedback.5c0dfd1f5951f3f0")} {jitter.contributingSessions} {translate("feedback.a2f649838b39e193")}</p>
            <p>{translate("feedback.24e8a78abfc68665")} {jitter.highSampleRatio === null ? translate("feedback.e2f79e5b60330bba") : `${(jitter.highSampleRatio * 100).toFixed(1)}%`} {translate("feedback.eb98f06b1ad7285b")} {jitter.severeSampleRatio === null ? translate("feedback.e2f79e5b60330bba") : `${(jitter.severeSampleRatio * 100).toFixed(1)}%`}.</p>
            <p>{translate("feedback.1804c28b1f59362d")} {jitter.firstObservedAt ? new Date(jitter.firstObservedAt).toLocaleDateString() : translate("feedback.e2f79e5b60330bba")} – {jitter.lastObservedAt ? new Date(jitter.lastObservedAt).toLocaleDateString() : translate("feedback.e2f79e5b60330bba")}{translate("feedback.60757b9f1917b5b7")} {feedbackAge(jitter.lastObservedAt)}.</p>
          </> : <p>{translate("feedback.a4173e5e0f71662a")}</p>}
          {(summary?.jitterGroups.length ?? 0) > 0 && <ul className="space-y-2" aria-label={translate("feedback.33d9c9852134751e")}>
            {summary?.jitterGroups.map((group) => <li key={group.routeMode}>{group.routeMode}: {group.meanMs.toFixed(2)} {translate("feedback.ef68be677f033a68")} {group.sampleCount} {translate("feedback.90906734c10cad44")} {group.contributingSessions} {translate("feedback.94111c07c21011ba")} {feedbackAge(group.lastObservedAt)}</li>)}
          </ul>}
          <p className="text-sm">{translate("feedback.846c37ab52486331")}</p>
          <p className="break-all text-sm">{translate("feedback.172f9f804400b247")}</p>
        </div>
      </details>
    </section>
    {!enabled ? <p role="status">{translate("feedback.87e0f0782703f42a")}</p> : <section className="space-y-4" aria-label={translate("feedback.cf271871770e94ad")} aria-busy={loading}>
      {loaded && currentVisibility && !expired && visibleReviews.length === 0 && <p>{translate("feedback.ca657232fa3104a0")}</p>}
      {expired && <p className="text-sm text-[#ffd78a]">{translate("feedback.9b249a3b6f98f7b5")}</p>}
      {visibleReviews.map((review) => {
        const hardware = review.hardwareSnapshot;
        const coarseDate = new Date(`${review.publishedDate.slice(0, 10)}T12:00:00Z`);
        return <article key={review.reviewId} className="space-y-3 rounded border border-[#495188] bg-[#0b0f23] p-3">
          <p><strong>{review.score !== null ? `${review.score}/10` : translate("feedback.936f84ca441362e4")}</strong> · {Number.isFinite(coarseDate.getTime()) ? coarseDate.toLocaleDateString() : translate("feedback.7de57f90e41f8206")}</p>
          {hardware && <p className="break-words text-sm text-[#9ec4df]">{translate("feedback.844f5c3cc9856cfe")} {[hardware.gpuName, hardware.rentedGpuCount ? `${hardware.rentedGpuCount} GPU(s)` : null, hardware.region, hardware.country].filter(Boolean).join(" · ") || "coarse hardware unavailable"}</p>}
          <p className="whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{review.comment ?? "N/A — Rating only; no comment."}</p>
          <div className="flex flex-wrap gap-3">
            <Button type="button" variant="ghost" className="min-h-11" disabled={moderating} onClick={() => { setReporting(review.reviewId); setReason(""); setBlocking(null); }}>{translate("feedback.caa4bf2a1441babe")}</Button>
            <Button type="button" variant="ghost" className="min-h-11" disabled={moderating} onClick={() => { setBlocking(review.reviewId); setReporting(null); }}>{translate("feedback.117e0030cdf402c6")}</Button>
          </div>
          {reporting === review.reviewId && <div className="space-y-3">
            <label className="block">{translate("feedback.db3a50907695a5a0")}<select className="mt-1 min-h-11 w-full border border-[#495188] bg-[#11162f] p-2 text-base" value={reason} disabled={moderating} onChange={(event) => setReason(event.target.value)}>
              <option value="">{translate("feedback.f9479b869e1045ab")}</option><option value="abuse">{translate("feedback.95b3cfea3534dd31")}</option><option value="personal_information">{translate("feedback.465f6e73e9bd7ee0")}</option><option value="spam">{translate("feedback.aa720ff59e6f6799")}</option><option value="other">{translate("feedback.e68d4015e5a91eda")}</option>
            </select></label>
            <div className="flex flex-wrap gap-3"><Button variant="secondary" disabled={!reason || moderating} onClick={() => void moderate("report", review.reviewId)}>{translate("feedback.b41fd589ad91ed78")}</Button><Button variant="ghost" disabled={moderating} onClick={() => setReporting(null)}>{translate("feedback.422a90a467f43daa")}</Button></div>
          </div>}
          {blocking === review.reviewId && <div className="space-y-3"><p>{translate("feedback.194f372b7de9bfd7")}</p><div className="flex flex-wrap gap-3"><Button variant="danger" disabled={moderating} onClick={() => void moderate("block", review.reviewId)}>{translate("feedback.71084a4132a56f60")}</Button><Button variant="ghost" disabled={moderating} onClick={() => setBlocking(null)}>{translate("feedback.119a718f0a036cbe")}</Button></div></div>}
        </article>;
      })}
      {loading && <p role="status">{translate("feedback.7d0e95c85eb56722")}</p>}
      {!loading && currentVisibility && nextCursor && <Button variant="secondary" className="min-h-11 w-full" onClick={() => void load(nextCursor)}>{translate("feedback.48692a311afa45d4")}</Button>}
      {!loading && loaded && currentVisibility && !nextCursor && visibleReviews.length > 0 && <p className="text-sm">{translate("feedback.c9125e3dde5e6ccd")}</p>}
      <Button type="button" variant="ghost" className="min-h-11" disabled={loading} onClick={() => { useFeedbackStore.getState().invalidate(); setRefresh((value) => value + 1); }}>{translate("feedback.e517b20931b98fa1")}</Button>
    </section>}
    {message && <p role="status" className="text-neon-lime">{message}</p>}
    {error && <p role="alert" className="text-[#ffc1cf]">{error}</p>}
  </FeedbackDialog>;
}
