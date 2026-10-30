use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{models::app_state::TransportKind, services::app_context::AppContext};
use noland_network_contracts::feedback::MeasurementEvidence;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::RwLock;
use tracing::warn;
use uuid::Uuid;

use super::status::FailSafeTransition;

const STATS_EVENT: &str = "network-monitor://stats";
const STATUS_EVENT: &str = "network-monitor://status";
const ERROR_EVENT: &str = "network-monitor://error";

#[derive(Clone)]
pub(crate) struct Reporter {
    app: AppHandle,
    session_id: Uuid,
    latest_stats: Arc<RwLock<Option<(Instant, Value)>>>,
    last_report: Arc<RwLock<Option<Instant>>>,
    rental: Option<u64>,
    batches: Arc<RwLock<VecDeque<ReportedBatch>>>,
}
#[derive(Clone)]
struct ReportedBatch {
    id: String,
    highest: u64,
    at: Instant,
    route: &'static str,
}

fn take_fresh_batch(
    batches: &mut VecDeque<ReportedBatch>,
    evidence: &MeasurementEvidence,
    now: Instant,
) -> Option<ReportedBatch> {
    let index = batches
        .iter()
        .position(|b| b.id == evidence.report_id && b.highest == evidence.highest_sequence)?;
    batches
        .remove(index)
        .filter(|b| now.saturating_duration_since(b.at) < Duration::from_secs(3))
}

#[cfg(test)]
mod feedback_tests {
    use super::*;
    #[test]
    fn a_new_send_cannot_freshen_an_old_heartbeat_and_echo_is_consumed_once() {
        let now = Instant::now();
        let mut batches = VecDeque::from([
            ReportedBatch {
                id: "old".into(),
                highest: 10,
                at: now - Duration::from_secs(5),
                route: "direct",
            },
            ReportedBatch {
                id: "new".into(),
                highest: 20,
                at: now,
                route: "relay",
            },
        ]);
        let mut e = MeasurementEvidence {
            metric_version: crate::models::feedback::METRIC_VERSION.into(),
            report_id: "old".into(),
            highest_sequence: 10,
            highest_received_sequence: Some(10),
            received_rtts: 2,
            window_age_ms: 60_000,
            sample_age_ms: 0,
        };
        assert!(take_fresh_batch(&mut batches, &e, now).is_none());
        e.report_id = "new".into();
        assert!(take_fresh_batch(&mut batches, &e, now).is_none()); // mismatched watermark
        e.highest_sequence = 20;
        assert_eq!(
            take_fresh_batch(&mut batches, &e, now).unwrap().route,
            "relay"
        );
        assert!(take_fresh_batch(&mut batches, &e, now).is_none());
    }
}

