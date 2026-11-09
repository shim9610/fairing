//! Screen-body layout helpers built on egui.
//!
//! [`shell::Layout`](crate::shell::Layout) describes the shell's content and chrome
//! regions. This module places content within the `Ui` handed to a screen.
//! The application supplies domain content and callback behavior.
//!
//! - Containers: [`page`], [`group`], [`Grid`], [`action_bar`], [`split_width`],
//!   [`split_height`], [`title`], [`section`] and [`note`].
//! - Rows: [`switch_row`], [`slider_row`], [`choice_rows`], [`info_row`],
//!   [`nav_row`], [`icon_row`], [`list_item`], [`ExpandableRow`], [`accordion`],
//!   [`advanced_rows`] and [`status_card`].
//! - Fitting: [`fit_text`], [`fit_size`], [`contain`] and [`cover_uv`].
//!
//! These helpers apply Fairing theme roles, margins and grouping conventions.
//! Layout, scrolling, interaction and clipping use
//! [egui `Ui`](https://docs.rs/egui/0.37.0/egui/struct.Ui.html) and its containers.
//! A grouped settings layout can be arranged as follows:

//! ```text
//!   Connection            ← title: the screen's name, large
//!
//!   Wireless              ← section: a subheading above the card (optional)
//!  ╭───────────────────╮
//!  │ Wi-Fi          [O] │  ← the rows inside a group. No separators
//!  │ Bluetooth      [O] │
//!  ╰───────────────────╯
//!   Nearby devices…      ← note: an explanation below the card (optional)
//! ```
//!
//! # That shape, on a screen of your own
//!
//! [`page_screen`] creates a scrolling page for the body; [`section_card`] groups
//! a section heading with a card.
//!
//! ```no_run
//! # fn main() {
//! # let shell: &mut fairing::Shell = todo!();
//! # let mut purge = false;
//! use fairing::layout::{info_row, note, page_screen, section_card, switch_row, title};
//!
//! shell.add(
//!     page_screen("app.heater", move |ui, cx| {
//!         title(ui, cx, "Heater");
//!         section_card(ui, cx, "Hopper", |ui, cx| {
//!             info_row(ui, cx, "Resin A", "72 %");
//!             info_row(ui, cx, "Resin B", "40 %");
//!         });
//!         section_card(ui, cx, "Maintenance", |ui, cx| {
//!             switch_row(ui, cx, "Purge on stop", None, &mut purge, true);
//!         });
//!         note(ui, cx, "Purging takes about a minute.");
//!     })
//!     .title("Heater")
//!     .icon(fairing::IconRef::Builtin("thermometer")),
//! );
//! # }
//! ```
//!
//! Built-in settings bodies can also be used in application-owned pages through
//! `settings::screens` (feature `settings`) or registered under another ID with
//! [`ScreenDecl::with_id`](crate::screen::ScreenDecl::with_id).
//!
//! Colours use theme roles. Row height, padding and corners use resolved metrics;
//! custom content can supply additional geometry and drawing. Review the complete
//! screen after changing palettes, sizing policy or content.

mod containers;
pub use containers::{
    action_bar, action_bar_with, chip_row, grid, group, group_with, header, note, page,
    page_screen, rail_split, rail_split_with, rail_width, section, section_card, section_card_with,
    split_height, split_width, title,
};
use containers::{
    card_divider, chrome_band, columns_for, f32_from, paint_dots, paint_scrim, rail_grip,
};
mod rows;
use rows::reveal_body;
pub use rows::{
    accordion, advanced_rows, choice_rows, hero, icon_row, icon_row_with_long_press, info_row,
    list_item, list_item_with, nav_row, slider_row, status_card, switch_row, tab_bar, total_row,
};
mod fitting;
use fitting::rounded;
pub use fitting::{fit_size, fit_text, wifi_icon};

pub use fairing_widgets::fit::{contain, cover_uv};

mod transit;
pub use transit::{transit, Motion, Side, Transit, CARD_SCALE, SLIDE};

use crate::icons::IconRef;
use crate::screen::{screen, Cx, ScreenDecl};
use crate::theme::{card_radius, ColorRole, Elevation};
use crate::unit::{round_i8, round_u8};
use crate::widgets::{unroll, ColorSpec, ListRow, TouchSlider, Unrolled};
use egui::{Response, Ui};

/// Container presentation style.
///
/// `Filled` uses a surface fill and elevation. `Outlined` uses an unfilled boundary;
/// `Divided` uses a full-width closing rule. `Tinted` uses the accent role.
/// A filled card uses the alternate surface role when its panel already has the
/// normal card fill. Custom palettes still determine the resulting contrast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Container {
    /// A filled card. The default, and what every container in the crate was.
    #[default]
    Filled,
    /// A hairline edge and no fill.
    Outlined,
    /// No box: the rows run full width and a rule closes the group.
    Divided,
    /// Tint the group using the accent role.
    /// Check the result against the configured background and content colours.
    Tinted,
}

/// **Layout decoration** — injects colours, corners, gaps and padding.
///
/// Everything defaults to `None` and a **theme token** goes in its place — hand over nothing and the
/// screen reads as one set; only what is handed over is different. There is no need to rewrite a
/// whole layout to put in one brand-coloured tile.
///
/// ```
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout::{self, Deco};
/// use fairing::ColorRole;
///
/// // The highlighted category in the brand colour, with square corners.
/// layout::Grid::new(2.0, 2.4)
///     .deco(Deco::new().fill(ColorRole::Primary).radius(4.0))
///     .show(ui, cx, &[1, 2, 3], |_ui, _cx, _cell| {});
/// # }
/// ```
///
/// The colour is a [`ColorSpec`], so it takes **both a role and a fixed colour** — a role follows
/// the theme toggle and a fixed colour does not.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Deco {
    kind: Container,
    fill: Option<ColorSpec>,
    stroke: Option<(f32, ColorSpec)>,
    radius: Option<f32>,
    gap: Option<f32>,
    padding: Option<f32>,
    pad_x: Option<f32>,
    /// Inset free content by `content_inset` (see [`Deco::pad_content`]).
    pad_content: bool,
    visual_inset: Option<f32>,
    elevation: Option<Elevation>,
}

