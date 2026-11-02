//! Additive provenance for the existing RTT jitter gauge. Legacy agents without this evidence are N/A.
use serde::{Deserialize, Serialize};
pub const RTT_JITTER_METRIC_VERSION: &str = "rtt_jitter_ewma_window60_report1s_v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementEvidence {
    pub metric_version: String,
    pub report_id: String,
    pub highest_sequence: u64,
    pub highest_received_sequence: Option<u64>,
    pub received_rtts: u64,
    pub window_age_ms: u64,
    pub sample_age_ms: u64,
}
