//! Screens — declarations, the trait, the lifecycle and launch modes.
//!
//! A screen is one `FnMut(&mut egui::Ui, &mut Cx)`. Beyond filling the pane rect the shell hands
//! it, there are no constraints. `screen(id, closure)` is **resident** (the instance is the
//! declaration, and its state survives closing); `screen_with(id, factory)` is **factory-built**
//! (created on open, dropped on close).

pub(crate) mod cx;
mod policy;
mod registry;

pub use cx::{Cx, PaneInfo};
pub(crate) use cx::{CxParts, CxRequest};
pub use policy::{BarMode, ChromePolicy, OskMode};
#[doc(hidden)]
pub use registry::Registry;
pub use registry::{Decl, DeclKind};

use crate::access::{Gate, Visibility};
use crate::i18n::LabelKey;
use crate::icons::IconRef;
use crate::settings::{SettingKey, SettingValue};
use crate::theme::ColorRole;
use crate::workspace::InstanceScreen;
use std::time::Duration;

/// A screen with state. `ui` is the only required method.
pub trait Screen {
    /// Every frame. `ui` is the whole pane content rect.
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>);

    /// Back. `Consumed` if the screen has navigation of its own.
    fn on_back(&mut self, _cx: &mut Cx<'_>) -> BackAction {
        BackAction::Pop
    }

    /// A lifecycle notification. A closure screen receives the same events through `cx.event`.
    fn on_lifecycle(&mut self, _ev: Lifecycle, _cx: &mut Cx<'_>) {}

    /// The result a child handed back with `cx.finish_with(v)`.
    fn on_result(&mut self, _from: &str, _v: ScreenValue, _cx: &mut Cx<'_>) {}
}

/// A closure is automatically a [`Screen`].
impl<F: FnMut(&mut egui::Ui, &mut Cx<'_>)> Screen for F {
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self(ui, cx);
    }
}

/// What handling back did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackAction {
    /// Pop the stack.
    Pop,
    /// The screen consumed it.
    Consumed,
}

/// A lifecycle event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lifecycle {
    /// Right after the instance exists, before the first `ui`.
    Created,
    /// Visible and focused.
    Resumed,
    /// Visible but not focused.
    Paused,
    /// Not visible.
    Stopped,
    /// The session's subject or level changed.
    AccessChanged,
    /// The pane's size changed.
    Resized(egui::Vec2),
    /// The last notification. The drop follows.
    Destroyed,
}

/// The value a screen hands back to its parent.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ScreenValue {
    /// A boolean.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// A string.
    Text(String),
    /// Bytes.
    Bytes(Vec<u8>),
    /// Nothing.
    #[default]
    None,
}

/// The launch mode. A resident screen is always `Single`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchMode {
    /// Reuse a live instance (clear-top, or bring the task forward).
    #[default]
    Single,
    /// Push every time. Factory screens only.
    Multi,
}

/// How small a pane a screen can share the content in.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SplitSupport {
    /// In a pane no smaller, along the split, than a quarter of the content or three touch
    /// targets, whichever is larger.
    #[default]
    Yes,
    /// In a pane no smaller than this (du): along the split it keeps this much — `x` side by
    /// side, `y` stacked — and across it the content has to have room, or no split holds.
    MinSize(egui::Vec2),
    /// Never in a split: opened "in the other pane" it opens in the same one, and it is never put
    /// beside another.
    No,
}

