use super::*;

impl FeedbackProgramService {
    /// Called synchronously at Vast's accepted-contract boundary, before fetching details.
    pub fn rental_accepted(&self, intent: &str, rental: u64) {
        let Ok(gate) = self.gate.read() else {
            return;
        };
        if self.collecting(&gate) {
            if let Some(id) = provider_id(&rental.to_string()) {
                if let Err(error) = self.journal.confirm(intent, id, None, gate.epoch) {
                    self.note(error);
                }
            }
        }
    }
    pub fn prepare_rental(&self, offer: &OfferCandidate) -> Option<String> {
        let gate = self.gate.read().ok()?;
        if !self.collecting(&gate) {
            return None;
        }
        let hardware = HardwareSnapshot {
            gpu_name: coarse_label(&offer.gpu_name, 128),
            rented_gpu_count: (offer.gpu_count > 0).then_some(offer.gpu_count),
            country: (offer.country.len() == 2
                && offer.country.bytes().all(|b| b.is_ascii_uppercase()))
            .then(|| offer.country.clone()),
            region: coarse_label(&offer.region, 80),
        };
        match self.journal.prepare(
            gate.epoch,
            offer.machine_id.clone(),
            offer.id.to_string(),
            hardware,
        ) {
            Ok(id) => Some(id),
            Err(error) => {
                self.note(error);
                None
            }
        }
    }
    pub async fn rental_confirmed(
        &self,
        context: &AppContext,
        intent: &str,
        rental: u64,
        machine_id: Option<String>,
    ) {
        self.verify_machine(rental, machine_id.as_deref());
        let id = {
            let Ok(gate) = self.gate.read() else {
                return;
            };
            if !self.collecting(&gate) {
                return;
            }
            let Some(rental_id) = provider_id(&rental.to_string()) else {
                return;
            };
            match self
                .journal
                .confirm(intent, rental_id, machine_id, gate.epoch)
            {
                Ok(id) => id,
                Err(error) => {
                    self.note(error);
                    None
                }
            }
        };
        if let Some(id) = id {
            self.fail(
                context
                    .update_state(|s| {
                        // The per-rental journal survives removal of either local source object.
                        let reference = format!("feedback/outbox-v1.json#{id}");
                        if s.instance.instance_id == Some(rental) {
                            s.instance.feedback_session_id = Some(id.clone());
                            s.instance.feedback_journal_ref = Some(reference.clone());
                        }
                        if let Some(r) = s
                            .provisioned_servers
                            .iter_mut()
                            .find(|r| r.instance_id == rental)
                        {
                            r.feedback_session_id = Some(id.clone());
                            r.feedback_journal_ref = Some(reference);
                        }
                    })
                    .await
                    .map(|_| ())
                    .map_err(|_| {
                        "Feedback rental references could not be saved; durable session retained"
                            .into()
                    }),
            );
        }
    }
    pub async fn reconcile_provider(
        &self,
        context: &AppContext,
        instances: &[crate::models::vast::VastInstance],
    ) {
        // Only a persisted accepted-contract ID can recover an unattributed intent.
        // Never match an old rental by GPU, IP, host, offer, or creation-time proximity.
        for intent in self.journal.intents() {
            if let Some(instance) = instances
                .iter()
                .find(|i| intent.provider_instance_id.as_deref() == Some(i.id.to_string().as_str()))
            {
                self.rental_confirmed(
                    context,
                    &intent.session_id,
                    instance.id,
                    instance.machine_id.clone(),
                )
                .await;
            }
        }
        let ids: std::collections::HashSet<_> = instances.iter().map(|i| i.id).collect();
        let Ok(gate) = self.gate.read() else {
            return;
        };
        if !self.collecting(&gate) {
            return;
        }
        if self.journal.records().is_ok_and(|records| {
            records.iter().any(|r| {
                r.snapshot.lifecycle.destroyed_at.is_none()
                    && r.snapshot.lifecycle.observation_complete
                    && r.snapshot
                        .provider_instance_id
                        .parse::<u64>()
                        .is_ok_and(|id| !ids.contains(&id))
            })
        }) {
            self.fail(self.journal.observation_gap(Some(&ids)));
        }
    }
    pub fn rental_reference(&self, rental: u64) -> Option<(String, String)> {
        self.journal.record(&rental.to_string()).map(|r| {
            (
                r.session_id.clone(),
                format!("feedback/outbox-v1.json#{}", r.session_id),
            )
        })
    }
    pub fn milestone(&self, rental: u64, event: Milestone) {
        let Ok(gate) = self.gate.read() else {
            return;
        };
        if self.collecting(&gate)
            && self
                .journal
                .record(&rental.to_string())
                .is_some_and(|r| !r.suspended)
        {
            self.fail(
                self.journal
                    .milestone(&rental.to_string(), gate.epoch, event),
            );
        }
    }
    pub fn external_absence(&self, rental: u64) {
        let Ok(gate) = self.gate.read() else {
            return;
        };
        if self.collecting(&gate)
            && self.journal.record(&rental.to_string()).is_some_and(|r| {
                r.snapshot.lifecycle.observation_complete
                    && r.snapshot.lifecycle.destroyed_at.is_none()
            })
        {
            self.fail(self.journal.mark_unknown(&rental.to_string()));
        }
    }
    pub async fn capture_frames(
        &self,
        context: &AppContext,
        runtime: &crate::moonlight::runtime::MoonlightRuntimeHandle,
    ) {
        let mut evidence = runtime.take_received_frame_evidence();
        if let Some(current) = runtime.subscribe_frame_evidence().borrow().clone() {
            evidence.push(current);
        }
        if evidence.is_empty() {
            return;
        }
        let state = context.load_state().await;
        for (host, _generation, observed_at) in evidence {
            if let Some(server) = state
                .provisioned_servers
                .iter()
                .find(|r| r.embedded_moonlight_host_id == host)
            {
                self.first_frame(server.instance_id, observed_at);
            }
        }
    }
    pub fn first_frame(&self, rental: u64, observed_at: Instant) {
        let Ok(gate) = self.gate.read() else {
            return;
        };
        if !self.collecting(&gate) {
            return;
        }
        let Some(record) = self
            .journal
            .record(&rental.to_string())
            .filter(|r| !r.suspended && r.snapshot.lifecycle.destroyed_at.is_none())
        else {
            return;
        };
        if let Ok(mut stream) = self.stream.lock() {
            *stream = Some((rental.to_string(), observed_at));
        }
        if record.snapshot.lifecycle.first_stream_frame_at.is_none() {
            if record.snapshot.lifecycle.ready_at.is_none()
                || record.snapshot.lifecycle.paired_at.is_none()
            {
                self.fail(self.journal.mark_unknown(&rental.to_string()));
            }
            self.fail(self.journal.milestone(
                &rental.to_string(),
                gate.epoch,
                Milestone::FirstFrame,
            ));
        }
    }
    pub fn stream_stopped(&self) {
        if let Ok(mut stream) = self.stream.lock() {
            *stream = None;
        }
        if let Ok(mut tracker) = self.metric.lock() {
            tracker.reset();
        }
        self.checkpoint();
    }
    pub fn checkpoint(&self) {
        // Suspension may checkpoint consented data, but default-off/withdrawn installs do not write a spool.
        if self.gate.read().map(|g| g.enabled).unwrap_or(false) {
            self.fail(self.journal.checkpoint());
        }
    }
    pub fn review_eligible(&self, rental: &str) -> bool {
        let Ok(gate) = self.gate.read() else {
            return false;
        };
        self.collecting(&gate)
            && self.journal.record(rental).is_some_and(|r| {
                !r.suspended
                    && r.snapshot.consent_epoch == gate.epoch
                    && r.snapshot.lifecycle.review_eligible()
                    && r.snapshot.lifecycle.destroyed_at.is_none()
            })
    }
    pub fn stage_review(
        &self,
        rental: &str,
        score: Option<u8>,
        comment: Option<String>,
    ) -> Result<(), String> {
        let gate = self
            .gate
            .read()
            .map_err(|_| "Feedback consent is unavailable")?;
        if !self.collecting(&gate) {
            return Err("Feedback participation is disabled or paused".into());
        }
        if score.is_some_and(|s| !(1..=10).contains(&s)) {
            return Err("Feedback score must be an integer from 1 to 10".into());
        }
        let comment = comment
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if comment.as_ref().is_some_and(|s| {
            s.encode_utf16().count() > 2000
                || s.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
        }) {
            return Err("Feedback comments must be plain text, at most 2000 characters".into());
        }
        // Null/null is a local cancellation action, never an empty review PUT.
        self.journal.stage(rental, gate.epoch, score, comment)
    }
    pub fn measurement_allowed(&self, rental: u64) -> bool {
        let Ok(gate) = self.gate.read() else {
            return false;
        };
        self.collecting(&gate)
            && self.journal.record(&rental.to_string()).is_some_and(|r| {
                !r.suspended
                    && r.snapshot.consent_epoch == gate.epoch
                    && r.snapshot.lifecycle.destroyed_at.is_none()
            })
            && self
                .stream
                .lock()
                .ok()
                .and_then(|s| s.clone())
                .is_some_and(|(id, at)| {
                    id == rental.to_string() && at.elapsed() < Duration::from_secs(3)
                })
    }
    pub fn jitter(
        &self,
        rental: u64,
        monitor: &str,
        evidence: &noland_network_contracts::feedback::MeasurementEvidence,
        measured_at: Instant,
        value: f64,
        route: &str,
    ) {
        let Ok(gate) = self.gate.read() else {
            return;
        };
        if !self.collecting(&gate)
            || !self
                .stream
                .lock()
                .ok()
                .and_then(|s| s.clone())
                .is_some_and(|(id, at)| {
                    id == rental.to_string() && at.elapsed() < Duration::from_secs(3)
                })
        {
            return;
        }
        let Some((boundary, observation)) = self.metric.lock().ok().map(|mut t| {
            let boundary = t.boundary(monitor, route);
            (
                boundary,
                t.observe(monitor, evidence, measured_at, value, route, Instant::now()),
            )
        }) else {
            return;
        };
        if boundary {
            self.fail(self.journal.checkpoint());
        }
        let Some((route, duration)) = observation else {
            return;
        };
        self.fail(self.journal.change(false, |s| {
            if let Some(r) = s.sessions.get_mut(&rental.to_string()).filter(|r| {
                r.snapshot.consent_epoch == gate.epoch
                    && r.snapshot.lifecycle.destroyed_at.is_none()
            }) {
                metric::accumulate(&mut r.snapshot.jitter_buckets, &route, value, duration);
                r.snapshot.revision += 1;
                r.updated_at = now();
            }
            Ok(())
        }));
    }
    pub fn suspend(&self) {
        self.background.store(true, Ordering::SeqCst);
        self.stream_stopped();
        let active = self.gate.read().map(|g| g.enabled).unwrap_or(false);
        if active {
            self.fail(self.journal.observation_gap(None));
            self.checkpoint();
        }
    }
    pub fn foreground(&self) {
        self.background.store(false, Ordering::SeqCst);
    }
    pub fn verify_machine(&self, rental: u64, machine: Option<&str>) {
        let Some(machine) = machine else {
            return;
        };
        let Ok(gate) = self.gate.read() else {
            return;
        };
        if !self.collecting(&gate)
            || !self
                .journal
                .record(&rental.to_string())
                .is_some_and(|r| !r.suspended && r.snapshot.provider_machine_id != machine)
        {
            return;
        }
        self.fail(self.journal.change(true, |s| {
            if let Some(r) = s.sessions.get_mut(&rental.to_string()) {
                if r.snapshot.provider_machine_id != machine {
                    r.suspended = true;
                    r.snapshot.lifecycle.observation_complete = false;
                    self.note("Provider machine identity conflicted with the selected offer; this rental is excluded from delivery");
                }
            }
            Ok(())
        }));
    }
}

