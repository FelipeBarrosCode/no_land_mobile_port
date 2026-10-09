use crate::errors::{AppError, AppResult};
use serde::Deserialize;
use std::ffi::{c_char, CStr};

unsafe extern "C" {
    fn noland_ios_clipboard_read() -> *mut c_char;
    fn noland_ios_clipboard_write(bytes: *const u8, length: usize) -> *mut c_char;
    fn noland_ios_keep_awake(active: bool);
    fn noland_ios_response_free(value: *mut c_char);
}

#[derive(Deserialize)]
struct Response {
    text: Option<String>,
    error: Option<String>,
}

fn decode(pointer: *mut c_char) -> AppResult<Response> {
    if pointer.is_null() {
        return Err(AppError::Command(
            "iOS clipboard returned no response".into(),
        ));
    }
    // Native response is malloc-owned and freed exactly once, even on decode failure.
    let response =
        unsafe { serde_json::from_slice::<Response>(CStr::from_ptr(pointer).to_bytes()) };
    unsafe {
        noland_ios_response_free(pointer);
    }
    let response = response?;
    if let Some(error) = response.error {
        return Err(AppError::Command(error));
    }
    Ok(response)
}

pub fn read_clipboard() -> AppResult<String> {
    decode(unsafe { noland_ios_clipboard_read() })?
        .text
        .ok_or_else(|| AppError::Command("iOS clipboard returned no text".into()))
}

pub fn write_clipboard(text: &str) -> AppResult<()> {
    decode(unsafe { noland_ios_clipboard_write(text.as_ptr(), text.len()) })?;
    Ok(())
}

pub fn keep_awake(active: bool) {
    unsafe {
        noland_ios_keep_awake(active);
    }
}