impl Deco {
    /// A decoration that changes nothing — every value a theme token.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            kind: Container::Filled,
            fill: None,
            stroke: None,
            radius: None,
            gap: None,
            padding: None,
            pad_x: None,
            pad_content: false,
            visual_inset: None,
            elevation: None,
        }
    }

    /// Select a container presentation kind — see [`Container`].
    ///
    /// It sets the fill, the edge, the radius and the elevation together, and every explicit knob
    /// on this builder still wins over it, so `Deco::new().container(Container::Outlined).fill(..)`
    /// is an outlined box that happens to be filled rather than a contradiction.
    #[must_use]
    pub const fn container(mut self, kind: Container) -> Self {
        self.kind = kind;
        self
    }

    /// **Which container kind this is**, as set by [`Self::container`].
    #[must_use]
    pub const fn container_kind(&self) -> Container {
        self.kind
    }

    /// The background colour. [`ColorRole::SurfaceVariant`] by default.
    #[must_use]
    pub fn fill(mut self, color: impl Into<ColorSpec>) -> Self {
        self.fill = Some(color.into());
        self
    }

    /// Do not draw a background at all — for when the callback draws its own.
    #[must_use]
    pub fn no_fill(mut self) -> Self {
        self.fill = Some(ColorSpec::Fixed(egui::Color32::TRANSPARENT));
        self
    }

    /// The border (a width in du, and a colour).
    #[must_use]
    pub fn stroke(mut self, width_du: f32, color: impl Into<ColorSpec>) -> Self {
        self.stroke = Some((width_du.max(0.0), color.into()));
        self
    }

    /// The corner radius (du). [`fairing::theme::card_radius`](crate::theme::card_radius) by
    /// default — `corner_radius × control.card_radius_ratio`.
    #[must_use]
    pub fn radius(mut self, du: f32) -> Self {
        self.radius = Some(du.max(0.0));
        self
    }

    /// The gap between items (du). `metrics.screen_inset` by default.
    #[must_use]
    pub fn gap(mut self, du: f32) -> Self {
        self.gap = Some(du.max(0.0));
        self
    }

    /// The inner padding **above and below** (du). By default `row_height × 0.10` for a card and 0
    /// elsewhere. For the sides, [`Deco::pad_x`].
    #[must_use]
    pub fn padding(mut self, du: f32) -> Self {
        self.padding = Some(du.max(0.0));
        self
    }

    /// Horizontal inner padding in du, default zero.
    /// Rows already inset their content; free content may need [`Deco::pad_content`] or
    /// an explicit padding value to align with neighboring rows.
    #[must_use]
    pub fn pad_x(mut self, du: f32) -> Self {
        self.pad_x = Some(du.max(0.0));
        self
    }

    /// Inset the card's **free content** — buttons, bars, meters, anything that is not a row —
    /// by `metrics.content_inset` on both sides.
    ///
    /// Rows bring their own inset and must not have this. Free content has none, and set
    /// against the card's edge it collides with the card's rounded corner: a bordered button an
    /// eye-token's inset short of the corner had its own corner cut by the card's curve.
    /// The same as [`pad_x`](Self::pad_x) with the token, without the number — and a
    /// wrapped row inside it wraps within the inset, so its second line lines up with its
    /// first. An explicit `pad_x` wins.
    #[must_use]
    pub const fn pad_content(mut self) -> Self {
        self.pad_content = true;
        self
    }

    /// **Separate the logical size from the visual size** (du). The hit area is left as it is and
    /// **only what is seen** shrinks by this much.
    ///
    /// A device UI often needs a large touch target (13 mm for a gloved hand) while wanting the
    /// screen to look dense. Make the cell small and it cannot be pressed; leave it large and it
    /// looks bare — this value is what pulls the two apart.
    ///
    /// [`Cell::rect`] is the logical (hit) one as it stands and [`Cell::visual`] is the shrunken one.
    /// The background and the border are drawn to `visual`.
    ///
    /// **All three helpers take it.** [`Grid`] shrinks only the cell's drawing and leaves the hit
    /// cell as it is, while [`group_with`] and [`action_bar_with`] leave the space taken as it is and
    /// **inset only the card or the bar**, making it look as though it floats. All three mean the
    /// same thing: the place stays, the drawing shrinks.
    ///
    /// ```no_run
    /// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
    /// # let items = [1, 2, 3];
    /// use fairing::layout::{Deco, Grid};
    ///
    /// // The finger presses a large cell and the eye sees a small tile.
    /// Grid::new(2.0, 2.0)
    ///     .deco(Deco::new().visual_inset(10.0))
    ///     .show(ui, cx, &items, |ui, cx, cell| {
    ///         let _ = (ui, cx, cell.visual, cell.rect);
    ///     });
    /// # }
    /// ```
    #[must_use]
    pub fn visual_inset(mut self, du: f32) -> Self {
        self.visual_inset = Some(du.max(0.0));
        self
    }

    /// The visual shrinkage (du). Not given, 0 — the place to draw is the place taken.
    fn visual_inset_of(self) -> f32 {
        self.visual_inset.unwrap_or(0.0)
    }

    /// Container elevation. Cards default to the theme's card elevation;
    /// [`action_bar_with`] uses `Elevation::Floating` by default.
    /// Use `Elevation::None` for this container or provide
    /// [`ElevationSpec::flat`](fairing_widgets::theme::ElevationSpec::flat) through
    /// [`ShellBuilder::elevation_spec`](crate::shell::ShellBuilder::elevation_spec)
    /// for a flat elevation configuration.
    #[must_use]
    pub fn elevation(mut self, level: Elevation) -> Self {
        self.elevation = Some(level);
        self
    }

    /// The level, or `fallback` where this `Deco` says nothing.
    fn elevation_of(self, fallback: Elevation) -> Elevation {
        self.elevation.unwrap_or(match self.kind {
            Container::Filled => fallback,
            // **Nothing that is not filled is raised.** Height is expressed on a dark palette as a
            // rim inside the silhouette (`ElevationStyle::Rim`), and a rim around a
            // container with no fill is just a box — which is the one thing `Outlined` already
            // draws on purpose and `Divided` must not draw at all. Measured on the settings
            // screens: with the rim left on, every "divider-only" group came out boxed.
            //
            // **`Tinted` is flat for a different reason**, and it is worth writing down because it
            // does have a fill: the tint is already the whole separation. A `Filled` box is the
            // same colour as the page it sits on and needs height to be a box at all; a washed one
            // is visibly not the page, and a shadow under it then says the same thing twice — on a
            // screen where nothing else is raised, loudly. The one on the console's groups was the
            // only shadow on the page.
            Container::Outlined | Container::Divided | Container::Tinted => Elevation::None,
        })
    }

    /// Resolve the background colour against the theme — and, for a `Filled` card, against the
    /// panel it is on: `ui`'s `panel_fill` is what the panel was painted (a [`Rail`] sets it for
    /// its page), and a card the panel's own colour takes the other surface step instead.
    fn fill_of(self, ui: &Ui, cx: &Cx<'_>) -> egui::Color32 {
        self.fill.map_or_else(
            || match self.kind {
                Container::Filled => {
                    let variant = cx.theme.color(ColorRole::SurfaceVariant);
                    if ui.visuals().panel_fill == variant {
                        cx.theme.color(ColorRole::Surface)
                    } else {
                        variant
                    }
                }
                Container::Tinted => cx
                    .theme
                    .color(ColorRole::Primary)
                    .gamma_multiply(cx.theme.control.fill_alpha),
                Container::Outlined | Container::Divided => egui::Color32::TRANSPARENT,
            },
            |c| c.resolve(cx.theme),
        )
    }

    /// The edge, where the kind draws one and the caller did not say otherwise.
    fn stroke_of(self, cx: &Cx<'_>) -> Option<(f32, ColorSpec)> {
        self.stroke.or(match self.kind {
            Container::Outlined => Some((
                cx.theme.control.stroke_hairline,
                ColorSpec::Role(ColorRole::Outline),
            )),
            Container::Filled | Container::Divided | Container::Tinted => None,
        })
    }

    /// The corner radius (px).
    fn radius_of(self, cx: &Cx<'_>) -> u8 {
        round_u8(self.radius.unwrap_or_else(|| match self.kind {
            Container::Filled | Container::Tinted => {
                card_radius(&cx.theme.metrics, &cx.theme.control)
            }
            // A hairline carries a big radius badly — see `Container`.
            Container::Outlined => cx.theme.metrics.corner_radius,
            Container::Divided => 0.0,
        }))
    }

    /// The gap (px).
    fn gap_of(self, cx: &Cx<'_>) -> f32 {
        self.gap.unwrap_or(cx.theme.metrics.screen_inset)
    }

    /// Draw the border (where there is one).
    fn paint_stroke(self, ui: &Ui, cx: &Cx<'_>, rect: egui::Rect) {
        if self.kind == Container::Divided {
            // The rule closes the group rather than boxing it, so it is drawn at the foot and runs
            // the full width — see `Container`.
            let y = rect.max.y + self.gap_of(cx) * 0.5;
            ui.painter().hline(
                rect.min.x..=rect.max.x,
                y,
                egui::Stroke::new(
                    cx.theme.control.stroke_hairline,
                    cx.theme.color(ColorRole::Outline),
                ),
            );
        }
        let Some((width, color)) = self.stroke_of(cx) else {
            return;
        };
        if width <= 0.0 {
            return;
        }
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(self.radius_of(cx)),
            egui::Stroke::new(width, color.resolve(cx.theme)),
            egui::StrokeKind::Inside,
        );
    }
}

