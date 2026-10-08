//! The desktop — the page grid plus the dock. M1's composition is static (the horizontal
//! swipe is M2 · A4; here the page changes **only by tapping the indicator**).
//!
//! The desktop is not a `Screen` but the workspace's `Home` view. Icons come from code
//! declarations (`.desktop()` / `.dock()`) and the config file overrides only the position, the
//! label and the locked marking.
//!
//! To draw the cells yourself, hand over a [`SlotPainter`] (through
//! [`crate::shell::ShellBuilder::slot_painter`]). The hit testing, the press decision and the gate
//! filtering stay the shell's, and [`SlotCx`] hands over the cell Rect, the [`IconSlot`], the press
//! scale and whether the gate passed.
//!
//! **Zero heap allocation per frame** is the goal. So, separately from the layout results
//! ([`Page`] · [`Dock`]), each icon has one `IconRecord` caching its label and badge galleys. The
//! render path only overwrites values and never builds a fresh `String` or `Vec`. The regression is
//! watched by the `idle desktop` scenario in `examples/bench.rs`.

// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the rest of pedantic stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod info;
mod swipe;

use info::{IconInfo, LongPressMode};
pub use swipe::PageSwipe;

use crate::access::{Access, Gate, Visibility};
use crate::config::DesktopConfig;
use crate::i18n::LabelKey;
use crate::icons::{builtin, IconColor, IconRef, IconSet, IconStyle};
use crate::motion::Animated;
use crate::screen::{DesktopPlacement, LaunchMode, Registry};
use crate::theme::{ColorRole, Metrics, MotionTokens, Theme};
use crate::widgets::{BadgeAnchor, BadgeTone, BadgeValue, CountBadge};
use egui::text::LayoutJob;
use egui::{Color32, Rect, Sense};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// The grid's column cap (so the touch targets do not shrink). It caps the automatic calculation and
/// is what [`crate::ShellConfig::validate`] caps `[desktop] columns` at.
pub(crate) const MAX_COLUMNS: u8 = 12;
/// The grid's row cap.
pub(crate) const MAX_ROWS: u8 = 8;
/// The dock band's background tint — `Surface × 0.6`. With the dock empty, the band is not drawn either.
const DOCK_BAND_TINT: f32 = 0.6;
// The indicator dots' two radii are `metrics.page_indicator_dot_off` and `_on`.

/// A badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Badge {
    /// A number.
    Count(u32),
    /// A dot.
    Dot,
    /// Short text.
    Text(String),
}

impl Badge {
    /// The borrowed form [`CountBadge`] draws.
    ///
    /// The stored one owns its string because an [`IconSlot`] outlives the frame; the drawn one
    /// borrows because a dock repaints every frame and must not allocate to do it.
    #[must_use]
    pub fn as_value(&self) -> BadgeValue<'_> {
        match self {
            Self::Count(n) => BadgeValue::Count(*n),
            Self::Dot => BadgeValue::Dot,
            Self::Text(t) => BadgeValue::Text(t),
        }
    }
}

/// The kind of declaration an icon points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// A screen → `LaunchAction::Open`.
    Screen,
    /// An action → `LaunchAction::Run`.
    Action,
}

/// One cell of the home grid or the dock (`IconSlot`).
#[derive(Debug, Clone, PartialEq)]
pub struct IconSlot {
    /// The declaration id (with no prefix).
    pub id: String,
    /// The label.
    pub label: LabelKey,
    /// What the info popover says under the title — a key, like the label.
    /// `None` shows the title, and the level it needs, alone.
    pub description: Option<LabelKey>,
    /// The icon.
    pub icon: IconRef,
    /// The gate. `None` means "use the id as the gate", and it is **settled to the id** when
    /// the slot is placed ([`Page::put`], or the dock), so a slot being drawn is always `Some`.
    pub gate: Option<Gate>,
    /// How a failed gate is expressed.
    pub visibility: Visibility,
    /// The badge.
    pub badge: Option<Badge>,
    /// A launch-mode override — it overrides the screen declaration's `.launch()` value. **In
    /// M1 it is always `None`** and `Shell::open_screen` does not look at it either — the way to give
    /// a per-slot override from the config (a `launch` key in `[[desktop.pages]]`) is M2, so this is
    /// only the slot for it.
    pub launch: Option<LaunchMode>,
    /// The kind.
    pub kind: SlotKind,
}

impl IconSlot {
    /// The gate name.
    #[must_use]
    pub fn gate_name(&self) -> Gate {
        self.gate.clone().unwrap_or_else(|| Gate::from(&self.id))
    }

    /// Settle the default gate (= the id) **once, at the moment of placing**. So that the render path
    /// does not build a `Gate::from(&id)` every frame (`From<&str>` is always
    /// `Owned`).
    fn resolve_gate(&mut self) {
        if self.gate.is_none() {
            self.gate = Some(Gate::from(&self.id));
        }
    }
}

/// A page — `columns × rows` cells.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    cells: Vec<Option<IconSlot>>,
    columns: u8,
    rows: u8,
}

impl Page {
    /// An empty page.
    #[must_use]
    pub fn new(columns: u8, rows: u8) -> Self {
        Self {
            cells: vec![None; usize::from(columns) * usize::from(rows)],
            columns,
            rows,
        }
    }

    fn index(&self, col: u8, row: u8) -> Option<usize> {
        (col < self.columns && row < self.rows)
            .then(|| usize::from(row) * usize::from(self.columns) + usize::from(col))
    }

    /// Look a cell up.
    #[doc(hidden)]
    #[must_use]
    pub fn slot_at(&self, col: u8, row: u8) -> Option<&IconSlot> {
        self.index(col, row)
            .and_then(|i| self.cells.get(i))
            .and_then(Option::as_ref)
    }

    /// The first free cell (row first).
    #[must_use]
    pub(crate) fn first_free(&self) -> Option<(u8, u8)> {
        let i = self.cells.iter().position(Option::is_none)?;
        let col = u8::try_from(i % usize::from(self.columns.max(1))).ok()?;
        let row = u8::try_from(i / usize::from(self.columns.max(1))).ok()?;
        Some((col, row))
    }

    /// Place. `false` where it is already taken. The default gate (= the id) is settled here — the
    /// render path does not build a `Gate` every frame.
    pub fn put(&mut self, col: u8, row: u8, mut slot: IconSlot) -> bool {
        match self.index(col, row).and_then(|i| self.cells.get_mut(i)) {
            Some(cell) if cell.is_none() => {
                slot.resolve_gate();
                *cell = Some(slot);
                true
            }
            _ => false,
        }
    }

    /// The filled cells (column, row, slot).
    pub fn slots(&self) -> impl Iterator<Item = (u8, u8, &IconSlot)> {
        let columns = usize::from(self.columns.max(1));
        self.cells.iter().enumerate().filter_map(move |(i, cell)| {
            let slot = cell.as_ref()?;
            Some((
                u8::try_from(i % columns).ok()?,
                u8::try_from(i / columns).ok()?,
                slot,
            ))
        })
    }

    /// The filled cells (mutable).
    pub(crate) fn slots_mut(&mut self) -> impl Iterator<Item = &mut IconSlot> {
        self.cells.iter_mut().filter_map(Option::as_mut)
    }
}

/// The dock (fixed). There is no cap on the count — see `Dock::push`'s comment.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dock {
    slots: Vec<IconSlot>,
    placement: DockPlacement,
}

/// Where the row of fixed icons goes.
///
/// A band along the bottom is a phone convention, not a device requirement. A portrait panel is
/// better with a rail down one side, and a wide instrument screen takes naturally to **a row across**
/// the desktop. All three come out of the same mechanism.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DockPlacement {
    /// A band against one edge of the screen. `Bottom` is today's dock. The grid shrinks by that much.
    Edge(crate::gesture::Edge),
    /// A row **across** the desktop. Rather than against an edge, it is drawn as a plate floating over
    /// the background — the plate is only as wide as the row of icons and its corners are rounded (the
    /// floating bar of an old PMP launcher, iOS's floating dock). The grid avoids the row and uses
    /// **the wider side**. `at` decides which side is left larger.
    Band {
        /// The row's direction.
        axis: Axis,
        /// The centre position relative to the content (0..=1). y for a horizontal row, x for a vertical one.
        at: f32,
    },
}

impl Default for DockPlacement {
    fn default() -> Self {
        Self::Edge(crate::gesture::Edge::Bottom)
    }
}

/// The row's direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Horizontal — the icons run left to right.
    Horizontal,
    /// Vertical — the icons run top to bottom.
    Vertical,
}

impl Dock {
    /// Set the placement.
    pub fn set_placement(&mut self, placement: DockPlacement) {
        self.placement = placement;
    }

    /// The current placement.
    #[must_use]
    pub fn placement(&self) -> DockPlacement {
        self.placement
    }

    /// Whether it is a placement that runs vertically.
    #[must_use]
    pub fn is_vertical(&self) -> bool {
        match self.placement {
            DockPlacement::Edge(e) => {
                matches!(e, crate::gesture::Edge::Left | crate::gesture::Edge::Right)
            }
            DockPlacement::Band { axis, .. } => axis == Axis::Vertical,
        }
    }

    /// The thickness this dock takes on the axis it crosses. 0 while it is empty.
    ///
    /// [`DockPlacement::Edge`] takes this much off the edge, and [`DockPlacement::Band`] divides the
    /// content by it.
    #[must_use]
    pub(crate) fn band_thickness(&self, metrics: &Metrics) -> f32 {
        if self.slots.is_empty() {
            return 0.0;
        }
        metrics.dock_height
    }

    /// This placement's Rect. `content` is the desktop's content area.
    #[doc(hidden)]
    #[must_use]
    pub fn rect_in(&self, content: Rect, metrics: &Metrics) -> Rect {
        let t = metrics.dock_height;
        match self.placement {
            DockPlacement::Edge(crate::gesture::Edge::Bottom) => Rect::from_min_size(
                egui::pos2(content.min.x, content.max.y - t),
                egui::vec2(content.width(), t),
            ),
            DockPlacement::Edge(crate::gesture::Edge::Top) => {
                Rect::from_min_size(content.min, egui::vec2(content.width(), t))
            }
            DockPlacement::Edge(crate::gesture::Edge::Left) => {
                Rect::from_min_size(content.min, egui::vec2(t, content.height()))
            }
            DockPlacement::Edge(crate::gesture::Edge::Right) => Rect::from_min_size(
                egui::pos2(content.max.x - t, content.min.y),
                egui::vec2(t, content.height()),
            ),
            DockPlacement::Band { axis, at } => {
                let at = at.clamp(0.0, 1.0);
                match axis {
                    Axis::Horizontal => Rect::from_center_size(
                        egui::pos2(content.center().x, content.min.y + content.height() * at),
                        egui::vec2(content.width(), t),
                    ),
                    Axis::Vertical => Rect::from_center_size(
                        egui::pos2(content.min.x + content.width() * at, content.center().y),
                        egui::vec2(t, content.height()),
                    ),
                }
            }
        }
    }

    /// The slots.
    #[must_use]
    pub fn slots(&self) -> &[IconSlot] {
        &self.slots
    }

    /// Whether it is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Put one in. `false` where the same id is already there.
    ///
    /// **There is no cap on the count**. A cap of five is a phone convention, not a device
    /// requirement — twelve along the bottom of a 1920 bar panel is normal, and so is eight down the
    /// left rail of a 480×800. Instead, they shrink once a cell would fall below the physical minimum
    /// touch size (the geometry rule).
    fn push(&mut self, mut slot: IconSlot) -> bool {
        if self.slots.iter().any(|s| s.id == slot.id) {
            return false;
        }
        slot.resolve_gate();
        self.slots.push(slot);
        true
    }
}

/// A procedural wallpaper callback.
pub type WallpaperPainter = Box<dyn Fn(&egui::Painter, Rect)>;

/// A procedural wallpaper callback **that receives the theme**.
///
/// Use this one where the wallpaper has to follow the dark/light change and `[theme.palette]`
/// overrides — do not capture the colours; read them from `theme` every frame. It is called every
/// frame, so **do not make a heap allocation inside the callback** (put any internal cache in a
/// `RefCell`).
pub type ThemedWallpaperPainter = Box<dyn Fn(&egui::Painter, Rect, &Theme)>;

/// How a raster wallpaper is fitted.
///
/// This being a crate with "no fixed screen size", the original's ratio differing from the panel's is
/// normal — stretching it out of shape must not be the default behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Fill the short side and crop the long one (what a background photo is expected to do).
    Cover,
    /// Fit it all in view and fill what is left over with the `background` role colour.
    Contain,
    /// The same as [`Wallpaper::Texture`] — stretched.
    Stretch,
}

