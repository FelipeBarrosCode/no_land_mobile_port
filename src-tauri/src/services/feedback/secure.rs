//! Dedicated device-bound Keychain namespace. Never falls back to files or the provider credential vault.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentRetry {
    pub request_id: String,
    pub recovery_proof: String,
    pub created_at: String,
    pub request: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionReceipt {
    pub receipt_id: String,
    pub status_token: String,
    pub status: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentAck {
    pub user_id: String,
    pub consent_epoch: u64,
    pub privacy_version: String,
    pub terms_version: String,
    pub consented_at: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Protected {
    pub schema_version: u32,
    pub installation_id: String,
    pub api_origin: String,
    pub credential: Option<String>,
    pub deletion_code: Option<String>,
    pub enrollment: Option<EnrollmentRetry>,
    pub enrollment_ack: Option<EnrollmentAck>,
    pub receipt: Option<DeletionReceipt>,
    #[serde(default)]
    pub stop_requested: bool,
    #[serde(default)]
    pub policy: Option<CachedPolicy>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CachedPolicy {
    pub paused: bool,
    pub privacy_version: String,
    pub terms_version: String,
}
// Deliberately no Debug implementation on any secret-bearing type.
pub fn supported() -> bool {
    cfg!(target_os = "ios")
}

#[cfg(target_os = "ios")]
mod platform {
    use std::ffi::{c_char, CStr};
    unsafe extern "C" {
        fn noland_ios_feedback_keychain_read() -> *mut c_char;
        fn noland_ios_feedback_keychain_write(bytes: *const u8, length: usize) -> *mut c_char;
        fn noland_ios_feedback_keychain_delete() -> *mut c_char;
        fn noland_ios_response_free(value: *mut c_char);
    }
    fn decode(ptr: *mut c_char) -> Result<serde_json::Value, String> {
        if ptr.is_null() {
            return Err("Feedback Keychain is unavailable".into());
        }
        let value =
            unsafe { serde_json::from_slice::<serde_json::Value>(CStr::from_ptr(ptr).to_bytes()) };
        unsafe {
            noland_ios_response_free(ptr);
        }
        let value = value.map_err(|_| "Feedback Keychain response is invalid")?;
        if value.get("error").is_some() {
            return Err(
                "Feedback Keychain is locked or unavailable; participation is paused".into(),
            );
        }
        Ok(value)
    }
    pub fn read() -> Result<Option<super::Protected>, String> {
        let value = decode(unsafe { noland_ios_feedback_keychain_read() })?;
        value
            .get("value")
            .and_then(|v| v.as_str())
            .map(|text| {
                let protected: super::Protected = serde_json::from_str(text).map_err(|_| {
                    "Feedback protected credential format is unsupported".to_string()
                })?;
                if protected.schema_version != 1
                    || !crate::models::feedback::valid_uuid(&protected.installation_id)
                {
                    return Err(
                        "Feedback protected credential version/identity is unsupported".into(),
                    );
                }
                Ok(protected)
            })
            .transpose()
    }
    pub fn write(value: &super::Protected) -> Result<(), String> {
        let bytes = serde_json::to_vec(value)
            .map_err(|_| "Cannot encode protected feedback credentials")?;
        decode(unsafe { noland_ios_feedback_keychain_write(bytes.as_ptr(), bytes.len()) })?;
        Ok(())
    }
    pub fn delete() -> Result<(), String> {
        decode(unsafe { noland_ios_feedback_keychain_delete() })?;
        Ok(())
    }
}
#[cfg(target_os = "ios")]
pub use platform::{delete, read, write};
#[cfg(not(target_os = "ios"))]
pub fn read() -> Result<Option<Protected>, String> {
    Err("Feedback enrollment is supported by the iOS device-bound credential adapter only in this port".into())
}
#[cfg(not(target_os = "ios"))]
pub fn write(_: &Protected) -> Result<(), String> {
    Err("No verified device-bound feedback credential backend for this target".into())
}
#[cfg(not(target_os = "ios"))]
pub fn delete() -> Result<(), String> {
    Err("No verified device-bound feedback credential backend for this target".into())
}