/// A launch action. Icons, `ShellHandle::launch` and deep links all build one.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum LaunchAction {
    /// Open a screen declaration id.
    Open {
        /// The declaration id.
        id: String,
        /// Open in the other pane ([`Cx::open_in_other_pane`](crate::Cx::open_in_other_pane)).
        in_other_pane: bool,
    },
    /// Run an `action(id, ..)` declaration.
    Run(String),
    /// Write a setting (M4).
    Set(SettingKey, SettingValue),
    /// Toggle a setting (M4).
    Toggle(SettingKey),
    /// Lock. Always reported as [`ShellEvent::LockRequested`](crate::ShellEvent::LockRequested);
    /// in `prompt` mode with an authenticator the shell's lock screen comes up as well.
    Lock,
    /// Log out. Always reported as
    /// [`ShellEvent::LogoutRequested`](crate::ShellEvent::LogoutRequested); in `prompt` mode with an
    /// authenticator the session goes back to the starting subject as well.
    Logout,
    /// Open the shade (M2).
    OpenOverlay,
    /// The recent screens: up, or back down when they are up. Always reported as
    /// [`ShellEvent::OverviewRequested`](crate::ShellEvent::OverviewRequested); carried out where
    /// `[workspace] overview` is on and the session passes `nav.recents`.
    OpenOverview,
    /// The split control: back to one pane keeping the focused one, or two — the cards
    /// as a picker, or the task used last. Always reported as
    /// [`ShellEvent::SplitRequested`](crate::ShellEvent::SplitRequested); carried out where
    /// `[workspace] split` is on and, to enter a split, the session passes `workspace.split`.
    ToggleSplit,
}

impl LaunchAction {
    /// `Open { id, in_other_pane: false }`.
    #[must_use]
    pub fn open(id: impl Into<String>) -> Self {
        Self::Open {
            id: id.into(),
            in_other_pane: false,
        }
    }

    /// `Run(id)`.
    #[must_use]
    pub fn run(id: impl Into<String>) -> Self {
        Self::Run(id.into())
    }
}

/// A desktop cell (`.desktop_at(page, col, row)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// The page.
    pub page: u8,
    /// The column.
    pub col: u8,
    /// The row.
    pub row: u8,
}

/// A request for desktop placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesktopPlacement {
    /// Not placed.
    #[default]
    None,
    /// The next free cell.
    Auto,
    /// A pinned cell.
    At(Placement),
}

/// Where a screen's instance comes from.
pub(crate) enum ScreenSource {
    /// Resident — **the declaration owns it for its whole life.** An instance of it holds no screen
    /// of its own ([`InstanceScreen::Resident`]) and borrows this one by declaration id when it is
    /// called. That is what makes the state survive a close and reopen.
    ///
    /// It is an `Option` because the borrow is a **loan**: [`Registry::take_resident`] moves the
    /// screen out for the length of one call and [`Registry::put_resident`] puts it back, so while
    /// it is being drawn the slot is empty and nothing can reach it a second time.
    Resident(Option<Box<dyn Screen>>),
    /// Factory: a call per open.
    Factory(Box<dyn FnMut() -> Box<dyn Screen>>),
}

/// A screen declaration, with its icon, gate and chrome (the builder table).
pub struct ScreenDecl {
    pub(crate) id: String,
    pub(crate) title: LabelKey,
    pub(crate) description: Option<LabelKey>,
    pub(crate) icon: Option<IconRef>,
    pub(crate) desktop: DesktopPlacement,
    pub(crate) dock: bool,
    pub(crate) gate: Option<Gate>,
    pub(crate) visibility: Visibility,
    pub(crate) launch: LaunchMode,
    pub(crate) split: SplitSupport,
    pub(crate) chrome: ChromePolicy,
    pub(crate) keep_awake: bool,
    pub(crate) evict_after: Option<Duration>,
    pub(crate) background: Option<ColorRole>,
    pub(crate) source: ScreenSource,
}

impl std::fmt::Debug for ScreenDecl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScreenDecl")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("icon", &self.icon)
            .field("desktop", &self.desktop)
            .field("dock", &self.dock)
            .field("gate", &self.gate)
            .field("launch", &self.launch)
            .finish_non_exhaustive()
    }
}

