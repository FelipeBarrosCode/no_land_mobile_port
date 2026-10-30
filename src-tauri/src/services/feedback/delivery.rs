use super::*;

impl FeedbackProgramService {
    pub(super) async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        bearer: Option<&str>,
        control_id: Option<&str>,
    ) -> Result<Value, ApiError> {
        if let Ok(retry) = self.retry.lock() {
            if retry.halted {
                return Err(ApiError::local("Feedback retry deadline is outside the supported clock range; delivery is paused"));
            }
            if retry.next > Instant::now() {
                return Err(ApiError {
                    status: None,
                    retryable: true,
                    retry_after: Some(retry.next.saturating_duration_since(Instant::now())),
                    message: "Feedback retry is waiting for its backoff/Retry-After window".into(),
                });
            }
        }
        let base = self
            .base_url
            .as_ref()
            .ok_or_else(|| ApiError::local("Feedback API URL is not configured"))?;
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| ApiError::local("Feedback HTTP adapter is unavailable"))?;
        let mut request = client.request(method.clone(), format!("{base}/v1{path}"));
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        if let Some(id) = control_id {
            request = request
                .header("Idempotency-Key", id)
                .header("X-Request-Id", id);
        }
        if let Some(body) = body {
            if method == Method::DELETE && path.ends_with("/review") {
                request = request.header("X-Review-Revision", body["revision"].to_string());
            } else {
                request = request.json(&body);
            }
        }
        // Last native gate immediately before delivery, not only at queue insertion/UI time.
        if bearer.is_some() && control_id.is_none() {
            let allowed = !self.delivery_pending.load(Ordering::SeqCst)
                && self
                    .gate
                    .read()
                    .map(|g| self.collecting(&g))
                    .unwrap_or(false);
            if !allowed {
                return Err(ApiError::local(
                    "Feedback participation is disabled or paused",
                ));
            }
        }
        let mut response = request.send().await.map_err(|_| ApiError {
            status: None,
            retryable: true,
            retry_after: None,
            message: "Feedback service is unreachable; the durable outbox will retry while active"
                .into(),
        })?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| {
                h.parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
                    chrono::DateTime::parse_from_rfc2822(h)
                        .ok()
                        .and_then(|date| {
                            date.signed_duration_since(chrono::Utc::now()).to_std().ok()
                        })
                })
            });
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ApiError {
            status: Some(status),
            retryable: true,
            retry_after,
            message: "Feedback response was interrupted; retry remains safe".into(),
        })? {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err(ApiError::local(
                    "Feedback response exceeds the native size bound",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            let code = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|v| v["code"].as_str().map(str::to_string))
                .filter(|s| {
                    s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
                .unwrap_or_else(|| "request_rejected".into());
            return Err(ApiError {
                status: Some(status),
                retryable: status == StatusCode::TOO_MANY_REQUESTS
                    || status.is_server_error()
                    || status == StatusCode::REQUEST_TIMEOUT,
                retry_after,
                message: format!("Feedback service: {code} (HTTP {})", status.as_u16()),
            });
        }
        if bytes.is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::local("Feedback response is not valid JSON"))
    }
    fn retry_error(&self, error: &ApiError) {
        self.note(error.message.clone());
        if error.retryable {
            if let Ok(mut retry) = self.retry.lock() {
                retry.failures = (retry.failures + 1).min(10);
                let exponential = Duration::from_secs((1_u64 << retry.failures).min(3600));
                // Never shorten Retry-After. Add a little random staggering without touching metric cadence.
                let next = error
                    .retry_after
                    .unwrap_or(exponential)
                    .checked_add(Duration::from_millis(rand::random::<u16>() as u64 % 1000))
                    .and_then(|delay| Instant::now().checked_add(delay));
                if let Some(next) = next {
                    retry.next = next;
                } else {
                    retry.halted = true;
                    self.note("Feedback Retry-After exceeds the supported clock range; delivery remains paused instead of retrying early");
                }
            }
        } else if matches!(
            error.status,
            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        ) {
            self.stop_local();
            self.persist_stop();
            self.credential_available.store(false, Ordering::SeqCst);
        }
    }
    fn retry_success(&self) {
        if let Ok(mut retry) = self.retry.lock() {
            retry.failures = 0;
            // Another request may have received 429 while this successful request was already in flight.
            if retry.next <= Instant::now() {
                retry.next = Instant::now();
            }
        }
    }
    async fn token(&self, context: &AppContext) -> Result<(String, u64), String> {
        let s = context.load_state().await.feedback_program;
        if !s.enabled
            || !s.structurally_valid()
            || s.pending_control_action.is_some()
            || !self
                .gate
                .read()
                .map(|g| self.collecting(&g) && g.epoch == s.consent_epoch)
                .unwrap_or(false)
        {
            return Err(
                "Feedback participation is disabled, paused, or synchronizing consent".into(),
            );
        }
        let protected = secure::read()?.ok_or("Protected feedback credential is unavailable")?;
        if protected.stop_requested || !credentials_match(&s, &protected, self.base_url.as_deref())
        {
            return Err(
                "Feedback credential is unavailable or bound to a different installation/endpoint"
                    .into(),
            );
        }
        Ok((
            protected
                .credential
                .ok_or("Feedback credential is unavailable")?,
            s.consent_epoch,
        ))
    }
    pub async fn participant_request(
        &self,
        context: &AppContext,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let (token, _) = self.token(context).await?;
        match self.request(method, path, body, Some(&token), None).await {
            Ok(value) => {
                self.retry_success();
                Ok(value)
            }
            Err(error) => {
                self.retry_error(&error);
                Err(error.message)
            }
        }
    }
    pub fn kick(context: AppContext) {
        tauri::async_runtime::spawn(async move {
            context.feedback.replay(&context).await;
        });
    }
    pub async fn replay(&self, context: &AppContext) {
        let Ok(_guard) = self.request_lock.try_lock() else {
            return;
        };
        if self.background.load(Ordering::SeqCst) {
            return;
        }
        if self
            .retry
            .lock()
            .map(|r| r.halted || r.next > Instant::now())
            .unwrap_or(true)
        {
            return;
        }
        let state = context.load_state().await.feedback_program;
        if state.pending_control_action.is_some() {
            let _controls = self.control_lock.lock().await;
            let state = context.load_state().await.feedback_program;
            let Some(control) = state.pending_control_action.clone() else {
                return;
            };
            if control.delivery_suspended {
                return;
            }
            match self.deliver_control(context, &state, &control).await {
                Ok(()) => self.retry_success(),
                Err(error) => {
                    self.retry_error(&error);
                    if !error.retryable && error.status.is_some() {
                        let _ = context
                            .update_state(|s| {
                                if let Some(p) = s
                                    .feedback_program
                                    .pending_control_action
                                    .as_mut()
                                    .filter(|p| p.request_id == control.request_id)
                                {
                                    p.delivery_suspended = true;
                                }
                            })
                            .await;
                    }
                }
            }
            return;
        }
        if state.enrollment_status == EnrollmentStatus::Deleted {
            self.poll_deletion_receipt().await;
            return;
        }
        if !state.enabled {
            return;
        } // Never fetch /program or do enrichment while disabled.
        if !self.credential_available.load(Ordering::SeqCst)
            || self.local_stop.load(Ordering::SeqCst)
        {
            return;
        }
        // Explicit cached collection pause/policy mismatch, not an inference from an outage.
        match self.program_notices().await {
            Ok(notices) => {
                if notices["privacyVersion"].as_str() != state.privacy_version.as_deref()
                    || notices["termsVersion"].as_str() != state.terms_version.as_deref()
                {
                    self.stop_local();
                    self.note("Feedback policy versions changed; participation is paused pending renewed acceptance");
                    return;
                }
            }
            Err(error) => {
                self.note(error);
                return;
            }
        }
        let Ok((token, epoch)) = self.token(context).await else {
            return;
        };
        if let Err(error) = self.journal.prune().and_then(|_| self.journal.checkpoint()) {
            self.note(error);
            return;
        }
        let Ok(records) = self.journal.records() else {
            return;
        };
        // Bounded work per active tick/foreground; never monopolize mobile background time.
        for record in records
            .into_iter()
            .filter(|r| {
                !r.suspended
                    && (r.snapshot.revision > r.acknowledged_revision
                        || r.review.as_ref().is_some_and(|v| {
                            v.revision > r.acknowledged_review_revision
                                && (r.snapshot.lifecycle.destroyed_at.is_some()
                                    || (v.score.is_none() && v.comment.is_none()))
                        }))
            })
            .take(10)
        {
            let s = &record.snapshot;
            if s.consent_epoch != epoch || self.token(context).await.is_err() {
                return;
            }
            if record.acknowledged_revision < s.revision {
                let Ok(body) = serde_json::to_value(s) else {
                    continue;
                };
                match self
                    .request(
                        Method::PUT,
                        &format!("/sessions/{}", record.session_id),
                        Some(body),
                        Some(&token),
                        None,
                    )
                    .await
                {
                    Ok(response) if response["revision"].as_u64() == Some(s.revision) => {
                        if self.token(context).await.is_err() {
                            return;
                        }
                        if let Err(error) =
                            self.journal
                                .acknowledge(&s.provider_instance_id, s.revision, false)
                        {
                            self.note(error);
                            return;
                        }
                    }
                    Ok(_) => {
                        self.note("Feedback snapshot acknowledgment has an unexpected revision");
                        return;
                    }
                    Err(error) => {
                        if self.token(context).await.is_err()
                            || context.load_state().await.feedback_program.consent_epoch != epoch
                        {
                            return;
                        }
                        self.suspend_record_on_permanent_error(&s.provider_instance_id, &error);
                        self.retry_error(&error);
                        return;
                    }
                }
            }
            let review = match self.journal.claim_review(&s.provider_instance_id) {
                Ok(review) => review,
                Err(error) => {
                    self.note(error);
                    return;
                }
            };
            if let Some(review) = review {
                let removed = review.score.is_none() && review.comment.is_none();
                if self.token(context).await.is_err() {
                    return;
                }
                let Ok(body) = serde_json::to_value(&review) else {
                    continue;
                };
                match self
                    .request(
                        if removed { Method::DELETE } else { Method::PUT },
                        &format!("/sessions/{}/review", record.session_id),
                        Some(body),
                        Some(&token),
                        None,
                    )
                    .await
                {
                    Ok(response)
                        if response["reviewRevision"]
                            .as_u64()
                            .or_else(|| response["revision"].as_u64())
                            == Some(review.revision) =>
                    {
                        if self.token(context).await.is_err() {
                            return;
                        }
                        self.fail(self.journal.acknowledge(
                            &s.provider_instance_id,
                            review.revision,
                            true,
                        ));
                    }
                    Ok(_) => {
                        self.note("Feedback review acknowledgment has an unexpected revision");
                        return;
                    }
                    Err(error) => {
                        if self.token(context).await.is_err()
                            || context.load_state().await.feedback_program.consent_epoch != epoch
                        {
                            return;
                        }
                        self.suspend_record_on_permanent_error(&s.provider_instance_id, &error);
                        self.retry_error(&error);
                        return;
                    }
                }
            }
        }
        self.retry_success();
    }
    fn suspend_record_on_permanent_error(&self, id: &str, error: &ApiError) {
        if !error.retryable && error.status.is_some() {
            self.fail(self.journal.change(true, |s| {
                if let Some(r) = s.sessions.get_mut(id) {
                    r.suspended = true;
                }
                Ok(())
            }));
        }
    }
}