/// The vertical gap between cards (relative to the row height).
pub(crate) const CARD_GAP: f32 = 0.38;
/// A total row's height, relative to an ordinary row's. It carries `type_scale.heading`
/// where a row carries `body`, and 1.375 of the text wants more than 1.0 of the row.
const TOTAL_ROW: f32 = 1.3;
/// The active tab's mark, as a fraction of its cell's width.
const TAB_MARK: f32 = 0.34;
/// A hero band's aspect where the caller gives none — 16:9, what both reference banners are.
const HERO_ASPECT: f32 = 16.0 / 9.0;
/// How much of a hero band's height the scrim ramps over.
const HERO_SCRIM: f32 = 0.55;
/// How far the explanation below a card sits from it (relative to the row height).
const NOTE_GAP: f32 = 0.10;
/// **The longest line an explanation runs to**, in ems of its own text size. Prose is read
/// comfortably at 45 to 75 characters a line, and an average character is about half an em, so
/// 36 ems is some 70 characters. Without it a note on a wide pane ran the full width — 150
/// characters a line on a 1024 px panel — and the eye lost its place going back for the next line.
const NOTE_MEASURE: f32 = 36.0;
// **Four text sizes used to live here as fractions of `row_height`** — section 0.22, title 0.44,
// note 0.23, slider 0.29 — and they are gone because a fraction of a row is not a type size. The
// row was the only handle they had when the type scale could not be reached, and it made the
// kiosk example's `+24 mm` of what its own comment calls "pure visual padding" into a **2.49x text
// multiplier**: measured, "In your cart" rendered at 35.2 du against a `small` token of 32.4.
// They read `theme.metrics.type_scale` now — the same five sizes every other text in the crate
// uses — and at the default the largest move is 12 %.
/// The subheading's letter-spacing, as a fraction of its own text size (0.06). Small text set in a
/// muted colour reads as an afterthought; tracking is what makes it read as a label instead.
const SECTION_TRACKING: f32 = 0.06;
/// The gap between a section heading and the card right below it (relative to the row height). The
/// heading **belongs to the card below** — it has to be narrower than the gap to the card above
/// (`CARD_GAP`) for the eye to tell which one it hangs on: about a quarter of it, a hair more
/// than the label's own descender room, so the text does not sit on the card's edge.
const SECTION_GAP: f32 = 0.1;
/// The list column's width fraction (relative to the Pane's width).
const LIST_FRACTION: f32 = 0.36;
/// The list column's cap (du). Wider than this and there is only empty space behind the labels.
const LIST_MAX: f32 = 360.0;
/// The minimum Pane width for going to a left/right split (du). Narrower and both columns feel cramped.
const SPLIT_MIN_WIDTH: f32 = 680.0;
/// The minimum aspect ratio for going to a left/right split. A tall screen is better in one column however wide it is.
const SPLIT_MIN_ASPECT: f32 = 1.35;
/// The minimum height for dividing top and bottom (as a multiple of the row height). It takes two
/// lines of title above and three rows of buttons below for the division to mean anything — lower
/// than that and neither band can hold anything.
const BAND_MIN_ROWS: f32 = 12.0;
/// The lower bound on the top band's share. Give it 0.1 and it is not a band but padding.
const BAND_TOP_MIN: f32 = 0.15;
/// The upper bound on the top band's share. Give it 0.7 and the control band is pushed below where the hand reaches.
const BAND_TOP_MAX: f32 = 0.60;
/// Minimum track width in du for a one-line slider row.
/// Narrower layouts place the track below the title.
const SLIDER_INLINE_TRACK: f32 = 240.0;
/// The share of the width the title takes on a one-line layout (relative to the width left).
const SLIDER_LABEL_FRACTION: f32 = 0.26;
/// The value column's width (relative to the row height). Enough that `100%` and `-20.0°C` are not cut off.
const SLIDER_VALUE_WIDTH: f32 = 1.15;
/// The slider row's padding above and below (relative to the row height). The track's hit height is
/// `touch_target`, so this value shrinks **only the padding**.
const SLIDER_PAD: f32 = 0.08;
/// The gap between the columns of a one-line layout (relative to the row height).
const SLIDER_COL_GAP: f32 = 0.20;
/// The title column's cap (relative to the row height). Wider than this leaves open ground behind a short title.
const SLIDER_LABEL_MAX: f32 = 2.4;
/// The share of the height left over that goes **above** when [`Grid::fill_height`] hits its cap. 0.5 is dead centre.
const FILL_LEAD: f32 = 0.5;

/// **The rail's share of the width when the chrome is an L** — narrower than a settings split's,
/// because this column holds icons and one word each rather than a list of settings rows.
const RAIL_FRACTION: f32 = 0.18;

/// The cap on that, so a very wide console does not give a six-entry rail a third of the glass.
const RAIL_MAX: f32 = 280.0;

/// The floor, so the arm can still hold an icon and its label side by side.
const RAIL_MIN: f32 = 132.0;

/// The collapsed arm as a multiple of `touch_target`: wide enough for an icon and its disc,
/// and nothing else, at whatever finger the disc is sized for.
const RAIL_COLLAPSED: f32 = 1.12;

/// Where [`Rail::show`] leaves a [`FoldFrame`] while its rail closure runs, so [`list_item`] can
/// draw the fold without being told about it.
const FOLD_KEY: &str = "fairing.layout.rail.fold";

/// What [`Rail::show`] leaves under [`FOLD_KEY`].
#[derive(Debug, Clone, Copy, Default)]
struct FoldFrame {
    /// How far through the fold this frame is: `0` open … `1` folded.
    k: f32,
    /// The chrome band's left and right edges — from the glass to the arm's edge. A folded icon is
    /// centred in this and not in its row: the row starts a screen inset in from the glass, the
    /// band is what the eye reads, and the two centres are six du apart. `None` where there is no
    /// rail to say — a row that is simply narrow — and the row itself is the band.
    band: Option<(f32, f32)>,
}

