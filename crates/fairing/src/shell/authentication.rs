//! Authentication for `Shell`.

use super::{
    AccessEvent, AppRef, AuthMethod, AuthOutcome, Authenticator, ChangeReason, Credential, Gate,
    Instant, LaunchAction, Lifecycle, Purpose, SessionChange, Shell, ShellEvent, OVERLAY_OPEN_GATE,
    RECENTS_GATE, SPLIT_GATE,
};

/// One authentication result is bound to one immutable continuation.
///
/// Prompt state is presentation; this value is the authorization transaction. It is never
/// exposed to an authenticator and cannot be replaced without issuing a new request identity.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum AuthorizationTarget {
    Prompt(Purpose),
    Routed {
        gate: Gate,
        then: Option<LaunchAction>,
    },
}

impl AuthorizationTarget {
    fn gate(&self) -> Gate {
        match self {
            Self::Prompt(Purpose::Unlock { gate, .. }) | Self::Routed { gate, .. } => gate.clone(),
            Self::Prompt(Purpose::Lock) => crate::access::prompt::LOCK_GATE,
        }
    }

    fn is_lock(&self) -> bool {
        matches!(self, Self::Prompt(Purpose::Lock))
    }

    fn into_action(self) -> Option<LaunchAction> {
        match self {
            Self::Prompt(Purpose::Unlock { then, .. }) | Self::Routed { then, .. } => then,
            Self::Prompt(Purpose::Lock) => None,
        }
    }

    fn references_declaration(&self, id: &str) -> bool {
        let action = match self {
            Self::Prompt(Purpose::Unlock { then, .. }) | Self::Routed { then, .. } => then,
            Self::Prompt(Purpose::Lock) => return false,
        };
        matches!(
            action,
            Some(LaunchAction::Open { id: target, .. } | LaunchAction::Run(target))
                if target == id
        )
    }
}

/// Split capability for one authentication attempt: the authenticator receives only `request`,
/// while the shell retains the exact target. Terminal handling consumes this pair.
pub(super) struct PendingAuthorization {
    request: crate::access::AuthRequestId,
    target: AuthorizationTarget,
}

impl PendingAuthorization {
    fn request(&self) -> crate::access::AuthRequestId {
        self.request
    }

    fn target_current(&self, prompt: &crate::access::prompt::Prompt) -> bool {
        match &self.target {
            AuthorizationTarget::Prompt(_) => prompt.request() == Some(self.request),
            AuthorizationTarget::Routed { .. } => !prompt.is_open(),
        }
    }
}

impl Shell {
    /// **Replace the authenticator** while the shell runs — the same as
    /// [`ShellBuilder::authenticator`](crate::ShellBuilder::authenticator) afterwards. It is given the waker first.
    ///
    /// A prompt up at the time — the lock screen included — opens again for the new one: its
    /// methods, and nothing the old one owed or locked. The old one is told to
    /// [`cancel`](Authenticator::cancel) first. Where the new one offers nothing, the prompt goes,
    /// as in `routing`.
    pub fn set_authenticator(&mut self, authenticator: impl Authenticator + 'static) {
        self.contain_fault(|shell| shell.set_authenticator_in(authenticator));
    }

