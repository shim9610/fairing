//! `ShellConfig` — the M1 sections of `fairing.toml` (`[shell]`, `[status_bar]`, `[nav_bar]`,
//! `[theme]`, `[desktop]`, `[motion]`, `[access]`), with loading and defaults.
//!
//! Configuration handles **placement and assignment only**. Declarations (screens, actions,
//! status items) live in code, and an id the config refers to that does not exist in code is
//! logged as a warning and ignored. The M2 sections (`[overlay]`, `[notify]`,
//! `[osk]`, `[gesture]`) are parsed whatever the features are — a section for a subsystem that
//! is off is read and ignored. A key this release does not know is ignored (serde's default).

use crate::error::{Error, Result};
use serde::Deserialize;

/// **The `[theme]` and `[motion]` sections live in [`fairing_widgets::config`]** — they
/// describe how an element looks and how long it takes, which is the element crate's business, and
/// the types that read them are there too. Re-exported so `fairing::config::MotionConfig` still
/// resolves and a config file needs no change.
pub use fairing_widgets::config::{
    DurationConfig, HomeConfig, MotionConfig, OskMotionConfig, OverviewMotionConfig,
    PageMotionConfig, PanesMotionConfig, PressConfig, PushConfig, ShadeConfig, SpringConfig,
    ThemeConfig, ToastMotionConfig,
};
use std::collections::BTreeMap;
use std::path::Path;

/// The whole of `fairing.toml`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ShellConfig {
    /// `[shell]`.
    pub shell: ShellSection,
    /// `[status_bar]`.
    pub status_bar: StatusBarConfig,
    /// `[nav_bar]`.
    pub nav_bar: NavBarConfig,
    /// `[theme]`.
    pub theme: ThemeConfig,
    /// `[desktop]`.
    pub desktop: DesktopConfig,
    /// `[motion]`.
    pub motion: MotionConfig,
    /// `[access]`.
    pub access: AccessConfig,
    /// `[overlay]` (M2).
    pub overlay: OverlayConfig,
    /// `[notify]` (M2).
    pub notify: NotifyConfig,
    /// `[osk]` (M2).
    pub osk: OskConfig,
    /// `[gesture]` (M2).
    pub gesture: GestureConfig,
    /// `[workspace]` (M5) — the split panes and the overview.
    pub workspace: WorkspaceConfig,
}

impl ShellConfig {
    /// The [`Theme`](crate::theme::Theme) these settings describe.
    ///
    /// The theme lives in the element crate and takes its five inputs directly
    /// ([`Theme::from_inputs`](crate::theme::Theme::from_inputs)), because it has no business
    /// knowing what else is in the shell's config file. This is the one-argument form
    /// for a caller that has the whole file in hand.
    ///
    /// # Errors
    /// [`Error::Config`] if `[theme] preset` is an unknown name, or a `[theme.palette]` role name
    /// or colour is wrong.
    pub fn build_theme(&self) -> crate::Result<crate::theme::Theme> {
        // Unset sizes start from the du defaults; the shell resolves the physical ones on its
        // first frame.
        let du = crate::theme::Metrics::default();
        crate::theme::Theme::from_inputs(&crate::theme::ThemeInputs {
            theme_name: &self.shell.theme,
            theme: &self.theme,
            motion: &self.motion,
            status_bar_height: self.status_bar.height.unwrap_or(du.status_bar_height),
            status_icon_size: self.status_bar.icon_size.unwrap_or(du.status_icon_size),
            nav_bar_height: self.nav_bar.height.unwrap_or(du.nav_bar_height),
        })
    }

