//! [`ShellBuilder`] — the compile-time customisation entry point.
//!
//! This is where a device developer changes the shell **from code, without touching the crate**.
//! Unlike what a config file does (layout and gate assignment), here the drawing itself changes:
//!
//! | Method | What it changes |
//! |---|---|
//! | [`theme`](ShellBuilder::theme) | Every palette, metric and motion token (`[shell] theme` ignored) |
//! | [`services`](ShellBuilder::services) | The backend bundle |
//! | [`status_bar_painter`](ShellBuilder::status_bar_painter) | The status bar, whole |
//! | [`status_bar_layout`](ShellBuilder::status_bar_layout) | Where the status bar's items go — the drawing stays the shell's |
//! | [`nav_bar_painter`](ShellBuilder::nav_bar_painter) | The nav bar, whole |
//! | [`nav_bar_layout`](ShellBuilder::nav_bar_layout) | Where the nav bar's items go — the drawing stays the shell's |
//! | [`toast_painter`](ShellBuilder::toast_painter) · [`toast_layout`](ShellBuilder::toast_layout) | Each toast's card · where the stack goes |
//! | [`heads_up_painter`](ShellBuilder::heads_up_painter) · [`heads_up_layout`](ShellBuilder::heads_up_layout) | The heads-up banner · where it goes |
//! | `osk_key_painter` · `osk_key_layout` (feature `osk`) | Each keyboard key · where the keys go |
//! | `shade_tile_layout` (feature `overlay`) | Where the shade's tiles go — the drawing stays the shell's |
//! | `shade_tile_painter` · `shade_panel_painter` (feature `overlay`) | Each quick-settings tile · the panel's ground |
//! | [`lock_screen_painter`](ShellBuilder::lock_screen_painter) · [`unlock_prompt_painter`](ShellBuilder::unlock_prompt_painter) | The lock screen's ground and clock · the unlock prompt's backdrop and card |
//! | [`recents_card_painter`](ShellBuilder::recents_card_painter) · [`recents_ground_painter`](ShellBuilder::recents_ground_painter) | Each recent screen's card · the ground under them |
//! | [`widget_painters`](ShellBuilder::widget_painters) | Each kind of widget, wherever it is drawn — the shell's own screens and prompt included |
//! | [`gesture_handle_painter`](ShellBuilder::gesture_handle_painter) | Each gesture handle's strip, and the arrow while it is swiped |
//! | [`slot_painter`](ShellBuilder::slot_painter) | Each desktop cell |
//! | [`authenticator`](ShellBuilder::authenticator) | Who may unlock what — the shell draws whatever it asks for |
//!
//! Narrower hooks than those are already open through the declarations
//! (`screen` / `action` / `status_item` / `nav_item`), `Wallpaper::Painter` and
//! `register_icon_painter`. There is a real example in `examples/custom_chrome.rs`.

use super::{Shell, ShellHandle};
use crate::access::{Access, Authenticator};
use crate::chrome::{BarCx, BarPainter, NavLayout};
use crate::chrome::{NavBar, PolicyDriver, StatusBar};
use crate::config::ShellConfig;
use crate::desktop::{DesktopView, SlotCx, SlotPainter};
use crate::error::Result;
use crate::fonts::{FontSet, FontSource};
use crate::gesture::{GestureEngine, GestureTuning};
use crate::i18n::{Strings, Translations};
use crate::icons::IconSet;
use crate::motion::AnimationStore;
use crate::notify::{
    HeadsUp, HeadsUpLayout, HeadsUpPainter, NotificationCenter, ToastLayout, ToastPainter,
    ToastQueue,
};
use crate::screen::Registry;
use crate::services::{Services, Waker};
use crate::settings::SettingsView;
use crate::shell::slots::{OskSlot, OverlaySlot};
use crate::shell::Layout;
use crate::theme::Palette;
use crate::theme::Theme;
use crate::theme::ThemeFade;
use crate::theme::{ComponentSpec, ControlSpec, ElevationSpec, MetricsSpec};
use crate::unit::{Dim, ScalePolicy, Span};
use crate::workspace::Workspace;
use std::time::{Duration, Instant};

/// The shell builder. Made with [`Shell::builder`].
pub struct ShellBuilder {
    config: ShellConfig,
    services: Option<Services>,
    theme: Option<Theme>,
    palettes: Option<(Palette, Palette)>,
    fonts: FontSet,
    translations: Vec<Translations>,
    scale_policy: ScalePolicy,
    metrics_spec: Option<MetricsSpec>,
    component_spec: Option<ComponentSpec>,
    control_spec: Option<ControlSpec>,
    elevation_spec: Option<ElevationSpec>,
    physical_mm: Option<(f32, f32)>,
    status_bar_painter: Option<BarPainter>,
    status_bar_layout: Option<crate::chrome::StatusLayout>,
    nav_bar_painter: Option<BarPainter>,
    nav_bar_layout: Option<NavLayout>,
    notify_hooks: NotifyHooks,
    #[cfg(feature = "osk")]
    osk_hooks: OskHooks,
    #[cfg(feature = "overlay")]
    shade_hooks: ShadeHooks,
    slot_painter: Option<SlotPainter>,
    image_loader: Option<crate::desktop::ImageLoader>,
    wallpaper: Option<crate::desktop::Wallpaper>,
    authenticator: Option<Box<dyn Authenticator>>,
    lock_screen_painter: Option<crate::access::LockScreenPainter>,
    unlock_prompt_painter: Option<crate::access::UnlockPromptPainter>,
    recents: crate::workspace::RecentsPainters,
    widget_painters: crate::widgets::WidgetPainters,
    gesture_handle_painter: Option<crate::gesture::HandlePainter>,
}

/// The toasts' and the heads-up banner's rungs 4 and 5, handed to their owners in
/// `build`.
#[derive(Default)]
struct NotifyHooks {
    toast_painter: Option<ToastPainter>,
    toast_layout: Option<ToastLayout>,
    heads_up_painter: Option<HeadsUpPainter>,
    heads_up_layout: Option<HeadsUpLayout>,
}

impl NotifyHooks {
    /// Give each hook to the queue or the banner that draws with it.
    fn install(self, toasts: &mut ToastQueue, heads_up: &mut HeadsUp) {
        if let Some(painter) = self.toast_painter {
            toasts.set_painter(painter);
        }
        if let Some(layout) = self.toast_layout {
            toasts.set_layout(layout);
        }
        if let Some(painter) = self.heads_up_painter {
            heads_up.set_painter(painter);
        }
        if let Some(layout) = self.heads_up_layout {
            heads_up.set_layout(layout);
        }
    }
}

/// The keyboard's rungs 4 and 5, handed to it in `build`.
#[cfg(feature = "osk")]
#[derive(Default)]
struct OskHooks {
    key_layout: Option<crate::osk::OskKeyLayout>,
    key_painter: Option<crate::osk::OskKeyPainter>,
}

#[cfg(feature = "osk")]
impl OskHooks {
    /// Give each hook to the keyboard.
    fn install(self, osk: &mut crate::osk::Osk) {
        if let Some(layout) = self.key_layout {
            osk.set_key_layout(layout);
        }
        if let Some(painter) = self.key_painter {
            osk.set_key_painter(painter);
        }
    }
}

