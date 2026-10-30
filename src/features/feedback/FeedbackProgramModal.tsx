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
      setError("Published notices could not be loaded. Joining is unavailable; normal rentals are unaffected.");
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
    <FeedbackDialog title="Feedback program" onClose={onClose}>
      <section className="space-y-3">
        <h3 className="font-display text-sm text-neon-cyan">Help improve machine recommendations</h3>
        <p>Join this optional program to share rental setup outcomes and streaming-jitter summaries. You may also leave a rating or comment after streaming. Renting and streaming work without joining.</p>
        <p>Required contributions include machine and rental identifiers, readiness/pairing/destruction milestones, supported jitter summaries, coarse rented hardware, platform, app version and measurement method. Raw video, audio and packet data are not contributed.</p>
        <p>A random app-installation identifier links your contributions. No name or email is required; the program does not collect MAC addresses or Vast API keys. This is pseudonymous, not anonymous: comments, optional context and service operational records can sometimes identify someone. Requests expose an IP address to the service infrastructure. Machine results and published reviews are visible to enrolled participants.</p>
        <p>You can stop future contributions or delete contributions here. Joining or rejoining starts with the <strong>next new rental</strong>, not a rental already in progress.</p>
      </section>

      <section className="space-y-2 rounded border border-[#495188] bg-[#0b0f23] p-3" aria-label="Native feedback status" aria-live="polite">
        <p><strong>{status.enabled ? "Contributing enabled" : "Feedback off"}</strong> · {status.enrollmentStatus.replace(/_/g, " ")}</p>
        {busy && <p>A native participation change is in progress. You can close this window and continue using No Land; closing does not undo a consent or deletion request already sent.</p>}
        {!status.configured && <p>No feedback service is configured in this native build. Enrollment is unavailable; no production address is assumed.</p>}
        {!status.supported && <p>Native feedback capability or protected credential storage is unavailable on this installation.</p>}
        {status.statusMessage && <p className="whitespace-pre-wrap break-words">{status.statusMessage}</p>}
        {status.capabilities?.jitterUnavailableReason && <p className="text-sm">Native jitter capability: {status.capabilities.jitterUnavailableReason}</p>}
        {status.enrollmentStatus === "enrolling" && <p className="text-[#ffd78a]">Enrollment acknowledgement is pending. Retry the original consent/profile choices; native reuses the protected recovery proof. Changed choices cannot replace an uncertain enrollment.</p>}
        {pending && <p className="text-[#ffd78a]">{pendingDeletion ? "Deletion pending" : status.pendingControlAction?.afterAck === "consent" || !status.pendingControlAction?.enabled ? "Withdrawal synchronization pending" : "Consent / rejoin or scope synchronization pending"}: {status.enabled ? "delivery waits for native acknowledgement" : "local collection is stopped"}. Native retains the control request for retry. Rejoining is unavailable until pending controls finish.</p>}
        {status.enrollmentStatus === "deleted" && <p>Deletion was acknowledged and contributions are excluded from service read responses. Server-side purge follows the published deletion policy; see the receipt status below. A future enrollment is a new participation choice.</p>}
        {status.deletionReceiptId && <p className="break-all">Deletion receipt: {status.deletionReceiptId}{status.deletionStatus ? ` · ${status.deletionStatus}` : ""}</p>}
        {status.enabled && <p>{status.pendingSessions} pending rental contribution{status.pendingSessions === 1 ? "" : "s"}. Uploads may wait for connectivity or native background execution.</p>}
        <Button type="button" variant="ghost" className="min-h-11" disabled={busy} onClick={() => void useFeedbackStore.getState().refreshStatus()}>Refresh native status</Button>
      </section>

      {noticesLoading && <p role="status">Loading published notices…</p>}
      {document && <section className="space-y-2" aria-label="Published feedback policies">
        <p className="whitespace-pre-wrap break-words">{document.disclosure}</p>
        <FeedbackPolicyLinks document={document} />
        <p className="break-words text-sm">Published contact: {document.contact}</p>
        <p className="text-sm">Privacy {document.privacyVersion} · Terms {document.termsVersion}</p>
        {(!document.available || document.collectionPaused) && <p className="text-[#ffd78a]">The feedback service is not accepting enrollment{document.collectionPaused ? " while collection is paused" : ""}. Native status governs existing contributions.</p>}
      </section>}
      {!noticesLoading && !document && status.configured && status.supported && <Button variant="ghost" onClick={() => void loadNotices()} disabled={busy}>Retry published notices</Button>}

      {!status.enabled && !pendingDeletion && <section className="space-y-4" aria-label={rejoining ? "Rejoin feedback program" : "Enrollment choices"}>
        <label className="flex min-h-11 items-start gap-3">
          <input type="checkbox" className="mt-1 size-5 shrink-0 accent-cyan-300" checked={accepted} disabled={!document || pending || busy} onChange={(event) => setAccepted(event.target.checked)} />
          <span>I agree to contribute the required measurements and setup outcomes described above and accept the Feedback Program Terms. <strong>Required to {rejoining ? "rejoin" : "join"}.</strong></span>
        </label>
        <label className="flex min-h-11 items-start gap-3">
          <input type="checkbox" className="mt-1 size-5 shrink-0 accent-cyan-300" checked={optionalContext} disabled={pending || busy} onChange={(event) => { setOptionalContext(event.target.checked); if (!event.target.checked) setProfile({}); }} />
          <span>Also share optional broad connection/location context and any profile fields I choose below. <strong>Optional; off by default.</strong></span>
        </label>
        {optionalContext && !rejoining && <fieldset className="grid min-w-0 gap-3 sm:grid-cols-2" disabled={busy || pending}>
          <legend className="mb-2 text-sm">Optional profile — leave every field blank if you prefer. Do not enter precise location or personal information.</legend>
          <label>Country code (optional)<input className={inputClass} maxLength={2} autoCapitalize="characters" placeholder="e.g. CA" value={profile.country ?? ""} onChange={(event) => setProfile({ ...profile, country: event.target.value.toUpperCase().replace(/[^A-Z]/g, "") })} /></label>
          <label>Broad region (optional)<input className={inputClass} maxLength={80} placeholder="e.g. province or state" value={profile.region ?? ""} onChange={(event) => setProfile({ ...profile, region: event.target.value })} /></label>
          <label>Connection (optional)<select className={inputClass} value={profile.connectionCategory ?? ""} onChange={(event) => setProfile({ ...profile, connectionCategory: event.target.value })}>
            <option value="">Not shared</option><option value="ethernet">Ethernet</option><option value="wifi">Wi-Fi</option><option value="cellular">Cellular</option><option value="other">Other</option>
          </select></label>
          <label>Bandwidth tier (optional)<select className={inputClass} value={profile.bandwidthTier ?? ""} onChange={(event) => setProfile({ ...profile, bandwidthTier: event.target.value })}>
            <option value="">Not shared</option><option value="under_25_mbps">Under 25 Mbps</option><option value="25_100_mbps">25–100 Mbps</option><option value="100_500_mbps">100–500 Mbps</option><option value="over_500_mbps">Over 500 Mbps</option>
          </select></label>
        </fieldset>}
        {!profileValid && <p className="text-[#ffd78a]">Enter a two-letter country code, or leave it blank.</p>}
        {!policiesMatch && document && <p className="text-[#ffd78a]">Published policies changed since enrollment. Native consent renewal is required before rejoining.</p>}
        <div className="flex flex-col gap-3 sm:flex-row">
          <Button type="button" className="min-h-11" disabled={!canJoin} loading={busy} onClick={() => void join()}>{enrolling ? "Retry original enrollment" : rejoining ? "Rejoin feedback program" : "Join feedback program"}</Button>
          <Button type="button" variant="ghost" className="min-h-11" disabled={busy} onClick={onClose}>Not now</Button>
        </div>
      </section>}

      {hasEnrollment && <section className="space-y-4 border-t border-[#495188] pt-4" aria-label="Manage participation">
        {status.enrollmentStatus === "enrolled" && status.enabled && <>
          <label className="flex min-h-11 items-start gap-3"><input type="checkbox" className="mt-1 size-5 shrink-0" checked={optionalContext} disabled={busy || pending} onChange={(event) => setOptionalContext(event.target.checked)} /><span>Share optional connection/location context. Turning this off also clears saved optional profile/context through native consent synchronization.</span></label>
          <Button variant="ghost" className="min-h-11" disabled={busy || pending || !status.enabled || optionalContext === status.optionalContextEnabled} onClick={() => void run(() => useFeedbackStore.getState().consent(true, optionalContext), "Optional scope change saved; native status shows any synchronization still pending.")}>Save optional scope</Button>
        </>}
        {(status.enrollmentStatus === "enrolled" || enrolling) && <>
          <p>Stopping clears unsent contributions and stops local collection. Already accepted history remains under the published retention policy. A request already received by the service may have been accepted before withdrawal.</p>
          <Button variant="ghost" className="min-h-11" disabled={busy || pendingDeletion} onClick={() => void run(() => useFeedbackStore.getState().consent(false, false), "Local contribution stopped. Check native status for withdrawal synchronization.")}>Stop contributing</Button>
        </>}
        <p>Delete my contributions also stops the program. The service excludes accepted contributions after acknowledgement and purges them under its published deletion policy. Offline deletion remains pending; native keeps the protected authentication material needed to complete it.</p>
        <label className="flex min-h-11 items-start gap-3"><input type="checkbox" className="mt-1 size-5 shrink-0" checked={deleteConfirmed} disabled={busy || pendingDeletion} onChange={(event) => setDeleteConfirmed(event.target.checked)} /><span>I want to delete this installation’s contributions and reviews.</span></label>
        <Button type="button" variant="danger" className="min-h-11" disabled={!deleteConfirmed || busy || pendingDeletion} onClick={() => void run(() => useFeedbackStore.getState().deleteContributions(), "Local contribution stopped. Deletion remains pending until native status confirms acknowledgement.")}>Delete my contributions</Button>
      </section>}
      {message && <p role="status" className="text-neon-lime">{message}</p>}
      {error && <p role="alert" className="text-[#ffc1cf]">{error}</p>}
      <p className="text-sm text-[#9ec4df]">{isIOS ? "On iPhone and iPad, background delivery is best-effort and resumes on foreground. " : "Delivery depends on native platform capability and connectivity. "}Unsupported or unmeasured jitter is N/A, never an assumed zero. Feedback errors never prevent renting, streaming, backup or destruction.</p>
    </FeedbackDialog>
  );
}