    fn set_authenticator_in(&mut self, authenticator: impl Authenticator + 'static) {
        self.pending_auth = None;
        if self.access.uses_session_tokens() {
            let from = self.access.reset(self.mono);
            self.session_changed(SessionChange {
                from,
                to: self.access.initial_subject().level,
                reason: ChangeReason::SessionToken,
            });
        }
        if self.prompt.is_open() {
            // The one going out stops what it was doing: a wait ends with an answer or with
            // `cancel`, never by being dropped mid-check.
            let _ = self.prompt.take_wait();
            if let Some(old) = self.access.authenticator_mut() {
                old.cancel();
            }
        }
        let mut authenticator = Box::new(authenticator);
        authenticator.attach(self.handle.waker().clone());
        self.access.install_authenticator(authenticator);
        self.prompt.forget_lockout();
        let now = self.mono;
        let lock = self.prompt.is_lock_screen();
        let Some(purpose) = self.prompt.close(now) else {
            return;
        };
        let Some(methods) = self.prompt_methods() else {
            if lock {
                self.events
                    .push(ShellEvent::Access(AccessEvent::LockScreenToggled(false)));
            }
            return;
        };
        let (gate, hint) = match &purpose {
            Purpose::Unlock { gate, .. } => (gate.clone(), self.access.hint(gate)),
            Purpose::Lock => (crate::access::prompt::LOCK_GATE, None),
        };
        let target = AuthorizationTarget::Prompt(purpose.clone());
        let Some(request) = self.new_auth_request(target) else {
            return;
        };
        if let Some(auth) = self.access.authenticator_mut() {
            auth.begin(request, &gate, now);
        }
        self.prompt.open(purpose, methods, hint, now);
        self.prompt.bind_request(request);
    }
    /// After a session change: end the instances that can no longer pass their gate, and queue
    /// an `AccessChanged` on every instance left (notified at stage 14). A timeout, a lock, a logout,
    /// an unlock and the integrator's `set_subject` all take this path.
    pub(super) fn propagate_access_change(&mut self) {
        self.revalidate_access();
        self.workspace.queue_all(Lifecycle::AccessChanged);
    }
    /// Policies may depend on time or external state without a change of subject.
    /// Revoke closed gates before commands and drawing, without resetting forms in
    /// screens that are still allowed by sending `AccessChanged` on every frame.
    pub(super) fn revalidate_access(&mut self) {
        let was_home = self.workspace.is_home();
        let Self {
            workspace,
            access,
            theme,
            events,
            ..
        } = self;
        let closed =
            workspace.close_where(|instance| !access.allows(instance.gate()), &theme.motion);
        for (id, instance) in closed {
            events.push(ShellEvent::ScreenClosed { id, instance });
        }
        self.note_went_home(was_home);
        // Revocation hides content immediately; repeated animated closes would restart
        // a card shade's tween on every frame.
        let mut immediate = self.theme.motion;
        immediate.reduce = true;
        if !self.access.allows(&OVERLAY_OPEN_GATE) && !self.overlay.is_closed() {
            self.overlay.close(&immediate);
        }
        if self.workspace.is_overview_open() {
            let gate = if self.workspace.is_overview_picking() {
                SPLIT_GATE
            } else {
                RECENTS_GATE
            };
            if self.access.allows(&gate) {
                let beside = self.may_enter_split();
                self.workspace.set_overview_beside(beside);
            } else {
                self.workspace.close_overview(&immediate);
            }
        }
    }
    /// Route a failed gate to an unlock request: emit the event, and — when
    /// the shell can prompt — the prompt over the screen. A granted unlock runs `then` through
    /// [`Shell::launch`] again, gate and all.
    pub(super) fn request_unlock(&mut self, gate: Gate, then: Option<LaunchAction>) {
        self.events
            .push(ShellEvent::Access(AccessEvent::UnlockRequested {
                gate: gate.clone(),
                then: then.clone(),
            }));
        // The lock screen is already a prompt; whatever asked behind it waits for it — and is
        // asked again once the lock screen is gone.
        if self.prompt.is_lock_screen() {
            self.after_lock = Some((gate, then));
            return;
        }
        let Some(methods) = self.prompt_methods() else {
            if self.access.uses_session_tokens() {
                self.cancel_prompt();
                let target = AuthorizationTarget::Routed {
                    gate: gate.clone(),
                    then,
                };
                let Some(request) = self.new_auth_request(target) else {
                    return;
                };
                self.events
                    .push(ShellEvent::Access(AccessEvent::SessionRequested {
                        request,
                        gate,
                    }));
                return;
            }
            if self.access.can_prompt() {
                log::warn!(
                    "the authenticator offers no method - \"{}\" goes out as UnlockRequested only",
                    gate.as_str()
                );
            }
            return;
        };
        let hint = self.access.hint(&gate);
        let purpose = Purpose::Unlock { gate, then };
        let target = AuthorizationTarget::Prompt(purpose.clone());
        let same_attempt = self
            .pending_auth
            .as_ref()
            .is_some_and(|pending| pending.target == target)
            && self
                .pending_auth
                .as_ref()
                .is_some_and(|pending| pending.target_current(&self.prompt));
        if !same_attempt {
            if self.prompt.is_open() || self.pending_auth.is_some() {
                // A challenge is bound to its complete continuation, not only its gate.
                self.cancel_prompt();
            }
            // Whatever was being dragged ends here (A9).
            self.gestures.cancel_held_over();
            let now = self.mono;
            let auth_gate = target.gate();
            let Some(request) = self.new_auth_request(target) else {
                return;
            };
            if let Some(auth) = self.access.authenticator_mut() {
                auth.begin(request, &auth_gate, now);
            }
        }
        self.prompt.open(purpose, methods, hint, self.mono);
        if let Some(request) = self
            .pending_auth
            .as_ref()
            .map(PendingAuthorization::request)
        {
            self.prompt.bind_request(request);
        }
    }
    /// The methods the shell's prompt would offer — `None` where it draws none: not `prompt` mode,
    /// a single level in legacy mode, no authenticator, or no offered methods (which behaves as
    /// `routing` — the lock and the logout included).
    pub(super) fn prompt_methods(&mut self) -> Option<Vec<AuthMethod>> {
        if !self.access.can_prompt() {
            return None;
        }
        let methods = self
            .access
            .authenticator_mut()
            .map(|auth| auth.methods())
            .unwrap_or_default();
        (!methods.is_empty()).then_some(methods)
    }
    /// What asked behind the lock screen, asked again now that it is gone: run where the session
    /// passes its gate now, the prompt where it does not.
    pub(super) fn ask_after_lock(&mut self, app: &mut AppRef<'_>) {
        let Some((gate, then)) = self.after_lock.take() else {
            return;
        };
        if self.access.allows(&gate) {
            if let Some(action) = then {
                self.launch_in(action, app);
            }
        } else {
            self.request_unlock(gate, then);
        }
    }
    /// Deliver a request-bound result from a host-owned authentication UI in token mode.
    /// `SessionRequested` supplies the opaque request. This cannot install an unsolicited
    /// token; cancellation, subject change and replacement invalidate the request.
    pub fn accept_auth_response(&mut self, response: crate::access::AuthResponse) {
        self.contain_fault(|shell| shell.accept_auth_response_in(response, &mut None));
    }

