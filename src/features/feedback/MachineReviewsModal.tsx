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
      if (sequence === requestSequence.current) setError("Reviews are unavailable. Retry without affecting search or Rent.");
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
    } catch { setError("That moderation action could not be confirmed. Please retry."); }
    finally { setModerating(false); }
  }

  const currentVisibility = enabled && reviewVisibility === visibilityRevision;
  const visibleReviews = currentVisibility ? reviews.filter((review) => review.expiresAt > now) : [];
  const jitter = summary?.jitter;
  return <FeedbackDialog title="Machine reviews" onClose={onClose}>
    <section className="space-y-2">
      <h3 className="break-words font-display text-sm text-neon-cyan">{offer.gpuName} · {offer.locationLabel}</h3>
      <p>Vast machine {machineId ?? "N/A"} · current offer: {offer.gpuCount} GPU{offer.gpuCount === 1 ? "" : "s"}. Hardware may differ from earlier rentals; historical context is shown with each review.</p>
      <p>Rating {summary?.reviews?.averageScore !== null && summary?.reviews?.averageScore !== undefined ? `${summary.reviews.averageScore.toFixed(1)}/10` : "N/A"} · {summary?.reviews?.ratingCount ?? "N/A"} ratings · {summary?.reviews?.commentCount ?? "N/A"} comments visible to you. Rating-only feedback is included.</p>
      <p className="text-sm">Reported, retained community history, not provider verification or a streaming guarantee. No Land setup outcomes do not change Vast filtering, sorting or automatic rental selection.</p>
      {summary?.asOf && <p className="text-sm">Summary updated {feedbackAge(summary.asOf)} ({new Date(summary.asOf).toLocaleString()}). This is the summary generation time, not the age of its underlying observations.</p>}
      <details className="rounded border border-[#495188] p-3">
        <summary className="min-h-11 cursor-pointer text-neon-cyan">Setup reliability and jitter details</summary>
        <div className="mt-2 space-y-2">
          <p>No Land setup: {summary?.reliability.ratio !== null && summary?.reliability.ratio !== undefined ? `${(summary.reliability.ratio * 100).toFixed(1)}%` : "N/A"} · {summary?.reliability.eligibleSessions ?? 0} eligible sessions ({summary?.reliability.successfulSessions ?? 0} successful, {summary?.reliability.failedSessions ?? 0} failed). Fewer than 2 valid outcomes retains Vast on the card; fewer than 10 is labelled a small sample.</p>
          <p>Success means ready and paired before destruction. Completely observed, abandoned setups can count as failures. Unknown or pending outcomes are not failures.</p>
          <p>Original Vast reliability: {Number.isFinite(offer.reliability) ? `${(offer.reliability * 100).toFixed(1)}%` : "N/A"} (provider uptime/health, not No Land setup success).</p>
          <p>Setup observation period: {summary?.reliability.firstObservedAt ? new Date(summary.reliability.firstObservedAt).toLocaleDateString() : "N/A"} – {summary?.reliability.lastObservedAt ? new Date(summary.reliability.lastObservedAt).toLocaleDateString() : "N/A"}. Latest outcome: {feedbackAge(summary?.reliability.lastObservedAt)}. All retained eligible history is used; sessions do not establish unique people.</p>
          {jitter ? <>
            <p>Jitter: mean {jitter.meanMs.toFixed(2)} ms, min {jitter.minMs.toFixed(2)} ms, max {jitter.maxMs.toFixed(2)} ms · {jitter.sampleCount} observations from {jitter.contributingSessions} sessions · all routes pooled.</p>
            <p>High (&gt;7 ms): {jitter.highSampleRatio === null ? "N/A" : `${(jitter.highSampleRatio * 100).toFixed(1)}%`} of observations. Severe (&gt;15 ms): {jitter.severeSampleRatio === null ? "N/A" : `${(jitter.severeSampleRatio * 100).toFixed(1)}%`}.</p>
            <p>Jitter observation period: {jitter.firstObservedAt ? new Date(jitter.firstObservedAt).toLocaleDateString() : "N/A"} – {jitter.lastObservedAt ? new Date(jitter.lastObservedAt).toLocaleDateString() : "N/A"}. Latest measurement: {feedbackAge(jitter.lastObservedAt)}.</p>
          </> : <p>Jitter N/A — no valid supported observations, including short or unsupported streams.</p>}
          {(summary?.jitterGroups.length ?? 0) > 0 && <ul className="space-y-2" aria-label="Route-specific jitter observations">
            {summary?.jitterGroups.map((group) => <li key={group.routeMode}>{group.routeMode}: {group.meanMs.toFixed(2)} ms mean · {group.sampleCount} observations · {group.contributingSessions} sessions · latest {feedbackAge(group.lastObservedAt)}</li>)}
          </ul>}
          <p className="text-sm">Sample-weighted RTT jitter EWMA over full 60-second windows reported every second, not packet counts or frame smoothness. All-routes pooling includes client/network effects and does not isolate host quality. No percentile is inferred.</p>
          <p className="break-all text-sm">Metric: rtt_jitter_ewma_window60_report1s_v1.</p>
        </div>
      </details>
    </section>
    {!enabled ? <p role="status">Feedback is off or consent synchronization is pending. Reviews are hidden.</p> : <section className="space-y-4" aria-label="Published reviews" aria-busy={loading}>
      {loaded && currentVisibility && !expired && visibleReviews.length === 0 && <p>N/A — No comments yet.</p>}
      {expired && <p className="text-sm text-[#ffd78a]">Earlier review pages expired after 60 seconds and were removed. Continue loading later pages below, or refresh to read from the start.</p>}
      {visibleReviews.map((review) => {
        const hardware = review.hardwareSnapshot;
        const coarseDate = new Date(`${review.publishedDate.slice(0, 10)}T12:00:00Z`);
        return <article key={review.reviewId} className="space-y-3 rounded border border-[#495188] bg-[#0b0f23] p-3">
          <p><strong>{review.score !== null ? `${review.score}/10` : "No rating"}</strong> · {Number.isFinite(coarseDate.getTime()) ? coarseDate.toLocaleDateString() : "Publication date unavailable"}</p>
          {hardware && <p className="break-words text-sm text-[#9ec4df]">At rental: {[hardware.gpuName, hardware.rentedGpuCount ? `${hardware.rentedGpuCount} GPU(s)` : null, hardware.region, hardware.country].filter(Boolean).join(" · ") || "coarse hardware unavailable"}</p>}
          <p className="whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{review.comment ?? "N/A — Rating only; no comment."}</p>
          <div className="flex flex-wrap gap-3">
            <Button type="button" variant="ghost" className="min-h-11" disabled={moderating} onClick={() => { setReporting(review.reviewId); setReason(""); setBlocking(null); }}>Report review</Button>
            <Button type="button" variant="ghost" className="min-h-11" disabled={moderating} onClick={() => { setBlocking(review.reviewId); setReporting(null); }}>Block contributor</Button>
          </div>
          {reporting === review.reviewId && <div className="space-y-3">
            <label className="block">Report reason<select className="mt-1 min-h-11 w-full border border-[#495188] bg-[#11162f] p-2 text-base" value={reason} disabled={moderating} onChange={(event) => setReason(event.target.value)}>
              <option value="">Choose a reason</option><option value="abuse">Abusive or hateful content</option><option value="personal_information">Personal information or secrets</option><option value="spam">Spam or misleading content</option><option value="other">Other policy violation</option>
            </select></label>
            <div className="flex flex-wrap gap-3"><Button variant="secondary" disabled={!reason || moderating} onClick={() => void moderate("report", review.reviewId)}>Submit report</Button><Button variant="ghost" disabled={moderating} onClick={() => setReporting(null)}>Cancel report</Button></div>
          </div>}
          {blocking === review.reviewId && <div className="space-y-3"><p>Hide reviews and rating contributions by this contributor for your installation? Shared valid setup/jitter measurements remain.</p><div className="flex flex-wrap gap-3"><Button variant="danger" disabled={moderating} onClick={() => void moderate("block", review.reviewId)}>Confirm block</Button><Button variant="ghost" disabled={moderating} onClick={() => setBlocking(null)}>Cancel block</Button></div></div>}
        </article>;
      })}
      {loading && <p role="status">Loading published reviews…</p>}
      {!loading && currentVisibility && nextCursor && <Button variant="secondary" className="min-h-11 w-full" onClick={() => void load(nextCursor)}>Load more reviews</Button>}
      {!loading && loaded && currentVisibility && !nextCursor && visibleReviews.length > 0 && <p className="text-sm">All retained reviews visible to you have been loaded.</p>}
      <Button type="button" variant="ghost" className="min-h-11" disabled={loading} onClick={() => { useFeedbackStore.getState().invalidate(); setRefresh((value) => value + 1); }}>Refresh / retry reviews</Button>
    </section>}
    {message && <p role="status" className="text-neon-lime">{message}</p>}
    {error && <p role="alert" className="text-[#ffc1cf]">{error}</p>}
  </FeedbackDialog>;
}
