//! `Cx` — the handle a screen uses to ask the shell for something. Most screens only use `ui`.

use super::{ChromePolicy, LaunchAction, Lifecycle, Registry, Screen, ScreenValue};
use crate::access::{Access, Gate, LevelTable, Session};
use crate::i18n::Strings;
use crate::icons::IconSet;
use crate::motion::{AnimationStore, Tween};
use crate::services::Services;
use crate::settings::SettingsView;
use crate::shell::ShellHandle;
use crate::theme::Theme;
use crate::workspace::InstanceId;
use std::collections::BTreeMap;
use std::time::Instant;

/// Information about the pane a screen is drawn in.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PaneInfo {
    /// The content rect (all of what the screen has to fill).
    pub rect: egui::Rect,
    /// The pane before the screen inset — what the pane's backing fills. [`Self::rect`] is this
    /// shrunk by the inset; a piece of chrome drawn inside a screen, such as
    /// [`layout::Rail`](crate::layout::Rail)'s arm, paints out to this edge so no strip of
    /// backing shows between it and the glass. The same as `rect` where nothing was inset.
    pub outer: egui::Rect,
    /// Whether two panes are up — `true` in both.
    pub is_split: bool,
    /// Whether this is the focused pane: the only one, or the one of two last pressed.
    pub is_focused: bool,
    /// The height the OSK is covering (M2). What a scroll area should stay clear of.
    pub inset_bottom: f32,
    /// This screen instance's id. [`InstanceId::NONE`] where there is no instance, as in actions and status items.
    pub instance: InstanceId,
}

/// A request `Cx` leaves for the shell. Handled in stage 14 of the frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CxRequest {
    /// `cx.open(id)` — push onto the same pane (the shell checks the gate).
    Open {
        /// The declaration id.
        id: String,
        /// Open in the other pane.
        in_other_pane: bool,
        /// The instance that asked.
        from: InstanceId,
    },
    /// `cx.finish()` / `cx.finish_with(v)`.
    Finish {
        /// Itself.
        instance: InstanceId,
        /// The value for the parent's `on_result`.
        value: Option<ScreenValue>,
    },
    /// `cx.set_chrome(p)`.
    SetChrome {
        /// The target.
        instance: InstanceId,
        /// The new policy.
        policy: ChromePolicy,
    },
    /// An arbitrary launch action.
    Launch(LaunchAction),
    /// `cx.knock(id)` — one knock on a hidden entry point.
    Knock {
        /// The entry point id.
        id: String,
    },
    /// `settings.credentials` asks for the management calls of one Apply. The
    /// authenticator is the shell's, so the screen leaves the calls here and the shell makes them
    /// in stage 14.
    #[cfg(feature = "settings")]
    Credentials(Vec<crate::access::CredentialOp>),
    /// `settings.credentials` came into view: list the entries again before the next frame —
    /// something outside the shell may have changed them, and on the settings home's right it
    /// is drawn without ever being opened.
    #[cfg(feature = "settings")]
    ListCredentials,
    /// `cx.request_power(req)` — **request** a power action. The shell does not
    /// carry it out; it only emits
    /// [`ShellEvent::PowerRequest`](crate::ShellEvent::PowerRequest).
    Power(crate::services::PowerRequest),
    /// A network row on `settings.wifi` was tapped. Like [`CxRequest::Power`] the shell does not
    /// carry it out — it only emits
    /// [`ShellEvent::WifiNetworkTapped`](crate::ShellEvent::WifiNetworkTapped).
    #[cfg(feature = "settings")]
    WifiNetworkTapped {
        /// The SSID.
        ssid: String,
        /// Secured.
        secured: bool,
        /// A saved profile exists.
        known: bool,
    },
    /// A network row on `settings.wifi` was held. Reported, not acted on — see
    /// [`CxRequest::WifiNetworkTapped`].
    #[cfg(feature = "settings")]
    WifiNetworkLongPressed {
        /// The SSID.
        ssid: String,
        /// A saved profile exists.
        known: bool,
    },
    /// `cx.set_setting(key, value)` — change a setting.
    SetSetting {
        /// The key.
        key: crate::settings::SettingKey,
        /// The new value.
        value: crate::settings::SettingValue,
    },
}

/// The pieces of the shell needed to build a [`Cx`]. `Shell::frame` splits its own fields into
/// one of these, and the workspace, chrome and desktop take it and build a `Cx` with
/// [`CxParts::cx`].
pub(crate) struct CxParts<'a> {
    /// The shell handle.
    pub(crate) shell: &'a ShellHandle,
    /// Access control (the session and the decisions).
    pub(crate) access: &'a Access,
    /// The backends.
    pub(crate) services: &'a mut Services,
    /// Reading settings.
    pub(crate) settings: &'a SettingsView,
    /// **The app's own state, lent to the shell for this frame**. `None` on the
    /// paths where the app handed nothing over — see [`Cx::app`].
    pub(crate) app: Option<&'a mut dyn std::any::Any>,
    /// Strings.
    pub(crate) strings: &'a Strings,
    /// The theme.
    pub(crate) theme: &'a Theme,
    /// The icon set (cache and custom icons).
    pub(crate) icons: &'a mut IconSet,
    /// The per-id animation store.
    pub(crate) animations: &'a mut AnimationStore,
    /// The integrator's widget painters, lent to every widget call.
    pub(crate) widget_painters: &'a mut fairing_widgets::widgets::WidgetPainters,
    /// The request queue.
    pub(crate) requests: &'a mut Vec<CxRequest>,
    /// Hidden entry point id → knocks remaining. Read by a screen drawing "3 to go".
    pub(crate) hidden: &'a BTreeMap<String, u8>,
    /// The frame's time.
    pub(crate) now: Instant,
    /// The frame number.
    pub(crate) frame: u64,
}