/// The shade's rungs 4 and 5, handed to it in `build`.
#[cfg(feature = "overlay")]
#[derive(Default)]
struct ShadeHooks {
    tile_layout: Option<crate::overlay::ShadeTileLayout>,
    tile_painter: Option<crate::overlay::ShadeTilePainter>,
    panel_painter: Option<crate::overlay::ShadePanelPainter>,
}

#[cfg(feature = "overlay")]
impl ShadeHooks {
    /// Give each hook to the shade.
    fn install(self, overlay: &mut crate::overlay::Overlay) {
        if let Some(layout) = self.tile_layout {
            overlay.set_tile_layout(layout);
        }
        if let Some(painter) = self.tile_painter {
            overlay.set_tile_painter(painter);
        }
        if let Some(painter) = self.panel_painter {
            overlay.set_panel_painter(painter);
        }
    }
}

impl std::fmt::Debug for ShellBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("ShellBuilder");
        f.field("theme", &self.theme.is_some())
            .field("palettes", &self.palettes.is_some())
            .field("fonts", &self.fonts.len())
            .field("translations", &self.translations.len())
            .field("physical_mm", &self.physical_mm)
            .field("services", &self.services.is_some())
            .field("status_bar_painter", &self.status_bar_painter.is_some())
            .field("status_bar_layout", &self.status_bar_layout.is_some())
            .field("nav_bar_painter", &self.nav_bar_painter.is_some())
            .field("nav_bar_layout", &self.nav_bar_layout.is_some())
            .field("toast_painter", &self.notify_hooks.toast_painter.is_some())
            .field("toast_layout", &self.notify_hooks.toast_layout.is_some())
            .field(
                "heads_up_painter",
                &self.notify_hooks.heads_up_painter.is_some(),
            )
            .field(
                "heads_up_layout",
                &self.notify_hooks.heads_up_layout.is_some(),
            );
        #[cfg(feature = "osk")]
        f.field("osk_key_painter", &self.osk_hooks.key_painter.is_some())
            .field("osk_key_layout", &self.osk_hooks.key_layout.is_some());
        #[cfg(feature = "overlay")]
        f.field("shade_tile_layout", &self.shade_hooks.tile_layout.is_some())
            .field(
                "shade_tile_painter",
                &self.shade_hooks.tile_painter.is_some(),
            )
            .field(
                "shade_panel_painter",
                &self.shade_hooks.panel_painter.is_some(),
            );
        f.field("slot_painter", &self.slot_painter.is_some())
            .field("image_loader", &self.image_loader.is_some())
            .field("wallpaper", &self.wallpaper)
            .field("authenticator", &self.authenticator.is_some())
            .field("lock_screen_painter", &self.lock_screen_painter.is_some())
            .field(
                "unlock_prompt_painter",
                &self.unlock_prompt_painter.is_some(),
            )
            .field("recents", &self.recents)
            .field("widget_painters", &self.widget_painters)
            .field(
                "gesture_handle_painter",
                &self.gesture_handle_painter.is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl ShellBuilder {
    pub(crate) fn new(config: ShellConfig) -> Self {
        Self {
            config,
            services: None,
            theme: None,
            palettes: None,
            fonts: FontSet::new(),
            translations: Vec::new(),
            scale_policy: ScalePolicy::default(),
            metrics_spec: None,
            component_spec: None,
            control_spec: None,
            elevation_spec: None,
            physical_mm: None,
            status_bar_painter: None,
            status_bar_layout: None,
            nav_bar_painter: None,
            nav_bar_layout: None,
            notify_hooks: NotifyHooks::default(),
            #[cfg(feature = "osk")]
            osk_hooks: OskHooks::default(),
            #[cfg(feature = "overlay")]
            shade_hooks: ShadeHooks::default(),
            slot_painter: None,
            image_loader: None,
            wallpaper: None,
            authenticator: None,
            lock_screen_painter: None,
            unlock_prompt_painter: None,
            recents: crate::workspace::RecentsPainters::default(),
            widget_painters: crate::widgets::WidgetPainters::new(),
            gesture_handle_painter: None,
        }
    }

    /// **The authenticator** the unlock prompt and the lock screen ask.
    ///
    /// It replaces the reference [`PinTable`](crate::access::PinTable) that
    /// `[access.pin_table]` would build. The shell draws whatever
    /// [`Authenticator::methods`] returns and applies the answer; how a credential is stored and
    /// checked stays yours.
    ///
    /// It only shows up in `[access] mode = "prompt"` with more than one level — in `routing` the
    /// prompt is never drawn and in `off` nothing is gated.
    ///
    /// ```no_run
    /// # use fairing::access::{AuthMethod, AuthOutcome, Authenticator, Credential};
    /// # struct Badges;
    /// # impl Authenticator for Badges {
    /// #     fn methods(&self) -> Vec<AuthMethod> { Vec::new() }
    /// #     fn submit(&mut self, _: Credential, _: std::time::Instant) -> AuthOutcome { AuthOutcome::Pending }
    /// # }
    /// # let ctx = egui::Context::default();
    /// let shell = fairing::Shell::builder(fairing::ShellConfig::default())
    ///     .authenticator(Badges)
    ///     .build(&ctx)?;
    /// # let _ = shell;
    /// # Ok::<(), fairing::Error>(())
    /// ```
    #[must_use]
    pub fn authenticator(mut self, authenticator: impl Authenticator + 'static) -> Self {
        self.authenticator = Some(Box::new(authenticator));
        self
    }

    /// **Draw the lock screen yourself** (rung 5). `painter` draws what
    /// the built-in lock screen draws under the way in — the ground over the whole screen, and the
    /// time and the date. The shell draws the way in over it as before — the top line, the method
    /// tabs, the keypad or the dots, Continue where it is allowed — and keeps what is typed, the
    /// answer, the lockout, the shake and the motion. See
    /// [`LockScreenCx`](crate::access::LockScreenCx).
    #[must_use]
    pub fn lock_screen_painter(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut crate::access::LockScreenCx<'_>) + 'static,
    ) -> Self {
        self.lock_screen_painter = Some(Box::new(painter));
        self
    }

    /// **Draw the unlock prompt's backdrop and card yourself** (rung 5).
    /// `painter` is called for the backdrop over the screen and for the card under the way
    /// in — [`PromptPiece`](crate::access::PromptPiece) says which. The shell draws the way in on
    /// the card as before, and keeps what is typed, the answer, the lockout, the shake and the
    /// motion. See [`UnlockPromptCx`](crate::access::UnlockPromptCx).
    #[must_use]
    pub fn unlock_prompt_painter(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut crate::access::UnlockPromptCx<'_>) + 'static,
    ) -> Self {
        self.unlock_prompt_painter = Some(Box::new(painter));
        self
    }

    /// **Draw the recent screens' cards yourself** (rung 5). `painter`
    /// draws each card whole — what the built-in card draws as its plate, the task's icon, title,
    /// when it was used and the level it needs. The shell keeps where the cards go and the
    /// carousel's drag, the tap that brings a task forward, the throw that closes one and the
    /// screen shrinking into its card, and draws a card's split button and "Close all" over them.
    /// See [`RecentCardCx`](crate::workspace::RecentCardCx).
    #[must_use]
    pub fn recents_card_painter(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut crate::workspace::RecentCardCx<'_>) + 'static,
    ) -> Self {
        self.recents.card = Some(Box::new(painter));
        self
    }

    /// **Draw the ground under the recent screens yourself** (rung 5).
    /// `painter` draws what the built-in recent screens draw under the cards: a plain ground over
    /// the content area — fading in over the desktop, or whole behind a task's screen as it
    /// shrinks into its card. See [`RecentsGroundCx`](crate::workspace::RecentsGroundCx).
    #[must_use]
    pub fn recents_ground_painter(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut crate::workspace::RecentsGroundCx<'_>) + 'static,
    ) -> Self {
        self.recents.ground = Some(Box::new(painter));
        self
    }

    /// **Draw the widgets yourself** (rung 5) — a painter per kind,
    /// each optional ([`WidgetPainters`](crate::widgets::WidgetPainters)). Every widget the shell
    /// draws gets them: your screens' through `cx.widgets()`, and the shell's own — the settings
    /// screens, the unlock prompt's keys and buttons, the recent screens' buttons. A widget keeps
    /// its press, its value and its motion; the painter is told its look and draws it.
    #[must_use]
    pub fn widget_painters(mut self, painters: crate::widgets::WidgetPainters) -> Self {
        self.widget_painters = painters;
        self
    }

    /// **Draw the gesture handles yourself** (rung 5) — each handle's strip, and the
    /// swipe from it while there is one. The shell keeps the strips, the recognition, the gestures
    /// and what they run. See [`HandleLook`](crate::gesture::HandleLook).
    #[must_use]
    pub fn gesture_handle_painter(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut crate::gesture::HandleLook<'_>) + 'static,
    ) -> Self {
        self.gesture_handle_painter = Some(Box::new(painter));
        self
    }

    /// Inject a theme **whole**. `[shell] theme` (dark|light) and `[theme.palette]` are then
    /// ignored (with a warning logged at startup) and this value is used as it stands — every
    /// palette, metric and motion token.
    ///
    /// The bar heights have a single source in `theme.metrics.{status_bar_height,
    /// nav_bar_height}`: [`build`](Self::build) puts those values into `StatusBar::height` and
    /// `NavBar::height` (in a theme built from the config, those two already equal `[status_bar]
    /// height` and `[nav_bar] height`).
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    /// Hand over a **dark/light palette pair**. `[theme] preset` and `[theme.palette]`
    /// are ignored (with a warning at startup) and these two become the pair.
    ///
    /// A different axis from [`theme`](Self::theme) — `theme` is the one theme on screen right
    /// now, and this is **both sides** for
    /// [`Shell::set_theme_dark`](crate::Shell::set_theme_dark) to choose from. Given both, `theme`
    /// decides the first screen and this pair decides every toggle after it.
    ///
    /// ```no_run
    /// # fn main() -> fairing::Result<()> {
    /// use fairing::theme::{Palette, Preset};
    ///
    /// let builder = fairing::Shell::builder(fairing::ShellConfig::default())
    ///     .palettes(Palette::preset(Preset::Abyss, true), Palette::preset(Preset::Abyss, false));
    /// # let _ = builder;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn palettes(mut self, dark: Palette, light: Palette) -> Self {
        self.palettes = Some((dark, light));
        self
    }

    /// The fonts to load. egui's `default_fonts` has no CJK glyphs, so Hangul
    /// comes out as □ — to use it, hand a font over here. The crate embeds no font of its own.
    ///
    /// Installing happens **once**, in [`ShellBuilder::build`]. `set_fonts` rebuilds the font
    /// atlas and is a heavy call, so it is not made from the frame loop.
    ///
    /// ```no_run
    /// # fn main() -> fairing::Result<()> {
    /// use fairing::fonts::{FontSet, FontSource};
    ///
    /// let mut fonts = FontSet::new();
    /// if let Some(path) = fairing::fonts::korean_font() {
    ///     fonts.push(FontSource::from_path("ko", &path)?);
    /// }
    /// # let _ = fonts;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn fonts(mut self, fonts: FontSet) -> Self {
        self.fonts = fonts;
        self
    }

    /// Add one font (the single-item version of [`ShellBuilder::fonts`]).
    #[must_use]
    pub fn font(mut self, source: FontSource) -> Self {
        self.fonts.push(source);
        self
    }

    /// **A language's table**: a language the crate has none for, or entries for
    /// one it has — they join the built-in table and win over its own. Your screens' own strings
    /// go here too, looked up with `cx.strings.get(..)`. Call it once a language; a second call
    /// for the same one adds to the first.
    ///
    /// Korean is built in; a language written in another script needs a font that has it
    /// ([`ShellBuilder::fonts`]) — the shell warns when the loaded fonts do not.
    ///
    /// ```no_run
    /// # fn build(ctx: &egui::Context) -> fairing::Result<fairing::Shell> {
    /// use fairing::i18n::Translations;
    ///
    /// let shell = fairing::Shell::builder(fairing::ShellConfig::default())
    ///     .translations(
    ///         Translations::new("de", "Deutsch")
    ///             .entry("Settings", "Einstellungen")
    ///             .entry("Pump pressure", "Pumpendruck"),
    ///     )
    ///     .build(ctx)?;
    /// # Ok(shell)
    /// # }
    /// ```
    #[must_use]
    pub fn translations(mut self, translations: Translations) -> Self {
        self.translations.push(translations);
        self
    }

    /// The physical scale policy. Finger size, viewing magnification, the density fallback
    /// and the `ppp` range.
    ///
    /// The default is **gloved** (`finger_mm = 13.0`) — with no list of target devices and every
    /// way of operating them in play, a shell that declares nothing has to work under the hardest
    /// conditions. On a bare-finger-only device, hand over `finger_mm: 9.0`.
    ///
    /// ```no_run
    /// use fairing::unit::ScalePolicy;
    /// # let cfg = fairing::ShellConfig::default();
    /// let b = fairing::Shell::builder(cfg).scale_policy(ScalePolicy::bare());
    /// ```
    #[must_use]
    pub fn scale_policy(mut self, policy: ScalePolicy) -> Self {
        self.scale_policy = policy;
        self
    }

    /// The physical-unit metrics spec. [`MetricsSpec::default`] if not given.
    ///
    /// To keep the pre-M2b rendering as it was, hand over [`MetricsSpec::legacy_du`].
    #[must_use]
    pub fn metrics_spec(mut self, spec: MetricsSpec) -> Self {
        self.metrics_spec = Some(spec);
        self
    }

    /// The component metrics spec — a score of values like the button padding, the toast
    /// spacing and the switch height. [`ComponentSpec::default`] if not given, which is today's
    /// rendering as it stands.
    ///
    /// This is the widget's share of rung 2 of the override ladder. Without this rung,
    /// changing one button's padding would mean climbing to rung 5 (a painter) and taking the
    /// drawing over whole.
    ///
    /// ```no_run
    /// # fn build(ctx: &egui::Context) -> fairing::Result<()> {
    /// use fairing::theme::ComponentSpec;
    /// use fairing::unit::{Dim, Span};
    ///
    /// let mut spec = ComponentSpec::default();
    /// spec.button[0] = Span::fixed(Dim::mm(4.0)); // the side padding at a physical 4 mm
    /// spec.slider_track_ratio = 0.8; // a thicker track
    /// # let _ = spec;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn component_spec(mut self, spec: ComponentSpec) -> Self {
        self.component_spec = Some(spec);
        self
    }

    /// The control vocabulary spec — the lengths every drawn control shares: the mark size
    /// a switch, checkbox and radio are built from, the stroke ladder, the focus gap, the gaps.
    /// [`ControlSpec::default`] if not given.
    ///
    /// **It belongs here for the same reason the other two do.** Until it was, `theme.control` was
    /// filled once from `ControlMetrics::default()` - which resolves against `Scale::identity()`,
    /// a **gloved 13 mm** finger at 1 px per du - and never resolved again. So every control drew
    /// at the gloved size on every panel: a switch measured 7.8 mm tall whether the device declared
    /// a 9 mm bare finger or nothing at all, while the row around it followed the real scale. The
    /// shell now resolves this each frame beside `metrics_spec` and `component_spec`.
    #[must_use]
    pub fn control_spec(mut self, spec: ControlSpec) -> Self {
        self.control_spec = Some(spec);
        self
    }

    /// The **elevation** tokens — how high a container sits, and by what mechanism.
    ///
    /// Resolved every frame beside the other three, for the same reason: both of its lengths are
    /// millimetres with a device-pixel floor, and a spec frozen at `Scale::identity()` would cast
    /// the same penumbra on a 4 px/mm panel as on a 20 px/mm one.
    ///
    /// The switch a manufacturer usually wants here is
    /// [`ElevationSpec::flat`](crate::theme::ElevationSpec::flat): an e-ink or 16-level display
    /// cannot reproduce a 5 % feather and dithers it into rings, so on those panels height should
    /// not be drawn at all rather than drawn thinner. To keep the height but change how it is
    /// expressed, set `ratios.style` to
    /// [`ElevationStyle::Rim`](crate::theme::ElevationStyle::Rim) instead.
    #[must_use]
    pub fn elevation_spec(mut self, spec: ElevationSpec) -> Self {
        self.elevation_spec = Some(spec);
        self
    }

    /// Pin the panel's physical size (in mm). It wins over the backend's `DisplayInfo::physical_mm`.
    ///
    /// For a device with no EDID, or one that lies — an integrator knows their own device's inches.
    #[must_use]
    pub fn physical_mm(mut self, width_mm: f32, height_mm: f32) -> Self {
        self.physical_mm = Some((width_mm, height_mm));
        self
    }

    /// The backend bundle. Not given, it is [`Services::builder`]'s default (`SystemClock` for the
    /// clock, `Null` for the rest).
    #[must_use]
    pub fn services(mut self, services: Services) -> Self {
        self.services = Some(services);
        self
    }

    /// Draw the status bar whole. The shell keeps only the Rect, the gate decisions,
    /// the item list and taps on the bar itself, and does not call the built-in rendering — the
    /// background is the painter's too.
    #[must_use]
    pub fn status_bar_painter(
        mut self,
        painter: impl FnMut(&mut egui::Ui, &mut BarCx<'_>) + 'static,
    ) -> Self {
        self.status_bar_painter = Some(Box::new(painter));
        self
    }

    /// **Place the status bar's items yourself** (rung 4). `layout` gets
    /// the bar and the rects as the shell would lay them out — one per item it would draw, in left,
    /// centre, right order, the ones the collapse left out empty — and changes the ones it wants;
    /// the shell draws and presses the items where the rects are, and keeps the measuring, the gates,
    /// the taps and [`StatusBar::item_rect`](crate::chrome::StatusBar::item_rect). See
    /// [`StatusLayoutCx`](crate::StatusLayoutCx).
    ///
    /// A [`status_bar_painter`](Self::status_bar_painter) draws the whole bar, items and all, so
    /// with one there is nothing for a layout to place: the layout is never called, and `build`
    /// says so once in the log.
    #[must_use]
    pub fn status_bar_layout(
        mut self,
        layout: impl FnMut(&crate::StatusLayoutCx<'_>, &mut [egui::Rect]) + 'static,
    ) -> Self {
        self.status_bar_layout = Some(Box::new(layout));
        self
    }

    /// Draw the nav bar whole. Back and home are asked for by the painter through
    /// `cx.shell.back()` and `cx.shell.home()`.
    #[must_use]
    pub fn nav_bar_painter(
        mut self,
        painter: impl FnMut(&mut egui::Ui, &mut BarCx<'_>) + 'static,
    ) -> Self {
        self.nav_bar_painter = Some(Box::new(painter));
        self
    }

    /// **Place the nav bar's items yourself** (rung 4). `layout` gets
    /// the bar and the cells as the shell would lay them out — one per item, in `[nav_bar] items`
    /// order — and changes the ones it wants; the shell draws the items where the cells are, and
    /// keeps the gates, the presses, the taps and [`NavBar::item_rect`](crate::chrome::NavBar::item_rect).
    /// See [`NavLayoutCx`](crate::NavLayoutCx).
    ///
    /// A [`nav_bar_painter`](Self::nav_bar_painter) draws the whole bar, items and all, so with
    /// one there is nothing for a layout to place: the layout is never called, and `build` says so
    /// once in the log.
    #[must_use]
    pub fn nav_bar_layout(
        mut self,
        layout: impl FnMut(&crate::NavLayoutCx<'_>, &mut [egui::Rect]) + 'static,
    ) -> Self {
        self.nav_bar_layout = Some(Box::new(layout));
        self
    }

    /// **Draw each toast yourself** (rung 5). The card is the
    /// painter's whole, background included; the shell keeps the queue, the hold, the fade and
    /// rise, and the tap that puts a toast away. See [`ToastCx`](crate::notify::ToastCx) — a card
    /// that needs more height than the floor says so with `set_height`.
    #[must_use]
    pub fn toast_painter(
        mut self,
        painter: impl FnMut(&mut egui::Ui, &mut crate::notify::ToastCx<'_>) + 'static,
    ) -> Self {
        self.notify_hooks.toast_painter = Some(Box::new(painter));
        self
    }

    /// **Place the toasts yourself** (rung 4). `layout` gets the stack placed the
    /// built-in way — at rest, the oldest first — and moves what it wants; the shell adds the
    /// entry's rise on top and draws there. See [`ToastLayoutCx`](crate::notify::ToastLayoutCx).
    #[must_use]
    pub fn toast_layout(
        mut self,
        layout: impl FnMut(&crate::notify::ToastLayoutCx<'_>, &mut [egui::Rect]) + 'static,
    ) -> Self {
        self.notify_hooks.toast_layout = Some(Box::new(layout));
        self
    }

    /// **Draw the heads-up banner yourself** (rung 5). The banner is the painter's whole;
    /// the shell keeps its slide in and out, its hold, its tap (which opens the notification) and
    /// its swipe up. See [`HeadsUpCx`](crate::notify::HeadsUpCx).
    #[must_use]
    pub fn heads_up_painter(
        mut self,
        painter: impl FnMut(&mut egui::Ui, &mut crate::notify::HeadsUpCx<'_>) + 'static,
    ) -> Self {
        self.notify_hooks.heads_up_painter = Some(Box::new(painter));
        self
    }

    /// **Place the heads-up banner yourself** (rung 4). `layout` gets the banner's rect
    /// placed the built-in way, at rest, and moves or widens it; the height stays the banner's own.
    /// See [`HeadsUpLayoutCx`](crate::notify::HeadsUpLayoutCx).
    #[must_use]
    pub fn heads_up_layout(
        mut self,
        layout: impl FnMut(&crate::notify::HeadsUpLayoutCx<'_>, &mut egui::Rect) + 'static,
    ) -> Self {
        self.notify_hooks.heads_up_layout = Some(Box::new(layout));
        self
    }

    /// **Place the keyboard's keys yourself** (rung 4; feature `osk`).
    /// `layout` gets the face on show and its keys placed the built-in way — one rect per key, in
    /// reading order — and moves what it wants; a key's hit area goes with its rect. What the keys
    /// are stays the [`OskLayout`](crate::osk::OskLayout)'s. See
    /// [`OskKeyLayoutCx`](crate::osk::OskKeyLayoutCx).
    #[cfg(feature = "osk")]
    #[must_use]
    pub fn osk_key_layout(
        mut self,
        layout: impl FnMut(&crate::osk::OskKeyLayoutCx<'_>, &mut [egui::Rect]) + 'static,
    ) -> Self {
        self.osk_hooks.key_layout = Some(Box::new(layout));
        self
    }

    /// **Draw each keyboard key yourself** (rung 5, feature `osk`). The key is the
    /// painter's whole, its face and its label; the panel behind the keys, the press and what the
    /// key types stay the shell's. See [`OskKeyCx`](crate::osk::OskKeyCx).
    #[cfg(feature = "osk")]
    #[must_use]
    pub fn osk_key_painter(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut crate::osk::OskKeyCx<'_>) + 'static,
    ) -> Self {
        self.osk_hooks.key_painter = Some(Box::new(painter));
        self
    }

    /// **Place the shade's tiles yourself** (rung 4; feature
    /// `overlay`). `layout` gets the quick-settings tiles placed the built-in way, at rest — one
    /// rect per tile, in `[overlay] tiles` order — and moves what it wants; the shell draws each
    /// tile where its rect is and keeps the pull, the stop, the taps, the long presses, the gates
    /// and the row a tile opens below them. See
    /// [`ShadeTileLayoutCx`](crate::overlay::ShadeTileLayoutCx).
    #[cfg(feature = "overlay")]
    #[must_use]
    pub fn shade_tile_layout(
        mut self,
        layout: impl FnMut(&crate::overlay::ShadeTileLayoutCx<'_>, &mut [egui::Rect]) + 'static,
    ) -> Self {
        self.shade_hooks.tile_layout = Some(Box::new(layout));
        self
    }

    /// **Draw each quick-settings tile yourself** (rung 5; feature
    /// `overlay`). `painter` is called once for every tile drawn, with its rect, label, icon,
    /// kind, state and whether it is lit, live, allowed and pressed, and draws the whole tile; the
    /// shell keeps where it goes (the built-in rows or a
    /// [`shade_tile_layout`](Self::shade_tile_layout)), its tap and long press, what a tap does
    /// and who may do it, and the row a tile opens. See
    /// [`ShadeTileCx`](crate::overlay::ShadeTileCx).
    #[cfg(feature = "overlay")]
    #[must_use]
    pub fn shade_tile_painter(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut crate::overlay::ShadeTileCx<'_>) + 'static,
    ) -> Self {
        self.shade_hooks.tile_painter = Some(Box::new(painter));
        self
    }

    /// **Draw the shade panel's ground yourself** (rung 5; feature
    /// `overlay`). `painter` draws what the built-in panel draws under its content — the plate,
    /// and a card's shadow, frosted backdrop and relief — and may set the colour the
    /// notification list fades into. The shell draws the content over it as before, and keeps the
    /// pull, the reveal, the stops and every press. See
    /// [`ShadePanelCx`](crate::overlay::ShadePanelCx).
    #[cfg(feature = "overlay")]
    #[must_use]
    pub fn shade_panel_painter(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut crate::overlay::ShadePanelCx<'_>) + 'static,
    ) -> Self {
        self.shade_hooks.panel_painter = Some(Box::new(painter));
        self
    }

    /// Draw one desktop cell. The hit testing, the press decision and the gate
    /// filtering stay the shell's, and [`SlotCx`] hands over the cell Rect, the
    /// [`IconSlot`](crate::desktop::IconSlot), the press scale and whether the gate passed.
    #[must_use]
    pub fn slot_painter(
        mut self,
        painter: impl FnMut(&mut egui::Ui, SlotCx<'_>) + 'static,
    ) -> Self {
        self.slot_painter = Some(Box::new(painter));
        self
    }

    /// The loader that makes a texture from a file path. **The crate does not decode
    /// images** — this hook is where an integrator's decoder is wired into the shell.
    ///
    /// Registering one brings `[desktop] wallpaper = "file:<path>"` to life. It is the way to put a
    /// different background in each shop without rebuilding the device, so on kiosks and signage
    /// this is usually the one. Replacing it at runtime is
    /// [`Shell::load_wallpaper`](crate::Shell::load_wallpaper).
    ///
    /// ```no_run
    /// # fn main() -> fairing::Result<()> {
    /// # let (ctx, config): (egui::Context, fairing::config::ShellConfig) = todo!();
    /// let shell = fairing::Shell::builder(config)
    ///     .image_loader(|ctx: &egui::Context, path: &std::path::Path| {
    ///         // `image` is an **integrator** dependency. Any decoder will do.
    ///         let bytes = std::fs::read(path).map_err(|e| fairing::Error::Io {
    ///             path: path.display().to_string(),
    ///             message: e.to_string(),
    ///         })?;
    ///         let rgba = image::load_from_memory(&bytes)
    ///             .map_err(|e| fairing::Error::Image(e.to_string()))?
    ///             .to_rgba8();
    ///         let size = [rgba.width() as usize, rgba.height() as usize];
    ///         let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    ///         Ok(ctx.load_texture(path.to_string_lossy(), image, egui::TextureOptions::LINEAR))
    ///     })
    ///     .build(&ctx)?;
    /// # let _ = shell;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn image_loader(
        mut self,
        loader: impl Fn(&egui::Context, &std::path::Path) -> Result<egui::TextureHandle> + 'static,
    ) -> Self {
        self.image_loader = Some(Box::new(loader));
        self
    }

    /// Set the wallpaper from code. **Stronger** than `[desktop] wallpaper`.
    ///
    /// The config file is the default for when an integrator has not decided in code, and this hook
    /// covers it. To read one from a file, wire up [`ShellBuilder::image_loader`] and write
    /// `wallpaper = "file:…"` in the config, or wrap a texture you already hold in
    /// [`Wallpaper::owned`](crate::desktop::Wallpaper::owned) and hand it over here.
    ///
    /// ```no_run
    /// # fn main() -> fairing::Result<()> {
    /// use fairing::desktop::Wallpaper;
    /// # let (ctx, config): (egui::Context, fairing::config::ShellConfig) = todo!();
    /// let shell = fairing::Shell::builder(config)
    ///     .wallpaper(Wallpaper::Gradient {
    ///         top: egui::Color32::from_rgb(0x10, 0x18, 0x28),
    ///         bottom: egui::Color32::from_rgb(0x04, 0x06, 0x0A),
    ///     })
    ///     .build(&ctx)?;
    /// # let _ = shell;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn wallpaper(mut self, wallpaper: crate::desktop::Wallpaper) -> Self {
        self.wallpaper = Some(wallpaper);
        self
    }

    /// **Turn one set of the crate's default assets on in a single line**.
    ///
    /// It sets the palette pair and the wallpaper together — all it does is shorten two lines to
    /// one.
    ///
    /// | | Palette | Wallpaper |
    /// |---|---|---|
    /// | [`Preset::Base`](crate::theme::Preset::Base) | Neutral dark/light | A flat `Background` role |
    /// | [`Preset::Abyss`](crate::theme::Preset::Abyss) | The deep sea drawn from the manta art | The procedural deep sea (feature `brand`) |
    ///
    /// **The branding is opt-in.** Call none of it and there is not a trace of the crate — the
    /// default is the neutral palette on a flat background, with neither the manta nor the deep sea
    /// anywhere.
    ///
    /// To override a piece at a time, call [`ShellBuilder::palettes`] or
    /// [`ShellBuilder::wallpaper`] **after** this. The later call wins. Guide 09 §8 has a table of
    /// how to override all of it.
    ///
    /// ```no_run
    /// # fn main() -> fairing::Result<()> {
    /// use fairing::theme::Preset;
    /// # let (ctx, config): (egui::Context, fairing::config::ShellConfig) = todo!();
    /// # let (my_dark, my_light) =
    /// #     (fairing::theme::Palette::dark(), fairing::theme::Palette::light());
    /// let shell = fairing::Shell::builder(config)
    ///     .preset(Preset::Abyss)          // one set of the crate's defaults
    ///     .palettes(my_dark, my_light)    // only the colours ours (the deep sea stays)
    ///     .build(&ctx)?;
    /// # let _ = shell;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn preset(mut self, preset: crate::theme::Preset) -> Self {
        use crate::theme::{Palette, Preset};
        self.palettes = Some((
            Palette::preset(preset, true),
            Palette::preset(preset, false),
        ));
        // **Inside** the crate `#[non_exhaustive]` does not apply, so this is an exhaustive match —
        // adding a preset has the compiler catch this place.
        self.wallpaper = Some(match preset {
            Preset::Abyss => abyss_wallpaper(),
            // **Flat, for two different reasons.** Base is neutral and has no picture to draw;
            // Linen has one available and declines it, because an image behind an appliance's
            // controls competes with the room the appliance is standing in.
            Preset::Base | Preset::Linen => {
                crate::desktop::Wallpaper::Solid(crate::theme::ColorRole::Background)
            }
        });
        self
    }

    /// Build the shell. `ctx` is the context [`Waker`] will wrap.
    ///
    /// # Errors
    /// Config validation failure ([`crate::Error::Config`]) — a value out of range, two or more
    /// level tables with no `default_gate`, or a bad role name or colour format in
    /// `[theme.palette]`.
    // Every part of the shell is assembled in one place — split up, the order between the parts (fonts →
    // theme → services → the handle) is scattered across several functions and reads worse.
    #[expect(clippy::too_many_lines, reason = "as long as the part count")]
    pub fn build(self, ctx: &egui::Context) -> Result<Shell> {
        let Self {
            config,
            services,
            theme,
            palettes,
            fonts,
            translations,
            scale_policy,
            metrics_spec,
            component_spec,
            control_spec,
            elevation_spec,
            physical_mm,
            status_bar_painter,
            status_bar_layout,
            nav_bar_painter,
            nav_bar_layout,
            notify_hooks,
            #[cfg(feature = "osk")]
            osk_hooks,
            #[cfg(feature = "overlay")]
            shade_hooks,
            slot_painter,
            image_loader,
            wallpaper,
            authenticator,
            lock_screen_painter,
            unlock_prompt_painter,
            recents,
            widget_painters,
            gesture_handle_painter,
        } = self;
        config.validate()?;
        if status_bar_painter.is_some() && status_bar_layout.is_some() {
            log::warn!("status_bar_layout is never called: status_bar_painter draws the whole bar");
        }
        // Rung 5 beats rung 4: a painter draws the items wherever it likes.
        let nav_bar_layout = if nav_bar_painter.is_some() && nav_bar_layout.is_some() {
            log::warn!("nav_bar_layout is never called: nav_bar_painter draws the whole bar");
            None
        } else {
            nav_bar_layout
        };
        // **The environment is read here and nowhere else** — once, before anything is sized.
        // IO inside a frame is banned, and an environment that changed mid-run would move every
        // dimension in the shell at once.
        let env_scale = super::env_scale::EnvScale::from_env(&scale_policy);
        let scale_policy = env_scale.apply(scale_policy);
        if let Some(win) = env_scale.win {
            // Always warned, never silent: a device whose scale came from the environment rather
            // than from its own configuration is a device someone has changed by hand.
            log::warn!(
                "[scale] {} overrides the configured density (ScalePolicy::allow_env forbids it)",
                win.var()
            );
        }
        let mut access = Access::from_config(&access_config(&config))?;
        let theme_injected = theme.is_some();
        let metrics_spec = resolve_metrics_spec(metrics_spec, theme_injected, &config);
        let (theme, palettes) = resolve_theme(theme, palettes, &config)?;
        // The fonts come first — `set_fonts` throws the font atlas and the galley cache away whole, so it
        // is called once, before the style. It runs even for an empty set — binding the strong
        // family (`fonts::STRONG_FAMILY`) is part of it, and epaint panics on an unbound family.
        fonts.install(ctx);
        if !fonts.has_bold() {
            // egui's own face has one weight, so the titles come out flat and nothing on the
            // screen says why.
            log::warn!(
                "[fonts] no bold face is registered, so screen titles and other strong text draw in the regular weight. Add one with ShellBuilder::font(FontSource::from_path(\"bold\", path)?.families(FontFamilies::Strong).priority(FontPriority::First)) - fairing::fonts::strong_font() finds a system bold (guide 01 §4.2)"
            );
        }
        theme.apply(ctx);
        let mut services = services.unwrap_or_else(|| Services::builder().build());
        let waker = Waker::new(ctx);
        services.attach(&waker);
        if let Some(mut authenticator) = authenticator {
            authenticator.attach(waker.clone());
            access.install_authenticator(authenticator);
        } else if let Some(table) = access.authenticator_mut() {
            table.attach(waker.clone());
        }
        // Every session timer is measured on the shell's clock, which starts here.
        let mono = Instant::now();
        access.start_clock(mono);
        let (handle, rx) = ShellHandle::pair(waker);
        let mut desktop = DesktopView::from_config(&config.desktop);
        load_file_wallpaper(ctx, &config.desktop, &mut desktop, image_loader.as_deref());
        // A wallpaper given in code is stronger than the config — `[desktop] wallpaper` is the default for
        // when the integrator did not decide.
        if let Some(wallpaper) = wallpaper {
            desktop.set_wallpaper(wallpaper);
        }
        if let Some(painter) = slot_painter {
            desktop.set_slot_painter(painter);
        }
        let mut status_bar = StatusBar::from_config(&config.status_bar);
        if let Some(layout) = status_bar_layout {
            status_bar.set_layout(layout);
        }
        // `[status_bar] clock_format` names the clock's **shape**, and the hour convention
        // that comes with it is the device's starting point. The `ui.clock_12h` setting is what the
        // owner turns afterwards, so it is seeded from the config here rather than defaulting to
        // 24-hour — otherwise `clock_format = "hm12"` would be quietly overruled on the first frame
        // by a setting nobody had written.
        let mut settings = SettingsView::default();
        settings.set(
            crate::settings::keys::UI_CLOCK_12H,
            crate::settings::SettingValue::Bool(
                status_bar
                    .configured_clock()
                    .is_some_and(crate::time::ClockFormat::hour12),
            ),
        );
        let mut nav_bar = NavBar::from_config(&config.nav_bar);
        // The single source of the bar heights is the theme tokens (with a theme built from the config, the values are the same).
        status_bar.height = theme.metrics.status_bar_height;
        nav_bar.height = theme.metrics.nav_bar_height;
        let tokens = theme.motion;
        let gestures = gesture_engine(&config, &tokens);
        let guard = PolicyDriver::new(
            Duration::from_millis(config.gesture.emergency_ms),
            config.gesture.emergency_corner_px,
        );
        #[cfg_attr(not(feature = "overlay"), allow(unused_mut))]
        let mut overlay =
            OverlaySlot::from_config(&config.overlay, config.notify.dismiss_button, &tokens);
        #[cfg(feature = "overlay")]
        shade_hooks.install(&mut overlay.0);
        #[cfg_attr(not(feature = "osk"), allow(unused_mut))]
        let mut osk = OskSlot::from_config(&config.osk);
        #[cfg(feature = "osk")]
        osk_hooks.install(&mut osk.0);
        let notifications = NotificationCenter::new(usize::from(config.notify.max_items));
        let mut toasts = ToastQueue::new(
            usize::from(config.notify.max_visible),
            Duration::from_millis(config.notify.toast_ms),
        );
        let mut heads_up = HeadsUp::new(theme.metrics.heads_up_height);
        notify_hooks.install(&mut toasts, &mut heads_up);
        let theme_fade = ThemeFade::idle(theme.palette);
        // The config was validated above, so the axis reads; `auto` where it somehow does not.
        let mut workspace = Workspace::new();
        workspace.set_split_axis(config.workspace.axis().ok().flatten());
        let back_edges = config
            .nav_bar
            .back_edges
            .iter()
            .filter_map(|e| crate::gesture::Edge::parse(e))
            .fold(
                crate::gesture::EdgeMask::NONE,
                crate::gesture::EdgeMask::with,
            );
        let strings = strings_for(&config.shell.locale, translations);
        let mut prompt = crate::access::prompt::Prompt::default();
        if let Some(painter) = lock_screen_painter {
            prompt.set_lock_screen_painter(painter);
        }
        if let Some(painter) = unlock_prompt_painter {
            prompt.set_unlock_prompt_painter(painter);
        }
        Ok(Shell {
            desktop,
            status_bar,
            nav_bar,
            strings,
            script_checked: None,
            config,
            theme,
            // The builder just called `theme.apply(ctx)`, so egui is already in step.
            theme_dirty: false,
            palettes,
            hidden: Vec::new(),
            hidden_remaining: std::collections::BTreeMap::new(),
            tap_press: None,
            gesture_regions: Vec::new(),
            region_zones: crate::gesture::RegionZones::NONE,
            region_track: None,
            region_out: crate::gesture::RegionOut::default(),
            last_screen: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 480.0)),
            services,
            access,
            registry: Registry::new(),
            workspace,
            animations: AnimationStore::new(),
            icons: IconSet::new(),
            settings,
            handle,
            rx,
            held_command: None,
            events: Vec::new(),
            requests: Vec::new(),
            layout: Layout::default(),
            frame_no: 0,
            last_dt: 0.0,
            mono,
            stale_wake_frames: 0,
            font_waits: 0,
            status_bar_painter,
            nav_bar_painter,
            nav_bar_layout,
            recents_painters: recents,
            widget_painters,
            handle_painter: gesture_handle_painter,
            image_loader,
            nav_item_checks: Vec::new(),
            last_touch: None,
            mouse_active: false,
            bar_rows: Vec::new(),
            scale_policy,
            metrics_spec,
            component_spec: component_spec
                .or_else(|| (!theme_injected).then(ComponentSpec::default)),
            control_spec: control_spec.or_else(|| (!theme_injected).then(ControlSpec::default)),
            elevation_spec: elevation_spec
                .or_else(|| (!theme_injected).then(ElevationSpec::default)),
            // The integrator's pin still beats the environment — see `env_scale`'s module docs on
            // why that way round. `env_px_per_mm` carries `FAIRING_PPI`, which is already a
            // density; `FAIRING_PHYSICAL_MM` joins the pin's own slot and is told apart by
            // `env_physical_mm`, because both need the root rect before they are one.
            physical_mm_pin: physical_mm.or_else(|| env_scale.physical_mm()),
            env_physical_mm: physical_mm.is_none() && env_scale.physical_mm().is_some(),
            env_px_per_mm: env_scale.px_per_mm(),
            scale: crate::unit::Scale::identity(),
            committed_ppp: 1.0,
            gestures,
            guard,
            overlay,
            osk,
            notifications,
            toasts,
            heads_up,
            theme_fade,
            overlay_was_closed: true,
            focus_covered: false,
            prompt,
            after_lock: None,
            osk_was_shown: false,
            split_was: false,
            bottom_held: false,
            back_edges,
        })
    }
}

/// The string table: the built-in ones and the integrator's, set to `[shell] locale`.
fn strings_for(locale: &str, translations: Vec<Translations>) -> Strings {
    let strings = translations
        .into_iter()
        .fold(Strings::new(locale), Strings::with);
    if !strings.has_locale(locale) {
        log::warn!(
            "[shell] locale = \"{locale}\" has no table - the text stays in English (add one with ShellBuilder::translations)"
        );
    }
    strings
}

/// `[access]` as the session reads it: `[shell] idle_lock_secs`, the key's old home, still counts
/// where `[access]` leaves it at 0 — with a warning, so the config gets moved.
fn access_config(config: &ShellConfig) -> crate::config::AccessConfig {
    let mut access = config.access.clone();
    let old = config.shell.idle_lock_secs;
    if old > 0 {
        if access.idle_lock_secs == 0 {
            log::warn!(
                "[shell] idle_lock_secs = {old} is read as [access] idle_lock_secs - move it there"
            );
            access.idle_lock_secs = old;
        } else if access.idle_lock_secs != old {
            log::warn!(
                "[shell] idle_lock_secs = {old} is ignored - [access] idle_lock_secs = {} is the key",
                access.idle_lock_secs
            );
        }
    }
    access
}

/// Settle the one theme and the palette pair. There are only three branches —
///
/// | Input | The pair | What happens to the config |
/// |---|---|---|
/// | [`ShellBuilder::palettes`] | Exactly as given | `[theme] preset` · `[theme.palette]` ignored, with a warning |
/// | [`ShellBuilder::theme`] | `(t.palette, t.palette)` | `warn_ignored_theme_config` warns |
/// | From the config | `(preset(p, true), preset(p, false))` with `[theme.palette]` applied to **both** | — |
///
/// The second branch, where both sides of the pair become the same, is what fixes the old bug — an
/// injected palette no longer vanishes into the neutral one at the first theme toggle.
///
/// Injecting a theme leaves the config's theme setting doing nothing, and ignoring that silently
/// would leave only "why is the colour not changing?" behind (the surface-the-error
/// principle) — hence the warning.
fn resolve_theme(
    theme: Option<Theme>,
    palettes: Option<(Palette, Palette)>,
    config: &ShellConfig,
) -> Result<(Theme, (Palette, Palette))> {
    let injected = theme.is_some();
    let theme = match theme {
        Some(theme) => {
            warn_ignored_theme_config(config);
            theme
        }
        None => config.build_theme()?,
    };
    let pair = match palettes {
        Some(pair) => {
            log::warn!(
                "a palette pair was injected, so [theme] preset and [theme.palette] are ignored"
            );
            pair
        }
        // With a theme injected, its palette is both sides — a toggle does not overturn the colours.
        None if injected => (theme.palette, theme.palette),
        None => {
            let preset = crate::theme::Preset::parse(&config.theme.preset).ok_or_else(|| {
                // Read from `Preset::ALL`, so a new preset cannot be left out of the message.
                let known: Vec<&str> = crate::theme::Preset::ALL
                    .iter()
                    .map(|preset| preset.as_str())
                    .collect();
                crate::Error::Config(format!(
                    "[theme] preset = \"{}\" is not a known preset ({})",
                    config.theme.preset,
                    known.join(" | ")
                ))
            })?;
            let mut dark = Palette::preset(preset, true);
            let mut light = Palette::preset(preset, false);
            dark.apply_overrides(&config.theme.palette)?;
            light.apply_overrides(&config.theme.palette)?;
            (dark, light)
        }
    };
    Ok((theme, pair))
}

fn warn_ignored_theme_config(config: &ShellConfig) {
    log::warn!(
        "a Theme was injected, so [shell] theme = \"{}\" is ignored",
        config.shell.theme
    );
    if !config.theme.palette.is_empty() {
        log::warn!(
            "a Theme was injected, so the {} [theme.palette] overrides are ignored too",
            config.theme.palette.len()
        );
    }
}

/// Assemble the gesture engine (which keeps `build` under 100 lines — clippy `too_many_lines`).
/// The values come from two places only: the motion tokens and `[gesture]`.
fn gesture_engine(config: &ShellConfig, tokens: &crate::theme::MotionTokens) -> GestureEngine {
    GestureEngine::new(GestureTuning {
        slop_px: tokens.slop_px,
        tap: tokens.tap,
        long_press: tokens.long_press,
        hold: Duration::from_millis(config.gesture.hold_ms),
        fling_px_s: tokens.fling_px_s,
        enabled: config.gesture.enabled,
    })
}

/// Which metrics spec to use (the ladder).
///
/// With a theme injected whole, its `metrics` wins — theme injection is a rung above tokens on the
/// ladder. Given `metrics_spec` as well, that one wins again. `None` means "the shell does not
/// touch it".
fn resolve_metrics_spec(
    spec: Option<MetricsSpec>,
    theme_injected: bool,
    config: &ShellConfig,
) -> Option<MetricsSpec> {
    let pinned = pinned_bar_sizes(config);
    match (spec, theme_injected) {
        (Some(spec), _) => {
            warn_ignored_bar_sizes(&pinned, "ShellBuilder::metrics_spec sets them");
            Some(spec)
        }
        (None, true) => {
            warn_ignored_bar_sizes(&pinned, "an injected Theme sets them");
            None
        }
        // The crate's spec, with what `[status_bar]`/`[nav_bar]` pin laid over it — the TOML used
        // to be overwritten by these defaults every frame and so did nothing.
        (None, false) => {
            let mut spec = MetricsSpec::default();
            let du = |v: f32| Span::fixed(Dim::du(v));
            if let Some(h) = config.status_bar.height {
                spec.status_bar_height = du(h).pinned();
            }
            if let Some(s) = config.status_bar.icon_size {
                spec.status_icon_size = du(s).pinned();
            }
            if let Some(h) = config.nav_bar.height {
                spec.nav_bar_height = du(h).pinned();
            }
            Some(spec)
        }
    }
}

/// The `[status_bar] height`, `[status_bar] icon_size` and `[nav_bar] height` the TOML gives.
fn pinned_bar_sizes(config: &ShellConfig) -> Vec<&'static str> {
    [
        (config.status_bar.height, "[status_bar] height"),
        (config.status_bar.icon_size, "[status_bar] icon_size"),
        (config.nav_bar.height, "[nav_bar] height"),
    ]
    .into_iter()
    .filter_map(|(value, key)| value.map(|_| key))
    .collect()
}

/// Code is the higher rung of the ladder: what it sets, the TOML does not — but the
/// TOML is not silently dropped either.
fn warn_ignored_bar_sizes(keys: &[&str], why: &str) {
    if !keys.is_empty() {
        log::warn!("{} ignored: {why}", keys.join(", "));
    }
}

/// Read `[desktop] wallpaper = "file:…"` through the integrator's loader and put it up as the
/// wallpaper.
///
/// **The shell comes up even on failure.** A device must not fail to start over one background, so
/// the error is logged and the `background` fallback `wallpaper_from_config` set is left as it is.
///
/// The decode happens **once**, here — outside the frame loop, so even a large original is done
/// before the first screen appears.
fn load_file_wallpaper(
    ctx: &egui::Context,
    cfg: &crate::config::DesktopConfig,
    view: &mut DesktopView,
    loader: Option<&crate::desktop::ImageLoaderFn>,
) {
    let Some(path) = cfg.wallpaper.strip_prefix("file:") else {
        return;
    };
    let Some(loader) = loader else {
        log::warn!(
            "[desktop] wallpaper = \"{}\" needs a `ShellBuilder::image_loader` - \
             using background instead (the crate does not decode images, guide 09 §1)",
            cfg.wallpaper
        );
        return;
    };
    let fit = crate::desktop::Fit::parse(&cfg.wallpaper_fit).unwrap_or_else(|| {
        log::warn!(
            "[desktop] wallpaper_fit = \"{}\" is none of cover, contain or stretch - using cover",
            cfg.wallpaper_fit
        );
        crate::desktop::Fit::Cover
    });
    match loader(ctx, std::path::Path::new(path)) {
        Ok(texture) => view.set_wallpaper(crate::desktop::Wallpaper::owned(texture, fit)),
        Err(e) => log::error!(
            "[desktop] wallpaper = \"file:{path}\" could not be read: {e} - falling back to background"
        ),
    }
}

/// The wallpaper [`ShellBuilder::preset`] uses for [`Preset::Abyss`](crate::theme::Preset::Abyss).
///
/// With the `brand` feature off, the drawing code is compiled out, so it warns and falls back to a
/// flat colour — the config and the palette are outside the feature gate, so the
/// colours survive.
fn abyss_wallpaper() -> crate::desktop::Wallpaper {
    #[cfg(feature = "brand")]
    {
        crate::brand::abyss_wallpaper(crate::brand::AbyssParams::default())
    }
    #[cfg(not(feature = "brand"))]
    {
        log::warn!(
            "preset is Abyss but the `brand` feature is off - keeping the palette, the background stays flat"
        );
        crate::desktop::Wallpaper::Solid(crate::theme::ColorRole::Background)
    }
}
