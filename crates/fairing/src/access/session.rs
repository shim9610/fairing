//! Issuer-owned, revocable multi-permission sessions. No token carries credentials.

use super::{session_rules::Observation, session_shared::Shared, Gate, Subject};
use crate::services::Waker;
use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// Process-local identity of one issued session; never reused, including after replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

/// One gate's base and additional temporary permission. Omitted gates are denied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionGrant {
    /// Operation name. Role names and their expansion belong to the authenticator.
    pub gate: Gate,
    /// Permit this gate for the session's lifetime.
    pub base: bool,
    /// Additional permission expires at this exact shell-clock deadline.
    pub temporary_until: Option<Instant>,
}

impl SessionGrant {
    /// A base grant for the same type used by a protected element.
    #[must_use]
    pub fn base_for<P: super::Permission>() -> Self {
        Self::base(Gate::borrowed(P::GATE))
    }
    /// A temporary grant for a typed requirement.
    #[must_use]
    pub fn temporary_for<P: super::Permission>(until: Instant) -> Self {
        Self::temporary(Gate::borrowed(P::GATE), until)
    }
    /// Reserve a typed requirement for later issuance.
    #[must_use]
    pub fn inactive_for<P: super::Permission>() -> Self {
        Self::inactive(Gate::borrowed(P::GATE))
    }
    /// A base permission.
    #[must_use]
    pub fn base(gate: impl Into<Gate>) -> Self {
        Self {
            gate: gate.into(),
            base: true,
            temporary_until: None,
        }
    }
    /// An additional expiring permission.
    #[must_use]
    pub fn temporary(gate: impl Into<Gate>, until: Instant) -> Self {
        Self {
            gate: gate.into(),
            base: false,
            temporary_until: Some(until),
        }
    }
    /// Reserve a gate in the session's immutable catalog without initially granting it.
    #[must_use]
    pub fn inactive(gate: impl Into<Gate>) -> Self {
        Self {
            gate: gate.into(),
            base: false,
            temporary_until: None,
        }
    }
}

/// A publication or issuance could not preserve the session contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// Duplicate gate in one grant set.
    DuplicateGate,
    /// A new gate needs a newly issued session/catalog.
    UnknownGate,
    /// Identity space was exhausted; identities never wrap.
    IdentityExhausted,
    /// A deadline exceeds the representable nanosecond range.
    DeadlineRange,
    /// The controller was revoked or exhausted its publication revisions.
    Ended,
}
impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for SessionError {}

struct SessionInner {
    id: SessionId,
    subject: Subject,
    origin: Instant,
    catalog: Vec<Gate>,
    shared: Shared,
}

/// Sole mutation capability. Keep this in the trusted authenticator, never in a screen.
///
/// ```
/// use fairing::access::{SessionController, SessionGrant, Subject};
/// let ctx = egui::Context::default();
/// let now = std::time::Instant::now();
/// let (mut owner, reader) = SessionController::issue(
///     Subject::default(), vec![SessionGrant::base("results.view")], None,
///     now, fairing::services::Waker::new(&ctx),
/// ).unwrap();
/// assert!(reader.snapshot(now).unwrap().allows(&"results.view".into()));
/// owner.revoke();
/// assert!(reader.snapshot(now).is_err());
/// ```
/// Dropping it terminally revokes every token clone. It is deliberately not Clone.
///
/// ```compile_fail
/// use fairing::access::SessionController;
/// fn duplicate(controller: &SessionController) -> SessionController { controller.clone() }
/// ```
pub struct SessionController {
    inner: Arc<SessionInner>,
    waker: Waker,
}