impl Fit {
    /// All of them.
    pub const ALL: [Self; 3] = [Self::Cover, Self::Contain, Self::Stretch];

    /// From a config string. `None` for a name it does not know.
    ///
    /// ```
    /// use fairing::desktop::Fit;
    ///
    /// assert_eq!(Fit::parse("contain"), Some(Fit::Contain));
    /// assert_eq!(Fit::parse("COVER"), Some(Fit::Cover));
    /// assert_eq!(Fit::parse("crop"), None);
    /// ```
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "cover" => Some(Self::Cover),
            "contain" => Some(Self::Contain),
            "stretch" => Some(Self::Stretch),
            _ => None,
        }
    }

    /// The name written in the config. The inverse of [`Fit::parse`].
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Contain => "contain",
            Self::Stretch => "stretch",
        }
    }
}

/// The whole `0..1` UV rectangle.
const FULL_UV: Rect = Rect {
    min: egui::pos2(0.0, 0.0),
    max: egui::pos2(1.0, 1.0),
};

/// The UV rectangle for a [`Fit`].
///
/// No lock and no query of texture metadata — the original's size is an argument. Recovering it
/// through `Context::tex_manager()` hands out an `Arc<RwLock<..>>` and so breaks the no-locks rule,
/// and `xtask sync-check` catches that name as a lock token.
#[must_use]
fn uv_for(fit: Fit, src: egui::Vec2, dst: egui::Vec2) -> Rect {
    if fit == Fit::Stretch || src.x <= 0.0 || src.y <= 0.0 || dst.x <= 0.0 || dst.y <= 0.0 {
        return FULL_UV;
    }
    let (src_ar, dst_ar) = (src.x / src.y, dst.x / dst.y);
    let (u, v) = if src_ar > dst_ar {
        (dst_ar / src_ar, 1.0)
    } else {
        (1.0, src_ar / dst_ar)
    };
    Rect::from_min_max(
        egui::pos2(0.5 - u * 0.5, 0.5 - v * 0.5),
        egui::pos2(0.5 + u * 0.5, 0.5 + v * 0.5),
    )
}

/// The wallpaper. There is no image decoder in the core — the integrator uploads the
/// texture.
///
/// It is `#[non_exhaustive]`: this move from five variants to seven is the moment to attach it, and
/// the next addition will not be a breaking change. A `match` **outside** the crate needs a `_` arm.
#[non_exhaustive]
pub enum Wallpaper {
    /// **A pair that follows the palette** — the dark form and the light one.
    ///
    /// [`Solid`](Self::Solid) follows a theme change because a role does, and
    /// [`ThemedPainter`](Self::ThemedPainter) follows because it is handed the theme. A picture
    /// follows nothing: a texture wallpaper is one image, so a shell that switches to its light
    /// palette at dusk keeps the dark painting under a pale UI, and the labels that were designed
    /// to read against it stop reading.
    ///
    /// This holds both and picks by `theme.dark`. The two sides need not be the same kind — a
    /// painting for the night and a flat colour for the day is a pair like any other.
    ///
    /// It is resolved **once, before the paint match**, so it does not nest: a pair inside a pair
    /// picks the same side twice and the inner one wins.
    Themed {
        /// Drawn while the palette is dark.
        dark: Box<Wallpaper>,
        /// Drawn while it is light.
        light: Box<Wallpaper>,
    },
    /// A flat role colour.
    Solid(ColorRole),
    /// A fixed flat colour.
    Fixed(Color32),
    /// A vertical gradient.
    Gradient {
        /// The top.
        top: Color32,
        /// The bottom.
        bottom: Color32,
    },
    /// An integrator texture. The UV is pinned to `0..1`, so where the original's ratio differs from
    /// the panel's it is **squashed** — to keep the ratio, use [`Wallpaper::TextureFit`].
    Texture {
        /// The texture.
        id: egui::TextureId,
    },
    /// An integrator texture fitted keeping the original's ratio.
    ///
    /// **It does not hold on to the texture.** It carries only a `TextureId`, so unless the integrator
    /// keeps the [`egui::TextureHandle`] alive the texture is released and the wallpaper comes out
    /// black. To leave the handle with the shell, use [`Wallpaper::owned`].
    ///
    /// This variant is for an integrator who already manages textures themselves (an atlas, streaming
    /// and so on).
    ///
    /// ```
    /// use fairing::desktop::{Fit, Wallpaper};
    ///
    /// // In practice these come from the integrator's decoder.
    /// let (id, source_size) = (egui::TextureId::default(), egui::vec2(2560.0, 1440.0));
    /// let wallpaper = Wallpaper::TextureFit { id, source_size, fit: Fit::Cover };
    /// assert_eq!(format!("{wallpaper:?}"), "TextureFit(Managed(0), Cover)");
    /// ```
    TextureFit {
        /// The texture.
        id: egui::TextureId,
        /// The original's size (px). A value the integrator already holds — the decoder's `dimensions()` as it stands.
        source_size: egui::Vec2,
        /// How it is fitted.
        fit: Fit,
    },
    /// A wallpaper that **holds the texture handle**.
    ///
    /// It draws the same as [`Wallpaper::TextureFit`] but holds the [`egui::TextureHandle`] here — so
    /// the texture is not released for as long as the wallpaper lives, with no separate keeping by the
    /// integrator. This variant removes the trap of `TextureHandle`'s `Drop` letting the texture go.
    ///
    /// `source_size` is read and cached **once, at construction**. `TextureHandle::size()` takes the
    /// texture manager's lock, so calling it from the paint loop breaks the rule that the UI thread
    /// never blocks — [`Wallpaper::owned`] does that one call for you.
    ///
    /// ```no_run
    /// use fairing::desktop::{Fit, Wallpaper};
    ///
    /// # let (ctx, image): (egui::Context, egui::ColorImage) = todo!();
    /// let texture = ctx.load_texture("wallpaper", image, egui::TextureOptions::LINEAR);
    /// let wallpaper = Wallpaper::owned(texture, Fit::Cover);
    /// ```
    Owned {
        /// The texture handle. The texture lives as long as this wallpaper does.
        texture: egui::TextureHandle,
        /// The original's size (px). Filled in by [`Wallpaper::owned`].
        source_size: egui::Vec2,
        /// How it is fitted.
        fit: Fit,
    },
    /// A procedural wallpaper.
    Painter(WallpaperPainter),
    /// A procedural wallpaper that receives the theme.
    ///
    /// Do not capture the colours; **read them from `theme` every frame** — that is what makes the
    /// wallpaper follow the dark/light change and `[theme.palette]` overrides.
    ///
    /// ```
    /// use fairing::desktop::Wallpaper;
    /// use fairing::{ColorRole, Theme};
    ///
    /// let wallpaper = Wallpaper::ThemedPainter(Box::new(
    ///     |painter: &egui::Painter, rect: egui::Rect, theme: &Theme| {
    ///         painter.rect_filled(rect, 0.0, theme.color(ColorRole::Background));
    ///     },
    /// ));
    /// assert_eq!(format!("{wallpaper:?}"), "ThemedPainter(..)");
    /// ```
    ThemedPainter(ThemedWallpaperPainter),
}

impl Wallpaper {
    /// A [`Themed`](Self::Themed) pair — the dark form and the light one.
    #[must_use]
    pub fn themed(dark: Self, light: Self) -> Self {
        Self::Themed {
            dark: Box::new(dark),
            light: Box::new(light),
        }
    }

    /// Which side of a [`Themed`](Self::Themed) pair applies; anything else is itself.
    ///
    /// One level deep on purpose — see the variant's docs.
    pub(crate) fn for_theme(&self, dark: bool) -> &Self {
        match self {
            Self::Themed { dark: d, light: l } => {
                if dark {
                    d
                } else {
                    l
                }
            }
            other => other,
        }
    }

    /// Build a wallpaper that holds the texture handle ([`Wallpaper::Owned`]).
    ///
    /// The original's size is read and cached here, once — and that one read takes the texture
    /// manager's lock, so call it **outside the paint loop**. Building the shell, or
    /// swapping the wallpaper, is the place for it.
    #[must_use]
    pub fn owned(texture: egui::TextureHandle, fit: Fit) -> Self {
        let source_size = texture.size_vec2();
        Self::Owned {
            texture,
            source_size,
            fit,
        }
    }
}

impl std::fmt::Debug for Wallpaper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Themed { dark, light } => write!(f, "Themed({dark:?}, {light:?})"),
            Self::Solid(role) => write!(f, "Solid({role:?})"),
            Self::Fixed(c) => write!(f, "Fixed({c:?})"),
            Self::Gradient { top, bottom } => write!(f, "Gradient({top:?}, {bottom:?})"),
            Self::Texture { id } => write!(f, "Texture({id:?})"),
            Self::TextureFit { id, fit, .. } => write!(f, "TextureFit({id:?}, {fit:?})"),
            Self::Owned { texture, fit, .. } => write!(f, "Owned({:?}, {fit:?})", texture.id()),
            Self::Painter(_) => f.write_str("Painter(..)"),
            Self::ThemedPainter(_) => f.write_str("ThemedPainter(..)"),
        }
    }
}

/// Legibility correction for the desktop labels (guide 09 §2.2).
///
/// **The default is [`Self::None`].** The shell lays no veil over the wallpaper and the picture itself
/// answers for legibility — there is no reason to fog a well-authored background. Turn it on only
/// where a photo or an art background puts a bright area over the icon grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LabelLegibility {
    /// Do nothing. **The default.**
    #[default]
    None,
    /// One dark shadow behind the label's text. It saves the text without killing the picture.
    Shadow,
    /// A translucent [`ColorRole::Scrim`] plate over the whole content. Certain, but it flattens the picture.
    Veil,
}

impl LabelLegibility {
    /// A `[desktop] label_legibility` name. An unknown name is `None`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "shadow" => Some(Self::Shadow),
            "veil" => Some(Self::Veil),
            _ => None,
        }
    }

    /// The name written in the config.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Shadow => "shadow",
            Self::Veil => "veil",
        }
    }

    /// All of them. Used when a config error message lists the candidates.
    pub const ALL: &'static [Self] = &[Self::None, Self::Shadow, Self::Veil];
}

/// The callback that makes a texture from a file path.
///
/// **The crate does not decode images.** Unpacking a PNG, JPEG or WebP is an integrator dependency
/// (`image`, `zune-image`, `stb`, a hardware decoder …), and this hook is where that is wired into
/// the shell. Register one and a single config line changes the wallpaper:
///
/// ```toml
/// [desktop]
/// wallpaper = "file:/opt/acme/brand/store-bg.webp"
/// wallpaper_fit = "cover"
/// ```
///
/// This path is what makes it possible to put a different background in each shop **without
/// rebuilding the device**. With no loader given, `file:` warns and falls back to `background` — a
/// device does not fail to start over one wallpaper.
///
/// The [`egui::TextureHandle`] handed back is **held** by the shell inside a [`Wallpaper::Owned`].
/// There is no need for the loader to keep it separately.
///
/// ```
/// use fairing::desktop::ImageLoader;
///
/// // Decoded with an integrator dependency. The crate takes no part in it.
/// let loader: ImageLoader = Box::new(|ctx: &egui::Context, path: &std::path::Path| {
///     let bytes = std::fs::read(path).map_err(|e| fairing::Error::Io {
///         path: path.display().to_string(),
///         message: e.to_string(),
///     })?;
///     let rgba = decode(&bytes).ok_or_else(|| fairing::Error::Image("decode failed".into()))?;
///     let image = egui::ColorImage::from_rgba_unmultiplied(rgba.0, &rgba.1);
///     Ok(ctx.load_texture(path.to_string_lossy(), image, egui::TextureOptions::LINEAR))
/// });
/// # fn decode(_: &[u8]) -> Option<([usize; 2], Vec<u8>)> { None }
/// # let _ = loader;
/// ```
pub type ImageLoader = Box<ImageLoaderFn>;

/// The inside of [`ImageLoader`]'s box. Needed to borrow it as an `Option<&ImageLoaderFn>`.
pub type ImageLoaderFn = dyn Fn(&egui::Context, &Path) -> crate::Result<egui::TextureHandle>;

