//! Feedback's allowlisted wire/persistence types. No provider credentials or raw telemetry belong here.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;
pub const METRIC_VERSION: &str = "rtt_jitter_ewma_window60_report1s_v1";
pub const THRESHOLD_VERSION: &str = "noland_jitter_bands_v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentStatus {
    NotEnrolled,
    Enrolling,
    Enrolled,
    Withdrawn,
    DeletionPending,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    Consent,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingControl {
    pub kind: ControlKind,
    pub request_id: String,
    pub expected_epoch: u64,
    pub enabled: bool,
    pub optional_context_enabled: bool,
    #[serde(default)]
    pub after_ack: Option<ControlKind>,
    #[serde(default)]
    pub delivery_suspended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedbackProgramState {
    pub schema_version: u32,
    pub enabled: bool,
    pub enrollment_status: EnrollmentStatus,
    pub installation_id: Option<String>,
    pub user_id: Option<String>,
    pub credential_ref: Option<String>,
    pub deletion_code_ref: Option<String>,
    pub consent_epoch: u64,
    pub privacy_version: Option<String>,
    pub terms_version: Option<String>,
    pub consented_at: Option<String>,
    pub optional_context_enabled: bool,
    pub pending_control_action: Option<PendingControl>,
}
impl Default for FeedbackProgramState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            enabled: false,
            enrollment_status: EnrollmentStatus::NotEnrolled,
            installation_id: None,
            user_id: None,
            credential_ref: None,
            deletion_code_ref: None,
            consent_epoch: 0,
            privacy_version: None,
            terms_version: None,
            consented_at: None,
            optional_context_enabled: false,
            pending_control_action: None,
        }
    }
}
impl FeedbackProgramState {
    pub fn structurally_valid(&self) -> bool {
        self.schema_version == SCHEMA_VERSION
            && self.consent_epoch <= 9_007_199_254_740_991
            && (!self.enabled
                || self.pending_control_action.as_ref().is_none_or(|p| {
                    p.kind == ControlKind::Consent && p.enabled && p.after_ack.is_none()
                }))
            && (!self.enabled
                || (self.enrollment_status == EnrollmentStatus::Enrolled
                    && self.installation_id.as_deref().is_some_and(valid_uuid)
                    && self.user_id.as_deref().is_some_and(valid_uuid)
                    && self.consent_epoch > 0
                    && self.credential_ref.is_some()
                    && self.deletion_code_ref.is_some()
                    && self
                        .privacy_version
                        .as_deref()
                        .is_some_and(|s| !s.is_empty())
                    && self.terms_version.as_deref().is_some_and(|s| !s.is_empty())
                    && self
                        .consented_at
                        .as_deref()
                        .is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())))
            && self.pending_control_action.as_ref().is_none_or(|p| {
                valid_uuid(&p.request_id)
                    && (p.expected_epoch > 0
                        || self.enrollment_status == EnrollmentStatus::Enrolling
                        || self.enrollment_status == EnrollmentStatus::DeletionPending
                        || self.enrollment_status == EnrollmentStatus::Withdrawn)
            })
    }
}
/// Consent must not be repaired piecemeal by the general permissive state migration.
pub fn decode_state(value: Option<&Value>) -> FeedbackProgramState {
    value
        .cloned()
        .and_then(|v| serde_json::from_value::<FeedbackProgramState>(v).ok())
        .filter(FeedbackProgramState::structurally_valid)
        .unwrap_or_default()
}
pub fn deserialize_state<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<FeedbackProgramState, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(decode_state(Some(&value)))
}
pub fn valid_uuid(value: &str) -> bool {
    value.len() == 36 && uuid::Uuid::parse_str(value).is_ok()
}
pub fn provider_id(value: &str) -> Option<String> {
    if value.is_empty()
        || value.len() > 19
        || value.starts_with('0')
        || !value.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    value
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
}
pub fn provider_id_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => provider_id(s),
        Value::Number(n) => n.as_u64().and_then(|id| provider_id(&id.to_string())),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventOrder {
    pub rental_started: Option<u64>,
    pub ready: Option<u64>,
    pub paired: Option<u64>,
    pub setup_success: Option<u64>,
    pub first_stream_frame: Option<u64>,
    pub destroyed: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lifecycle {
    pub rental_started_at: String,
    pub ready_at: Option<String>,
    pub paired_at: Option<String>,
    pub setup_success_at: Option<String>,
    pub first_stream_frame_at: Option<String>,
    pub destroyed_at: Option<String>,
    pub observation_complete: bool,
    pub last_event_sequence: u64,
    pub event_order: EventOrder,
}
#[derive(Debug, Clone, Copy)]
pub enum Milestone {
    Ready,
    Paired,
    FirstFrame,
    Destroyed,
}
impl Lifecycle {
    pub fn new() -> Self {
        Self {
            rental_started_at: now(),
            observation_complete: true,
            last_event_sequence: 1,
            event_order: EventOrder {
                rental_started: Some(1),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    pub fn observe(&mut self, milestone: Milestone) -> bool {
        // Delayed evidence is allowed through observe_ordered; normal callbacks never invent pre-destroy order.
        if self.first_stream_frame_at.is_some()
            && matches!(milestone, Milestone::Ready | Milestone::Paired)
        {
            // Do not invent earlier evidence from a later verification. Ordered recovery
            // may still add a milestone if its original durable sequence is known.
            return false;
        }
        let order = self.last_event_sequence + 1;
        self.observe_ordered(milestone, now(), order)
    }
    pub fn observe_ordered(&mut self, milestone: Milestone, at: String, order: u64) -> bool {
        let (time, sequence) = match milestone {
            Milestone::Ready => (&mut self.ready_at, &mut self.event_order.ready),
            Milestone::Paired => (&mut self.paired_at, &mut self.event_order.paired),
            Milestone::FirstFrame => (
                &mut self.first_stream_frame_at,
                &mut self.event_order.first_stream_frame,
            ),
            Milestone::Destroyed => (&mut self.destroyed_at, &mut self.event_order.destroyed),
        };
        if time.is_some() || order <= 1 || self.event_order.destroyed.is_some_and(|d| order >= d) {
            return false;
        }
        *time = Some(at);
        *sequence = Some(order);
        self.last_event_sequence = self.last_event_sequence.max(order);
        if self.setup_success_at.is_none() && self.ready_at.is_some() && self.paired_at.is_some() {
            // Setup is logically simultaneous with the later prerequisite; sequence, not upload/wall time, orders it.
            self.setup_success_at = if self.event_order.ready > self.event_order.paired {
                self.ready_at.clone()
            } else {
                self.paired_at.clone()
            };
            self.event_order.setup_success = Some(
                self.event_order
                    .ready
                    .unwrap()
                    .max(self.event_order.paired.unwrap()),
            );
        }
        true
    }
    pub fn review_eligible(&self) -> bool {
        self.first_stream_frame_at.is_some()
            && self
                .event_order
                .first_stream_frame
                .is_some_and(|f| self.event_order.destroyed.is_none_or(|d| f < d))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub platform: String,
    pub app_version: String,
    pub collector_version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HardwareSnapshot {
    pub gpu_name: Option<String>,
    pub rented_gpu_count: Option<u32>,
    pub country: Option<String>,
    pub region: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JitterBucket {
    pub metric_version: String,
    pub route_mode: String,
    pub completeness: String,
    pub nominal_report_interval_ms: u64,
    pub threshold_version: String,
    pub high_threshold_ms: f64,
    pub severe_threshold_ms: f64,
    pub sample_count: u64,
    pub sum_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub high_sample_count: u64,
    pub severe_sample_count: u64,
    pub observed_duration_ms: u64,
    pub first_observed_at: String,
    pub last_observed_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub consent_epoch: u64,
    pub provider: String,
    pub provider_machine_id: String,
    pub provider_instance_id: String,
    pub provider_offer_id: Option<String>,
    pub lifecycle: Lifecycle,
    pub context: Context,
    pub hardware_snapshot: HardwareSnapshot,
    pub jitter_buckets: Vec<JitterBucket>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewDraft {
    pub revision: u64,
    pub score: Option<u8>,
    pub comment: Option<String>,
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl SessionSnapshot {
    pub fn valid(&self) -> bool {
        let l = &self.lifecycle;
        let o = &l.event_order;
        let milestones = [
            (&l.ready_at, o.ready),
            (&l.paired_at, o.paired),
            (&l.setup_success_at, o.setup_success),
            (&l.first_stream_frame_at, o.first_stream_frame),
            (&l.destroyed_at, o.destroyed),
        ];
        self.schema_version == SCHEMA_VERSION
            && self.revision > 0
            && self.revision <= 9_007_199_254_740_991
            && self.consent_epoch > 0
            && self.provider == "vast"
            && provider_id(&self.provider_machine_id).as_ref() == Some(&self.provider_machine_id)
            && provider_id(&self.provider_instance_id).as_ref() == Some(&self.provider_instance_id)
            && self
                .provider_offer_id
                .as_ref()
                .is_none_or(|id| provider_id(id).as_ref() == Some(id))
            && valid_time(&l.rental_started_at)
            && o.rental_started == Some(1)
            && l.last_event_sequence > 0
            && milestones.iter().all(|(time, sequence)| {
                time.is_some() == sequence.is_some()
                    && time.as_deref().is_none_or(valid_time)
                    && sequence.is_none_or(|n| n > 1 && n <= l.last_event_sequence)
            })
            && o.setup_success.is_none_or(|n| {
                o.ready.is_some_and(|r| r <= n)
                    && o.paired.is_some_and(|p| p <= n)
                    && o.destroyed.is_none_or(|d| n < d)
            })
            && o.first_stream_frame.is_none_or(|f| {
                [o.ready, o.paired, o.setup_success]
                    .iter()
                    .all(|n| n.is_none_or(|n| n <= f))
                    && o.destroyed.is_none_or(|d| f < d)
            })
            && matches!(
                self.context.platform.as_str(),
                "ios" | "android" | "macos" | "windows" | "linux"
            )
            && !self.context.app_version.is_empty()
            && !self.context.collector_version.is_empty()
            && self.hardware_snapshot.valid()
            && self.jitter_buckets.len() <= 12
            && self.jitter_buckets.iter().all(|b| {
                b.sample_count > 0
                    && b.sample_count <= 9_007_199_254_740_991
                    && b.sum_ms.is_finite()
                    && b.min_ms.is_finite()
                    && b.max_ms.is_finite()
                    && b.min_ms >= 0.0
                    && b.max_ms >= b.min_ms
                    && b.sum_ms >= 0.0
                    && b.high_sample_count <= b.sample_count
                    && b.severe_sample_count <= b.high_sample_count
                    && valid_time(&b.first_observed_at)
                    && valid_time(&b.last_observed_at)
                    && matches!(b.route_mode.as_str(), "direct" | "relay" | "mixed")
                    && b.nominal_report_interval_ms > 0
                    && !b.metric_version.is_empty()
            })
    }
}
impl HardwareSnapshot {
    pub fn valid(&self) -> bool {
        self.gpu_name
            .as_deref()
            .is_none_or(|s| s.encode_utf16().count() <= 128 && !s.chars().any(char::is_control))
            && self.rented_gpu_count.is_none_or(|n| n > 0)
            && self
                .country
                .as_deref()
                .is_none_or(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase()))
            && self
                .region
                .as_deref()
                .is_none_or(|s| s.encode_utf16().count() <= 80 && !s.chars().any(char::is_control))
    }
}
pub fn valid_time(value: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(value).is_ok()
}

// Public response projections deliberately discard unknown service fields, including any private IDs.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramDocument {
    pub schema_version: u32,
    pub available: bool,
    pub collection_paused: bool,
    pub privacy_version: String,
    pub terms_version: String,
    pub privacy_url: Option<String>,
    pub terms_url: Option<String>,
    pub deletion_url: Option<String>,
    pub contact: Option<String>,
    pub disclosure: String,
    pub metrics: Vec<ProgramMetric>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramMetric {
    pub metric_version: String,
    pub description: String,
    pub unit: String,
    pub nominal_report_interval_ms: u64,
    pub completeness: String,
    pub threshold_version: String,
    pub high_threshold_ms: f64,
    pub severe_threshold_ms: f64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummariesResponse {
    pub summaries: Vec<MachineSummary>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSummary {
    pub provider_machine_id: String,
    pub reliability: ReliabilitySummary,
    pub jitter: Option<JitterSummary>,
    pub reviews: ReviewSummary,
    pub as_of: Option<String>,
    #[serde(default)]
    pub jitter_groups: Vec<JitterGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_observed_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_date: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReliabilitySummary {
    pub source: String,
    pub ratio: Option<f64>,
    pub eligible_sessions: u64,
    pub successful_sessions: u64,
    pub failed_sessions: u64,
    pub basis: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_observed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_at: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JitterSummary {
    pub mean_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub sample_count: u64,
    pub contributing_sessions: u64,
    pub high_sample_ratio: f64,
    pub severe_sample_ratio: f64,
    pub metric_version: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal_report_interval_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completeness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_threshold_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severe_threshold_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_duration_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_observed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_at: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JitterGroup {
    #[serde(flatten)]
    pub statistics: JitterSummary,
    pub route_mode: String,
}
impl JitterSummary {
    pub fn valid(&self) -> bool {
        self.metric_version == METRIC_VERSION
            && self.sample_count > 0
            && self.contributing_sessions > 0
            && [self.mean_ms, self.min_ms, self.max_ms]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0)
            && self.min_ms <= self.mean_ms
            && self.mean_ms <= self.max_ms
            && (0.0..=1.0).contains(&self.high_sample_ratio)
            && (0.0..=self.high_sample_ratio).contains(&self.severe_sample_ratio)
            && self.nominal_report_interval_ms.is_none_or(|n| n == 1000)
            && self
                .completeness
                .as_deref()
                .is_none_or(|s| s == "full_window")
            && self
                .threshold_version
                .as_deref()
                .is_none_or(|s| s == THRESHOLD_VERSION)
            && self.high_threshold_ms.is_none_or(|n| n == 7.0)
            && self.severe_threshold_ms.is_none_or(|n| n == 15.0)
            && self.first_observed_at.as_deref().is_none_or(valid_time)
            && self.last_observed_at.as_deref().is_none_or(valid_time)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSummary {
    pub comment_count: u64,
    pub rating_count: u64,
    pub average_score: Option<f64>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewsResponse {
    pub reviews: Vec<PublishedReview>,
    pub next_cursor: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedReview {
    pub review_id: String,
    pub score: Option<u8>,
    pub comment: Option<String>,
    pub published_date: String,
    pub hardware_snapshot: Option<HardwareSnapshot>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feedback_defaults_and_malformed_consent_fail_closed() {
        for v in [
            serde_json::json!(null),
            serde_json::json!({"enabled": true}),
            serde_json::json!({"schemaVersion": 99, "enabled": true}),
        ] {
            assert!(!decode_state(Some(&v)).enabled);
        }
    }
    #[test]
    fn provider_ids_never_round_or_use_host_offer_aliases() {
        assert_eq!(
            provider_id_value(Some(&serde_json::json!(9007199254740993u64))),
            Some("9007199254740993".into())
        );
        assert_eq!(provider_id("9223372036854775808"), None);
        assert_eq!(provider_id("0"), None);
        assert_eq!(provider_id("1.5"), None);
    }
    #[test]
    fn connection_is_not_a_frame_and_reconnect_does_not_duplicate_milestones() {
        let mut l = Lifecycle::new();
        l.observe(Milestone::Ready);
        l.observe(Milestone::Paired);
        assert!(l.setup_success_at.is_some());
        assert!(!l.review_eligible());
        let sequence = l.last_event_sequence;
        assert!(!l.observe(Milestone::Paired));
        assert_eq!(l.last_event_sequence, sequence);
        l.observe(Milestone::FirstFrame);
        assert!(l.review_eligible());
    }
    #[test]
    fn public_summary_projection_preserves_provenance_but_omits_private_fields() {
        let jitter = serde_json::json!({"meanMs": 2.0, "minMs": 0.0, "maxMs": 3.0, "sampleCount": 60,
            "contributingSessions": 1, "highSampleRatio": 0.0, "severeSampleRatio": 0.0,
            "metricVersion": METRIC_VERSION, "scope": "all_routes", "nominalReportIntervalMs": 1000,
            "completeness": "full_window", "thresholdVersion": THRESHOLD_VERSION, "highThresholdMs": 7.0,
            "severeThresholdMs": 15.0, "observedDurationMs": 60000, "firstObservedAt": now(), "lastObservedAt": now()});
        let mut group = jitter.clone();
        group["scope"] = serde_json::json!("route");
        group["routeMode"] = serde_json::json!("direct");
        let value = serde_json::json!({"providerMachineId": "123", "reliability": {"source": "no_land", "ratio": null,
            "eligibleSessions": 1, "successfulSessions": 1, "failedSessions": 0, "basis": "ready_and_paired_before_destroy",
            "scope": "retained_history", "firstObservedAt": now(), "lastObservedAt": now()}, "jitter": jitter,
            "jitterGroups": [group], "reviews": {"commentCount": 0, "ratingCount": 0, "averageScore": null},
            "asOf": now(), "sessionId": "must-not-cross-webview", "userId": "must-not-cross-webview"});
        let summary: MachineSummary = serde_json::from_value(value).unwrap();
        assert!(summary.jitter.as_ref().unwrap().valid());
        let projected = serde_json::to_value(summary).unwrap();
        assert_eq!(projected["jitterGroups"][0]["routeMode"], "direct");
        assert_eq!(projected["jitter"]["nominalReportIntervalMs"], 1000);
        assert!(projected["reliability"]["firstObservedAt"].is_string());
        assert!(projected.get("sessionId").is_none());
        assert!(projected.get("userId").is_none());
    }
    #[test]
    fn never_backdates_later_verification_to_before_a_frame() {
        let mut l = Lifecycle::new();
        l.observe(Milestone::FirstFrame);
        assert!(!l.observe(Milestone::Paired));
        assert!(l.setup_success_at.is_none());
    }
    #[test]
    fn typed_program_round_trip_does_not_rescue_partial_or_future_consent() {
        let mut p = FeedbackProgramState::default();
        p.enabled = true;
        assert!(!decode_state(Some(&serde_json::to_value(&p).unwrap())).enabled);
        p.enrollment_status = EnrollmentStatus::Enrolled;
        p.installation_id = Some(uuid::Uuid::new_v4().to_string());
        p.user_id = Some(uuid::Uuid::new_v4().to_string());
        p.credential_ref = Some("ref-only".into());
        p.deletion_code_ref = Some("ref-only".into());
        p.privacy_version = Some("1".into());
        p.terms_version = Some("1".into());
        p.consented_at = Some(now());
        p.consent_epoch = 1;
        let mut value = serde_json::to_value(&p).unwrap();
        assert!(decode_state(Some(&value)).enabled);
        value["schemaVersion"] = serde_json::json!(99);
        assert!(!decode_state(Some(&value)).enabled);
        value["schemaVersion"] = serde_json::json!(1);
        value["enabled"] = serde_json::json!("true");
        assert!(!decode_state(Some(&value)).enabled);
    }
    #[test]
    fn delayed_earlier_evidence_preserves_durable_order() {
        let mut l = Lifecycle::new();
        l.observe_ordered(Milestone::Destroyed, now(), 9);
        assert!(l.observe_ordered(Milestone::Ready, now(), 3));
        assert!(l.observe_ordered(Milestone::Paired, now(), 4));
        assert!(l.setup_success_at.is_some());
        assert_eq!(l.last_event_sequence, 9);
        assert!(!l.observe(Milestone::FirstFrame));
    }
}