/// Read-only capability. Clones observe the same issuer-controlled state.
/// Private storage cannot be written or converted into a controller in safe client code.
/// This is an API ownership boundary, not isolation from malicious in-process host code.
///
/// ```compile_fail
/// use fairing::access::SessionToken;
/// fn revoke(token: &mut SessionToken) { token.revoke(); }
/// ```
///
/// ```compile_fail
/// use fairing::access::{SessionController, SessionToken};
/// fn promote(token: SessionToken) -> SessionController { token.into() }
/// ```
#[derive(Clone)]
pub struct SessionToken(Arc<SessionInner>);
impl PartialEq for SessionToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for SessionToken {}
impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionToken")
            .field("id", &self.0.id)
            .finish_non_exhaustive()
    }
}
impl fmt::Debug for SessionController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionController")
            .field("id", &self.inner.id)
            .finish_non_exhaustive()
    }
}

/// Why a read-only session observation cannot authorize work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionReadError {
    /// Issuer revocation or controller disposal. Terminal.
    Revoked,
    /// Session deadline reached. Terminal for the observing shell.
    Expired,
    /// An atomic publication overlaps the read. Deny work and retry on a later update.
    Updating,
}
impl fmt::Display for SessionReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for SessionReadError {}

/// Version-bound borrowed observation. A changed version makes further checks deny.
pub struct SessionSnapshot<'a> {
    token: &'a SessionToken,
    revision: u64,
    now: u64,
}
impl SessionSnapshot<'_> {
    /// Check the permission type used by a protected element.
    #[must_use]
    pub fn allows_permission<P: super::Permission>(&self) -> bool {
        self.allows_name(P::GATE)
    }
    /// Immutable identity metadata; level is a display/legacy-policy attribute, not a grant.
    #[must_use]
    pub fn subject(&self) -> &Subject {
        &self.token.0.subject
    }
    /// Publication revision of this observation.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Check one explicit permission, failing closed on publication or revocation.
    #[must_use]
    pub fn allows(&self, gate: &Gate) -> bool {
        self.allows_name(gate.as_str())
    }
    pub(crate) fn allows_name(&self, name: &str) -> bool {
        self.token
            .0
            .catalog
            .binary_search_by(|item| item.as_str().cmp(name))
            .is_ok_and(|index| self.token.0.shared.allows(index, self.now, self.revision))
    }
}

fn deadline(origin: Instant, at: Option<Instant>) -> Result<u64, SessionError> {
    at.map_or(Ok(u64::MAX), |at| {
        u64::try_from(at.saturating_duration_since(origin).as_nanos())
            .ok()
            .filter(|n| *n != u64::MAX)
            .ok_or(SessionError::DeadlineRange)
    })
}
fn normalize(mut grants: Vec<SessionGrant>) -> Result<Vec<SessionGrant>, SessionError> {
    grants.sort_by(|a, b| a.gate.as_str().cmp(b.gate.as_str()));
    if grants
        .windows(2)
        .any(|w| matches!(w, [a, b] if a.gate == b.gate))
    {
        return Err(SessionError::DuplicateGate);
    }
    Ok(grants)
}
fn times(origin: Instant, grants: &[SessionGrant]) -> Result<Vec<(u64, u64)>, SessionError> {
    grants
        .iter()
        .map(|grant| {
            Ok((
                if grant.base { u64::MAX } else { 0 },
                grant
                    .temporary_until
                    .map_or(Ok(0), |at| deadline(origin, Some(at)))?,
            ))
        })
        .collect()
}