impl CxParts<'_> {
    /// Build a `Cx` **with no registry**, for the places that are not drawing a screen — an action,
    /// a status item, a nav item, a quick-settings tile. [`Cx::draw_screen`] answers `false` there.
    pub(crate) fn cx(&mut self, pane: PaneInfo, event: Option<Lifecycle>) -> Cx<'_> {
        self.cx_in(pane, event, None)
    }

    /// Build a `Cx` **that can reach the registry** — for a screen's own call, so it can embed
    /// another declaration's screen with [`Cx::draw_screen`].
    pub(crate) fn cx_in<'a>(
        &'a mut self,
        pane: PaneInfo,
        event: Option<Lifecycle>,
        registry: Option<&'a mut Registry>,
    ) -> Cx<'a> {
        Cx {
            registry,
            shell: self.shell,
            session: self.access.session(),
            services: &mut *self.services,
            settings: self.settings,
            app: self.app.as_deref_mut(),
            strings: self.strings,
            theme: self.theme,
            icons: &mut *self.icons,
            pane,
            now: self.now,
            event,
            access: self.access,
            animations: &mut *self.animations,
            widget_painters: &mut *self.widget_painters,
            requests: &mut *self.requests,
            hidden: self.hidden,
            frame: self.frame,
        }
    }
}

/// The [`PaneInfo`] an **embedded** screen gets ([`Cx::draw_screen`]).
///
/// Its rect is the space it was really given rather than the host's pane, its instance is
/// [`InstanceId::NONE`] because it has none, and the OSK inset is only however much of the keyboard
/// reaches into its own rect — a guest drawn in the top half of the pane is not covered at all.
fn guest_pane(host: &PaneInfo, rect: egui::Rect) -> PaneInfo {
    // How far the OSK reaches up from the bottom of the host's pane, minus whatever of that lies
    // below the guest's own bottom edge.
    let covered_from = host.rect.max.y - host.inset_bottom;
    PaneInfo {
        rect,
        outer: rect,
        is_split: host.is_split,
        is_focused: host.is_focused,
        inset_bottom: (rect.max.y - covered_from).clamp(0.0, rect.height()),
        instance: InstanceId::NONE,
    }
}

/// The handle to reach for when you need it.
pub struct Cx<'a> {
    /// notify / toast / launch / close … from any thread.
    pub shell: &'a ShellHandle,
    /// The current subject and level (read-only).
    pub session: &'a Session,
    /// Reading backend snapshots, and commands.
    pub services: &'a mut Services,
    /// Reading settings.
    pub settings: &'a SettingsView,
    /// The app's own state for this frame — read it with [`Cx::app`] / [`Cx::app_mut`].
    app: Option<&'a mut dyn std::any::Any>,
    /// i18n.
    pub strings: &'a Strings,
    /// The theme.
    pub theme: &'a Theme,
    /// Icons (drawing built-in and custom ones). Not in the first design of `Cx`, but status items and icon buttons needed it, so it was added in M1.
    pub icons: &'a mut IconSet,
    /// rect, `is_split`, `is_focused`, `inset_bottom`, instance id.
    pub pane: PaneInfo,
    /// The frame's time.
    pub now: Instant,
    /// The lifecycle event that arrived since the last `ui` call. This is how a closure screen receives them.
    pub event: Option<Lifecycle>,
    access: &'a Access,
    /// The declaration store, present only while a **screen** is being called. It is what
    /// [`Cx::draw_screen`] and [`Cx::has_screen`] read; everywhere else it is `None`.
    registry: Option<&'a mut Registry>,
    animations: &'a mut AnimationStore,
    widget_painters: &'a mut fairing_widgets::widgets::WidgetPainters,
    requests: &'a mut Vec<CxRequest>,
    /// Hidden entry point id → knocks remaining.
    hidden: &'a BTreeMap<String, u8>,
    frame: u64,
}