fn coarse_label(value: &str, max: usize) -> Option<String> {
    let mut text = String::new();
    let mut units = 0;
    for c in value.trim().chars().filter(|c| !c.is_control()) {
        if units + c.len_utf16() > max {
            break;
        }
        text.push(c);
        units += c.len_utf16();
    }
    (!text.is_empty()).then_some(text)
}

static IOS_CONTEXT: OnceLock<AppContext> = OnceLock::new();
#[cfg(target_os = "ios")]
static IOS_RUNTIME: OnceLock<crate::moonlight::runtime::MoonlightRuntimeHandle> = OnceLock::new();
#[cfg(target_os = "ios")]
extern "C" fn ios_lifecycle(active: bool) {
    let Some(context) = IOS_CONTEXT.get() else {
        return;
    };
    if active {
        context.feedback.foreground();
        if let Ok(state) = context.state.try_read() {
            if state.feedback_program.enabled {
                context.feedback.reconcile_state(&state.feedback_program);
            }
        }
        FeedbackProgramService::kick(context.clone());
    } else {
        // Capture any coalesced first frame before suspension. Never block UIKit on an async state lock.
        if let (Some(runtime), Ok(state)) = (IOS_RUNTIME.get(), context.state.try_read()) {
            let mut frames = runtime.take_received_frame_evidence();
            if let Some(current) = runtime.subscribe_frame_evidence().borrow().clone() {
                frames.push(current);
            }
            for (host, _generation, at) in frames {
                if let Some(server) = state
                    .provisioned_servers
                    .iter()
                    .find(|r| r.embedded_moonlight_host_id == host)
                {
                    context.feedback.first_frame(server.instance_id, at);
                }
            }
        }
        context.feedback.suspend();
    } // Synchronous disk checkpoint before returning to UIKit; never waits for HTTP.
}
pub fn start(context: AppContext, moonlight: &crate::moonlight::composition::MoonlightManager) {
    #[cfg(target_os = "ios")]
    {
        unsafe extern "C" {
            fn noland_ios_feedback_lifecycle_install(callback: extern "C" fn(bool));
        }
        let _ = IOS_CONTEXT.set(context.clone());
        let _ = IOS_RUNTIME.set(moonlight.runtime.clone());
        unsafe {
            noland_ios_feedback_lifecycle_install(ios_lifecycle);
        }
    }
    let mut frames = moonlight.runtime.subscribe_frame_evidence();
    let frame_runtime = moonlight.runtime.clone();
    let frame_context = context.clone();
    tauri::async_runtime::spawn(async move {
        while frames.changed().await.is_ok() {
            frame_context
                .feedback
                .capture_frames(&frame_context, &frame_runtime)
                .await;
            if frames.borrow().is_none() {
                frame_context.feedback.stream_stopped();
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut upload_tick = 0_u8;
        loop {
            tick.tick().await;
            if !context.feedback.background.load(Ordering::SeqCst) {
                context.feedback.checkpoint();
            }
            if upload_tick % 2 == 0 {
                context.feedback.replay(&context).await;
            }
            upload_tick = upload_tick.wrapping_add(1);
        }
    });
}
