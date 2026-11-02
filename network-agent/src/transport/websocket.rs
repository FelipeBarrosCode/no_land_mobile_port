use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context};
use futures_util::{SinkExt, StreamExt};
use noland_network_contracts::control::{AuthHello, CONTROL_SUBPROTOCOL};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    net::{TcpListener, TcpStream},
    time::{interval, MissedTickBehavior},
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        handshake::server::{Request, Response},
        http::HeaderValue,
        Message,
    },
    WebSocketStream,
};
use uuid::Uuid;

use crate::{
    classifier::{classify, thresholds::ClassifierThresholds, Classification, ReasonCode},
    remote_control::RemoteControl,
    telemetry::{
        metrics::{snapshot, MetricsSnapshot},
        sample::MeasurementSample,
        state::StatusTransition,
    },
    SharedState,
};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Auth {
        #[serde(rename = "sessionId")]
        session_id: Uuid,
        token: String,
    },
    MeasurementReport {
        #[serde(rename = "sessionId")]
        session_id: Uuid,
        samples: Vec<MeasurementSample>,
        #[serde(default, rename = "reportId")]
        report_id: Option<String>,
    },
    GetState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StateView {
    published_status: crate::classifier::Quality,
    bad_since: Option<u64>,
    alert_eligible: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatsUpdate {
    #[serde(rename = "type")]
    message_type: &'static str,
    session_id: Uuid,
    generated_at_ms: u64,
    runtime_seconds: u64,
    metrics: MetricsSnapshot,
    measurement_evidence: Option<noland_network_contracts::feedback::MeasurementEvidence>,
    classification: Classification,
    state: StateView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyMetrics {
    median_rtt_ms: Option<f64>,
    p95_rtt_ms: Option<f64>,
    jitter_ms: f64,
    loss_percent: f64,
    spike_percent: f64,
    longest_loss_burst: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusChanged {
    #[serde(rename = "type")]
    message_type: &'static str,
    session_id: Uuid,
    previous: crate::classifier::Quality,
    current: crate::classifier::Quality,
    reasons: Vec<ReasonCode>,
    key_metrics: KeyMetrics,
    alert_eligible: bool,
}

struct Evaluation {
    stats: StatsUpdate,
    transition: Option<StatusChanged>,
}

pub async fn run(
    addr: SocketAddr,
    shared: SharedState,
    thresholds: Arc<ClassifierThresholds>,
    remote_control: Option<Arc<RemoteControl>>,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let shared = shared.clone();
        let thresholds = thresholds.clone();
        let remote_control = remote_control.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, shared, thresholds, remote_control).await
            {
                eprintln!("WebSocket connection closed: {error:#}");
            }
        });
    }
}