impl Cx<'_> {
    /// **The app's own state** — whatever it handed to
    /// [`Shell::frame_with`](crate::Shell::frame_with) this frame.
    ///
    /// The app owns its state and owns the shell; the shell borrows the state for the length of a
    /// frame and lends it on to the screens. Nothing is captured, nothing is shared, and there is no
    /// cell:
    ///
    /// ```no_run
    /// # struct Console;
    /// # impl Console { fn temperature(&self) -> f32 { 0.0 } }
    /// struct App {
    ///     shell: fairing::Shell,
    ///     console: Console,          // ← the app owns both
    /// }
    ///
    /// impl App {
    ///     fn frame(&mut self, ui: &mut egui::Ui) {
    ///         self.shell.frame_with(ui, &mut self.console);
    ///     }
    /// }
    ///
    /// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
    /// // …and inside any screen:
    /// if let Some(console) = cx.app::<Console>() {
    ///     ui.label(format!("{:.1} °C", console.temperature()));
    /// }
    /// # }
    /// ```
    ///
    /// `None` where the frame was driven by plain [`Shell::frame`](crate::Shell::frame), or where
    /// `T` is not the type that was handed over. One value goes over per frame — an app with several
    /// things to share hands over one `struct` holding them.
    ///
    /// It is also `None` in code the app runs **between** frames, where the shell is not holding
    /// anything: an `action(id, ..)` closure reached by a direct
    /// [`Shell::launch`](crate::Shell::launch), an `on_back` reached by
    /// [`Shell::back`](crate::Shell::back), a hidden entry point's action reached by
    /// [`Shell::knock`](crate::Shell::knock). Each has a pair —
    /// [`launch_with`](crate::Shell::launch_with), [`back_with`](crate::Shell::back_with),
    /// [`knock_with`](crate::Shell::knock_with) — that lends the state for the length of the call.
    #[must_use]
    pub fn app<T: std::any::Any>(&self) -> Option<&T> {
        self.app.as_ref().and_then(|app| app.downcast_ref())
    }

    /// The same, mutably.
    ///
    /// It borrows the whole `Cx`, so it cannot be held while you also reach for `cx.theme` or call a
    /// `layout::` helper. Where you need both at once, use [`Cx::with_app`].
    pub fn app_mut<T: std::any::Any>(&mut self) -> Option<&mut T> {
        self.app.as_mut().and_then(|app| app.downcast_mut())
    }

    /// **The app's state and the `Cx` at the same time**.
    ///
    /// `cx.app_mut::<T>()` borrows all of `cx`, so this does not compile:
    ///
    /// ```compile_fail
    /// # struct Order { total: u32 }
    /// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
    /// let Some(order) = cx.app_mut::<Order>() else { return };  // cx is borrowed…
    /// fairing::layout::title(ui, cx, "Cart");                   // …and wanted again here
    /// order.total += 1;                                         // …while the first is still live
    /// # }
    /// ```
    ///
    /// So the reference is taken **out** of the `Cx` for the length of the call and put straight
    /// back, the same loan [`Cx::draw_screen`] uses for screens. Inside, both are yours:
    ///
    /// ```no_run
    /// # struct Order { total: u32 }
    /// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
    /// cx.with_app::<Order, _>(|order, cx| {
    ///     fairing::layout::title(ui, cx, "Cart");
    ///     order.total += 1;
    /// });
    /// # }
    /// ```
    ///
    /// `None` where nothing was handed over, where `T` is the wrong type, or where it is **already
    /// out on loan** — a `with_app` inside a `with_app` gets `None` rather than a second `&mut`, so
    /// a cycle stops instead of aliasing. A panic inside puts the reference back before carrying on.
    pub fn with_app<T: std::any::Any, R>(
        &mut self,
        f: impl FnOnce(&mut T, &mut Self) -> R,
    ) -> Option<R> {
        let app = self.app.take()?;
        let Some(value) = app.downcast_mut::<T>() else {
            self.app = Some(app);
            return None;
        };
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(value, self)));
        self.app = Some(app);
        match out {
            Ok(value) => Some(value),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// **Whether a screen is declared under this id.**
    ///
    /// Registration only — it says nothing about the gate. For a list of entries pointing at other
    /// screens (the built-in `settings.home` is one) [`Cx::screen_allowed`] is the one to filter on:
    /// a row for a screen this session may not open is a row that does nothing.
    ///
    /// Answers `false` outside a screen's own call, and for a declaration that is not a screen (an
    /// action, a tile).
    #[must_use]
    pub fn has_screen(&self, id: &str) -> bool {
        self.registry
            .as_ref()
            .is_some_and(|registry| registry.has_screen(id))
    }

    /// **Whether this session may open this screen** — it is registered *and* its own gate passes.
    ///
    /// Not the same as `allows(id)`. A declaration's gate defaults to its id but `.gate(..)` can
    /// name another, and only the declaration knows which
    /// ([`ScreenDecl::gate_name`](crate::screen::ScreenDecl::gate_name)); filtering a list on the id
    /// alone lists screens the session cannot reach.
    #[must_use]
    pub fn screen_allowed(&self, id: &str) -> bool {
        self.registry.as_ref().is_some_and(|registry| {
            registry
                .screen(id)
                .is_some_and(|decl| self.access.allows(&decl.gate_name()))
        })
    }

    /// **Whether [`Cx::draw_screen`] would draw this one here** — everything it needs, checked
    /// before the press rather than after: registered, its gate passes, it is a **resident**
    /// declaration, and its screen is not already out on loan.
    ///
    /// A factory declaration (`screen_with`) owns no screen between opens, so it can only be
    /// opened, never embedded — a list offering both has to know which a row is before it is
    /// pressed.
    #[must_use]
    pub fn can_draw_screen(&self, id: &str) -> bool {
        self.screen_allowed(id)
            && self
                .registry
                .as_ref()
                .is_some_and(|registry| registry.has_resident(id))
    }

    /// **Draw another declaration's screen here, inside this one**.
    ///
    /// The whole of a two-pane settings layout is this: the list on the left, and on the right the
    /// screen the selected row points at — whichever declaration is registered under that id,
    /// including one the integrator wrote or one that replaced a built-in. Answers `true` where it
    /// drew.
    ///
    /// ```no_run
    /// # fn detail(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>, id: &str) {
    /// if !cx.draw_screen(ui, id) {
    ///     ui.label("nothing registered under that id");
    /// }
    /// # }
    /// ```
    ///
    /// # What the guest gets
    ///
    /// A `Cx` of its own in every way that would otherwise be a lie:
    ///
    /// - **`cx.pane.rect` is the space it was actually given** (`ui`'s), not the host's pane. A
    ///   scroll built on the host's height would run past the bottom of the column and the overrun
    ///   could never be scrolled into view.
    /// - **`cx.pane.instance` is [`InstanceId::NONE`]** — it is drawn, not opened, so there is no
    ///   instance to close. Without this a guest calling `cx.finish()` would close **the host**.
    /// - **`cx.event` is `None`.** The host's lifecycle is the host's; the guest was not created,
    ///   resumed or resized, and a resident screen keys its initialisation off `cx.event`.
    /// - `cx.pane.inset_bottom` is however much of the OSK reaches into the guest's own rect.
    ///
    /// It **draws**; it does not open. There is no instance, so no lifecycle events, no chrome of
    /// its own, no back handling and no place on the workspace stack. To open it properly, use
    /// [`Cx::open`].
    ///
    /// The loan is per **call**, not per frame, so a screen that is open as an instance *and*
    /// embedded somewhere else is drawn twice in the one frame. There is still only one of it, so
    /// both show the same state — but the widget ids inside it are used twice and egui says so.
    /// Embed a screen that is not also on the stack.
    ///
    /// # When it answers `false`
    ///
    /// Exactly when [`Cx::can_draw_screen`] is `false`, which is the one to ask before drawing a row
    /// that leads here:
    ///
    /// - Nothing is registered under `id`, or what is is not a screen.
    /// - **The screen's gate does not pass for this session.** The host cannot wave a guest past its
    ///   own gate — a host that lists entries would otherwise be a way round access control.
    /// - The declaration is a **factory** one (`screen_with`): it owns no screen between opens.
    /// - The screen is **already out on loan** — it is the one drawing, or one further up the
    ///   nesting. A screen that asks for itself gets `false` rather than a panic, so a cycle stops
    ///   instead of recursing.
    /// - This `Cx` was not built for a screen (an action, a status item, a tile body).
    pub fn draw_screen(&mut self, ui: &mut egui::Ui, id: &str) -> bool {
        if !self.screen_allowed(id) {
            return false;
        }
        let host_pane = self.pane;
        let host_event = self.event.take();
        self.pane = guest_pane(&host_pane, ui.available_rect_before_wrap());
        let drew = self
            .with_resident(id, |screen, cx| screen.ui(ui, cx))
            .is_some();
        self.pane = host_pane;
        self.event = host_event;
        drew
    }

    /// Take a resident screen out of its declaration for the length of one call, then put it back.
    ///
    /// Taking rather than borrowing is what makes nesting safe — see
    /// [`Registry::take_resident`](crate::screen::Registry::take_resident). `None` where there was
    /// nothing to take.
    ///
    /// The call is wrapped in [`catch_unwind`](std::panic::catch_unwind) **only so that the screen
    /// goes home before the panic carries on**. Nothing is swallowed: the payload is resumed
    /// immediately. Without it, an integrator who wraps the frame in `catch_unwind` — which the old
    /// shared cell made pointless anyway, since it panicked of its own accord — would find that one
    /// panic emptied the declaration for good and the screen silently never drew again.
    pub(crate) fn with_resident<R>(
        &mut self,
        id: &str,
        f: impl FnOnce(&mut dyn Screen, &mut Self) -> R,
    ) -> Option<R> {
        let mut screen = self.registry.as_mut()?.take_resident(id)?;
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut *screen, self)));
        // Where the declaration went in the meantime (`shell.remove(id)` from inside its own `ui`)
        // this drops the screen, which is right — there is nothing left for it to belong to.
        if let Some(registry) = self.registry.as_mut() {
            registry.put_resident(id, screen);
        }
        match out {
            Ok(value) => Some(value),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Push onto the same pane (the shell checks the gate).
    pub fn open(&mut self, id: &str) {
        self.requests.push(CxRequest::Open {
            id: id.to_owned(),
            in_other_pane: false,
            from: self.pane.instance,
        });
    }

    /// **Open in the other pane**. With one pane, a split: this screen keeps its side and
    /// `id` slides in beside it. Split already, a push onto the other pane. The focus goes with
    /// it. Where no split can hold — a [`SplitSupport`](crate::SplitSupport) minimum,
    /// `allow_split`, `[workspace] split`, the `workspace.split` gate — it opens in this pane, as
    /// [`Cx::open`] would. The shell checks `id`'s gate either way.
    pub fn open_in_other_pane(&mut self, id: &str) {
        self.requests.push(CxRequest::Open {
            id: id.to_owned(),
            in_other_pane: true,
            from: self.pane.instance,
        });
    }

    /// Pop yourself.
    pub fn finish(&mut self) {
        self.requests.push(CxRequest::Finish {
            instance: self.pane.instance,
            value: None,
        });
    }

    /// Pop, then the parent's `on_result`.
    pub fn finish_with(&mut self, value: ScreenValue) {
        self.requests.push(CxRequest::Finish {
            instance: self.pane.instance,
            value: Some(value),
        });
    }

    /// Change the chrome at runtime.
    pub fn set_chrome(&mut self, policy: ChromePolicy) {
        self.requests.push(CxRequest::SetChrome {
            instance: self.pane.instance,
            policy,
        });
    }

    /// **One knock on a hidden entry point**. The screen calls it from its own
    /// secret spot — the shell does not know where you pressed.
    ///
    /// Android's "tap the build number seven times" has this shape. Register the entry point
    /// in advance with [`Shell::add_hidden_entry`](crate::Shell::add_hidden_entry); when it
    /// completes, the shell checks the gate and then runs the action.
    ///
    /// Read the remaining count with [`Cx::knock_remaining`] — for showing "3 to go", the way
    /// Android does. An unknown id does nothing.
    ///
    /// On the trigger side this arrives as
    /// [`KnockInput::pokes`](crate::access::KnockInput::pokes) `= 1`. Triggers that watch
    /// coordinates or keys advance on their own without this call.
    ///
    /// ```no_run
    /// # fn ui(cx: &mut fairing::Cx<'_>, ui: &mut egui::Ui) {
    /// if ui.label("Model ACME-7").clicked() {
    ///     cx.knock("service");
    /// }
    /// # }
    /// ```
    pub fn knock(&mut self, id: &str) {
        self.requests.push(CxRequest::Knock { id: id.to_owned() });
    }

    /// How many more knocks a hidden entry point needs, once it is down to the entry's
    /// [`hint_from`](crate::access::HiddenEntry::hint_from). `None` for an unknown id, for an
    /// entry with no `hint_from` or not yet down to it, or for a **trigger that cannot be
    /// counted** ([`KnockTrigger::remaining`](crate::access::KnockTrigger::remaining) is `None`).
    ///
    /// **This frame's [`Cx::knock`] is not in it yet** — requests are handled at the end of the
    /// frame.
    #[must_use]
    pub fn knock_remaining(&self, id: &str) -> Option<u8> {
        self.hidden.get(id).copied()
    }

    /// **Change a setting**. The built-in settings screens and integrator screens
    /// take the same path.
    ///
    /// It applies at the end of the frame and emits
    /// [`ShellEvent::SettingChanged`](crate::ShellEvent::SettingChanged) — **persisting it is the
    /// integrator's.** The crate does not write files (where, how often and with what
    /// transactional guarantees differs per device), and
    /// [`Shell::restore_settings`](crate::Shell::restore_settings) puts stored values back.
    ///
    /// A built-in key also reaches its backend (`display.brightness` the display's
    /// `set_brightness`); a key of your own is only recorded, and acting on it is
    /// yours.
    ///
    /// **This frame's [`Cx::settings`] still holds the old value** — requests are handled at the
    /// end of the frame. Draw from your own local state and use this to announce the change.
    ///
    /// ```
    /// # fn ui(cx: &mut fairing::Cx<'_>, on: bool) {
    /// use fairing::settings::{keys, SettingValue};
    ///
    /// cx.set_setting(keys::UI_SILENT, SettingValue::Bool(on));
    /// # }
    /// ```
    pub fn set_setting(
        &mut self,
        key: impl Into<crate::settings::SettingKey>,
        value: crate::settings::SettingValue,
    ) {
        self.requests.push(CxRequest::SetSetting {
            key: key.into(),
            value,
        });
    }

    /// **Request a power action**: restart, shutdown, suspend.
    ///
    /// # The shell does not carry it out
    ///
    /// It only emits [`ShellEvent::PowerRequest`](crate::ShellEvent::PowerRequest); the action
    /// happens when the integrator calls
    /// [`Shell::commit_power`](crate::Shell::commit_power). **Not calling it is the veto** —
    /// machines routinely have to close a valve and flush logs before power goes, and the crate
    /// cannot do that cleanup for them.
    ///
    /// ```
    /// # fn handle(shell: &mut fairing::Shell) {
    /// use fairing::ShellEvent;
    ///
    /// for event in shell.poll_events() {
    ///     if let ShellEvent::PowerRequest(request) = event {
    ///         // …clean up…
    ///         let _ = shell.commit_power(request);   // not calling it is the refusal
    ///     }
    /// }
    /// # }
    /// ```
    pub fn request_power(&mut self, request: crate::services::PowerRequest) {
        self.requests.push(CxRequest::Power(request));
    }

    /// Report that a Wi-Fi network row was tapped, and **do nothing else** — the shell turns it
    /// into [`ShellEvent::WifiNetworkTapped`](crate::ShellEvent::WifiNetworkTapped) and connecting
    /// is the integrator's. Used by the built-in `settings.wifi`.
    #[cfg(feature = "settings")]
    pub(crate) fn wifi_network_tapped(&mut self, ssid: &str, secured: bool, known: bool) {
        self.requests.push(CxRequest::WifiNetworkTapped {
            ssid: ssid.to_owned(),
            secured,
            known,
        });
    }

    /// Report that a Wi-Fi network row was held, and **do nothing else** — the shell turns it into
    /// [`ShellEvent::WifiNetworkLongPressed`](crate::ShellEvent::WifiNetworkLongPressed) and
    /// forgetting the network is the integrator's.
    #[cfg(feature = "settings")]
    pub(crate) fn wifi_network_long_pressed(&mut self, ssid: &str, known: bool) {
        self.requests.push(CxRequest::WifiNetworkLongPressed {
            ssid: ssid.to_owned(),
            known,
        });
    }

    /// Ask the shell to make the management calls of one Apply on the authenticator's entries —
    /// the built-in `settings.credentials`. What they came to is [`Cx::admin_note`] on the
    /// next frame.
    #[cfg(feature = "settings")]
    pub(crate) fn credential_ops(&mut self, ops: Vec<crate::access::CredentialOp>) {
        self.requests.push(CxRequest::Credentials(ops));
    }

    /// Ask for the entries to be listed again before the next frame (`settings.credentials`).
    #[cfg(feature = "settings")]
    pub(crate) fn list_credentials(&mut self) {
        self.requests.push(CxRequest::ListCredentials);
    }

    /// Whether the session may give `level`, or change an entry at it (`settings.credentials`).
    #[cfg(feature = "settings")]
    pub(crate) fn may_grant(&self, level: crate::access::Level) -> bool {
        self.access.may_grant(level)
    }

    /// The shell's frame number — what tells a screen it was not drawn on the frame before.
    #[cfg(feature = "settings")]
    pub(crate) fn frame(&self) -> u64 {
        self.frame
    }

    /// The authenticator's entries as last listed (`settings.credentials`).
    #[cfg(feature = "settings")]
    pub(crate) fn credentials(&self) -> &[crate::access::CredentialEntry] {
        self.access.credentials()
    }

    /// The kinds of secret a new one can be (`settings.credentials`).
    #[cfg(feature = "settings")]
    pub(crate) fn secret_kinds(&self) -> &[crate::access::SecretKind] {
        self.access.secret_kinds()
    }

    /// What the last Apply came to (`settings.credentials`).
    #[cfg(feature = "settings")]
    pub(crate) fn admin_note(&self) -> Option<&crate::access::AdminNote> {
        self.access.admin_note()
    }

    /// An arbitrary launch action (the shell checks the gate).
    pub fn launch(&mut self, action: LaunchAction) {
        self.requests.push(CxRequest::Launch(action));
    }

    /// For hiding a section inside a screen (delegates to `policy.allows`).
    #[must_use]
    pub fn allows(&self, gate: impl Into<Gate>) -> bool {
        self.access.allows(&gate.into())
    }

    /// The level table (`[access] levels`).
    ///
    /// The [`Level`](crate::access::Level) from [`Cx::session`] is **a number**. Writing
    /// "Service level" on screen needs the name, and that table is built from the config and
    /// held by the shell — this is the only way to it. Without it the integrator writes
    /// `[access] levels` a second time in code, and when the two drift the screen shows the
    /// wrong level name. The demo actually did.
    ///
    /// ```no_run
    /// # fn draw(cx: &fairing::Cx<'_>) -> String {
    /// let level = cx.levels().get(cx.session.subject.level);
    /// level.map_or_else(|| "unknown".to_owned(), |d| d.label.clone())
    /// # }
    /// ```
    #[must_use]
    pub fn levels(&self) -> &LevelTable {
        self.access.table()
    }

    /// An animated value for a stateless closure. An id seen for the first time
    /// lands on `target` immediately.
    ///
    /// `id` only has to be unique **within the instance**. There is one store in the shell, so
    /// [`PaneInfo::instance`] is mixed in here — two instances of the same declaration opened
    /// with `LaunchMode::Multi` can both use `egui::Id::new("fade")` without their values
    /// mixing, and closing and reopening a screen gives a new instance id, so the animation
    /// starts over. Contexts with no instance (actions and status items,
    /// [`InstanceId::NONE`]) share one namespace.
    ///
    /// A zero-length tween like [`Tween::instant`] returns `target` on the spot.
    pub fn animate(&mut self, id: egui::Id, target: f32, tween: Tween) -> f32 {
        self.animations
            .animate(scope_id(id, self.pane.instance), target, tween, self.frame)
    }

    /// **Say that this screen is mid-motion this frame** without a value from [`Cx::animate`] —
    /// a transition it times itself, a list that refills over several frames. The shell then
    /// counts the frame as animating (`Shell::is_animating`), which keeps the repaint policy
    /// awake and tells a wait-for-rest (the tours' `Settle`, a test's) that it is not at rest
    /// yet. [`crate::layout::transit`] calls it while a page is on its way; a screen with a
    /// motion of its own should too.
    pub fn keep_animating(&mut self) {
        self.animations.keep_animating();
    }

    /// Lend a [`WidgetCx`](fairing_widgets::WidgetCx) — what a control needs, and nothing else.
    ///
    /// The widgets live in [`fairing_widgets`] and take only the theme, the icons and one
    /// animation call. This hands those three over for the length of a widget call, so
    /// a control cannot reach the registry, the services or the access gate even by accident —
    /// which is the whole reason the element layer is a crate of its own.
    pub fn widgets(&mut self) -> fairing_widgets::WidgetCx<'_> {
        fairing_widgets::WidgetCx {
            theme: self.theme,
            icons: &mut *self.icons,
            anims: &mut *self.animations,
            anim_scope: egui::Id::new(self.pane.instance.0),
            frame: self.frame,
            inset_bottom: self.pane.inset_bottom,
            painters: Some(&mut *self.widget_painters),
        }
    }
}