/// **How far a fold goes** — [`Rail::fold_to`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Fold {
    /// To a column of icons: the words go, the glyphs slide to the middle of what is left, and
    /// the bar beside the live one melts into a disc behind it. The default.
    #[default]
    Icons,
    /// Away entirely: the page takes the whole screen. A drag from the left edge brings the arm
    /// back, as does a flick with [`FoldGesture::Anywhere`], and so does [`Rail::set_folded`]
    /// from a button the page draws itself.
    Away,
}

/// Where the rail's fold gesture is recognized.
/// `Anywhere` is the default; `Arm` limits it to the rail arm. Page drags claimed by
/// controls or kept by custom pointer regions are excluded from folding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FoldGesture {
    /// A drag across the arm, and nowhere else.
    Arm,
    /// That, and the same drag anywhere on the page: past the same couple of dozen du with the
    /// same horizontal intent, at once, wherever the finger came down. A drag that a control
    /// owns — a slider, a switch, a text field, anything that follows the finger and says so with
    /// [`crate::drag::claim`] — is that control's and never folds; a button, a tile or a page
    /// scrolling under the finger makes no claim, and a swipe that began on one folds the rail.
    /// A region that reads the pointer itself — a canvas, a chart, a control of the integrator's
    /// own — keeps every drag that begins in it with [`crate::drag::keep`]; that takes nothing
    /// from the region, whose own pans, pinches and scrolls arrive as before, and only tells the
    /// rail to stay out. Presses in the shell's edge zones are left to the shell's own edge
    /// gestures. The default.
    #[default]
    Anywhere,
}

/// **What [`Rail::show`] hands back.**
pub struct RailPick<A, B> {
    /// What the rail closure returned, or `None` where the arm was not drawn at all: a panel too
    /// narrow to carry one, or a [`Fold::Away`] rail that is folded.
    pub picked: Option<A>,
    /// What the page closure returned.
    pub page: B,
    /// Whether the arm is folded, or [hidden](Rail::hidden) by the app. A screen laying a header
    /// out beside the rail wants this; a screen that only draws rows does not, because
    /// [`list_item`] reads the fold itself.
    pub collapsed: bool,
}

/// **A screen whose navigation rail is part of the chrome**, as a builder — [`rail_split`] is this
/// with everything left at its default.
///
/// See [`rail_split`] for the shape and why it is one call rather than two.
#[derive(Debug, Clone, Copy)]
pub struct Rail {
    deco: Deco,
    collapsible: bool,
    fold: Fold,
    gesture: FoldGesture,
    hidden: bool,
    salt: &'static str,
}

impl Default for Rail {
    fn default() -> Self {
        Self::new()
    }
}

