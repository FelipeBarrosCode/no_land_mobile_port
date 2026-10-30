use crate::{models::feedback::*, utils::atomic_file::write_atomically};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

const MAX_SESSIONS: usize = 100;
const MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_AGE_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RentalIntent {
    pub session_id: String,
    pub consent_epoch: u64,
    pub machine_id: Option<String>,
    pub offer_id: String,
    pub hardware: HardwareSnapshot,
    pub created_at: String,
    pub provider_instance_id: Option<String>,
    pub accepted_at: Option<String>,
    pub observation_complete: bool,
}
// Older journals did not track uncertain PUT delivery; conservatively DELETE.
fn legacy_review_may_have_been_sent() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRecord {
    pub session_id: String,
    pub initial_consent_epoch: u64,
    pub snapshot: SessionSnapshot,
    pub acknowledged_revision: u64,
    pub review: Option<ReviewDraft>,
    pub acknowledged_review_revision: u64,
    #[serde(default = "legacy_review_may_have_been_sent")]
    pub review_may_have_been_sent: bool,
    pub updated_at: String,
    pub suspended: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Spool {
    schema_version: u32,
    installation_id: Option<String>,
    pub sessions: BTreeMap<String, SessionRecord>,
    pub intents: BTreeMap<String, RentalIntent>,
    pub dropped_sessions: u64,
}
impl Default for Spool {
    fn default() -> Self {
        Self {
            schema_version: 1,
            installation_id: None,
            sessions: BTreeMap::new(),
            intents: BTreeMap::new(),
            dropped_sessions: 0,
        }
    }
}
struct Memory {
    spool: Spool,
    dirty: bool,
    error: Option<String>,
}
pub struct Journal {
    path: PathBuf,
    memory: Mutex<Memory>,
}
impl Journal {
    pub fn new(parent: &Path, load: bool, owner: Option<&str>) -> Self {
        let path = parent.join("feedback").join("outbox-v1.json");
        let loaded = if load && path.exists() {
            Self::load(&path, owner)
        } else {
            Ok(Spool {
                installation_id: owner.map(str::to_string),
                ..Spool::default()
            })
        };
        let (spool, error) = match loaded {
            Ok(mut spool) => {
                // A process gap is not complete observation. Preserve established successes,
                // but never turn missing evidence into a setup failure after a restart.
                for intent in spool.intents.values_mut() {
                    if intent.accepted_at.is_some() {
                        intent.observation_complete = false;
                    }
                }
                for record in spool.sessions.values_mut() {
                    if record.snapshot.lifecycle.destroyed_at.is_none()
                        && record.snapshot.lifecycle.observation_complete
                    {
                        record.snapshot.lifecycle.observation_complete = false;
                        record.snapshot.revision += 1;
                    }
                }
                (spool, None)
            }
            Err(error) => (Spool::default(), Some(error)),
        };
        Self {
            path,
            memory: Mutex::new(Memory {
                spool,
                dirty: load,
                error,
            }),
        }
    }
    fn load(path: &Path, owner: Option<&str>) -> Result<Spool, String> {
        if std::fs::metadata(path)
            .map_err(|_| "Feedback journal metadata is unavailable")?
            .len()
            > MAX_BYTES as u64
        {
            return Err("Feedback journal exceeds its bound; collection paused".into());
        }
        let body = std::fs::read(path).map_err(|_| "Feedback journal cannot be read")?;
        let spool: Spool = serde_json::from_slice(&body)
            .map_err(|_| "Feedback journal is invalid; collection paused (file preserved)")?;
        if spool.schema_version != 1
            || owner.is_none()
            || spool.installation_id.as_deref() != owner
            || spool.sessions.len() + spool.intents.len() > MAX_SESSIONS
            || spool.sessions.iter().any(|(id, r)| {
                provider_id(id).is_none()
                    || id != &r.snapshot.provider_instance_id
                    || !valid_uuid(&r.session_id)
                    || provider_id(&r.snapshot.provider_machine_id).is_none()
                    || !r.snapshot.valid()
                    || r.initial_consent_epoch == 0
                    || r.initial_consent_epoch > r.snapshot.consent_epoch
                    || r.acknowledged_revision > r.snapshot.revision
                    || !valid_time(&r.updated_at)
                    || r.review.as_ref().is_some_and(|v| {
                        v.revision == 0
                            || v.score.is_some_and(|s| !(1..=10).contains(&s))
                            || v.comment
                                .as_ref()
                                .is_some_and(|s| s.encode_utf16().count() > 2000)
                    })
                    || r.acknowledged_review_revision > r.review.as_ref().map_or(0, |v| v.revision)
            })
            || spool.intents.iter().any(|(id, i)| {
                id != &i.session_id
                    || !valid_uuid(id)
                    || i.consent_epoch == 0
                    || provider_id(&i.offer_id).as_ref() != Some(&i.offer_id)
                    || i.machine_id
                        .as_ref()
                        .is_some_and(|id| provider_id(id).as_ref() != Some(id))
                    || !valid_time(&i.created_at)
                    || !i.hardware.valid()
                    || i.accepted_at.is_some() != i.provider_instance_id.is_some()
                    || i.accepted_at.as_deref().is_some_and(|at| !valid_time(at))
                    || i.provider_instance_id
                        .as_ref()
                        .is_some_and(|id| provider_id(id).as_ref() != Some(id))
            })
        {
            return Err("Feedback journal schema is unsupported; collection paused".into());
        }
        Ok(spool)
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Memory>, String> {
        self.memory
            .lock()
            .map_err(|_| "Feedback journal lock is unavailable".into())
    }
    pub fn error(&self) -> Option<String> {
        self.lock().ok().and_then(|m| m.error.clone())
    }
    pub fn pending_count(&self) -> usize {
        self.lock()
            .map(|m| {
                m.spool
                    .sessions
                    .values()
                    .filter(|r| {
                        r.snapshot.revision > r.acknowledged_revision
                            || r.review
                                .as_ref()
                                .is_some_and(|v| v.revision > r.acknowledged_review_revision)
                    })
                    .count()
                    + m.spool
                        .intents
                        .values()
                        .filter(|i| i.accepted_at.is_some())
                        .count()
            })
            .unwrap_or(0)
    }
    pub fn dropped(&self) -> u64 {
        self.lock().map(|m| m.spool.dropped_sessions).unwrap_or(0)
    }
    pub fn records(&self) -> Result<Vec<SessionRecord>, String> {
        Ok(self.lock()?.spool.sessions.values().cloned().collect())
    }
    pub fn intents(&self) -> Vec<RentalIntent> {
        self.lock()
            .map(|m| m.spool.intents.values().cloned().collect())
            .unwrap_or_default()
    }
    pub fn record(&self, id: &str) -> Option<SessionRecord> {
        self.lock().ok()?.spool.sessions.get(id).cloned()
    }
    fn write(&self, spool: &Spool) -> Result<(), String> {
        let body = serde_json::to_vec(spool).map_err(|_| "Feedback journal cannot be encoded")?;
        if body.len() > MAX_BYTES {
            return Err("Feedback journal is full; new feedback is paused".into());
        }
        let parent = self
            .path
            .parent()
            .ok_or("Feedback journal path is unavailable")?;
        std::fs::create_dir_all(parent)
            .map_err(|_| "Feedback journal directory cannot be created")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "Feedback journal protection failed")?;
        }
        protect_path(parent)?;
        write_atomically(&self.path, &body).map_err(|_| {
            "Feedback journal checkpoint failed; normal rental operations remain available"
                .to_string()
        })?;
        protect_path(&self.path)
    }
    pub fn checkpoint(&self) -> Result<(), String> {
        let mut m = self.lock()?;
        if let Some(error) = &m.error {
            return Err(error.clone());
        }
        if m.dirty {
            self.write(&m.spool)?;
            m.dirty = false;
        }
        Ok(())
    }
    pub fn change<T>(
        &self,
        durable: bool,
        f: impl FnOnce(&mut Spool) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut m = self.lock()?;
        if let Some(error) = &m.error {
            return Err(error.clone());
        }
        let mut next = m.spool.clone();
        let result = f(&mut next)?;
        if next.sessions.len() + next.intents.len() > MAX_SESSIONS {
            return Err("Feedback journal is full; new rentals will not contribute".into());
        }
        if durable {
            self.write(&next)?;
        }
        m.spool = next;
        m.dirty = !durable;
        Ok(result)
    }
    pub fn bind_owner(&self, owner: &str) -> Result<(), String> {
        let mut m = self.lock()?;
        let next = Spool {
            installation_id: Some(owner.into()),
            ..Spool::default()
        };
        self.write(&next)?;
        m.spool = next;
        m.dirty = false;
        m.error = None;
        Ok(())
    }
    pub fn clear(&self) -> Result<(), String> {
        let mut m = self.lock()?;
        let next = Spool {
            installation_id: m.spool.installation_id.clone(),
            ..Spool::default()
        };
        // Do not create any feedback file on a never-enrolled installation.
        if self.path.exists() {
            self.write(&next)?;
        }
        m.spool = next;
        m.dirty = false;
        m.error = None;
        Ok(())
    }
    pub fn prune(&self) -> Result<(), String> {
        self.change(true, |s| {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(MAX_AGE_DAYS);
            let old: Vec<_> = s
                .sessions
                .iter()
                .filter(|(_, r)| {
                    chrono::DateTime::parse_from_rfc3339(&r.snapshot.lifecycle.rental_started_at)
                        .is_ok_and(|at| at < cutoff)
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in old {
                s.sessions.remove(&id);
                s.dropped_sessions += 1;
            }
            s.intents.retain(|_, i| {
                chrono::DateTime::parse_from_rfc3339(&i.created_at).is_ok_and(|at| at >= cutoff)
            });
            Ok(())
        })
    }
    pub fn prepare(
        &self,
        epoch: u64,
        machine_id: Option<String>,
        offer_id: String,
        hardware: HardwareSnapshot,
    ) -> Result<String, String> {
        self.prune()?;
        let id = uuid::Uuid::new_v4().to_string();
        self.change(true, |s| {
            // Retire acknowledged terminal records before refusing a new session, never an active rental's identity.
            if s.sessions.len() + s.intents.len() >= MAX_SESSIONS {
                let oldest = s
                    .sessions
                    .iter()
                    .filter(|(_, r)| {
                        r.snapshot.lifecycle.destroyed_at.is_some()
                            && r.acknowledged_revision == r.snapshot.revision
                            && r.review
                                .as_ref()
                                .is_none_or(|v| v.revision == r.acknowledged_review_revision)
                    })
                    .min_by_key(|(_, r)| &r.updated_at)
                    .map(|(id, _)| id.clone());
                if let Some(oldest) = oldest {
                    s.sessions.remove(&oldest);
                }
            }
            s.intents.insert(
                id.clone(),
                RentalIntent {
                    session_id: id.clone(),
                    consent_epoch: epoch,
                    machine_id,
                    offer_id,
                    hardware,
                    created_at: now(),
                    provider_instance_id: None,
                    accepted_at: None,
                    observation_complete: true,
                },
            );
            Ok(id.clone())
        })
    }
    pub fn confirm(
        &self,
        intent_id: &str,
        rental_id: String,
        machine_id: Option<String>,
        epoch: u64,
    ) -> Result<Option<String>, String> {
        self.change(true, |s| {
            if let Some(existing) = s.sessions.get_mut(&rental_id) {
                if existing.snapshot.consent_epoch != epoch {
                    return Ok(None);
                }
                if machine_id
                    .as_ref()
                    .is_some_and(|id| id != &existing.snapshot.provider_machine_id)
                {
                    existing.suspended = true;
                    return Ok(None);
                }
                let session_id = existing.session_id.clone();
                s.intents.remove(intent_id);
                return Ok(Some(session_id));
            }
            let Some(intent) = s.intents.get_mut(intent_id) else {
                return Ok(None);
            };
            if intent.consent_epoch != epoch
                || intent
                    .provider_instance_id
                    .as_ref()
                    .is_some_and(|id| id != &rental_id)
            {
                return Ok(None);
            }
            intent.provider_instance_id = Some(rental_id.clone());
            if intent.accepted_at.is_none() {
                intent.accepted_at = Some(now());
            }
            let intent = intent.clone();
            // A provider conflict is excluded, not silently joined to a different physical machine.
            if machine_id.is_some()
                && intent.machine_id.is_some()
                && machine_id != intent.machine_id
            {
                return Ok(None);
            }
            let Some(machine_id) = machine_id
                .or(intent.machine_id)
                .and_then(|id| provider_id(&id))
            else {
                return Ok(None);
            };
            s.intents.remove(intent_id);
            let mut lifecycle = Lifecycle::new();
            lifecycle.rental_started_at = intent.accepted_at.unwrap_or_else(now);
            lifecycle.observation_complete = intent.observation_complete;
            let snapshot = SessionSnapshot {
                schema_version: 1,
                revision: 1,
                consent_epoch: epoch,
                provider: "vast".into(),
                provider_machine_id: machine_id,
                provider_instance_id: rental_id.clone(),
                provider_offer_id: Some(intent.offer_id),
                lifecycle,
                context: Context {
                    platform: std::env::consts::OS.into(),
                    app_version: env!("CARGO_PKG_VERSION").into(),
                    collector_version: METRIC_VERSION.into(),
                },
                hardware_snapshot: intent.hardware,
                jitter_buckets: vec![],
            };
            s.sessions.insert(
                rental_id,
                SessionRecord {
                    session_id: intent_id.into(),
                    initial_consent_epoch: epoch,
                    snapshot,
                    acknowledged_revision: 0,
                    review: None,
                    acknowledged_review_revision: 0,
                    review_may_have_been_sent: false,
                    updated_at: now(),
                    suspended: false,
                },
            );
            Ok(Some(intent_id.into()))
        })
    }
    pub fn milestone(&self, id: &str, epoch: u64, event: Milestone) -> Result<(), String> {
        self.change(true, |s| {
            if let Some(r) = s
                .sessions
                .get_mut(id)
                .filter(|r| r.snapshot.consent_epoch == epoch)
            {
                if r.snapshot.lifecycle.observe(event) {
                    r.snapshot.revision += 1;
                    r.updated_at = now();
                }
            }
            Ok(())
        })
    }
    pub fn observation_gap(
        &self,
        absent_from: Option<&std::collections::HashSet<u64>>,
    ) -> Result<(), String> {
        self.change(true, |s| {
            for intent in s.intents.values_mut() {
                if intent.accepted_at.is_some() {
                    intent.observation_complete = false;
                }
            }
            for r in s.sessions.values_mut() {
                if r.snapshot.lifecycle.destroyed_at.is_none()
                    && r.snapshot.lifecycle.observation_complete
                    && absent_from.is_none_or(|ids| {
                        r.snapshot
                            .provider_instance_id
                            .parse::<u64>()
                            .is_ok_and(|id| !ids.contains(&id))
                    })
                {
                    r.snapshot.lifecycle.observation_complete = false;
                    r.snapshot.revision += 1;
                    r.updated_at = now();
                }
            }
            Ok(())
        })
    }
    pub fn mark_unknown(&self, id: &str) -> Result<(), String> {
        self.change(true, |s| {
            if let Some(r) = s.sessions.get_mut(id) {
                if r.snapshot.lifecycle.observation_complete
                    && r.snapshot.lifecycle.destroyed_at.is_none()
                {
                    r.snapshot.lifecycle.observation_complete = false;
                    r.snapshot.revision += 1;
                    r.updated_at = now();
                }
            }
            Ok(())
        })
    }
    pub fn stage(
        &self,
        id: &str,
        epoch: u64,
        score: Option<u8>,
        comment: Option<String>,
    ) -> Result<(), String> {
        self.change(true, |s| {
            let r = s
                .sessions
                .get_mut(id)
                .filter(|r| {
                    r.snapshot.consent_epoch == epoch
                        && !r.suspended
                        && r.snapshot.lifecycle.review_eligible()
                        && (r.snapshot.lifecycle.destroyed_at.is_none()
                            || (score.is_none() && comment.is_none()))
                })
                .ok_or("This rental has no enrolled native frame evidence")?;
            let revision = r
                .review
                .as_ref()
                .map_or(0, |v| v.revision)
                .checked_add(1)
                .ok_or("Feedback review revision exhausted")?;
            r.review = Some(ReviewDraft {
                revision,
                score,
                comment,
            });
            r.updated_at = now();
            Ok(())
        })
    }
    // Claim the latest action atomically with the write-ahead delivery marker. A lost
    // PUT acknowledgment must still cause a later cancellation to DELETE remotely.
    pub fn claim_review(&self, id: &str) -> Result<Option<ReviewDraft>, String> {
        self.change(true, |s| {
            let Some(r) = s.sessions.get_mut(id) else {
                return Ok(None);
            };
            let Some(review) = r.review.clone() else {
                return Ok(None);
            };
            if r.suspended || review.revision <= r.acknowledged_review_revision {
                return Ok(None);
            }
            let removed = review.score.is_none() && review.comment.is_none();
            if removed && !r.review_may_have_been_sent && r.acknowledged_review_revision == 0 {
                r.acknowledged_review_revision = review.revision;
                return Ok(None);
            }
            if !removed {
                if r.snapshot.lifecycle.destroyed_at.is_none()
                    || !r.snapshot.lifecycle.review_eligible()
                {
                    return Ok(None);
                }
                r.review_may_have_been_sent = true;
            }
            Ok(Some(review))
        })
    }
    pub fn acknowledge(&self, id: &str, revision: u64, review: bool) -> Result<(), String> {
        self.change(true, |s| {
            if let Some(r) = s.sessions.get_mut(id) {
                if review {
                    r.acknowledged_review_revision = r.acknowledged_review_revision.max(revision);
                } else {
                    r.acknowledged_revision = r.acknowledged_revision.max(revision);
                }
            }
            Ok(())
        })
    }
    pub fn epoch(&self, epoch: u64) -> Result<(), String> {
        self.change(true, |s| {
            for r in s.sessions.values_mut() {
                if r.snapshot.consent_epoch != epoch {
                    r.snapshot.consent_epoch = epoch;
                    r.snapshot.revision += 1;
                }
            }
            // Unconfirmed rentals cannot cross a consent boundary.
            s.intents.clear();
            Ok(())
        })
    }
}

fn protect_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "ios")]
    {
        unsafe extern "C" {
            fn noland_ios_feedback_protect_spool(path: *const std::ffi::c_char) -> i32;
        }
        let path = std::ffi::CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| "Invalid feedback spool path")?;
        if unsafe { noland_ios_feedback_protect_spool(path.as_ptr()) } != 1 {
            return Err(
                "Feedback spool could not be device-protected and excluded from backup".into(),
            );
        }
    }
    #[cfg(not(target_os = "ios"))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn journal() -> (PathBuf, Journal) {
        let p = std::env::temp_dir().join(format!("noland-feedback-test-{}", uuid::Uuid::new_v4()));
        (
            p.clone(),
            Journal::new(&p, false, Some("e2883b2b-65d5-4b9c-b422-3c3c367c569b")),
        )
    }
    #[test]
    fn journal_is_stable_per_rental_and_review_stays_independent() {
        let (p, j) = journal();
        let intent = j
            .prepare(1, Some("123".into()), "456".into(), Default::default())
            .unwrap();
        assert_eq!(
            j.confirm(&intent, "789".into(), None, 1).unwrap(),
            Some(intent.clone())
        );
        assert_eq!(
            j.confirm(&intent, "789".into(), None, 1).unwrap(),
            Some(intent.clone())
        );
        assert!(j.stage("789", 1, Some(9), None).is_err());
        j.milestone("789", 1, Milestone::FirstFrame).unwrap();
        j.stage("789", 1, Some(9), None).unwrap();
        assert!(j
            .record("789")
            .unwrap()
            .snapshot
            .lifecycle
            .destroyed_at
            .is_none());
        j.milestone("789", 1, Milestone::Destroyed).unwrap();
        let restored = Journal::new(&p, true, Some("e2883b2b-65d5-4b9c-b422-3c3c367c569b"));
        assert_eq!(restored.record("789").unwrap().session_id, intent);
        assert_eq!(restored.record("789").unwrap().review.unwrap().revision, 1);
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn skip_replaces_failed_destroy_draft_and_survives_restart() {
        let (p, j) = journal();
        let intent = j
            .prepare(1, Some("123".into()), "456".into(), Default::default())
            .unwrap();
        j.confirm(&intent, "789".into(), None, 1).unwrap();
        j.milestone("789", 1, Milestone::FirstFrame).unwrap();
        j.stage("789", 1, Some(9), Some("old content".into()))
            .unwrap();
        j.stage("789", 1, None, None).unwrap();
        let restored = Journal::new(&p, true, Some("e2883b2b-65d5-4b9c-b422-3c3c367c569b"));
        let review = restored.record("789").unwrap().review.unwrap();
        assert_eq!(review.revision, 2);
        assert!(review.score.is_none() && review.comment.is_none());
        assert!(restored.claim_review("789").unwrap().is_none());
        restored.stage("789", 1, Some(8), None).unwrap();
        restored.milestone("789", 1, Milestone::Destroyed).unwrap();
        assert_eq!(restored.claim_review("789").unwrap().unwrap().revision, 3);
        // No PUT acknowledgment: cancellation must still reach remote DELETE.
        restored.stage("789", 1, None, None).unwrap();
        let removal = restored.claim_review("789").unwrap().unwrap();
        assert_eq!(removal.revision, 4);
        assert!(removal.score.is_none() && removal.comment.is_none());
        restored.acknowledge("789", 3, true).unwrap();
        assert_eq!(restored.claim_review("789").unwrap().unwrap().revision, 4);
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn gaps_and_ambiguous_absence_are_not_failures() {
        let (p, j) = journal();
        let intent = j
            .prepare(1, Some("123".into()), "456".into(), Default::default())
            .unwrap();
        j.confirm(&intent, "789".into(), None, 1).unwrap();
        let restored = Journal::new(&p, true, Some("e2883b2b-65d5-4b9c-b422-3c3c367c569b"));
        assert!(
            !restored
                .record("789")
                .unwrap()
                .snapshot
                .lifecycle
                .observation_complete
        );
        assert!(restored
            .record("789")
            .unwrap()
            .snapshot
            .lifecycle
            .destroyed_at
            .is_none());
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn missing_machine_waits_for_verified_details_without_guessing_an_offer_or_host() {
        let (p, j) = journal();
        let id = j
            .prepare(1, None, "456".into(), Default::default())
            .unwrap();
        assert!(j
            .confirm(&id, "9007199254740993".into(), None, 1)
            .unwrap()
            .is_none());
        assert!(j.record("9007199254740993").is_none());
        let accepted = j.intents()[0].accepted_at.clone();
        j.confirm(
            &id,
            "9007199254740993".into(),
            Some("9223372036854775807".into()),
            1,
        )
        .unwrap();
        let snapshot = j.record("9007199254740993").unwrap().snapshot;
        assert_eq!(Some(snapshot.lifecycle.rental_started_at.clone()), accepted);
        let body = serde_json::to_value(snapshot).unwrap();
        assert_eq!(body["providerMachineId"], "9223372036854775807");
        assert_eq!(body["providerInstanceId"], "9007199254740993");
        assert_eq!(body["providerOfferId"], "456");
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn a_new_installation_cannot_replay_an_old_installations_journal() {
        let (p, j) = journal();
        let id = j
            .prepare(1, Some("123".into()), "456".into(), Default::default())
            .unwrap();
        j.confirm(&id, "789".into(), None, 1).unwrap();
        let before = std::fs::read(p.join("feedback/outbox-v1.json")).unwrap();
        let wrong_owner = Journal::new(&p, true, Some("9a9bc7a4-a34c-454b-8349-ab31bcb71da4"));
        assert!(wrong_owner.error().is_some());
        assert!(wrong_owner.checkpoint().is_err());
        assert_eq!(
            before,
            std::fs::read(p.join("feedback/outbox-v1.json")).unwrap()
        );
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn queue_count_and_byte_caps_do_not_replace_the_previous_durable_spool() {
        let (p, j) = journal();
        j.change(true, |s| {
            for _ in 0..MAX_SESSIONS {
                let id = uuid::Uuid::new_v4().to_string();
                s.intents.insert(
                    id.clone(),
                    RentalIntent {
                        session_id: id,
                        consent_epoch: 1,
                        machine_id: Some("123".into()),
                        offer_id: "456".into(),
                        hardware: Default::default(),
                        created_at: now(),
                        provider_instance_id: None,
                        accepted_at: None,
                        observation_complete: true,
                    },
                );
            }
            Ok(())
        })
        .unwrap();
        assert!(j
            .prepare(1, Some("123".into()), "456".into(), Default::default())
            .is_err());
        let before = std::fs::read(p.join("feedback/outbox-v1.json")).unwrap();
        assert!(j
            .change(true, |s| {
                s.intents.values_mut().next().unwrap().hardware.gpu_name =
                    Some("x".repeat(MAX_BYTES));
                Ok(())
            })
            .is_err());
        assert_eq!(
            before,
            std::fs::read(p.join("feedback/outbox-v1.json")).unwrap()
        );
        assert_eq!(j.intents().len(), MAX_SESSIONS);
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn no_enrollment_creates_no_spool_even_on_checkpoint_or_clear() {
        let p = std::env::temp_dir().join(format!("noland-feedback-off-{}", uuid::Uuid::new_v4()));
        let j = Journal::new(&p, false, None);
        j.checkpoint().unwrap();
        j.clear().unwrap();
        assert!(!p.exists());
    }
    #[test]
    fn scope_changes_keep_the_rental_and_independent_review_revision() {
        let (p, j) = journal();
        let id = j
            .prepare(1, Some("123".into()), "456".into(), Default::default())
            .unwrap();
        j.confirm(&id, "789".into(), None, 1).unwrap();
        j.milestone("789", 1, Milestone::Ready).unwrap();
        j.milestone("789", 1, Milestone::Paired).unwrap();
        j.milestone("789", 1, Milestone::FirstFrame).unwrap();
        j.stage("789", 1, Some(8), None).unwrap();
        let before = j.record("789").unwrap();
        j.epoch(2).unwrap();
        let after = j.record("789").unwrap();
        assert_eq!(before.session_id, after.session_id);
        assert_eq!(after.initial_consent_epoch, 1);
        assert_eq!(after.snapshot.consent_epoch, 2);
        assert!(after.snapshot.revision > before.snapshot.revision);
        assert_eq!(after.review.unwrap().revision, 1);
        assert!(after.snapshot.lifecycle.setup_success_at.is_some());
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn expiry_is_not_extended_by_retries_and_preserves_no_synthetic_destruction() {
        let (p, j) = journal();
        let id = j
            .prepare(1, Some("123".into()), "456".into(), Default::default())
            .unwrap();
        j.confirm(&id, "789".into(), None, 1).unwrap();
        j.change(true, |s| {
            s.sessions
                .get_mut("789")
                .unwrap()
                .snapshot
                .lifecycle
                .rental_started_at = (chrono::Utc::now() - chrono::Duration::days(31)).to_rfc3339();
            Ok(())
        })
        .unwrap();
        j.prune().unwrap();
        assert!(j.record("789").is_none());
        assert_eq!(j.dropped(), 1);
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn consent_boundary_does_not_join_existing_rental() {
        let (p, j) = journal();
        let intent = j
            .prepare(1, Some("123".into()), "456".into(), Default::default())
            .unwrap();
        assert!(j.confirm(&intent, "789".into(), None, 2).unwrap().is_none());
        assert!(j.record("789").is_none());
        std::fs::remove_dir_all(p).unwrap();
    }
}