    /// Build one from a TOML string.
    ///
    /// # Errors
    /// [`Error::Config`] on a TOML syntax error or a type mismatch.
    pub fn from_toml(text: &str) -> Result<Self> {
        let config: Self = toml::from_str(text).map_err(|err| Error::Config(err.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Read from a file. **A file that is not there means the defaults.** Called once at startup —
    /// there is no file IO inside the frame loop.
    ///
    /// # Errors
    /// [`Error::Io`] if the file is there but cannot be read; [`Error::Config`] if its contents
    /// are wrong.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            log::info!("{} is missing - using the default config", path.display());
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path).map_err(|err| Error::Io {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
        Self::from_toml(&text)
    }

    /// Validate the ranges and the cross-constraints. The access-control level table,
    /// `default_gate` and `pin_table` are validated by
    /// [`crate::access::Access::from_config`], and the `[theme.palette]` role names and colour
    /// formats by [`ShellConfig::build_theme`] — each by whichever side knows those names. All
    /// three are called at startup by [`crate::Shell::builder`].
    ///
    /// # Errors
    /// [`Error::Config`] on a value out of range.
    pub fn validate(&self) -> Result<()> {
        // 0 means "automatic". With no cap, a value like `columns = 255, rows = 255` takes a 60-thousand-
        // cell `Vec` per page and the render path walks it every frame.
        if self.desktop.columns > crate::desktop::MAX_COLUMNS {
            return Err(Error::Config(format!(
                "[desktop] columns must be 0 (auto) or at most {}",
                crate::desktop::MAX_COLUMNS
            )));
        }
        if self.desktop.rows > crate::desktop::MAX_ROWS {
            return Err(Error::Config(format!(
                "[desktop] rows must be 0 (auto) or at most {}",
                crate::desktop::MAX_ROWS
            )));
        }
        // The three sizes are pinned straight into the metrics, so `nan` and `inf` —
        // both valid TOML floats — would reach the layout. They are refused with the rest.
        let bad_size = |v: Option<f32>| v.is_some_and(|v| !v.is_finite() || v <= 0.0);
        if self.status_bar.enabled && bad_size(self.status_bar.height) {
            return Err(Error::Config(
                "[status_bar] height must be a number greater than 0".to_owned(),
            ));
        }
        if bad_size(self.status_bar.icon_size) {
            return Err(Error::Config(
                "[status_bar] icon_size must be a number greater than 0".to_owned(),
            ));
        }
        if self.nav_bar.enabled && bad_size(self.nav_bar.height) {
            return Err(Error::Config(
                "[nav_bar] height must be a number greater than 0".to_owned(),
            ));
        }
        self.validate_motion()?;
        self.validate_chrome()?;
        self.workspace.axis().map(|_| ())
    }

    /// Validate the `[motion]` ranges.
    fn validate_motion(&self) -> Result<()> {
        let m = &self.motion;
        if m.spring.k <= 0.0 || m.spring.c < 0.0 {
            return Err(Error::Config(
                "[motion.spring] k must be greater than 0 and c at least 0".to_owned(),
            ));
        }
        if !(0.0..=1.0).contains(&m.snap_ratio) {
            return Err(Error::Config(
                "[motion] snap_ratio must be within 0..=1".to_owned(),
            ));
        }
        if m.fling_px_s < 0.0 || m.slop_px < 0.0 {
            return Err(Error::Config(
                "[motion] fling_px_s and slop_px cannot be negative".to_owned(),
            ));
        }
        if !(0.0..=1.0).contains(&m.push.parallax) || !(0.0..=1.0).contains(&m.push.dim) {
            return Err(Error::Config(
                "[motion.push] parallax and dim must be within 0..=1".to_owned(),
            ));
        }
        if !(f32::EPSILON..=1.0).contains(&m.home.desktop_scale) {
            return Err(Error::Config(
                "[motion.home] desktop_scale must be greater than 0 and at most 1".to_owned(),
            ));
        }
        if !(f32::EPSILON..=1.0).contains(&m.press.scale) {
            return Err(Error::Config(
                "[motion.press] scale must be greater than 0 and at most 1".to_owned(),
            ));
        }
        if !(0.0..=1.0).contains(&m.shade.snap_ratio) || m.shade.rubber < 0.0 {
            return Err(Error::Config(
                "[motion.shade] snap_ratio must be within 0..=1 and rubber at least 0".to_owned(),
            ));
        }
        if m.page.fling_px_s < 0.0 || m.page.rubber < 0.0 || m.page.rubber_max < 0.0 {
            return Err(Error::Config(
                "[motion.page] fling_px_s, rubber and rubber_max cannot be negative".to_owned(),
            ));
        }
        Ok(())
    }

    /// Validate `[overlay]`, `[osk]`, `[notify]` and `[nav_bar]` (M2).
    fn validate_chrome(&self) -> Result<()> {
        if !(f32::EPSILON..=1.0).contains(&self.overlay.max_height_ratio) {
            return Err(Error::Config(
                "[overlay] max_height_ratio must be greater than 0 and at most 1".to_owned(),
            ));
        }
        if !(0.0..=1.0).contains(&self.overlay.card_glass) {
            return Err(Error::Config(
                "[overlay] card_glass must be within 0..=1".to_owned(),
            ));
        }
        if !(0.0..=2.0).contains(&self.overlay.card_relief) {
            return Err(Error::Config(
                "[overlay] card_relief must be within 0..=2".to_owned(),
            ));
        }
        if !(self.overlay.split_ratio > 0.0 && self.overlay.split_ratio < 1.0) {
            return Err(Error::Config(
                "[overlay] split_ratio must be between 0 and 1, exclusive".to_owned(),
            ));
        }
        if !(f32::EPSILON..=1.0).contains(&self.overlay.card_width_ratio) {
            return Err(Error::Config(
                "[overlay] card_width_ratio must be greater than 0 and at most 1".to_owned(),
            ));
        }
        if self.overlay.tile_columns == 0 {
            return Err(Error::Config(
                "[overlay] tile_columns must be at least 1".to_owned(),
            ));
        }
        if !(f32::EPSILON..=1.0).contains(&self.osk.height_ratio) || self.osk.min_key_px <= 0.0 {
            return Err(Error::Config(
                "[osk] height_ratio must be greater than 0 and at most 1, and min_key_px greater than 0"
                    .to_owned(),
            ));
        }
        if self.notify.max_visible == 0 {
            return Err(Error::Config(
                "[notify] max_visible must be at least 1".to_owned(),
            ));
        }
        for edge in &self.nav_bar.back_edges {
            if edge != "left" && edge != "right" {
                return Err(Error::Config(format!(
                    "[nav_bar] back_edges: `{edge}` must be \"left\" or \"right\""
                )));
            }
        }
        // **A device with no back at all is not allowed to boot**. `gesture` has no back
        // button — back is a `back_edges` swipe — so with `back_edges` empty there is no way out of
        // the stack at all, the kind of accident found only after shipping. A bar turned off
        // (`enabled = false`) does not catch here: it is normal for the screen to draw its own back then
        // (as the kiosk example does), and the style value means nothing.
        if self.nav_bar.enabled
            && self.nav_bar.style == "gesture"
            && self.nav_bar.back_edges.is_empty()
        {
            return Err(Error::Config(
                "[nav_bar] style = \"gesture\" has no back button, so an empty back_edges leaves no way back. \
                 Give it an edge (\"left\" or \"right\") or set style to \
                 \"buttons\""
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// `[shell]`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct ShellSection {
    /// The locale (`"en"`, `"ko"`).
    pub locale: String,
    /// `"dark"` | `"light"`.
    pub theme: String,
    /// **Deprecated — the key is `[access] idle_lock_secs`.** Read in its place when that one is
    /// 0, with a warning at start-up.
    pub idle_lock_secs: u64,
    /// `"reactive"` or `"continuous"` (for debugging).
    pub repaint: RepaintMode,
    /// The mouse cursor policy: `"auto"` (default), `"hidden"` or `"visible"`.
    pub cursor: CursorPolicy,
}

impl Default for ShellSection {
    fn default() -> Self {
        Self {
            locale: "en".to_owned(),
            theme: "dark".to_owned(),
            idle_lock_secs: 0,
            repaint: RepaintMode::Reactive,
            cursor: CursorPolicy::Auto,
        }
    }
}

/// The mouse cursor policy (how it is operated is a runtime axis).
///
/// **A touch device must not have an arrow floating on it.** But every kind of input is in
/// scope (capacitive, resistive, gloved, a trackball, an industrial HMI with a USB mouse), so
/// the cursor cannot simply be removed either. The default therefore **follows the last input
/// used**.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorPolicy {
    /// Follow the last input — hidden after touch, visible after the mouse. **The default.**
    ///
    /// Straight after boot, with no input yet, it is **hidden**. Most devices are touch, so
    /// appearing only when a mouse is plugged in is the better way round.
    #[default]
    Auto,
    /// Always hidden. Not even a plugged-in mouse shows it — for pinning the device to touch only.
    Hidden,
    /// Always visible. During development, or on a device where the mouse is the main input.
    Visible,
}

/// The repaint policy.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RepaintMode {
    /// Draw only on input, a wake or an animation (the default).
    #[default]
    Reactive,
    /// Draw every frame (for debugging).
    Continuous,
}

/// The Abyss background quality tier (`[desktop.abyss] tier`). **Outside the
/// feature gate** (rule G) — the config parses with `brand` off.
///
/// **Quality is not derived from the resolution.** Fill rate scales with resolution, but on this
/// crate's target hardware GPU performance does not — the smaller the industrial panel, the
/// weaker its GPU. Choosing by resolution would hand an 800×480 software renderer the heaviest
/// tier. The right answer is the integrator, not an automatic one.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AbyssTier {
    /// The water column and the veil only (~56 vertices). Software renderers, fbdev.
    Flat,
    /// Plus caustics, the reef and the hero manta (~256 vertices). **The default.**
    #[default]
    Lite,
    /// All of it (~870 vertices).
    Full,
}

/// `[desktop.abyss]` — the Abyss procedural background. **Outside the feature
/// gate** (rule G): with `brand` off it parses and is ignored.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct AbyssConfig {
    /// The quality tier.
    pub tier: AbyssTier,
    /// The scatter seed. **The composition does not move** — only the bubbles, fish and rocks
    /// do; the manta formation, the ray count, the light position and the band heights are
    /// authored constants.
    pub seed: u32,
    /// The god ray count (0..=16).
    pub rays: u8,
    /// The bubble count (0..=64).
    pub bubbles: u8,
    /// The fish count (0..=64).
    pub fish: u8,
    /// The manta count (0..=6). At 1, only the hero.
    pub mantas: u8,
    /// The light's horizontal position (as a fraction of the screen width).
    pub light_x: f32,
    /// The top legibility veil's alpha.
    pub veil_top: f32,
    /// The veil alpha over the icon grid. **The labels are in the middle of the screen** —
    /// dropping it to 0 and losing them is the integrator's choice.
    pub veil_field: f32,
    /// The veil alpha over the bottom band (the dock and the indicator).
    pub veil_bottom: f32,
    /// Move the mantas every frame. **This breaks zero fps at rest** — `false` by default.
    pub animate: bool,
    /// The bake budget (ms). Over it, the tier drops one step with a warning.
    pub bake_budget_ms: f32,
    /// Where the colours come from: `"palette"` (the default, following the theme) or `"art"`
    /// (fixed colours measured from the reference art).
    ///
    /// The mid-water in the art is blue while the palette's `primary` is cyan, so no derivation
    /// reaches the art's colour. When the art itself is what you need, use
    /// `"art"`. An unknown name warns and falls back to `palette` — the background is
    /// fail-open.
    pub colors: String,
}

impl Default for AbyssConfig {
    fn default() -> Self {
        Self {
            tier: AbyssTier::Lite,
            seed: 0xFA12_1234,
            rays: 10,
            bubbles: 32,
            fish: 40,
            mantas: 5,
            light_x: 0.50,
            veil_top: 0.22,
            veil_field: 0.18,
            veil_bottom: 0.16,
            animate: false,
            bake_budget_ms: 1.5,
            colors: "palette".to_owned(),
        }
    }
}

/// `[status_bar]`. The slot lists are declaration ids.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct StatusBarConfig {
    /// On or off.
    pub enabled: bool,
    /// The height in du, **pinned**. Unset (the default), it is the crate's physical height —
    /// 7 mm, at least 32 du.
    ///
    /// Set, it replaces that one value of the crate's metrics spec. An integrator's own
    /// [`ShellBuilder::metrics_spec`](crate::ShellBuilder::metrics_spec) or
    /// [`ShellBuilder::theme`](crate::ShellBuilder::theme) wins over it — the code is the higher
    /// rung of the ladder — and `build` warns that it was ignored.
    pub height: Option<f32>,
    /// The left slot's ids, in order.
    pub left: Vec<String>,
    /// The centre slot.
    pub center: Vec<String>,
    /// The right slot.
    pub right: Vec<String>,
    /// The status icons' size in du, **pinned**. Unset (the default), it is the crate's physical
    /// size — 3.6 mm, at least 18 du. The same rule as [`height`](Self::height).
    pub icon_size: Option<f32>,
    /// A palette role name, or `#RRGGBB`.
    pub icon_color: String,
    /// Whether a tap on the status bar toggles the shade (M2).
    pub tap_opens_shade: bool,
    /// The clock **shape**: `"hm"`, `"hms"`, `"date_hm"`, or their 12-hour spellings `"hm12"`,
    /// `"hms12"`, `"date_hm12"`.
    ///
    /// The hour convention here is only the starting point. The device owner's
    /// [`keys::UI_CLOCK_12H`](crate::settings::keys::UI_CLOCK_12H) setting — the "24-hour time"
    /// switch on `settings.datetime` — turns whichever shape this names into its other half while
    /// the shell runs. `"hms"` and `"hms12"` therefore differ only in what is drawn
    /// before anyone touches that switch.
    pub clock_format: String,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            height: None,
            left: vec!["status.clock".to_owned()],
            center: Vec::new(),
            // M2: `status.notifications` (the bell plus the unread badge, a tap = the shade) is first in
            // the default right slot. The gate is the id, as for every built-in item.
            right: vec![
                "status.notifications".to_owned(),
                "status.bluetooth".to_owned(),
                "status.wifi".to_owned(),
                "status.battery".to_owned(),
            ],
            icon_size: None,
            icon_color: "on_surface".to_owned(),
            tap_opens_shade: true,
            clock_format: "hm".to_owned(),
        }
    }
}