impl Rail {
    /// A rail with the crate's defaults: the page panel in `SurfaceVariant`, and no folding.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            deco: Deco::new(),
            collapsible: false,
            fold: Fold::Icons,
            gesture: FoldGesture::Anywhere,
            hidden: false,
            salt: "fairing.layout.rail",
        }
    }

    /// Colours and corners for the **page panel**. The chrome is the bar's own colour by
    /// definition — see [`rail_split_with`].
    #[must_use]
    pub const fn deco(mut self, deco: Deco) -> Self {
        self.deco = deco;
        self
    }

    /// Enable swipe folding, off by default.
    ///
    /// When enabled, recognition covers the arm and page unless `fold_gesture` narrows
    /// it to the arm. `fold_to` chooses icons-only or fully-away folding. During the
    /// fold, labels fade, icons move and the active mark changes shape. `list_item`
    /// reads the rail's retained fold state. Application-controlled `hidden` removes
    /// the rail independently of gesture folding.
    #[must_use]
    pub const fn collapsible(mut self, collapsible: bool) -> Self {
        self.collapsible = collapsible;
        self
    }

    /// How far [`Self::collapsible`] folds: to icons, or away entirely.
    #[must_use]
    pub const fn fold_to(mut self, fold: Fold) -> Self {
        self.fold = fold;
        self
    }

    /// Where the fold gesture is read: anywhere on the page too (the default), or on the arm
    /// only. Takes effect with [`Self::collapsible`].
    #[must_use]
    pub const fn fold_gesture(mut self, gesture: FoldGesture) -> Self {
        self.gesture = gesture;
        self
    }

    /// **Put away by the app, for as long as it says**. A home screen that is a wall
    /// of tiles, a run screen that wants every millimetre for the reading: some screens have no
    /// use for the rail, and hiding it is not the same as folding it. The arm slides off with the
    /// fold's own motion and the page takes the whole width; the status bar stays, being the
    /// shell's and not the rail's. The hand cannot bring a hidden rail back — no grip strip, no
    /// swipe — so the condition is the app's alone. Pass it every frame; `false` restores the
    /// rail as it was, open or folded, and it slides back in.
    ///
    /// It is a builder flag rather than a second `show`, so one rail call serves every screen:
    /// `Rail::new().collapsible(true).hidden(state.page == Page::Home)`.
    #[must_use]
    pub const fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// Separate two rails on one screen. Only needed where there are two.
    #[must_use]
    pub const fn id_salt(mut self, salt: &'static str) -> Self {
        self.salt = salt;
        self
    }

    /// Whether this rail is folded — the bit the gesture writes.
    #[must_use]
    pub fn is_folded(self, ctx: &egui::Context) -> bool {
        ctx.data(|d| {
            d.get_temp::<bool>(egui::Id::unique(self.salt))
                .unwrap_or(false)
        })
    }

    /// **Fold or open the rail from code** — for a menu button the page draws, which a
    /// [`Fold::Away`] rail wants since there is nothing left on screen to press. The width still
    /// tweens from here exactly as it does from a gesture.
    pub fn set_folded(self, ctx: &egui::Context, folded: bool) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(self.salt), folded));
    }

    /// Draw it. See [`rail_split`] for what the two closures are and why the rail returns its pick.
    pub fn show<A, B>(
        self,
        ui: &mut Ui,
        cx: &mut Cx<'_>,
        rail: impl FnOnce(&mut Ui, &mut Cx<'_>) -> A,
        page: impl FnOnce(&mut Ui, &mut Cx<'_>) -> B,
    ) -> RailPick<A, B> {
        let full = ui.available_rect_before_wrap();
        let Some(open_w) = rail_width(cx) else {
            return RailPick {
                picked: None,
                page: page(ui, cx),
                collapsed: false,
            };
        };
        let id = egui::Id::unique(self.salt);
        let folded = self.collapsible && self.is_folded(ui.ctx());
        // **The width is animated, the state is a bool.** Folding is one bit; how far through the
        // fold this frame is, is a tween, and `Cx::animate` is where the crate keeps those so
        // `[motion] reduce` turns it off everywhere at once.
        let shut = match self.fold {
            Fold::Icons => cx.theme.metrics.touch_target * RAIL_COLLAPSED,
            Fold::Away => 0.0,
        };
        // Hidden by the app, the arm goes all the way off whatever the fold would have left, and
        // comes back to wherever the fold state says once the app lets go.
        let target = if self.hidden {
            0.0
        } else if folded {
            shut
        } else {
            open_w
        };
        let arm = cx.animate(id.with("w"), target, cx.theme.motion.push);
        // How far through the fold this frame is, whichever way it is going. The rows read it.
        let progress = ((open_w - arm) / (open_w - shut).max(1.0)).clamp(0.0, 1.0);

        let arm_rect =
            egui::Rect::from_min_max(full.min, egui::pos2(full.left() + arm, full.bottom()));
        // **The arm is the bar's colour, and nothing is drawn between them.** This is the whole of
        // the shape: `chrome::status_bar` fills with `Surface`, so does this, and the two meet with
        // no seam. Painting the arm only - rather than `full` - keeps the page's panel the one
        // thing that decides what is under the page.
        //
        // It is painted **out to the glass**: the screen's rect sits a `screen_inset` in from the
        // pane on every side, and a chrome stroke that stopped there would leave a strip of the
        // pane's backing between it and the edge. The rows keep the inset - only the paint, and
        // the band a folded icon is centred in, reach the edge.
        let band = chrome_band(cx, full, arm_rect);
        if arm >= 0.5 {
            ui.painter()
                .rect_filled(band, 0.0, cx.theme.color(ColorRole::Surface));
        }

        let page_rect =
            egui::Rect::from_min_max(egui::pos2(arm_rect.right(), full.top()), full.max);
        // Only the corner in the elbow is rounded. The other three are against the glass, where a
        // radius would show the chrome through the gap it opens (the same rule `action_bar` follows).
        // An arm thinner than the radius flattens it, so a rail folding away leaves the page square
        // against the glass on that side too rather than showing the chrome through the corner.
        let radius = self.deco.radius_of(cx).min(round_u8(arm));
        let page_fill = self.deco.fill_of(ui, cx);
        ui.painter().rect_filled(
            page_rect,
            egui::CornerRadius {
                nw: radius,
                ne: 0,
                sw: radius,
                se: 0,
            },
            page_fill,
        );

        // A hidden rail reads no gesture: not the swipe, and not the grip strip a folded-away
        // arm leaves — the app said, and the hand cannot say otherwise.
        if self.collapsible && !self.hidden {
            self.read_fold_gesture(ui, cx, full, band, id);
        }

        // An arm thinner than a du is not drawn at all - nor asked what it would have picked.
        let picked = (arm >= 1.0).then(|| {
            let mut rail_ui = ui.new_child(egui::UiBuilder::new().max_rect(arm_rect));
            // Clipped to the band, not the arm: a folded icon and its disc are centred in the
            // band, and on a panel whose glyphs are large the disc reaches past the screen inset
            // the arm starts at. Cut there, the disc lost its left edge (user report).
            rail_ui.set_clip_rect(band);
            let fold_id = egui::Id::unique(FOLD_KEY);
            let frame = FoldFrame {
                k: progress,
                band: Some((band.left(), band.right())),
            };
            ui.ctx().data_mut(|d| d.insert_temp(fold_id, frame));
            let picked = rail(&mut rail_ui, cx);
            ui.ctx().data_mut(|d| d.remove_temp::<FoldFrame>(fold_id));
            picked
        });

        let mut page_ui = ui.new_child(egui::UiBuilder::new().max_rect(page_rect));
        page_ui.set_clip_rect(page_rect);
        // What the page was painted, for the cards on it to contrast with (`Deco::fill_of`).
        page_ui.visuals_mut().panel_fill = page_fill;
        RailPick {
            picked,
            page: page(&mut page_ui, cx),
            collapsed: folded || self.hidden,
        }
    }

    /// Read the pointer to recognize rail folding without registering a full-arm
    /// interactive widget over the row controls.
    ///
    /// A horizontally dominant drag past the edge threshold folds or opens the rail.
    /// With `FoldGesture::Anywhere`, page presses in shell edge zones and drags claimed
    /// or kept by controls are excluded. The recognized ownership is retained through
    /// release, including the press point needed after egui drops its current press.
    fn read_fold_gesture(
        self,
        ui: &Ui,
        cx: &Cx<'_>,
        full: egui::Rect,
        band: egui::Rect,
        id: egui::Id,
    ) {
        let press_id = id.with("press");
        let owned_id = id.with("owned");
        // `interact_pos`, not `latest_pos`: a finger lifting off a touch screen is a `PointerGone`
        // on the release frame, which takes the latest position with it and leaves this one.
        // The press point is `drag::press_point`, which reads this frame's events as well as
        // `press_origin`: a flick quick enough to land whole in one frame of a slow panel is
        // pressed and released before `press_origin` is ever seen set (the reading is shared with
        // the gestures and the shade, which learned the same lesson).
        let (origin, now, down) = ui.input(|i| {
            (
                crate::drag::press_point(i),
                i.pointer.interact_pos(),
                i.pointer.any_down(),
            )
        });
        let ctx = ui.ctx();
        if let Some(origin) = origin {
            ctx.data_mut(|d| d.insert_temp(press_id, origin));
        }
        let remembered = ctx.data(|d| d.get_temp::<egui::Pos2>(press_id));
        let from = origin.or(remembered);
        // A control that follows the finger says so, frame by frame, and a region that reads the
        // pointer itself keeps every drag that began in it; the drag is left to them until the
        // finger lifts. Kept here because no claim is made on the release frame — and let go
        // when nothing is down rather than on `any_released`, which a touch that is cancelled
        // (a `PointerGone` with no release) never sends.
        if crate::drag::claimed(ctx) || from.is_some_and(|at| crate::drag::kept(ctx, at)) {
            ctx.data_mut(|d| d.insert_temp(owned_id, true));
        }
        let owned = ctx.data(|d| d.get_temp::<bool>(owned_id).unwrap_or(false));
        if !down {
            ctx.data_mut(|d| {
                d.remove_temp::<egui::Pos2>(press_id);
                d.remove_temp::<bool>(owned_id);
            });
        }
        let Some((from, to)) = from.zip(now) else {
            return;
        };
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let grip_w = rail_grip(cx);
        if dx.abs() <= grip_w || dx.abs() <= dy.abs() {
            return;
        }
        let grip = band.with_max_x(band.left() + band.width().max(grip_w));
        if grip.contains(from) {
            ctx.data_mut(|d| d.insert_temp(id, dx < 0.0));
            return;
        }
        if self.gesture != FoldGesture::Anywhere || owned {
            return;
        }
        let edge = cx.theme.metrics.edge_px;
        let on_page =
            full.contains(from) && from.x - full.left() >= edge && full.right() - from.x >= edge;
        if on_page {
            ctx.data_mut(|d| d.insert_temp(id, dx < 0.0));
        }
    }
}

/// One grid cell — handed over with **the logical and the visual separated**.
///
/// The point is that the callback receives both. The background and the border are drawn to
/// [`Self::visual`], but to draw beyond the cell (a badge outside the corner, a press that grows)
/// take [`Self::rect`] as the reference — **the grid does not clip a cell.** Within the screen's
/// Rect it can overflow freely; only remember that the next cell is drawn over it, so a drawing that
/// intrudes on a neighbour has to think about the order.
#[derive(Debug, Clone, Copy)]
pub struct Cell<'a, T> {
    /// **The logical Rect** — the hit area, and the place the layout took. Unaffected by `Deco::visual_inset`.
    pub rect: egui::Rect,
    /// **The visual Rect** — the place to draw, shrunk by `Deco::visual_inset`. The same as `rect` by default.
    pub visual: egui::Rect,
    /// This cell's response (taps and presses).
    pub response: &'a Response,
    /// The item this cell holds.
    pub item: &'a T,
}

