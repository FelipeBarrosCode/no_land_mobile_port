use super::*;

impl FeedbackProgramService {
    pub async fn consent(
        &self,
        context: &AppContext,
        enabled: bool,
        optional_context_enabled: bool,
    ) -> Result<ProgramStatus, String> {
        // Local withdrawal takes effect before waiting for any outstanding command/network work.
        let generation = self.stop_generation.load(Ordering::SeqCst);
        if !enabled {
            self.stop_local();
            self.persist_stop();
        }
        let _guard = self.control_lock.lock().await;
        let state = context.load_state().await.feedback_program;
        if state.installation_id.is_none() || state.enrollment_status == EnrollmentStatus::Deleted {
            return Err("Enroll explicitly before changing participation".into());
        }
        if enabled && state.pending_control_action.is_some() {
            return Err("Pending controls must synchronize before rejoining".into());
        }
        if enabled && state.enrollment_status == EnrollmentStatus::Enrolling {
            return Err("Retry the original enrollment first".into());
        }
        if enabled
            && !credentials_match(
                &state,
                &secure::read()?.ok_or("Protected feedback credentials are unavailable")?,
                self.base_url.as_deref(),
            )
        {
            return Err(
                "Protected feedback credentials are unavailable or bound to another API endpoint"
                    .into(),
            );
        }
        if enabled
            && secure::read()?.and_then(|p| p.policy).is_some_and(|p| {
                Some(p.privacy_version.as_str()) != state.privacy_version.as_deref()
                    || Some(p.terms_version.as_str()) != state.terms_version.as_deref()
            })
        {
            return Err("Published policy versions changed; old consent cannot be silently resumed. Deletion remains available.".into());
        }
        if !enabled {
            self.fail(self.journal.clear());
        }
        if enabled && self.stop_generation.load(Ordering::SeqCst) != generation {
            return Err("A newer withdrawal superseded this participation change".into());
        }
        if enabled {
            self.local_stop.store(false, Ordering::SeqCst);
        }
        self.delivery_pending.store(true, Ordering::SeqCst);
        let saved = context.update_state(|s| {
            let p = &mut s.feedback_program;
            if !enabled { p.enabled = false; }
            if !enabled || !optional_context_enabled { p.optional_context_enabled = false; }
            if let Some(pending) = &mut p.pending_control_action {
                if pending.kind != ControlKind::Delete && pending.enabled { pending.after_ack = Some(ControlKind::Consent); }
                pending.delivery_suspended = false; // Explicit user retry retains the exact original control identity.
            } else {
                p.pending_control_action = Some(PendingControl { kind: ControlKind::Consent,
                    request_id: uuid::Uuid::new_v4().to_string(), expected_epoch: p.consent_epoch,
                    enabled, optional_context_enabled: enabled && optional_context_enabled, after_ack: None, delivery_suspended: false });
                // Scope-only changes keep basic local collection, but delivery waits for the acknowledged epoch.
                if !p.enabled { p.enrollment_status = EnrollmentStatus::Withdrawn; }
            }
        }).await.map_err(|_| "Participation stopped locally, but its control request could not be persisted; retry before closing the app")?;
        if !enabled {
            self.note("Local collection stopped. Withdrawal synchronization is pending; previously accepted history remains until retention/deletion.");
        } else if !saved.feedback_program.enabled {
            self.note("Rejoining awaits acknowledgment; existing rentals will not be joined.");
        } else {
            self.note("Scope synchronization is pending; contribution delivery is paused.");
        }
        Self::kick(context.clone());
        Ok(self.status(context).await)
    }
    pub async fn delete_contributions(
        &self,
        context: &AppContext,
    ) -> Result<ProgramStatus, String> {
        self.stop_local();
        self.persist_stop();
        self.delivery_pending.store(true, Ordering::SeqCst);
        let _guard = self.control_lock.lock().await;
        let state = context.load_state().await.feedback_program;
        if state.installation_id.is_none() {
            return Err("There is no local feedback enrollment to delete".into());
        }
        self.fail(self.journal.clear());
        context.update_state(|s| {
            let p = &mut s.feedback_program;
            p.enabled = false; p.optional_context_enabled = false; p.enrollment_status = EnrollmentStatus::DeletionPending;
            if p.pending_control_action.as_ref().is_some_and(|c| c.delivery_suspended && c.kind != ControlKind::Delete) {
                p.pending_control_action = None; // A permanently rejected consent must not strand an authenticated deletion.
            }
            if let Some(pending) = &mut p.pending_control_action {
                if pending.kind != ControlKind::Delete { pending.after_ack = Some(ControlKind::Delete); }
                else { pending.delivery_suspended = false; } // Explicit retry, not an automatic loop.
            } else {
                p.pending_control_action = Some(PendingControl { kind: ControlKind::Delete,
                    request_id: uuid::Uuid::new_v4().to_string(), expected_epoch: p.consent_epoch,
                    enabled: false, optional_context_enabled: false, after_ack: None, delivery_suspended: false });
            }
        }).await.map_err(|_| "Feedback stopped locally, but deletion could not be persisted; retry before closing the app")?;
        // Authentication and deletion recovery code remain in Keychain until server acknowledgment is durable.
        self.note("Collection stopped locally. Deletion synchronization is pending; keep this installation until acknowledged.");
        Self::kick(context.clone());
        Ok(self.status(context).await)
    }
    pub(super) async fn apply_consent_ack(
        &self,
        context: &AppContext,
        control: &PendingControl,
        response: Value,
    ) -> Result<(), String> {
        let epoch = response["consentEpoch"]
            .as_u64()
            .filter(|e| *e > control.expected_epoch)
            .ok_or("Invalid consent epoch acknowledgment")?;
        if response["enabled"].as_bool() != Some(control.enabled)
            || response["optionalContextEnabled"].as_bool()
                != Some(control.optional_context_enabled)
        {
            return Err("Consent acknowledgment does not match the durable request".into());
        }
        if control.enabled && !self.local_stop.load(Ordering::SeqCst) {
            if !context.load_state().await.feedback_program.enabled {
                self.journal.clear()?;
            }
            self.journal.epoch(epoch)?;
        }
        // A newer immediate stop must never be undone by an older in-flight acknowledgment.
        let mut protected = secure::read()?.ok_or("Protected consent credential is unavailable")?;
        protected.stop_requested = !control.enabled || self.local_stop.load(Ordering::SeqCst);
        if !control.optional_context_enabled {
            protected.enrollment = None;
        }
        secure::write(&protected)?;
        let saved = context
            .update_state(|s| {
                let p = &mut s.feedback_program;
                if p.pending_control_action
                    .as_ref()
                    .is_none_or(|current| current.request_id != control.request_id)
                {
                    return;
                }
                let after = p
                    .pending_control_action
                    .as_ref()
                    .and_then(|current| current.after_ack.clone());
                p.consent_epoch = epoch;
                p.enabled = control.enabled && after.is_none();
                p.optional_context_enabled = p.enabled && control.optional_context_enabled;
                p.enrollment_status = if p.enabled {
                    EnrollmentStatus::Enrolled
                } else if after == Some(ControlKind::Delete) {
                    EnrollmentStatus::DeletionPending
                } else {
                    EnrollmentStatus::Withdrawn
                };
                p.pending_control_action = after.map(|kind| PendingControl {
                    kind,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_epoch: epoch,
                    enabled: false,
                    optional_context_enabled: false,
                    after_ack: None,
                    delivery_suspended: false,
                });
            })
            .await
            .map_err(|_| {
                "Consent acknowledgment could not be persisted; safe retry remains queued"
            })?;
        self.delivery_pending.store(
            saved.feedback_program.pending_control_action.is_some(),
            Ordering::SeqCst,
        );
        self.restore_gate(&saved.feedback_program);
        if let Ok(mut m) = self.message.lock() {
            *m = None;
        }
        Ok(())
    }
    pub(super) async fn apply_deletion_ack(
        &self,
        context: &AppContext,
        mut protected: secure::Protected,
        response: Value,
    ) -> Result<(), String> {
        let receipt_id = response["receiptId"]
            .as_str()
            .filter(|s| valid_uuid(s))
            .ok_or("Deletion receipt identity is invalid")?
            .to_string();
        let token = response["statusToken"]
            .as_str()
            .filter(|s| s.len() >= 32 && s.len() <= 4096)
            .ok_or("Deletion receipt capability is invalid")?
            .to_string();
        let status = response["status"]
            .as_str()
            .filter(|s| {
                matches!(
                    *s,
                    "pending" | "accepted" | "completed" | "complete" | "processing" | "deleted"
                )
            })
            .ok_or("Deletion receipt status is invalid")?
            .to_string();
        protected.receipt = Some(secure::DeletionReceipt {
            receipt_id,
            status_token: token,
            status,
        });
        // Receipt is protected first. A crash here reuses this acknowledgment, not a revoked credential.
        secure::write(&protected)?;
        self.finish_deletion_ack(context, &mut protected).await
    }
    pub(super) async fn finish_deletion_ack(
        &self,
        context: &AppContext,
        protected: &mut secure::Protected,
    ) -> Result<(), String> {
        context
            .update_state(|s| {
                let p = &mut s.feedback_program;
                p.enabled = false;
                p.optional_context_enabled = false;
                p.pending_control_action = None;
                p.enrollment_status = EnrollmentStatus::Deleted;
                p.credential_ref = None;
                p.deletion_code_ref = None;
            })
            .await
            .map_err(|_| {
                "Deletion acknowledgment could not be persisted; protected receipt retained"
            })?;
        self.journal.clear()?;
        protected.credential = None;
        protected.deletion_code = None;
        protected.stop_requested = true;
        self.delivery_pending.store(false, Ordering::SeqCst);
        protected.enrollment = None;
        protected.enrollment_ack = None;
        secure::write(protected)?;
        self.credential_available.store(false, Ordering::SeqCst);
        self.note("Deletion acknowledged. Public exclusion and server-side purge follow the configured service policy.");
        Ok(())
    }
}
