//! Pooled gateway sessions and per-identity concurrency control.
//!
//! The rust-gvm-api gateway authenticates every request-scoped `Basic` call
//! by opening a gvmd session for it and closing that session afterwards, and
//! it caps those sessions at ten per gvmd user. Sending the configured
//! credential on every tool call therefore costs one gvmd login per call and
//! fails outright under parallel tool calls. In session mode (the default)
//! the client opens one gateway session per identity instead and reuses its
//! bearer token until the gateway's idle timeout would expire it.
//!
//! Security properties of the pool:
//!
//! - it is keyed by a SHA-256 digest of the `Authorization` value, never by
//!   the credential itself, so forwarded caller passwords are not retained;
//! - a token is only ever used for the identity whose credential created it;
//! - tokens are [`SecretString`]s that never appear in logs or `Debug` output;
//! - the pool is bounded in size and age, and evicted sessions are closed at
//!   the gateway by the client.
//!
//! The same per-identity slot carries the in-flight gate that keeps parallel
//! requests of one identity below the gateway's session cap in both modes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use secrecy::{ExposeSecret, SecretString};
use tokio::sync::Semaphore;

/// Identities tracked at most; the least recently used one is evicted.
pub const MAX_IDENTITIES: usize = 64;

/// A pooled session is replaced after this long even while it is active, so
/// a token's lifetime stays bounded independently of gateway settings.
pub const MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// A session counts as expired this long before the gateway would expire it,
/// so a request never races the idle timeout.
const EXPIRY_MARGIN: Duration = Duration::from_secs(15);

/// Idle timeout assumed when the gateway reports none.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// A pooled session that has not been used for this long is closed at the
/// gateway, so the slot it occupies in the gateway's per-user cap goes back
/// to other instances between bursts of activity. The next call logs in
/// again (one login per burst instead of one per call).
pub const IDLE_RELEASE: Duration = Duration::from_secs(60);

/// How long session creation is suspended after a failure other than the
/// gateway's limits (for example a gateway without the session endpoint);
/// requests fall back to request-scoped `Basic` meanwhile.
pub const SUSPEND_AFTER_FAILURE: Duration = Duration::from_secs(60);

/// Digest of an `Authorization` value. Two callers with the same credential
/// share one key; the credential itself is never stored.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdentityKey([u8; 32]);

impl IdentityKey {
    pub fn of(authorization: &str) -> Self {
        let digest = ring::digest::digest(&ring::digest::SHA256, authorization.as_bytes());
        let mut key = [0u8; 32];
        key.copy_from_slice(digest.as_ref());
        Self(key)
    }
}