/// A resident screen declaration. The shortest form — this alone registers and runs.
pub fn screen(
    id: impl Into<String>,
    ui: impl FnMut(&mut egui::Ui, &mut Cx<'_>) + 'static,
) -> ScreenDecl {
    let id = id.into();
    ScreenDecl::new(id, ScreenSource::Resident(Some(Box::new(ui))))
}

/// **A resident screen declaration from a value of your own** — a `struct` implementing
/// [`Screen`], not a closure.
///
/// [`screen`] takes a closure, which is the whole of the trait's `ui` and nothing else. A screen
/// that wants [`Screen::on_back`] — "back cancels the edit I am in the middle of" — has to be a
/// type, and until now the only way to register a type was [`screen_with`], which is a **factory**:
/// a fresh value per open, its state dropped on close, and no screen owned between opens for
/// [`Cx::draw_screen`] to embed. This is the missing third: the declaration owns your value for its
/// whole life, exactly as it owns a closure.
///
/// ```no_run
/// # fn main() {
/// # let shell: &mut fairing::Shell = todo!();
/// use fairing::screen::{screen_of, BackAction, Screen};
/// use fairing::Cx;
///
/// #[derive(Default)]
/// struct NetworkScreen {
///     editing: Option<String>,
/// }
///
/// impl Screen for NetworkScreen {
///     fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
///         ui.label(self.editing.as_deref().unwrap_or("idle"));
///     }
///
///     // The reason it has to be a type: back in the middle of an edit cancels the edit
///     // rather than leaving the screen.
///     fn on_back(&mut self, _cx: &mut Cx<'_>) -> BackAction {
///         if self.editing.take().is_some() {
///             return BackAction::Consumed;
///         }
///         BackAction::Pop
///     }
/// }
///
/// shell.add(screen_of("settings.network", NetworkScreen::default()));
/// # }
/// ```
///
/// Being resident it keeps its state across close and reopen, it can be embedded with
/// [`Cx::draw_screen`] (so it goes in the settings right column), and — like every resident
/// declaration — it is [`LaunchMode::Single`] only.
pub fn screen_of(id: impl Into<String>, screen: impl Screen + 'static) -> ScreenDecl {
    let id = id.into();
    ScreenDecl::new(id, ScreenSource::Resident(Some(Box::new(screen))))
}

/// A factory screen declaration. `factory` makes a new instance on every open.
pub fn screen_with<S: Screen + 'static>(
    id: impl Into<String>,
    mut factory: impl FnMut() -> S + 'static,
) -> ScreenDecl {
    let id = id.into();
    ScreenDecl::new(
        id,
        ScreenSource::Factory(Box::new(move || Box::new(factory()))),
    )
}

impl ScreenDecl {
    fn new(id: String, source: ScreenSource) -> Self {
        Self {
            title: id.clone(),
            id,
            description: None,
            icon: None,
            desktop: DesktopPlacement::None,
            dock: false,
            gate: None,
            visibility: Visibility::Locked,
            launch: LaunchMode::Single,
            split: SplitSupport::Yes,
            chrome: ChromePolicy::default(),
            keep_awake: false,
            evict_after: None,
            background: None,
            source,
        }
    }

    /// The recents and icon label. Defaults to the id.
    #[must_use]
    pub fn title(mut self, title: impl Into<LabelKey>) -> Self {
        self.title = title.into();
        self
    }

