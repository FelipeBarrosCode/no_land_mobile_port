use crate::models::feedback::{now, JitterBucket, METRIC_VERSION, THRESHOLD_VERSION};
use noland_network_contracts::feedback::MeasurementEvidence;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct MetricTracker {
    monitor: Option<String>,
    highest: Option<u64>,
    received: Option<u64>,
    first: Option<Instant>,
    last_report: Option<Instant>,
    last_accepted: Option<Instant>,
    route: Option<String>,
    route_changed: Option<Instant>,
}
impl MetricTracker {
    pub fn boundary(&self, monitor: &str, route: &str) -> bool {
        self.monitor.as_deref().is_some_and(|old| old != monitor)
            || self.route.as_deref().is_some_and(|old| old != route)
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn observe(
        &mut self,
        monitor: &str,
        evidence: &MeasurementEvidence,
        measured_at: Instant,
        value: f64,
        route: &str,
        now_mono: Instant,
    ) -> Option<(String, u64)> {
        if evidence.metric_version != METRIC_VERSION
            || !value.is_finite()
            || value < 0.0
            || evidence.received_rtts < 2
            || evidence.sample_age_ms >= 3000
            || now_mono.saturating_duration_since(measured_at) >= Duration::from_secs(3)
        {
            return None;
        }
        if self.monitor.as_deref() != Some(monitor) {
            self.reset();
            self.monitor = Some(monitor.into());
        }
        if self
            .highest
            .is_some_and(|old| evidence.highest_sequence <= old)
        {
            return None;
        }
        self.highest = Some(evidence.highest_sequence);
        // Loss-only batches and repeated heartbeats cannot resample an old jitter gauge.
        let received = evidence.highest_received_sequence?;
        if received > evidence.highest_sequence {
            return None;
        }
        if self.received.is_some_and(|old| received <= old) {
            return None;
        }
        self.received = Some(received);
        if self
            .last_report
            .is_some_and(|at| measured_at.saturating_duration_since(at) > Duration::from_secs(3))
        {
            self.first = None;
            self.last_accepted = None;
        }
        self.last_report = Some(measured_at);
        let first = *self.first.get_or_insert(measured_at);
        let route = match route {
            "direct" => "direct",
            "relay" => "relay",
            _ => "mixed",
        };
        if self.route.as_deref().is_some_and(|old| old != route) {
            self.route_changed = Some(measured_at);
        }
        self.route = Some(route.into());
        if evidence.window_age_ms < 60_000
            || measured_at.saturating_duration_since(first) < Duration::from_secs(60)
        {
            return None;
        }
        // Fixed 1s cadence; extra fresh replies must not over-weight fast-report clients.
        if self.last_accepted.is_some_and(|at| {
            measured_at.saturating_duration_since(at) < Duration::from_millis(900)
        }) {
            return None;
        }
        let duration = self.last_accepted.map_or(1000, |at| {
            measured_at
                .saturating_duration_since(at)
                .as_millis()
                .min(1000) as u64
        });
        self.last_accepted = Some(measured_at);
        let mixed = self
            .route_changed
            .is_some_and(|at| measured_at.saturating_duration_since(at) < Duration::from_secs(60));
        Some((if mixed { "mixed" } else { route }.into(), duration))
    }
}
pub fn accumulate(buckets: &mut Vec<JitterBucket>, route: &str, value: f64, duration: u64) {
    let at = now();
    let bucket = if let Some(index) = buckets.iter().position(|b| {
        b.metric_version == METRIC_VERSION
            && b.route_mode == route
            && b.completeness == "full_window"
            && b.nominal_report_interval_ms == 1000
            && b.threshold_version == THRESHOLD_VERSION
    }) {
        &mut buckets[index]
    } else {
        if buckets.len() >= 12 {
            return;
        }
        buckets.push(JitterBucket {
            metric_version: METRIC_VERSION.into(),
            route_mode: route.into(),
            completeness: "full_window".into(),
            nominal_report_interval_ms: 1000,
            threshold_version: THRESHOLD_VERSION.into(),
            high_threshold_ms: 7.0,
            severe_threshold_ms: 15.0,
            sample_count: 0,
            sum_ms: 0.0,
            min_ms: value,
            max_ms: value,
            high_sample_count: 0,
            severe_sample_count: 0,
            observed_duration_ms: 0,
            first_observed_at: at.clone(),
            last_observed_at: at.clone(),
        });
        buckets.last_mut().unwrap()
    };
    if !(bucket.sum_ms + value).is_finite() {
        return;
    }
    bucket.sample_count += 1;
    bucket.sum_ms += value;
    bucket.min_ms = bucket.min_ms.min(value);
    bucket.max_ms = bucket.max_ms.max(value);
    bucket.high_sample_count += u64::from(value > 7.0);
    bucket.severe_sample_count += u64::from(value > 15.0);
    bucket.observed_duration_ms += duration;
    // The metric/cadence use monotonic time; a wall-clock correction must not regress a cumulative bucket.
    if chrono::DateTime::parse_from_rfc3339(&at).ok()
        >= chrono::DateTime::parse_from_rfc3339(&bucket.last_observed_at).ok()
    {
        bucket.last_observed_at = at;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn evidence(n: u64) -> MeasurementEvidence {
        MeasurementEvidence {
            metric_version: METRIC_VERSION.into(),
            report_id: uuid::Uuid::new_v4().to_string(),
            highest_sequence: n,
            highest_received_sequence: Some(n),
            received_rtts: 300,
            window_age_ms: 60_000,
            sample_age_ms: 0,
        }
    }
    #[test]
    fn rejects_duplicate_stale_loss_only_and_startup_not_real_zero() {
        let start = Instant::now();
        let mut tracker = MetricTracker::default();
        let e = evidence(1);
        assert!(tracker
            .observe("monitor", &e, start, 0.0, "direct", start)
            .is_none());
        for n in 2..=60 {
            let at = start + Duration::from_secs(n - 1);
            assert!(tracker
                .observe("monitor", &evidence(n), at, 0.0, "direct", at)
                .is_none());
        }
        let at = start + Duration::from_secs(60);
        let e = evidence(61);
        assert!(tracker
            .observe("monitor", &e, at, 0.0, "direct", at)
            .is_some());
        assert!(tracker
            .observe("monitor", &e, at, 0.0, "direct", at)
            .is_none());
        assert!(tracker
            .observe(
                "monitor",
                &evidence(62),
                at,
                0.0,
                "direct",
                at + Duration::from_secs(4)
            )
            .is_none());
        let mut loss = evidence(63);
        loss.highest_received_sequence = Some(61);
        assert!(tracker
            .observe("monitor", &loss, at, 0.0, "direct", at)
            .is_none());
    }
    #[test]
    fn mixed_window_and_weighted_totals() {
        let mut buckets = vec![];
        for _ in 0..60 {
            accumulate(&mut buckets, "direct", 2.0, 1000);
        }
        for _ in 0..600 {
            accumulate(&mut buckets, "relay", 8.0, 1000);
        }
        let sum: f64 = buckets.iter().map(|b| b.sum_ms).sum();
        let count: u64 = buckets.iter().map(|b| b.sample_count).sum();
        assert!((sum / count as f64 - 7.454545).abs() < 0.00001);
        let start = Instant::now();
        let mut tracker = MetricTracker::default();
        for n in 1..=61 {
            let at = start + Duration::from_secs(n);
            tracker.observe("m", &evidence(n), at, 2.0, "direct", at);
        }
        let at = start + Duration::from_secs(62);
        assert_eq!(
            tracker
                .observe("m", &evidence(62), at, 2.0, "relay", at)
                .unwrap()
                .0,
            "mixed"
        );
        assert_eq!(buckets[0].high_sample_count, 0);
        assert_eq!(buckets[1].high_sample_count, 600);
    }
}