impl std::fmt::Debug for IdentityKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A short prefix is enough to correlate log lines; it does not
        // identify the credential.
        write!(
            f,
            "IdentityKey({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

/// One gateway session held on behalf of an identity.
pub struct PooledSession {
    token: SecretString,
    idle_timeout: Duration,
    created: Instant,
    last_used: Instant,
}

impl PooledSession {
    pub fn new(token: String, expires_in_secs: Option<u64>) -> Self {
        let now = Instant::now();
        Self {
            token: SecretString::from(token),
            idle_timeout: expires_in_secs
                .map(Duration::from_secs)
                .unwrap_or(DEFAULT_IDLE_TIMEOUT),
            created: now,
            last_used: now,
        }
    }

    fn usable_at(&self, now: Instant) -> bool {
        now.duration_since(self.created) < MAX_AGE
            && now.duration_since(self.last_used) < self.idle_timeout.saturating_sub(EXPIRY_MARGIN)
    }
}

#[derive(Default)]
struct SlotState {
    session: Option<PooledSession>,
    suspended_until: Option<Instant>,
}

/// Per-identity state: the pooled session, the creation lock and the
/// in-flight gate.
pub struct Slot {
    state: Mutex<SlotState>,
    /// Serializes session creation so a burst of first calls performs one
    /// login instead of one per call.
    create_lock: tokio::sync::Mutex<()>,
    gate: Arc<Semaphore>,
    last_touch: Mutex<Instant>,
}

impl Slot {
    fn new(max_in_flight: usize) -> Self {
        Self {
            state: Mutex::new(SlotState::default()),
            create_lock: tokio::sync::Mutex::new(()),
            gate: Arc::new(Semaphore::new(max_in_flight)),
            last_touch: Mutex::new(Instant::now()),
        }
    }

    /// The in-flight gate of this identity.
    pub fn gate(&self) -> Arc<Semaphore> {
        Arc::clone(&self.gate)
    }

    /// Hold this while deciding whether a session must be created.
    pub async fn creating(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.create_lock.lock().await
    }

    /// The current session token if it is still usable. An expired session
    /// is dropped and its token returned separately so the caller can close
    /// it at the gateway.
    pub fn current(&self) -> (Option<String>, Option<String>) {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match state.session.as_mut() {
            Some(session) if session.usable_at(now) => {
                session.last_used = now;
                (Some(session.token.expose_secret().to_owned()), None)
            }
            Some(_) => {
                let stale = state
                    .session
                    .take()
                    .map(|s| s.token.expose_secret().to_owned());
                (None, stale)
            }
            None => (None, None),
        }
    }

    /// Store a freshly created session.
    pub fn install(&self, session: PooledSession) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.session = Some(session);
        state.suspended_until = None;
    }

    /// Forget the session if `token` is the one currently pooled (the
    /// gateway rejected it). Returns whether anything was dropped.
    pub fn invalidate(&self, token: &str) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let matches = state
            .session
            .as_ref()
            .is_some_and(|s| s.token.expose_secret() == token);
        if matches {
            state.session = None;
        }
        matches
    }

    /// Whether session creation is currently suspended for this identity.
    pub fn is_suspended(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .suspended_until
            .is_some_and(|until| until > Instant::now())
    }

    /// Suspend session creation for `duration`; requests use request-scoped
    /// `Basic` authentication meanwhile.
    pub fn suspend(&self, duration: Duration) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.suspended_until = Some(Instant::now() + duration);
    }

    /// Mark the identity as recently used (LRU bookkeeping).
    pub fn touch(&self) {
        *self.last_touch.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    fn last_touch(&self) -> Instant {
        *self.last_touch.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Take the pooled session token, if any (shutdown and eviction).
    pub fn take_token(&self) -> Option<String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .session
            .take()
            .map(|s| s.token.expose_secret().to_owned())
    }

    /// Take the pooled session token if the session has been idle for at
    /// least `idle_release` (the reaper closes it at the gateway).
    pub fn take_if_idle(&self, idle_release: Duration) -> Option<String> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let idle = state
            .session
            .as_ref()
            .is_some_and(|s| now.duration_since(s.last_used) >= idle_release);
        if !idle {
            return None;
        }
        state
            .session
            .take()
            .map(|s| s.token.expose_secret().to_owned())
    }

    /// Whether a session is currently pooled (diagnostics and tests).
    pub fn has_session(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .session
            .is_some()
    }
}

/// Registry of per-identity slots, bounded to [`MAX_IDENTITIES`].
pub struct SessionPool {
    slots: Mutex<HashMap<IdentityKey, Arc<Slot>>>,
    max_in_flight: usize,
    idle_release: Duration,
}

impl std::fmt::Debug for SessionPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let identities = self.slots.lock().map(|m| m.len()).unwrap_or(0);
        f.debug_struct("SessionPool")
            .field("identities", &identities)
            .field("max_in_flight", &self.max_in_flight)
            .finish()
    }
}

