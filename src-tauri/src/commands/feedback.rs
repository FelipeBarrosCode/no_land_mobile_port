//! The cross-client feedback command seam. Provider operations never await these requests.
use reqwest::Method;
use serde_json::{json, Value};
use tauri::State;

use crate::{
    models::feedback::{provider_id, valid_uuid},
    services::{
        app_context::AppContext,
        feedback::{EnrollmentRequest, ProgramStatus},
    },
};

#[tauri::command]
pub async fn feedback_program_status(
    context: State<'_, AppContext>,
) -> Result<ProgramStatus, String> {
    Ok(context.feedback.status(context.inner()).await)
}
#[tauri::command]
pub async fn feedback_program_notices(context: State<'_, AppContext>) -> Result<Value, String> {
    context.feedback.program_notices().await
}
#[tauri::command]
pub async fn feedback_program_enroll(
    context: State<'_, AppContext>,
    request: EnrollmentRequest,
) -> Result<ProgramStatus, String> {
    context.feedback.enroll(context.inner(), request).await
}
#[tauri::command]
pub async fn feedback_program_consent(
    context: State<'_, AppContext>,
    enabled: bool,
    optional_context_enabled: bool,
) -> Result<ProgramStatus, String> {
    context
        .feedback
        .consent(context.inner(), enabled, optional_context_enabled)
        .await
}
#[tauri::command]
pub async fn feedback_program_delete(
    context: State<'_, AppContext>,
) -> Result<ProgramStatus, String> {
    context.feedback.delete_contributions(context.inner()).await
}
#[tauri::command]
pub async fn feedback_machine_summaries(
    context: State<'_, AppContext>,
    machine_ids: Vec<String>,
) -> Result<Value, String> {
    if machine_ids.is_empty() || machine_ids.len() > 100 {
        return Err("Request between 1 and 100 machine IDs".into());
    }
    let mut ids = Vec::new();
    for id in machine_ids {
        let id = provider_id(&id).ok_or("Machine IDs must be positive decimal bigint strings")?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let value = context
        .feedback
        .participant_request(
            context.inner(),
            Method::POST,
            "/machines/summaries",
            Some(json!({"provider": "vast", "machineIds": ids})),
        )
        .await?;
    // Decode to allowlisted DTOs rather than forwarding arbitrary service fields into the webview.
    let mut result: crate::models::feedback::SummariesResponse =
        serde_json::from_value(value).map_err(|_| "Unsupported feedback summary response")?;
    if result.summaries.len() != ids.len()
        || ids.iter().any(|id| {
            result
                .summaries
                .iter()
                .filter(|s| &s.provider_machine_id == id)
                .count()
                != 1
        })
    {
        return Err("Feedback summaries do not match the requested machines".into());
    }
    for summary in &mut result.summaries {
        let r = &mut summary.reliability;
        if r.source != "no_land"
            || r.basis != "ready_and_paired_before_destroy"
            || r.scope != "retained_history"
            || r.successful_sessions.checked_add(r.failed_sessions) != Some(r.eligible_sessions)
        {
            return Err("Unsupported or inconsistent feedback reliability definition".into());
        }
        if r.eligible_sessions < 2 {
            r.ratio = None;
        }
        if r.ratio.is_some_and(|ratio| {
            !(0.0..=1.0).contains(&ratio)
                || (ratio - r.successful_sessions as f64 / r.eligible_sessions as f64).abs() > 1e-9
        }) {
            r.ratio = None;
        }
        if summary
            .jitter
            .as_ref()
            .is_some_and(|j| !j.valid() || j.scope != "all_routes")
        {
            summary.jitter = None;
        }
        if summary.jitter_groups.len() > 3 {
            summary.jitter_groups.clear();
        }
        summary.jitter_groups.retain(|j| {
            j.statistics.valid()
                && j.statistics.scope == "route"
                && matches!(j.route_mode.as_str(), "direct" | "relay" | "mixed")
        });
    }
    serde_json::to_value(result).map_err(|_| "Could not encode feedback summaries".into())
}
#[tauri::command]
pub async fn feedback_machine_reviews(
    context: State<'_, AppContext>,
    machine_id: String,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<Value, String> {
    let machine =
        provider_id(&machine_id).ok_or("Machine ID must be a positive decimal bigint string")?;
    let limit = limit.unwrap_or(20);
    if !(1..=50).contains(&limit) || cursor.as_ref().is_some_and(|s| s.len() > 2048) {
        return Err("Review page exceeds its bounds".into());
    }
    let mut query = form_urlencoded::Serializer::new(String::new());
    query.append_pair("limit", &limit.to_string());
    if let Some(cursor) = cursor {
        query.append_pair("cursor", &cursor);
    }
    let value = context
        .feedback
        .participant_request(
            context.inner(),
            Method::GET,
            &format!("/machines/{machine}/reviews?{}", query.finish()),
            None,
        )
        .await?;
    let result: crate::models::feedback::ReviewsResponse =
        serde_json::from_value(value).map_err(|_| "Unsupported feedback review response")?;
    if result.reviews.len() > limit as usize
        || result.next_cursor.as_ref().is_some_and(|s| s.len() > 2048)
        || result.reviews.iter().any(|r| {
            !valid_uuid(&r.review_id)
                || r.score.is_some_and(|s| !(1..=10).contains(&s))
                || r.comment.as_ref().is_some_and(|s| s.chars().count() > 2000)
        })
    {
        return Err("Feedback review response exceeds its bounds".into());
    }
    serde_json::to_value(result).map_err(|_| "Could not encode feedback reviews".into())
}
#[tauri::command]
pub async fn feedback_review_report(
    context: State<'_, AppContext>,
    review_id: String,
    reason: String,
) -> Result<Value, String> {
    if !valid_uuid(&review_id)
        || reason.trim().is_empty()
        || reason.encode_utf16().count() > 500
        || reason.chars().any(char::is_control)
    {
        return Err("Provide a review ID and a plain-text reason of at most 500 characters".into());
    }
    context
        .feedback
        .participant_request(
            context.inner(),
            Method::POST,
            &format!("/reviews/{review_id}/reports"),
            Some(json!({"reason": reason.trim()})),
        )
        .await?;
    Ok(json!({"accepted": true}))
}
#[tauri::command]
pub async fn feedback_review_block_author(
    context: State<'_, AppContext>,
    review_id: String,
) -> Result<Value, String> {
    if !valid_uuid(&review_id) {
        return Err("Invalid review identity".into());
    }
    context
        .feedback
        .participant_request(
            context.inner(),
            Method::POST,
            &format!("/reviews/{review_id}/block-author"),
            None,
        )
        .await?;
    Ok(json!({"blocked": true}))
}
#[tauri::command]
pub async fn feedback_review_eligibility(
    context: State<'_, AppContext>,
    moonlight: State<'_, crate::moonlight::composition::MoonlightManager>,
    provider_instance_id: String,
) -> Result<Value, String> {
    let id = provider_id(&provider_instance_id)
        .ok_or("Rental ID must be a positive decimal bigint string")?;
    context
        .feedback
        .capture_frames(context.inner(), &moonlight.runtime)
        .await;
    Ok(json!({"eligible": context.feedback.review_eligible(&id)}))
}
#[tauri::command]
pub async fn feedback_stage_review(
    context: State<'_, AppContext>,
    provider_instance_id: String,
    score: Option<u8>,
    comment: Option<String>,
) -> Result<Value, String> {
    let id = provider_id(&provider_instance_id)
        .ok_or("Rental ID must be a positive decimal bigint string")?;
    context.feedback.stage_review(&id, score, comment)?;
    Ok(json!({"staged": true}))
}