/// The callback that draws one desktop cell.
pub type SlotPainter = Box<dyn FnMut(&mut egui::Ui, SlotCx<'_>)>;

/// The per-cell context a [`SlotPainter`] receives. It hands over the geometry and state the shell
/// settled, and leaves **only the drawing** — the hit test (`ui.interact`), the press decision, the
/// gate filtering and recording the A2 origin Rect are the shell's.
pub struct SlotCx<'a> {
    /// The cell Rect (the whole touch target).
    pub cell: Rect,
    /// The icon Rect **before** the press scale is applied. This Rect is the A2 icon zoom's origin.
    pub icon: Rect,
    /// The icon Rect with the press scale applied (`icon` scaled by `scale` about the cell's centre).
    pub pressed_icon: Rect,
    /// This cell's declaration (id, label, icon, badge, gate).
    pub slot: &'a IconSlot,
    /// The press scale (A7, 1.0 → `motion.press.scale`).
    pub scale: f32,
    /// Whether it is held down.
    pub pressed: bool,
    /// Whether the gate passed. With `false`, a padlock or a disabled look is needed.
    pub allowed: bool,
    /// Whether it is a dock cell (`false` for a grid cell).
    pub in_dock: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icon set (built-in and custom drawing).
    pub icons: &'a mut IconSet,
    /// The shell's time.
    pub now: Instant,
    /// The string table. `slot.label` is a key: draw `strings.get(&slot.label)`, so the label
    /// follows the language as the built-in rendering's does.
    pub strings: &'a crate::i18n::Strings,
}

impl std::fmt::Debug for SlotCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlotCx")
            .field("cell", &self.cell)
            .field("id", &self.slot.id)
            .field("scale", &self.scale)
            .field("allowed", &self.allowed)
            .finish_non_exhaustive()
    }
}

/// What happened on the desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopAction {
    /// An icon tap (one that passed its gate). The shell runs it with `launch`.
    Tap(String),
    /// A locked icon was tapped → an unlock request.
    TapLocked(String),
    /// A long press (the M6 info popover).
    LongPress(String),
}

/// The pieces of the shell [`DesktopView::ui`] needs.
pub struct DesktopCtx<'a> {
    /// The theme.
    pub theme: &'a Theme,
    /// The gate decisions.
    pub access: &'a Access,
    /// The icons.
    pub icons: &'a mut IconSet,
    /// The time.
    pub now: Instant,
    /// The label legibility correction (guide 09 §2.2).
    pub legibility: LabelLegibility,
    /// The string table: an icon's label is a key, looked up as it is drawn.
    pub strings: &'a crate::i18n::Strings,
}

/// One icon cell's record across frames. The list is built only on [`DesktopView::rebuild`] and the
/// render path only overwrites values — the label and badge galleys are cached so no string is laid
/// out per frame.
#[derive(Debug)]
struct IconRecord {
    /// The declaration id.
    id: String,
    /// The icon Rect drawn last frame (the value before the press scale).
    rect: Option<Rect>,
}

/// The press feedback state (A7). There is one pointer, so one cell is pressed.
#[derive(Debug, Clone, Copy)]
struct Press {
    /// The pressed cell's `egui::Id` (idle where there is none).
    key: Option<egui::Id>,
    /// The scale, `1 → press_scale`.
    scale: Animated<f32>,
    /// Last frame's time (for working out dt).
    last: Option<Instant>,
}

impl Press {
    fn new() -> Self {
        Self {
            key: None,
            scale: Animated::new(1.0),
            last: None,
        }
    }

    /// The start of a frame. It advances using the difference in shell time as dt (headless uses virtual time).
    fn tick(&mut self, now: Instant) -> bool {
        let dt = self.last.map_or(0.0, |last| {
            now.saturating_duration_since(last).as_secs_f32()
        });
        self.last = Some(now);
        self.scale.tick(dt)
    }

    /// One cell's scale. With `down`, towards the pressed target; otherwise back to 1.
    fn scale_for(&mut self, key: egui::Id, down: bool, tokens: &MotionTokens) -> f32 {
        if down {
            if self.key != Some(key) {
                self.key = Some(key);
                self.scale.snap(1.0);
            }
            if (self.scale.target() - tokens.press_scale).abs() > f32::EPSILON {
                self.scale.to(tokens.press_scale, tokens.press);
            }
            self.scale.value()
        } else if self.key == Some(key) {
            if (self.scale.target() - 1.0).abs() > f32::EPSILON {
                self.scale.to(1.0, tokens.press_release);
            }
            let value = self.scale.value();
            if !self.scale.is_animating() {
                self.key = None;
            }
            value
        } else {
            1.0
        }
    }

    fn is_animating(&self) -> bool {
        self.scale.is_animating()
    }

    /// A frame with no cell pressed: bring any remaining press back to 1.
    fn release(&mut self, tokens: &MotionTokens) {
        let Some(key) = self.key else { return };
        let _ = self.scale_for(key, false, tokens);
    }
}

/// Tracking the horizontal page drag (A4). There is one pointer, so one drag is in progress.
#[derive(Debug, Clone, Copy, Default)]
struct PageDrag {
    /// Where within the grid the press went down. `None` means this press is not a page candidate
    /// (it started on the dock or the indicator, or there is only one page).
    origin: Option<egui::Pos2>,
    /// Whether it has passed the slop and is driving the page. While it is, that frame's icon presses and taps are cancelled.
    active: bool,
}

/// The slot one declaration makes, plus its placement request. [`DesktopView::rebuild`] builds them
/// and [`DesktopView::place_all`] moves them onto the pages and the dock (when a screen size change
/// alters the automatic grid, they are placed again from the same list).
#[derive(Debug, Clone)]
struct SlotEntry {
    slot: IconSlot,
    /// The place set by the code declaration or a config override.
    placement: DesktopPlacement,
    /// The page set by `[[desktop.pages]]` in the config (with no position given, filling starts from this page).
    page_hint: Option<usize>,
    /// The dock order (the `desktop.dock` list's order, then the `.dock()` declarations).
    dock_order: Option<usize>,
    /// An internal marker for `place_all`.
    placed: bool,
}

/// The desktop view.
pub struct DesktopView {
    columns: u8,
    rows: u8,
    /// `columns = 0` → worked out automatically from the content Rect.
    auto_columns: bool,
    /// `rows = 0` → automatic.
    auto_rows: bool,
    entries: Vec<SlotEntry>,
    pages: Vec<Page>,
    dock: Dock,
    wallpaper: Wallpaper,
    /// The label legibility correction (guide 09 §2.2). [`LabelLegibility::None`] by default.
    legibility: LabelLegibility,
    label_lines: u8,
    page_index: usize,
    /// The per-icon records (last frame's Rect, the galley cache). Built only on `rebuild` / `place_all`.
    records: Vec<IconRecord>,
    /// The wrap width the label galleys were made at. A change in cell width throws the galleys away.
    label_width: f32,
    press: Press,
    /// The tap Rects of last frame's page indicator (as many as there are pages). Exposed as
    /// [`DesktopView::page_indicator_rect`] so that tests and integrators need not duplicate the
    /// layout arithmetic.
    indicator_rects: Vec<Rect>,
    /// The [`Wallpaper::Gradient`] mesh cache (not rebuilt while the `Rect` and the two colours are
    /// unchanged). A `Mesh` is two `Vec`s, so rebuilding it every frame is two heap allocations a
    /// frame.
    gradient: Option<(Rect, Color32, Color32, Arc<egui::Mesh>)>,
    /// The integrator's cell painter. Where there is one, it replaces the built-in cell rendering (the icon, the padlock, the badge, the label).
    slot_painter: Option<SlotPainter>,
    /// The page swipe's driving value (A4, M2). `page_index` is the settled page and `swipe.pos()` the real position.
    swipe: PageSwipe,
    /// The horizontal drag in progress (A4).
    drag: PageDrag,
    /// Last frame's grid width = one page's width `W` (A4's `pos = start − dx / W`). `ui` refreshes
    /// it every frame — it is not the union of the icon Rects (that is narrower than the screen,
    /// depending on the column count).
    page_width: f32,
    /// The info popover a long press brings up.
    info: IconInfo,
    /// `[desktop] long_press`.
    long_press: LongPressMode,
    /// This frame's long press, from the shell's gesture engine — matched against the cells as
    /// they are drawn, and gone at the end of the frame.
    held: Option<egui::Pos2>,
    /// **The press under way is spent**: a long press answered it, or it put the popover away. No
    /// icon takes its release and no page follows it. The next press clears it.
    spent: bool,
}

impl std::fmt::Debug for DesktopView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopView")
            .field("columns", &self.columns)
            .field("rows", &self.rows)
            .field("pages", &self.pages.len())
            .field("dock", &self.dock.slots.len())
            .finish_non_exhaustive()
    }
}

/// `[desktop] wallpaper` → a [`Wallpaper`].
///
/// It looks at three kinds of name in order: `"abyss"` (the procedural deep sea) → a palette role
/// name → `#RRGGBB`. None of them warns and gives the `background` role — unlike `[theme.palette]`,
/// this is **fail-open** (a shell failing to come up because a wallpaper could not be read would be
/// awkward).
fn wallpaper_from_config(cfg: &DesktopConfig) -> Wallpaper {
    if cfg.wallpaper.starts_with("file:") {
        // There is no `egui::Context` here, so no texture can be uploaded. Only the slot is taken and
        // the shell builder fills it through `ShellBuilder::image_loader` — with no loader, or on a
        // failure, this fallback stays and the device comes up.
        return Wallpaper::Solid(ColorRole::Background);
    }
    if cfg.wallpaper == "abyss" {
        #[cfg(feature = "brand")]
        {
            return crate::brand::abyss_wallpaper(crate::brand::AbyssParams::from_config(
                &cfg.abyss,
            ));
        }
        #[cfg(not(feature = "brand"))]
        {
            log::warn!(
                "[desktop] wallpaper = \"abyss\" but the `brand` feature is off - using background instead"
            );
            return Wallpaper::Solid(ColorRole::Background);
        }
    }
    ColorRole::parse(&cfg.wallpaper).map_or_else(
        || {
            crate::icons::IconColor::parse(&cfg.wallpaper).map_or_else(
                || {
                    log::warn!(
                        "[desktop] wallpaper = \"{}\" is neither abyss, a palette role, nor #RRGGBB - using background instead",
                        cfg.wallpaper
                    );
                    Wallpaper::Solid(ColorRole::Background)
                },
                |c| match c {
                    crate::icons::IconColor::Fixed(color) => Wallpaper::Fixed(color),
                    crate::icons::IconColor::Role(role) => Wallpaper::Solid(role),
                },
            )
        },
        Wallpaper::Solid,
    )
}

impl DesktopView {
    /// Build an empty view from the config. The shell fills the icons in from the declarations.
    #[must_use]
    pub fn from_config(cfg: &DesktopConfig) -> Self {
        let wallpaper = wallpaper_from_config(cfg);
        let legibility = LabelLegibility::parse(&cfg.label_legibility).unwrap_or_else(|| {
            log::warn!(
                "[desktop] label_legibility = \"{}\" is not a known mode (none | shadow | veil) - using none",
                cfg.label_legibility
            );
            LabelLegibility::None
        });
        Self {
            legibility,
            columns: if cfg.columns == 0 { 4 } else { cfg.columns },
            rows: if cfg.rows == 0 { 3 } else { cfg.rows },
            auto_columns: cfg.columns == 0,
            auto_rows: cfg.rows == 0,
            entries: Vec::new(),
            pages: Vec::new(),
            dock: Dock {
                slots: Vec::new(),
                placement: dock_placement(cfg),
            },
            wallpaper,
            label_lines: cfg.label_lines.max(1),
            page_index: 0,
            records: Vec::new(),
            label_width: 0.0,
            press: Press::new(),
            indicator_rects: Vec::new(),
            gradient: None,
            slot_painter: None,
            swipe: PageSwipe::new(1, 0, &MotionTokens::default().page),
            drag: PageDrag::default(),
            page_width: 1.0,
            info: IconInfo::new(),
            long_press: LongPressMode::from_config(&cfg.long_press),
            held: None,
            spent: false,
        }
    }

    /// The desktop layer's id.
    #[doc(hidden)]
    #[must_use]
    pub fn layer_id() -> egui::LayerId {
        egui::LayerId::new(egui::Order::Background, Self::area_id())
    }

    /// The desktop `Area`'s id.
    #[must_use]
    pub(crate) fn area_id() -> egui::Id {
        egui::Id::new("fairing.desktop")
    }

    /// Change where the dock goes. A band along the bottom, a rail down either side, a row
    /// along the top and a row across the desktop are all the same mechanism.
    pub fn set_dock_placement(&mut self, placement: DockPlacement) {
        self.dock.set_placement(placement);
    }

    /// Replace the wallpaper.
    pub fn set_wallpaper(&mut self, wallpaper: Wallpaper) {
        self.wallpaper = wallpaper;
    }

    /// The current wallpaper.
    #[must_use]
    pub fn wallpaper(&self) -> &Wallpaper {
        &self.wallpaper
    }

    /// Replace the cell painter. Usually given through
    /// [`crate::shell::ShellBuilder::slot_painter`]. With a painter in place, the built-in cell
    /// rendering (the icon, the padlock, the badge, the label, the press tint) is never called.
    pub fn set_slot_painter(&mut self, painter: SlotPainter) {
        self.slot_painter = Some(painter);
    }

