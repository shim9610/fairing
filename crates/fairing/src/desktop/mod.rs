//! The desktop: page grid and dock, with page changes through indicator taps,
//! horizontal swipes or application requests.
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
//! Per-icon records and selected geometry are reused between frames. Rendering can
//! still allocate, including label-layout strings and translated icon paths.
//! The `idle desktop` scenario in `examples/bench.rs` measures one reference workload.

// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the rest of pedantic stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod grid_render;
use grid_render::{carve_around, draw_grid, soft_band};
mod dock_render;
use dock_render::{dock_placement, draw_dock, draw_indicator, paint_rail_item, rail_radius};
mod icon_render;
use icon_render::{draw_slot, label_row_h};

mod page_state;
mod rail_render;
mod render;

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

/// Dock placement: attach the fixed icon row to an edge or place it as a band.
/// The placement determines its orientation and occupied desktop region.
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

/// Desktop wallpaper source. Raster textures are decoded and uploaded by the
/// application. This enum is `#[non_exhaustive]`; downstream matches need a wildcard
/// arm for future variants.
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
    /// `source_size` is read and cached once during construction by [`Wallpaper::owned`],
    /// avoiding a repeated texture-manager metadata query while painting.
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

/// Desktop-label treatment: no correction (default), text shadow or content veil.
/// Choose a treatment after checking rendered labels against the actual wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LabelLegibility {
    /// Do nothing. **The default.**
    #[default]
    None,
    /// A dark shadow behind the label text.
    Shadow,
    /// A translucent tinted plate over the content. Check the composited label contrast.
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
/// rebuilding the device**. With no loader given, `file:` logs a warning and uses the
/// `background` role for this wallpaper request.
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
    /// The automatic grid's two candidates (on the whole area, and with the indicator taken out)
    /// it was last fitted from — fitted again only when one changes, or the entries do.
    fitted: Option<((u8, u8), (u8, u8))>,
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

/// Parse the named wallpaper: procedural `"abyss"`, palette role or `#RRGGBB`.
/// An unrecognized name logs a warning and returns the background role.
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
            fitted: None,
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
        egui::Id::unique("fairing.desktop")
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
}

/// Label-veil colour: the theme background RGB with the scrim alpha.
///
/// This tints the wallpaper toward the background used by the label palette.
/// Check composited label contrast when changing the wallpaper or palette.
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

/// The label shadow's offset (relative to the text size).
const SHADOW_DROP: f32 = 0.07;
/// The icon shadow's offset (relative to the icon's width). Being a larger shape than the label, a smaller ratio is enough.
const ICON_SHADOW_DROP: f32 = 0.045;
/// Opacity used when painting the label shadow.
const SHADOW_ALPHA: u8 = 170;

/// The vertical gap between rail entries (relative to the row height).
const RAIL_GAP: f32 = 0.16;
/// A rail entry's corners = `corner_radius` × this.
const RAIL_RADIUS: f32 = 2.2;

impl DesktopView {}

#[cfg(test)]
mod tests {
    use super::{dock_placement, DockPlacement};
    use crate::config::DesktopConfig;
    use crate::gesture::Edge;

    /// A `dock_band` that is not a number leaves the dock on its edge rather than drawn nowhere.
    #[test]
    fn a_nan_dock_band_is_the_edges_dock() {
        let cfg = DesktopConfig {
            dock_band: Some(f32::NAN),
            ..DesktopConfig::default()
        };
        assert_eq!(dock_placement(&cfg), DockPlacement::Edge(Edge::Bottom));
        let cfg = DesktopConfig {
            dock_band: Some(0.5),
            ..DesktopConfig::default()
        };
        assert!(matches!(dock_placement(&cfg), DockPlacement::Band { .. }));
    }
}