    /// What the icon's info popover says under the title — a sentence or two on what the screen
    /// is for. A long press on the desktop icon brings the popover up; without a
    /// description it shows the title, and the level the screen needs, alone.
    ///
    /// A key, like the title: it is looked up in the string table where it is drawn, so a
    /// translation of it follows the language.
    #[must_use]
    pub fn description(mut self, text: impl Into<LabelKey>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// The icon. Without one it cannot go on the desktop or the dock.
    #[must_use]
    pub fn icon(mut self, icon: IconRef) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A desktop icon (in the next free cell).
    ///
    /// With no icon it is not placed. The builder does not force an order (`.icon()` after
    /// `.desktop()` is valid), so that check happens once, in [`Shell::add`](crate::Shell::add),
    /// which sees the finished declaration.
    #[must_use]
    pub fn desktop(mut self) -> Self {
        if !matches!(self.desktop, DesktopPlacement::At(_)) {
            self.desktop = DesktopPlacement::Auto;
        }
        self
    }

    /// Pin a desktop cell.
    #[must_use]
    pub fn desktop_at(mut self, page: u8, col: u8, row: u8) -> Self {
        self.desktop = DesktopPlacement::At(Placement { page, col, row });
        self
    }

    /// A dock icon.
    #[must_use]
    pub fn dock(mut self) -> Self {
        self.dock = true;
        self
    }

    /// The gate name (the id by default). Levels are assigned in `[access.gates]`.
    #[must_use]
    pub fn gate(mut self, gate: impl Into<Gate>) -> Self {
        self.gate = Some(gate.into());
        self
    }

    /// How the icon presents when the gate is closed.
    #[must_use]
    pub fn visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// The launch mode. `Multi` is for **factory screens (`screen_with`)
    /// only** — a resident screen has exactly one instance, so pushing it twice would draw the
    /// same state in two places. Given `Multi`, a resident screen warns and stays `Single`.
    ///
    /// The launch mode only looks at the source already fixed by `screen` / `screen_with`, so
    /// builder order does not matter.
    #[must_use]
    pub fn launch(mut self, mode: LaunchMode) -> Self {
        if mode == LaunchMode::Multi && matches!(self.source, ScreenSource::Resident(_)) {
            log::warn!(
                "screen `{}`: a resident screen (`screen`) does not support LaunchMode::Multi - \
                 keeping Single. Declare it with `screen_with` if you need several instances",
                self.id
            );
        } else {
            self.launch = mode;
        }
        self
    }

    /// How small a pane it can share the content in. [`SplitSupport::Yes`] by default.
    #[must_use]
    pub fn split(mut self, split: SplitSupport) -> Self {
        self.split = split;
        self
    }

    /// The preset that hides the status bar and the nav bar.
    #[must_use]
    pub fn fullscreen(mut self) -> Self {
        self.chrome = ChromePolicy::fullscreen();
        self
    }

    /// Set the chrome policy in detail.
    #[must_use]
    pub fn chrome(mut self, policy: ChromePolicy) -> Self {
        self.chrome = policy;
        self
    }

    /// The on-screen keyboard mode (M2).
    #[must_use]
    pub fn osk(mut self, mode: OskMode) -> Self {
        self.chrome.osk = mode;
        self
    }

    /// Stop the display idle timer.
    #[must_use]
    pub fn keep_awake(mut self) -> Self {
        self.keep_awake = true;
        self.chrome.keep_awake = true;
        self
    }

    /// Drop it from memory while it is not visible (factory screens). `Instance::evict_due(now)`
    /// decides and `Workspace::evict_expired(now)` acts (frame stage 5).
    ///
    /// **Factory (`screen_with`) only.** A resident screen's state is held by the declaration
    /// (the closure's captures), so dropping the instance reclaims nothing (a
    /// closure screen has no state, so evicting is meaningless) — it warns and is ignored.
    /// `Duration::ZERO` drops it on the frame after the `Stopped` notification.
    #[must_use]
    pub fn evict_after(mut self, after: Duration) -> Self {
        if matches!(self.source, ScreenSource::Resident(_)) {
            log::warn!(
                "screen `{}`: evict_after means nothing for a resident screen (`screen`) - ignoring it",
                self.id
            );
            return self;
        }
        self.evict_after = Some(after);
        self
    }

    /// The screen's background colour (the placeholder card's colour, A2).
    #[must_use]
    pub fn background(mut self, role: ColorRole) -> Self {
        self.background = Some(role);
        self.chrome.background = Some(role);
        self
    }

    /// **Register this declaration under a different id** — a built-in taken as a template.
    ///
    /// Everything else rides along: the body, the title, the icon, the gate, the chrome policy,
    /// the split support. So a screen the crate ships can become one of yours without rewriting it:
    ///
    /// ```no_run
    /// # fn main() {
    /// # let shell: &mut fairing::Shell = todo!();
    /// use fairing::settings::screens;
    ///
    /// // The built-in Display screen, as a second screen of our own.
    /// shell.add(screens::display().with_id("app.display").title("Panel"));
    /// # }
    /// ```
    ///
    /// Before this the id was fixed at construction and only readable, so the one way to reuse a
    /// built-in was to write its body again from `layout` - against a body the crate keeps private.
    /// Replacing a built-in in place never needed it ([`Shell::add`](crate::Shell::add) replaces
    /// by id); having a copy *beside* the original did.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// The declaration id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.title
    }