/// The per-item background colour callback ([`Grid::fill_with`]).
type FillFn<'a, T> = dyn Fn(&T) -> ColorSpec + 'a;

/// **A grid** — the column count comes from a minimum cell width, and the items flow into tiles.
///
/// Have the integrator set the column count and it has to be set again every time the panel changes.
/// Here they say only **how big a cell has to be** and the width decides the columns — 2 columns on
/// a 760 px portrait becomes 4 on a 1600 px landscape.
///
/// Colours, corners and gaps are injected through [`Deco`]. Where a different colour per item is
/// needed it is [`Grid::fill_with`], and to draw the background yourself, `Deco::new().no_fill()`.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// # let items = ["a", "b", "c"];
/// use fairing::layout::{Deco, Grid};
/// use fairing::ColorRole;
///
/// Grid::new(3.0, 2.4)
///     .deco(Deco::new().radius(20.0).stroke(1.0, ColorRole::Outline))
///     .fill_with(|item: &&str| {
///         // Sold out is dimmed — the per-item colour is decided here.
///         if *item == "b" { ColorRole::Muted.into() } else { ColorRole::SurfaceVariant.into() }
///     })
///     .show(ui, cx, &items, |ui, cx, cell| {
///         if cell.response.clicked() { /* …add to the basket… */ }
///         ui.painter().text(
///             cell.visual.center(),
///             egui::Align2::CENTER_CENTER,
///             *cell.item,
///             egui::FontId::proportional(cell.visual.height() * 0.2),
///             cx.theme.color(fairing::ColorRole::OnSurface),
///         );
///     });
/// # }
/// ```
pub struct Grid<'a, T> {
    min_cell_rows: f32,
    cell_rows: f32,
    max_cols: Option<usize>,
    fill_aspect: Option<f32>,
    cell_height: Option<Box<dyn Fn(f32) -> f32 + 'a>>,
    deco: Deco,
    fill_with: Option<Box<FillFn<'a, T>>>,
}

impl<T> std::fmt::Debug for Grid<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grid")
            .field("min_cell_rows", &self.min_cell_rows)
            .field("cell_rows", &self.cell_rows)
            .field("max_cols", &self.max_cols)
            .field("fill_aspect", &self.fill_aspect)
            .field("cell_height", &self.cell_height.is_some())
            .field("deco", &self.deco)
            .field("fill_with", &self.fill_with.is_some())
            .finish()
    }
}

impl<'a, T> Grid<'a, T> {
    /// Set cell height from its realized width in du.
    /// The callback returns height in du and replaces `cell_rows` as the lower bound;
    /// `fill_aspect` still supplies the upper ratio cap.
    ///
    /// ```no_run
    /// # fn f(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>, items: &[u8]) {
    /// # use fairing::{layout, widgets::ASPECT_SQUARE};
    /// // The text block is the same on every tile, so it is measured once and closed over.
    /// let text_h = cx.theme.metrics.row_height;
    /// layout::Grid::new(2.2, 2.1)
    ///     .cell_height(move |w| w / ASPECT_SQUARE + text_h)
    ///     .show(ui, cx, items, |ui, cx, cell| { let _ = (ui, cx, cell); });
    /// # }
    /// ```
    #[must_use]
    pub fn cell_height(mut self, height_for: impl Fn(f32) -> f32 + 'a) -> Self {
        self.cell_height = Some(Box::new(height_for));
        self
    }

    /// Give the minimum cell width and the cell height as multiples of `metrics.row_height` — leave
    /// them in absolute du and they do not follow the glove policy.
    #[must_use]
    pub const fn new(min_cell_rows: f32, cell_rows: f32) -> Self {
        Self {
            min_cell_rows,
            cell_rows,
            max_cols: None,
            fill_aspect: None,
            cell_height: None,
            deco: Deco::new(),
            fill_with: None,
        }
    }

    /// Set an upper bound on column count. The default has no explicit cap.
    ///
    /// This does not clamp to item count: `max_columns(6)` with three items retains
    /// six-column widths. Use `max_columns(items.len())` to cap by the current count.
    /// Zero is ignored.
    ///
    /// ```no_run
    /// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
    /// # let methods = ["Card", "Mobile pay", "Cash"];
    /// use fairing::layout::Grid;
    ///
    /// // Only three, so three to a row however wide it gets — no empty fourth cell appears.
    /// Grid::new(2.0, 3.0)
    ///     .max_columns(3)
    ///     .show(ui, cx, &methods, |_ui, _cx, _cell| {});
    /// # }
    /// ```
    #[must_use]
    pub const fn max_columns(mut self, cols: usize) -> Self {
        self.max_cols = if cols == 0 { None } else { Some(cols) };
        self
    }

    /// Expand cell height into available vertical space, with an aspect-ratio cap.
    ///
    /// `cell_rows`, or the `cell_height` callback when supplied, sets the lower bound.
    /// The ratio caps height relative to cell width only when that cap is at least
    /// the lower bound; otherwise the lower bound wins. Remaining vertical space is
    /// split according to the alignment token. Values at or below zero disable this
    /// option. The same cap/floor rules apply to unbounded available height.
    ///
    /// ```no_run
    /// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
    /// # let methods = ["Card", "Mobile pay", "Cash"];
    /// use fairing::layout::Grid;
    ///
    /// // A lower bound of 3 rows, an upper of 1.1 × the width. In a tall band it grows that far and stops.
    /// Grid::new(2.0, 3.0)
    ///     .max_columns(3)
    ///     .fill_height(1.1)
    ///     .show(ui, cx, &methods, |_ui, _cx, _cell| {});
    /// # }
    /// ```
    #[must_use]
    pub fn fill_height(mut self, max_aspect: f32) -> Self {
        self.fill_aspect = (max_aspect > 0.0).then_some(max_aspect);
        self
    }

    /// Colours, corners and gaps.
    #[must_use]
    pub const fn deco(mut self, deco: Deco) -> Self {
        self.deco = deco;
        self
    }

    /// A different background colour per item. Stronger than [`Deco::fill`].
    #[must_use]
    pub fn fill_with(mut self, f: impl Fn(&T) -> ColorSpec + 'a) -> Self {
        self.fill_with = Some(Box::new(f));
        self
    }

