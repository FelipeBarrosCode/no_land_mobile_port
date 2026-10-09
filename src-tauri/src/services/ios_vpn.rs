//! Synchronous Rust side of the iOS Network Extension control contract.
//! Calls run from existing blocking tasks; Swift bounds every system callback.
use std::{
    ffi::{c_char, CStr, CString},
    path::Path,
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

const REFERENCE_PREFIX: &str = "vpn.configuration.";

unsafe extern "C" {
    fn nl_apple_vpn_install_and_start(request: *const c_char) -> *mut c_char;
    fn nl_apple_vpn_reconnect(request: *const c_char) -> *mut c_char;
    fn nl_apple_vpn_stop(request: *const c_char) -> *mut c_char;
    fn nl_apple_vpn_remove(request: *const c_char) -> *mut c_char;
    fn nl_apple_vpn_status(request: *const c_char) -> *mut c_char;
    fn nl_apple_vpn_update(request: *const c_char) -> *mut c_char;
    fn nl_apple_vpn_runtime(request: *const c_char) -> *mut c_char;
    fn nl_apple_vpn_configuration(request: *const c_char) -> *mut c_char;
    fn nl_apple_string_free(value: *mut c_char);
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnStatus {
    pub ok: bool,
    pub status: String,
    pub configuration_reference: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    pub ok: bool,
    pub error: Option<String>,
    pub active: bool,
    pub launch_id: String,
    pub config_fingerprint: String,
    pub peer_public_key: String,
    pub endpoint: String,
    pub mtu: u16,
    pub latest_handshake_age_secs: Option<u64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallRequest<'a> {
    configuration_reference: &'a str,
    configuration: &'a str,
    operation_id: &'a str,
    instance_id: u64,
    launch_id: &'a str,
    config_fingerprint: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceRequest<'a> {
    configuration_reference: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateRequest<'a> {
    configuration_reference: &'a str,
    configuration: &'a str,
    expected_config_fingerprint: &'a str,
    config_fingerprint: &'a str,
    transition_id: uuid::Uuid,
}

#[derive(Deserialize)]
struct ConfigurationResponse {
    ok: bool,
    configuration: Option<String>,
    error: Option<String>,
}

fn call<T: DeserializeOwned>(
    value: &impl Serialize,
    invoke: unsafe extern "C" fn(*const c_char) -> *mut c_char,
) -> AppResult<T> {
    let encoded = serde_json::to_vec(value)?;
    let encoded = CString::new(encoded)
        .map_err(|_| AppError::InvalidInput("Apple VPN request contains an interior NUL".into()))?;
    let pointer = unsafe { invoke(encoded.as_ptr()) };
    if pointer.is_null() {
        return Err(AppError::State(
            "Apple VPN bridge returned no response".into(),
        ));
    }
    let bytes = unsafe { CStr::from_ptr(pointer).to_bytes().to_vec() };
    unsafe { nl_apple_string_free(pointer) };
    serde_json::from_slice(&bytes).map_err(|error| {
        AppError::State(format!("Apple VPN bridge returned invalid JSON: {error}"))
    })
}

fn validate_status(status: VpnStatus) -> AppResult<VpnStatus> {
    if status.ok {
        Ok(status)
    } else {
        Err(AppError::Command(status.error.clone().unwrap_or_else(
            || format!("iOS managed VPN operation failed ({})", status.status),
        )))
    }
}

fn validate_runtime(runtime: Runtime) -> AppResult<Runtime> {
    if runtime.ok {
        Ok(runtime)
    } else {
        Err(AppError::State(runtime.error.clone().unwrap_or_else(
            || "iOS packet tunnel runtime is unavailable".into(),
        )))
    }
}

pub fn instance_id(config_path: &Path) -> AppResult<u64> {
    config_path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            AppError::State(format!(
                "WireGuard path does not contain an instance identity: {}",
                config_path.display()
            ))
        })
}

pub fn reference(config_path: &Path) -> AppResult<String> {
    Ok(format!("{REFERENCE_PREFIX}{}", instance_id(config_path)?))
}

pub fn install(
    config_path: &Path,
    configuration: &str,
    launch_id: &str,
    fingerprint: &str,
) -> AppResult<VpnStatus> {
    let reference = reference(config_path)?;
    validate_status(call(
        &InstallRequest {
            configuration_reference: &reference,
            configuration,
            operation_id: &uuid::Uuid::new_v4().to_string(),
            instance_id: instance_id(config_path)?,
            launch_id,
            config_fingerprint: fingerprint,
        },
        nl_apple_vpn_install_and_start,
    )?)
}

pub fn reconnect(config_path: &Path) -> AppResult<VpnStatus> {
    let reference = reference(config_path)?;
    validate_status(call(
        &ReferenceRequest {
            configuration_reference: &reference,
        },
        nl_apple_vpn_reconnect,
    )?)
}

pub fn stop(config_path: &Path) -> AppResult<VpnStatus> {
    let reference = reference(config_path)?;
    validate_status(call(
        &ReferenceRequest {
            configuration_reference: &reference,
        },
        nl_apple_vpn_stop,
    )?)
}

pub fn remove(config_path: &Path) -> AppResult<VpnStatus> {
    let reference = reference(config_path)?;
    validate_status(call(
        &ReferenceRequest {
            configuration_reference: &reference,
        },
        nl_apple_vpn_remove,
    )?)
}

pub fn status(config_path: &Path) -> AppResult<VpnStatus> {
    let reference = reference(config_path)?;
    call(
        &ReferenceRequest {
            configuration_reference: &reference,
        },
        nl_apple_vpn_status,
    )
}

pub fn update(
    config_path: &Path,
    configuration: &str,
    expected: &str,
    next: &str,
    transition_id: uuid::Uuid,
) -> AppResult<Runtime> {
    let reference = reference(config_path)?;
    validate_runtime(call(
        &UpdateRequest {
            configuration_reference: &reference,
            configuration,
            expected_config_fingerprint: expected,
            config_fingerprint: next,
            transition_id,
        },
        nl_apple_vpn_update,
    )?)
}

pub fn runtime(config_path: &Path) -> AppResult<Runtime> {
    let reference = reference(config_path)?;
    validate_runtime(call(
        &ReferenceRequest {
            configuration_reference: &reference,
        },
        nl_apple_vpn_runtime,
    )?)
}

pub fn configuration(config_path: &Path) -> AppResult<String> {
    let reference = reference(config_path)?;
    let response: ConfigurationResponse = call(
        &ReferenceRequest {
            configuration_reference: &reference,
        },
        nl_apple_vpn_configuration,
    )?;
    if !response.ok {
        return Err(AppError::State(response.error.unwrap_or_else(|| {
            "The protected WireGuard configuration is unavailable".into()
        })));
    }
    response
        .configuration
        .ok_or_else(|| AppError::State("The protected WireGuard configuration is empty".into()))
}