/// The id namespace for [`Cx::animate`]. A function, so the shell and the tests can recompute the same value.
pub(crate) fn scope_id(id: egui::Id, instance: InstanceId) -> egui::Id {
    id.with(instance.0)
}

/// A unit test that wants a `Cx` needs every shell piece (the handle, access, the backends, the
/// theme …). Shared by the in-crate tests in `screen` and `workspace`.
#[cfg(test)]
pub(crate) mod fixture {
    use super::{CxParts, CxRequest, PaneInfo};
    use crate::access::Access;
    use crate::config::AccessConfig;
    use crate::i18n::Strings;
    use crate::icons::IconSet;
    use crate::motion::AnimationStore;
    use crate::services::{Services, Waker};
    use crate::settings::SettingsView;
    use crate::shell::{Command, ShellHandle};
    use crate::theme::Theme;
    use crate::workspace::InstanceId;
    use std::collections::BTreeMap;
    use std::time::Instant;

    /// The shell pieces needed to build a `Cx`. Owned by the test.
    pub(crate) struct Fixture {
        handle: ShellHandle,
        /// The receiver has to stay alive or `ShellHandle::send` fails (it is never read).
        _rx: crate::inbox::Inbox<Command>,
        access: Access,
        services: Services,
        settings: SettingsView,
        hidden: BTreeMap<String, u8>,
        strings: Strings,
        theme: Theme,
        icons: IconSet,
        animations: AnimationStore,
        widget_painters: fairing_widgets::widgets::WidgetPainters,
        /// The requests `Cx` left behind (the test inspects them).
        pub(crate) requests: Vec<CxRequest>,
        /// The shell's time. The test pushes it forward to fake virtual time.
        pub(crate) now: Instant,
        /// The frame number.
        pub(crate) frame: u64,
    }