    /// Deliver a host-owned authentication result with state available to its continuation.
    pub fn accept_auth_response_with(
        &mut self,
        response: crate::access::AuthResponse,
        app: &mut dyn std::any::Any,
    ) {
        self.contain_fault(|shell| shell.accept_auth_response_in(response, &mut Some(app)));
    }

    /// Cancel either a built-in prompt or a host-owned token authentication challenge.
    pub fn cancel_authentication(&mut self) {
        self.contain_fault(Self::cancel_prompt);
    }

    fn accept_auth_response_in(
        &mut self,
        response: crate::access::AuthResponse,
        app: &mut AppRef<'_>,
    ) {
        self.sync_session(app);
        self.observe_subject_change(app);
        let active = self
            .pending_auth
            .as_ref()
            .map(PendingAuthorization::request);
        let target_current = self
            .pending_auth
            .as_ref()
            .is_some_and(|pending| pending.target_current(&self.prompt));
        if response.request.completes(active, target_current) {
            self.finish_auth_outcome(response.outcome, app);
        }
    }

    /// Observe issuer updates independently of the bounded command queue and prompt state.
    pub(super) fn sync_session(&mut self, _app: &mut AppRef<'_>) {
        if let Some(change) = self.access.sync_token(self.mono) {
            self.session_changed(change);
        }
    }
    /// Stage 1: enforce old deadlines before commands and current input can change the session.
    pub(super) fn expire_access(&mut self, now: Instant) {
        if let Some(change) = self.access.tick(now) {
            self.session_changed(change);
        }
        // Due is asked first, so the quiet stretch is spent even while the lock screen is
        // already up — and the device is not told to lock a panel that is locked.
        if self.access.idle_lock_due(now) && !self.prompt.is_lock_screen() {
            self.lock_now(true);
        }
        self.revalidate_access();
    }
    /// Stage 4: current activity and the authenticator's late answers, after backend polling.
    pub(super) fn tick_access(&mut self, now: Instant, input_activity: bool, app: &mut AppRef<'_>) {
        self.sync_session(app);
        self.revalidate_access();
        #[cfg(feature = "settings")]
        self.access.refresh_credentials();
        if input_activity {
            self.access.record_activity(now);
        }
        if self.prompt.is_open() {
            let outcome = self
                .access
                .authenticator_mut()
                .and_then(|auth| auth.poll(now));
            if let Some(response) = outcome {
                self.accept_auth_response_in(response, app);
            }
        }
    }
    /// Announce a change of subject and pass it on: `SessionChanged`, the screens that no longer
    /// pass closed, and — on the way down — the shade and an unlock prompt closed too.
    pub(super) fn session_changed(&mut self, change: SessionChange) {
        self.events
            .push(ShellEvent::Access(AccessEvent::SessionChanged {
                from: change.from,
                to: change.to,
                reason: change.reason,
            }));
        if change.to < change.from
            || matches!(
                change.reason,
                ChangeReason::Timeout | ChangeReason::ElevationExpired | ChangeReason::SessionToken
            )
        {
            if !self.overlay.is_closed() {
                self.overlay.close(&self.theme.motion);
            }
            if (self.prompt.is_open() || self.pending_auth.is_some())
                && !self.prompt.is_lock_screen()
            {
                self.cancel_prompt();
            }
        }
        // The cards follow the session too: short of the gate they came up behind they go, and
        // their split buttons come and go with `workspace.split`.
        if self.workspace.is_overview_open() {
            let gate = if self.workspace.is_overview_picking() {
                SPLIT_GATE
            } else {
                RECENTS_GATE
            };
            if self.access.allows(&gate) {
                let beside = self.may_enter_split();
                self.workspace.set_overview_beside(beside);
            } else {
                self.workspace.close_overview(&self.theme.motion);
            }
        }
        self.propagate_access_change();
    }
    /// A new integrator subject invalidates the previous person's authentication even when
    /// their levels are equal. Direct `set_subject` is observed before polling too.
    pub(super) fn observe_subject_change(&mut self, app: &mut AppRef<'_>) {
        let Some(from) = self.access.take_subject_change() else {
            return;
        };
        let to = self.access.session().subject.level;
        let lock = self.prompt.is_lock_screen();
        if (self.prompt.is_open() || self.pending_auth.is_some()) && !lock {
            self.cancel_prompt();
        }
        self.session_changed(SessionChange {
            from,
            to,
            reason: ChangeReason::Integrator,
        });
        if lock {
            if to > self.access.initial_subject().level {
                self.cancel_prompt();
                self.events
                    .push(ShellEvent::Access(AccessEvent::LockScreenToggled(false)));
                self.ask_after_lock(app);
            } else if let Some(methods) = self.prompt_methods() {
                self.show_lock(methods);
            }
        }
    }
    /// Lock: [`LaunchAction::Lock`], or `idle_lock_secs` running out (`idle`).
    ///
    /// Emits [`ShellEvent::LockRequested`]. With a prompt to draw the
    /// shell carries it out: the session goes back to where it started and the lock screen
    /// comes up. Without one the idle lock still takes the session back — that is the timer's
    /// job — and the control is the integrator's.
    pub(super) fn lock_now(&mut self, idle: bool) {
        self.events.push(ShellEvent::LockRequested);
        let methods = self.prompt_methods();
        let can_prompt = methods.is_some();
        if !can_prompt && !idle && !self.access.uses_session_tokens() {
            return;
        }
        let now = self.mono;
        if self.access.away_from_start() {
            let from = self.access.reset(now);
            let to = self.access.initial_subject().level;
            self.session_changed(SessionChange {
                from,
                to,
                reason: ChangeReason::Lock,
            });
        }
        let Some(methods) = methods else {
            return;
        };
        self.show_lock(methods);
    }
    /// Start a fresh lock challenge; repeated explicit locks revoke the old check too.
    pub(super) fn show_lock(&mut self, methods: Vec<AuthMethod>) {
        let already_locked = self.prompt.is_lock_screen();
        // An unlock under way gives way to the lock, and so does the shade.
        if self.prompt.is_open() || self.pending_auth.is_some() {
            self.cancel_prompt();
        }
        if !self.overlay.is_closed() {
            self.overlay.close(&self.theme.motion);
        }
        self.gestures.cancel_held_over();
        let Some(request) = self.new_auth_request(AuthorizationTarget::Prompt(Purpose::Lock))
        else {
            return;
        };
        if let Some(auth) = self.access.authenticator_mut() {
            auth.begin(request, &crate::access::prompt::LOCK_GATE, self.mono);
        }
        // A new lock starts with nothing waiting behind it.
        self.after_lock = None;
        self.prompt.open(Purpose::Lock, methods, None, self.mono);
        self.prompt.bind_request(request);
        if !already_locked {
            self.events
                .push(ShellEvent::Access(AccessEvent::LockScreenToggled(true)));
        }
    }
    /// Handle logout and emit [`ShellEvent::LogoutRequested`]. Token mode detaches
    /// local authority; legacy mode resets the subject when a prompt is available.
    pub(super) fn end_session(&mut self) {
        self.events.push(ShellEvent::LogoutRequested);
        if self.access.uses_session_tokens() {
            self.pending_auth = None;
            self.after_lock = None;
            let from = self.access.reset(self.mono);
            self.cancel_prompt();
            self.session_changed(SessionChange {
                from,
                to: self.access.initial_subject().level,
                reason: ChangeReason::Logout,
            });
            return;
        }
        let Some(methods) = self.prompt_methods() else {
            return;
        };
        self.after_lock = None;
        let locked = self.prompt.is_lock_screen();
        if locked {
            self.show_lock(methods);
        } else if self.prompt.is_open() {
            self.cancel_prompt();
        }
        if !self.access.away_from_start() {
            return;
        }
        let from = self.access.reset(self.mono);
        let to = self.access.initial_subject().level;
        self.session_changed(SessionChange {
            from,
            to,
            reason: ChangeReason::Logout,
        });
    }
    /// Transfer the entered credential to the authenticator. A new attempt
    /// cancels the pending request before submitting its replacement. The prompt
    /// can accept new input while the previous authentication result is pending.
    pub(super) fn submit_credential(&mut self, credential: Credential, app: &mut AppRef<'_>) {
        let now = self.mono;
        let Some(target) = self
            .pending_auth
            .as_ref()
            .map(|pending| pending.target.clone())
        else {
            return;
        };
        if !self
            .pending_auth
            .as_ref()
            .is_some_and(|pending| pending.target_current(&self.prompt))
        {
            self.cancel_prompt();
            return;
        }
        let Some(request) = self.new_auth_request(target) else {
            return;
        };
        self.prompt.bind_request(request);
        let called_off = self.prompt.take_wait();
        let outcome = self.access.authenticator_mut().map(|auth| {
            if called_off {
                auth.cancel();
            }
            auth.submit(request, credential, now)
        });
        if let Some(outcome) = outcome {
            self.finish_auth_outcome(outcome, app);
        }
    }
    pub(super) fn finish_auth_outcome(&mut self, outcome: AuthOutcome, app: &mut AppRef<'_>) {
        if !self
            .pending_auth
            .as_ref()
            .is_some_and(|pending| pending.target_current(&self.prompt))
        {
            self.cancel_prompt();
            return;
        }
        let target = if matches!(outcome, AuthOutcome::Pending) {
            self.pending_auth
                .as_ref()
                .map(|pending| pending.target.clone())
        } else {
            self.pending_auth.take().map(|pending| pending.target)
        };
        let Some(target) = target else {
            return;
        };
        self.apply_outcome_for(target, outcome, app);
        // A terminal refusal consumes its token too. External readers need a fresh
        // challenge to accept another credential without closing/reopening the prompt.
        if self.pending_auth.is_none() && !self.faulted {
            if let Some(purpose) = self.prompt.purpose() {
                let target = AuthorizationTarget::Prompt(purpose);
                let gate = target.gate();
                let Some(request) = self.new_auth_request(target) else {
                    return;
                };
                self.prompt.bind_request(request);
                if let Some(auth) = self.access.authenticator_mut() {
                    auth.begin(request, &gate, self.mono);
                }
            }
        }
    }
    /// Apply the authenticator's answer. Legacy grants use `unlock_mode`; session grants
    /// install the issuer's token. Successful continuations re-enter the launch gate.
    fn apply_outcome_for(
        &mut self,
        target: AuthorizationTarget,
        outcome: AuthOutcome,
        app: &mut AppRef<'_>,
    ) {
        let now = self.mono;
        let gate = target.gate();
        match outcome {
            AuthOutcome::Session(token) => self.apply_session_outcome(token, target, now, app),
            AuthOutcome::Granted(subject) => {
                let lock = target.is_lock();
                let level = subject.level;
                // A level the table does not have would pass every gate; and on the unlock
                // prompt a grant at or below the session's own level opens nothing — it must not
                // lower the session (closing what it has open) either. The lock screen takes the
                // session's own level (the start, where the lock left it), never one below it.
                let unknown = self.access.table().get(level).is_none();
                if unknown {
                    log::warn!(
                        "access: the authenticator granted level {}, which [access] levels does not have - refused",
                        level.0
                    );
                }
                let held = self.access.session().subject.level;
                if self.access.uses_session_tokens()
                    || !crate::access::rules::grant_allowed(!unknown, lock, held.0, level.0)
                {
                    // Not a way in, so not a right answer either: the authenticator counts it,
                    // and a lockout it reaches is applied as one.
                    let counted = self
                        .access
                        .authenticator_mut()
                        .and_then(|auth| auth.refused(now));
                    match counted {
                        Some(locked @ AuthOutcome::Locked { .. }) => {
                            self.apply_outcome_for(target, locked, app);
                        }
                        Some(AuthOutcome::Denied { message }) => {
                            self.apply_outcome_for(target, AuthOutcome::Denied { message }, app);
                        }
                        _ => {
                            self.events
                                .push(ShellEvent::Access(AccessEvent::Denied { gate }));
                            let text = crate::access::prompt::labels::NOT_ENOUGH.to_owned();
                            self.prompt.denied(text, now);
                        }
                    }
                    return;
                }
                if matches!(target, AuthorizationTarget::Prompt(_)) {
                    self.prompt.close(now);
                }
                let mode = self.access.unlock_mode();
                let (subject_id, level) = (subject.id.clone(), subject.level);
                let from = self.access.unlock(subject, now);
                self.events.push(ShellEvent::Access(AccessEvent::Unlocked {
                    gate,
                    subject_id,
                    level,
                    mode,
                }));
                self.session_changed(SessionChange {
                    from,
                    to: level,
                    reason: ChangeReason::Unlock,
                });
                if lock {
                    self.events
                        .push(ShellEvent::Access(AccessEvent::LockScreenToggled(false)));
                    self.ask_after_lock(app);
                }
                if let Some(action) = target.into_action() {
                    self.launch_in(action, app);
                }
            }
            AuthOutcome::Denied { message } => {
                self.events
                    .push(ShellEvent::Access(AccessEvent::Denied { gate }));
                self.prompt.denied(message, now);
            }
            AuthOutcome::Locked { until, message } => {
                self.events
                    .push(ShellEvent::Access(AccessEvent::Locked { gate, until }));
                self.prompt.locked(until, message);
            }
            AuthOutcome::Pending => {
                if matches!(target, AuthorizationTarget::Prompt(_)) {
                    self.prompt.wait();
                }
            }
        }
    }
    fn apply_session_outcome(
        &mut self,
        token: crate::access::SessionToken,
        target: AuthorizationTarget,
        now: Instant,
        app: &mut AppRef<'_>,
    ) {
        let gate = target.gate();
        let lock = target.is_lock();
        let from = self.access.session().subject.level;
        if !self.access.install_session(token, now) {
            self.events
                .push(ShellEvent::Access(AccessEvent::Denied { gate }));
            if matches!(target, AuthorizationTarget::Prompt(_)) {
                self.prompt
                    .denied(crate::access::prompt::labels::NOT_ENOUGH.to_owned(), now);
            }
            return;
        }
        if matches!(target, AuthorizationTarget::Prompt(_)) {
            self.prompt.close(now);
        }
        let subject = &self.access.session().subject;
        let level = subject.level;
        self.events.push(ShellEvent::Access(AccessEvent::Unlocked {
            gate,
            subject_id: subject.id.clone(),
            level,
            mode: crate::access::UnlockMode::Switch,
        }));
        self.session_changed(SessionChange {
            from,
            to: level,
            reason: ChangeReason::Unlock,
        });
        if lock {
            self.events
                .push(ShellEvent::Access(AccessEvent::LockScreenToggled(false)));
            self.ask_after_lock(app);
        }
        if let Some(action) = target.into_action() {
            self.launch_in(action, app);
        }
    }
    /// Issue a fresh request identity bound to one exact target, quarantining the shell if
    /// identities are exhausted.
    fn new_auth_request(
        &mut self,
        target: AuthorizationTarget,
    ) -> Option<crate::access::AuthRequestId> {
        let request = crate::access::AuthRequestId::issue();
        self.pending_auth = request.map(|request| PendingAuthorization { request, target });
        if request.is_none() {
            // Exhaustion never reuses an identifier, including across shell replacements.
            self.quarantine();
        }
        request
    }
    /// Close the prompt without an answer and notify the authenticator, if installed.
    pub(super) fn cancel_prompt(&mut self) {
        self.pending_auth = None;
        if let Some(auth) = self.access.authenticator_mut() {
            auth.cancel();
        }
        self.prompt.close(self.mono);
    }

    /// A declaration replacement changes the meaning of an id-based continuation. Its previous
    /// authentication cannot authorize the replacement body, even when its gate is unchanged.
    pub(super) fn cancel_authorization_for_decl(&mut self, id: &str) {
        if self
            .pending_auth
            .as_ref()
            .is_some_and(|pending| pending.target.references_declaration(id))
        {
            self.cancel_prompt();
        }
    }
    /// The lock screen's Continue: leave as the starting subject, which the lock already made the
    /// session.
    pub(super) fn continue_from_lock(&mut self, app: &mut AppRef<'_>) {
        if !self.prompt.is_lock_screen() || !self.access.lock_screen_continue() {
            return;
        }
        self.cancel_prompt();
        self.events
            .push(ShellEvent::Access(AccessEvent::LockScreenToggled(false)));
        self.ask_after_lock(app);
    }
}
