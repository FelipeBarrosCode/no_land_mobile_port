use super::*;

impl FeedbackProgramService {
    pub(super) async fn deliver_control(
        &self,
        context: &AppContext,
        state: &FeedbackProgramState,
        control: &PendingControl,
    ) -> Result<(), ApiError> {
        let mut protected = secure::read().map_err(ApiError::local)?.ok_or_else(|| {
            ApiError::local("Feedback control credential is unavailable; request retained")
        })?;
        if Some(protected.installation_id.as_str()) != state.installation_id.as_deref()
            || self.base_url.as_deref() != Some(protected.api_origin.as_str())
        {
            return Err(ApiError::local(
                "Feedback controls are bound to the original installation/API endpoint",
            ));
        }
        if protected.receipt.is_some() {
            return self
                .finish_deletion_ack(context, &mut protected)
                .await
                .map_err(ApiError::local);
        }
        let mut control = control.clone();
        if protected.credential.is_none() {
            // A lost enrollment response can be recovered to authenticate a subsequent withdrawal/deletion,
            // without ever turning local collection on or creating another contributor.
            let retry = protected.enrollment.clone().ok_or_else(|| {
                ApiError::local("Enrollment recovery proof is unavailable; control remains pending")
            })?;
            let created = chrono::DateTime::parse_from_rfc3339(&retry.created_at)
                .map_err(|_| ApiError::local("Enrollment recovery time is invalid"))?;
            if chrono::Utc::now()
                .signed_duration_since(created)
                .num_hours()
                >= 24
            {
                return Err(ApiError::local("Enrollment recovery expired; contact the configured operator. Deletion intent and proof remain protected."));
            }
            let original: EnrollmentRequest = serde_json::from_value(retry.request.clone())
                .map_err(|_| ApiError::local("Protected enrollment request is unsupported"))?;
            validate_enrollment(&original).map_err(ApiError::local)?;
            let mut body = retry.request;
            body["installationId"] = json!(protected.installation_id);
            body["requestId"] = json!(retry.request_id);
            body["recoveryProof"] = json!(retry.recovery_proof);
            let response = self
                .request(
                    Method::POST,
                    "/enrollments",
                    Some(body),
                    None,
                    Some(&retry.request_id),
                )
                .await?;
            protected.credential = response["credential"]
                .as_str()
                .filter(|s| s.len() >= 32)
                .map(str::to_string);
            protected.deletion_code = response["deletionCode"]
                .as_str()
                .filter(|s| s.len() >= 32)
                .map(str::to_string);
            let epoch = response["consentEpoch"]
                .as_u64()
                .filter(|e| *e > 0)
                .ok_or_else(|| ApiError::local("Recovered enrollment epoch is invalid"))?;
            if protected.credential.is_none() || protected.deletion_code.is_none() {
                return Err(ApiError::local(
                    "Recovered enrollment credentials are invalid",
                ));
            }
            let user_id = response["userId"]
                .as_str()
                .filter(|s| valid_uuid(s))
                .ok_or_else(|| ApiError::local("Recovered enrollment identity is invalid"))?;
            let consented_at = response["consentedAt"]
                .as_str()
                .filter(|s| valid_time(s))
                .ok_or_else(|| ApiError::local("Recovered consent time is invalid"))?;
            if response["privacyVersion"].as_str() != Some(original.privacy_version.as_str())
                || response["termsVersion"].as_str() != Some(original.terms_version.as_str())
            {
                return Err(ApiError::local(
                    "Recovered enrollment policy versions do not match the proof-bound request",
                ));
            }
            protected.enrollment_ack = Some(secure::EnrollmentAck {
                user_id: user_id.into(),
                consent_epoch: epoch,
                privacy_version: original.privacy_version,
                terms_version: original.terms_version,
                consented_at: consented_at.into(),
            });
            secure::write(&protected).map_err(ApiError::local)?;
        }
        // Recover a crash between the protected credential write and the normal atomic state acknowledgment.
        if control.expected_epoch == 0 {
            let ack = protected.enrollment_ack.as_ref().ok_or_else(|| {
                ApiError::local("Protected enrollment acknowledgment is unavailable")
            })?;
            context
                .update_state(|s| {
                    let p = &mut s.feedback_program;
                    p.user_id = Some(ack.user_id.clone());
                    p.consent_epoch = ack.consent_epoch;
                    p.privacy_version = Some(ack.privacy_version.clone());
                    p.terms_version = Some(ack.terms_version.clone());
                    p.consented_at = Some(ack.consented_at.clone());
                    if let Some(current) = p
                        .pending_control_action
                        .as_mut()
                        .filter(|p| p.request_id == control.request_id)
                    {
                        current.expected_epoch = ack.consent_epoch;
                    }
                })
                .await
                .map_err(|_| {
                    ApiError::local("Recovered enrollment control could not be checkpointed")
                })?;
            control.expected_epoch = ack.consent_epoch;
        }
        if protected.enrollment.is_some() {
            protected.enrollment = None; // Original optional profile/proof are no longer needed once credentials and epoch are durable.
            secure::write(&protected).map_err(ApiError::local)?;
        }
        let credential = protected
            .credential
            .as_deref()
            .ok_or_else(|| ApiError::local("Feedback control credential is unavailable"))?;
        match control.kind {
            ControlKind::Consent => {
                let response = self.request(Method::PUT, "/me/consent", Some(json!({
                    "requestId": control.request_id, "expectedEpoch": control.expected_epoch,
                    "enabled": control.enabled, "optionalContextEnabled": control.optional_context_enabled,
                })), Some(credential), Some(&control.request_id)).await?;
                self.apply_consent_ack(context, &control, response)
                    .await
                    .map_err(ApiError::local)
            }
            ControlKind::Delete => {
                let response = match self
                    .request(
                        Method::DELETE,
                        "/me",
                        None,
                        Some(credential),
                        Some(&control.request_id),
                    )
                    .await
                {
                    Ok(response) => response,
                    Err(error)
                        if matches!(
                            error.status,
                            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
                        ) =>
                    {
                        let code = protected.deletion_code.as_ref().ok_or_else(|| {
                            ApiError::local(
                                "Deletion recovery code is unavailable; original control retained",
                            )
                        })?;
                        self.request(
                            Method::POST,
                            "/deletions",
                            Some(json!({"deletionCode": code, "requestId": control.request_id})),
                            None,
                            Some(&control.request_id),
                        )
                        .await?
                    }
                    Err(error) => return Err(error),
                };
                self.apply_deletion_ack(context, protected, response)
                    .await
                    .map_err(ApiError::local)
            }
        }
    }
    pub(super) async fn poll_deletion_receipt(&self) {
        let Ok(Some(mut protected)) = secure::read() else {
            return;
        };
        if self.base_url.as_deref() != Some(protected.api_origin.as_str()) {
            return;
        }
        let Some(receipt) = protected.receipt.as_ref() else {
            return;
        };
        if matches!(
            receipt.status.as_str(),
            "completed" | "complete" | "deleted"
        ) {
            return;
        }
        match self
            .request(
                Method::GET,
                &format!("/deletions/{}", receipt.receipt_id),
                None,
                Some(&receipt.status_token),
                Some(&receipt.receipt_id),
            )
            .await
        {
            Ok(response) => {
                if let Some(status) = response["status"].as_str().filter(|s| {
                    matches!(
                        *s,
                        "pending"
                            | "accepted"
                            | "completed"
                            | "complete"
                            | "processing"
                            | "deleted"
                    )
                }) {
                    if let Some(receipt) = &mut protected.receipt {
                        receipt.status = status.into();
                    }
                    if let Err(error) = secure::write(&protected) {
                        self.note(error);
                    }
                }
            }
            Err(error) => self.retry_error(&error),
        }
    }
}