    impl Fixture {
        /// Build one with the default config (one level table).
        pub(crate) fn new() -> crate::Result<Self> {
            let ctx = egui::Context::default();
            let (handle, rx) = ShellHandle::pair(Waker::new(&ctx));
            Ok(Self {
                handle,
                _rx: rx,
                access: Access::from_config(&AccessConfig::default())?,
                services: Services::null(),
                settings: SettingsView::default(),
                hidden: BTreeMap::new(),
                strings: Strings::default(),
                theme: Theme::dark(),
                icons: IconSet::new(),
                animations: AnimationStore::new(),
                widget_painters: fairing_widgets::widgets::WidgetPainters::new(),
                requests: Vec::new(),
                now: Instant::now(),
                frame: 0,
            })
        }

        /// Lend out the shell pieces at the current time and frame.
        pub(crate) fn parts(&mut self) -> CxParts<'_> {
            CxParts {
                shell: &self.handle,
                access: &self.access,
                services: &mut self.services,
                settings: &self.settings,
                app: None,
                strings: &self.strings,
                theme: &self.theme,
                icons: &mut self.icons,
                animations: &mut self.animations,
                widget_painters: &mut self.widget_painters,
                requests: &mut self.requests,
                hidden: &self.hidden,
                now: self.now,
                frame: self.frame,
            }
        }
    }

    /// A focused 100×100 pane.
    pub(crate) fn pane(instance: InstanceId) -> PaneInfo {
        PaneInfo {
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0)),
            outer: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0)),
            is_split: false,
            is_focused: true,
            inset_bottom: 0.0,
            instance,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{pane, Fixture};
    use super::{scope_id, CxRequest};
    use crate::motion::Tween;
    use crate::screen::{ChromePolicy, Lifecycle, ScreenValue};
    use crate::workspace::InstanceId;
    use std::time::Duration;

    /// The instance namespace: the same `egui::Id` in a different instance is a different entry.
    #[test]
    fn scope_id_separates_instances() {
        let raw = egui::Id::new("fade");
        assert_ne!(scope_id(raw, InstanceId(1)), scope_id(raw, InstanceId(2)));
        assert_eq!(scope_id(raw, InstanceId(1)), scope_id(raw, InstanceId(1)));
        assert_ne!(scope_id(raw, InstanceId::NONE), raw);
        assert_ne!(
            scope_id(egui::Id::new("a"), InstanceId(1)),
            scope_id(raw, InstanceId(1))
        );
    }

    /// Two instances opened with `Multi` do not mix values even using the same id.
    #[test]
    fn animate_does_not_mix_two_instances() -> crate::Result<()> {
        let mut fixture = Fixture::new()?;
        let mut parts = fixture.parts();
        let raw = egui::Id::new("fade");
        let tween = Tween::cubic_out(Duration::from_millis(100));
        // An id seen for the first time is at its target at once — the two instances start from different values.
        let a0 = parts.cx(pane(InstanceId(1)), None).animate(raw, 0.0, tween);
        let b0 = parts.cx(pane(InstanceId(2)), None).animate(raw, 1.0, tween);
        assert!(a0.abs() < f32::EPSILON);
        assert!((b0 - 1.0).abs() < f32::EPSILON);
        assert_eq!(parts.animations.len(), 2, "one entry per instance");
        // Now both head for 1.0, but number 1 is still at its starting point.
        let a1 = parts.cx(pane(InstanceId(1)), None).animate(raw, 1.0, tween);
        let b1 = parts.cx(pane(InstanceId(2)), None).animate(raw, 1.0, tween);
        assert!(a1.abs() < f32::EPSILON, "number 1 sets out from 0");
        assert!((b1 - 1.0).abs() < f32::EPSILON, "number 2 is already at 1");
        Ok(())
    }

    /// `Tween::instant` reaches the target on the spot, with no tick (the `motion.reduce` path).
    #[test]
    fn animate_with_instant_tween_jumps_to_target() -> crate::Result<()> {
        let mut fixture = Fixture::new()?;
        let mut parts = fixture.parts();
        let id = egui::Id::new("reduce");
        let first = parts
            .cx(pane(InstanceId(7)), None)
            .animate(id, 0.0, Tween::instant());
        assert!(first.abs() < f32::EPSILON);
        let jumped = parts
            .cx(pane(InstanceId(7)), None)
            .animate(id, 1.0, Tween::instant());
        assert!(
            (jumped - 1.0).abs() < f32::EPSILON,
            "an instant tween arrives in one go"
        );
        parts.animations.tick(1.0 / 60.0, 1);
        assert!(
            !parts.animations.is_animating(),
            "after arriving instantly no repaint is needed"
        );
        // A tween with a duration takes several frames.
        let slow = parts.cx(pane(InstanceId(7)), None).animate(
            id,
            0.0,
            Tween::cubic_out(Duration::from_millis(100)),
        );
        assert!(
            (slow - 1.0).abs() < f32::EPSILON,
            "the first frame is still at the start"
        );
        assert!(parts.animations.is_animating());
        Ok(())
    }

    /// `cx.event` and `cx.pane` are carried across verbatim by `CxParts::cx`.
    #[test]
    fn cx_carries_the_lifecycle_event_and_pane() -> crate::Result<()> {
        let mut fixture = Fixture::new()?;
        let mut parts = fixture.parts();
        let cx = parts.cx(pane(InstanceId(3)), Some(Lifecycle::Created));
        assert_eq!(cx.event, Some(Lifecycle::Created));
        assert_eq!(cx.pane.instance, InstanceId(3));
        assert!(cx.pane.is_focused);
        let cx = parts.cx(pane(InstanceId::NONE), None);
        assert_eq!(cx.event, None);
        // The default config has one level table, so every gate passes.
        assert!(cx.allows("anything"));
        Ok(())
    }

    /// A `Cx`'s requests queue in order and the shell handles them in stage 14.
    #[test]
    fn cx_queues_requests_in_order() -> crate::Result<()> {
        let mut fixture = Fixture::new()?;
        {
            let mut parts = fixture.parts();
            let mut cx = parts.cx(pane(InstanceId(5)), None);
            cx.open("b");
            cx.open_in_other_pane("c");
            cx.set_chrome(ChromePolicy::fullscreen());
            cx.finish_with(ScreenValue::Int(7));
            cx.finish();
        }
        let expected = vec![
            CxRequest::Open {
                id: "b".to_owned(),
                in_other_pane: false,
                from: InstanceId(5),
            },
            CxRequest::Open {
                id: "c".to_owned(),
                in_other_pane: true,
                from: InstanceId(5),
            },
            CxRequest::SetChrome {
                instance: InstanceId(5),
                policy: ChromePolicy::fullscreen(),
            },
            CxRequest::Finish {
                instance: InstanceId(5),
                value: Some(ScreenValue::Int(7)),
            },
            CxRequest::Finish {
                instance: InstanceId(5),
                value: None,
            },
        ];
        assert_eq!(fixture.requests, expected);
        Ok(())
    }
}