impl SessionPool {
    pub fn new(max_in_flight: usize) -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
            max_in_flight: max_in_flight.max(1),
            idle_release: IDLE_RELEASE,
        }
    }

    /// Override how long a session may stay unused before the reaper closes
    /// it (tests and tuning).
    pub fn with_idle_release(mut self, idle_release: Duration) -> Self {
        self.idle_release = idle_release;
        self
    }

    /// How long a pooled session may stay unused before it is closed.
    pub fn idle_release(&self) -> Duration {
        self.idle_release
    }

    /// The per-identity in-flight limit.
    pub fn max_in_flight(&self) -> usize {
        self.max_in_flight
    }

    /// Tokens of every session that has been idle for `idle_release`; the
    /// sessions are removed from the pool and must be closed by the caller.
    pub fn reap_idle(&self) -> Vec<String> {
        let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots
            .values()
            .filter_map(|slot| slot.take_if_idle(self.idle_release))
            .collect()
    }

    /// The slot for `key`, created on first use. When the pool is full the
    /// least recently used identity is evicted; its session token, if any,
    /// is returned so the caller can close it at the gateway.
    pub fn slot(&self, key: IdentityKey) -> (Arc<Slot>, Option<String>) {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(slot) = slots.get(&key) {
            slot.touch();
            return (Arc::clone(slot), None);
        }
        let mut evicted = None;
        if slots.len() >= MAX_IDENTITIES {
            let oldest = slots
                .iter()
                .min_by_key(|(_, slot)| slot.last_touch())
                .map(|(key, _)| *key);
            if let Some(oldest) = oldest
                && let Some(slot) = slots.remove(&oldest)
            {
                evicted = slot.take_token();
            }
        }
        let slot = Arc::new(Slot::new(self.max_in_flight));
        slots.insert(key, Arc::clone(&slot));
        (slot, evicted)
    }

    /// Remove every pooled session and return the tokens (shutdown).
    pub fn drain(&self) -> Vec<String> {
        let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots
            .values()
            .filter_map(|slot| slot.take_token())
            .collect()
    }

    /// Number of identities currently tracked (diagnostics and tests).
    pub fn len(&self) -> usize {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_key_is_a_digest_not_the_credential() {
        let key = IdentityKey::of("Basic YWRtaW46czNjcmV0");
        let rendered = format!("{key:?}");
        assert!(rendered.starts_with("IdentityKey("));
        assert!(!rendered.contains("YWRtaW46"));
        assert_eq!(key, IdentityKey::of("Basic YWRtaW46czNjcmV0"));
        assert_ne!(key, IdentityKey::of("Basic b3RoZXI6b3RoZXI="));
    }

    #[test]
    fn session_expires_before_the_gateway_idle_timeout() {
        let mut session = PooledSession::new("tok".into(), Some(60));
        let now = Instant::now();
        assert!(session.usable_at(now));
        session.last_used = now - Duration::from_secs(46);
        assert!(
            !session.usable_at(now),
            "15 s margin before the 60 s idle timeout"
        );
        session.last_used = now - Duration::from_secs(44);
        assert!(session.usable_at(now));
    }

    #[test]
    fn session_is_replaced_after_max_age() {
        let mut session = PooledSession::new("tok".into(), None);
        session.created = Instant::now() - MAX_AGE;
        assert!(!session.usable_at(Instant::now()));
    }

    #[test]
    fn slot_hands_out_current_token_and_drops_expired_ones() {
        let slot = Slot::new(4);
        assert_eq!(slot.current(), (None, None));
        slot.install(PooledSession::new("tok-1".into(), Some(300)));
        assert_eq!(slot.current(), (Some("tok-1".into()), None));

        let mut expired = PooledSession::new("tok-2".into(), Some(300));
        expired.created = Instant::now() - MAX_AGE;
        slot.install(expired);
        assert_eq!(slot.current(), (None, Some("tok-2".into())));
        assert!(!slot.has_session());
    }

    #[test]
    fn invalidate_only_drops_the_matching_token() {
        let slot = Slot::new(4);
        slot.install(PooledSession::new("tok-1".into(), None));
        assert!(!slot.invalidate("tok-other"));
        assert!(slot.has_session());
        assert!(slot.invalidate("tok-1"));
        assert!(!slot.has_session());
    }

    #[test]
    fn suspension_expires() {
        let slot = Slot::new(4);
        assert!(!slot.is_suspended());
        slot.suspend(Duration::from_secs(30));
        assert!(slot.is_suspended());
        slot.suspend(Duration::ZERO);
        assert!(!slot.is_suspended());
    }

    #[test]
    fn pool_evicts_the_least_recently_used_identity() {
        let pool = SessionPool::new(2);
        let mut first_key = None;
        for i in 0..MAX_IDENTITIES {
            let key = IdentityKey::of(&format!("Basic {i}"));
            first_key.get_or_insert(key);
            let (slot, evicted) = pool.slot(key);
            assert!(evicted.is_none());
            slot.install(PooledSession::new(format!("tok-{i}"), None));
            // Keep timestamps strictly ordered on fast machines.
            std::thread::sleep(Duration::from_millis(1));
        }
        let (_slot, evicted) = pool.slot(IdentityKey::of("Basic extra"));
        assert_eq!(evicted.as_deref(), Some("tok-0"));
        assert_eq!(pool.len(), MAX_IDENTITIES);
    }

    #[test]
    fn reaper_takes_only_idle_sessions() {
        let pool = SessionPool::new(2).with_idle_release(Duration::from_millis(20));
        let (fresh, _) = pool.slot(IdentityKey::of("Basic fresh"));
        let (idle, _) = pool.slot(IdentityKey::of("Basic idle"));
        idle.install(PooledSession::new("tok-idle".into(), None));
        std::thread::sleep(Duration::from_millis(25));
        fresh.install(PooledSession::new("tok-fresh".into(), None));
        assert_eq!(pool.reap_idle(), vec!["tok-idle".to_string()]);
        assert!(!idle.has_session());
        assert!(fresh.has_session());
        assert!(pool.reap_idle().is_empty());
    }

    #[test]
    fn drain_returns_every_token_once() {
        let pool = SessionPool::new(2);
        let (a, _) = pool.slot(IdentityKey::of("Basic a"));
        a.install(PooledSession::new("tok-a".into(), None));
        let (b, _) = pool.slot(IdentityKey::of("Basic b"));
        b.install(PooledSession::new("tok-b".into(), None));
        let mut tokens = pool.drain();
        tokens.sort();
        assert_eq!(tokens, vec!["tok-a".to_string(), "tok-b".to_string()]);
        assert!(pool.drain().is_empty());
        let rendered = format!("{pool:?}");
        assert!(!rendered.contains("tok-"));
    }
}