    /// Draw.
    pub fn show(
        self,
        ui: &mut Ui,
        cx: &mut Cx<'_>,
        items: &[T],
        mut cell: impl FnMut(&mut Ui, &mut Cx<'_>, Cell<'_, T>),
    ) {
        if items.is_empty() {
            return;
        }
        let m = &cx.theme.metrics;
        let gap = self.deco.gap_of(cx);
        let radius = self.deco.radius_of(cx);
        let min_w = (m.row_height * self.min_cell_rows.max(1.0)).max(m.touch_target);
        let avail = ui.available_width();
        // The fill has to be measured **before drawing** — once the first row is drawn the height left has shrunk.
        let avail_h = ui.available_height();
        // Where there is a cap it stops there. **It does not clamp to the item count** — on a screen items
        // flow into, 1 column at 1 item becoming 6 at 12 would change the tile width midway. Where that is
        // wanted, write `max_columns(items.len())`.
        let cols = columns_for(avail, min_w, gap)
            .min(self.max_cols.unwrap_or(usize::MAX))
            .max(1);
        let cols_f = f32_from(cols);
        let tile_w = ((avail - gap * (cols_f + 1.0)) / cols_f).max(min_w * 0.5);
        // **After `tile_w`, not before it** — the whole point of `cell_height` is that the answer
        // depends on the width the columns actually came out at.
        let min_h = self.cell_height.as_ref().map_or_else(
            || m.row_height * self.cell_rows.max(1.0),
            |height_for| height_for(tile_w).max(m.touch_target),
        );
        // What one row takes up is `the cell height + gap` (the loop below appends a `gap` after each row).
        // So the fill divides with that share taken out too.
        let rows_f = f32_from(items.len().div_ceil(cols));
        let cell_h = self.fill_aspect.map_or(min_h, |aspect| {
            // The cap can be below the floor — it is, in a very narrow column. `clamp` panics then, so
            // only a cap that beat the floor is used.
            let cap = (tile_w * aspect).max(min_h);
            (avail_h / rows_f - gap).clamp(min_h, cap)
        });
        // The height left over under the cap is divided above and below.
        let lead = (avail_h - rows_f * (cell_h + gap)) * FILL_LEAD;
        if self.fill_aspect.is_some() && lead > 0.0 {
            ui.add_space(lead);
        }
        let base = self.deco.fill_of(ui, cx);
        let pressed = cx.theme.color(ColorRole::Pressed);
        let inset = self.deco.visual_inset.unwrap_or(0.0);

        for row in items.chunks(cols) {
            ui.horizontal(|ui| {
                // **egui's default item spacing is turned off.** The `tile_w` formula sets aside only
                // `gap × (cols + 1)`, but `horizontal` also inserts `item_spacing.x` (10 du by default)
                // between the `add_space` and each tile — at 2 columns that adds 40 du and the right-hand
                // column is cut off the screen. Caught by actually writing the kiosk menu.
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(gap);
                for item in row {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(tile_w, cell_h), egui::Sense::click());
                    // **The logical (hit) one is `rect`, and the visual may be smaller** — this is where
                    // the cell a gloved hand presses stays large and only the tile seen shrinks
                    // (`visual_inset`).
                    let visual = rect.shrink(inset);
                    // The crate lays the background down — it has to follow the same convention as a card
                    // for the screen to read as one set. What goes on top of it is the callback's.
                    let fill = if response.is_pointer_button_down_on() {
                        pressed
                    } else {
                        self.fill_with
                            .as_ref()
                            .map_or(base, |f| f(item).resolve(cx.theme))
                    };
                    if fill.a() > 0 {
                        ui.painter()
                            .rect_filled(visual, egui::CornerRadius::same(radius), fill);
                    }
                    self.deco.paint_stroke(ui, cx, visual);
                    cell(
                        ui,
                        cx,
                        Cell {
                            rect,
                            visual,
                            response: &response,
                            item,
                        },
                    );
                    ui.add_space(gap);
                }
            });
            // The same between rows — one `horizontal` is one item, so a vertical gap is added as well.
            ui.add_space(gap - ui.spacing().item_spacing.y);
        }
    }
}

/// One cell of a [`tab_bar`].
///
/// Public fields, so `Tab { label: "Orders", icon: icon::LIST }` compiles outside the crate.
#[derive(Debug, Clone)]
pub struct Tab<'a> {
    /// The word under the glyph.
    pub label: &'a str,
    /// The glyph.
    pub icon: IconRef,
}

/// Expandable row with an application-owned open state.
///
/// Tapping the header toggles the revealed body. An optional header switch handles
/// its own hit region without opening the row. The summary is shown while closed.
/// The body is indented to align with header text, and following content moves as
/// its height animates. `reveal(false)` disables the default scroll-to-body behavior.
/// Use [`accordion`] to coordinate one-open-at-a-time state across several rows.
///
/// ```no_run
/// # fn body(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>, open: &mut bool, night: &mut bool) {
/// use fairing::{icon, layout::{self, ExpandableRow}};
/// layout::group(ui, cx, |ui, cx| {
///     ExpandableRow::new("display", "Display")
///         .icon(icon::DISPLAY)
///         .subtitle("Resolution, scale")
///         .summary("1920 × 1080")
///         .show(ui, cx, open, |ui, cx| {
///             layout::info_row(ui, cx, "Scale", "150 %");
///             layout::switch_row(ui, cx, "Night light", None, night, true);
///         });
/// });
/// # }
/// ```
pub struct ExpandableRow<'a> {
    id_salt: &'a str,
    title: &'a str,
    subtitle: Option<&'a str>,
    icon: Option<IconRef>,
    summary: Option<&'a str>,
    switch: Option<&'a mut bool>,
    enabled: bool,
    reveal: bool,
}

impl std::fmt::Debug for ExpandableRow<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExpandableRow")
            .field("id_salt", &self.id_salt)
            .field("title", &self.title)
            .field("subtitle", &self.subtitle)
            .field("summary", &self.summary)
            .field("switch", &self.switch.as_deref())
            .field("enabled", &self.enabled)
            .field("reveal", &self.reveal)
            .finish_non_exhaustive()
    }
}

/// What an [`ExpandableRow`] did this frame.
#[derive(Debug)]
pub struct Expanded {
    /// The header row's response.
    pub header: Response,
    /// The row opened or closed on this frame.
    pub toggled: bool,
    /// The header's switch was toggled on this frame.
    pub switched: bool,
    /// The band the body took this frame, while there is one — its eased height, so it is
    /// short while the row is opening and closing.
    pub body: Option<egui::Rect>,
}

impl<'a> ExpandableRow<'a> {
    /// The header's title. The salt names the row's animation and its memory.
    #[must_use]
    pub const fn new(id_salt: &'a str, title: &'a str) -> Self {
        Self {
            id_salt,
            title,
            subtitle: None,
            icon: None,
            summary: None,
            switch: None,
            enabled: true,
            reveal: true,
        }
    }

    /// A second line under the title.
    #[must_use]
    pub const fn subtitle(mut self, subtitle: &'a str) -> Self {
        self.subtitle = Some(subtitle);
        self
    }

    /// A leading icon.
    #[must_use]
    pub fn icon(mut self, icon: IconRef) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A value at the right **while the row is closed** — what is inside, in a word.
    #[must_use]
    pub const fn summary(mut self, summary: &'a str) -> Self {
        self.summary = Some(summary);
        self
    }

    /// A switch in the header that a tap on it toggles, without opening the row.
    #[must_use]
    pub fn switch(mut self, on: &'a mut bool) -> Self {
        self.switch = Some(on);
        self
    }

    /// Enabled. Disabled, the header takes no tap and the body stays as it is.
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Scroll the page so the opening body shows (the default).
    #[must_use]
    pub const fn reveal(mut self, reveal: bool) -> Self {
        self.reveal = reveal;
        self
    }