impl SessionController {
    /// Issue one session after successful authentication, using the supplied shell-clock `now`.
    /// The initial grant list also declares every gate this controller may later update.
    /// Include inactive entries for permissions that can be granted later; adding a new gate
    /// or changing identity requires a new pair. No role ordering is imposed.
    ///
    /// # Errors
    /// Duplicate gates, unrepresentable deadlines or exhausted identity space.
    #[allow(deprecated, reason = "fetch_update supports the declared MSRV")]
    pub fn issue(
        subject: Subject,
        grants: Vec<SessionGrant>,
        expires_at: Option<Instant>,
        now: Instant,
        waker: Waker,
    ) -> Result<(Self, SessionToken), SessionError> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let grants = normalize(grants)?;
        let values = times(now, &grants)?;
        let until = deadline(now, expires_at)?;
        let id = NEXT
            .fetch_update(
                Ordering::SeqCst,
                Ordering::SeqCst,
                super::session_rules::next_identity,
            )
            .map(SessionId)
            .map_err(|_| SessionError::IdentityExhausted)?;
        let inner = Arc::new(SessionInner {
            id,
            subject,
            origin: now,
            catalog: grants.into_iter().map(|g| g.gate).collect(),
            shared: Shared::new(until, &values),
        });
        Ok((
            Self {
                inner: Arc::clone(&inner),
                waker,
            },
            SessionToken(inner),
        ))
    }
    /// The immutable session identity.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.inner.id
    }
    /// Atomically replace all permissions and the overall expiry. Omitted catalog gates lose
    /// both base and temporary rights. Validation completes before any publication.
    /// Do not publish a late canceled authentication attempt; return its request-bound token
    /// through Authenticator instead. This controller is trusted authorization authority.
    ///
    /// # Errors
    /// Duplicate/unknown gates, deadline overflow, revocation or revision exhaustion.
    pub fn replace_grants(
        &mut self,
        grants: Vec<SessionGrant>,
        expires_at: Option<Instant>,
        now: Instant,
    ) -> Result<(), SessionError> {
        let grants = normalize(grants)?;
        if grants.iter().any(|grant| {
            self.inner
                .catalog
                .binary_search_by(|g| g.as_str().cmp(grant.gate.as_str()))
                .is_err()
        }) {
            return Err(SessionError::UnknownGate);
        }
        let until = deadline(self.inner.origin, expires_at)?;
        let mut values = vec![(0, 0); self.inner.catalog.len()];
        let encoded = times(self.inner.origin, &grants)?;
        for (grant, value) in grants.iter().zip(encoded) {
            if let Ok(index) = self
                .inner
                .catalog
                .binary_search_by(|g| g.as_str().cmp(grant.gate.as_str()))
            {
                if let Some(slot) = values.get_mut(index) {
                    *slot = value;
                }
            }
        }
        let elapsed = u64::try_from(now.saturating_duration_since(self.inner.origin).as_nanos())
            .unwrap_or(u64::MAX);
        let ok = self.inner.shared.publish(elapsed, until, &values);
        self.waker.wake();
        if ok {
            Ok(())
        } else {
            Err(SessionError::Ended)
        }
    }
    /// Terminally revoke this session and wake its consumer. Repeated calls are harmless.
    pub fn revoke(&mut self) {
        self.inner.shared.revoke();
        self.waker.wake();
    }
}
impl Drop for SessionController {
    fn drop(&mut self) {
        self.inner.shared.revoke();
        let _ = crate::runtime::contain(|| self.waker.wake());
    }
}
impl SessionToken {
    /// Immutable identity of this pair.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.0.id
    }
    /// Read one publication without retrying, blocking, cloning a Subject or allocating.
    ///
    /// # Errors
    /// Revoked/expired sessions and an overlapping publication never authorize work.
    pub fn snapshot(&self, now: Instant) -> Result<SessionSnapshot<'_>, SessionReadError> {
        let elapsed = u64::try_from(now.saturating_duration_since(self.0.origin).as_nanos())
            .unwrap_or(u64::MAX);
        match self.0.shared.observe(elapsed) {
            Observation::Active(revision) => Ok(SessionSnapshot {
                token: self,
                revision,
                now: elapsed,
            }),
            Observation::Revoked => Err(SessionReadError::Revoked),
            Observation::Expired => Err(SessionReadError::Expired),
            Observation::Busy => Err(SessionReadError::Updating),
        }
    }
    pub(crate) fn next_deadline(&self, now: Instant) -> Option<Instant> {
        let elapsed = u64::try_from(now.saturating_duration_since(self.0.origin).as_nanos())
            .unwrap_or(u64::MAX);
        self.0
            .shared
            .next_deadline(elapsed)
            .and_then(|n| self.0.origin.checked_add(Duration::from_nanos(n)))
    }
}

