import { translate } from "../../lib/i18n";
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
        () => useFeedbackStore.getState().notify(translate("feedback.a7672767396b2be8")));
      useFeedbackStore.getState().invalidate();
      void useFeedbackStore.getState().refreshStatus();
    } catch {
      setError(translate("feedback.ef643b45b779954e"));
    } finally { inFlight.current = false; setDestroying(false); }
  }

  return <section className="space-y-4 rounded border border-red-500/40 bg-red-900/20 p-3 text-base text-[#ffc1cf]" aria-label={translate("feedback.5bae153ab7e21d87", { value0: instanceId })}>
    <p>{translate("feedback.c320e3609717fd2e")} {instanceId}{translate("feedback.4e7ebcf61c4fd052")}</p>
    {showFeedback && <fieldset className="min-w-0 space-y-3 border-t border-[#495188] pt-3 text-[#c6dbf4]" disabled={disabled}>
      <legend className="pt-3 font-semibold">{translate("feedback.9cef2ccf592b9212")}</legend>
      <p className="text-sm">{translate("feedback.131282adc15ddb50")}</p>
      <p id={`${groupName}-scale`} className="text-sm">{translate("feedback.80242377ab2a6d95")}</p>
      <div role="radiogroup" aria-label={translate("feedback.50dfaa4bd2b362df")} aria-describedby={`${groupName}-scale`} className="grid grid-cols-[repeat(auto-fit,minmax(2.75rem,1fr))] gap-2">
        {Array.from({ length: 10 }, (_, index) => index + 1).map((value) => <label key={value} className="relative cursor-pointer">
          <input type="radio" className="peer sr-only" name={groupName} value={value} checked={score === value} onChange={() => setScore(value)} />
          <span className="flex min-h-11 items-center justify-center border border-[#495188] bg-[#0b0f23] text-white peer-checked:border-neon-cyan peer-checked:bg-[#1b2f4d] peer-focus-visible:outline peer-focus-visible:outline-2 peer-focus-visible:outline-neon-cyan">{value}</span>
        </label>)}
      </div>
      {score !== null && <button type="button" className="min-h-11 px-1 text-neon-cyan underline" onClick={() => setScore(null)}>{translate("feedback.041431fca4013ca6")}</button>}
      <label className="block">{translate("feedback.c271966972e67cf9")}
        <textarea className="mt-1 min-h-28 w-full resize-y border border-[#495188] bg-[#0b0f23] p-3 text-base text-white" maxLength={FEEDBACK_COMMENT_LIMIT} value={comment} onChange={(event) => setComment(event.target.value)} aria-describedby={`${groupName}-comment-help`} />
      </label>
      <p id={`${groupName}-comment-help`} className="text-sm">{comment.length}/{FEEDBACK_COMMENT_LIMIT} {translate("feedback.5dfa59df796851f2")}</p>
      <p className="text-sm">{translate("feedback.de805711f0cdf0ae")}</p>
    </fieldset>}
    <div className="flex flex-col gap-3">
      {showFeedback && <Button type="button" variant="danger" className="min-h-11 w-full" disabled={disabled || !draft} onClick={() => void destroy(true)}>{translate("feedback.8e770652ddca8841")}</Button>}
      <Button type="button" variant="danger" className="min-h-11 w-full" disabled={disabled} onClick={() => void destroy(false)}>{showFeedback ? translate("feedback.e36ec24fb60dd10d") : translate("feedback.3100ab8bacd41b70")}</Button>
      <Button type="button" variant="ghost" className="min-h-11 w-full" disabled={destroying} onClick={onCancel}>{translate("runtime.cancel")}</Button>
    </div>
    {destroying && <p role="status">{translate("feedback.a12a8072e8160996")}</p>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