impl Reporter {
    pub(crate) fn new(
        app: AppHandle,
        session_id: Uuid,
        rental: Option<u64>,
        latest_stats: Arc<RwLock<Option<(Instant, Value)>>>,
    ) -> Self {
        Self {
            app,
            session_id,
            latest_stats,
            last_report: Arc::new(RwLock::new(None)),
            rental,
            batches: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    pub(crate) fn feedback_report_id(&self) -> Option<String> {
        let rental = self.rental?;
        let context = self.app.try_state::<AppContext>()?;
        context
            .feedback
            .measurement_allowed(rental)
            .then(|| Uuid::new_v4().to_string())
    }
    pub(crate) async fn measurements_reported(&self, id: Option<&str>, highest: u64) {
        let at = Instant::now();
        *self.last_report.write().await = Some(at);
        let Some(id) = id else {
            return;
        }; // Operational monitoring needs no feedback-specific batch metadata while off.
        let route = if let (Some(rental), Some(context)) =
            (self.rental, self.app.try_state::<AppContext>())
        {
            let state = context.load_state().await;
            state
                .provisioned_servers
                .iter()
                .find(|r| r.instance_id == rental)
                .and_then(|r| r.network.active_transport.as_ref())
                .map_or("mixed", |t| match t {
                    TransportKind::Direct => "direct",
                    TransportKind::CloudflareTurn => "relay",
                })
        } else {
            "mixed"
        };
        let mut batches = self.batches.write().await;
        batches.push_back(ReportedBatch {
            id: id.into(),
            highest,
            at,
            route,
        });
        while batches.len() > 8 {
            batches.pop_front();
        }
    }

    async fn contribute(&self, value: &Value) {
        let Some(rental) = self.rental else {
            return;
        };
        let Some(context) = self.app.try_state::<AppContext>() else {
            return;
        };
        let Ok(evidence) =
            serde_json::from_value::<MeasurementEvidence>(value["measurementEvidence"].clone())
        else {
            return;
        };
        // An echo must identify a batch this monitor actually sent. A newly sent batch
        // cannot freshen an older agent heartbeat, even if the heartbeat arrives later.
        let batch = {
            let mut batches = self.batches.write().await;
            take_fresh_batch(&mut batches, &evidence, Instant::now())
        };
        let Some(batch) = batch else {
            return;
        };
        let Some(jitter) = value["metrics"]["last60Seconds"]["jitterMs"].as_f64() else {
            return;
        };
        if value["metrics"]["last60Seconds"]["received"].as_u64() != Some(evidence.received_rtts) {
            return;
        }
        context.feedback.jitter(
            rental,
            &self.session_id.to_string(),
            &evidence,
            batch.at,
            jitter,
            batch.route,
        );
    }

    pub(crate) async fn forward_agent_text(&self, text: &str) {
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(error) => {
                self.error(format!("agent sent invalid JSON: {error}"));
                return;
            }
        };

        match value.get("type").and_then(Value::as_str) {
            Some("stats_update") => {
                if value["sessionId"].as_str() != Some(self.session_id.to_string().as_str()) {
                    return;
                }
                // Agent heartbeats can repeat old statistics. Freshness follows
                // actual client measurement reports, including measured timeouts.
                if let Some(measured_at) = *self.last_report.read().await {
                    *self.latest_stats.write().await = Some((measured_at, value.clone()));
                }
                self.contribute(&value).await;
                self.emit(STATS_EVENT, value);
            }
            Some("status_changed") => {
                self.notify_if_alert_eligible(&value);
                self.emit(STATUS_EVENT, value);
            }
            Some("error") => self.emit(ERROR_EVENT, value),
            Some(message_type) => {
                self.error(format!(
                    "agent sent unsupported message type {message_type:?}"
                ));
            }
            None => self.error("agent message is missing a string type"),
        }
    }

    pub(crate) fn fail_safe(&self, transition: FailSafeTransition) {
        let value = match transition {
            FailSafeTransition::ConnectionLost => json!({
                "type": "status_changed",
                "sessionId": self.session_id,
                "previous": "GOOD",
                "current": "BAD",
                "reasons": ["CONNECTION_LOST"],
                "keyMetrics": {
                    "medianRttMs": null,
                    "p95RttMs": null,
                    "jitterMs": 0.0,
                    "lossPercent": 100.0,
                    "spikePercent": 0.0,
                    "longestLossBurst": 0
                },
                "alertEligible": true,
                "source": "client_fail_safe"
            }),
            FailSafeTransition::Recovered => json!({
                "type": "status_changed",
                "sessionId": self.session_id,
                "previous": "BAD",
                "current": "GOOD",
                "reasons": [],
                "keyMetrics": {
                    "medianRttMs": null,
                    "p95RttMs": null,
                    "jitterMs": 0.0,
                    "lossPercent": 0.0,
                    "spikePercent": 0.0,
                    "longestLossBurst": 0
                },
                "alertEligible": false,
                "source": "client_fail_safe"
            }),
        };
        self.notify_if_alert_eligible(&value);
        self.emit(STATUS_EVENT, value);
    }

    fn notify_if_alert_eligible(&self, value: &Value) {
        let is_bad = value.get("current").and_then(Value::as_str) == Some("BAD");
        let alert_eligible = value
            .get("alertEligible")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !is_bad || !alert_eligible {
            return;
        }

        let connection_lost =
            value
                .get("reasons")
                .and_then(Value::as_array)
                .is_some_and(|reasons| {
                    reasons
                        .iter()
                        .any(|reason| reason.as_str() == Some("CONNECTION_LOST"))
                });
        let (title, body) = if connection_lost {
            (
                "No Land — Connection lost",
                "The connection to your gaming PC appears to be lost.",
            )
        } else {
            (
                "No Land — Connection unstable",
                "High latency variation or packet loss may affect streaming.",
            )
        };

        if let Err(error) = self
            .app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .sound("Glass")
            .show()
        {
            warn!(%error, "failed to show native network warning");
        }
    }

    pub(crate) fn error(&self, message: impl Into<String>) {
        self.emit(
            ERROR_EVENT,
            json!({
                "type": "error",
                "sessionId": self.session_id,
                "message": message.into(),
                "source": "network_monitor"
            }),
        );
    }

    fn emit(&self, event: &str, payload: Value) {
        if let Err(error) = self.app.emit(event, payload) {
            warn!(event, %error, "failed to emit network monitor event");
        }
    }
}