/// `[nav_bar]`. M1 implements `buttons` only.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct NavBarConfig {
    /// On or off. `false` draws no bar and the content grows by that much (edge gestures are M2).
    pub enabled: bool,
    /// `"buttons"` | `"gesture"` — the bottom edge's swipes in place of the buttons.
    /// Gestures come with the bar: with `enabled = false` the style means nothing.
    pub style: String,
    /// The button items: `"back"`, `"home"`, `"recents"`, `"split"`, or a `nav_item` declaration id.
    pub items: Vec<String>,
    /// The height in du, **pinned**. Unset (the default), it is the crate's physical height — one
    /// finger plus 8 du, at least 56 du. The same rule as
    /// [`StatusBarConfig::height`].
    pub height: Option<f32>,
    /// The edges that accept a back gesture (`nav.back_edges`, A3): `"left"` or
    /// `"right"`. Empty means no gesture back. In M2 it works in the `Buttons` style too.
    pub back_edges: Vec<String>,
}

impl Default for NavBarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            style: "buttons".to_owned(),
            items: vec!["back".to_owned(), "home".to_owned(), "recents".to_owned()],
            height: None,
            back_edges: vec!["left".to_owned()],
        }
    }
}

/// `[desktop]`. Icons come from code declarations; this only overrides position and label.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct DesktopConfig {
    /// The column count. 0 derives it from the cell size.
    pub columns: u8,
    /// The row count. 0 is automatic.
    pub rows: u8,
    /// The dock's item ids.
    ///
    /// **There is no count limit**. Five is the phone convention, not a device
    /// requirement — twelve along the bottom of a 1920 bar panel is normal, and so is eight in a
    /// left rail on 480×800. Cells shrink only once they fall below the physical minimum touch
    /// size.
    pub dock: Vec<String>,
    /// Which edge the dock attaches to: `"bottom"` (the default), `"top"`, `"left"` or
    /// `"right"`.
    ///
    /// A band along the bottom is the phone convention. A portrait panel may want a side rail,
    /// and a wide instrument screen a row along the top.
    pub dock_edge: String,
    /// Set this and the dock does not attach to an edge but becomes **a band across the
    /// desktop**. The value is its centre position relative to the content (0..=1). The band's
    /// background is not painted, so only the icons float over the wallpaper.
    pub dock_band: Option<f32>,
    /// `[[desktop.pages]]`.
    pub pages: Vec<PageConfig>,
    /// The label line count.
    pub label_lines: u8,
    /// The background: a palette role name (`"background"`), `#RRGGBB`, `"abyss"` (the
    /// procedural deep sea), or `"file:<path>"` (a raster image).
    ///
    /// `"abyss"` warns and falls back to the `background` role when the `brand` feature is off.
    ///
    /// `"file:…"` is only read with the `raster` feature on — it is the way to give each site a
    /// different background **without rebuilding the device** (guide 09 §2). Use an absolute
    /// path. If it cannot be read, the shell logs the error and falls back to `background`
    /// rather than refusing to start — one image must never stop the machine from booting.
    pub wallpaper: String,
    /// How `wallpaper = "file:…"` is fitted: `"cover"` (the default), `"contain"` or
    /// `"stretch"`.
    ///
    /// Use one image across devices with different aspect ratios and `cover` crops, `contain`
    /// leaves margins and `stretch` distorts. A background photo is usually `cover`; something
    /// that must not be cropped, like a logo, is `contain`.
    pub wallpaper_fit: String,
    /// **The icon rail** — pin the icons down one side of the screen and open screens beside
    /// them (the kiosk layout). `"none"` (the default), `"left"` or `"right"`.
    ///
    /// In the default layout the icon grid owns the middle of the screen and an opening screen
    /// **covers** it (A2, the home ↔ task transition). A shop kiosk wants the
    /// opposite — the menu always visible and only the content changing. With the rail on, the
    /// icons live in one column and screens open only in what is left.
    pub rail: String,
    /// The rail's width, as a fraction of the content width. `0` means the default (0.26).
    ///
    /// A fraction is the point — an absolute du would mean different things on a 480 px panel
    /// and a 1920 px one. The computed width never goes below one icon cell
    /// (`metrics.icon_cell`).
    pub rail_width: f32,
    /// Desktop label legibility correction: `"none"` (the default), `"shadow"` or `"veil"`
    /// (guide 09 §2.2).
    ///
    /// **`none` being the default is the rule.** The shell does not lay a veil over the
    /// background; **the artwork itself is responsible for legibility** — a background authored
    /// dark needs no correction, and turning correction on by default would fog a good
    /// background too.
    ///
    /// Photo and artwork backgrounds, though, routinely put a bright area over the icon grid
    /// (the manta reference art's white belly does). For an integrator who cannot change the
    /// background there are two prescriptions:
    ///
    /// | Value | What it does | Cost |
    /// |---|---|---|
    /// | `"none"` | Nothing | 0 |
    /// | `"shadow"` | One dark shadow behind the label text | Text draw calls ×2 |
    /// | `"veil"` | A translucent `scrim` sheet over the whole content | One rectangle |
    ///
    /// `shadow` is usually the answer — it saves the labels without flattening the picture.
    pub label_legibility: String,
    /// `[desktop.abyss]` — how the procedural deep-sea background is composed.
    /// Only used when `wallpaper = "abyss"`.
    pub abyss: AbyssConfig,
    /// What a long press on a desktop icon does: `"info"` (the default) brings up
    /// the icon's info popover — its title, the level it needs and its description — and
    /// `"none"` leaves the press to you. Either way it is reported as
    /// [`ShellEvent::IconLongPressed`](crate::ShellEvent::IconLongPressed).
    ///
    /// `"none"` is for a device that raises a menu of its own on the event, so the two do not
    /// come up together. An unknown value warns and behaves as `"info"`.
    pub long_press: String,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            columns: 4,
            rows: 3,
            dock: Vec::new(),
            dock_edge: "bottom".to_owned(),
            dock_band: None,
            pages: Vec::new(),
            label_lines: 2,
            wallpaper: "background".to_owned(),
            wallpaper_fit: "cover".to_owned(),
            rail: "none".to_owned(),
            rail_width: 0.0,
            label_legibility: "none".to_owned(),
            abyss: AbyssConfig::default(),
            long_press: "info".to_owned(),
        }
    }
}

