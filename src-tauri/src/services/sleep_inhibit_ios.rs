use crate::errors::AppResult;

pub struct SleepInhibitService;

impl SleepInhibitService {
    pub fn ensure_active() -> AppResult<String> {
        super::ios_platform::keep_awake(true);
        Ok("Sleep prevention enabled for this streaming session".into())
    }

    pub fn stop() -> AppResult<String> {
        super::ios_platform::keep_awake(false);
        Ok("Sleep prevention stopped".into())
    }
}
