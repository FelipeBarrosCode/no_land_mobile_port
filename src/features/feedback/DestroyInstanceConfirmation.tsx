import { useEffect, useId, useRef, useState } from "react";
import { Button } from "../../components/ui/Button";
import { feedbackNative } from "./api";
import { FEEDBACK_COMMENT_LIMIT, feedbackReadsEnabled, feedbackViewerKey, normalizeReviewDraft, safeProviderInstanceId, stageFeedbackThenDestroy } from "./model";
import { useFeedbackStore } from "./store";

export function DestroyInstanceConfirmation({ instanceId, onDestroy, onCancel, actionDisabled }: {
  instanceId: number;
  onDestroy: () => Promise<void>;
  onCancel: () => void;
  actionDisabled: boolean;
}) {
  const status = useFeedbackStore((state) => state.status);
  const enabled = feedbackReadsEnabled(status);
  const nativeInstanceId = safeProviderInstanceId(instanceId);
  const viewerKey = feedbackViewerKey(status, 0);
  const eligibilityKey = JSON.stringify([viewerKey, nativeInstanceId]);
  const [eligibility, setEligibility] = useState<{ key: string; eligible: boolean } | null>(null);
  const [score, setScore] = useState<number | null>(null);
  const [comment, setComment] = useState("");
  const [destroying, setDestroying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const groupName = useId();

  useEffect(() => {
    let cancelled = false;
    setEligibility(null);
    setScore(null);
    setComment("");
    if (enabled && nativeInstanceId) {
      void feedbackNative.eligible(nativeInstanceId).then((value) => {
        if (!cancelled) setEligibility({ key: eligibilityKey, eligible: value });
      }).catch(() => { /* Optional eligibility failure must not change destruction availability. */ });
    }
    return () => { cancelled = true; };
  }, [enabled, nativeInstanceId, eligibilityKey]);

  const showFeedback = enabled && eligibility?.key === eligibilityKey && eligibility.eligible && nativeInstanceId !== null;
  const draft = normalizeReviewDraft(score, comment);
  const disabled = actionDisabled || destroying;

  async function destroy(submit: boolean) {
    if (inFlight.current || actionDisabled || (submit && (!showFeedback || !draft))) return;
    inFlight.current = true;
    setDestroying(true); setError(null);
    const currentViewerKey = feedbackViewerKey(useFeedbackStore.getState().status, 0);
    const sameViewer = currentViewerKey !== null && currentViewerKey === viewerKey;
    const stagedDraft = sameViewer && showFeedback
      ? (submit ? draft : { score: null, comment: null })
      : null;
    try {
      await stageFeedbackThenDestroy(stagedDraft,
        (value) => feedbackNative.stageReview(nativeInstanceId!, value),
        onDestroy,
        () => useFeedbackStore.getState().notify("Feedback saving or clearing could not be confirmed locally; a previously saved review may remain. Destruction will continue normally without waiting for the feedback service."));
      useFeedbackStore.getState().invalidate();
      void useFeedbackStore.getState().refreshStatus();
    } catch {
      setError("Destruction could not be confirmed. Check the rental status before retrying. The latest durably saved review action is retained by native for retry; review publication requires confirmed provider deletion.");
    } finally { inFlight.current = false; setDestroying(false); }
  }

  return <section className="space-y-4 rounded border border-red-500/40 bg-red-900/20 p-3 text-base text-[#ffc1cf]" aria-label={`Confirm destruction of instance ${instanceId}`}>
    <p>Destroy instance {instanceId}? This ends the rental. The established backup and provider deletion flow will run.</p>
    {showFeedback && <fieldset className="min-w-0 space-y-3 border-t border-[#495188] pt-3 text-[#c6dbf4]" disabled={disabled}>
      <legend className="pt-3 font-semibold">How smooth was streaming on this machine? (optional)</legend>
      <p className="text-sm">Native recorded a streaming frame for this tracked rental, not just pairing. Frame receipt/submission does not prove actual presentation or a smooth experience.</p>
      <p id={`${groupName}-scale`} className="text-sm">1 = very choppy · 10 = very smooth. No score is preselected.</p>
      <div role="radiogroup" aria-label="Optional streaming score from 1 to 10" aria-describedby={`${groupName}-scale`} className="grid grid-cols-[repeat(auto-fit,minmax(2.75rem,1fr))] gap-2">
        {Array.from({ length: 10 }, (_, index) => index + 1).map((value) => <label key={value} className="relative cursor-pointer">
          <input type="radio" className="peer sr-only" name={groupName} value={value} checked={score === value} onChange={() => setScore(value)} />
          <span className="flex min-h-11 items-center justify-center border border-[#495188] bg-[#0b0f23] text-white peer-checked:border-neon-cyan peer-checked:bg-[#1b2f4d] peer-focus-visible:outline peer-focus-visible:outline-2 peer-focus-visible:outline-neon-cyan">{value}</span>
        </label>)}
      </div>
      {score !== null && <button type="button" className="min-h-11 px-1 text-neon-cyan underline" onClick={() => setScore(null)}>Clear rating</button>}
      <label className="block">Comment (optional)
        <textarea className="mt-1 min-h-28 w-full resize-y border border-[#495188] bg-[#0b0f23] p-3 text-base text-white" maxLength={FEEDBACK_COMMENT_LIMIT} value={comment} onChange={(event) => setComment(event.target.value)} aria-describedby={`${groupName}-comment-help`} />
      </label>
      <p id={`${groupName}-comment-help`} className="text-sm">{comment.length}/{FEEDBACK_COMMENT_LIMIT} characters. Published as plain text to enrolled participants after confirmed destruction. Do not include personal information, passwords or secrets.</p>
      <p className="text-sm">Choose a score, a comment, both, or skip. A review is saved locally first and never waits for a feedback API upload.</p>
    </fieldset>}
    <div className="flex flex-col gap-3">
      {showFeedback && <Button type="button" variant="danger" className="min-h-11 w-full" disabled={disabled || !draft} onClick={() => void destroy(true)}>Submit and destroy</Button>}
      <Button type="button" variant="danger" className="min-h-11 w-full" disabled={disabled} onClick={() => void destroy(false)}>{showFeedback ? "Skip and destroy" : "Confirm destroy"}</Button>
      <Button type="button" variant="ghost" className="min-h-11 w-full" disabled={destroying} onClick={onCancel}>Cancel</Button>
    </div>
    {destroying && <p role="status">Preparing destruction… Feedback availability will not prevent the normal backup/deletion path.</p>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