/// One page of `[[desktop.pages]]`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PageConfig {
    /// The icon overrides, in row-major order.
    pub icons: Vec<IconOverride>,
}

/// The override for one icon cell in a page.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct IconOverride {
    /// The declaration id (no prefix).
    pub id: String,
    /// A label override.
    pub label: Option<String>,
    /// A description override — what the icon's info popover says under the title.
    /// Like `label`, it is a key looked up in the string table.
    pub description: Option<String>,
    /// A built-in icon name override.
    pub icon: Option<String>,
    /// How a closed gate presents: `"show"` (a padlock) or `"hide"`.
    pub locked: Option<String>,
    /// Pin the column (without it, the next cell in list order).
    pub col: Option<u8>,
    /// Pin the row.
    pub row: Option<u8>,
}

/// `[overlay]`. `edge_px` comes from `theme.metrics.edge_px` and the snap ratio
/// from `[motion.shade]`, each a single source, so `[overlay]` has no keys of its own for
/// them.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct OverlayConfig {
    /// `"unified"` (the Android style, the default): one shade wherever it is pulled from — the
    /// tiles at the top, the notifications below, the footer at the bottom.
    ///
    /// `"split"` (One UI's and iOS's): two panels, picked by where the pull starts — left of
    /// [`OverlayConfig::split_ratio`] the notifications, right of it the controls.
    /// Each panel holds only its own — the list and "clear all", or the tiles, the lock
    /// and the settings — and while one is open, a pull or a status bar tap on the other side brings
    /// the other in. Each fills the height — down to the bottom of the content area, past
    /// `max_height_ratio`, a card keeping its inset. A split shade has no stop on the way down:
    /// `two_step` is ignored, with a warning, because each panel already does one thing. With
    /// `reveal = "card"` and `card_anchor = "press"` the notifications rest on the left and the
    /// controls on the right.
    pub layout: String,
    /// The tile ids in order: built-in `tile.*` and `tile()` declaration ids. A declaration not in the list goes on the end.
    pub tiles: Vec<String>,
    /// Tiles per row.
    pub tile_columns: u8,
    /// The footer buttons, **left to right**: `"clear_all"` · `"lock"` · `"settings"`. An unknown
    /// name warns at start-up and is skipped, and an empty list draws none of them — the subject's
    /// name and the level dot stay, being state rather than controls.
    ///
    /// They used to be drawn unconditionally, so a device with no lock concept, or one that never
    /// registered `settings.home`, carried buttons it could not remove.
    pub footer: Vec<String>,
    /// The shade's maximum height (a fraction of the screen height, A1: `H = min(content, 0.85 × screen)`).
    pub max_height_ratio: f32,
    /// How long a peeked hidden status bar stays revealed (ms).
    pub peek_ms: u64,
    /// **Open the shade in two steps** — the first pull stops at the tiles, a second carries on to
    /// the notifications. `false`, one step to the bottom, is the default and what every
    /// panel did before.
    ///
    /// It is a choice about what the device is for. On a panel whose notifications are the point,
    /// a stop on the way is one pull too many; on an instrument where the shade is mostly reached
    /// for to toggle Wi-Fi, stopping at the tiles keeps three quarters of the screen showing the
    /// run that is still going. The crate cannot tell which, and guessing wrong costs a gesture
    /// every time.
    ///
    /// Resting at the stop the shade lies over a live, undimmed screen, and it does not
    /// wait to be closed: a press anywhere outside it — on the page, on a bar — closes it, and
    /// still lands where it was aimed.
    pub two_step: bool,
    /// **How the shade comes into view**: `"curtain"` or `"card"`.
    ///
    /// `"curtain"` — the default, and what every panel did before — hangs the panel from the top
    /// edge and draws it down over the page: the content is pinned to the top and only as much as
    /// has been pulled is revealed (A1). It is the shade of Android up to One UI 6, and
    /// of the crate's first two years.
    ///
    /// `"card"` is the shade of One UI 7 and 8: a floating card, inset from the sides with all
    /// four corners round, that is already its final size at its final place — what the pull
    /// drives is how far it has *arrived*. It fades in from nothing, comes down a short way and
    /// its edge sharpens from a soft blob into a corner; the page behind it is not dimmed. It is
    /// gone in a tenth of the time it took to come. On a wide screen it rests on the side the
    /// pull started from ([`OverlayConfig::card_anchor`]).
    ///
    /// An unknown name warns at start-up and draws the curtain.
    pub reveal: String,
    /// **A card's width as a share of the screen's** (`reveal = "card"`). Measured off One UI 8:
    /// 0.445 of a tablet's width. It never goes below what the tile row needs at one touch
    /// target per tile; and where the card would then cover more than three quarters of the
    /// screen, leaving only a sliver of page beside it, it takes the whole width less its side
    /// insets instead — a phone's shade rather than a tablet's.
    pub card_width_ratio: f32,
    /// **Which side of a wide screen a card rests on** (`reveal = "card"`): `"press"`,
    /// `"left"`, `"right"` or `"center"`.
    ///
    /// `"press"` — the default — puts it on the side the pull started from, the way a tablet's
    /// shade does; a shade opened without a finger (a status-bar tap, `open()`) rests in the
    /// middle. On a screen the card fills, the anchor makes no difference. An unknown name warns
    /// and behaves as `"press"`.
    pub card_anchor: String,
    /// **Where a split shade's two panels divide the top edge** (`layout = "split"`), as a share
    /// of the width from the left: a pull starting left of it opens the notifications, right of it
    /// the controls.
    ///
    /// 0.5 — the default — gives each panel its half, which on a wide screen puts the card under
    /// the hand that pulled it. A phone-shaped curtain wants One UI's 0.7, where the controls are
    /// the right third.
    pub split_ratio: f32,
    /// **How opaque a card is over its frosted backdrop** (`reveal = "card"`): 0 is clear
    /// glass, 1 turns the glass off.
    ///
    /// A card asks the runner for one screenshot as it first shows, frosts it — shrunk and blurred
    /// once — and lies on it, so what is behind comes through as colour and shape, not as text to
    /// read. 0.7 — the default — lets three tenths of it through: the page's big shapes
    /// show under the card as soft colour, and the card's own text keeps its contrast. Where no
    /// screenshot comes back (a headless test, a runner without the command), the card stays
    /// solid: a translucent plate over the *sharp* page would make both unreadable. So it does,
    /// until it next opens, where the theme switches or the window resizes under an open card —
    /// the frost would show a page that is no longer there.
    pub card_glass: f32,
    /// **How raised a card looks** (`reveal = "card"`): 0 is flat, 1 — the default — is
    /// as designed, 2 is twice it.
    ///
    /// The light comes from above: the card's top edge catches it (a hairline brighter than the
    /// face, strongest along the top and fading down the sides), its bottom edge is in shade (a
    /// darker one, the same way up), and its face has a faint sheen at the top. It lies inside the
    /// elevation's own ink — the cast shadow of a light palette, the rim of a dark one — and does
    /// not replace it, so even at 0 a card keeps what separates it from the page.
    ///
    /// A panel that cannot reproduce a soft ramp — an e-ink or 16-level display, the one
    /// `ElevationStyle::Off` is for — wants 0 here, as it wants `card_glass = 1`: the sheen is a
    /// ramp, and so is the frost.
    pub card_relief: f32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            layout: "unified".to_owned(),
            tiles: [
                "tile.wifi",
                "tile.bluetooth",
                "tile.brightness",
                "tile.volume",
                "tile.lock",
                "tile.theme",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            tile_columns: 6,
            footer: ["clear_all", "lock", "settings"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            max_height_ratio: 0.85,
            peek_ms: 2000,
            two_step: false,
            reveal: "curtain".to_owned(),
            card_width_ratio: 0.46,
            card_anchor: "press".to_owned(),
            split_ratio: 0.5,
            card_glass: 0.7,
            card_relief: 1.0,
        }
    }
}