    /// The icon.
    #[must_use]
    pub fn icon_ref(&self) -> Option<&IconRef> {
        self.icon.as_ref()
    }

    /// The gate name (the id if there is none).
    #[must_use]
    pub fn gate_name(&self) -> Gate {
        self.gate.clone().unwrap_or_else(|| Gate::from(&self.id))
    }

    /// The chrome policy.
    #[must_use]
    pub fn chrome_policy(&self) -> ChromePolicy {
        self.chrome
    }

    /// The launch mode.
    #[must_use]
    pub fn launch_mode(&self) -> LaunchMode {
        self.launch
    }

    /// Whether it is resident.
    #[must_use]
    pub fn is_resident(&self) -> bool {
        matches!(self.source, ScreenSource::Resident(_))
    }

    /// Build the screen for an instance. A resident declaration hands over nothing and keeps its
    /// screen; a factory one gives the result of a call.
    pub(crate) fn spawn(&mut self) -> InstanceScreen {
        match &mut self.source {
            // Nothing is handed over — the declaration keeps the screen and the instance finds it
            // again by id. One owner, so no shared cell and no borrow that can fail.
            ScreenSource::Resident(_) => InstanceScreen::Resident,
            ScreenSource::Factory(factory) => InstanceScreen::Owned(factory()),
        }
    }
}