#[allow(clippy::result_large_err)]
async fn handle_connection(
    stream: TcpStream,
    shared: SharedState,
    thresholds: Arc<ClassifierThresholds>,
    remote_control: Option<Arc<RemoteControl>>,
) -> anyhow::Result<()> {
    let selected_control = Arc::new(AtomicBool::new(false));
    let selected_control_for_handshake = selected_control.clone();
    let control_available = remote_control.is_some();
    let mut websocket =
        accept_hdr_async(stream, move |request: &Request, mut response: Response| {
            let offered_control = request
                .headers()
                .get("sec-websocket-protocol")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    value
                        .split(',')
                        .any(|protocol| protocol.trim() == CONTROL_SUBPROTOCOL)
                });
            if offered_control && control_available {
                response.headers_mut().insert(
                    "sec-websocket-protocol",
                    HeaderValue::from_static(CONTROL_SUBPROTOCOL),
                );
                selected_control_for_handshake.store(true, Ordering::Release);
            }
            Ok(response)
        })
        .await
        .context("WebSocket handshake failed")?;
    let first = websocket
        .next()
        .await
        .ok_or_else(|| anyhow!("connection closed before auth"))??;
    if selected_control.load(Ordering::Acquire) {
        let remote_control = remote_control
            .context("privileged control is unavailable because no host secret is installed")?;
        let hello = parse_json_message::<AuthHello>(first)
            .context("first control message must be AuthHello")?;
        return remote_control.handle(websocket, hello).await;
    }
    let ClientMessage::Auth { session_id, token } = parse_client_message(first)? else {
        send_error(
            &mut websocket,
            "AUTH_REQUIRED",
            "first message must be auth",
        )
        .await?;
        return Ok(());
    };
    let token = decode_token(&token).ok_or_else(|| anyhow!("invalid auth token"));
    let token = match token {
        Ok(token) => token,
        Err(error) => {
            send_error(
                &mut websocket,
                "INVALID_TOKEN",
                "token must be exactly 64 hexadecimal characters",
            )
            .await?;
            return Err(error);
        }
    };

    {
        let mut registry = shared.lock().await;
        registry.register(session_id, token, Instant::now());
    }
    send_evaluation(
        &mut websocket,
        evaluate(&shared, session_id, &token, &thresholds).await?,
    )
    .await?;

    let mut ticker = interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let evaluation = match evaluate(&shared, session_id, &token, &thresholds).await {
                    Ok(evaluation) => evaluation,
                    Err(_) => {
                        send_error(&mut websocket, "SESSION_REPLACED", "session is no longer registered with this connection").await?;
                        return Ok(());
                    }
                };
                send_evaluation(&mut websocket, evaluation).await?;
            }
            incoming = websocket.next() => {
                let Some(incoming) = incoming else { return Ok(()); };
                let message = match parse_client_message(incoming?) {
                    Ok(message) => message,
                    Err(error) => {
                        send_error(&mut websocket, "INVALID_MESSAGE", &error.to_string()).await?;
                        continue;
                    }
                };
                match message {
                    ClientMessage::Auth { .. } => {
                        send_error(&mut websocket, "ALREADY_AUTHENTICATED", "auth is only accepted as the first message").await?;
                    }
                    ClientMessage::GetState => {
                        let evaluation = evaluate(&shared, session_id, &token, &thresholds).await?;
                        send_evaluation(&mut websocket, evaluation).await?;
                    }
                    ClientMessage::MeasurementReport { session_id: report_session, samples, report_id } => {
                        if report_session != session_id {
                            send_error(&mut websocket, "WRONG_SESSION", "report sessionId does not match authenticated session").await?;
                            continue;
                        }
                        if samples.len() > 100 {
                            send_error(&mut websocket, "TOO_MANY_SAMPLES", "reports may contain at most 100 samples").await?;
                            continue;
                        }
                        if !samples.iter().all(MeasurementSample::is_valid) {
                            send_error(&mut websocket, "INVALID_SAMPLE", "successful samples require a non-negative rttMs and lost samples must omit it").await?;
                            continue;
                        }
                        {
                            let mut registry = shared.lock().await;
                            let Some(session) = registry.get_mut(&session_id) else {
                                send_error(&mut websocket, "UNKNOWN_SESSION", "session is not registered").await?;
                                return Ok(());
                            };
                            if session.token != token {
                                send_error(&mut websocket, "SESSION_REPLACED", "session is no longer registered with this connection").await?;
                                return Ok(());
                            }
                            let measured_at = Instant::now();
                            if let Some(report_id) = report_id.filter(|id| Uuid::parse_str(id).is_ok()) {
                                if let Some(highest) = samples.iter().map(|s| s.sequence).max() {
                                    let received = samples.iter().filter(|s| !s.lost && s.rtt_ms.is_some()).map(|s| s.sequence).max();
                                    if session.feedback_evidence.as_ref().is_none_or(|(_, previous, _, _)| highest > *previous) {
                                        let previous_received = session.feedback_evidence.as_ref().and_then(|(_, _, r, _)| *r);
                                        session.feedback_evidence = Some((report_id, highest, received.max(previous_received), measured_at));
                                    }
                                }
                            }
                            session.window.extend(samples, measured_at);
                        }
                        let evaluation = evaluate(&shared, session_id, &token, &thresholds).await?;
                        send_evaluation(&mut websocket, evaluation).await?;
                    }
                }
            }
        }
    }
}

fn parse_client_message(message: Message) -> anyhow::Result<ClientMessage> {
    parse_json_message(message)
}

fn parse_json_message<T: serde::de::DeserializeOwned>(message: Message) -> anyhow::Result<T> {
    match message {
        Message::Text(text) => serde_json::from_str(text.as_ref()).context("invalid JSON message"),
        _ => Err(anyhow!("expected a text JSON message")),
    }
}