/// `[notify]`.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct NotifyConfig {
    /// Show a new notification briefly as a heads-up banner at the top.
    pub heads_up: bool,
    /// How many toasts are visible at once.
    pub max_visible: u8,
    /// The maximum the notification centre retains (over it, the oldest non-persistent ones are dropped).
    pub max_items: u16,
    /// The default time a toast shows (ms).
    pub toast_ms: u64,
    /// **Draw the dismiss ×** on shade notification rows.
    ///
    /// Turned off, the ways to dismiss are **a left swipe and "clear all"**. On a
    /// finger-only device that is cleaner; on one where swipes register poorly — gloves, a
    /// resistive panel — the button has to be there. Which makes it policy, not taste.
    pub dismiss_button: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            heads_up: true,
            max_visible: 2,
            max_items: 100,
            toast_ms: 3000,
            dismiss_button: true,
        }
    }
}

/// `[osk]`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct OskConfig {
    /// The height = the screen height × this ratio — but no row of keys taller than
    /// `metrics.osk_max_key` (one and a half fingers), so a tall panel does not get rows the size
    /// of a palm. The cap is a token: change it with
    /// [`ShellBuilder::metrics_spec`](crate::ShellBuilder::metrics_spec), or set it to `None`
    /// there to lift it and let this ratio alone decide.
    pub height_ratio: f32,
    /// The minimum key height (du). It wins over the cap where the two cross.
    pub min_key_px: f32,
    /// The default layout: `"qwerty"`, `"numpad"`, or `"hangul"` (= `"ko"`, the dubeolsik).
    ///
    /// With `"hangul"` the jamo do not go in as they are; [`crate::osk::HangulComposer`]
    /// composes them into syllables. A `한/영` key on the bottom row switches to
    /// the English qwerty and back. **A font with Hangul glyphs has to be installed with it** —
    /// egui's default fonts have no CJK, so the key labels come out as □
    /// ([`crate::fonts`], [`crate::ShellBuilder::fonts`]).
    pub layout: String,
    /// A decimal point key on the numpad.
    pub numpad_decimal: bool,
    /// A ± key on the numpad.
    pub numpad_sign: bool,
}

