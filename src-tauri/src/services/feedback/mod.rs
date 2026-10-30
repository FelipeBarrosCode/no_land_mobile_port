//! Native-only consent boundary. Nothing here can create/destroy a provider rental.
mod control_delivery;
mod controls;
mod delivery;
mod journal;
mod lifecycle;
mod metric;
mod secure;
pub use lifecycle::start;

use super::app_context::AppContext;
use crate::models::{app_state::OfferCandidate, feedback::*};
use journal::Journal;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex, OnceLock, RwLock,
    },
    time::{Duration, Instant},
};

const CREDENTIAL_REF: &str = "secure-store://feedback/mobile-port/installation";
const DELETION_REF: &str = "secure-store://feedback/mobile-port/deletion-code";
const MAX_RESPONSE: usize = 512 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnrollmentRequest {
    pub privacy_version: String,
    pub terms_version: String,
    pub participation_accepted: bool,
    pub optional_context_enabled: bool,
    #[serde(default = "empty_object")]
    pub optional_profile: Value,
}
fn empty_object() -> Value {
    json!({})
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramStatus {
    #[serde(flatten)]
    pub program: FeedbackProgramState,
    pub configured: bool,
    pub supported: bool,
    pub status_message: Option<String>,
    pub pending_sessions: usize,
    pub capabilities: Value,
    pub deletion_receipt_id: Option<String>,
    pub deletion_status: Option<String>,
}
#[derive(Default)]
struct Gate {
    enabled: bool,
    epoch: u64,
}
struct Retry {
    failures: u32,
    next: Instant,
    halted: bool,
}
#[derive(Debug)]
struct ApiError {
    status: Option<StatusCode>,
    retryable: bool,
    retry_after: Option<Duration>,
    message: String,
}
impl ApiError {
    fn local(message: impl Into<String>) -> Self {
        Self {
            status: None,
            retryable: false,
            retry_after: None,
            message: message.into(),
        }
    }
}
pub struct FeedbackProgramService {
    base_url: Option<String>,
    client: Option<reqwest::Client>,
    gate: RwLock<Gate>,
    journal: Journal,
    message: Mutex<Option<String>>,
    retry: Mutex<Retry>,
    request_lock: tokio::sync::Mutex<()>,
    pub control_lock: tokio::sync::Mutex<()>,
    paused: AtomicBool,
    local_stop: AtomicBool,
    stop_generation: AtomicU64,
    delivery_pending: AtomicBool,
    background: AtomicBool,
    stream: Mutex<Option<(String, Instant)>>,
    metric: Mutex<metric::MetricTracker>,
    notices: Mutex<Option<Value>>,
    pub(crate) credential_available: AtomicBool,
}
impl FeedbackProgramService {
    pub fn new(state_path: &Path, state: &FeedbackProgramState) -> Self {
        let base_url = configured_url(option_env!("NOLAND_FEEDBACK_API_URL"));
        let protected = if state.installation_id.is_some() {
            secure::read().ok().flatten()
        } else {
            None
        };
        let available = protected
            .as_ref()
            .is_some_and(|p| credentials_match(state, p, base_url.as_deref()));
        let journal = Journal::new(
            state_path.parent().unwrap_or(Path::new(".")),
            state.enabled && state.structurally_valid(),
            state.installation_id.as_deref(),
        );
        let stopped = protected.as_ref().is_some_and(|p| p.stop_requested);
        let paused = protected
            .as_ref()
            .and_then(|p| p.policy.as_ref())
            .is_some_and(|p| {
                p.paused
                    || Some(p.privacy_version.as_str()) != state.privacy_version.as_deref()
                    || Some(p.terms_version.as_str()) != state.terms_version.as_deref()
            });
        let enabled = state.structurally_valid()
            && state.enabled
            && available
            && !stopped
            && journal.error().is_none();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .ok();
        Self {
            base_url,
            client,
            gate: RwLock::new(Gate {
                enabled,
                epoch: state.consent_epoch,
            }),
            journal,
            message: Mutex::new(None),
            retry: Mutex::new(Retry {
                failures: 0,
                next: Instant::now(),
                halted: false,
            }),
            request_lock: tokio::sync::Mutex::new(()),
            control_lock: tokio::sync::Mutex::new(()),
            paused: AtomicBool::new(paused),
            local_stop: AtomicBool::new(stopped),
            stop_generation: AtomicU64::new(0),
            delivery_pending: AtomicBool::new(state.pending_control_action.is_some()),
            background: AtomicBool::new(false),
            stream: Mutex::new(None),
            metric: Mutex::new(Default::default()),
            notices: Mutex::new(None),
            credential_available: AtomicBool::new(available),
        }
    }
    fn note(&self, message: impl Into<String>) {
        if let Ok(mut m) = self.message.lock() {
            *m = Some(message.into());
        }
    }
    fn fail(&self, result: Result<(), String>) {
        if let Err(message) = result {
            self.note(message);
        }
    }
    fn collecting(&self, gate: &Gate) -> bool {
        gate.enabled
            && gate.epoch > 0
            && !self.paused.load(Ordering::SeqCst)
            && !self.local_stop.load(Ordering::SeqCst)
            && !self.background.load(Ordering::SeqCst)
            && self.credential_available.load(Ordering::SeqCst)
            && self.base_url.is_some()
            && secure::supported()
    }
    fn stop_local(&self) {
        self.stop_generation.fetch_add(1, Ordering::SeqCst);
        self.local_stop.store(true, Ordering::SeqCst);
        if let Ok(mut g) = self.gate.write() {
            g.enabled = false;
        }
        self.stream_stopped();
    }
    fn persist_stop(&self) {
        // Independent device-bound stop latch also fails closed if the normal state write fails.
        if let Ok(Some(mut protected)) = secure::read() {
            protected.stop_requested = true;
            self.fail(secure::write(&protected));
        }
    }
    pub fn reconcile_state(&self, state: &FeedbackProgramState) {
        if !state.enabled || !state.structurally_valid() {
            self.stop_local();
        }
        self.delivery_pending
            .store(state.pending_control_action.is_some(), Ordering::SeqCst);
        self.restore_gate(state);
    }
    fn restore_gate(&self, state: &FeedbackProgramState) {
        let available = secure::read().ok().flatten().is_some_and(|p| {
            credentials_match(state, &p, self.base_url.as_deref())
                && !p.stop_requested
                && p.policy.as_ref().is_none_or(|policy| {
                    Some(policy.privacy_version.as_str()) == state.privacy_version.as_deref()
                        && Some(policy.terms_version.as_str()) == state.terms_version.as_deref()
                })
        });
        self.credential_available.store(available, Ordering::SeqCst);
        if let Ok(mut g) = self.gate.write() {
            g.enabled = state.enabled
                && !self.local_stop.load(Ordering::SeqCst)
                && state.structurally_valid()
                && available
                && self.journal.error().is_none();
            g.epoch = state.consent_epoch;
        }
    }
    pub async fn status(&self, context: &AppContext) -> ProgramStatus {
        // Local-only: no notices fetch, enrollment, queue creation, or HTTP retry.
        let mut program = context.load_state().await.feedback_program;
        let enabled = self
            .gate
            .read()
            .map(|g| self.collecting(&g))
            .unwrap_or(false);
        let protected = if program.installation_id.is_some() {
            secure::read().ok().flatten()
        } else {
            None
        };
        let credentials_present = protected.as_ref().is_some_and(|p| {
            credentials_match(&program, p, self.base_url.as_deref()) && !p.stop_requested
        });
        program.enabled &= enabled && credentials_present;
        let receipt = protected.and_then(|p| p.receipt);
        let message = self
            .message
            .lock()
            .ok()
            .and_then(|m| m.clone())
            .or_else(|| self.journal.error())
            .or_else(|| {
                if self.base_url.is_none() {
                    Some(
                        "Feedback is unavailable: NOLAND_FEEDBACK_API_URL is not configured".into(),
                    )
                } else if !secure::supported() {
                    Some("No verified device-bound feedback adapter for this target".into())
                } else if program.enrollment_status == EnrollmentStatus::Enrolled
                    && !credentials_present
                {
                    Some("Protected feedback credentials are unavailable".into())
                } else if self.paused.load(Ordering::SeqCst) {
                    Some("Feedback collection is paused by the service".into())
                } else if self.journal.dropped() > 0 {
                    Some("Old pending feedback expired; no setup failure was inferred".into())
                } else {
                    None
                }
            });
        ProgramStatus {
            program,
            configured: self.base_url.is_some(),
            supported: secure::supported(),
            status_message: message,
            pending_sessions: self.journal.pending_count(),
            capabilities: json!({
                "setupLifecycle": secure::supported(), "nativeReceivedFrameEvidence": secure::supported(),
                "jitter": "requires_updated_network_agent", "jitterMetricVersion": METRIC_VERSION,
                "jitterUnavailableReason": "Legacy/missing agents, startup/paused/stale/loss-only windows are N/A; no RTT-variance substitution",
                "backgroundDeliveryGuaranteed": false }),
            deletion_receipt_id: receipt.as_ref().map(|r| r.receipt_id.clone()),
            deletion_status: receipt.map(|r| r.status),
        }
    }
    pub async fn program_notices(&self) -> Result<Value, String> {
        let document = match self
            .request(Method::GET, "/program", None, None, None)
            .await
        {
            Ok(document) => document,
            Err(error) => {
                self.retry_error(&error);
                return Err(error.message);
            }
        };
        let projected: ProgramDocument = serde_json::from_value(document)
            .map_err(|_| "Unsupported feedback program document")?;
        if projected.schema_version != 1
            || projected.privacy_version.is_empty()
            || projected.terms_version.is_empty()
            || projected.privacy_version.len() > 128
            || projected.terms_version.len() > 128
            || projected.metrics.len() > 32
            || (projected.available
                && (projected
                    .contact
                    .as_deref()
                    .is_none_or(|s| s.trim().is_empty())
                    || projected.disclosure.trim().is_empty()
                    || [
                        &projected.privacy_url,
                        &projected.terms_url,
                        &projected.deletion_url,
                    ]
                    .iter()
                    .any(|url| configured_url(url.as_deref()).is_none())))
        {
            return Err("Feedback publication information is unavailable or unsupported".into());
        }
        let document =
            serde_json::to_value(projected).map_err(|_| "Unsupported feedback notices")?;
        let paused = document["collectionPaused"].as_bool() == Some(true)
            || document["available"].as_bool() != Some(true);
        self.paused.store(paused, Ordering::SeqCst);
        // Persist an explicit service pause, not a timeout-derived guess. Never create an identity here.
        if let Ok(Some(mut protected)) = secure::read() {
            if self.base_url.as_deref() == Some(protected.api_origin.as_str()) {
                let policy = Some(secure::CachedPolicy {
                    paused,
                    privacy_version: document["privacyVersion"]
                        .as_str()
                        .unwrap_or_default()
                        .into(),
                    terms_version: document["termsVersion"].as_str().unwrap_or_default().into(),
                });
                if protected.policy != policy {
                    protected.policy = policy;
                    self.fail(secure::write(&protected));
                }
            }
        }
        if let Ok(mut n) = self.notices.lock() {
            *n = Some(document.clone());
        }
        Ok(document)
    }
    pub async fn enroll(
        &self,
        context: &AppContext,
        request: EnrollmentRequest,
    ) -> Result<ProgramStatus, String> {
        let generation = self.stop_generation.load(Ordering::SeqCst);
        let _guard = self.control_lock.lock().await;
        validate_enrollment(&request)?;
        if !secure::supported() {
            return Err(
                "This target has no verified device-bound feedback credential store".into(),
            );
        }
        let base = self
            .base_url
            .as_ref()
            .ok_or("Feedback API URL is not configured")?;
        let state = context.load_state().await.feedback_program;
        if state.pending_control_action.is_some() {
            return Err("Resolve pending withdrawal/deletion before enrolling".into());
        }
        if state.enrollment_status == EnrollmentStatus::Enrolled
            || state.enrollment_status == EnrollmentStatus::Withdrawn
        {
            return Err("Use the participation control to resume an existing enrollment".into());
        }
        let notices = self.program_notices().await?;
        if notices["available"] != true
            || notices["collectionPaused"] != false
            || notices["privacyVersion"] != request.privacy_version
            || notices["termsVersion"] != request.terms_version
        {
            return Err(
                "Review and accept the currently available feedback notices before enrolling"
                    .into(),
            );
        }
        if self.stop_generation.load(Ordering::SeqCst) != generation {
            return Err("A newer stop/deletion request superseded enrollment".into());
        }
        if state.installation_id.is_none() {
            self.local_stop.store(false, Ordering::SeqCst);
        }
        if state.enrollment_status == EnrollmentStatus::Deleted {
            if secure::read()?
                .and_then(|p| p.receipt)
                .is_some_and(|r| !matches!(r.status.as_str(), "complete" | "completed" | "deleted"))
            {
                return Err("Wait for the protected deletion receipt to complete before creating a new installation".into());
            }
            secure::delete()?; // Deliberate reset after acknowledged deletion, never an implicit reinstall recovery.
            self.local_stop.store(false, Ordering::SeqCst);
        }
        let mut protected = if state.enrollment_status == EnrollmentStatus::Enrolling {
            let p = secure::read()?
                .ok_or("Enrollment retry proof is unavailable; participation remains off")?;
            if p.installation_id != state.installation_id.clone().unwrap_or_default()
                || &p.api_origin != base
            {
                return Err("Enrollment retry identity/endpoint does not match".into());
            }
            p
        } else {
            // Do not use a hardware ID, Moonlight identity, Keychain orphan, or provider account as installation identity.
            secure::Protected {
                schema_version: 1,
                installation_id: uuid::Uuid::new_v4().to_string(),
                api_origin: base.clone(),
                credential: None,
                deletion_code: None,
                receipt: None,
                enrollment_ack: None,
                stop_requested: false,
                policy: Some(secure::CachedPolicy {
                    paused: false,
                    privacy_version: request.privacy_version.clone(),
                    terms_version: request.terms_version.clone(),
                }),
                enrollment: Some(secure::EnrollmentRetry {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    recovery_proof: random_proof(),
                    created_at: now(),
                    request: serde_json::to_value(&request)
                        .map_err(|_| "Invalid enrollment request")?,
                }),
            }
        };
        let retry = protected
            .enrollment
            .clone()
            .ok_or("Enrollment retry proof is missing")?;
        if serde_json::to_value(&request).ok().as_ref() != Some(&retry.request) {
            return Err("Retry the original enrollment choices; do not replace an unacknowledged enrollment".into());
        }
        let created = chrono::DateTime::parse_from_rfc3339(&retry.created_at)
            .map_err(|_| "Invalid enrollment retry time")?;
        if protected.enrollment_ack.is_none()
            && chrono::Utc::now()
                .signed_duration_since(created)
                .num_hours()
                >= 24
        {
            return Err("Enrollment recovery expired after 24 hours; contact the configured operator before resetting an uncertain enrollment".into());
        }
        secure::write(&protected)?; // Must succeed and verify before the first request.
        context
            .update_state(|s| {
                s.feedback_program.installation_id = Some(protected.installation_id.clone());
                s.feedback_program.enabled = false;
                s.feedback_program.enrollment_status = EnrollmentStatus::Enrolling;
                s.feedback_program.credential_ref = Some(CREDENTIAL_REF.into());
                s.feedback_program.deletion_code_ref = Some(DELETION_REF.into());
            })
            .await
            .map_err(|_| "Could not persist feedback enrollment intent")?;
        let mut body = retry.request;
        body["installationId"] = json!(protected.installation_id);
        body["requestId"] = json!(retry.request_id);
        body["recoveryProof"] = json!(retry.recovery_proof);
        let result = if let (Some(ack), Some(credential), Some(deletion_code)) = (
            protected.enrollment_ack.as_ref(),
            protected.credential.as_ref(),
            protected.deletion_code.as_ref(),
        ) {
            Ok(
                json!({"userId": ack.user_id, "credential": credential, "deletionCode": deletion_code,
                "consentEpoch": ack.consent_epoch, "privacyVersion": ack.privacy_version, "termsVersion": ack.terms_version,
                "consentedAt": ack.consented_at}),
            )
        } else {
            self.request(
                Method::POST,
                "/enrollments",
                Some(body),
                None,
                Some(&retry.request_id),
            )
            .await
        };
        let response = match result {
            Ok(response) => {
                self.retry_success();
                response
            }
            Err(error) => {
                self.retry_error(&error);
                return Err(error.message);
            }
        };
        let user = response["userId"]
            .as_str()
            .filter(|s| valid_uuid(s))
            .ok_or("Enrollment response has no valid contributor identity")?
            .to_string();
        let credential = response["credential"]
            .as_str()
            .filter(|s| s.len() >= 32 && s.len() <= 4096)
            .ok_or("Enrollment credential is invalid")?
            .to_string();
        let deletion = response["deletionCode"]
            .as_str()
            .filter(|s| s.len() >= 32 && s.len() <= 4096)
            .ok_or("Enrollment deletion recovery code is invalid")?
            .to_string();
        let epoch = response["consentEpoch"]
            .as_u64()
            .filter(|e| *e > 0)
            .ok_or("Enrollment epoch is invalid")?;
        let consented_at = response["consentedAt"]
            .as_str()
            .filter(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
            .ok_or("Enrollment consent time is invalid")?
            .to_string();
        if response["privacyVersion"] != request.privacy_version
            || response["termsVersion"] != request.terms_version
        {
            return Err("Enrollment policy acknowledgment does not match accepted versions".into());
        }
        protected.credential = Some(credential);
        protected.deletion_code = Some(deletion);
        protected.enrollment_ack = Some(secure::EnrollmentAck {
            user_id: user.clone(),
            consent_epoch: epoch,
            privacy_version: request.privacy_version.clone(),
            terms_version: request.terms_version.clone(),
            consented_at: consented_at.clone(),
        });
        protected.stop_requested |= self.local_stop.load(Ordering::SeqCst);
        // Keep the proof until state acknowledgment is durable, to recover a crash between these stores.
        secure::write(&protected)?;
        self.journal.bind_owner(&protected.installation_id)?;
        let saved = context
            .update_state(|s| {
                s.feedback_program = FeedbackProgramState {
                    schema_version: 1,
                    enabled: !self.local_stop.load(Ordering::SeqCst),
                    enrollment_status: EnrollmentStatus::Enrolled,
                    installation_id: Some(protected.installation_id.clone()),
                    user_id: Some(user),
                    credential_ref: Some(CREDENTIAL_REF.into()),
                    deletion_code_ref: Some(DELETION_REF.into()),
                    consent_epoch: epoch,
                    privacy_version: Some(request.privacy_version),
                    terms_version: Some(request.terms_version),
                    consented_at: Some(consented_at),
                    optional_context_enabled: request.optional_context_enabled,
                    pending_control_action: None,
                };
            })
            .await
            .map_err(|_| {
                "Could not acknowledge feedback enrollment locally; retry the original enrollment"
            })?;
        protected.enrollment = None;
        if let Err(error) = secure::write(&protected) {
            self.note(error);
        }
        self.delivery_pending.store(false, Ordering::SeqCst);
        self.restore_gate(&saved.feedback_program);
        Ok(self.status(context).await)
    }
}

fn configured_url(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    let url = reqwest::Url::parse(raw).ok()?;
    let local_debug = cfg!(debug_assertions)
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if (url.scheme() != "https" && !(local_debug && url.scheme() == "http"))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    Some(url.as_str().trim_end_matches('/').to_string())
}
fn credentials_match(s: &FeedbackProgramState, p: &secure::Protected, base: Option<&str>) -> bool {
    s.installation_id.as_deref() == Some(p.installation_id.as_str())
        && base == Some(p.api_origin.as_str())
        && s.credential_ref.as_deref() == Some(CREDENTIAL_REF)
        && s.deletion_code_ref.as_deref() == Some(DELETION_REF)
        && p.credential.as_ref().is_some_and(|v| v.len() >= 32)
        && s.consent_epoch > 0
}
fn random_proof() -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}
fn validate_enrollment(request: &EnrollmentRequest) -> Result<(), String> {
    if !request.participation_accepted
        || request.privacy_version.is_empty()
        || request.privacy_version.len() > 128
        || request.terms_version.is_empty()
        || request.terms_version.len() > 128
    {
        return Err("Explicit participation and current policy acceptance are required".into());
    }
    let profile = request
        .optional_profile
        .as_object()
        .ok_or("Optional profile must be an object")?;
    if (!request.optional_context_enabled && profile.values().any(|v| !v.is_null()))
        || profile.iter().any(|(k, v)| {
            let max = match k.as_str() {
                "country" => 2,
                "region" => 80,
                "connectionCategory" | "bandwidthTier" => 32,
                _ => return true,
            };
            if v.is_null() {
                return false;
            }
            v.as_str().is_none_or(|s| {
                s.encode_utf16().count() > max
                    || s.chars().any(char::is_control)
                    || (k == "country"
                        && !s.is_empty()
                        && (s.len() != 2 || !s.bytes().all(|b| b.is_ascii_uppercase())))
            })
        })
    {
        return Err("Optional feedback profile contains an unsupported field or value".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_guessed_endpoint_or_embedded_url_credentials() {
        assert!(configured_url(None).is_none());
        assert!(configured_url(Some("")).is_none());
        assert!(configured_url(Some("https://token@example.com")).is_none());
        assert!(configured_url(Some("http://example.com")).is_none());
        assert_eq!(
            configured_url(Some("https://feedback.example.test/")),
            Some("https://feedback.example.test".into())
        );
    }
    #[test]
    fn explicit_consent_and_profile_allowlist() {
        let mut r = EnrollmentRequest {
            privacy_version: "v1".into(),
            terms_version: "v1".into(),
            participation_accepted: false,
            optional_context_enabled: false,
            optional_profile: json!({}),
        };
        assert!(validate_enrollment(&r).is_err());
        r.participation_accepted = true;
        assert!(validate_enrollment(&r).is_ok());
        r.optional_profile = json!({"mac": "no"});
        assert!(validate_enrollment(&r).is_err());
    }
}