    /// Whether a cell painter is in place.
    #[must_use]
    pub fn has_slot_painter(&self) -> bool {
        self.slot_painter.is_some()
    }

    /// Rebuild the pages and the dock from the registry's declarations plus the config overrides.
    ///
    /// The order is ① build the candidates from the declarations (screens then actions, only those
    /// with an icon), ② lay the `[[desktop.pages]]` overrides (label, icon, `locked`, `col`/`row`) on
    /// top, ③ settle the dock from the `desktop.dock` list and the `.dock()` declarations, then ④
    /// place what is left, positioned ones first and then in declaration order. An id that went into
    /// the dock is not placed on the grid again (the same id drawn twice makes `icon_rect`
    /// ambiguous). An id that was never declared warns and is ignored.
    pub(crate) fn rebuild(&mut self, registry: &Registry, cfg: &DesktopConfig) {
        let mut entries = slot_entries(registry);
        carry_badges(&self.entries, &mut entries);
        apply_overrides(&mut entries, cfg);
        apply_dock_order(&mut entries, cfg);
        self.entries = entries;
        self.place_all();
        self.swipe.configure(self.pages.len(), self.swipe_width());
        self.swipe
            .snap(self.page_index.min(self.pages.len().saturating_sub(1)));
    }

    /// Place [`Self::entries`] on the current grid (`columns × rows`). They are placed again from the
    /// original list so that the overrides are not lost when a screen size change alters the automatic
    /// grid.
    fn place_all(&mut self) {
        let Self {
            columns,
            rows,
            entries,
            pages,
            dock,
            page_index,
            records,
            label_width,
            ..
        } = self;
        let (columns, rows) = ((*columns).max(1), (*rows).max(1));
        pages.clear();
        // The placements are values the config and the integrator settled, so a rebuild must not clear them — only the slots are emptied.
        dock.slots.clear();
        for entry in entries.iter_mut() {
            entry.placed = false;
        }
        // 1) The dock (the config list's order, then the `.dock()` declarations' order).
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by_key(|i| {
            entries
                .get(*i)
                .and_then(|e| e.dock_order)
                .unwrap_or(usize::MAX)
        });
        for index in order {
            let Some(entry) = entries.get_mut(index) else {
                continue;
            };
            if entry.dock_order.is_some() && dock.push(entry.slot.clone()) {
                entry.placed = true;
            }
        }
        // 2) The positioned ones.
        for entry in entries.iter_mut() {
            if entry.placed {
                continue;
            }
            let DesktopPlacement::At(p) = entry.placement else {
                continue;
            };
            ensure_page(pages, usize::from(p.page), columns, rows);
            let put = pages
                .get_mut(usize::from(p.page))
                .is_some_and(|page| page.put(p.col, p.row, entry.slot.clone()));
            if put {
                entry.placed = true;
            } else {
                log::warn!(
                    "desktop: the slot for `{}` (page {}, col {}, row {}) is taken or outside the grid - placing it automatically",
                    entry.slot.id, p.page, p.col, p.row
                );
            }
        }
        // 3) The rest, in declaration order.
        for entry in entries.iter_mut() {
            if entry.placed || entry.placement == DesktopPlacement::None {
                continue;
            }
            place_auto(
                pages,
                entry.page_hint.unwrap_or(0),
                columns,
                rows,
                entry.slot.clone(),
            );
            entry.placed = true;
        }
        if pages.is_empty() {
            pages.push(Page::new(columns, rows));
        }
        *page_index = (*page_index).min(pages.len() - 1);
        // The record list is built only here (the render path only refreshes values).
        records.clear();
        for slot in pages
            .iter()
            .flat_map(|p| p.slots().map(|(_, _, s)| s))
            .chain(dock.slots.iter())
        {
            records.push(IconRecord {
                id: slot.id.clone(),
                rect: None,
            });
        }
        *label_width = 0.0;
    }

    /// Every page's icons (the dock excluded).
    #[must_use]
    pub fn icons(&self) -> Vec<&IconSlot> {
        self.pages
            .iter()
            .flat_map(|p| p.slots().map(|(_, _, s)| s))
            .collect()
    }

    /// The dock's slots.
    #[must_use]
    pub fn dock(&self) -> &[IconSlot] {
        self.dock.slots()
    }

    /// The pages.
    #[must_use]
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// The current page.
    #[must_use]
    pub fn page(&self) -> usize {
        self.page_index
    }

    /// Move to a page (out of range is ignored). It settles at once — to go by the spring, [`Self::swipe_to`].
    pub fn set_page(&mut self, index: usize) {
        if index < self.pages.len() {
            self.page_index = index;
            self.swipe.configure(self.pages.len(), self.swipe_width());
            self.swipe.snap(index);
        }
    }

    /// Move to a page by the spring, as an indicator tap does (A4). Under `reduce` it settles at once.
    pub fn swipe_to(&mut self, index: usize, tokens: &MotionTokens) {
        if index < self.pages.len() {
            self.swipe.configure(self.pages.len(), self.swipe_width());
            if tokens.reduce {
                self.swipe.snap(index);
                self.page_index = index;
            } else {
                self.swipe.go_to(index, &tokens.page);
            }
        }
    }

    /// The real page position (A4's `pos`). At rest it is `page() as f32`.
    #[doc(hidden)]
    #[must_use]
    pub fn page_pos(&self) -> f32 {
        self.swipe.pos()
    }

    /// The page swipe's state.
    #[must_use]
    pub fn swipe(&self) -> &PageSwipe {
        &self.swipe
    }

    /// Settle a running page spring at once — called by the shell when it starts something of **higher
    /// priority** (the shade). While a finger is driving the page it does nothing (that
    /// is not the same finger, and the release rules decide it).
    pub(crate) fn settle_page(&mut self) {
        if self.swipe.is_dragging() {
            return;
        }
        let page = self.swipe.page().min(self.pages.len().saturating_sub(1));
        self.swipe.snap(page);
        self.page_index = page;
    }

    /// Frame stage 5: advance the page spring. On settling it brings `page_index` into line. `true` while it is moving.
    pub fn tick(&mut self, dt: f32) -> bool {
        self.info.tick(dt);
        let moving = self.swipe.tick(dt);
        if !moving && !self.swipe.is_dragging() {
            let page = self.swipe.page().min(self.pages.len().saturating_sub(1));
            self.page_index = page;
        }
        moving
    }

    /// One page's width `W` — last frame's grid width (1 before the first frame).
    fn swipe_width(&self) -> f32 {
        self.page_width.max(1.0)
    }

    /// The A4 finger binding. Pressed within the grid, with a horizontal movement past the slop and
    /// **greater than the vertical**, it drives the page. `true` while it is active — that frame's icon
    /// presses and taps are cancelled.
    ///
    /// `raw` is [`raw_press`]'s result (where it went down, where it is now). On the frame of the
    /// release it is `None`, and [`PageSwipe::release`] is called there — egui's `velocity()` is still
    /// valid on that frame (the 100 ms window regression).
    fn drive_page_swipe(
        &mut self,
        ui: &egui::Ui,
        grid: Rect,
        raw: Option<(egui::Pos2, egui::Pos2)>,
        tokens: &MotionTokens,
    ) -> bool {
        let vx = ui.input(|i| i.pointer.velocity().x);
        let Some((origin, now)) = raw else {
            if self.drag.active {
                let target = self.swipe.release(vx, &tokens.page);
                if tokens.reduce {
                    self.swipe.snap(target);
                }
                self.page_index = target;
            }
            self.drag = PageDrag::default();
            return false;
        };
        if self.drag.origin.is_none() && self.pages.len() > 1 && grid.contains(origin) {
            self.drag.origin = Some(origin);
        }
        let Some(start) = self.drag.origin else {
            return false;
        };
        let delta = now - start;
        if !self.drag.active && delta.x.abs() > tokens.slop_px && delta.x.abs() > delta.y.abs() {
            self.drag.active = true;
            self.swipe.begin();
        }
        if self.drag.active {
            self.swipe.drag(delta.x, vx);
        }
        self.drag.active
    }