#[cfg(all(test, feature = "settings"))]
mod tests {
    use super::*;
    use crate::access::{
        Access, AdminResult, AuthMethod, AuthOutcome, AuthRequestId, Authenticator, Credential,
        CredentialAdmin, CredentialEntry, CredentialOp, Secret,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    struct Admin {
        owner: Rc<RefCell<SessionController>>,
        calls: Rc<Cell<usize>>,
        lists: Rc<Cell<usize>>,
    }
    impl Authenticator for Admin {
        fn methods(&self) -> Vec<AuthMethod> {
            Vec::new()
        }
        fn submit(&mut self, _: AuthRequestId, _: Credential, _: Instant) -> AuthOutcome {
            AuthOutcome::Pending
        }
        fn admin(&mut self) -> Option<&mut dyn CredentialAdmin> {
            Some(self)
        }
    }
    impl CredentialAdmin for Admin {
        fn list(&self) -> Vec<CredentialEntry> {
            self.lists.set(self.lists.get() + 1);
            Vec::new()
        }
        fn set_secret(&mut self, _: &str, _: Credential) -> AdminResult {
            self.calls.set(self.calls.get() + 1);
            self.owner.borrow_mut().revoke();
            Ok(())
        }
        fn set_level(&mut self, _: &str, _: super::super::Level) -> AdminResult {
            self.calls.set(self.calls.get() + 1);
            Ok(())
        }
        fn add(&mut self, _: &str, _: super::super::Level, _: Credential) -> AdminResult {
            self.calls.set(self.calls.get() + 1);
            Ok(())
        }
        fn remove(&mut self, _: &str) -> AdminResult {
            self.calls.set(self.calls.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn credential_batches_check_each_current_operation_and_stop_after_revocation(
    ) -> crate::Result<()> {
        let ctx = egui::Context::default();
        let now = Instant::now();
        let (controller, token) = SessionController::issue(
            Subject::default(),
            vec![
                SessionGrant::base("settings.credentials"),
                SessionGrant::base("settings.credentials.set_secret"),
                SessionGrant::base("settings.credentials.remove"),
            ],
            None,
            now,
            Waker::new(&ctx),
        )
        .map_err(|e| crate::Error::Config(e.to_string()))?;
        let owner = Rc::new(RefCell::new(controller));
        let calls = Rc::new(Cell::new(0));
        let config = crate::config::AccessConfig {
            session_tokens: true,
            ..crate::config::AccessConfig::default()
        };
        let mut access = Access::from_config(&config)?;
        access.install_authenticator(Box::new(Admin {
            owner,
            calls: Rc::clone(&calls),
            lists: Rc::new(Cell::new(0)),
        }));
        assert!(access.install_session(token, now));
        access.apply_admin(
            vec![
                CredentialOp::SetSecret {
                    id: "one".into(),
                    secret: Secret::Pin("1234".into()),
                },
                CredentialOp::Remove { id: "two".into() },
            ],
            true,
        );
        assert_eq!(
            calls.get(),
            1,
            "credential operation after revocation reached the authenticator"
        );
        assert_eq!(
            access
                .admin_note()
                .map(|note| (note.done.len(), note.refused.len())),
            Some((1, 1))
        );
        Ok(())
    }

    #[test]
    fn opening_the_credentials_screen_does_not_grant_credential_write_permissions(
    ) -> crate::Result<()> {
        let ctx = egui::Context::default();
        let now = Instant::now();
        let (controller, token) = SessionController::issue(
            Subject::default(),
            vec![SessionGrant::base("settings.credentials")],
            None,
            now,
            Waker::new(&ctx),
        )
        .map_err(|e| crate::Error::Config(e.to_string()))?;
        let calls = Rc::new(Cell::new(0));
        let config = crate::config::AccessConfig {
            session_tokens: true,
            ..crate::config::AccessConfig::default()
        };
        let mut access = Access::from_config(&config)?;
        access.install_authenticator(Box::new(Admin {
            owner: Rc::new(RefCell::new(controller)),
            calls: Rc::clone(&calls),
            lists: Rc::new(Cell::new(0)),
        }));
        assert!(access.install_session(token, now));
        access.apply_admin(
            vec![
                CredentialOp::Remove { id: "one".into() },
                CredentialOp::SetLevel {
                    id: "one".into(),
                    level: super::super::Level(0),
                },
            ],
            true,
        );
        assert_eq!(
            calls.get(),
            0,
            "screen permission authorized an unissued credential operation"
        );
        Ok(())
    }
    #[test]
    fn credential_listing_requires_a_current_view_grant_and_refreshes_after_publication(
    ) -> crate::Result<()> {
        let ctx = egui::Context::default();
        let now = Instant::now();
        let (controller, token) = SessionController::issue(
            Subject::default(),
            vec![SessionGrant::inactive("settings.credentials")],
            None,
            now,
            Waker::new(&ctx),
        )
        .map_err(|e| crate::Error::Config(e.to_string()))?;
        let owner = Rc::new(RefCell::new(controller));
        let lists = Rc::new(Cell::new(0));
        let config = crate::config::AccessConfig {
            session_tokens: true,
            ..crate::config::AccessConfig::default()
        };
        let mut access = Access::from_config(&config)?;
        access.install_authenticator(Box::new(Admin {
            owner: Rc::clone(&owner),
            calls: Rc::new(Cell::new(0)),
            lists: Rc::clone(&lists),
        }));
        assert!(access.install_session(token, now));
        access.refresh_credentials();
        assert_eq!(
            lists.get(),
            0,
            "unissued view permission fetched account entries"
        );
        owner
            .borrow_mut()
            .replace_grants(vec![SessionGrant::base("settings.credentials")], None, now)
            .map_err(|e| crate::Error::Config(e.to_string()))?;
        let _ = access.sync_token(now);
        access.refresh_credentials();
        assert_eq!(lists.get(), 1);
        owner.borrow_mut().revoke();
        let _ = access.sync_token(now);
        access.refresh_credentials();
        assert_eq!(lists.get(), 1, "revoked session fetched account entries");
        Ok(())
    }
    #[test]
    fn credential_policy_revocation_is_rechecked_before_dispatch() -> crate::Result<()> {
        struct RevokePolicy(Rc<RefCell<SessionController>>);
        impl crate::access::AccessPolicy for RevokePolicy {
            fn allows(&self, _: &Subject, _: &Gate) -> bool {
                self.0.borrow_mut().revoke();
                true
            }
        }
        let ctx = egui::Context::default();
        let now = Instant::now();
        let (controller, token) = SessionController::issue(
            Subject::default(),
            vec![SessionGrant::base("settings.credentials.remove")],
            None,
            now,
            Waker::new(&ctx),
        )
        .map_err(|e| crate::Error::Config(e.to_string()))?;
        let owner = Rc::new(RefCell::new(controller));
        let calls = Rc::new(Cell::new(0));
        let config = crate::config::AccessConfig {
            session_tokens: true,
            ..crate::config::AccessConfig::default()
        };
        let mut access = Access::from_config(&config)?;
        access.install_authenticator(Box::new(Admin {
            owner: Rc::clone(&owner),
            calls: Rc::clone(&calls),
            lists: Rc::new(Cell::new(0)),
        }));
        assert!(access.install_session(token, now));
        access.set_policy(Box::new(RevokePolicy(owner)));
        access.apply_admin(vec![CredentialOp::Remove { id: "one".into() }], true);
        assert_eq!(
            calls.get(),
            0,
            "policy callback revoked account authority before dispatch"
        );
        Ok(())
    }
}