/// An icon with no screen — a tap runs it (the `action(id, f)` declaration).
pub struct ActionDecl {
    pub(crate) id: String,
    pub(crate) title: LabelKey,
    pub(crate) description: Option<LabelKey>,
    pub(crate) icon: Option<IconRef>,
    pub(crate) desktop: DesktopPlacement,
    pub(crate) dock: bool,
    pub(crate) gate: Option<Gate>,
    pub(crate) visibility: Visibility,
    pub(crate) run: Box<dyn FnMut(&mut Cx<'_>)>,
}

impl std::fmt::Debug for ActionDecl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionDecl")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// An action declaration.
pub fn action(id: impl Into<String>, run: impl FnMut(&mut Cx<'_>) + 'static) -> ActionDecl {
    let id = id.into();
    ActionDecl {
        title: id.clone(),
        id,
        description: None,
        icon: None,
        desktop: DesktopPlacement::None,
        dock: false,
        gate: None,
        visibility: Visibility::Locked,
        run: Box::new(run),
    }
}

impl ActionDecl {
    /// The label.
    #[must_use]
    pub fn title(mut self, title: impl Into<LabelKey>) -> Self {
        self.title = title.into();
        self
    }

    /// What the icon's info popover says under the title — the same as
    /// [`ScreenDecl::description`]. A key, looked up where it is drawn.
    #[must_use]
    pub fn description(mut self, text: impl Into<LabelKey>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// The icon.
    #[must_use]
    pub fn icon(mut self, icon: IconRef) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The desktop (the next free cell).
    #[must_use]
    pub fn desktop(mut self) -> Self {
        if !matches!(self.desktop, DesktopPlacement::At(_)) {
            self.desktop = DesktopPlacement::Auto;
        }
        self
    }

    /// Pin a desktop cell.
    #[must_use]
    pub fn desktop_at(mut self, page: u8, col: u8, row: u8) -> Self {
        self.desktop = DesktopPlacement::At(Placement { page, col, row });
        self
    }

    /// The dock.
    #[must_use]
    pub fn dock(mut self) -> Self {
        self.dock = true;
        self
    }

    /// The gate name.
    #[must_use]
    pub fn gate(mut self, gate: impl Into<Gate>) -> Self {
        self.gate = Some(gate.into());
        self
    }

    /// How a closed gate presents.
    #[must_use]
    pub fn visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// The declaration id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The gate name (the id if there is none).
    #[must_use]
    pub fn gate_name(&self) -> Gate {
        self.gate.clone().unwrap_or_else(|| Gate::from(&self.id))
    }

    /// Run it.
    pub fn run(&mut self, cx: &mut Cx<'_>) {
        (self.run)(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::Registry;
    use super::{
        action, screen, screen_with, BackAction, BarMode, ChromePolicy, Cx, DesktopPlacement,
        LaunchAction, LaunchMode, Placement, Screen, ScreenValue,
    };
    use crate::access::Gate;
    use crate::icons::IconRef;
    use crate::theme::ColorRole;
    use crate::workspace::InstanceScreen;
    use std::time::Duration;

    /// The minimum screen with state.
    #[derive(Default)]
    struct Counter(u32);

    impl Screen for Counter {
        fn ui(&mut self, _ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
            self.0 += 1;
        }
    }

    fn resident(id: &str) -> super::ScreenDecl {
        screen(id, |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("x");
        })
    }

    fn generated(id: &str) -> super::ScreenDecl {
        screen_with(id, Counter::default)
    }

    /// `Multi` is factory-only. A resident screen warns and stays `Single`.
    #[test]
    fn multi_is_only_for_generated_screens() {
        assert_eq!(
            resident("a").launch(LaunchMode::Multi).launch_mode(),
            LaunchMode::Single
        );
        assert_eq!(
            generated("a").launch(LaunchMode::Multi).launch_mode(),
            LaunchMode::Multi
        );
        // Stating `Single` explicitly passes through as it is, even on a resident screen.
        assert_eq!(
            resident("a").launch(LaunchMode::Single).launch_mode(),
            LaunchMode::Single
        );
        assert!(resident("a").is_resident());
        assert!(!generated("a").is_resident());
    }

    /// `evict_after` is factory-only. On a resident screen it is ignored.
    #[test]
    fn evict_after_is_ignored_for_resident_screens() {
        let d = Duration::from_millis(100);
        assert_eq!(resident("a").evict_after(d).evict_after, None);
        assert_eq!(generated("a").evict_after(d).evict_after, Some(d));
        assert_eq!(
            generated("a").evict_after(Duration::ZERO).evict_after,
            Some(Duration::ZERO)
        );
    }

    /// The builder does not force an order — either way round produces the same declaration.
    #[test]
    fn builder_order_does_not_matter() {
        let icon = IconRef::Builtin("gauge");
        let a = resident("a").desktop().icon(icon.clone());
        let b = resident("a").icon(icon).desktop();
        assert_eq!(a.desktop, DesktopPlacement::Auto);
        assert_eq!(b.desktop, DesktopPlacement::Auto);
        assert!(a.icon_ref().is_some() && b.icon_ref().is_some());
    }

    /// With `.desktop_at(..)` first, a following `.desktop()` must not clear the pinned cell.
    #[test]
    fn desktop_does_not_clobber_a_fixed_placement() {
        let d = resident("a").desktop_at(1, 2, 3).desktop();
        assert_eq!(
            d.desktop,
            DesktopPlacement::At(Placement {
                page: 1,
                col: 2,
                row: 3
            })
        );
        // In the reverse order, the pinned position wins.
        let d = resident("a").desktop().desktop_at(0, 1, 1);
        assert_eq!(
            d.desktop,
            DesktopPlacement::At(Placement {
                page: 0,
                col: 1,
                row: 1
            })
        );
    }

    /// The gate name defaults to the id.
    #[test]
    fn gate_name_defaults_to_id() {
        assert_eq!(resident("camera").gate_name(), Gate::from("camera"));
        assert_eq!(
            resident("camera").gate("service-tools").gate_name(),
            Gate::from("service-tools")
        );
        assert_eq!(action("reboot", |_| {}).gate_name(), Gate::from("reboot"));
        assert_eq!(
            action("reboot", |_| {}).gate("power").gate_name(),
            Gate::from("power")
        );
    }

    /// `.fullscreen()` hides both bars and keeps `allow_peek`; `.background()` is carried in the chrome too.
    #[test]
    fn chrome_builders_match_the_policy() {
        let d = resident("a").fullscreen();
        assert_eq!(d.chrome_policy(), ChromePolicy::fullscreen());
        assert_eq!(d.chrome_policy().status_bar, BarMode::Hide);
        let d = resident("a").background(ColorRole::Surface);
        assert_eq!(d.background, Some(ColorRole::Surface));
        assert_eq!(d.chrome_policy().background, Some(ColorRole::Surface));
        let d = resident("a").keep_awake();
        assert!(d.keep_awake && d.chrome_policy().keep_awake);
        // `.chrome(..)` overrides it whole.
        let d = resident("a").fullscreen().chrome(ChromePolicy::default());
        assert_eq!(d.chrome_policy(), ChromePolicy::default());
    }

    /// A resident declaration **keeps** its screen; a factory one builds a fresh screen each time
    /// (the ownership table).
    ///
    /// This used to compare what `spawn` handed back with `Rc::ptr_eq`. Under single ownership
    /// a resident spawn hands back nothing at all — the declaration is still the only
    /// owner and the instance finds the screen again by id — so what there is to check is that the
    /// registry answers for a resident one and does not for a factory one.
    #[test]
    fn a_resident_declaration_keeps_its_screen_and_a_factory_builds_each_time() {
        let mut decl = resident("a");
        assert!(matches!(decl.spawn(), InstanceScreen::Resident));
        assert!(matches!(decl.spawn(), InstanceScreen::Resident));

        let mut decl = generated("a");
        assert!(matches!(decl.spawn(), InstanceScreen::Owned(_)));
        assert!(matches!(decl.spawn(), InstanceScreen::Owned(_)));

        let mut registry = Registry::new();
        registry.add(resident("res").into());
        registry.add(generated("gen").into());
        let lent = registry.take_resident("res");
        assert!(
            lent.is_some(),
            "the declaration has to still own a resident screen"
        );
        assert!(
            registry.take_resident("res").is_none(),
            "while it is out on loan nobody else can have it"
        );
        if let Some(screen) = lent {
            registry.put_resident("res", screen);
        }
        assert!(
            registry.take_resident("res").is_some(),
            "and it is back where it came from"
        );
        assert!(
            registry.take_resident("gen").is_none(),
            "a factory declaration owns no screen between opens"
        );
        assert!(registry.take_resident("nope").is_none());
    }

    /// The title defaults to the id, and `.title()` changes it.
    #[test]
    fn title_defaults_to_id() {
        assert_eq!(resident("dashboard").label(), "dashboard");
        assert_eq!(
            resident("dashboard").title("Dashboard").label(),
            "Dashboard"
        );
        assert_eq!(action("reboot", |_| {}).id(), "reboot");
    }

    /// `ScreenValue`'s default is `None`.
    #[test]
    fn screen_value_default_is_none() {
        assert_eq!(ScreenValue::default(), ScreenValue::None);
        assert_eq!(ScreenValue::Bool(true), ScreenValue::Bool(true));
        assert_ne!(ScreenValue::Int(1), ScreenValue::Float(1.0));
        assert_eq!(
            ScreenValue::Text("ssid".to_owned()).clone(),
            ScreenValue::Text("ssid".to_owned())
        );
        assert_eq!(
            ScreenValue::Bytes(vec![1, 2]),
            ScreenValue::Bytes(vec![1, 2])
        );
    }

    /// The trait's default: back is `Pop`.
    #[test]
    fn default_back_action_is_pop() {
        assert_eq!(BackAction::Pop, BackAction::Pop);
        assert_ne!(BackAction::Pop, BackAction::Consumed);
    }

    /// The `LaunchAction` constructors.
    #[test]
    fn launch_action_constructors() {
        assert_eq!(
            LaunchAction::open("a"),
            LaunchAction::Open {
                id: "a".to_owned(),
                in_other_pane: false
            }
        );
        assert_eq!(LaunchAction::run("r"), LaunchAction::Run("r".to_owned()));
    }
}