    /// The icon Rect drawn last frame (the A2 icon zoom's origin). `None` where it is not on the
    /// current page or the dock, or is `Hidden`. The press scale is not applied, so the A2 origin does
    /// not wobble.
    #[must_use]
    pub fn icon_rect(&self, id: &str) -> Option<Rect> {
        self.records
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.rect)
    }

    /// Whether the press feedback (A7) is running. It becomes `true` on **the very frame** of the
    /// press (the tint comes up at once) and `false` once the scale has come back to 1 after the
    /// release.
    #[must_use]
    pub fn is_pressed(&self) -> bool {
        self.press.key.is_some()
    }

    /// The tap Rect of a page indicator dot drawn last frame. With only one page it is not drawn, so
    /// `None` (the A4 horizontal swipe is M2 — in M1 this tap is the only way to change page).
    #[must_use]
    pub fn page_indicator_rect(&self, index: usize) -> Option<Rect> {
        self.indicator_rects
            .get(index)
            .copied()
            .filter(Rect::is_positive)
    }

    /// Refresh a badge. `false` where there is no such id.
    pub fn set_badge(&mut self, id: &str, badge: Option<&Badge>) -> bool {
        let mut found = false;
        for entry in &mut self.entries {
            if entry.slot.id == id {
                entry.slot.badge = badge.cloned();
                found = true;
            }
        }
        for page in &mut self.pages {
            for slot in page.slots_mut() {
                if slot.id == id {
                    slot.badge = badge.cloned();
                    found = true;
                }
            }
        }
        for slot in &mut self.dock.slots {
            if slot.id == id {
                slot.badge = badge.cloned();
                found = true;
            }
        }
        found
    }

    /// Work the automatic grid (`columns` / `rows = 0`) out from the content's width and height. A
    /// change in the value places everything again from the same declaration list — which happens once,
    /// on a screen rotation or a split.
    fn fit_grid(&mut self, grid: Rect, cell: f32) {
        if !self.auto_columns && !self.auto_rows {
            return;
        }
        let cell = cell.max(1.0);
        let columns = if self.auto_columns {
            ((grid.width() / cell).floor() as i32).clamp(1, i32::from(MAX_COLUMNS)) as u8
        } else {
            self.columns
        };
        let rows = if self.auto_rows {
            ((grid.height() / cell).floor() as i32).clamp(1, i32::from(MAX_ROWS)) as u8
        } else {
            self.rows
        };
        if (columns, rows) != (self.columns, self.rows) {
            self.columns = columns;
            self.rows = rows;
            self.place_all();
        }
    }

    /// Draw. `rect` is the whole content Rect. **Zero heap allocation per frame** — the slots are
    /// walked borrowed, and the Rect records and the galley cache have only their values overwritten
    /// (the regression is `examples/bench.rs`'s idle desktop).
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        cx: &mut DesktopCtx<'_>,
    ) -> Option<DesktopAction> {
        self.paint_wallpaper(ui.painter(), rect, cx.theme);
        // The veil goes **just above the wallpaper and below the icons** (guide 09 §2.2). Laid over the
        // icons it flattens the labels with them.
        if self.legibility == LabelLegibility::Veil {
            ui.painter().rect_filled(rect, 0.0, veil(cx.theme));
        }
        let metrics = cx.theme.metrics;
        // The automatic grid is reckoned on the area with the dock and the indicator taken out. A change
        // of placement can change the page count, so the area is worked out once more.
        self.fit_grid(self.grid_rect(rect, &metrics), metrics.icon_cell);
        // Advance the press animation. No repaint is requested here — the shell's stage 15 decides it
        // together, through `DesktopView::is_animating`.
        self.press.tick(cx.now);
        let grid = self.grid_rect(rect, &metrics);
        let cell = metrics
            .icon_cell
            .min(grid.width() / f32::from(self.columns.max(1)))
            .min(grid.height() / f32::from(self.rows.max(1)))
            .max(1.0);
        self.refresh_label_width(cell, metrics.desktop_label_pad);

        // A spent press neither presses an icon nor drives the page.
        let raw_press = if self.spent {
            None
        } else {
            raw_press(ui, Self::layer_id())
        };
        // A long press is the desktop's where the desktop is what the finger is on — not under the
        // shade, the prompt or a screen.
        let held = self
            .held
            .filter(|pos| !self.spent && ui.ctx().layer_id_at(*pos) == Some(Self::layer_id()));
        let mut long = None;
        // A4: a horizontal drag over the grid drives the page. The width `W` is the grid's.
        self.page_width = grid.width();
        // The rubber band is taken again from the current theme every frame — holding the value from
        // construction leaves the pull-back resistance at the end on its default however `[motion]
        // page.rubber` changes.
        self.swipe.set_rubber(cx.theme.motion.page.rubber);
        self.swipe.configure(self.pages.len(), grid.width());
        let drag_active = self.drive_page_swipe(ui, grid, raw_press, &cx.theme.motion);
        let mut action = None;
        let mut pressed = None;
        let goto_page;
        {
            let Self {
                pages,
                dock,
                records,
                page_index,
                columns,
                rows,
                label_lines,
                label_width,
                press,
                indicator_rects,
                slot_painter,
                swipe,
                info,
                spent,
                ..
            } = self;
            for record in records.iter_mut() {
                record.rect = None;
            }
            let mut paint = SlotPaint {
                records,
                press,
                label_lines: *label_lines,
                label_width: *label_width,
                label_row_h: label_row_h(ui.ctx(), cx.theme),
                pressed: &mut pressed,
                raw_press,
                drag_active,
                record_dx: Some(0.0),
                slot_painter: slot_painter.as_mut(),
                held,
                spent: *spent,
                info,
                long: &mut long,
            };
            // Only the two pages (`floor` and `ceil`) are drawn, at their x offsets. What is off-screen
            // is cut by the `Ui`'s clip (the content) — making more child `Ui`s would grow the `UiStack`
            // per frame.
            let mut drawn = usize::MAX;
            for (index, x) in swipe.visible() {
                if index == drawn {
                    continue;
                }
                drawn = index;
                let Some(page) = pages.get(index) else {
                    continue;
                };
                // The A2 origin Rect is recorded in **the settled page's** own coordinates (so the icon
                // zoom's origin does not wobble along mid-swipe).
                paint.record_dx = (index == *page_index).then_some(x);
                let shifted = grid.translate(egui::vec2(x, 0.0));
                if let Some(a) =
                    draw_grid(ui, page, shifted, cell, (*columns, *rows), cx, &mut paint)
                {
                    action = Some(a);
                }
            }
            paint.record_dx = Some(0.0);
            if let Some(a) = draw_dock(ui, dock, rect, cell, cx, &mut paint) {
                action = Some(a);
            }
            goto_page = draw_indicator(ui, rect, dock, pages.len(), swipe, cx, indicator_rects);
        }
        if let Some(index) = goto_page {
            self.swipe_to(index, &cx.theme.motion);
        }
        if pressed.is_none() {
            // With no cell pressed this frame, the release animation is run to the end.
            self.press.release(&cx.theme.motion);
        }
        if let (Some(DesktopAction::LongPress(id)), Some(cell)) = (&action, long) {
            self.answer_long_press(id, cell, &cx.theme.motion);
        }
        action
    }

    /// A long press on the icon `id`, drawn in `cell`, was the desktop's: its press is
    /// spent, so its release launches nothing, and the info popover comes up unless
    /// `[desktop] long_press = "none"`.
    fn answer_long_press(&mut self, id: &str, cell: Rect, tokens: &MotionTokens) {
        self.spent = true;
        if self.long_press == LongPressMode::Info {
            self.info.open(id, cell, tokens);
        }
    }

    /// The gesture engine reported a long press at `pos` this frame (frame stage 5). The cells
    /// drawn this frame decide whose it is.
    pub(crate) fn hold_at(&mut self, pos: egui::Pos2) {
        self.held = Some(pos);
    }

    /// A press went down this frame (frame stage 5). Over an open popover it puts the popover away,
    /// and it is spent; any other press starts afresh.
    pub(crate) fn note_press(&mut self, tokens: &MotionTokens) {
        if self.info.is_open() {
            self.info.close(tokens);
            self.spent = true;
        } else {
            self.spent = false;
        }
    }

    /// Put the info popover away (it fades where it is).
    pub(crate) fn close_info(&mut self, tokens: &MotionTokens) {
        self.info.close(tokens);
    }

    /// Whether the info popover is up and taking presses.
    pub(crate) fn info_open(&self) -> bool {
        self.info.is_open()
    }

    /// **Probe**: the icon whose info popover is up. `None` while it is closed or on its
    /// way out.
    #[doc(hidden)]
    #[must_use]
    pub fn info_icon(&self) -> Option<&str> {
        self.info.id()
    }

    /// **Probe**: the info popover's card as last drawn, while it is up.
    #[doc(hidden)]
    #[must_use]
    pub fn info_rect(&self) -> Option<Rect> {
        self.info.card()
    }

    /// Draw the info popover (frame stage 9, after the workspace and the rail) — the shield over
    /// `root` while it is up, the card inside `bounds`. A long press this frame did not match is
    /// dropped here.
    pub(crate) fn info_ui(
        &mut self,
        ctx: &egui::Context,
        root: Rect,
        bounds: Rect,
        cx: &mut DesktopCtx<'_>,
    ) {
        self.held = None;
        let Self { info, entries, .. } = self;
        let slot = info
            .subject()
            .and_then(|id| entries.iter().find(|e| e.slot.id == id))
            .map(|e| &e.slot);
        info.ui(ctx, root, bounds, slot, cx);
    }

    /// Whether an icon press (A7) is running. The shell folds it into
    /// [`crate::Shell::is_animating`] so that the repaint policy is settled in one place — the nav bar's
    /// presses use the shell's [`crate::motion::AnimationStore`], but the desktop holds state per cell
    /// and uses a store of its own.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.press.is_animating() || self.swipe.is_animating() || self.info.is_animating()
    }

    /// A change in cell width throws the label galleys away (they are rebuilt at the next layout).
    fn refresh_label_width(&mut self, cell: f32, pad: f32) {
        let width = (cell - pad).max(pad);
        if (self.label_width - width).abs() > 0.5 {
            self.label_width = width;
        }
    }

    /// The area the grid may use (with the dock and the indicator taken out). The indicator takes space
    /// only with **two or more pages**.
    fn grid_rect(&self, rect: Rect, metrics: &Metrics) -> Rect {
        let indicator = if self.pages.len() > 1 {
            metrics.page_indicator_height
        } else {
            0.0
        };
        // The indicator is always at the bottom. A dock against an edge takes from that edge, and one
        // crossing the desktop divides the content and leaves **the wider side**.
        let mut out = Rect::from_min_max(
            rect.min,
            egui::pos2(rect.max.x, (rect.max.y - indicator).max(rect.min.y)),
        );
        let t = self.dock.band_thickness(metrics);
        if t > 0.0 {
            out = match self.dock.placement() {
                DockPlacement::Edge(crate::gesture::Edge::Bottom) => Rect::from_min_max(
                    out.min,
                    egui::pos2(out.max.x, (out.max.y - t).max(out.min.y)),
                ),
                DockPlacement::Edge(crate::gesture::Edge::Top) => Rect::from_min_max(
                    egui::pos2(out.min.x, (out.min.y + t).min(out.max.y)),
                    out.max,
                ),
                DockPlacement::Edge(crate::gesture::Edge::Left) => Rect::from_min_max(
                    egui::pos2((out.min.x + t).min(out.max.x), out.min.y),
                    out.max,
                ),
                DockPlacement::Edge(crate::gesture::Edge::Right) => Rect::from_min_max(
                    out.min,
                    egui::pos2((out.max.x - t).max(out.min.x), out.max.y),
                ),
                DockPlacement::Band { axis, .. } => {
                    carve_around(out, self.dock.rect_in(rect, metrics), axis)
                }
            };
        }
        out
    }

    /// The label legibility correction (guide 09 §2.2).
    #[must_use]
    pub const fn legibility(&self) -> LabelLegibility {
        self.legibility
    }

    /// Change the label legibility correction. Used together with swapping the wallpaper at runtime —
    /// `Shadow` on changing to a photo background, `None` on coming back to a flat colour.
    pub const fn set_legibility(&mut self, legibility: LabelLegibility) {
        self.legibility = legibility;
    }

    /// The wallpaper. [`Wallpaper::Gradient`] caches its mesh to keep zero heap allocation per frame.
    fn paint_wallpaper(&mut self, painter: &egui::Painter, rect: Rect, theme: &Theme) {
        // **The pair is resolved before the match**, so it costs one line rather than a second copy
        // of every arm below it.
        match self.wallpaper.for_theme(theme.dark) {
            // Already picked above; a pair inside a pair keeps the inner one.
            Wallpaper::Themed { dark, light } => {
                let inner = if theme.dark { dark } else { light };
                log::warn!("[wallpaper] a Themed pair inside a Themed pair: {inner:?}");
            }
            Wallpaper::Solid(role) => {
                painter.rect_filled(rect, 0.0, theme.color(*role));
            }
            Wallpaper::Fixed(color) => {
                painter.rect_filled(rect, 0.0, *color);
            }
            Wallpaper::Gradient { top, bottom } => {
                let hit = self
                    .gradient
                    .as_ref()
                    .filter(|(r, t, b, _)| *r == rect && t == top && b == bottom);
                let mesh = if let Some((_, _, _, mesh)) = hit {
                    Arc::clone(mesh)
                } else {
                    let mut mesh = egui::Mesh::default();
                    mesh.colored_vertex(rect.left_top(), *top);
                    mesh.colored_vertex(rect.right_top(), *top);
                    mesh.colored_vertex(rect.right_bottom(), *bottom);
                    mesh.colored_vertex(rect.left_bottom(), *bottom);
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(0, 2, 3);
                    let mesh = Arc::new(mesh);
                    self.gradient = Some((rect, *top, *bottom, Arc::clone(&mesh)));
                    mesh
                };
                painter.add(egui::Shape::mesh(mesh));
            }
            Wallpaper::Texture { id } => {
                painter.image(
                    *id,
                    rect,
                    FULL_UV,
                    // Not a role colour but **the multiplicative identity** — it draws the texture in
                    // its original colours.
                    Color32::WHITE,
                );
            }
            Wallpaper::TextureFit {
                id,
                source_size,
                fit,
            } => paint_fitted(painter, rect, theme, *id, *source_size, *fit),
            Wallpaper::Owned {
                texture,
                source_size,
                fit,
                // `texture.id()` is `#[inline]` and takes no lock — the only `TextureHandle` method
                // that may be called from the paint loop.
            } => paint_fitted(painter, rect, theme, texture.id(), *source_size, *fit),
            Wallpaper::Painter(cb) => cb(painter, rect),
            Wallpaper::ThemedPainter(cb) => cb(painter, rect, theme),
        }
    }
}

/// **The label veil's colour** — the ground colour at the scrim's alpha.
///
/// It used to be `Scrim` itself, which is a **black alpha in every preset**. Over a dark wallpaper
/// that is right by accident: the labels are light, so darkening the picture is the direction that
/// helps them. Over a light one it is simply wrong — it greys a pale picture down and the dark
/// labels it was meant to serve gain nothing, which is what the light manta plate showed the moment
/// it was laid in.
///
/// So it pushes towards whatever the labels contrast *against*: `Background`, dark on a dark
/// palette and pale on a light one. The brand art already reasoned this way for its own veil ("under
/// a light preset a black veil is simply wrong", `brand::abyss`); this is the desktop catching up.
/// The alpha stays the scrim's. On a dark palette the colour moves from pure black to that
/// palette's own near-black ground — `#04121F` on Abyss — which is a small, deliberate shift: the
/// veil now tints towards the page the labels sit on rather than towards nothing in particular.
fn veil(theme: &Theme) -> egui::Color32 {
    let alpha = theme.color(ColorRole::Scrim).a();
    let ground = theme.color(ColorRole::Background);
    egui::Color32::from_rgba_unmultiplied(ground.r(), ground.g(), ground.b(), alpha)
}

/// Paint a texture keeping the original's ratio (shared by [`Wallpaper::TextureFit`] and [`Wallpaper::Owned`]).
fn paint_fitted(
    painter: &egui::Painter,
    rect: Rect,
    theme: &Theme,
    id: egui::TextureId,
    source_size: egui::Vec2,
    fit: Fit,
) {
    // `Contain` would have to push the UV outside `0..1` and so lean on texture wrapping — instead it
    // **shrinks the target Rect**.
    if fit == Fit::Contain && source_size.x > 0.0 && source_size.y > 0.0 {
        let scale = (rect.width() / source_size.x).min(rect.height() / source_size.y);
        let inner = Rect::from_center_size(rect.center(), source_size * scale);
        painter.rect_filled(rect, 0.0, theme.color(ColorRole::Background));
        painter.image(id, inner, FULL_UV, Color32::WHITE);
    } else {
        let uv = uv_for(fit, source_size, rect.size());
        painter.image(id, rect, uv, Color32::WHITE);
    }
}

