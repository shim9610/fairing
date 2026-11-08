//! Navigation for `Shell`.

use super::{
    back_route, labels, ActionDecl, AppRef, BackAction, Command, CxParts, CxRequest, DeclKind,
    Gate, Instance, InstanceId, LaunchAction, LaunchMode, OverviewMode, PaneInfo, ScreenDecl,
    Shell, ShellEvent, SplitBlocker, Task, OVERLAY_OPEN_GATE, RECENTS_GATE, SPLIT_GATE,
};

impl Shell {
    /// Dispatch a launch action using its current gate decision. A failed gate does not open and
    /// raises an `UnlockRequested`. For `Set` / `Toggle` the key name is the gate ("writing a
    /// settings value").
    pub fn launch(&mut self, action: LaunchAction) {
        self.contain_fault(|shell| shell.launch_in(action, &mut None));
    }
    /// [`Shell::launch`] **with the app's state in hand**.
    ///
    /// Inside a frame the shell already has it, so a tap that runs an `action(id, ..)` closure gives
    /// it `cx.app()`. Between frames it does not — the app is holding its own state, not the shell —
    /// so a direct `shell.launch(LaunchAction::run("calibrate"))` would run that closure with
    /// `cx.app() == None`. This is the same call with the state lent for its length:
    ///
    /// ```no_run
    /// # struct Console;
    /// # struct App { shell: fairing::Shell, console: Console }
    /// # impl App {
    /// fn calibrate(&mut self) {
    ///     self.shell
    ///         .launch_with(fairing::LaunchAction::run("calibrate"), &mut self.console);
    /// }
    /// # }
    /// ```
    ///
    /// Only `LaunchAction::Run` actually runs your code on the spot; `Open` puts a screen up and its
    /// `ui` runs in the next frame, which has the state anyway.
    pub fn launch_with(&mut self, action: LaunchAction, app: &mut dyn std::any::Any) {
        self.contain_fault(|shell| shell.launch_in(action, &mut Some(app)));
    }
    /// The frame's own launch — it carries the app's state, so an `action(id, ..)` closure reached
    /// by a tap sees `cx.app()`. The public [`Shell::launch`] is this with nothing lent,
    /// because between frames the shell is not holding the app's state; use
    /// [`ShellHandle::launch`](crate::ShellHandle::launch) instead and it lands inside the next
    /// frame with the state in hand.
    pub(super) fn launch_in(&mut self, action: LaunchAction, app: &mut AppRef<'_>) {
        self.sync_session(app);
        if self.faulted {
            return;
        }
        self.observe_subject_change(app);
        match action {
            LaunchAction::Open { ref id, .. } => {
                let id = id.clone();
                self.open_screen(&id, action);
            }
            LaunchAction::Run(id) => self.run_action(&id, app),
            LaunchAction::Set(key, value) => self.set_setting(key, value),
            LaunchAction::Toggle(key) => self.toggle_setting(key),
            LaunchAction::OpenOverlay => {
                let gate = OVERLAY_OPEN_GATE;
                if self.access.allows(&gate) {
                    self.overlay.open(&self.theme.motion);
                } else {
                    self.request_unlock(gate, Some(LaunchAction::OpenOverlay));
                }
            }
            // **Always reported**, and in `prompt` mode with an authenticator **also
            // carried out**: there the shell owns the session's way up — its prompt — so it
            // owns the way down too. Elsewhere what locking and logging out mean is the device's.
            // The arms are written out rather than left to a catch-all so that a new variant
            // cannot be swallowed here again the way these three once were.
            LaunchAction::Lock => self.lock_now(false),
            LaunchAction::Logout => self.end_session(),
            LaunchAction::OpenOverview => self.open_overview(),
            LaunchAction::ToggleSplit => self.toggle_split(),
        }
    }
    /// The overview control (the nav bar's `"recents"`, `LaunchAction::OpenOverview`).
    /// Always reported as [`ShellEvent::OverviewRequested`], and the shell's own
    /// overview comes up where `[workspace] overview` is on. Pressed with it up, it goes back.
    pub(super) fn open_overview(&mut self) {
        self.events.push(ShellEvent::OverviewRequested);
        if !self.config.workspace.overview {
            return;
        }
        let tokens = self.theme.motion;
        if self.workspace.is_overview_open() {
            self.workspace.close_overview(&tokens);
        } else if self.access.allows(&RECENTS_GATE) {
            // The cards come up over everything but the shell's own chrome — the shade goes up
            // first, as it does for home.
            if !self.overlay.is_closed() {
                self.overlay.close(&tokens);
            }
            let beside = self.may_enter_split();
            self.workspace
                .open_overview(OverviewMode::Recents { beside }, &tokens);
        } else {
            self.request_unlock(RECENTS_GATE, Some(LaunchAction::OpenOverview));
        }
    }
    /// Whether a split may come up — `[workspace] split` on and the session through
    /// [`SPLIT_GATE`] — or is up already.
    pub(super) fn may_enter_split(&self) -> bool {
        self.config.workspace.split
            && (self.workspace.is_split() || self.access.allows(&SPLIT_GATE))
    }
    /// The split control (the split tile, the nav bar's `"split"` item,
    /// `LaunchAction::ToggleSplit`). Split, back to one pane keeping the focused one; one pane in
    /// the Tasks view, the task used last beside it. Always reported as
    /// [`ShellEvent::SplitRequested`], and carried out where `[workspace] split` is on.
    pub(super) fn toggle_split(&mut self) {
        self.events.push(ShellEvent::SplitRequested);
        if !self.config.workspace.split {
            return;
        }
        let tokens = self.theme.motion;
        // Pressed again over the cards, it goes back.
        if self.workspace.is_overview_open() {
            self.workspace.close_overview(&tokens);
            return;
        }
        if self.workspace.is_split() {
            let keep = self.workspace.focused_pane();
            self.workspace.unsplit(keep, &tokens);
            return;
        }
        if !self.access.allows(&SPLIT_GATE) {
            self.request_unlock(SPLIT_GATE, Some(LaunchAction::ToggleSplit));
            return;
        }
        // Where no split can come up, it says why rather than doing nothing.
        if let Some(blocker) = self.workspace.split_blocker() {
            let label = match blocker {
                SplitBlocker::NothingOnShow => labels::SPLIT_NOTHING_ON_SHOW,
                SplitBlocker::Refused => labels::SPLIT_REFUSED,
                SplitBlocker::NothingFits => labels::SPLIT_NOTHING_FITS,
            };
            let text = self.strings.get(label).to_owned();
            self.toast(text);
            return;
        }
        if !self.overlay.is_closed() {
            self.overlay.close(&tokens);
        }
        // With the shell's overview, the cards say what can go beside — only what fits is offered;
        // without it, the task used last of those that fit.
        if self.config.workspace.overview {
            self.workspace.open_overview(OverviewMode::Picker, &tokens);
        } else if let Some(task) = self.workspace.most_recent_other_task() {
            let _ = self.workspace.show_in_other_pane(task, &tokens);
        }
    }
    /// Back (the priority: the prompt → close the overlay → close the OSK → `on_back` → pop →
    /// ignored at home). Back cancels an unlock prompt and does nothing on the lock screen — the
    /// way out of that is the prompt.
    pub fn back(&mut self) {
        self.contain_fault(|shell| shell.back_in(&mut None));
    }
    /// [`Shell::back`] **with the app's state in hand** — for a screen whose
    /// [`Screen::on_back`](crate::Screen::on_back) reads it. See [`Shell::launch_with`].
    pub fn back_with(&mut self, app: &mut dyn std::any::Any) {
        self.contain_fault(|shell| shell.back_in(&mut Some(app)));
    }
    /// The frame's own back — see [`Shell::launch_in`] for why there are two.
    pub(super) fn back_in(&mut self, app: &mut AppRef<'_>) {
        let group = back_route::classify(back_route::BackState {
            faulted: self.faulted,
            prompt: self.prompt.is_open(),
            lock: self.prompt.is_lock_screen(),
            info: self.desktop.info_open(),
            overlay: !self.overlay.is_closed(),
            keyboard: self.osk.is_shown(),
            overview: self.workspace.is_overview_open(),
            home: self.workspace.is_home(),
        });
        match back_route::dispatch(group) {
            back_route::BackTarget::Ignore => return,
            back_route::BackTarget::CancelPrompt => {
                self.cancel_prompt();
                return;
            }
            back_route::BackTarget::CloseInfo => {
                self.desktop.close_info(&self.theme.motion);
                return;
            }
            back_route::BackTarget::CloseOverlay => {
                self.overlay.close(&self.theme.motion);
                return;
            }
            back_route::BackTarget::HideKeyboard => {
                self.osk.hide();
                return;
            }
            back_route::BackTarget::CloseOverview => {
                self.workspace.close_overview(&self.theme.motion);
                return;
            }
            back_route::BackTarget::Workspace => {}
        }
        let consumed = self
            .with_parts(app, |workspace, registry, parts| {
                let rect = workspace.focused_pane_rect();
                let is_split = workspace.is_split();
                let task = workspace.active_task_mut()?;
                let top = task.top_mut()?;
                let pane = PaneInfo {
                    rect,
                    outer: rect,
                    is_split,
                    is_focused: true,
                    inset_bottom: 0.0,
                    instance: top.id(),
                };
                let mut cx = parts.cx_in(pane, None, Some(registry));
                Some(top.on_back(&mut cx) == BackAction::Consumed)
            })
            .unwrap_or(false);
        if consumed {
            return;
        }
        let closing = self
            .workspace
            .focused()
            .map(|i| (i.decl_id().to_owned(), i.id()));
        // A root pop (ending the task → home) hands the A2 close's origin as the root icon Rect
        // the desktop remembers **right now** — the last Rect the Workspace remembered can be stale
        // after an M2 page swipe.
        let at_root = self
            .workspace
            .active_task()
            .is_some_and(|task| task.len() <= 1);
        let popped = if at_root {
            let icon_rect = self
                .workspace
                .active_task()
                .and_then(Task::root_id)
                .and_then(|id| self.desktop.icon_rect(id));
            self.workspace
                .close_active_task_with(icon_rect, &self.theme.motion)
        } else {
            self.workspace.pop(&self.theme.motion)
        };
        if popped {
            if let Some((id, instance)) = closing {
                self.events.push(ShellEvent::ScreenClosed { id, instance });
            }
            if self.workspace.is_home() {
                self.events.push(ShellEvent::WentHome);
            }
        }
    }
    /// Home. The task stays alive. An open shade is closed with it — the A1 interruption rule,
    /// "home/back while opening → `Settling → Closed`" (the shade closes even when pressed on the home
    /// screen).
    pub fn home(&mut self) {
        self.desktop.close_info(&self.theme.motion);
        if !self.overlay.is_closed() {
            self.overlay.close(&self.theme.motion);
        }
        if self.workspace.is_home() {
            // The overview over the desktop goes back down — home is where it already is.
            if self.workspace.is_overview_open() {
                self.workspace.close_overview(&self.theme.motion);
            }
            return;
        }
        let icon_rect = self
            .workspace
            .active_task()
            .and_then(|t| t.root_id().map(str::to_owned))
            .and_then(|id| self.desktop.icon_rect(&id));
        self.workspace.go_home(icon_rect, &self.theme.motion);
        self.events.push(ShellEvent::WentHome);
    }
    /// Stage 2 handles at most the builder's command budget and leaves remaining work queued.
    /// `true` requests another update. This is a count limit, not a callback-duration guarantee.
    pub(super) fn drain_commands(&mut self, app: &mut AppRef<'_>) -> bool {
        for _ in 0..self.commands_per_update {
            // `Inbox` has no blocking call to reach for, so this loop cannot become one.
            let Ok(command) = self.rx.try_recv() else {
                return false;
            };
            self.sync_session(app);
            match command {
                Command::Launch(action) => self.launch_in(action, app),
                Command::CloseScreen(id) => self.close_instance(id, None, app),
                Command::Back => self.back_in(app),
                Command::Home => self.home(),
                Command::SetSubject(subject) => {
                    self.access.set_subject_at(subject, self.mono);
                    self.observe_subject_change(app);
                }
                // The same as `LaunchAction::Logout`.
                Command::Logout => self.end_session(),
                Command::SetBadge { id, badge } => {
                    self.desktop.set_badge(&id, badge.as_ref());
                }
                Command::SetSetting { key, value } => self.set_setting(key, value),
                Command::RequestUnlock { gate, then } => self.request_unlock(gate, then),
                Command::Notify(notification) => self.notify(notification),
                Command::Toast(toast) => self.toasts.push(toast),
                Command::DismissNotification(id) => {
                    self.dismiss_notification(id);
                }
                Command::ToggleOverlay => self.toggle_overlay(app),
                Command::SetMotion(tokens) => self.set_motion(tokens),
            }
        }
        // Include producers constructing reserved payloads. Successful sends wake the
        // shell too; no extra command is removed from bounded storage merely to peek.
        self.rx.stats().outstanding != 0
    }
    pub(super) fn gate_of(&self, id: &str) -> Gate {
        self.registry
            .screen(id)
            .map(ScreenDecl::gate_name)
            .or_else(|| {
                self.registry
                    .actions()
                    .iter()
                    .find(|a| a.id() == id)
                    .map(ActionDecl::gate_name)
            })
            .unwrap_or_else(|| Gate::from(id))
    }
    /// **Where a long press on a tile leads** ([`QuickTile::long_press`](crate::overlay::QuickTile::long_press)).
    /// `None` where there is none.
    ///
    /// What the declaration attached comes first, and without it it falls back to **the built-in tile's
    /// default pairing** — press and hold the Wi-Fi tile and the Wi-Fi settings open. It is used only
    /// where the paired screen is actually registered (turned off with `[settings] only`, or with the
    /// feature taken out, nothing happening is better).
    #[cfg(feature = "overlay")]
    pub(super) fn tile_long_press(&self, id: &str) -> Option<LaunchAction> {
        let declared = self
            .registry
            .tiles()
            .iter()
            .find(|t| t.id() == id)
            .and_then(|t| t.tile().long_press.clone());
        declared.or_else(|| {
            let screen = crate::overlay::tiles::builtin_long_press(id)?;
            self.registry.screen(screen)?;
            Some(LaunchAction::open(screen))
        })
    }
    /// With no overlay there are no tiles — this branch is never reached in the first place.
    #[cfg(not(feature = "overlay"))]
    #[expect(clippy::unused_self, reason = "the same shape as feature-on")]
    pub(super) fn tile_long_press(&self, _id: &str) -> Option<LaunchAction> {
        None
    }
    pub(super) fn action_for(&self, id: &str) -> Option<LaunchAction> {
        match self.registry.kind(id)? {
            DeclKind::Screen => Some(LaunchAction::open(id)),
            DeclKind::Action => Some(LaunchAction::run(id)),
            DeclKind::StatusItem | DeclKind::NavItem | DeclKind::Tile => None,
        }
    }
    /// Run one declaration by id (an icon tap). `Open` for a screen, `Run` for an action.
    pub(super) fn launch_decl(&mut self, id: &str, app: &mut AppRef<'_>) {
        match self.action_for(id) {
            Some(action) => self.launch_in(action, app),
            None => log::warn!("launch: no declaration `{id}`"),
        }
    }
    pub(super) fn open_screen(&mut self, id: &str, action: LaunchAction) {
        // "open in the other pane" — where the shell's split is on and the session may
        // enter one; otherwise it opens where a split that cannot hold would, in the same pane.
        let in_other_pane = matches!(
            action,
            LaunchAction::Open {
                in_other_pane: true,
                ..
            }
        ) && self.may_enter_split();
        let Some(decl) = self.registry.screen(id) else {
            log::warn!("launch: screen `{id}` was never declared");
            return;
        };
        let gate = decl.gate_name();
        let mode = decl.launch_mode();
        if !self.access.allows(&gate) {
            self.request_unlock(gate, Some(action));
            return;
        }
        let tokens = self.theme.motion;
        // What opens is on show: the overview, if it is up, gets out of the way first.
        self.workspace.clear_overview();
        let icon_rect = self.desktop.icon_rect(id);
        if mode == LaunchMode::Single {
            // Single: ① inside the current task → clear-top, ② in another task → bring that task
            // forward, ③ neither → afresh (a new task at home, otherwise a push).
            if !self.workspace.is_home() {
                // The instances cleared away (everything above the lowest `id`) receive a `ScreenClosed`.
                let cleared: Vec<(String, InstanceId)> = self
                    .workspace
                    .active_task()
                    .and_then(|task| {
                        let pos = task.iter().position(|i| i.decl_id() == id)?;
                        Some(
                            task.iter()
                                .skip(pos + 1)
                                .map(|i| (i.decl_id().to_owned(), i.id()))
                                .collect(),
                        )
                    })
                    .unwrap_or_default();
                if self.workspace.clear_top_to(id) {
                    for (id, instance) in cleared {
                        self.events.push(ShellEvent::ScreenClosed { id, instance });
                    }
                    return;
                }
            }
            if let Some(index) = self.workspace.find_task(id) {
                // In a task of its own: beside the focused pane where asked, else brought forward.
                if in_other_pane && self.workspace.show_in_other_pane(index, &tokens) {
                    return;
                }
                if self.workspace.resume_task(index, icon_rect, &tokens) {
                    return;
                }
            }
        }
        let instance_id = self.workspace.alloc_id();
        let Some(decl) = self.registry.screen_mut(id) else {
            return;
        };
        let screen = decl.spawn();
        let instance = Instance::new(instance_id, decl, screen);
        if in_other_pane && self.workspace.can_split(instance.split_support()) {
            self.workspace.open_in_other_pane(instance, &tokens);
        } else {
            self.workspace.open(instance, icon_rect, &tokens);
        }
        // The entries are listed again whenever their screen opens — something outside the
        // shell may have changed them since.
        #[cfg(feature = "settings")]
        if id == crate::access::CREDENTIALS_SCREEN {
            self.access.mark_credentials_dirty();
        }
        self.events.push(ShellEvent::ScreenOpened {
            id: id.to_owned(),
            instance: instance_id,
        });
    }
    pub(super) fn run_action(&mut self, id: &str, app: &mut AppRef<'_>) {
        let Some(gate) = self.registry.action_mut(id).map(|a| a.gate_name()) else {
            log::warn!("launch: action `{id}` was never declared");
            return;
        };
        if !self.access.allows(&gate) {
            self.request_unlock(gate, Some(LaunchAction::run(id)));
            return;
        }
        let pane = PaneInfo {
            rect: self.layout.content,
            outer: self.layout.content,
            is_split: false,
            is_focused: true,
            inset_bottom: 0.0,
            instance: InstanceId::NONE,
        };
        let now = self.mono;
        let frame = self.frame_no;
        let Self {
            registry,
            handle,
            access,
            services,
            settings,
            strings,
            theme,
            icons,
            widget_painters,
            animations,
            requests,
            hidden_remaining,
            ..
        } = self;
        let mut parts = CxParts {
            shell: handle,
            access,
            services,
            settings,
            app: app.as_deref_mut(),
            strings,
            theme,
            icons,
            animations,
            widget_painters,
            requests,
            hidden: hidden_remaining,
            now,
            frame,
        };
        if let Some(action) = registry.action_mut(id) {
            let mut cx = parts.cx(pane, None);
            action.run(&mut cx);
        }
    }
    pub(super) fn close_instance(
        &mut self,
        id: InstanceId,
        value: Option<crate::screen::ScreenValue>,
        app: &mut AppRef<'_>,
    ) {
        // The screen closing, and the one under it in its own task — the parent its result goes
        // to. Split, that is not always the focused pane's top: a screen in the other pane can
        // finish on its own (an answer arriving), without a press there.
        let closing = self.workspace.tasks().iter().find_map(|t| {
            let mut below = None;
            for instance in t.iter() {
                if instance.id() == id {
                    return Some((instance.decl_id().to_owned(), below));
                }
                below = Some(instance.id());
            }
            None
        });
        let Some((decl_id, parent)) = closing else {
            return;
        };
        let was_home = self.workspace.is_home();
        if !self.workspace.close_instance(id, &self.theme.motion) {
            return;
        }
        self.events.push(ShellEvent::ScreenClosed {
            id: decl_id.clone(),
            instance: id,
        });
        if let (Some(value), Some(parent)) = (value, parent) {
            self.with_parts(app, |workspace, registry, parts| {
                let pane = workspace.pane_holding(parent);
                let rect = pane
                    .and_then(|p| workspace.pane_rect(p))
                    .unwrap_or_else(|| workspace.focused_pane_rect());
                let is_split = workspace.is_split();
                let is_focused = pane.is_some_and(|p| p == workspace.focused_pane());
                let parent = workspace.instance_mut(parent)?;
                let pane = PaneInfo {
                    rect,
                    outer: rect,
                    is_split,
                    is_focused,
                    inset_bottom: 0.0,
                    instance: parent.id(),
                };
                let mut cx = parts.cx_in(pane, None, Some(registry));
                parent.on_result(&decl_id, value, &mut cx);
                Some(())
            });
        }
        self.note_went_home(was_home);
    }
    pub(super) fn process_requests(&mut self, app: &mut AppRef<'_>) {
        let requests = std::mem::take(&mut self.requests);
        for request in requests {
            self.sync_session(app);
            match request {
                CxRequest::Open {
                    id,
                    in_other_pane,
                    from,
                } => {
                    // A screen opens from its own pane: split, the pane it is in takes the focus
                    // first, so "this pane" and "the other pane" are its own and the other.
                    let user = self.workspace.focused_pane();
                    let from_pane = self.workspace.pane_holding(from);
                    let on_its_own = from_pane.is_some_and(|p| p != user);
                    // A back swipe the user is dragging in their pane carries on through it.
                    let held = if on_its_own {
                        self.workspace.hold_back_gesture()
                    } else {
                        None
                    };
                    self.workspace.focus_pane_of(from);
                    self.launch_in(LaunchAction::Open { id, in_other_pane }, app);
                    // No press put the focus there (a press would have): the screen opened on
                    // its own, and what it opened landed in its pane — the user keeps theirs.
                    if on_its_own && Some(self.workspace.focused_pane()) == from_pane {
                        self.workspace.return_focus(user);
                    }
                    if let Some(held) = held {
                        self.workspace.resume_back_gesture(held);
                    }
                }
                CxRequest::Finish { instance, value } => self.close_instance(instance, value, app),
                CxRequest::SetChrome { instance, policy } => {
                    if let Some(i) = self.workspace.instance_mut(instance) {
                        i.set_chrome(policy);
                    }
                }
                CxRequest::Launch(action) => self.launch_in(action, app),
                CxRequest::Knock { id } => self.knock(&id),
                CxRequest::SetSetting { key, value } => self.set_setting(key, value),
                CxRequest::Power(request) => {
                    // **It is not carried out.** The integrator tidies up and then calls `commit_power`.
                    self.events.push(ShellEvent::PowerRequest(request));
                }
                #[cfg(feature = "settings")]
                CxRequest::Credentials(ops) => {
                    // The screen was drawn behind its gate, but the session can have dropped
                    // since — an earlier request this frame may have locked the panel.
                    let gate = self
                        .registry
                        .screen(crate::access::CREDENTIALS_SCREEN)
                        .map_or_else(
                            || Gate::borrowed(crate::access::CREDENTIALS_SCREEN),
                            ScreenDecl::gate_name,
                        );
                    let allowed = self.access.allows(&gate);
                    self.access.apply_admin(ops, allowed);
                }
                #[cfg(feature = "settings")]
                CxRequest::ListCredentials => self.access.mark_credentials_dirty(),
                #[cfg(feature = "settings")]
                CxRequest::WifiNetworkLongPressed { ssid, known } => {
                    // **It does not forget.** Where a hold leads is the integrator's.
                    self.events
                        .push(ShellEvent::WifiNetworkLongPressed { ssid, known });
                }
                #[cfg(feature = "settings")]
                CxRequest::WifiNetworkTapped {
                    ssid,
                    secured,
                    known,
                } => {
                    // **It does not connect.** The password, where the profile lives and whether a
                    // saved network should reconnect on a tap are the device's policy.
                    self.events.push(ShellEvent::WifiNetworkTapped {
                        ssid,
                        secured,
                        known,
                    });
                }
            }
        }
    }
}
