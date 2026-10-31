//! Job-bound gateway sessions for asynchronous report exports.
//!
//! The gateway binds an export job to the session that created it and
//! cancels the job as soon as that session ends. With request-scoped
//! `Basic` authentication the gateway tears the session down at the end of
//! the creating request, so a purely stateless pass-through could never
//! complete an export. This is the one place gvm-mcp holds gateway state:
//! one session per export job, opened with the caller's own credentials,
//! usable only by that caller, and deleted as soon as the artifact is
//! downloaded, the job is cancelled, or it reaches a terminal state.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use secrecy::{ExposeSecret, SecretString};

/// Upper bound on concurrently tracked export sessions per server. The
/// oldest entry is evicted (and its session closed) beyond this.
const MAX_TRACKED: usize = 64;

/// Entries older than this are dropped on the next insert: the gateway
/// removes terminal jobs 15 minutes after completion and idles sessions out
/// before that when nobody polls.
const MAX_AGE: Duration = Duration::from_secs(20 * 60);

struct JobSession {
    token: SecretString,
    /// The `Authorization` value of the identity that created the job; only
    /// the same identity may use the session.
    owner: SecretString,
    created: Instant,
}

/// Registry of export jobs this server created and the gateway session each
/// one is bound to.
#[derive(Default)]
pub struct JobSessions {
    inner: Mutex<HashMap<String, JobSession>>,
}

impl std::fmt::Debug for JobSessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tracked = self.inner.lock().map(|m| m.len()).unwrap_or(0);
        f.debug_struct("JobSessions")
            .field("tracked", &tracked)
            .finish()
    }
}

impl JobSessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember that `job_id` runs on the session `token`, created by
    /// `owner`. Returns the tokens of sessions evicted to make room or
    /// because they aged out; the caller closes those at the gateway.
    pub fn track(&self, job_id: String, token: String, owner: &str) -> Vec<String> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut evicted = Vec::new();
        let now = Instant::now();
        map.retain(|_, session| {
            let keep = now.duration_since(session.created) < MAX_AGE;
            if !keep {
                evicted.push(session.token.expose_secret().to_owned());
            }
            keep
        });
        while map.len() >= MAX_TRACKED {
            let oldest = map
                .iter()
                .min_by_key(|(_, s)| s.created)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => {
                    if let Some(session) = map.remove(&id) {
                        evicted.push(session.token.expose_secret().to_owned());
                    }
                }
                None => break,
            }
        }
        map.insert(
            job_id,
            JobSession {
                token: SecretString::from(token),
                owner: SecretString::from(owner.to_owned()),
                created: now,
            },
        );
        evicted
    }

    /// The session token bound to `job_id`, if this server created the job
    /// and `owner` is the identity that did so.
    pub fn token_for(&self, job_id: &str, owner: &str) -> Option<String> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(job_id)
            .filter(|session| session.owner.expose_secret() == owner)
            .map(|session| session.token.expose_secret().to_owned())
    }

    /// Forget `job_id` (same ownership rule) and return its session token
    /// so the caller can close the session at the gateway.
    pub fn untrack(&self, job_id: &str, owner: &str) -> Option<String> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let owned = map
            .get(job_id)
            .is_some_and(|session| session.owner.expose_secret() == owner);
        if !owned {
            return None;
        }
        map.remove(job_id)
            .map(|session| session.token.expose_secret().to_owned())
    }

    /// Number of tracked jobs (diagnostics/tests).
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_only_visible_to_its_owner() {
        let jobs = JobSessions::new();
        assert!(
            jobs.track("job-1".into(), "tok".into(), "Basic a")
                .is_empty()
        );
        assert_eq!(jobs.token_for("job-1", "Basic a").as_deref(), Some("tok"));
        assert_eq!(jobs.token_for("job-1", "Basic b"), None);
        assert_eq!(jobs.untrack("job-1", "Basic b"), None);
        assert_eq!(jobs.untrack("job-1", "Basic a").as_deref(), Some("tok"));
        assert!(jobs.is_empty());
    }

    #[test]
    fn oldest_session_is_evicted_beyond_the_cap() {
        let jobs = JobSessions::new();
        for i in 0..MAX_TRACKED {
            assert!(
                jobs.track(format!("job-{i}"), format!("tok-{i}"), "o")
                    .is_empty()
            );
        }
        let evicted = jobs.track("job-extra".into(), "tok-extra".into(), "o");
        assert_eq!(evicted, vec!["tok-0".to_string()]);
        assert_eq!(jobs.len(), MAX_TRACKED);
        assert_eq!(jobs.token_for("job-0", "o"), None);
    }

    #[test]
    fn debug_output_hides_tokens() {
        let jobs = JobSessions::new();
        jobs.track("job-1".into(), "very-secret".into(), "Basic a");
        let rendered = format!("{jobs:?}");
        assert!(!rendered.contains("very-secret"));
        assert!(!rendered.contains("Basic a"));
    }
}