    /// Draw the header and, while `open` (or still closing), the body under it.
    pub fn show(
        self,
        ui: &mut Ui,
        cx: &mut Cx<'_>,
        open: &mut bool,
        body: impl FnOnce(&mut Ui, &mut Cx<'_>),
    ) -> Expanded {
        card_divider(ui, cx);
        let id = ui.scope_id().with(self.id_salt);
        let mut row = ListRow::new(self.title)
            .disclosure(*open)
            .separator(false)
            .enabled(self.enabled);
        if let Some(subtitle) = self.subtitle {
            row = row.subtitle(subtitle);
        }
        if let Some(icon) = self.icon {
            row = row.icon(icon);
        }
        if let (Some(summary), false) = (self.summary, *open) {
            row = row.trailing(summary);
        }
        if let Some(on) = self.switch.as_deref() {
            row = row.trailing_switch(*on);
        }
        let press = row.show_with_long_press(ui, &mut cx.widgets());
        let mut toggled = false;
        let mut switched = false;
        if press.tapped {
            let gap = cx.theme.control.gap;
            let on_switch = press
                .switch
                .zip(press.response.interact_pointer_pos())
                .is_some_and(|(slot, p)| {
                    let reach = (press.response.rect.height() - slot.height()).max(0.0) * 0.5;
                    slot.expand2(egui::vec2(gap, reach)).contains(p)
                });
            if let (true, Some(on)) = (on_switch, self.switch) {
                *on = !*on;
                switched = true;
            } else {
                *open = !*open;
                toggled = true;
            }
        }
        if toggled && *open && self.reveal {
            ui.data_mut(|d| d.insert_temp(id.with("reveal"), true));
        }

        let indent = cx.theme.control.icon + cx.theme.components.list_row.pad;
        let unrolled = unroll(ui, &mut cx.widgets(), id, *open, indent)
            .map(|band| band.show(ui, |ui| body(ui, cx)));
        if let Some(out) = unrolled {
            if out.rect.height() > 1.0 {
                // The line between the header and its body — the body's first row draws none
                // at the top of its own band.
                let pad = cx.theme.components.list_row.pad;
                ui.painter().hline(
                    out.rect.min.x + pad..=out.rect.max.x,
                    out.rect.min.y,
                    egui::Stroke::new(
                        cx.theme.control.stroke_hairline,
                        cx.theme.color(ColorRole::Outline),
                    ),
                );
            }
            reveal_body(ui, id, press.response.rect, &out);
        }
        Expanded {
            header: press.response,
            toggled,
            switched,
            body: unrolled.map(|out| out.rect),
        }
    }
}

/// How much of the selected rail entry's height its accent bar takes.
///
/// Measured off the console reference: a 21 du bar against a 29 du pill. Short enough to read as a
/// mark beside the pill rather than as a second edge on it, long enough that the eye catches it
/// down a column of six. It is a share and not a token because it is a proportion *of the pill*,
/// and a token would be one more length free to drift away from the row it marks — which is how
/// an earlier version of this bar shrank to nothing.
const BAR_SHARE: f32 = 0.72;

/// The bar's gutter, as a multiple of the bar's own width: the bar, then a gap the same again
/// before the pill starts.
const BAR_GUTTER: f32 = 2.0;

/// Selection-mark style for a rail or list entry.
/// `PillAndBar` is the default. Choose a style after checking the intended palette,
/// background, display size and viewing conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SelectMark {
    /// A translucent accent pill behind the row with a bar down its leading edge. The default.
    #[default]
    PillAndBar,
    /// The pill alone. Quieter, and right where the rail is narrow enough that a bar crowds the
    /// icon.
    Pill,
    /// The bar alone. A dense list, where a fill behind one row of many reads as a stripe.
    Bar,
    /// Neither. The accent on the icon and the label is the whole mark — for a rail that is
    /// already a coloured panel, where a tint on a tint says nothing.
    Tint,
}

/// The disc a folded rail draws behind its live icon, as a multiple of the icon's drawn side.
const DISC_RATIO: f32 = 2.0;

/// The disc's alpha as a multiple of the open pill's `fill_alpha`: a step darker, not a plate.
const DISC_TINT: f32 = 1.5;

/// The stretch of a fold over which the words fade out (and, unfolding, back in).
const LABEL_FADE: f32 = 0.35;

/// Where the pill and the bar go for a row occupying `rect` — shared by the open marks and the
/// fold, so the fold starts from exactly the shapes the open row drew.
struct MarkGeometry {
    pill: egui::Rect,
    pill_radius: f32,
    bar: egui::Rect,
    bar_w: f32,
    inset: f32,
}

/// Minimum scale used by this fitting path; smaller available space overflows instead.
const FIT_MIN_SIZE: f32 = 8.0;

#[cfg(test)]
mod tests {
    use super::{contain, cover_uv};
    use egui::{pos2, vec2, Rect};

    fn box_(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), vec2(w, h))
    }

    /// Whichever axis overflows, it stays inside `into` and keeps the aspect ratio.
    #[test]
    fn contain_never_overflows_and_keeps_the_aspect() {
        for (sw, sh) in [(200.0, 100.0), (100.0, 200.0), (50.0, 50.0), (3.0, 7.0)] {
            let into = box_(120.0, 90.0);
            let at = contain(vec2(sw, sh), into);
            assert!(
                at.width() <= into.width() + 0.01 && at.height() <= into.height() + 0.01,
                "{sw}×{sh} overflowed {into:?}: {at:?}"
            );
            assert!(
                (at.width() / at.height() - sw / sh).abs() < 1e-3,
                "{sw}×{sh}'s aspect ratio was not kept: {at:?}"
            );
            assert!(
                (at.center() - into.center()).length() < 0.01,
                "it is not centred: {at:?}"
            );
            // One side has to fit exactly — room left on both means it could have grown further.
            let snug = (at.width() - into.width()).abs() < 0.01
                || (at.height() - into.height()).abs() < 0.01;
            assert!(snug, "{sw}×{sh} came out too small inside {into:?}: {at:?}");
        }
    }

    /// The UV stays inside the original (0…1) and **only the cropped axis** narrows.
    #[test]
    fn cover_uv_crops_only_the_overflowing_axis() {
        let square = box_(100.0, 100.0);
        // A wide original → the sides are cropped.
        let wide = cover_uv(vec2(200.0, 100.0), square);
        assert!((wide.width() - 0.5).abs() < 1e-4, "{wide:?}");
        assert!((wide.height() - 1.0).abs() < 1e-4, "{wide:?}");
        // A tall original → the top and bottom are cropped.
        let tall = cover_uv(vec2(100.0, 200.0), square);
        assert!((tall.width() - 1.0).abs() < 1e-4, "{tall:?}");
        assert!((tall.height() - 0.5).abs() < 1e-4, "{tall:?}");
        for uv in [wide, tall] {
            assert!(
                uv.min.x >= -1e-4 && uv.min.y >= -1e-4 && uv.max.x <= 1.0 + 1e-4,
                "the UV went outside the source: {uv:?}"
            );
            assert!(
                (uv.center() - pos2(0.5, 0.5)).length() < 1e-4,
                "it does not read the centre: {uv:?}"
            );
        }
    }

    /// A texture not yet uploaded (size 0) gives a usable value instead of blowing up.
    #[test]
    fn a_source_with_no_size_falls_back_instead_of_dividing_by_zero() {
        let into = box_(120.0, 90.0);
        assert_eq!(contain(vec2(0.0, 0.0), into), into);
        assert_eq!(contain(vec2(-1.0, 10.0), into), into);
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        assert_eq!(cover_uv(vec2(0.0, 10.0), into), full);
        // The same where the place's side is 0 — which really happens on a folded panel.
        assert_eq!(cover_uv(vec2(100.0, 50.0), box_(0.0, 90.0)), full);
    }
}