impl Default for OskConfig {
    fn default() -> Self {
        Self {
            height_ratio: 0.38,
            min_key_px: 48.0,
            layout: "qwerty".to_owned(),
            numpad_decimal: true,
            numpad_sign: false,
        }
    }
}

/// `[gesture]`. The slop, tap and long press are `[motion]`, and the edge width is `theme.metrics.edge_px`.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct GestureConfig {
    /// Gestures on or off, globally. Off means no edge swipes, long presses or flings. **The
    /// emergency gesture** (a 2 s press in a top corner) is a safety device and
    /// remains — the engine keeps tracking the press (`hold`) alone.
    pub enabled: bool,
    /// How long a finger must be still after a swipe to count as held (ms, `SwipeHold`).
    pub hold_ms: u64,
    /// The emergency gesture: the top-corner long-press duration (ms; 2 s by default).
    pub emergency_ms: u64,
    /// The emergency corner's side (px).
    pub emergency_corner_px: f32,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            hold_ms: 150,
            emergency_ms: 2000,
            emergency_corner_px: 64.0,
        }
    }
}

/// `[workspace]` (M5) — the shell's split panes and its overview.
///
/// Both are the shell's own and on by default. Off, the controls that would open them are only
/// reported ([`ShellEvent::SplitRequested`](crate::ShellEvent::SplitRequested),
/// [`ShellEvent::OverviewRequested`](crate::ShellEvent::OverviewRequested)) — for a device that
/// keeps one screen at a time, or has an overview of its own.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// The shell's split panes: `cx.open_in_other_pane`, the split tile and the nav bar's
    /// `"split"` item open the other pane. Off, `open_in_other_pane` opens in the same pane.
    pub split: bool,
    /// How two panes divide the content: `"auto"` (side by side on content at least as wide as
    /// it is tall, one above the other otherwise), `"side_by_side"` or `"stacked"`.
    pub split_axis: String,
    /// The shell's overview of recent screens: the nav bar's `"recents"` item and
    /// `LaunchAction::OpenOverview` open it.
    pub overview: bool,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            split: true,
            split_axis: "auto".to_owned(),
            overview: true,
        }
    }
}