/// The bundle of out-of-shell state [`draw_slot`] uses (the records, the presses, the label settings).
struct SlotPaint<'a> {
    records: &'a mut Vec<IconRecord>,
    press: &'a mut Press,
    label_lines: u8,
    label_width: f32,
    /// The **actual** height of one label line (du). Not the formula `desktop_label_size ×
    /// desktop_label_line` but the value egui was asked for — with a fallback font (CJK and the like)
    /// loaded, the line height is the maximum across the whole family and comes out larger than the
    /// formula, and laying the place out by the formula cuts the bottom off the text.
    label_row_h: f32,
    /// The cell pressed this frame.
    pressed: &'a mut Option<egui::Id>,
    /// This frame's raw press (where it went down, where it is now). egui's `Response` is the previous
    /// pass's interaction snapshot and so is a frame late — to keep A7's "answers on the first
    /// frame", the input state is read directly.
    raw_press: Option<(egui::Pos2, egui::Pos2)>,
    /// Where the horizontal page drag is active, this frame's presses and taps are cancelled (A4,
    /// "no icon tap mid-drag").
    drag_active: bool,
    /// Whether this page records the A2 origin Rect, and if so the x offset to subtract. `None` means
    /// it does not record (a neighbouring page mid-swipe).
    record_dx: Option<f32>,
    /// The integrator's cell painter. Where there is one, it replaces the built-in cell rendering.
    slot_painter: Option<&'a mut SlotPainter>,
    /// This frame's long press, where it is the desktop's.
    held: Option<egui::Pos2>,
    /// The press under way is spent — no cell presses or takes its release.
    spent: bool,
    /// The info popover, told where its icon is drawn.
    info: &'a mut IconInfo,
    /// The cell the long press landed in.
    long: &'a mut Option<Rect>,
}

impl SlotPaint<'_> {
    fn record(&mut self, id: &str) -> Option<&mut IconRecord> {
        self.records.iter_mut().find(|r| r.id == id)
    }
}

/// Extend the pages so that `index` exists.
fn ensure_page(pages: &mut Vec<Page>, index: usize, columns: u8, rows: u8) {
    while pages.len() <= index {
        pages.push(Page::new(columns, rows));
    }
}

/// Place in the first free cell from page `from` on. With no room, it adds a page.
fn place_auto(pages: &mut Vec<Page>, from: usize, columns: u8, rows: u8, slot: IconSlot) {
    ensure_page(pages, from, columns, rows);
    for page in pages.iter_mut().skip(from) {
        if let Some((col, row)) = page.first_free() {
            page.put(col, row, slot);
            return;
        }
    }
    pages.push(Page::new(columns, rows));
    let last = pages.len() - 1;
    if let Some(page) = pages.get_mut(last) {
        page.put(0, 0, slot);
    }
}

/// The registry's screen and action declarations as slot candidates (in insertion order).
fn slot_entries(registry: &Registry) -> Vec<SlotEntry> {
    let mut entries: Vec<SlotEntry> = Vec::new();
    for decl in registry.screens() {
        if let Some(icon) = decl.icon.clone() {
            entries.push(SlotEntry {
                slot: IconSlot {
                    id: decl.id.clone(),
                    label: decl.title.clone(),
                    description: decl.description.clone(),
                    icon,
                    gate: decl.gate.clone(),
                    visibility: decl.visibility,
                    badge: None,
                    launch: None,
                    kind: SlotKind::Screen,
                },
                placement: decl.desktop,
                page_hint: None,
                dock_order: decl.dock.then_some(usize::MAX - 1),
                placed: false,
            });
        }
    }
    for decl in registry.actions() {
        if let Some(icon) = decl.icon.clone() {
            entries.push(SlotEntry {
                slot: IconSlot {
                    id: decl.id.clone(),
                    label: decl.title.clone(),
                    description: decl.description.clone(),
                    icon,
                    gate: decl.gate.clone(),
                    visibility: decl.visibility,
                    badge: None,
                    launch: None,
                    kind: SlotKind::Action,
                },
                placement: decl.desktop,
                page_hint: None,
                dock_order: decl.dock.then_some(usize::MAX - 1),
                placed: false,
            });
        }
    }
    entries
}

/// Carry the previous badges over to the rebuilt candidates (`shell.set_badge` survives a rebuild).
fn carry_badges(previous: &[SlotEntry], entries: &mut [SlotEntry]) {
    for entry in entries.iter_mut() {
        if let Some(old) = previous.iter().find(|p| p.slot.id == entry.slot.id) {
            entry.slot.badge.clone_from(&old.slot.badge);
        }
    }
}

/// Lay the `[[desktop.pages]]` overrides (label, description, icon, `locked`, `col`/`row`) on top.
fn apply_overrides(entries: &mut [SlotEntry], cfg: &DesktopConfig) {
    for (page_no, page_cfg) in cfg.pages.iter().enumerate() {
        for over in &page_cfg.icons {
            let Some(entry) = entries.iter_mut().find(|e| e.slot.id == over.id) else {
                log::warn!(
                    "desktop.pages: ignoring `{}` - no such declaration",
                    over.id
                );
                continue;
            };
            if let Some(label) = &over.label {
                entry.slot.label.clone_from(label);
            }
            if let Some(description) = &over.description {
                entry.slot.description = Some(description.clone());
            }
            if let Some(icon) = &over.icon {
                match builtin::NAMES.iter().find(|n| **n == icon.as_str()) {
                    Some(name) => entry.slot.icon = IconRef::Builtin(name),
                    None => log::warn!(
                        "desktop.pages: icon name `{icon}` for `{}` is not in the built-in set (ignored)",
                        over.id
                    ),
                }
            }
            match over.locked.as_deref() {
                Some("hide") => entry.slot.visibility = Visibility::Hidden,
                Some("show") => entry.slot.visibility = Visibility::Locked,
                Some(other) => log::warn!(
                    "desktop.pages: locked = `{other}` for `{}` is neither \"show\" nor \"hide\" (ignored)",
                    over.id
                ),
                None => {}
            }
            entry.page_hint = Some(page_no);
            entry.placement = match (over.col, over.row) {
                (Some(col), Some(row)) => DesktopPlacement::At(crate::screen::Placement {
                    page: u8::try_from(page_no).unwrap_or(u8::MAX),
                    col,
                    row,
                }),
                _ => DesktopPlacement::Auto,
            };
        }
    }
}

/// Plant the `desktop.dock` list's order into the candidates. The `.dock()` declarations come after the list.
fn apply_dock_order(entries: &mut [SlotEntry], cfg: &DesktopConfig) {
    for (order, id) in cfg.dock.iter().enumerate() {
        match entries.iter_mut().find(|e| &e.slot.id == id) {
            Some(entry) => entry.dock_order = Some(order),
            None => log::warn!("desktop.dock: ignoring `{id}` - no such declaration"),
        }
    }
}

/// This frame's raw press: (where it went down, where it is now). It is handed back only while the
/// desktop layer is the topmost interactable layer at that spot — mid-A2 transition
/// (`interactable(false)`), or where a screen covers it, the press tint does not come up.
fn raw_press(ui: &egui::Ui, layer: egui::LayerId) -> Option<(egui::Pos2, egui::Pos2)> {
    let (down, origin, current) = ui.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.press_origin(),
            i.pointer.interact_pos(),
        )
    });
    if !down {
        return None;
    }
    let (origin, current) = (origin?, current?);
    (ui.ctx().layer_id_at(origin) == Some(layer)).then_some((origin, current))
}

/// A scale about a centre (the A7 press).
fn scale_about(rect: Rect, center: egui::Pos2, scale: f32) -> Rect {
    Rect::from_min_max(
        center + (rect.min - center) * scale,
        center + (rect.max - center) * scale,
    )
}

/// The current page's grid. The cells are square and the grid is centred.
fn draw_grid(
    ui: &mut egui::Ui,
    page: &Page,
    grid: Rect,
    cell: f32,
    dims: (u8, u8),
    cx: &mut DesktopCtx<'_>,
    paint: &mut SlotPaint<'_>,
) -> Option<DesktopAction> {
    let (columns, rows) = dims;
    let origin = egui::pos2(
        grid.center().x - cell * f32::from(columns) / 2.0,
        grid.center().y - cell * f32::from(rows) / 2.0,
    );
    let mut action = None;
    for (col, row, slot) in page.slots() {
        let cell_rect = Rect::from_min_size(
            origin + egui::vec2(f32::from(col) * cell, f32::from(row) * cell),
            egui::vec2(cell, cell),
        );
        if let Some(a) = draw_slot(ui, cell_rect, slot, false, cx, paint) {
            action = Some(a);
        }
    }
    action
}

/// Paint a band along an edge, fading only its inner edge.
///
/// The outer 2/3 is solid `color` and the inner 1/3 falls to alpha 0. Eight vertices and four
/// triangles, and whatever the background is (flat, a gradient, procedural, a photo) the boundary
/// does not read as a line.
fn soft_band(painter: &egui::Painter, rect: Rect, edge: crate::gesture::Edge, color: Color32) {
    /// What fraction of the band's thickness the fade takes.
    const FADE: f32 = 0.34;
    let (solid, fade) = match edge {
        crate::gesture::Edge::Bottom => (
            Rect::from_min_max(
                egui::pos2(rect.min.x, rect.min.y + rect.height() * FADE),
                rect.max,
            ),
            Rect::from_min_max(
                rect.min,
                egui::pos2(rect.max.x, rect.min.y + rect.height() * FADE),
            ),
        ),
        crate::gesture::Edge::Top => (
            Rect::from_min_max(
                rect.min,
                egui::pos2(rect.max.x, rect.max.y - rect.height() * FADE),
            ),
            Rect::from_min_max(
                egui::pos2(rect.min.x, rect.max.y - rect.height() * FADE),
                rect.max,
            ),
        ),
        crate::gesture::Edge::Left => (
            Rect::from_min_max(
                rect.min,
                egui::pos2(rect.max.x - rect.width() * FADE, rect.max.y),
            ),
            Rect::from_min_max(
                egui::pos2(rect.max.x - rect.width() * FADE, rect.min.y),
                rect.max,
            ),
        ),
        crate::gesture::Edge::Right => (
            Rect::from_min_max(
                egui::pos2(rect.min.x + rect.width() * FADE, rect.min.y),
                rect.max,
            ),
            Rect::from_min_max(
                rect.min,
                egui::pos2(rect.min.x + rect.width() * FADE, rect.max.y),
            ),
        ),
    };
    painter.rect_filled(solid, 0.0, color);
    // The fade band: transparent on the inner edge, `color` on the outer.
    let vertical = matches!(
        edge,
        crate::gesture::Edge::Left | crate::gesture::Edge::Right
    );
    let inner_first = matches!(
        edge,
        crate::gesture::Edge::Bottom | crate::gesture::Edge::Right
    );
    let (a, b) = if vertical {
        (
            [fade.left_top(), fade.left_bottom()],
            [fade.right_top(), fade.right_bottom()],
        )
    } else {
        (
            [fade.left_top(), fade.right_top()],
            [fade.left_bottom(), fade.right_bottom()],
        )
    };
    let (edge_a, edge_b) = if inner_first {
        (Color32::TRANSPARENT, color)
    } else {
        (color, Color32::TRANSPARENT)
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(a[0], edge_a);
    mesh.colored_vertex(a[1], edge_a);
    mesh.colored_vertex(b[0], edge_b);
    mesh.colored_vertex(b[1], edge_b);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(egui::Shape::mesh(mesh));
}

/// Hand back **the wider side** left after a crossing row has divided the content.
///
/// Where the row goes outside the content, nothing is cut — it then hides nothing.
fn carve_around(content: Rect, band: Rect, axis: Axis) -> Rect {
    let (lo, hi, c_lo, c_hi) = match axis {
        Axis::Horizontal => (band.min.y, band.max.y, content.min.y, content.max.y),
        Axis::Vertical => (band.min.x, band.max.x, content.min.x, content.max.x),
    };
    let before = (lo - c_lo).max(0.0);
    let after = (c_hi - hi).max(0.0);
    if before <= 0.0 && after <= 0.0 {
        return content;
    }
    let (keep_lo, keep_hi) = if before >= after {
        (c_lo, lo.min(c_hi))
    } else {
        (hi.max(c_lo), c_hi)
    };
    match axis {
        Axis::Horizontal => Rect::from_min_max(
            egui::pos2(content.min.x, keep_lo),
            egui::pos2(content.max.x, keep_hi),
        ),
        Axis::Vertical => Rect::from_min_max(
            egui::pos2(keep_lo, content.min.y),
            egui::pos2(keep_hi, content.max.y),
        ),
    }
}

/// The dock (fixed, with no cap on the count). While it is empty the band is not drawn either.
fn draw_dock(
    ui: &mut egui::Ui,
    dock: &Dock,
    rect: Rect,
    cell: f32,
    cx: &mut DesktopCtx<'_>,
    paint: &mut SlotPaint<'_>,
) -> Option<DesktopAction> {
    if dock.is_empty() {
        return None;
    }
    let metrics = cx.theme.metrics;
    let dock_rect = dock.rect_in(rect, &metrics);
    let count = dock.slots.len();
    #[expect(clippy::cast_precision_loss, reason = "the cell count is small")]
    let n = count as f32;
    let vertical = dock.is_vertical();
    // The cells are divided along the axis only. They shrink once they would fall below the physical
    // minimum touch size — the geometry settles it rather than a cap on the count.
    let span = if vertical {
        dock_rect.height()
    } else {
        dock_rect.width()
    };
    let step = (span / n).min(cell.max(1.0));
    let start = if vertical {
        dock_rect.center().y - step * n / 2.0
    } else {
        dock_rect.center().x - step * n / 2.0
    };
    let surface = cx
        .theme
        .color(ColorRole::Surface)
        .gamma_multiply(DOCK_BAND_TINT);
    match dock.placement() {
        // A dock against an edge is a band running right up to the edge — the screen's border is the
        // plate's border. Its **inner corners are softened**, though: painted as a flat rectangle it
        // becomes a knife line across the screen over a procedural background or a photograph and cuts
        // the water column dead (which is exactly what happened).
        DockPlacement::Edge(edge) => {
            soft_band(ui.painter(), dock_rect, edge, surface);
        }
        // A crossing row is **a floating plate**. It is widened only as far as the row of icons and its
        // corners rounded, so it reads as an object laid on the background rather than a horizontal line
        // cutting it.
        DockPlacement::Band { .. } => {
            let pad = metrics.corner_radius;
            let run = if vertical {
                Rect::from_min_max(
                    egui::pos2(dock_rect.min.x, start),
                    egui::pos2(dock_rect.max.x, start + step * n),
                )
            } else {
                Rect::from_min_max(
                    egui::pos2(start, dock_rect.min.y),
                    egui::pos2(start + step * n, dock_rect.max.y),
                )
            };
            let plate = run.expand2(egui::vec2(if vertical { 0.0 } else { pad }, pad * 0.5));
            // Rounded to a stadium, the plate reads as an oval smudge. It is rounded to twice the theme's
            // radius at most, and stops at half the short side.
            let radius = (pad * 2.0).min(plate.height().min(plate.width()) * 0.5);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the corner radius is a small positive pixel count"
            )]
            let radius = egui::CornerRadius::same(radius.clamp(0.0, 255.0) as u8);
            ui.painter().rect_filled(plate, radius, surface);
            ui.painter().rect_stroke(
                plate,
                radius,
                egui::Stroke::new(1.0, cx.theme.color(ColorRole::Outline)),
                egui::StrokeKind::Inside,
            );
        }
    }
    let mut action = None;
    for (i, slot) in dock.slots.iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "the cell count is small")]
        let offset = start + step * i as f32;
        let cell_rect = if vertical {
            Rect::from_min_size(
                egui::pos2(dock_rect.min.x, offset),
                egui::vec2(dock_rect.width(), step),
            )
        } else {
            Rect::from_min_size(
                egui::pos2(offset, dock_rect.min.y),
                egui::vec2(step, dock_rect.height()),
            )
        };
        if let Some(a) = draw_slot(ui, cell_rect, slot, true, cx, paint) {
            action = Some(a);
        }
    }
    action
}

