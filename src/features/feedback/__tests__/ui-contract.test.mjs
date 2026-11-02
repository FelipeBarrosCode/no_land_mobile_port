import test from "node:test";
import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";

const source = (path) => readFile(new URL(path, import.meta.url), "utf8");

test("UI feedback uses only the exact native commands, no HTTP/credential storage/user-agent logic", async () => {
  const api = await source("../api.ts");
  const commands = [...api.matchAll(/"(feedback_[a-z_]+)"/g)].map((match) => match[1]);
  assert.deepEqual(commands, ["feedback_program_status", "feedback_program_notices", "feedback_program_enroll",
    "feedback_program_consent", "feedback_program_delete", "feedback_machine_summaries", "feedback_machine_reviews",
    "feedback_review_report", "feedback_review_block_author", "feedback_review_eligibility", "feedback_stage_review"]);
  const files = (await readdir(new URL("../", import.meta.url))).filter((name) => /\.tsx?$/.test(name));
  for (const file of files) {
    const text = await source(`../${file}`);
    assert.doesNotMatch(text, /\bfetch\s*\(|XMLHttpRequest|navigator\.userAgent|(?:localStorage|sessionStorage)\.(?:getItem|setItem)|Authorization\s*:/, file);
    assert.doesNotMatch(text, /dangerouslySetInnerHTML/, file);
  }
});

test("program UI has unchecked independent scopes, optional profile and server policy links", async () => {
  const modal = await source("../FeedbackProgramModal.tsx");
  assert.match(modal, /\[accepted, setAccepted\] = useState\(false\)/);
  assert.match(modal, /\[optionalContext, setOptionalContext\] = useState\(false\)/);
  assert.match(modal, /!pending && !busy && accepted && policiesMatch/);
  assert.match(modal, /optionalFeedbackProfile\(optionalContext, profile\)/);
  assert.match(modal, /const rejoining =/);
  assert.match(modal, /Stop contributing/);
  assert.match(modal, /Delete my contributions/);
  assert.match(modal, /pendingControlAction\?\.enabled/);
  assert.match(modal, /pendingControlAction\?\.afterAck === "delete"/);
  assert.match(modal, /Boolean\(status\.installationId\)/);
  assert.match(modal, /Retry original enrollment/);
  assert.match(modal, /Server-side purge follows the published deletion policy/);
  assert.match(modal, /You can close this window and continue using No Land/);
  assert.doesNotMatch(modal, /<FeedbackDialog[^>]*busy=/);
  const links = await source("../FeedbackPolicyLinks.tsx");
  for (const key of ["privacyUrl", "termsUrl", "deletionUrl"]) assert.ok(links.includes(`document.${key}`));
  assert.match(links, /safePolicyUrl/);
  assert.match(links, /rel="noopener noreferrer"/);
});

test("actual search cards enrich displayed native IDs and keep provider sorting/filtering untouched", async () => {
  const picker = await source("../../servers/ServerPickerModal.tsx");
  assert.match(picker, /useMachineFeedback\(displayedOffers\.map\(feedbackOfferMachineId\), open\)/);
  assert.match(picker, /const machineId = feedbackOfferMachineId\(offer\)/);
  const backend = await source("../../../lib/backend.ts");
  assert.match(backend, /providerMachineId: feedbackOfferMachineId\(offer\)/);
  assert.match(picker, /right\.reliability - left\.reliability/);
  assert.match(picker, /left\.reliability - right\.reliability/);
  assert.match(picker, /const reliabilityPercent = offer\.reliability \* 100/);
  assert.match(picker, /vastReliability=\{offer\.reliability\}/);
  assert.match(picker, /Vast reliability sort/);
  assert.match(picker, /Vast reliability filter/);
  const metrics = await source("../MachineFeedbackMetrics.tsx");
  assert.match(metrics, /aria-label=\{`View machine reviews/);
  assert.match(metrics, /event\.preventDefault\(\); event\.stopPropagation\(\)/);
  assert.match(metrics, /onKeyDown=\{\(event\) => event\.stopPropagation\(\)\}/);
  assert.match(metrics, /No valid observations/);
  assert.match(metrics, /Small sample/);
  assert.match(metrics, /Latest setup outcome:/);
  assert.match(metrics, /Updates are not new observations/);
  assert.match(metrics, /N\/A · 0 comments/);
});

test("existing destroy confirmation gates optional review on native evidence without preselecting a score", async () => {
  const actions = await source("../../shared-storage-manager/InstanceCardActions.tsx");
  assert.match(actions, /showDestroyConfirm && \(\s*<DestroyInstanceConfirmation/);
  const dialog = await source("../DestroyInstanceConfirmation.tsx");
  assert.match(dialog, /if \(enabled && nativeInstanceId\)/);
  assert.match(dialog, /feedbackNative\.eligible\(nativeInstanceId\)/);
  assert.match(dialog, /const showFeedback = enabled && eligibility\?\.key === eligibilityKey/);
  assert.match(dialog, /currentViewerKey === viewerKey/);
  assert.match(dialog, /useState<number \| null>\(null\)/);
  assert.match(dialog, /maxLength=\{FEEDBACK_COMMENT_LIMIT\}/);
  assert.match(dialog, /stageFeedbackThenDestroy/);
  assert.match(dialog, /submit \? draft : \{ score: null, comment: null \}/);
  assert.match(dialog, /sameViewer && showFeedback/);
  assert.match(dialog, /saving or clearing could not be confirmed/);
  assert.match(dialog, /onClick=\{onCancel\}/);
  for (const label of ["Submit and destroy", "Skip and destroy", "Cancel"]) assert.ok(dialog.includes(label));
  assert.doesNotMatch(dialog, /embeddedMoonlightPaired|feedbackNative\.enroll/);
});

test("native cancellation uses a durable independent tombstone and revisioned DELETE, not an empty PUT", async () => {
  const journal = await source("../../../../src-tauri/src/services/feedback/journal.rs");
  const delivery = await source("../../../../src-tauri/src/services/feedback/delivery.rs");
  const lifecycle = await source("../../../../src-tauri/src/services/feedback/lifecycle.rs");
  assert.match(journal, /fn legacy_review_may_have_been_sent\(\) -> bool/);
  assert.match(journal, /pub fn claim_review/);
  assert.match(journal, /r\.review_may_have_been_sent = true/);
  assert.match(journal, /checked_add\(1\)/);
  assert.match(journal, /skip_replaces_failed_destroy_draft_and_survives_restart/);
  assert.doesNotMatch(lifecycle, /skipping feedback needs no draft/);
  assert.match(delivery, /self\.journal\.claim_review/);
  assert.match(delivery, /if removed \{\s*Method::DELETE\s*\} else \{\s*Method::PUT\s*\}/);
  assert.match(delivery, /header\("X-Review-Revision", body\["revision"\]\.to_string\(\)\)/);
});

test("entry points and dialogs contain phone/tablet/keyboard accessibility structure", async () => {
  const dashboard = await source("../../dashboard/DashboardScreen.tsx");
  assert.equal((dashboard.match(/>Feedback program<\/Button>/g) ?? []).length, 2);
  const dialog = await source("../FeedbackDialog.tsx");
  assert.match(dialog, /labelledBy=\{titleId\}/);
  assert.match(dialog, /event\.key === "Escape"/);
  assert.match(dialog, /event\.key !== "Tab"/);
  assert.match(dialog, /previousFocus\.focus\(\)/);
  assert.match(dialog, /ModalBody/);
  const reviews = await source("../MachineReviewsModal.tsx");
  assert.match(reviews, /whitespace-pre-wrap break-words/);
  assert.match(reviews, /Load more reviews/);
  assert.match(reviews, /Report review/);
  assert.match(reviews, /Block contributor/);
  assert.match(reviews, /reviewVisibility === visibilityRevision/);
  assert.match(reviews, /review\.expiresAt > now/);
  assert.match(reviews, /Route-specific jitter observations/);
  assert.match(reviews, /Original Vast reliability:/);
  const menu = await source("../../../components/ui/MobileSideMenu.tsx");
  assert.match(menu, /if \(!open\) return null/);
  assert.match(menu, /previousFocus\.focus\(\)/);
  assert.match(menu, /event\.key !== "Tab"/);
  const runtime = await source("../FeedbackRuntime.tsx");
  assert.doesNotMatch(runtime, /loadNotices|\.enroll\(|\.summaries\(/);
});

test("new feedback sources and handoff documentation contain no trailing whitespace", async () => {
  const files = (await readdir(new URL("../", import.meta.url))).filter((name) => /\.tsx?$/.test(name));
  const paths = [...files.map((file) => `../${file}`), "../../../../docs/FEEDBACK_UI.md"];
  for (const path of paths) {
    const text = await source(path);
    assert.doesNotMatch(text, /[\t ]+$/m, path);
    assert.ok(text.endsWith("\n"), path);
  }
});