impl WorkspaceConfig {
    /// `split_axis` read: `None` for `"auto"`.
    ///
    /// # Errors
    /// [`Error::Config`] for anything but `"auto"`, `"side_by_side"` or `"stacked"`.
    pub fn axis(&self) -> Result<Option<crate::workspace::SplitAxis>> {
        match self.split_axis.as_str() {
            "auto" => Ok(None),
            "side_by_side" => Ok(Some(crate::workspace::SplitAxis::SideBySide)),
            "stacked" => Ok(Some(crate::workspace::SplitAxis::Stacked)),
            other => Err(Error::Config(format!(
                "[workspace] split_axis = \"{other}\" must be \"auto\", \"side_by_side\" or \"stacked\""
            ))),
        }
    }
}

/// `[access]`. The levels, gates and mode are all the integrator's to define.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct AccessConfig {
    /// `"prompt"` | `"routing"` | `"off"`.
    pub mode: String,
    /// The level names, low to high. One means no authentication.
    pub levels: Vec<String>,
    /// The starting subject's level name. Without it, the lowest.
    pub initial: Option<String>,
    /// What happens to an unassigned gate: `"top"`, `"bottom"` or `<level>`. Required with two or more levels.
    pub default_gate: Option<String>,
    /// `[access.gates]` — gate name → level name.
    pub gates: BTreeMap<String, String>,
    /// What a granted unlock does to the session: `"switch"` makes the granted subject the
    /// session, `"temporary"` (the default) holds it for `temporary_secs` and then puts the
    /// previous subject back.
    pub unlock_mode: String,
    /// How long a temporary unlock lasts, in seconds, counted **from the unlock** — not from the
    /// last touch. Must be more than 0 with `unlock_mode = "temporary"`.
    pub temporary_secs: u64,
    /// With no input for this many seconds the session goes back to the starting subject. 0
    /// turns it off.
    pub session_timeout_secs: u64,
    /// With no input for this many seconds the panel locks: the session goes back to the starting
    /// subject and, in `prompt` mode with an authenticator, the lock screen comes up.
    /// 0 turns it off.
    pub idle_lock_secs: u64,
    /// `[access.lock_screen]`.
    pub lock_screen: LockScreenConfig,
    /// `[access.pin_table]` — the data for the reference authenticator,
    /// [`PinTable`](crate::access::PinTable). With no PINs there is no
    /// `PinTable`, and `prompt` mode needs an authenticator from
    /// [`ShellBuilder::authenticator`](crate::ShellBuilder::authenticator) to draw anything.
    pub pin_table: PinTableConfig,
    /// `[access.pattern_table]` — patterns for the same reference authenticator: a level may
    /// have a PIN, a pattern or both, and the prompt offers each kind there is.
    pub pattern_table: PatternTableConfig,
}

/// `[access.lock_screen]`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LockScreenConfig {
    /// Offer a "Continue" button that leaves the lock screen **without** authenticating — as the
    /// starting subject, so only what that subject passes is open. Off by default: a lock screen
    /// that anyone can step past is not a lock.
    pub allow_continue: bool,
}

impl Default for AccessConfig {
    fn default() -> Self {
        Self {
            mode: "prompt".to_owned(),
            levels: vec!["default".to_owned()],
            initial: None,
            default_gate: None,
            gates: BTreeMap::new(),
            unlock_mode: "temporary".to_owned(),
            temporary_secs: 300,
            session_timeout_secs: 0,
            idle_lock_secs: 0,
            lock_screen: LockScreenConfig::default(),
            pin_table: PinTableConfig::default(),
            pattern_table: PatternTableConfig::default(),
        }
    }
}

/// `[access.pin_table]` (the reference implementation). Level name → PIN collects
/// into `pins`; `attempt_limit`, `lock_secs`, `shuffle` and `max_len` are the fixed fields, so no
/// level can be named after one of them.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PinTableConfig {
    /// This many wrong tries in a row lock the prompt (optional). Counted in memory, and shared
    /// with `[access.pattern_table]` — a wrong pattern and a wrong PIN are both a wrong try.
    pub attempt_limit: Option<u32>,
    /// How long the lockout lasts, in seconds (60 when left out).
    pub lock_secs: Option<u64>,
    /// A fresh digit layout each time the prompt opens.
    pub shuffle: bool,
    /// The most digits a PIN may have, 1 to 16 (16 when left out): a longer PIN in the table is
    /// a config error, the keypad takes no more, and `settings.credentials` sets no longer one.
    pub max_len: Option<u8>,
    /// Level name → the PIN in plain text (digits, up to `max_len`). Every remaining key collects
    /// here.
    #[serde(flatten)]
    pub pins: BTreeMap<String, String>,
}

/// `[access.pattern_table]` (the reference implementation). Level name → pattern
/// collects into `patterns`; `grid`, `min_points`, `show_path`, `attempt_limit` and `lock_secs`
/// are the fixed fields.
///
/// A pattern is its dots in the order drawn, numbered row by row from 1 at the top left —
/// `"1-2-3-6-9"`, with `-`, `,` or spaces between them. On the 3 × 3 grid the digits alone do
/// (`"12369"`), the way the dots sit on a phone's keypad.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct PatternTableConfig {
    /// Dots on a side, 3 to 5.
    pub grid: u8,
    /// The fewest dots a pattern has — four by default. The pad submits nothing shorter, and a
    /// shorter pattern in the table is a config error.
    pub min_points: u8,
    /// Draw the path as the finger draws it. Off leaves nothing on the glass for a glance over
    /// the shoulder — and nothing for the person drawing either.
    pub show_path: bool,
    /// As `[access.pin_table]`'s, and the same count: where both set one, the stricter holds.
    pub attempt_limit: Option<u32>,
    /// As `[access.pin_table]`'s; where both set one, the longer holds.
    pub lock_secs: Option<u64>,
    /// Level name → the pattern in plain text. Every remaining key collects here.
    #[serde(flatten)]
    pub patterns: BTreeMap<String, String>,
}