/// The page indicator. A tap goes to that page. The dots **cross-fade** by A4's weight
/// `1 − |i − pos|` — the radius `DOT_R_OFF → DOT_R_ON`, the colour `Muted → OnSurface`. Mid-swipe two
/// dots are half lit at once. It hands back the page pressed.
fn draw_indicator(
    ui: &mut egui::Ui,
    rect: Rect,
    dock: &Dock,
    count: usize,
    swipe: &PageSwipe,
    cx: &DesktopCtx<'_>,
    rects: &mut Vec<Rect>,
) -> Option<usize> {
    rects.clear();
    if count <= 1 {
        return None;
    }
    rects.resize(count, Rect::NOTHING);
    let metrics = cx.theme.metrics;
    // The indicator sits at the bottom of the screen. With a bottom-band dock it goes above it — a dock
    // anywhere else (the top, the sides, a crossing row) does not cover the bottom, so it does not push it.
    let dock_h = if matches!(
        dock.placement(),
        DockPlacement::Edge(crate::gesture::Edge::Bottom)
    ) {
        dock.band_thickness(&metrics)
    } else {
        0.0
    };
    let y = rect.max.y - dock_h - metrics.page_indicator_height / 2.0;
    let hit_height = metrics
        .page_indicator_height
        .max(metrics.touch_target * metrics.page_indicator_hit_ratio);
    let mut goto = None;
    for i in 0..count {
        let step = metrics.page_indicator_step;
        let x = rect.center().x + (i as f32 - (count as f32 - 1.0) / 2.0) * step;
        let center = egui::pos2(x, y);
        let hit = Rect::from_center_size(center, egui::vec2(step, hit_height));
        if let Some(slot) = rects.get_mut(i) {
            *slot = hit;
        }
        let response = ui.interact(
            hit,
            egui::Id::new(("fairing.desktop.page", i)),
            Sense::click(),
        );
        if response.clicked() {
            goto = Some(i);
        }
        let weight = swipe.dot_weight(i);
        let color = cx
            .theme
            .color(ColorRole::Muted)
            .lerp_to_gamma(cx.theme.color(ColorRole::OnSurface), weight);
        let (off, on) = (
            metrics.page_indicator_dot_off,
            metrics.page_indicator_dot_on,
        );
        let radius = (on - off).mul_add(weight, off);
        ui.painter().circle_filled(center, radius, color);
    }
    goto
}

/// One cell: the gate filter → the icon plus the label plus the badge → the tap. It hands back what
/// happened.
///
/// The hit test, the press decision, the gate filter and recording the A2 origin Rect are **always
/// the shell's**. Where there is an integrator [`SlotPainter`], only the drawing is handed over —
/// the press tint is the painter's too.
fn draw_slot(
    ui: &mut egui::Ui,
    cell: Rect,
    slot: &IconSlot,
    in_dock: bool,
    cx: &mut DesktopCtx<'_>,
    paint: &mut SlotPaint<'_>,
) -> Option<DesktopAction> {
    // The gate is settled when the slot is placed (`IconSlot::resolve_gate`). `None` happens only where
    // an integrator cleared it through `slots_mut()`, so it is taken by the name decision — neither path
    // builds a `Gate` per frame.
    let allowed = match &slot.gate {
        Some(gate) => cx.access.allows(gate),
        None => cx.access.allows_name(&slot.id),
    };
    // Hidden: on a failed gate, as though it were not there at all.
    if !allowed && slot.visibility == Visibility::Hidden {
        return None;
    }
    let metrics = cx.theme.metrics;
    let key = egui::Id::new(("fairing.desktop.slot", &slot.id));
    let response = ui.interact(cell, key, Sense::click());
    // A `Response`'s press is the previous pass's snapshot and so is a frame late. The raw input is read
    // alongside it so it answers on the very frame of the press (A7, "≤ 16 ms").
    let down = !paint.drag_active
        && !paint.spent
        && (response.is_pointer_button_down_on()
            || paint
                .raw_press
                .is_some_and(|(origin, now)| cell.contains(origin) && cell.contains(now)));
    if down {
        *paint.pressed = Some(key);
    }
    paint.info.see(&slot.id, cell);
    let scale = paint.press.scale_for(key, down, &cx.theme.motion);

    let icon_size = metrics
        .icon_size
        .min(cell.width() * metrics.desktop_icon_ratio);
    // **The larger** of the measured row height and the token formula is used. The measurement alone
    // gets denser than today depending on the font; the formula alone cuts the bottom off the text with
    // a fallback font.
    let line_h = paint
        .label_row_h
        .max(metrics.desktop_label_size * metrics.desktop_label_line);
    let label_h = f32::from(paint.label_lines) * line_h;
    let content_h = icon_size + metrics.desktop_label_gap + label_h;
    let top = cell.min.y + ((cell.height() - content_h) / 2.0).max(metrics.desktop_content_min_pad);
    let icon_rect = Rect::from_center_size(
        egui::pos2(cell.center().x, top + icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    // What is recorded is the Rect before the press (so the A2 origin does not wobble mid-press), in
    // **the settled page's** coordinates with the page-swipe offset taken out (A4).
    if let Some(dx) = paint.record_dx {
        if let Some(record) = paint.record(&slot.id) {
            record.rect = Some(icon_rect.translate(egui::vec2(-dx, 0.0)));
        }
    }
    let drawn_icon = scale_about(icon_rect, cell.center(), scale);
    if let Some(painter) = paint.slot_painter.as_deref_mut() {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cell));
        painter(
            &mut child,
            SlotCx {
                cell,
                icon: icon_rect,
                pressed_icon: drawn_icon,
                slot,
                scale,
                pressed: down,
                allowed,
                in_dock,
                theme: cx.theme,
                icons: &mut *cx.icons,
                now: cx.now,
                strings: cx.strings,
            },
        );
    } else {
        draw_slot_content(
            ui, cell, drawn_icon, icon_rect, slot, allowed, down, scale, cx, paint,
        );
    }

    if paint.drag_active {
        // A4's "no icon tap mid-drag" — a press past the slop is taken by the page.
        return None;
    }
    if response.clicked() && !paint.spent {
        return Some(if allowed {
            DesktopAction::Tap(slot.id.clone())
        } else {
            DesktopAction::TapLocked(slot.id.clone())
        });
    }
    // The gesture engine's long press, not egui's `long_touched()`: that one needs real touch
    // events and waits out `max_click_duration` (0.8 s), so it never fired on a panel that
    // delivers touch as the mouse (the quick tiles had the same problem).
    if paint.held.is_some_and(|pos| cell.contains(pos)) {
        *paint.long = Some(cell);
        return Some(DesktopAction::LongPress(slot.id.clone()));
    }
    None
}

/// The built-in cell rendering: the press tint → the icon → the padlock → the badge → the label.
#[allow(clippy::too_many_arguments)] // These are the values drawing one cell needs (grouping them buys nothing).
fn draw_slot_content(
    ui: &egui::Ui,
    cell: Rect,
    drawn_icon: Rect,
    icon_rect: Rect,
    slot: &IconSlot,
    allowed: bool,
    down: bool,
    scale: f32,
    cx: &mut DesktopCtx<'_>,
    paint: &mut SlotPaint<'_>,
) {
    let metrics = cx.theme.metrics;
    let painter = ui.painter().clone();
    // A7: it answers on the first frame with the tint, and the scale shrinks over 80 ms.
    if down {
        painter.rect_filled(
            cell.shrink(metrics.desktop_press_inset),
            metrics.corner_radius,
            cx.theme.color(ColorRole::Pressed),
        );
    }
    // The stroke is a token in 24-grid units (2.0 by default — the same as `IconStyle`'s default before the promotion).
    let style = IconStyle {
        stroke: Some(metrics.desktop_icon_stroke),
        ..IconStyle::sized(drawn_icon.width()).enabled(allowed)
    };
    // The icons get the same prescription as the labels — a white stroked icon disappears before a label
    // does on a bright background (the wrench over the white belly of the manta original does).
    if cx.legibility == LabelLegibility::Shadow {
        let drop = (drawn_icon.width() * ICON_SHADOW_DROP).max(1.0);
        let shadow = IconStyle {
            color: IconColor::Fixed(shadow_color(cx.theme)),
            ..style
        };
        cx.icons.paint(
            &painter,
            drawn_icon.translate(egui::vec2(0.0, drop)),
            &slot.icon,
            &shadow,
            cx.theme,
        );
    }
    if !cx
        .icons
        .paint(&painter, drawn_icon, &slot.icon, &style, cx.theme)
    {
        // An empty generated set, or a name it does not know: the placeholder circle.
        painter.circle_stroke(
            drawn_icon.center(),
            drawn_icon.width() / 2.0 - 2.0,
            egui::Stroke::new(2.0, style.resolve_color(cx.theme)),
        );
    }
    if !allowed {
        draw_lock(&painter, drawn_icon, cx);
    }
    draw_badge(&painter, drawn_icon, slot, cx);
    draw_label(&painter, cell, icon_rect, slot, allowed, scale, cx, paint);
}

/// A failed gate plus `Visibility::Locked`: the padlock badge.
fn draw_lock(painter: &egui::Painter, icon: Rect, cx: &mut DesktopCtx<'_>) {
    let lock = cx.theme.metrics.desktop_lock_size;
    // The border padding is added **to the outer circle only**. Growing `lock` itself would grow `inner`
    // and `IconStyle` below with it and have the padlock fill the circle (which is what happened once
    // during the promotion, and the pixel comparison caught it).
    let ring = lock + cx.theme.metrics.desktop_lock_ring_pad;
    let badge = Rect::from_center_size(icon.right_bottom(), egui::vec2(ring, ring));
    painter.circle_filled(
        badge.center(),
        badge.width() / 2.0,
        cx.theme.color(ColorRole::Surface),
    );
    let inner = Rect::from_center_size(badge.center(), egui::vec2(lock, lock));
    let style = IconStyle::sized(lock).color(crate::icons::IconColor::Role(ColorRole::Warning));
    if !cx
        .icons
        .paint(painter, inner, &builtin::LOCK, &style, cx.theme)
    {
        painter.circle_filled(
            badge.center(),
            cx.theme.metrics.desktop_badge_dot_r,
            cx.theme.color(ColorRole::Warning),
        );
    }
}

