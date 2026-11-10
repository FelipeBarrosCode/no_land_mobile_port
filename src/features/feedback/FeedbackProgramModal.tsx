import { translate } from "../../lib/i18n";
import { useEffect, useState } from "react";
import { Button } from "../../components/ui/Button";
import { isIOS } from "../../lib/platform";
import { FeedbackDialog } from "./FeedbackDialog";
import { FeedbackPolicyLinks } from "./FeedbackPolicyLinks";
import { optionalFeedbackProfile } from "./model";
import { useFeedbackStore } from "./store";
import type { FeedbackOptionalProfile, FeedbackProgramDocument } from "./types";

const inputClass = "min-h-11 w-full min-w-0 border border-[#495188] bg-[#0b0f23] px-3 py-2 text-base text-white";

export function FeedbackProgramModal({ onClose }: { onClose: () => void }) {
  const status = useFeedbackStore((state) => state.status);
  const busy = useFeedbackStore((state) => state.busy);
  const [document, setDocument] = useState<FeedbackProgramDocument | null>(null);
  const [noticesLoading, setNoticesLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [accepted, setAccepted] = useState(false);
  const [optionalContext, setOptionalContext] = useState(false);
  const [profile, setProfile] = useState<FeedbackOptionalProfile>({});
  const [deleteConfirmed, setDeleteConfirmed] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function loadNotices() {
    setError(null);
    setNoticesLoading(true);
    setDocument(null);
    setAccepted(false);
    try {
      const current = await useFeedbackStore.getState().refreshStatus();
      if (!current.configured || !current.supported) return;
      setDocument(await useFeedbackStore.getState().loadNotices());
      await useFeedbackStore.getState().refreshStatus();
    } catch {
      setError(translate("feedback.7b0d4107b8a5793f"));
    } finally { setNoticesLoading(false); }
  }

  useEffect(() => { void loadNotices(); }, []);
  useEffect(() => {
    if (status.enrollmentStatus === "enrolling") return;
    setOptionalContext(status.enrollmentStatus === "enrolled" ? status.optionalContextEnabled : false);
    setProfile({});
  }, [status.enrollmentStatus, status.optionalContextEnabled]);

  const pending = Boolean(status.pendingControlAction);
  const pendingDeletion = status.pendingControlAction?.kind === "delete" ||
    status.pendingControlAction?.afterAck === "delete" || status.enrollmentStatus === "deletion_pending";
  const hasEnrollment = Boolean(status.installationId) && status.enrollmentStatus !== "deleted";
  const enrolling = status.enrollmentStatus === "enrolling";
  const rejoining = hasEnrollment && !enrolling && !status.enabled && !pendingDeletion;
  const policiesMatch = !rejoining || (document?.privacyVersion === status.privacyVersion && document?.termsVersion === status.termsVersion);
  const profileValid = !optionalContext || !profile.country || /^[A-Z]{2}$/.test(profile.country);
  const canJoin = status.configured && status.supported && document?.available === true &&
    !document.collectionPaused && !pending && !busy && accepted && policiesMatch && profileValid;

  async function run(action: () => Promise<void>, success: string) {
    setError(null);
    setMessage(null);
    try { await action(); setMessage(success); setAccepted(false); }
    catch (error) { setError(error instanceof Error ? error.message : "The feedback change could not be confirmed."); }
  }

  async function join() {
    if (!canJoin || !document) return;
    const store = useFeedbackStore.getState();
    await run(() => rejoining ? store.consent(true, optionalContext) : store.enroll({
      privacyVersion: document.privacyVersion, termsVersion: document.termsVersion,
      participationAccepted: true, optionalContextEnabled: optionalContext,
      optionalProfile: optionalFeedbackProfile(optionalContext, profile),
    }), "Your choice was saved locally. Contributions begin with your next new rental once native consent is confirmed.");
  }

  return (
    <FeedbackDialog title={translate("feedback.54698a0574acf1a3")} onClose={onClose}>
      <section className="space-y-3">
        <h3 className="font-display text-sm text-neon-cyan">{translate("feedback.c4ad539bd426dcde")}</h3>
        <p>{translate("feedback.50022a0d0589c9c0")}</p>
        <p>{translate("feedback.1d97c9248dedac93")}</p>
        <p>{translate("feedback.723075d24813a94a")}</p>
        <p>{translate("feedback.f6d13f39e8032b8f")} <strong>{translate("feedback.d09ee644f70758e5")}</strong>{translate("feedback.943faa333b52a511")}</p>
      </section>

      <section className="space-y-2 rounded border border-[#495188] bg-[#0b0f23] p-3" aria-label={translate("feedback.3059d7fcfe906138")} aria-live="polite">
        <p><strong>{status.enabled ? translate("feedback.a3cc92422105eba9") : translate("feedback.acdd60f846d3d3ec")}</strong> · {status.enrollmentStatus.replace(/_/g, " ")}</p>
        {busy && <p>{translate("feedback.ccd5885504f0f130")}</p>}
        {!status.configured && <p>{translate("feedback.8243442d1c544534")}</p>}
        {!status.supported && <p>{translate("feedback.ebbca74088e1c7bd")}</p>}
        {status.statusMessage && <p className="whitespace-pre-wrap break-words">{status.statusMessage}</p>}
        {status.capabilities?.jitterUnavailableReason && <p className="text-sm">{translate("feedback.617ac6fa3f3f29ac")} {status.capabilities.jitterUnavailableReason}</p>}
        {status.enrollmentStatus === "enrolling" && <p className="text-[#ffd78a]">{translate("feedback.a37e10aea14d8660")}</p>}
        {pending && <p className="text-[#ffd78a]">{pendingDeletion ? translate("feedback.6145243bf47edd7f") : status.pendingControlAction?.afterAck === "consent" || !status.pendingControlAction?.enabled ? translate("feedback.9975bdc10a794748") : translate("feedback.d583fbcebec32218")}: {status.enabled ? translate("feedback.6b29297c18f80ded") : translate("feedback.9bc5c4b7fc5282cc")}{translate("feedback.47ff393e8b7c737d")}</p>}
        {status.enrollmentStatus === "deleted" && <p>{translate("feedback.4d27e74d4a21714e")}</p>}
        {status.deletionReceiptId && <p className="break-all">{translate("feedback.67690105065472ed")} {status.deletionReceiptId}{status.deletionStatus ? ` · ${status.deletionStatus}` : ""}</p>}
        {status.enabled && <p>{status.pendingSessions} {translate("feedback.679b80a07a4e576c")}{status.pendingSessions === 1 ? "" : translate("generated.043a718774c572bd")}{translate("feedback.e326991bcf7fb1ce")}</p>}
        <Button type="button" variant="ghost" className="min-h-11" disabled={busy} onClick={() => void useFeedbackStore.getState().refreshStatus()}>{translate("feedback.1bb44623c8fcb4f2")}</Button>
      </section>

      {noticesLoading && <p role="status">{translate("feedback.a845a8a83d94899d")}</p>}
      {document && <section className="space-y-2" aria-label={translate("feedback.b988749b5f851fa5")}>
        <p className="whitespace-pre-wrap break-words">{document.disclosure}</p>
        <FeedbackPolicyLinks document={document} />
        <p className="break-words text-sm">{translate("feedback.2357c2b3bde2e883")} {document.contact}</p>
        <p className="text-sm">{translate("feedback.54a57c3147c49f33")} {document.privacyVersion} {translate("feedback.14c7f24661d9ade4")} {document.termsVersion}</p>
        {(!document.available || document.collectionPaused) && <p className="text-[#ffd78a]">{translate("feedback.99c27a538d7dbf4e")}{document.collectionPaused ? translate("feedback.5659a23a6af8fc6c") : ""}{translate("feedback.8dd5709b6b4eeb7b")}</p>}
      </section>}
      {!noticesLoading && !document && status.configured && status.supported && <Button variant="ghost" onClick={() => void loadNotices()} disabled={busy}>{translate("feedback.cf48e6455453cb8e")}</Button>}

      {!status.enabled && !pendingDeletion && <section className="space-y-4" aria-label={rejoining ? translate("feedback.2598179d579f9913") : translate("feedback.457e9f442032509f")}>
        <label className="flex min-h-11 items-start gap-3">
          <input type="checkbox" className="mt-1 size-5 shrink-0 accent-cyan-300" checked={accepted} disabled={!document || pending || busy} onChange={(event) => setAccepted(event.target.checked)} />
          <span>{translate("feedback.d9842cd2237d7f92")} <strong>{translate("feedback.205c33dd9eef1ee3")} {rejoining ? translate("feedback.a58e6e7f676cce7a") : translate("feedback.58393216032be625")}.</strong></span>
        </label>
        <label className="flex min-h-11 items-start gap-3">
          <input type="checkbox" className="mt-1 size-5 shrink-0 accent-cyan-300" checked={optionalContext} disabled={pending || busy} onChange={(event) => { setOptionalContext(event.target.checked); if (!event.target.checked) setProfile({}); }} />
          <span>{translate("feedback.5f0dedf72ada9137")} <strong>{translate("feedback.0556cdcc58f77e39")}</strong></span>
        </label>
        {optionalContext && !rejoining && <fieldset className="grid min-w-0 gap-3 sm:grid-cols-2" disabled={busy || pending}>
          <legend className="mb-2 text-sm">{translate("feedback.76acc36cf2aa13a7")}</legend>
          <label>{translate("feedback.2acad5d586137d41")}<input className={inputClass} maxLength={2} autoCapitalize="characters" placeholder={translate("feedback.82b0850df43ae51d")} value={profile.country ?? ""} onChange={(event) => setProfile({ ...profile, country: event.target.value.toUpperCase().replace(/[^A-Z]/g, "") })} /></label>
          <label>{translate("feedback.857e5c48442e3b74")}<input className={inputClass} maxLength={80} placeholder={translate("feedback.4bbc33ed48f731d3")} value={profile.region ?? ""} onChange={(event) => setProfile({ ...profile, region: event.target.value })} /></label>
          <label>{translate("feedback.2fbe404dcfd77861")}<select className={inputClass} value={profile.connectionCategory ?? ""} onChange={(event) => setProfile({ ...profile, connectionCategory: event.target.value })}>
            <option value="">{translate("feedback.f8a76298d2e87743")}</option><option value="ethernet">{translate("feedback.681e12098d288b65")}</option><option value="wifi">{translate("feedback.337253f2e8d4d14a")}</option><option value="cellular">{translate("feedback.144d4f9ab93fbac6")}</option><option value="other">{translate("feedback.f97e9da0e3b879f0")}</option>
          </select></label>
          <label>{translate("feedback.77e5cd227c26f064")}<select className={inputClass} value={profile.bandwidthTier ?? ""} onChange={(event) => setProfile({ ...profile, bandwidthTier: event.target.value })}>
            <option value="">{translate("feedback.f8a76298d2e87743")}</option><option value="under_25_mbps">{translate("feedback.86cf9d25d9e171d4")}</option><option value="25_100_mbps">{translate("feedback.034f7c7aa932d5a3")}</option><option value="100_500_mbps">{translate("feedback.16a3a706e8c97919")}</option><option value="over_500_mbps">{translate("feedback.1cf4a3d70e4aaa37")}</option>
          </select></label>
        </fieldset>}
        {!profileValid && <p className="text-[#ffd78a]">{translate("feedback.cc6616409771e99b")}</p>}
        {!policiesMatch && document && <p className="text-[#ffd78a]">{translate("feedback.d94eca4bf62eed7d")}</p>}
        <div className="flex flex-col gap-3 sm:flex-row">
          <Button type="button" className="min-h-11" disabled={!canJoin} loading={busy} onClick={() => void join()}>{enrolling ? translate("feedback.a873d591f8787fda") : rejoining ? translate("feedback.2598179d579f9913") : translate("feedback.ed621cd87ba8beef")}</Button>
          <Button type="button" variant="ghost" className="min-h-11" disabled={busy} onClick={onClose}>{translate("feedback.a0e63d7c7125d29a")}</Button>
        </div>
      </section>}

      {hasEnrollment && <section className="space-y-4 border-t border-[#495188] pt-4" aria-label={translate("feedback.2ae737c32b2fcee7")}>
        {status.enrollmentStatus === "enrolled" && status.enabled && <>
          <label className="flex min-h-11 items-start gap-3"><input type="checkbox" className="mt-1 size-5 shrink-0" checked={optionalContext} disabled={busy || pending} onChange={(event) => setOptionalContext(event.target.checked)} /><span>{translate("feedback.33516f4605feb8ca")}</span></label>
          <Button variant="ghost" className="min-h-11" disabled={busy || pending || !status.enabled || optionalContext === status.optionalContextEnabled} onClick={() => void run(() => useFeedbackStore.getState().consent(true, optionalContext), "Optional scope change saved; native status shows any synchronization still pending.")}>{translate("feedback.7d9c9bf0b62de8d9")}</Button>
        </>}
        {(status.enrollmentStatus === "enrolled" || enrolling) && <>
          <p>{translate("feedback.cc4c89ae8588817c")}</p>
          <Button variant="ghost" className="min-h-11" disabled={busy || pendingDeletion} onClick={() => void run(() => useFeedbackStore.getState().consent(false, false), "Local contribution stopped. Check native status for withdrawal synchronization.")}>{translate("feedback.32d9a48cbdf0666f")}</Button>
        </>}
        <p>{translate("feedback.e122ca4816524ba6")}</p>
        <label className="flex min-h-11 items-start gap-3"><input type="checkbox" className="mt-1 size-5 shrink-0" checked={deleteConfirmed} disabled={busy || pendingDeletion} onChange={(event) => setDeleteConfirmed(event.target.checked)} /><span>{translate("feedback.dc2d94c781fc6848")}</span></label>
        <Button type="button" variant="danger" className="min-h-11" disabled={!deleteConfirmed || busy || pendingDeletion} onClick={() => void run(() => useFeedbackStore.getState().deleteContributions(), "Local contribution stopped. Deletion remains pending until native status confirms acknowledgement.")}>{translate("feedback.0647ee7bc948bd1a")}</Button>
      </section>}
      {message && <p role="status" className="text-neon-lime">{message}</p>}
      {error && <p role="alert" className="text-[#ffc1cf]">{error}</p>}
      <p className="text-sm text-[#9ec4df]">{isIOS ? translate("feedback.cff466e1ce61256d") : translate("feedback.5bc0974cb8c8da94")}{translate("feedback.60d197f815d6784b")}</p>
    </FeedbackDialog>
  );
}