impl Default for PatternTableConfig {
    fn default() -> Self {
        Self {
            grid: 3,
            min_points: 4,
            show_path: true,
            attempt_limit: None,
            lock_secs: None,
            patterns: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ShellConfig;

    #[test]
    fn default_and_toml_round_trip() {
        let cfg = ShellConfig::from_toml(
            r#"
[status_bar]
height = 40
right = ["status.wifi"]

[access]
levels = ["viewer", "maintainer"]
default_gate = "top"
[access.gates]
"settings.wifi" = "viewer"

[motion.push]
ms = 100
"#,
        );
        let cfg = cfg.unwrap_or_default();
        assert_eq!(cfg.status_bar.height, Some(40.0));
        assert_eq!(cfg.status_bar.right, vec!["status.wifi".to_owned()]);
        assert_eq!(cfg.access.levels.len(), 2);
        assert_eq!(cfg.motion.push.ms, 100);
        assert_eq!(cfg.motion.pop.ms, 200);
    }

    /// **A configuration with no way back refuses to boot.**
    ///
    /// `gesture` has no back button. Empty `back_edges` on top of that leaves a device with no
    /// way out of the stack, and that is discovered after it ships.
    #[test]
    fn gesture_nav_without_back_edges_refuses_to_start() {
        let nav = |style: &str, edges: &str, enabled: bool| {
            ShellConfig::from_toml(&format!(
                "[nav_bar]\nenabled = {enabled}\nstyle = \"{style}\"\nback_edges = [{edges}]\n"
            ))
        };
        // There is no way out — it is blocked.
        let msg = nav("gesture", "", true).err().map(|e| e.to_string());
        let Some(msg) = msg else {
            unreachable!("gesture + empty back_edges was let through")
        };
        assert!(
            msg.contains("back_edges"),
            "it does not say what the problem is: {msg}"
        );
        // With an edge there is a way back — it passes.
        assert!(
            nav("gesture", "\"left\"", true).is_ok(),
            "there are edges and it still refused"
        );
        // With the button style the buttons are the back, even with the edges empty.
        assert!(nav("buttons", "", true).is_ok(), "it refused buttons");
        // A screen with the bar off draws its own back (the kiosk example) — the style value means nothing.
        assert!(
            nav("gesture", "", false).is_ok(),
            "the bar is off and it still refused"
        );
    }

    #[test]
    fn unknown_sections_are_ignored_and_bad_types_fail() {
        assert!(ShellConfig::from_toml("[overlay]\nlayout = \"unified\"\n").is_ok());
        assert!(ShellConfig::from_toml("[status_bar]\nheight = \"tall\"\n").is_err());
    }

    /// The reference configuration example has to parse as written — including a
    /// `pin_table` that mixes the fixed keys (`attempt_limit`, `lock_secs`) with dynamic level
    /// name keys (`operator`, `maintainer`).
    #[test]
    fn design_doc_example_parses() {
        let cfg = ShellConfig::from_toml(
            r#"
[shell]
locale = "ko"
theme = "dark"
idle_lock_secs = 300
repaint = "reactive"

[status_bar]
left  = ["status.clock", "status.user"]
right = ["status.notifications", "status.bluetooth", "status.wifi", "status.battery"]

[nav_bar]
style = "buttons"
items = ["back", "home", "recents"]

[desktop]
columns = 4
rows = 3
dock = ["settings.home", "dashboard"]

[[desktop.pages]]
icons = [
  { id = "dashboard" },
  { id = "settings.wifi", label = "Wi-Fi", icon = "wifi", locked = "show" },
]

[access]
mode = "prompt"
levels = ["viewer", "operator", "maintainer"]
initial = "viewer"
default_gate = "top"
unlock_mode = "temporary"
temporary_secs = 300

[access.gates]
"settings.wifi" = "operator"
"settings.network.edit" = "maintainer"
"dashboard" = "viewer"
"status.clock" = "bottom"

[access.pin_table]
operator = "1234"
maintainer = "987654"
attempt_limit = 5
lock_secs = 60
"#,
        );
        assert!(
            cfg.is_ok(),
            "the reference example has to be a valid config: {cfg:?}"
        );
        let cfg = cfg.unwrap_or_default();
        assert_eq!(cfg.shell.locale, "ko");
        assert_eq!(cfg.desktop.pages.len(), 1);
        assert_eq!(cfg.desktop.pages.first().map(|p| p.icons.len()), Some(2));
        assert_eq!(cfg.access.levels, vec!["viewer", "operator", "maintainer"]);
        assert_eq!(cfg.access.pin_table.attempt_limit, Some(5));
        assert_eq!(cfg.access.pin_table.lock_secs, Some(60));
        assert_eq!(
            cfg.access
                .pin_table
                .pins
                .get("operator")
                .map(String::as_str),
            Some("1234")
        );
    }

    #[test]
    fn out_of_range_values_are_config_errors() {
        assert!(ShellConfig::from_toml("[status_bar]\nheight = 0\n").is_err());
        assert!(ShellConfig::from_toml("[nav_bar]\nheight = -1\n").is_err());
        // Valid TOML floats that are not sizes (these keys take effect, so they are checked).
        assert!(ShellConfig::from_toml("[status_bar]\nheight = nan\n").is_err());
        assert!(ShellConfig::from_toml("[status_bar]\nicon_size = inf\n").is_err());
        assert!(ShellConfig::from_toml("[nav_bar]\nheight = -nan\n").is_err());
        assert!(ShellConfig::from_toml("[nav_bar]\nheight = 64.0\n").is_ok());
        assert!(ShellConfig::from_toml("[motion]\nsnap_ratio = 1.5\n").is_err());
        assert!(ShellConfig::from_toml("[motion.spring]\nk = 0\n").is_err());
        assert!(ShellConfig::from_toml("[motion.home]\ndesktop_scale = 0\n").is_err());
        assert!(ShellConfig::from_toml("[motion.press]\nscale = 1.5\n").is_err());
    }
}