/// The `Count` / `Dot` / `Text` badge.
fn draw_badge(painter: &egui::Painter, icon: Rect, slot: &IconSlot, cx: &DesktopCtx<'_>) {
    let Some(badge) = &slot.badge else {
        return;
    };
    // Through the widget rather than a pill and a galley of its own. The pair it replaces wrote
    // `OnPrimary` on `Danger`, which measures **2.79** in base dark — a count the operator is
    // meant to read, 38 % under the 4.5 a word needs. `CountBadge` carries the ink per tone, so
    // the digits here go from white to near-black in three of the four palettes. That is the fix.
    // The rect it returns is the space it occupied, halo included; a dock icon has nothing to
    // keep clear of it, so it is dropped here rather than threaded through the cell layout.
    let _occupied = CountBadge::new(badge.as_value())
        .tone(BadgeTone::Alert)
        .paint_over(painter, cx.theme, icon, BadgeAnchor::TopEnd);
}

/// The label (wrapped over `label_lines` lines, with `…` where it overflows). The galley is rebuilt only when the width changes.
#[allow(clippy::too_many_arguments)] // An internal helper that takes the painter, the geometry and the state in one go.
fn draw_label(
    painter: &egui::Painter,
    cell: Rect,
    icon: Rect,
    slot: &IconSlot,
    allowed: bool,
    scale: f32,
    cx: &DesktopCtx<'_>,
    paint: &mut SlotPaint<'_>,
) {
    let lines = paint.label_lines;
    let width = paint.label_width;
    // **The galleys are not held across frames.** A `Galley` carries UVs into the font atlas, so when a
    // new glyph arrives and the atlas grows, those UVs shift wholesale — a cached galley draws the wrong
    // characters from then on (reproducible straight away with a CJK font loaded). epaint keeps a cache
    // of its own inside `layout_job` (`GalleyCache`) and clears it when the fonts change, so calling it
    // every frame is both cheap and right.
    let mut job = LayoutJob::simple(
        cx.strings.get(&slot.label).to_owned(),
        egui::FontId::proportional(cx.theme.metrics.desktop_label_size),
        Color32::PLACEHOLDER,
        width,
    );
    job.halign = egui::Align::Center;
    job.wrap.max_rows = usize::from(lines);
    job.wrap.overflow_character = Some('…');
    let galley = painter.layout_job(job);
    let galley = &galley;
    let color = if allowed {
        cx.theme.color(ColorRole::OnSurface)
    } else {
        cx.theme.color(ColorRole::Muted)
    };
    let anchor = egui::pos2(
        cell.center().x,
        icon.max.y + cx.theme.metrics.desktop_label_gap,
    );
    let pos = cell.center() + (anchor - cell.center()) * scale;
    // The shadow comes **first** — it stops the label being lost on a bright background (a photograph, an
    // art plate). egui has no blur, so it is approximated with one dark copy offset by a step. Offset by
    // two or more it reads double, and a higher alpha makes the label look thick on a dark background.
    if cx.legibility == LabelLegibility::Shadow {
        let drop = (cx.theme.metrics.desktop_label_size * SHADOW_DROP).max(1.0);
        painter.galley(
            pos + egui::vec2(0.0, drop),
            Arc::clone(galley),
            shadow_color(cx.theme),
        );
    }
    painter.galley(pos, Arc::clone(galley), color);
}

/// The label shadow's offset (relative to the text size).
const SHADOW_DROP: f32 = 0.07;
/// The icon shadow's offset (relative to the icon's width). Being a larger shape than the label, a smaller ratio is enough.
const ICON_SHADOW_DROP: f32 = 0.045;
/// The label shadow's alpha. Enough to save the text on a bright background without showing on a dark one.
const SHADOW_ALPHA: u8 = 170;

/// The label shadow's colour. It takes [`ColorRole::Scrim`]'s hue but pins the alpha to the shadow's
/// own — a palette with a faint scrim must not make the shadow disappear.
fn shadow_color(theme: &Theme) -> Color32 {
    let scrim = theme.color(ColorRole::Scrim);
    Color32::from_rgba_unmultiplied(scrim.r(), scrim.g(), scrim.b(), SHADOW_ALPHA)
}

/// The actual height of one label line (du). egui's line height is the maximum across **the whole
/// font family**, so with a CJK fallback font loaded it comes out larger than the formula
/// `desktop_label_size × desktop_label_line`. It is called once per frame — the call takes the
/// context's write lock, so it must not be called per cell.
fn label_row_h(ctx: &egui::Context, theme: &Theme) -> f32 {
    let font = egui::FontId::proportional(theme.metrics.desktop_label_size);
    ctx.fonts_mut(|fonts| fonts.row_height(&font))
}

/// `[desktop] dock_edge` · `dock_band` → [`DockPlacement`].
fn dock_placement(cfg: &DesktopConfig) -> DockPlacement {
    if let Some(at) = cfg.dock_band {
        // A crossing row's direction is settled by the edge name — `left` / `right` makes it vertical.
        let axis = match cfg.dock_edge.as_str() {
            "left" | "right" => Axis::Vertical,
            _ => Axis::Horizontal,
        };
        return DockPlacement::Band { axis, at };
    }
    let edge = match cfg.dock_edge.as_str() {
        "top" => crate::gesture::Edge::Top,
        "left" => crate::gesture::Edge::Left,
        "right" => crate::gesture::Edge::Right,
        "bottom" => crate::gesture::Edge::Bottom,
        other => {
            log::warn!("[desktop] dock_edge = \"{other}\" is not a known edge - using bottom");
            crate::gesture::Edge::Bottom
        }
    };
    DockPlacement::Edge(edge)
}

/// The vertical gap between rail entries (relative to the row height).
const RAIL_GAP: f32 = 0.16;
/// A rail entry's corners = `corner_radius` × this.
const RAIL_RADIUS: f32 = 2.2;

impl DesktopView {
    /// The **first openable** icon id on the rail.
    ///
    /// In a rail layout the state of being at home is awkward — there is no kiosk with only a menu and
    /// nothing beside it. The shell uses this to open the first screen automatically.
    #[must_use]
    pub(crate) fn first_rail_entry(&self, access: &Access) -> Option<&str> {
        self.entries
            .iter()
            .map(|e| &e.slot)
            .find(|slot| slot.gate.as_ref().is_none_or(|gate| access.allows(gate)))
            .map(|slot| slot.id.as_str())
    }

    /// **The icon rail** — the icons drawn in one vertical column (the kiosk layout).
    ///
    /// # What is different from the grid
    ///
    /// [`DesktopView::ui`]'s grid is the home screen, and an opened screen **covers** it (A2). The
    /// rail is the opposite — the shell draws it **every frame** and a screen opens only in the area
    /// left over ([`Layout::rail`](crate::shell::Layout::rail)). The menu down the left of a shop kiosk
    /// is exactly this shape: the categories are always in view and only the content changes.
    ///
    /// So there is no wallpaper, no page turning and no dock. It draws as many as fit in one column and
    /// scrolls vertically past that.
    ///
    /// `open` is the declaration id of the screen open right now — that entry is highlighted. Without
    /// seeing which of the list is open, a rail is just a row of buttons.
    pub(crate) fn rail_ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        open: Option<&str>,
        cx: &mut DesktopCtx<'_>,
    ) -> Option<DesktopAction> {
        let m = cx.theme.metrics;
        ui.painter()
            .rect_filled(rect, 0.0, cx.theme.color(ColorRole::Surface));
        // One solid line between the rail and the content. Colour alone does not divide the two areas —
        // depending on the palette, Surface and Background can be close (as happened with the settings cards).
        let border_x = if rect.center().x < ui.max_rect().center().x {
            rect.max.x
        } else {
            rect.min.x
        };
        ui.painter().vline(
            border_x,
            rect.top()..=rect.bottom(),
            egui::Stroke::new(1.0, cx.theme.color(ColorRole::Outline)),
        );

        let mut action = None;
        let inset = m.screen_inset;
        let item_h = m.icon_cell.max(m.touch_target);
        let gap = m.row_height * RAIL_GAP;
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(inset * 0.5, 0.0)))
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        // The rail is drawn on the shell's own layer, which `layer_id_at` may not count as an area:
        // a long press is the rail's where nothing else is over the spot.
        let layer = ui.layer_id();
        let held = self.held.filter(|pos| {
            !self.spent
                && rect.contains(*pos)
                && ui.ctx().layer_id_at(*pos).is_none_or(|at| at == layer)
        });
        let spent = self.spent;
        let mut long = None;
        let Self { entries, info, .. } = self;
        egui::ScrollArea::vertical()
            .id_salt("fairing.desktop.rail")
            .max_height(rect.height())
            .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                ui.add_space(gap);
                for entry in entries.iter() {
                    let slot = &entry.slot;
                    let allowed = slot.gate.as_ref().is_none_or(|gate| cx.access.allows(gate));
                    if !allowed && slot.visibility == Visibility::Hidden {
                        continue;
                    }
                    let (item, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), item_h),
                        egui::Sense::click(),
                    );
                    info.see(&slot.id, item);
                    if response.clicked() && !spent {
                        action = Some(if allowed {
                            DesktopAction::Tap(slot.id.clone())
                        } else {
                            DesktopAction::TapLocked(slot.id.clone())
                        });
                    }
                    if held.is_some_and(|pos| item.contains(pos)) {
                        long = Some(item);
                        action = Some(DesktopAction::LongPress(slot.id.clone()));
                    }
                    let is_open = open == Some(slot.id.as_str());
                    if is_open || (response.is_pointer_button_down_on() && !spent) {
                        ui.painter().rect_filled(
                            item.shrink(2.0),
                            egui::CornerRadius::same(rail_radius(m.corner_radius)),
                            cx.theme.color(if is_open {
                                ColorRole::SurfaceVariant
                            } else {
                                ColorRole::Pressed
                            }),
                        );
                    }
                    paint_rail_item(ui, item, slot, allowed, is_open, cx);
                    ui.add_space(gap);
                }
            });
        if let (Some(DesktopAction::LongPress(id)), Some(cell)) = (&action, long) {
            self.answer_long_press(id, cell, &cx.theme.motion);
        }
        action
    }
}

/// One rail entry — the icon above, the label below.
///
/// Putting the label **below** the icon is the same convention as the grid. Beside it, a two-line
/// label is cut off whole on a narrow rail.
fn paint_rail_item(
    ui: &egui::Ui,
    item: Rect,
    slot: &IconSlot,
    allowed: bool,
    is_open: bool,
    cx: &mut DesktopCtx<'_>,
) {
    let m = cx.theme.metrics;
    let icon_size = m.icon_size * 0.72;
    let label_size = m.desktop_label_size;
    let icon_rect = Rect::from_center_size(
        egui::pos2(item.center().x, item.top() + icon_size * 0.72),
        egui::Vec2::splat(icon_size),
    );
    let color = if !allowed {
        ColorRole::Muted
    } else if is_open {
        ColorRole::Primary
    } else {
        ColorRole::OnSurface
    };
    let style = IconStyle {
        stroke: Some(m.desktop_icon_stroke),
        ..IconStyle::sized(icon_size)
            .enabled(allowed)
            .color(IconColor::Role(color))
    };
    cx.icons
        .paint(ui.painter(), icon_rect, &slot.icon, &style, cx.theme);
    let job = egui::text::LayoutJob::simple(
        cx.strings.get(&slot.label).to_owned(),
        egui::FontId::proportional(label_size),
        Color32::PLACEHOLDER,
        item.width(),
    );
    let galley = ui.painter().layout_job(egui::text::LayoutJob {
        halign: egui::Align::Center,
        wrap: egui::text::TextWrapping {
            max_rows: 1,
            overflow_character: Some('…'),
            ..job.wrap.clone()
        },
        ..job
    });
    ui.painter().galley(
        egui::pos2(item.center().x, icon_rect.max.y + m.desktop_label_gap),
        galley,
        cx.theme.color(color),
    );
}

/// A rail entry's corner radius (`CornerRadius` is a `u8`).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "it is after clamp(0,255), so it is inside u8's range"
)]
fn rail_radius(corner: f32) -> u8 {
    (corner * RAIL_RADIUS).round().clamp(0.0, 255.0) as u8
}