async fn evaluate(
    shared: &SharedState,
    session_id: Uuid,
    token: &[u8; 32],
    thresholds: &ClassifierThresholds,
) -> anyhow::Result<Evaluation> {
    let now = Instant::now();
    let generated_at_ms = unix_time_ms();
    let mut registry = shared.lock().await;
    let session = registry
        .get_mut(&session_id)
        .ok_or_else(|| anyhow!("session not registered"))?;
    if &session.token != token {
        return Err(anyhow!("session replaced"));
    }

    session.window.prune(now);
    let runtime_seconds = session.runtime(now).as_secs();
    let metrics = snapshot(&session.window, now);
    let measurement_evidence =
        session
            .feedback_evidence
            .as_ref()
            .map(|(report_id, highest, received, at)| {
                noland_network_contracts::feedback::MeasurementEvidence {
                    metric_version: noland_network_contracts::feedback::RTT_JITTER_METRIC_VERSION
                        .into(),
                    report_id: report_id.clone(),
                    highest_sequence: *highest,
                    highest_received_sequence: *received,
                    received_rtts: metrics.last_60_seconds.received as u64,
                    window_age_ms: session.window.first_sample_at().map_or(0, |first| {
                        now.saturating_duration_since(first).as_millis() as u64
                    }),
                    sample_age_ms: now.saturating_duration_since(*at).as_millis() as u64,
                }
            });
    let classification = classify(&session.window, now, thresholds);
    let transition = session.state.evaluate(&classification, now);
    let state = StateView {
        published_status: session.state.published(),
        bad_since: session.state.bad_since().map(|bad_since| {
            generated_at_ms
                .saturating_sub(now.saturating_duration_since(bad_since).as_millis() as u64)
        }),
        alert_eligible: session.state.alert_eligible(),
    };
    let transition = transition.map(|transition| status_changed(session_id, transition, &metrics));

    Ok(Evaluation {
        stats: StatsUpdate {
            message_type: "stats_update",
            session_id,
            generated_at_ms,
            runtime_seconds,
            metrics,
            measurement_evidence,
            classification,
            state,
        },
        transition,
    })
}

fn status_changed(
    session_id: Uuid,
    transition: StatusTransition,
    metrics: &MetricsSnapshot,
) -> StatusChanged {
    let window = &metrics.last_60_seconds;
    StatusChanged {
        message_type: "status_changed",
        session_id,
        previous: transition.previous,
        current: transition.current,
        reasons: transition.reasons,
        key_metrics: KeyMetrics {
            median_rtt_ms: window.median_rtt_ms,
            p95_rtt_ms: window.p95_rtt_ms,
            jitter_ms: window.jitter_ms,
            loss_percent: window.loss_percent,
            spike_percent: window.spike_percent,
            longest_loss_burst: window.longest_loss_burst,
        },
        alert_eligible: transition.alert_eligible,
    }
}

async fn send_evaluation(
    websocket: &mut WebSocketStream<TcpStream>,
    evaluation: Evaluation,
) -> anyhow::Result<()> {
    websocket
        .send(Message::Text(serde_json::to_string(&evaluation.stats)?))
        .await?;
    if let Some(transition) = evaluation.transition {
        websocket
            .send(Message::Text(serde_json::to_string(&transition)?))
            .await?;
    }
    Ok(())
}

async fn send_error(
    websocket: &mut WebSocketStream<TcpStream>,
    code: &str,
    message: &str,
) -> anyhow::Result<()> {
    websocket
        .send(Message::Text(
            json!({"type": "error", "code": code, "message": message}).to_string(),
        ))
        .await?;
    Ok(())
}

fn decode_token(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let decoded = hex::decode(value).ok()?;
    decoded.try_into().ok()
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::decode_token;

    #[tokio::test]
    async fn evidence_echoes_the_processed_batch_and_counts_received_rtts_not_sends() {
        let shared = crate::shared_state(4, 20);
        let id = uuid::Uuid::from_u128(1);
        let token = [7; 32];
        let at = std::time::Instant::now();
        {
            let mut registry = shared.lock().await;
            registry.register(id, token, at);
            let session = registry.get_mut(&id).unwrap();
            session.window.extend(
                vec![
                    crate::telemetry::sample::MeasurementSample {
                        sequence: 1,
                        rtt_ms: Some(10.0),
                        lost: false,
                    },
                    crate::telemetry::sample::MeasurementSample {
                        sequence: 2,
                        rtt_ms: None,
                        lost: true,
                    },
                    crate::telemetry::sample::MeasurementSample {
                        sequence: 3,
                        rtt_ms: Some(20.0),
                        lost: false,
                    },
                ],
                at,
            );
            session.feedback_evidence = Some(("processed-batch".into(), 3, Some(3), at));
        }
        let result = super::evaluate(
            &shared,
            id,
            &token,
            &crate::classifier::thresholds::ClassifierThresholds::default(),
        )
        .await
        .unwrap();
        let evidence = result.stats.measurement_evidence.unwrap();
        assert_eq!(evidence.report_id, "processed-batch");
        assert_eq!(evidence.highest_sequence, 3);
        assert_eq!(evidence.received_rtts, 2);
        assert_eq!(result.stats.metrics.last_60_seconds.sent, 3);
        assert_eq!(
            evidence.metric_version,
            noland_network_contracts::feedback::RTT_JITTER_METRIC_VERSION
        );
    }
    #[test]
    fn token_requires_exact_hex_length_and_accepts_uppercase() {
        assert_eq!(decode_token(&"AA".repeat(32)), Some([0xaa; 32]));
        assert!(decode_token(&"aa".repeat(31)).is_none());
        assert!(decode_token(&"gg".repeat(32)).is_none());
    }
}
