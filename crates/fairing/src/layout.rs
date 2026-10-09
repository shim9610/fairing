//! **Laying out a screen's body** — helpers for dividing up the Rect a screen is handed.
//!
//! # What is different from [`shell::Layout`](crate::shell::Layout)
//!
//! [`shell::Layout`](crate::shell::Layout) is **the result of the shell dividing the screen** — it
//! takes the status bar, the nav bar, the OSK, the edge zones and the rail off and hands the
//! `content` that is left to the screen. Up to there, an integrator has nothing to do.
//!
//! This module is what comes next. A screen is one egui closure and asks for nothing more — it
//! receives one `Ui`, and **dividing the inside of it is the integrator's**. Yet the layouts a
//! device UI needs over and over are a settled few — a
//! scrolling body with an action bar pinned at the bottom, a list grouped into cards, a list and a
//! body that split left and right where there is width to spare, a grid whose column count comes
//! from a minimum cell size. They are gathered here.
//!
//! # The boundary — layout and styling, and no further
//!
//! What this module decides is **what goes where, in how many columns** and **how it looks** (the
//! cards, the padding, the role colours, the touch targets). What goes inside is the callback's.
//!
//! So **no screen-content components are made** — a cart line, a star rating, a price format, an
//! allergen mark look different in every trade and the crate cannot guess. Open one and unit
//! pickers follow it in, each asking for options of its own.
//!
//! **This line moved once, and the move is worth recording.** It used to name "a quantity stepper"
//! first among those, and that was wrong: a stepper is not trade content, it is a *scalar editor*,
//! the same `−  n  +` in a kiosk, on a thermostat and on an oven timer. It belongs with
//! [`Switch`](crate::widgets::Switch) and [`TouchSlider`], which edit
//! a value the caller owns, and it is now [`Stepper`](crate::widgets::Stepper). The test that
//! sorted the three was not "does a shop use it" but **"is it the same object in every trade"** —
//! a cart line is not, a stepper is.
//!
//! Layout, by contrast, has nothing to do with the trade — a bar pinned at the bottom is the same
//! problem on a payment screen and on a process-confirmation screen. That line is where this ends.
//!
//! The built-in settings screens are this module's first consumer — all of them are
//! written with these helpers.
//!
//! # Two layers — vessels and rows
//!
//! The helpers here come in two layers. By name they look mixed together, but they do different
//! things.
//!
//! * **Vessels** — they divide up the space. [`page`] · [`group`] · [`Grid`] · [`action_bar`] ·
//!   [`split_width`] · [`split_height`] · [`title`] · [`section`] · [`note`]. They know nothing of
//!   what goes inside and hand the callback only a place.
//! * **Rows** — they lay out one row inside a vessel. [`switch_row`] · [`slider_row`] ·
//!   [`choice_rows`] · [`info_row`] · [`nav_row`] · [`icon_row`] · [`list_item`] ·
//!   [`ExpandableRow`] (with [`accordion`]) · [`advanced_rows`] ·
//!   [`status_card`]. The label on the left, the control on the right — that **placing** is all
//!   they do.
//! * **Fitting** — they fit something into a place. Text through [`fit_text`] · [`fit_size`],
//!   pictures through [`contain`] · [`cover_uv`]. They take no `Ui` and deal only in a `Painter` or
//!   in numbers, so they work where there is no `Ui` to hand — inside a grid cell, say.
//!
//! That is why the row helpers were not sent to [`widgets`](crate::widgets). `widgets` are **the
//! things drawn** (one switch, one track), and a row helper is **the layout** deciding where beside
//! the label that thing goes. Making someone writing one settings screen alternate between
//! `layout::group` and `widgets::switch_row` is not drawing a boundary; it is splitting the import
//! list in two.
//!
//! # Grouped into cards, divided by space
//!
//! Stack rows flat and divide them with nothing but thin lines and a twenty-row screen reads as one
//! block. Instead, related rows are grouped into a **rounded card** ([`group`]) and the cards are
//! spaced apart — then a scan breaks card by card and where one thing ends is clear without looking
//! for a line.
//!
//! Which is why **there are no separators inside a card.** The card's boundary already expresses the
//! group, and a line on top of that makes one card look divided again.
//!
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
//! The picture above is not the settings module's — it is this module's, and it declares for any
//! screen in one call. [`page_screen`] gives the body a scrolling page and writes the id once;
//! [`section_card`] is the heading-and-card pair the picture shows.
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
//! Nothing in there is the settings module's to lend. A built-in screen is the same call with a
//! body that happens to read [`Cx::settings`](crate::Cx) — which is why
//! [`settings::screens`](crate::settings::screens) exports its bodies: to be dropped into a page of
//! yours, or cloned under an id of yours with
//! [`ScreenDecl::with_id`](crate::screen::ScreenDecl::with_id).
//!
//! # Every colour is a role
//!
//! There is not one `Color32` literal in this file. Every colour goes through a [`ColorRole`], so
//! when a manufacturer swaps the colour system out through `[theme.palette]` or
//! [`ShellBuilder::palettes`](crate::shell::ShellBuilder::palettes), **the settings screens follow
//! whole.** Guide 09 §8 is that table.
//!
//! # Every dimension is a token too
//!
//! The row height derives from `metrics.row_height`, the padding from `metrics.screen_inset` and the
//! corners from `metrics.corner_radius`. They sit on the `1 du = 1 egui point` anchor, so
//! changing the finger policy (a gloved 13 mm in place of the bare 9 mm default) grows the
//! settings screens with it — which is why the numbers are not written down again here.

pub use fairing_widgets::fit::{contain, cover_uv};

mod transit;
pub use transit::{transit, Motion, Side, Transit, CARD_SCALE, SLIDE};

use crate::icons::IconRef;
use crate::screen::{screen, Cx, ScreenDecl};
use crate::theme::{card_radius, ColorRole, Elevation};
use crate::unit::{round_i8, round_u8};
use crate::widgets::{unroll, ColorSpec, ListRow, TouchSlider, Unrolled};
use egui::{Response, Ui};

/// **The three ways a container is set apart from the page**, and what each of them says.
///
/// The crate drew one kind — a filled card — and a screen made of nothing but filled cards is the
/// "big grey boxes repeating, so nothing tells you which element matters" the design review named.
/// The three reference screens use all three and pick by purpose, which is the point: the choice
/// carries meaning, so it has one word each rather than four independent knobs set the same way by
/// hand at every call site.
///
/// | | draws | says |
/// |---|---|---|
/// | [`Filled`](Self::Filled) | `SurfaceVariant` — or `Surface` on a panel that is already `SurfaceVariant` — `card_radius`, raised | **a thing you act on** — a group of settings rows, a stat strip, a media tile, a bar you press |
/// | [`Outlined`](Self::Outlined) | a hairline, no fill, `corner_radius`, flat | **a thing you read** — a chart, a reading, a result |
/// | [`Divided`](Self::Divided) | nothing, and a full-width rule under it | **a plain list** — rows that run edge to edge with no box |
///
/// The built-in settings screens group their rows in `Filled` cards, as One UI and iOS do. They
/// were `Divided` once, and at a one-finger row a page of edge-to-edge rows read as one long list
/// with the sections lost in it.
///
/// `Outlined` takes the smaller `corner_radius` rather than `card_radius` deliberately: a box with
/// no fill has only its edge, and a big radius on a hairline reads as a bubble. Part of why the
/// card radius looks generous is that until now everything was filled.
///
/// `Divided` is full-bleed — no side margin — because a rule that stops short of the screen's edge
/// reads as the bottom of a card that forgot to draw the rest of itself.
///
/// **A `Filled` card contrasts with whatever it is on.** Its fill is `SurfaceVariant`, one step
/// in from the `Surface` a screen is drawn on; on a panel that is itself `SurfaceVariant` — the
/// page inside a [`Rail`]'s elbow — the same card would be the panel's own colour and vanish, so
/// there it takes `Surface`, the step the other way. The panel says what it is through the
/// `Ui`'s `panel_fill`, which the rail sets for its page; a card never has to be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Container {
    /// A filled card. The default, and what every container in the crate was.
    #[default]
    Filled,
    /// A hairline edge and no fill.
    Outlined,
    /// No box: the rows run full width and a rule closes the group.
    Divided,
    /// **A wash of the accent** at `control.fill_alpha`, with no edge — the same tint a lit
    /// `FeatureCard` carries, as a container.
    ///
    /// For a group that should read as **lit** rather than merely raised — a selection, a live
    /// section — or on a panel where neither surface step is wanted. (A [`Self::Filled`] card on
    /// a `SurfaceVariant` panel takes `Surface` of its own accord, so this is no longer the only
    /// way to put a card on a rail's page.)
    ///
    /// It is the accent and not a grey because the accent is the one colour a palette guarantees
    /// stands off every surface role — a grey a shade from `SurfaceVariant` works in the palette it
    /// was picked in and nowhere else.
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

    /// **Which of the three container kinds this is** — see [`Container`].
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

    /// The inner padding **at the sides** (du). **Zero by default, and that is not an oversight.**
    ///
    /// A card full of rows must have none: a [`ListRow`] insets its own content by
    /// `metrics.content_inset`, so a card that also insets would push every label twice as far in
    /// and the rows would no longer line up with the heading above them.
    ///
    /// A card holding anything else — a label, a figure, a body an integrator drew — has the
    /// opposite problem, and until this method existed there was no way to fix it: the card's
    /// `inner_margin` was a hard zero on the horizontal axis and [`Deco::padding`] reached only the
    /// vertical one, so that content sat flush against the corner. Which of the two a card is, the
    /// crate cannot know; the caller can.
    ///
    /// `cx.theme.metrics.content_inset` is the value that lines such content up with the rows on
    /// neighbouring cards.
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

    /// **How high this container sits.**
    ///
    /// [`theme.elevation.card`](fairing_widgets::theme::ElevationMetrics::card) by default, which
    /// is [`Elevation::Raised`] — a card is a sheet lying on the page.
    /// [`action_bar_with`] defaults to [`Elevation::Floating`] instead, because a bar pinned over
    /// content that scrolls under it is by definition above that content; the deepest shadow on
    /// every reference screen measured for this is on exactly that bar.
    ///
    /// [`Elevation::None`] turns it off for one container. To turn it off for the whole shell —
    /// an e-ink or 16-level panel cannot reproduce a soft ramp and dithers it into rings — hand
    /// [`ElevationSpec::flat`](fairing_widgets::theme::ElevationSpec::flat) to
    /// [`ShellBuilder::elevation_spec`](crate::shell::ShellBuilder::elevation_spec).
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
/// The minimum track width for putting a slider row on **one line** (du). Shorter than this and one
/// finger's width is worth several percent of the value, so fine adjustment is impossible — the
/// track then goes on a line below the title.
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
/// The switch's width = its height × this. The same ratio as `widgets::Switch` — a purely visual
/// The title column's cap (relative to the row height). Wider than this leaves open ground behind a short title.
const SLIDER_LABEL_MAX: f32 = 2.4;
/// The share of the height left over that goes **above** when [`Grid::fill_height`] hits its cap. 0.5 is dead centre.
const FILL_LEAD: f32 = 0.5;

/// Whether to use a left/right split, and if so the left column's width.
///
/// **It looks at both the width and the aspect ratio.** Unfolding a foldable, or sitting on a wide
/// panel, makes one entry per list row a waste, but a tall screen split into two columns leaves both
/// sides cramped however large it is.
///
/// The judgement is made on **the Pane, not the screen** — when the workspace splits in half,
/// the settings screen inside it has to go back to one column.
#[must_use]
pub fn split_width(cx: &Cx<'_>) -> Option<f32> {
    let rect = cx.pane.rect;
    let (w, h) = (rect.width(), rect.height().max(1.0));
    (w >= SPLIT_MIN_WIDTH && w / h >= SPLIT_MIN_ASPECT).then(|| (w * LIST_FRACTION).min(LIST_MAX))
}

/// **The rail's share of the width when the chrome is an L** — narrower than a settings split's,
/// because this column holds icons and one word each rather than a list of settings rows.
const RAIL_FRACTION: f32 = 0.18;

/// The cap on that, so a very wide console does not give a six-entry rail a third of the glass.
const RAIL_MAX: f32 = 280.0;

/// The floor, so the arm can still hold an icon and its label side by side.
const RAIL_MIN: f32 = 132.0;

/// **A screen whose navigation rail is part of the chrome** — the top bar and the rail read as one
/// surface, an L lying on its side, with the page as a lighter panel inside its elbow.
///
/// # Why this is a layout and not two calls
///
/// The crate's other screen anatomies divide a screen into bands and *draw a line* between them,
/// because with an arbitrary palette `Surface` and `Background` can be near enough to each other
/// that colour alone does not separate anything — that is written on `Desktop::rail_ui`, and it is
/// true for a rail that is a region of the page. It stops being true when the rail is **chrome**:
/// the top bar already paints `Surface`, so a rail arm painting the same role continues it, and a
/// line drawn between the two would cut the shape in half rather than divide anything. What
/// separates the page from the chrome is then the page's own panel, one step lighter, with its
/// inner corner rounded where it meets the elbow.
///
/// So the two decisions — *no line between the arms*, and *the page is a panel rather than a
/// background* — only hold together if one thing makes both. Made separately by a caller, the first
/// one alone is a rail that has merged into the bar and lost its edge.
///
/// # Folding
///
/// On a panel too narrow or too upright to divide, [`split_width`]'s judgement, the rail is not
/// drawn at all and the page takes the whole screen. `rail` is not called, so a caller does not
/// need to test for it. Where the entries still have to be reachable, put them on the page's
/// header or in a nav bar — a rail folded onto a narrow panel is a drawer, and a drawer that opens
/// itself over the page is the thing device UIs get wrong.
///
/// # Colours
///
/// `deco` paints the **page panel**, not the chrome: the chrome is the bar's own colour by
/// definition, and a rail that did not match it would not be an L. The default fill is
/// [`ColorRole::SurfaceVariant`], a step in from `Surface` in every palette the crate ships,
/// light and dark. The page `Ui` carries that fill as its `panel_fill`, so a
/// [`Container::Filled`] card on it — a [`group`], a [`Grid`]'s tiles, an [`action_bar`] —
/// takes `Surface` and stands off the panel, where it would otherwise have been the panel's
/// own colour and vanished.
///
/// # What the rail hands back
///
/// The rail closure **returns** what was picked rather than writing it, and `rail_split` hands it
/// back beside the page's own result, `None` where the rail was folded away. That is not
/// ceremony: the rail writes the selection and the page reads it in the same frame, so a rail that
/// mutated the caller's state directly would need a second mutable borrow of it while the page
/// closure still held one, and no caller can hand out both. Returning the pick moves the write
/// after both closures have finished, where it is one line and it compiles. It is the same shape
/// [`action_bar`] uses, which returns the bar's result and not the body's.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// # let mut open = 0usize;
/// use fairing::{icon, layout};
/// let (picked, ()) = layout::rail_split(
///     ui,
///     cx,
///     |ui, cx| {
///         let names = ["Home", "Run", "Results"];
///         (0..names.len()).find(|i| {
///             layout::list_item(ui, cx, icon::HOME, names[*i], open == *i).clicked()
///         })
///     },
///     |ui, cx| layout::note(ui, cx, "the page"),
/// );
/// if let Some(i) = picked.flatten() {
///     open = i;
/// }
/// # }
/// ```
pub fn rail_split<A, B>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    rail: impl FnOnce(&mut Ui, &mut Cx<'_>) -> A,
    page: impl FnOnce(&mut Ui, &mut Cx<'_>) -> B,
) -> (Option<A>, B) {
    let out = Rail::new().show(ui, cx, rail, page);
    (out.picked, out.page)
}

/// Inject the page panel's colours and corner into [`rail_split`].
///
/// For anything more than that — a rail a swipe can fold, or two rails on one screen — use
/// [`Rail`], which this is a shorthand for.
pub fn rail_split_with<A, B>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    deco: Deco,
    rail: impl FnOnce(&mut Ui, &mut Cx<'_>) -> A,
    page: impl FnOnce(&mut Ui, &mut Cx<'_>) -> B,
) -> (Option<A>, B) {
    let out = Rail::new().deco(deco).show(ui, cx, rail, page);
    (out.picked, out.page)
}

/// The collapsed arm: wide enough for an icon and its disc, and nothing else.
const RAIL_COLLAPSED: f32 = 64.0;

/// How far a finger has to travel across the arm before it counts as a fold rather than a tap.
const RAIL_SWIPE_DU: f32 = 24.0;

/// The strip along the left edge that still counts as the arm once it has folded away, so a
/// [`Fold::Away`] rail can be dragged back out without anything to press.
const RAIL_GRIP_DU: f32 = 24.0;

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

/// **Where the fold gesture is read** — [`Rail::fold_gesture`].
///
/// [`Anywhere`](Self::Anywhere) is the default. It began as the opt-in, and the one
/// integrator with a collapsible rail in the field never opted in: their rail folded from the arm
/// and from nowhere else, which was reported three times as the page swipe not working. A rail
/// that is collapsible at all is one the hand is meant to put away, and a hand on a touch panel
/// lands where it lands; reading the gesture on a strip a finger wide is the surprise, not the
/// safe choice. [`Arm`](Self::Arm) is there for a page whose own horizontal drags are
/// not controls that say so.
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

    /// **Let a swipe fold it and back.** Off by default; on, the swipe is read anywhere — across
    /// the arm or across the page — unless [`Self::fold_gesture`] narrows it to the arm.
    ///
    /// Off by default because folding is a *choice about the product*, not an improvement. On a
    /// console bolted to a bench the rail is the map of the machine and it should not be possible
    /// to lose it by brushing the glass; on a cart display where the page is a plate map and every
    /// millimetre counts, folding it is the point. The crate cannot tell which one it is in, and
    /// the failure modes are not symmetric — a rail that folds when nobody meant it to is a
    /// support call, a rail that does not fold is a preference.
    ///
    /// How far it folds is [`Self::fold_to`]; where the gesture is read is
    /// [`Self::fold_gesture`]. A screen that should have no rail at all is a different thing —
    /// the app puts the rail away with [`Self::hidden`], and the hand cannot bring it back.
    /// The fold is one motion: the words fade first, the icons slide to
    /// the middle of the narrowing arm, and the bar beside the live entry melts into a tinted disc
    /// behind its icon — a bar with nothing beside it read as a stray line. [`list_item`] does all
    /// of that by reading the fold itself, so a rail built out of rows needs no branch for it.
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
            d.get_temp::<bool>(egui::Id::new(self.salt))
                .unwrap_or(false)
        })
    }

    /// **Fold or open the rail from code** — for a menu button the page draws, which a
    /// [`Fold::Away`] rail wants since there is nothing left on screen to press. The width still
    /// tweens from here exactly as it does from a gesture.
    pub fn set_folded(self, ctx: &egui::Context, folded: bool) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(self.salt), folded));
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
        let id = egui::Id::new(self.salt);
        let folded = self.collapsible && self.is_folded(ui.ctx());
        // **The width is animated, the state is a bool.** Folding is one bit; how far through the
        // fold this frame is, is a tween, and `Cx::animate` is where the crate keeps those so
        // `[motion] reduce` turns it off everywhere at once.
        let shut = match self.fold {
            Fold::Icons => RAIL_COLLAPSED,
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
            let fold_id = egui::Id::new(FOLD_KEY);
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

    /// **Read the pointer, do not claim it.**
    ///
    /// An `interact` over the whole arm is the obvious way to catch the swipe and it does not
    /// work: registered before the rows it never sees a drag, because the rows are over it and a
    /// later widget wins egui's hit test; registered after, it wins, and then a tap on an entry
    /// folds the rail instead of opening the entry. There is no order that gives both. So the
    /// gesture is read straight off the pointer, which takes part in no hit test at all and leaves
    /// every row's tap exactly as it was.
    ///
    /// One rule, wherever the press began: a drag past [`RAIL_SWIPE_DU`] with horizontal intent
    /// — `|dx| > |dy|`, so a finger scrolling a long rail or page does not fold it on the way
    /// past — folds or opens **at once**, as the drag crosses the threshold. On the arm (or in
    /// the grip strip a folded-away arm leaves behind) that is all; on the page, with
    /// [`FoldGesture::Anywhere`], two things are left alone: a press in the shell's edge zones,
    /// which are the shell's own gestures, and a drag a control owns — a control that follows
    /// the finger says so ([`crate::drag::claim`]), frame by frame, and its drag is its own. That
    /// ownership is kept by hand from there to the release, because no claim is made on the
    /// release frame; so is the press point, which egui forgets on that frame.
    ///
    /// It used to be egui's word and a height: the widget holding the drag, and whether it was
    /// no taller than two rows. A button senses drags too, for its long-press ring, and is one
    /// row tall — so a swipe that began on a button never folded the rail.
    ///
    /// The first build decided the page swipe on release, by speed or by a long distance, so the
    /// page would never reflow under a finger still down. It did not register for a mouse that
    /// stops before it lets go and drags less than the distance asked (user report, twice). A
    /// drag past a couple of dozen du is decidedly a drag to egui, so nothing under the finger is
    /// clicked when it lifts — the reflow costs nothing — and the rule the arm has always had
    /// is the one a hand expects everywhere.
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
        if dx.abs() <= RAIL_SWIPE_DU || dx.abs() <= dy.abs() {
            return;
        }
        let grip = band.with_max_x(band.left() + band.width().max(RAIL_GRIP_DU));
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

/// The chrome band the arm paints: `arm_rect`, pushed out to the pane's edge on any side where
/// only the screen inset keeps it from the glass. The right edge is the arm's.
fn chrome_band(cx: &Cx<'_>, full: egui::Rect, arm_rect: egui::Rect) -> egui::Rect {
    let pane = cx.pane.outer;
    let inset = cx.theme.metrics.screen_inset + 0.5;
    let left = if full.left() - pane.left() <= inset {
        pane.left()
    } else {
        full.left()
    };
    let top = if full.top() - pane.top() <= inset {
        pane.top()
    } else {
        full.top()
    };
    let bottom = if pane.bottom() - full.bottom() <= inset {
        pane.bottom()
    } else {
        full.bottom()
    };
    egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(arm_rect.right(), bottom))
}

/// **The rail arm's width, or `None` where the panel cannot carry one** — [`rail_split`]'s
/// judgement, made on the same grounds as [`split_width`]'s and with the rail's own proportions.
///
/// A caller normally does not need this: `rail_split` folds by itself. It is public so a screen
/// can lay its header out knowing whether a rail is beside it.
#[must_use]
pub fn rail_width(cx: &Cx<'_>) -> Option<f32> {
    let rect = cx.pane.rect;
    let (w, h) = (rect.width(), rect.height().max(1.0));
    (w >= SPLIT_MIN_WIDTH && w / h >= SPLIT_MIN_ASPECT)
        .then(|| (w * RAIL_FRACTION).clamp(RAIL_MIN, RAIL_MAX))
}

/// Decide whether to divide top and bottom, and if so hand back **the top band's height** —
/// [`split_width`]'s vertical counterpart.
///
/// On an upright panel (an ordering kiosk, an access terminal, portrait signage) the top of the
/// screen is out of reach. So the top becomes a **band to look at** and the bottom a **band to
/// press** — which [`action_bar`] cannot do, since it only pins a bar to the bottom and cannot move
/// the body's **starting point** down.
///
/// **The judgement is made on the Pane and the height measured from `ui`.** "Is this device worth
/// dividing into bands" is a question about the device, so like [`split_width`] it looks at
/// `cx.pane`; the height handed back is measured from where the caller will actually cut (`ui`'s
/// remaining Rect). Wider than it is tall, or lower than **12 times the row height**, gives `None`
/// (lower than that and neither band can hold anything), and then the screen can simply use the
/// whole `ui` — the same convention as `split_width` returning `None` when it is narrow.
///
/// **The crate does not set the ratio.** How much the top band should be is decided by the mounting
/// height (the stand, the panel size, the statutory control range), and that is not something the
/// crate can know. Give `top` that share and it is clamped to **0.15 … 0.60** — 0.1 is padding
/// rather than a band, and 0.7 pushes the control band below where the hand reaches.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout;
/// // A 900 mm panel on an 800 mm stand — the top third is the eye's.
/// let Some(hero_h) = layout::split_height(ui, cx, 0.34) else {
///     return; // laid on its side, or a low panel: draw it as one block
/// };
/// let full = ui.available_rect_before_wrap();
/// let split = full.top() + hero_h;
/// # let _ = split;
/// # }
/// ```
#[must_use]
pub fn split_height(ui: &Ui, cx: &Cx<'_>, top: f32) -> Option<f32> {
    let pane = cx.pane.rect;
    let avail = ui.available_rect_before_wrap();
    let tall_enough = pane.height() >= cx.theme.metrics.row_height * BAND_MIN_ROWS;
    (pane.height() > pane.width() && tall_enough)
        .then(|| avail.height() * top.clamp(BAND_TOP_MIN, BAND_TOP_MAX))
}

/// Wrap one settings screen. **The vertical scroll and the side padding are given here, once.**
///
/// A device screen is short — eight rows of 56 do not fit on a 480 px panel. Left to each screen to
/// remember, one would be missed and only that screen would be cut off at the bottom.
///
/// **One id per page.** The id is the scroll position's identity, and nothing else tells two
/// pages apart: three tabs drawn through one call with one id share one offset, so a tab
/// scrolled halfway opens the next tab halfway (the console example's three boards did). Anything
/// that hashes will do — `("board", tab)` gives each tab its own.
pub fn page(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    id: impl std::hash::Hash + std::fmt::Debug,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
) {
    let bottom = cx.pane.inset_bottom;
    let gap = cx.theme.metrics.row_height * CARD_GAP;
    // The height is **stated explicitly.** Where the parent's size is undecided, `available_height()`
    // is infinite and the `ScrollArea` decides "nothing overflows", killing the scroll entirely.
    let height = cx.pane.rect.height().max(cx.theme.metrics.row_height);
    egui::ScrollArea::vertical()
        .id_salt(&id)
        .max_height(height)
        // The default is `DragScroll::OnTouch`, so a list does not push on a device operated with a
        // mouse or a trackball. Touch is common on devices, but it is not the only thing.
        .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(gap);
            body(ui, cx);
            // The bottom is padded so the OSK and the nav bar do not eat the last card.
            ui.add_space(bottom + gap * 2.0);
            keep_focus_clear(ui, cx, &id);
        });
}

/// **A page keeps what is being typed into clear of the keyboard**.
///
/// The keyboard lies over the bottom of the pane and pushes nothing. A page pads its foot
/// by the keyboard's height so its rows *can* be scrolled clear of it, but nothing scrolled them:
/// a field in the lower half of the screen was typed into blind (user report). So while the
/// keyboard is rising, or the focus has just moved to another widget, the page scrolls the
/// focused widget up until it sits half a row above the keys. Once it is clear the page is left
/// alone — a page the operator scrolled by hand while typing stays where they put it.
///
/// Two pages side by side each see the one focus; only the page whose view holds the widget
/// moves.
fn keep_focus_clear(ui: &Ui, cx: &Cx<'_>, id: &(impl std::hash::Hash + std::fmt::Debug)) {
    let inset = cx.pane.inset_bottom;
    let ctx = ui.ctx();
    let inset_key = egui::Id::new(("fairing.page.keyboard", id));
    let focus_key = inset_key.with("focus");
    let expect_key = inset_key.with("expect");
    let was = ctx.data(|d| d.get_temp::<f32>(inset_key).unwrap_or(0.0));
    ctx.data_mut(|d| d.insert_temp(inset_key, inset));
    if inset <= 0.0 {
        ctx.data_mut(|d| {
            d.remove_temp::<Option<egui::Id>>(focus_key);
            d.remove_temp::<f32>(expect_key);
        });
        return;
    }
    let Some(focused) = ctx.memory(egui::Memory::focused) else {
        return;
    };
    let last = ctx
        .data(|d| d.get_temp::<Option<egui::Id>>(focus_key))
        .flatten();
    ctx.data_mut(|d| d.insert_temp(focus_key, Some(focused)));
    let rising = inset > was + 0.01;
    let moved = last != Some(focused);
    if !rising && !moved {
        ctx.data_mut(|d| d.remove_temp::<f32>(expect_key));
        return;
    }
    let Some(rect) = ctx.read_response(focused).map(|r| r.rect) else {
        return;
    };
    if !ui.clip_rect().intersects(rect) {
        return;
    }
    let clear = cx.pane.rect.max.y - inset - cx.theme.metrics.row_height * 0.5;
    // Where the widget's foot will be once every scroll already asked for has landed. A request
    // takes egui a frame or two to show in the rect, and measuring the gap again from the old
    // rect in the meantime asked for it twice over - the first build sent the field off the top.
    // A foot higher than expected is the operator's own scroll, and is taken as it stands.
    let observed = rect.max.y;
    let expected = if moved {
        observed
    } else {
        ctx.data(|d| d.get_temp::<f32>(expect_key))
            .unwrap_or(observed)
    };
    let base = expected.min(observed);
    let gap = base - clear;
    let next = if gap > 0.5 {
        // A negative delta moves the content up. At once, not eased: the keyboard's own rise is
        // the motion, and this is measured again on every frame of it.
        ui.scroll_with_delta_animation(
            egui::Vec2::new(0.0, -gap),
            egui::style::ScrollAnimation::none(),
        );
        base - gap
    } else {
        base
    };
    ctx.data_mut(|d| d.insert_temp(expect_key, next));
}

/// **A settings-shaped screen, declared in one call.** The body is wrapped in [`page`], so it
/// scrolls, and the id is written once.
///
/// This is the shape every built-in settings screen has, and the shape an integrator's own screen
/// usually wants: a scrolling column of sections and cards. Written out it is two closures and the
/// id twice —
///
/// ```no_run
/// # fn main() {
/// # let shell: &mut fairing::Shell = todo!();
/// # fn body(_: &mut egui::Ui, _: &mut fairing::Cx<'_>) {}
/// use fairing::{layout, screen};
///
/// shell.add(screen("app.heater", |ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>| {
///     layout::page(ui, cx, "app.heater", |ui, cx| body(ui, cx));
/// }).title("Heater"));
/// # }
/// ```
///
/// — and the `page` is easy to forget, which costs nothing until that one screen is the only one
/// cut off at the bottom. The crate had this helper from the start and kept it private to the
/// settings module, so every integrator wrote the long form:
///
/// ```no_run
/// # fn main() {
/// # let shell: &mut fairing::Shell = todo!();
/// # fn body(_: &mut egui::Ui, _: &mut fairing::Cx<'_>) {}
/// use fairing::layout;
///
/// shell.add(layout::page_screen("app.heater", body).title("Heater"));
/// # }
/// ```
///
/// Everything [`ScreenDecl`] carries is still yours to set afterwards — the title, the icon, the
/// gate, `desktop()`, the chrome policy.
pub fn page_screen(
    id: impl Into<String>,
    mut body: impl FnMut(&mut Ui, &mut Cx<'_>) + 'static,
) -> ScreenDecl {
    let id = id.into();
    let salt = id.clone();
    screen(id, move |ui: &mut Ui, cx: &mut Cx<'_>| {
        page(ui, cx, &salt, |ui, cx| body(ui, cx));
    })
}

/// **A section heading and the card under it**, which is how they nearly always come: 25 of the
/// crate's 32 `section` calls are immediately followed by a `group`.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout::{info_row, section_card};
///
/// section_card(ui, cx, "Hopper", |ui, cx| {
///     let _ = info_row(ui, cx, "Resin A", "72 %");
///     let _ = info_row(ui, cx, "Resin B", "40 %");
/// });
/// # }
/// ```
///
/// A card with no heading over it is [`group`]; a heading with something other than a card under
/// it is [`section`]. This is only the pair.
pub fn section_card(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
) {
    section_card_with(ui, cx, title, Deco::new(), body);
}

/// [`section_card`] with the container said out loud — see [`Container`].
///
/// A screen that groups its rows one way has to group **all** of them that way: a settings page
/// with a divider-only group above a filled one reads as two screens stitched together, which is
/// what the built-ins looked like the first time this was tried with only `group_with` converted.
pub fn section_card_with(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    deco: Deco,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
) {
    section(ui, cx, title);
    group_with(ui, cx, deco, body);
}

/// **A horizontally scrolling lane of chips.**
///
/// The vessel only — what goes in it is the caller's, which is how a *multi*-select filter row is
/// written: put [`Chip`](crate::widgets::Chip)s in it yourself, each editing a bit of your own.
/// For the ordinary single-select case, [`ChipRow`](crate::widgets::ChipRow) owns the index and
/// keeps the "exactly one" invariant in one place.
///
/// The lane is inset by `metrics.screen_inset` at each end rather than by the screen's own margin,
/// which is what a full-bleed scroller looks like: the reference row this was measured from starts
/// ten pixels outside its own card column, so the chips run under the screen edge instead of
/// stopping short of it.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// # let mut hot = false;
/// use fairing::layout::chip_row;
/// use fairing::widgets::Chip;
///
/// chip_row(ui, cx, "menu.filters", |ui, cx| {
///     let _ = Chip::new("Hot", &mut hot).show(ui, &mut cx.widgets());
/// });
/// # }
/// ```
pub fn chip_row<R>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    id: &str,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>) -> R,
) -> R {
    let inset = cx.theme.metrics.screen_inset;
    let gap = cx.theme.control.gap;
    let out = egui::ScrollArea::horizontal()
        .id_salt(id)
        .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.add_space(inset);
                let out = body(ui, cx);
                ui.add_space(inset);
                out
            })
            .inner
        });
    crate::widgets::paint_lane_fade(
        ui,
        &cx.widgets(),
        out.inner_rect,
        out.content_size.x,
        out.state.offset.x,
    );
    ui.add_space(cx.theme.metrics.row_height * CARD_GAP);
    out.inner
}

/// **A scrolling body with a bar pinned at the bottom** — the basic form of a payment or
/// confirmation screen.
///
/// A confirm button tacked onto the end of a list has to be scrolled to when the list is long. On a
/// device that is another way of saying "payment does not work". The action has to be **always in
/// the same place**.
///
/// `bar` is pinned to the bottom and `body` scrolls within the height left. The return value is
/// whatever `bar` handed back, so a button click can be passed straight out.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout;
/// use fairing::widgets::{BigButton, ButtonKind};
///
/// let pay = layout::action_bar(
///     ui,
///     cx,
///     1.8,
///     |ui, cx| {
///         // A body that may run long — it scrolls by itself.
///         for line in 0..40 {
///             layout::info_row(ui, cx, "Item", &line.to_string());
///         }
///     },
///     |ui, cx| {
///         BigButton::new("Pay")
///             .kind(ButtonKind::Primary)
///             .show(ui, &mut cx.widgets())
///             .clicked()
///     },
/// );
/// # let _ = pay;
/// # }
/// ```
///
/// `bar_rows` gives the bar's height as a multiple of `metrics.row_height` — leave it in absolute du
/// and it does not follow the glove policy.
pub fn action_bar<R>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    bar_rows: f32,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
    bar: impl FnOnce(&mut Ui, &mut Cx<'_>) -> R,
) -> R {
    action_bar_with(ui, cx, bar_rows, Deco::new(), body, bar)
}

/// Inject colours and corners into [`action_bar`]. For painting a payment bar in the brand colour.
pub fn action_bar_with<R>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    bar_rows: f32,
    deco: Deco,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
    bar: impl FnOnce(&mut Ui, &mut Cx<'_>) -> R,
) -> R {
    let m = &cx.theme.metrics;
    let height = m.row_height * bar_rows.max(1.0);
    let radius = deco.radius_of(cx);
    let fill = deco.fill_of(ui, cx);
    let inset = m.screen_inset;
    // When the OSK comes up the bar hides below it — screens where confirm has to be pressed with the
    // keyboard up are common.
    let bottom = cx.pane.inset_bottom;

    let area = ui.available_rect_before_wrap();
    // **The logical place.** The body flows only this far and the scroll does not go below it.
    let slot = egui::Rect::from_min_max(
        egui::pos2(area.left(), area.bottom() - height - bottom),
        egui::pos2(area.right(), area.bottom() - bottom),
    );
    // **The place drawn.** Given a `visual_inset` the bar lifts off the screen's edge and becomes a
    // floating bar — the place is still taken, so the body does not flow underneath it.
    let vi = deco.visual_inset_of();
    let bar_rect = if vi > 0.0 { slot.shrink(vi) } else { slot };
    let body_rect = egui::Rect::from_min_max(
        area.min,
        egui::pos2(area.right(), slot.top().max(area.top())),
    );

    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body_rect));
    body_ui.set_clip_rect(body_rect);
    egui::ScrollArea::vertical()
        .id_salt("fairing.layout.action_bar")
        .max_height(body_rect.height())
        .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
        .auto_shrink([false, false])
        .show(&mut body_ui, |ui| {
            body(ui, cx);
            ui.add_space(cx.theme.metrics.row_height * CARD_GAP);
        });

    let level = deco.elevation_of(Elevation::Floating);
    // A bar draws in order, so its height goes down first and the fill covers the shadow's
    // interior — no reserved index is needed here, unlike a card whose rect `Frame` decides.
    {
        let corner = egui::CornerRadius::same(radius);
        let ink = cx.theme.elevate(level);
        if ink.shadow != egui::epaint::Shadow::NONE {
            ui.painter().add(ink.shadow.as_shape(bar_rect, corner));
        }
        if ink.rim != egui::Stroke::NONE {
            ui.painter()
                .rect_stroke(bar_rect, corner, ink.rim, egui::StrokeKind::Inside);
        }
    }
    if fill.a() > 0 {
        // A bar against the edge is rounded on its top corners only — the bottom is the screen's end and
        // the curvature is cut. A floating bar (`visual_inset`) shows all four.
        let corner = if vi > 0.0 {
            egui::CornerRadius::same(radius)
        } else {
            egui::CornerRadius {
                nw: radius,
                ne: radius,
                sw: 0,
                se: 0,
            }
        };
        ui.painter().rect_filled(bar_rect, corner, fill);
    }
    deco.paint_stroke(ui, cx, bar_rect);
    // **`Divided` is full-bleed here too**. A bar of buttons is inset from the screen's
    // edge, because a `BigButton` flush against the glass reads as a cut-off one. A bar holding a
    // *row* is the opposite case: `list_item` and the rest do their own insetting, so a second one
    // here puts the foot's icon a `screen_inset` to the right of the icons in the list above it,
    // which is what a rail's pinned Power Off once looked like. The container kind
    // already carries this distinction everywhere else, so it carries it here.
    let bar_inset = match deco.container_kind() {
        Container::Divided => 0.0,
        Container::Filled | Container::Outlined | Container::Tinted => inset,
    };
    let mut bar_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_rect.shrink2(egui::vec2(bar_inset, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar_ui.set_clip_rect(bar_rect);
    let out = bar(&mut bar_ui, cx);
    ui.advance_cursor_after_rect(area);
    out
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
    /// **Let the cell's height be answered from its realised width.**
    ///
    /// A grid picks its column count from the width it is given, so a caller writing
    /// `Grid::new(_, rows)` is guessing a height for a width it cannot see yet. Where the thing in
    /// the cell has a shape of its own — a [`MediaCard`](crate::widgets::MediaCard) with an aspect,
    /// most of all — the guess is wrong and the widget pays for it out of its drawing. Measured on
    /// the kiosk's menu grid: a tile asking for a square picture was handed 282.7 du where it
    /// needed 636.4, and cropped 71 % off every photograph rather than overrun the cell.
    ///
    /// The function is given the cell's width in du and answers the height in du. It replaces
    /// `cell_rows` as the floor; `fill_aspect` still caps it.
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

    /// **The column cap.** However much width is left, it makes no more columns than this.
    ///
    /// There is no cap by default, and that principle ("as many as the width allows") is right for a
    /// product grid — on a wide panel it is better to see more in a row. But **a strip with a fixed
    /// count** is different: five category chips, three payment methods, two of eat-in/take-away —
    /// the requirement is "all on one row" and the width comes second.
    ///
    /// Without this, an integrator ends up **working the minimum cell width back from the width** —
    /// five lines like `(avail / wanted_columns) / row_height * 0.86` turn up wherever the grid is
    /// used, and nobody knows why that `0.86` is 0.86. It was duplicated in six places in the kiosk
    /// example.
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
    ///
    /// **It does not clamp to the item count** — `max_columns(6)` on a list of three puts three cells
    /// in a 6-column grid. That is so a tile's width does not change midway on a screen items flow
    /// into; where "only as many as the items" is wanted, write `max_columns(items.len())`.
    ///
    /// 0 is ignored (the same as no cap).
    #[must_use]
    pub const fn max_columns(mut self, cols: usize) -> Self {
        self.max_cols = if cols == 0 { None } else { Some(cols) };
        self
    }

    /// **Give the height left over back to the cells** — [`max_columns`](Self::max_columns)'s
    /// vertical counterpart.
    ///
    /// A grid fills from the top and does not use the height left over. On a wide screen it does not
    /// show, but **on an upright panel three tiles cling to the top and the bottom half is entirely
    /// empty.** The kiosk example measured `remaining_height / rows / row_height` itself in four
    /// places and sat it down once more with `add_space`.
    ///
    /// The `cell_rows` given to [`new`](Self::new) becomes the **lower bound** and the value given
    /// here is the upper — and the upper bound is a **height-to-tile-width ratio**. It is a ratio
    /// rather than a row multiple because that is what an integrator actually wants to stop: three
    /// cells stretching tall leave the icons floating in space. Written as a row multiple, nobody
    /// would know why that number is `6.4`.
    ///
    /// Where the cap leaves height over, **the grid is sat in the middle vertically.** Pile the space
    /// left at the top and the control band droops; pile it at the bottom and the tiles cling to the
    /// ceiling.
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
    ///
    /// It is safe even where the parent's height is undecided and the height left is infinite — the
    /// ratio cap catches it first. Anything at or below 0 is ignored (the same as not filling).
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

/// [`Grid`] with the default decoration, in one line (the same as `Grid::new(..).show(..)`).
pub fn grid<T>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    min_cell_rows: f32,
    cell_rows: f32,
    items: &[T],
    cell: impl FnMut(&mut Ui, &mut Cx<'_>, Cell<'_, T>),
) {
    Grid::new(min_cell_rows, cell_rows).show(ui, cx, items, cell);
}

/// How many columns fit the width (at least 1).
fn columns_for(avail: f32, min_cell: f32, gap: f32) -> usize {
    let mut cols = 1usize;
    while gap.mul_add(f32_from(cols + 1) + 1.0, min_cell * f32_from(cols + 1)) <= avail {
        cols += 1;
        if cols >= 12 {
            break;
        }
    }
    cols
}

/// `usize` → `f32`. The column count is 12 or under, so it is exact.
#[expect(clippy::cast_precision_loss, reason = "at most 12 columns")]
fn f32_from(value: usize) -> f32 {
    value as f32
}

/// The screen title. Once, large, above the cards — it says what the right column is in a left/right split.
///
/// [`header`] with no subtitle and no actions, so the one that has them and the one that does not
/// start their text at the same place.
pub fn title(ui: &mut Ui, cx: &mut Cx<'_>, text: &str) {
    header(ui, cx, text, None, |_, _| ());
}

/// **A screen's opening: a title, a muted line under it, and a place for the screen's own actions.**
///
/// The three reference screens open the same way and it is most of why they read as one product —
/// "Appearance" over "Display and interface" with *Restore defaults* on the right, "Dashboard" over
/// "Device overview" with a picker and a *Pause*, "Quick settings" with a close. Before this the
/// crate had a title and nothing else, so a screen needing an action in its header hand-rolled a
/// `ui.horizontal` and no two lined up — and the built-in settings screens simply went without.
///
/// # What is settled here
///
/// The left edge is `metrics.content_inset`, the same one the rows below start at (a heading that
/// does not reads as a different column). The subtitle is `type_scale.small` in
/// [`ColorRole::Muted`], one `control.line_gap` under the title. The trailing slot is laid out
/// **right to left** and centred on the *title's* line rather than on the whole block, which is
/// where the references put it: with a subtitle present, centring on the block would drop a button
/// half a line below the name it belongs to.
///
/// The title takes the display face ([`Theme::display`](crate::theme::Theme::display)), seeded from
/// the strong faces, so with no display font registered this is the bold draw it always was. A
/// title is the **only** place the crate reaches for that face — a headline font may be missing
/// glyphs a value row cannot do without.
///
/// # It does not fight the chrome
///
/// The title the shell knows (`ScreenDecl::title`) names the screen in the task switcher and in the
/// back button's label; this one is the page's own heading. They are the same words on purpose and
/// in two different places, the way a browser tab and an `<h1>` are. `Cx::set_subject` is unrelated
/// — that is the access level, not a name.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout;
/// use fairing::widgets::{BigButton, ButtonKind};
///
/// layout::header(ui, cx, "Appearance", Some("Display and interface"), |ui, cx| {
///     let _ = BigButton::new("Restore defaults")
///         .kind(ButtonKind::Normal)
///         .show(ui, &mut cx.widgets());
/// });
/// # }
/// ```
pub fn header<R>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    subtitle: Option<&str>,
    actions: impl FnOnce(&mut Ui, &mut Cx<'_>) -> R,
) -> R {
    let (inset, gap, line_gap) = (
        cx.theme.metrics.content_inset,
        cx.theme.metrics.row_height * CARD_GAP,
        cx.theme.control.line_gap,
    );
    let title_font = cx.theme.display(cx.theme.metrics.type_scale.heading);
    let sub_font = egui::FontId::proportional(cx.theme.metrics.type_scale.small);
    let title_h = ui.ctx().fonts_mut(|f| f.row_height(&title_font));
    let sub_h = subtitle.map_or(0.0, |_| {
        line_gap + ui.ctx().fonts_mut(|f| f.row_height(&sub_font))
    });
    // **A band with actions in it is at least a control tall**, so a button in the header has
    // room and two headers with buttons sit at one height. A band with none is as tall as its
    // words: reserving a control's height under a bare title left a gap between the title and the
    // first card that read as a missing row, and at a one-finger row it was most of one.
    let block_h = title_h + sub_h;
    let control_h = crate::theme::control_height(&cx.theme.metrics, &cx.theme.control);
    let top = ui.cursor().min;
    let width = ui.available_width();
    let with_actions = block_h.max(control_h);
    // The actions first, right to left — then the words take what is left. Painted first and
    // unbounded, a title ran under the actions as soon as the pane was narrower than both: a split
    // pane put "Dashboard" through its own badge.
    // Centred on the title's line, not on the band: see the doc.
    let slot = egui::Rect::from_min_max(
        egui::pos2(top.x + inset, top.y),
        egui::pos2(
            top.x + width - inset,
            top.y + title_h.max(with_actions - sub_h),
        ),
    );
    let (out, used) = {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(slot)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let out = actions(&mut child, cx);
        (out, child.min_rect())
    };
    let has_actions = used.width() > 0.5;
    let band_h = if has_actions { with_actions } else { block_h };
    let (band, _) = ui.allocate_exact_size(egui::vec2(width, band_h), egui::Sense::hover());
    let text = egui::Rect::from_min_max(
        egui::pos2(band.min.x + inset, band.min.y),
        egui::pos2(band.max.x - inset, band.max.y),
    );
    let words_end = if has_actions {
        (used.min.x - inset).max(text.min.x)
    } else {
        text.max.x
    };
    let room = (words_end - text.min.x).max(1.0);
    // **Stacked from the top, not centred on it.** Centring put the subtitle's baseline exactly on
    // the band's bottom edge — the arithmetic came out flush to the last decimal — so a pixel of
    // rounding, or a face whose descender runs past `row_height`, let it under whatever was drawn
    // next. It showed up as a subtitle sliced in half by the first card below it.
    let mut y = band.min.y + (band_h - block_h).max(0.0) * 0.5;
    let painter = ui.painter();
    let line = |text: &str, font: egui::FontId, color: egui::Color32| {
        let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        painter.layout_job(job)
    };
    let title_color = cx.theme.color(ColorRole::OnSurface);
    painter.galley(
        egui::pos2(text.min.x, y),
        line(title, title_font, title_color),
        title_color,
    );
    y += title_h + line_gap;
    if let Some(sub) = subtitle {
        let muted = cx.theme.color(ColorRole::Muted);
        painter.galley(egui::pos2(text.min.x, y), line(sub, sub_font, muted), muted);
    }
    // **Put the cursor back below the whole band.** The actions live in a child whose rect stops at
    // the title's line, and a scope advances its parent by what the child used — so a header with
    // no actions left the cursor *above* the subtitle, and the next thing drawn landed on it. It
    // showed as a subtitle with its descenders sliced off by the card below.
    ui.advance_cursor_after_rect(band);
    ui.add_space(gap);
    out
}

/// A subheading above a card. Distinguished by **a muted colour, small text and letter-spacing**
/// rather than by weight — so that the hierarchy does not collapse when a manufacturer changing the
/// typeface (guide 09 §3) puts in a font with no weight axis.
///
/// # Why it is not the accent colour
///
/// It used to be [`ColorRole::Primary`], which meant every subheading on every settings page was
/// painted in the accent. An accent that marks all the furniture marks nothing: with "Networks",
/// "Display" and "About" all in the same blue, the one thing on the page that *is* selected had no
/// colour left to say so. The accent now belongs to state — selection, press, focus — and a
/// subheading is quiet furniture, set apart by tracking instead.
pub fn section(ui: &mut Ui, cx: &Cx<'_>, text: &str) {
    let m = &cx.theme.metrics;
    // `content_inset` — see `title`. A subheading belongs to the card below it and has to line up
    // with that card's rows, not sit its own distance in.
    let size = m.type_scale.small;
    // **Its own text tall, not a control tall.** Laid out in `ui.horizontal`, the line took egui's
    // `interact_size` as its height — one touch target — and the heading floated in a band as
    // tall as a row, as far from its card as from the one above it. A `Frame` indents it without
    // that floor.
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: round_i8(m.content_inset),
            right: 0,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(size)
                    // Tracking scales with the text, so it stays proportional at any density.
                    .extra_letter_spacing(size * SECTION_TRACKING)
                    .color(cx.theme.color(ColorRole::Muted)),
            );
        });
    ui.add_space(m.row_height * SECTION_GAP);
}

/// The explanation below a card. One line on what the group does — pushed into a row's subtitle it would thicken the list.
pub fn note(ui: &mut Ui, cx: &Cx<'_>, text: &str) {
    let m = &cx.theme.metrics;
    let pad = round_i8(m.content_inset);
    ui.add_space(m.row_height * NOTE_GAP);
    // **The padding is given through a `Frame`.** Pushed with `add_space` inside a `horizontal_wrapped`,
    // **only the first line** is indented and the wrapped lines cling to the left edge — an explanation
    // running over two lines came out stepped. A `Frame` narrows the wrap width with it.
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: pad,
            right: pad,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_max_width(m.type_scale.small * NOTE_MEASURE);
            ui.label(
                egui::RichText::new(text)
                    .size(m.type_scale.small)
                    .color(cx.theme.color(ColorRole::Muted)),
            );
        });
    // The same gap a card leaves under itself, so the next section's heading does not sit on the
    // explanation's last line.
    ui.add_space(m.row_height * CARD_GAP);
}

/// The hairline between two rows **inside** a card.
///
/// # Why the card's edge is not enough
///
/// The rule used to be that the card's own boundary expresses the group and a line inside it makes
/// one card look divided again. Measured against the shells this one stands beside, that is wrong:
/// iOS, One UI and Windows 11 all rule between the rows of a grouped list, because three rows with
/// nothing between them read as one block of text rather than as three things you can press.
///
/// # Why it is geometric and holds no state
///
/// It is drawn **above** each row but the first, which is what keeps the last row from leaving a
/// stray line inside the card's bottom padding — the reason the old `ListRow` separator had to rely
/// on the container clipping it. "But the first" is read off the cursor: the first thing placed in
/// a container sits exactly on that container's content top, so a row whose cursor is still there
/// is the first one. A counter in the `Ui`'s temp data was tried first and drew a line above the
/// first row anyway; a rule that needs no bookkeeping cannot get the bookkeeping wrong.
///
/// It starts where the row's text does ([`crate::theme::ListRowMetrics::pad`]) and runs to the
/// card's edge, which is how iOS insets it — the icon column stays open.
fn card_divider(ui: &mut Ui, cx: &Cx<'_>) {
    let y = ui.cursor().min.y;
    if y <= ui.max_rect().min.y + 0.5 {
        return;
    }
    let pad = cx.theme.components.list_row.pad;
    let rect = ui.max_rect();
    ui.painter().hline(
        rect.min.x + pad..=rect.max.x,
        y,
        egui::Stroke::new(1.0, cx.theme.color(ColorRole::Outline)),
    );
}

/// Group a few rows into a **rounded card**. The space between cards divides the groups.
///
/// No separators are drawn on the rows inside — the card's boundary already expresses the group, and
/// a line on top of that makes one card look divided again.
pub fn group(ui: &mut Ui, cx: &mut Cx<'_>, body: impl FnOnce(&mut Ui, &mut Cx<'_>)) {
    group_with(ui, cx, Deco::new(), body);
}

/// Inject colours, corners and padding into [`group`].
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout::{group_with, info_row, Deco};
/// use fairing::ColorRole;
///
/// // The warning group is set apart by its border.
/// group_with(
///     ui,
///     cx,
///     Deco::new().stroke(1.0, ColorRole::Warning).radius(8.0),
///     |ui, cx| {
///         info_row(ui, cx, "Pressure", "Critical");
///     },
/// );
/// # }
/// ```
pub fn group_with(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    deco: Deco,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
) {
    let m = &cx.theme.metrics;
    let inset = m.screen_inset;
    let gap = m.row_height * CARD_GAP;
    let pad = deco.padding.unwrap_or(m.card_pad);
    // Why the default background is **`SurfaceVariant`**: a screen Pane's default background is already
    // `Surface` (`workspace::draw_pane`), so with the cards at `Surface` too the colours match and the
    // group disappears whole — which is how it was built at first, and measuring pixels caught it.
    // On a panel that is `SurfaceVariant` itself the card takes `Surface` — see `Container`.
    let fill = deco.fill_of(ui, cx);
    let radius = deco.radius_of(cx);
    // `visual_inset` narrows **the card only** — the place stays and the drawing is inset, making a
    // "floating" card. To shrink the place itself, `screen_inset` is the one.
    let vi = deco.visual_inset_of();
    // **The shadow is reserved before the frame and set after it.** A card's rect is not known
    // until `Frame::show` returns, and a shape added afterwards would paint *over* the card rather
    // than under it. `Shape::Noop` holds the place in the paint order; this is the pattern egui's
    // own `Frame` uses for its background.
    // The margins the card is drawn inside — the shadow, the rim and the stroke follow the
    // drawing, not the place: drawn on the place, the rim stood `screen_inset` clear of the
    // fill on both sides and hugged it top and bottom.
    let outer = egui::Margin::symmetric(
        round_i8(if deco.kind == Container::Divided {
            vi
        } else {
            inset + vi
        }),
        round_i8(vi),
    );
    let under = ui.painter().add(egui::Shape::Noop);
    let level = deco.elevation_of(cx.theme.card_elevation());
    let response = egui::Frame::NONE
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(radius))
        // **`Divided` is full-bleed.** Its rule closes a group rather than boxing it, and a rule
        // that stops short of the screen's edge reads as the bottom of a card that forgot to draw
        // the rest of itself — see `Container`.
        .outer_margin(outer)
        // Breathing room above and below the card. The first and last rows against the corners make the rounding look cut.
        .inner_margin(egui::Margin::symmetric(
            round_i8(deco.pad_x.unwrap_or(if deco.pad_content {
                m.content_inset
            } else {
                0.0
            })),
            round_i8(pad),
        ))
        .show(ui, |ui| {
            // **A card takes the width it is given; it does not grow to fit its content.**
            //
            // egui's default is the other way round. A child wider than the space it was handed
            // expands the parent's `max_rect` through `advance_cursor_after_rect`, and because a
            // card usually sits in a column, that widened column is then inherited by **every
            // later card in it**. Measured on the palette sheet: one segmented control 65 px too
            // wide for its half-width column pushed the cards below it 65, 65, 130 and 245 px past
            // the panel's right edge, where the screen clipped their trailing text away. The
            // control that did not fit was the fourth-most visible symptom of its own defect.
            //
            // So the cursor is advanced by a rect pinned to the card's own bounds — the same
            // number on both sides, which is also what the old `set_width(available_width())` was
            // for. A child that will not fit now bleeds over its own card's edge, where it is
            // obvious and local, instead of moving its neighbours.
            //
            // It is **not clipped**: a [`Dropdown`](crate::widgets::Dropdown) opens its list as a
            // floating layer bounded by `ui.clip_rect()`, so narrowing the clip here would shut
            // the list inside the card that opened it.
            // The body runs in a **child** `Ui`, not this one: `new_child` reports nothing back on
            // its own (unlike `scope`), so an over-wide grandchild expands the child's `min_rect`
            // and stops there. The cursor is then advanced by hand, by a rect pinned to the card's
            // own bounds on both sides — which is also the job the old `set_width` was doing.
            let bounds = ui.max_rect();
            let mut inner =
                ui.new_child(egui::UiBuilder::new().max_rect(bounds).layout(*ui.layout()));
            body(&mut inner, cx);
            let used = inner.min_rect().max.y;
            ui.advance_cursor_after_rect(egui::Rect::from_min_max(
                bounds.min,
                egui::pos2(bounds.max.x, used),
            ));
        });
    let rect = response.response.rect - outer;
    let corner = egui::CornerRadius::same(radius);
    let ink = cx.theme.elevate(level);
    if ink.shadow != egui::epaint::Shadow::NONE {
        ui.painter().set(under, ink.shadow.as_shape(rect, corner));
    }
    if ink.rim != egui::Stroke::NONE {
        ui.painter()
            .rect_stroke(rect, corner, ink.rim, egui::StrokeKind::Inside);
    }
    deco.paint_stroke(ui, cx, rect);
    ui.add_space(gap);
}

/// A row leading into a sub-screen (with a chevron on the right).
pub fn nav_row(ui: &mut Ui, cx: &mut Cx<'_>, title: &str, value: Option<&str>) -> Response {
    card_divider(ui, cx);
    let mut row = ListRow::new(title).chevron(true).separator(false);
    if let Some(value) = value {
        row = row.trailing(value);
    }
    row.show(ui, &mut cx.widgets())
}

/// **A full-bleed picture with a headline over it**, and page dots under it where there is more
/// than one.
///
/// Both reference kiosks open with one: a 16:9 band carrying a promotion, a line of text and a CTA,
/// with three or four dots at its foot. It is a [`layout`](self) function and not a widget because
/// it divides the screen — it takes the width it is given and the height its aspect asks for, and
/// hands the caller a `Ui` for whatever goes on top.
///
/// # Why the scrim is a gradient and not a role
///
/// Text over a photograph has **no guaranteed contrast** — the crate's own gate cannot help,
/// because the ground is the integrator's picture. The shell already answers this for the desktop's
/// wallpaper labels with [`LabelLegibility`](crate::desktop::LabelLegibility), and its middle
/// answer is the right one here: a scrim, laid as a gradient from transparent at the top to
/// [`Scrim`](fairing_widgets::theme::ColorRole::Scrim)'s own alpha at the foot, so the headline sits
/// on something dark and the top of the picture is untouched.
///
/// A flat plate over the whole band (`Veil`) is certain and kills the photograph, which is the
/// reason the band exists; a text shadow alone (`Shadow`) is not enough under a headline this size.
/// The gradient is the crate's own `LabelLegibility::Veil` made directional.
///
/// # Why the dots are the caller's count and the crate's drawing
///
/// Which page is showing, and how paging happens, is the caller's — a promotion carousel is timed
/// in one shop and swiped in another. What the crate settles is that the dots are there, that the
/// live one is [`Primary`](fairing_widgets::theme::ColorRole::Primary) and the rest are
/// `OnPrimary` at the `badge_alpha`, and that they sit inside the picture rather than under it, so
/// the band costs exactly its own height. Past twelve pages twelve dots are drawn, a window that
/// follows the live one.
pub fn hero<R>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    aspect: f32,
    pages: (usize, usize),
    body: impl FnOnce(&mut Ui, &mut Cx<'_>, egui::Rect) -> R,
) -> R {
    let m = &cx.theme.metrics;
    let width = ui.available_width();
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        HERO_ASPECT
    };
    let height = (width / aspect).max(m.touch_target);
    let (outer, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let inset = m.screen_inset;
    let rect = outer.shrink2(egui::vec2(inset, 0.0));
    let radius = egui::CornerRadius::same(round_u8(card_radius(m, &cx.theme.control)));

    // The band's own ground, for the frames before a picture has arrived.
    ui.painter()
        .rect_filled(rect, radius, cx.theme.color(ColorRole::SurfaceVariant));
    let out = body(ui, cx, rect);
    paint_scrim(ui, cx, rect);
    paint_dots(ui, cx, rect, pages);
    ui.add_space(m.row_height * CARD_GAP);
    out
}

/// The scrim: transparent at the top, `Scrim`'s own alpha at the foot.
///
/// A mesh rather than a stack of `rect_filled`s, because a ramp built from bands shows its bands on
/// an 8-bit panel — the same reason the elevation token's own doc gives for its pixel floors.
fn paint_scrim(ui: &Ui, cx: &Cx<'_>, rect: egui::Rect) {
    let base = cx.theme.color(ColorRole::Scrim);
    let clear = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 0);
    let mut mesh = egui::Mesh::default();
    let top = rect.top() + rect.height() * (1.0 - HERO_SCRIM);
    let band = egui::Rect::from_min_max(egui::pos2(rect.min.x, top), rect.max);
    for (pos, color) in [
        (band.left_top(), clear),
        (band.right_top(), clear),
        (band.left_bottom(), base),
        (band.right_bottom(), base),
    ] {
        mesh.colored_vertex(pos, color);
    }
    for v in &mut mesh.vertices {
        v.uv = egui::pos2(0.0, 0.0);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(2, 1, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// The page dots, inside the picture's foot.
fn paint_dots(ui: &Ui, cx: &Cx<'_>, rect: egui::Rect, (current, count): (usize, usize)) {
    const MAX_DOTS: usize = 12;
    if count < 2 {
        return;
    }
    // At most `MAX_DOTS` dots: past that, a window of them that keeps the live one in it.
    let first = current
        .saturating_sub(MAX_DOTS / 2)
        .min(count.saturating_sub(MAX_DOTS));
    let count = u16::try_from(count.min(MAX_DOTS)).unwrap_or(u16::MAX);
    let d = cx.theme.control.badge_h * cx.theme.control.badge_dot_ratio * 0.5;
    let gap = cx.theme.control.gap;
    let total = (d + gap).mul_add(f32::from(count) - 1.0, d);
    let y = rect.max.y - cx.theme.metrics.card_pad - d * 0.5;
    let mut x = rect.center().x - total * 0.5 + d * 0.5;
    let live = cx.theme.color(ColorRole::Primary);
    let waiting = cx
        .theme
        .color(ColorRole::OnPrimary)
        .gamma_multiply(cx.theme.control.badge_alpha);
    for slot in 0..count {
        let on = first + usize::from(slot) == current;
        ui.painter()
            .circle_filled(egui::pos2(x, y), d * 0.5, if on { live } else { waiting });
        x += d + gap;
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

/// **A strip of equal cells that switches what the whole screen shows.**
///
/// Draws at the cursor and takes `metrics.nav_bar_height`, so it sits against whichever edge the
/// caller puts it — under a [`page`] for a bottom bar, before one for a top bar. Returns the index
/// just picked, and only then. A label wider than its cell is cut with an ellipsis, and a glyph
/// wider than its cell shrinks to fit, so nothing runs over the next cell.
///
/// # Which of the three to reach for
///
/// The crate now has three ways to show a row of choices, and they are three different objects:
///
/// | | What it is | Shape |
/// |---|---|---|
/// | [`SegmentedControl`](crate::widgets::SegmentedControl) | A **setting** with a few values | One track, 2–4 equal cells, capped at `segment_max` |
/// | [`ChipRow`](crate::widgets::ChipRow) | A **filter** over a list | 5–8 separate objects, unequal, scrolls |
/// | `tab_bar` | **Navigation** — which screen you are on | N equal cells, full width, pinned to an edge, no track |
///
/// A tab bar is chrome and the other two are content. That is why this is not "a segmented control
/// with the cap lifted": a segmented control lives inside a card and edits a value, and lifting its
/// cap would give a *setting* eight 6 mm cells on the smallest panel, which is the reason the cap
/// is there.
///
/// # Why the active cell is not filled
///
/// The instrument-panel reference fills its active tab; the two consumer references colour the
/// glyph and the word and add a short bar. This crate takes the second, for the reason task #5
/// settled for the quick tiles: an accent that fills furniture has nothing left to say when
/// something is actually selected. The mark is **three channels** — the glyph and the label go to
/// `Primary`, and a bar sits against the bar's own inner edge — so it survives greyscale and it
/// does not flood.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// # let mut tab = 0;
/// use fairing::layout::{tab_bar, Tab};
/// use fairing::icon;
///
/// let tabs = [
///     Tab { label: "Home", icon: icon::HOME },
///     Tab { label: "Orders", icon: icon::LIST },
///     Tab { label: "Help", icon: icon::INFO },
/// ];
/// if let Some(picked) = tab_bar(ui, cx, "shop.tabs", &tabs, &mut tab) {
///     let _ = picked;
/// }
/// # }
/// ```
pub fn tab_bar(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    id: &str,
    tabs: &[Tab<'_>],
    selected: &mut usize,
) -> Option<usize> {
    if tabs.is_empty() {
        return None;
    }
    let m = &cx.theme.metrics;
    let (height, gap, icon) = (
        m.nav_bar_height,
        cx.theme.control.gap,
        cx.theme.control.icon,
    );
    let (bar, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(bar, 0, cx.theme.color(ColorRole::Surface));
    ui.painter().hline(
        bar.x_range(),
        bar.min.y,
        egui::Stroke::new(
            cx.theme.control.stroke_hairline,
            cx.theme.color(ColorRole::Outline),
        ),
    );

    let mut picked = None;
    // A bar with more cells than a `u16` can count is not a bar; the cap keeps the width
    // arithmetic in a range an `f32` represents exactly.
    let count = u16::try_from(tabs.len()).unwrap_or(u16::MAX);
    let cell_w = bar.width() / f32::from(count);
    for (slot, tab) in (0_u16..).zip(tabs.iter()) {
        let index = usize::from(slot);
        let cell = egui::Rect::from_min_size(
            egui::pos2(cell_w.mul_add(f32::from(slot), bar.min.x), bar.min.y),
            egui::vec2(cell_w, bar.height()),
        );
        let response = ui.interact(cell, ui.id().with((id, index)), egui::Sense::click());
        if response.clicked() && index != *selected {
            *selected = index;
            picked = Some(index);
        }
        let on = index == *selected;
        let ink = cx.theme.color(if on {
            ColorRole::Primary
        } else {
            ColorRole::Muted
        });
        // The label and the glyph stay inside their own cell. A label wider than the cell is cut
        // with an ellipsis, like every other one-line text in the crate, rather than running over
        // its neighbours; the glyph shrinks only when the cell is narrower than it.
        let room = gap.mul_add(-2.0, cell_w).max(1.0);
        let icon = icon.min(room);
        let font = egui::FontId::proportional(m.type_scale.small);
        let mut job = egui::text::LayoutJob::simple_singleline(tab.label.to_owned(), font, ink);
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let galley = ui.painter().layout_job(job);
        let block = icon + gap + galley.rect.height();
        let top = cell.center().y - block * 0.5;
        let at = egui::Rect::from_center_size(
            egui::pos2(cell.center().x, top + icon * 0.5),
            egui::Vec2::splat(icon),
        );
        let style = crate::icons::IconStyle {
            color: crate::icons::IconColor::Fixed(ink),
            ..crate::icons::IconStyle::default()
        };
        {
            let widgets = cx.widgets();
            widgets
                .icons
                .paint(ui.painter(), at, &tab.icon, &style, widgets.theme);
        }
        ui.painter().galley(
            egui::pos2(
                cell.center().x - galley.rect.width() * 0.5,
                top + icon + gap,
            ),
            galley,
            ink,
        );
        if on {
            // The third channel: a short bar on the strip's own inner edge, so a greyscale render
            // still says which cell is live.
            let w = cell_w * TAB_MARK;
            let h = cx.theme.control.stroke_mark;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(cell.center().x - w * 0.5, bar.min.y),
                    egui::vec2(w, h),
                ),
                egui::CornerRadius::same(round_u8(h * 0.5)),
                cx.theme.color(ColorRole::Primary),
            );
        }
    }
    picked
}

/// **A summary row heavier than the ones above it** — the last line of a bill, a cycle total, a
/// run count.
///
/// Every reference screen this was measured from draws the same three-part figure: two or three
/// quiet [`info_row`]s, a rule, and one loud row. This is the loud one.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout::{group, info_row, total_row};
///
/// group(ui, cx, |ui, cx| {
///     info_row(ui, cx, "Subtotal", "€ 34,00");
///     info_row(ui, cx, "Service charge", "€ 1,70");
///     total_row(ui, cx, "Total", "€ 35,70");
/// });
/// # }
/// ```
///
/// # Why the step is the type scale's own and not a new token
///
/// Measured off the three references, the total's text against the rows above it is 1.31 (Bella
/// Tavola), 1.38 (the `McDonald's` kiosk) and 1.72 (the Sichuan kiosk, whose figures are CJK and whose total
/// is deliberately enormous). `type_scale.heading / type_scale.body` is **1.375** — inside the
/// band the two Latin screens set, and already what the crate means by "one size up". A token of
/// its own would be a fourth number nobody could relate to the other three.
///
/// # Why the value is not the accent
///
/// All three references make the total the loudest text on the screen, and one paints it in its
/// brand red. `Primary` measures 3.81 on `surface` in base dark, under the 4.5 a word needs, so
/// **the accent is never a text colour** — the loudness here is size and weight, which is what the
/// other two references use and what survives a palette swap.
///
/// # Why it is not `info_row` with a flag
///
/// A total is not a row that happens to be bold: it carries a rule above it, a taller band and a
/// different type step. Folding three changes into a flag on the general row would give the
/// general row a mode.
pub fn total_row(ui: &mut Ui, cx: &mut Cx<'_>, title: &str, value: &str) -> Response {
    let m = &cx.theme.metrics;
    let height = m.row_height * TOTAL_ROW;
    let ink = cx.theme.color(ColorRole::OnSurface);
    let font = cx.theme.strong(m.type_scale.heading);
    let inset = m.content_inset;
    let gap = cx.theme.control.gap;

    card_divider(ui, cx);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    // The figure is laid out first and the title is given what is left: a long title must never
    // push the number off the row, because the number is the thing being read.
    let figure = painter.layout_no_wrap(value.to_owned(), font.clone(), ink);
    let room = 2.0f32
        .mul_add(-inset, rect.width() - figure.rect.width() - gap)
        .max(0.0);
    let label = painter.layout(title.to_owned(), font, ink, room);
    painter.galley(
        egui::pos2(
            rect.min.x + inset,
            rect.center().y - label.rect.height() * 0.5,
        ),
        label,
        ink,
    );
    painter.galley(
        egui::pos2(
            rect.max.x - inset - figure.rect.width(),
            rect.center().y - figure.rect.height() * 0.5,
        ),
        figure,
        ink,
    );
    response
}

/// **A row that opens in place**: a settings row for a header and, under it, a body
/// that unrolls when the row is tapped and rolls back when it is tapped again. The body is the
/// caller's — rows, a slider, a note — laid out one icon column in from the header's edge, so
/// its titles line up under the header's own. What is under the row moves down; nothing floats.
///
/// # The rules it keeps
///
/// - **The whole row opens it.** The chevron at the far right is the sign, not the target; it
///   points down while closed and up while open, and it is `⌄`, never `›`: `›` is a
///   [`nav_row`] that goes to another screen, `⌄` opens here.
/// - **A control in the header keeps its own patch.** With [`ExpandableRow::switch`] the row
///   carries a switch that a tap on *it* toggles, without opening the row — the one place the
///   crate's "anywhere on the row toggles" rule ([`switch_row`]) is turned round, because here
///   the row already means something else. The patch is the switch's drawn size, widened to a
///   finger.
/// - **The summary is for the closed row.** [`ExpandableRow::summary`] is shown at the right
///   while the row is closed, so it answers without being opened, and goes when it is open.
/// - **Open is the caller's state**, a `&mut bool` like a switch's: a body is content, not a
///   popup, and a screen wants to open one from code, remember it, or open only one at a time
///   ([`accordion`]). Closed by default — never the first one open on its own.
/// - **The body is revealed.** When the row opens near the bottom of the page, the page scrolls
///   just enough for the body's end to show; when the body is taller than the pane, the header
///   is put at the top instead, so it never leaves the screen. Off with
///   [`ExpandableRow::reveal`]`(false)`.
/// - **Two levels at most.** A body may hold another expandable row; past that the thing wanted
///   is a screen of its own, with a [`nav_row`] to it.
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
        let id = ui.id().with(self.id_salt);
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
                    egui::Stroke::new(1.0, cx.theme.color(ColorRole::Outline)),
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

/// Scroll the page so an opening body is seen: just enough for its end to show, or — where the
/// body is taller than the pane — the header to the top, so the header never leaves the screen.
/// Asked for every frame of the opening and once more when it has settled, against the body's
/// **whole** height, so the scroll lands where the body will end and not where it is this frame.
fn reveal_body(ui: &Ui, id: egui::Id, header: egui::Rect, out: &Unrolled) {
    let flag = id.with("reveal");
    if !ui.data(|d| d.get_temp::<bool>(flag).unwrap_or(false)) {
        return;
    }
    let pane = ui.clip_rect();
    let whole = egui::Rect::from_min_max(
        header.min,
        egui::pos2(header.max.x, out.rect.min.y + out.natural),
    );
    if whole.height() > pane.height() {
        ui.scroll_to_rect(header, Some(egui::Align::Min));
    } else {
        ui.scroll_to_rect(whole, None);
    }
    if out.settled {
        ui.data_mut(|d| d.remove_temp::<bool>(flag));
    }
}

/// **The "Advanced" row at the end of a group** (after Android's settings): one row
/// that stands for the rows a group keeps back, its subtitle naming them, and that goes away
/// when tapped as they unroll in its place. One way, for the visit: `revealed` is the caller's,
/// so a screen that wants them back hidden on its next visit resets it.
///
/// `title` is the row's own word — "Advanced", or the screen's language for it. `hidden` names
/// what is behind it, for the subtitle; empty, the row has none. Returns `true` on the frame the
/// row was tapped.
pub fn advanced_rows(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    id_salt: &str,
    title: &str,
    hidden: &[&str],
    revealed: &mut bool,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
) -> bool {
    let id = ui.id().with(id_salt);
    let mut tapped = false;
    if !*revealed {
        card_divider(ui, cx);
        let names = hidden.join(", ");
        let mut row = ListRow::new(title).disclosure(false).separator(false);
        if !names.is_empty() {
            row = row.subtitle(&names);
        }
        if row.show(ui, &mut cx.widgets()).clicked() {
            *revealed = true;
            tapped = true;
        }
    }
    if *revealed {
        // No indent: these are the group's own rows, not a body under a header.
        if let Some(band) = unroll(ui, &mut cx.widgets(), id, true, 0.0) {
            let _ = band.show(ui, |ui| body(ui, cx));
        }
    }
    tapped
}

/// **One open at a time.** Run one [`ExpandableRow`] (or anything with a `&mut bool`) as slot
/// `index` of a group whose open slot the caller keeps in `open`: opening this one closes the
/// one that was open, and closing it leaves none open. It is a helper and not a mode of the row
/// because auto-collapsing "takes control away from the user" (Windows' own words) and belongs
/// to a short panel, decided by the screen.
///
/// ```no_run
/// # fn body(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>, which: &mut Option<usize>) {
/// use fairing::layout::{self, ExpandableRow};
/// layout::accordion(which, 0, |open| {
///     ExpandableRow::new("net", "Network").show(ui, cx, open, |ui, cx| {
///         layout::info_row(ui, cx, "Wi-Fi", "fairing-lab");
///     });
/// });
/// # }
/// ```
pub fn accordion(open: &mut Option<usize>, index: usize, row: impl FnOnce(&mut bool)) {
    let mut this = *open == Some(index);
    row(&mut this);
    if this {
        *open = Some(index);
    } else if *open == Some(index) {
        *open = None;
    }
}

/// A read-only row. Neither a tap nor a chevron.
pub fn info_row(ui: &mut Ui, cx: &mut Cx<'_>, title: &str, value: &str) -> Response {
    card_divider(ui, cx);
    ListRow::new(title)
        .trailing(value)
        .chevron(false)
        .separator(false)
        .enabled(false)
        .show(ui, &mut cx.widgets())
}

/// A toggle row. `true` **only when the value changed**.
///
/// Pressing anywhere on the row toggles it — with only the switch as the touch target, a gloved hand
/// cannot hit it.
pub fn switch_row(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    subtitle: Option<&str>,
    on: &mut bool,
    enabled: bool,
) -> bool {
    card_divider(ui, cx);
    // **A composed `ListRow`, not a second copy of a row.** This function used to draw its own
    // background, its own title and its own subtitle with `painter.text` at `screen_inset`, because
    // `ListRow` had no trailing-widget slot. The cost was measured: a switch row's label sat four du
    // left of a nav row's label in the same card — five pixels on a real render — and a fix to
    // `ListRow` reached none of the built-in settings screens. The slot (`ListRow::trailing_switch`)
    // is a closed set of the crate's own controls. A slot for an *arbitrary* widget stays refused:
    // that is where screen content, which belongs to the integrator, would start entering the crate.
    //
    // The row owns the tap: pressing anywhere on it toggles, because with only the switch live a
    // gloved hand cannot hit it.
    let mut row = ListRow::new(title)
        .trailing_switch(*on)
        .chevron(false)
        .separator(false)
        .enabled(enabled);
    if let Some(subtitle) = subtitle {
        row = row.subtitle(subtitle);
    }
    let response = row.show(ui, &mut cx.widgets());
    if response.clicked() {
        *on = !*on;
        return true;
    }
    false
}

/// A column that takes **exactly** the width and height asked for. `Ui::allocate_ui_with_layout`
/// advances the cursor only by the content's size, so content narrower than requested pulls the next
/// column forward — where a layout is built out of column widths, that is a collapse of the
/// alignment. This one consumes the place it asked for as it stands.
fn column(ui: &mut Ui, width: f32, height: f32, align: egui::Align, add: impl FnOnce(&mut Ui)) {
    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width.max(0.0), height));
    let layout = match align {
        egui::Align::Max => egui::Layout::right_to_left(egui::Align::Center),
        _ => egui::Layout::left_to_right(egui::Align::Center),
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    child.set_clip_rect(child.clip_rect().intersect(rect));
    add(&mut child);
    ui.advance_cursor_after_rect(rect);
}

/// A slider row. `true` if the value changed.
///
/// **The layout divides by width.** With more than 240 du left for the track, the title, the track
/// and the value are laid along one line; with less, the track drops to a line below the title.
/// Always stacking two lines where one would do makes the card more than twice as thick as a switch
/// row — for one and the same item. Forcing one line on a narrow screen, conversely, shortens the
/// track until a finger cannot adjust it finely.
///
/// Whichever layout it is, the track's **hit height is `touch_target`**. What was reduced for density
/// is the empty padding above and below the track, not the width a finger touches (the glove
/// policy).
pub fn slider_row(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    enabled: bool,
) -> bool {
    card_divider(ui, cx);
    let m = cx.theme.metrics.row_height;
    let touch = cx.theme.metrics.touch_target;
    // The row's content inset, the same one `ListRow` uses. It was `screen_inset` (12 du) against
    // `ListRow`'s 16, which is the four-du gap that showed as five pixels between a slider row's
    // label and a nav row's in the same card.
    let inset = cx.theme.metrics.content_inset;
    let text = if enabled {
        ColorRole::OnSurface
    } else {
        ColorRole::Muted
    };
    let label = |cx: &Cx<'_>| {
        egui::RichText::new(title)
            .size(cx.theme.metrics.type_scale.body)
            .color(cx.theme.color(text))
    };
    let reading = |cx: &Cx<'_>, v: f32| {
        egui::RichText::new(format!("{}{suffix}", rounded(v)))
            .size(cx.theme.metrics.type_scale.body)
            .color(cx.theme.color(ColorRole::Primary))
    };

    let inner = (ui.available_width() - inset * 2.0).max(m);
    let gap = m * SLIDER_COL_GAP;
    let value_w = m * SLIDER_VALUE_WIDTH;
    let label_w = (inner * SLIDER_LABEL_FRACTION).min(m * SLIDER_LABEL_MAX);
    let track_w = inner - label_w - value_w - gap * 2.0;
    let mut changed = false;

    // ── The one-line layout ──
    if track_w >= SLIDER_INLINE_TRACK {
        // **One rect, placed like a `ListRow`'s.** This used to be `add_space` · `horizontal` ·
        // `add_space`, which leaves the row's height to egui - and egui inserts `item_spacing.y`
        // (11.8 du here) between each of those three, on top of the card's own margins. A `ListRow`
        // in the same card never asks: it allocates one rect and puts everything on
        // `rect.center().y`. The two disagreed by 6.5 px, measured, with the slider riding high.
        let row_h = touch + m * SLIDER_PAD * 2.0;
        let (band, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_h),
            egui::Sense::hover(),
        );
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_center_size(
                    band.center(),
                    egui::vec2(band.width(), touch),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        {
            let ui = &mut ui;
            // The column gap is already set aside by the arithmetic below. With egui's default spacing on
            // top, the three columns exceed the card's width and the value is pushed off to the right.
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(inset);
            column(ui, label_w, touch, egui::Align::Min, |ui| {
                ui.add(egui::Label::new(label(cx)).truncate());
            });
            ui.add_space(gap);
            column(ui, track_w, touch, egui::Align::Min, |ui| {
                changed = TouchSlider::new(value, range)
                    .enabled(enabled)
                    .show(ui, &mut cx.widgets())
                    .changed();
            });
            ui.add_space(gap);
            column(ui, value_w, touch, egui::Align::Max, |ui| {
                ui.label(reading(cx, *value));
            });
            ui.add_space(inset);
        }
        return changed;
    }

    // ── The two-line layout ──
    //
    // The same discipline as the one-line branch: this stack is bracketed by one pad on each side
    // and egui's `item_spacing.y` is taken out of the vertical, so the card's own margins are the
    // only thing deciding where the block sits.
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(m * SLIDER_PAD);
    ui.horizontal(|ui| {
        ui.add_space(inset);
        ui.label(label(cx));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(inset);
            ui.label(reading(cx, *value));
        });
    });
    // The title and the track are **one lump**; with `item_spacing.y` zeroed above there is nothing
    // to take back out between them, which is what the negative nudge here used to be for.
    ui.horizontal(|ui| {
        ui.add_space(inset);
        let width = (ui.available_width() - inset).max(m);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_size(
                    ui.cursor().min,
                    egui::vec2(width, touch),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        changed = TouchSlider::new(value, range)
            .enabled(enabled)
            .show(&mut child, &mut cx.widgets())
            .changed();
        ui.advance_cursor_after_rect(child.min_rect());
    });
    ui.add_space(m * SLIDER_PAD);
    changed
}

/// An exclusive choice list. When the selected item's index changes, it hands that index back.
///
/// It does not use a dropdown — a popup means aiming twice on touch, and a device screen usually has
/// the vertical room to lay the list out as it is.
pub fn choice_rows(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    options: &[(&str, Option<&str>)],
    selected: usize,
) -> Option<usize> {
    let mut picked = None;
    for (index, (label, subtitle)) in options.iter().enumerate() {
        card_divider(ui, cx);
        let mut row = ListRow::new(*label).chevron(false).separator(false);
        if let Some(subtitle) = subtitle {
            row = row.subtitle(*subtitle);
        }
        // **A radio on every row, not a tick on one.** A tick marks the chosen row and leaves the
        // others blank, so nothing on an unchosen row says it can be chosen at all; a radio group
        // is what a pick-one list is. The crate ships the control - this is the screen that was
        // still drawing its own mark instead of using it.
        row = row.trailing_radio(index == selected);
        if row.show(ui, &mut cx.widgets()).clicked() && index != selected {
            picked = Some(index);
        }
    }
    picked
}

/// The status card at the top of a screen. It gives what is going on at a glance.
///
/// A `tone` of `None` is a neutral card; given one, that role's colour is laid down faintly
/// (connected = `Primary`, failed = `Danger`).
pub fn status_card(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    detail: Option<&str>,
    tone: Option<ColorRole>,
) {
    let m = &cx.theme.metrics;
    let height = m.row_height * if detail.is_some() { 1.45 } else { 1.1 };
    // The title in body, the detail in small — type sizes are the scale's, never a share of the
    // row (a gloved row made the words half as big again). The two lines sit either side of the
    // card's centre, a line gap apart.
    let (title_size, detail_size) = (m.type_scale.body, m.type_scale.small);
    let line_gap = cx.theme.control.line_gap;
    // As in `slider_row`: a card's content starts where a row's content starts.
    let inset = m.content_inset;
    let radius = card_radius(m, &cx.theme.control);
    let gap = m.row_height * CARD_GAP;
    let (outer, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height + gap),
        egui::Sense::hover(),
    );
    let rect = egui::Rect::from_min_max(
        egui::pos2(outer.left() + inset, outer.top()),
        egui::pos2(outer.right() - inset, outer.top() + height),
    );
    // A neutral card is `SurfaceVariant` — in the dark palette `Surface` is nearly `Background` and the
    // card's boundary disappears (measured on Abyss).
    let base = cx.theme.color(ColorRole::SurfaceVariant);
    let fill = tone.map_or(base, |role| base.lerp_to_gamma(cx.theme.color(role), 0.22));
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(round_u8(radius)), fill);
    let title_y = if detail.is_some() {
        rect.center().y - f32::midpoint(detail_size, line_gap)
    } else {
        rect.center().y
    };
    painter.text(
        egui::pos2(rect.left() + inset, title_y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(title_size),
        cx.theme.color(ColorRole::OnSurface),
    );
    if let Some(detail) = detail {
        painter.text(
            egui::pos2(
                rect.left() + inset,
                rect.center().y + f32::midpoint(title_size, line_gap),
            ),
            egui::Align2::LEFT_CENTER,
            detail,
            egui::FontId::proportional(detail_size),
            cx.theme.color(ColorRole::Muted),
        );
    }
}

/// A **pressable** row with an icon. The icon's colour is a role, so it follows the palette.
///
/// A chevron is always attached — this row means "press it and something happens", and every caller
/// in the built-in settings screens looks at `.clicked()`. A read-only row is [`info_row`], and a row
/// that needs an icon but cannot be pressed can use [`ListRow`] directly — a chevron failing to say
/// "pressing here does nothing" is worse.
pub fn icon_row(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    icon: IconRef,
    title: &str,
    subtitle: Option<&str>,
    trailing: Option<&str>,
) -> Response {
    card_divider(ui, cx);
    let mut row = ListRow::new(title)
        .icon(icon)
        .icon_color(ColorRole::Primary)
        .separator(false);
    if let Some(subtitle) = subtitle {
        row = row.subtitle(subtitle);
    }
    if let Some(trailing) = trailing {
        row = row.trailing(trailing);
    }
    row.show(ui, &mut cx.widgets())
}

/// [`icon_row`] that times a long press as well — for a list where holding a row means something
/// (forgetting a saved network, a context menu). See [`ListRow::show_with_long_press`] for why the
/// row times its own press rather than asking egui.
pub fn icon_row_with_long_press(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    icon: IconRef,
    title: &str,
    subtitle: Option<&str>,
    trailing: Option<&str>,
) -> crate::widgets::RowPress {
    card_divider(ui, cx);
    let mut row = ListRow::new(title)
        .icon(icon)
        .icon_color(ColorRole::Primary)
        .separator(false);
    if let Some(subtitle) = subtitle {
        row = row.subtitle(subtitle);
    }
    if let Some(trailing) = trailing {
        row = row.trailing(trailing);
    }
    row.show_with_long_press(ui, &mut cx.widgets())
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

/// **How a list marks the entry that is open.**
///
/// A rail's selection is the one thing on the screen that says *where you are*, and which mark
/// reads best is a property of the panel rather than of the crate. A console under bench lighting
/// wants the pill and the bar together; a dense desktop list wants the bar alone, because a pill
/// behind every second row turns the column into stripes; a screen whose rail is already an accent
/// colour wants neither and lets the label carry it.
///
/// [`Self::PillAndBar`] is the default because it is the only one of the four that survives being
/// looked at from two metres away and at an angle, which is the instrument case the crate is for.
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

/// Which of the two marks a [`SelectMark`] draws: `(pill, bar)`.
const fn mark_wants(mark: SelectMark) -> (bool, bool) {
    match mark {
        SelectMark::PillAndBar => (true, true),
        SelectMark::Pill => (true, false),
        SelectMark::Bar => (false, true),
        SelectMark::Tint => (false, false),
    }
}

/// Where the pill and the bar go for a row occupying `rect` — shared by the open marks and the
/// fold, so the fold starts from exactly the shapes the open row drew.
struct MarkGeometry {
    pill: egui::Rect,
    pill_radius: f32,
    bar: egui::Rect,
    bar_w: f32,
    inset: f32,
}

fn mark_geometry(cx: &Cx<'_>, rect: egui::Rect) -> MarkGeometry {
    let inset = cx.theme.metrics.screen_inset * 0.5;
    let bar_w = cx.theme.control.accent_bar;
    // **The bar sits beside the pill, not inside it.**
    //
    // It was inside, and then it had to dodge the pill's corner arc, which is what an earlier
    // version spent its arithmetic on and why a short row left it a stub floating in the fill.
    // Outside there is no arc to dodge: the bar is a mark in the leading gutter and the pill
    // begins after it, which is both what the console reference draws and the simpler shape to be
    // right about.
    //
    // The gutter is reserved whether or not a bar is drawn, so an unselected row, a `Pill` and a
    // `PillAndBar` all start their fill at the same x. Sizing the pill off whether it happens to
    // have a bar would shift the whole column sideways as the selection moved.
    let gutter = bar_w * BAR_GUTTER;
    let pill = egui::Rect::from_min_max(
        egui::pos2(rect.left() + inset + gutter, rect.top()),
        egui::pos2(rect.right() - inset, rect.bottom()),
    );
    // A share of the pill's height, so the mark cannot fade out as the row and the corner radius
    // move apart — a height taken off the radius once shrank to zero that way, and not depending
    // on the radius keeps that fixed.
    let half = pill.height() * 0.5 * BAR_SHARE;
    let bar = egui::Rect::from_center_size(
        egui::pos2(rect.left() + inset + bar_w * 0.5, rect.center().y),
        egui::vec2(bar_w, half * 2.0),
    );
    MarkGeometry {
        pill,
        pill_radius: cx.theme.metrics.control_radius.min(pill.height() * 0.5),
        bar,
        bar_w,
        inset,
    }
}

/// The selection mark's shapes for a row occupying `rect`, back to front.
///
/// Shapes rather than paint calls, because the caller does not know `rect` until the row has been
/// drawn: `ListRow` floors its own height against the touch target and against its text, so the
/// box it allocates is regularly taller than `metrics.row_height`. Painting the mark first, from
/// that token, is what put the pill 6 du above the label it was meant to be behind. The
/// caller reserves two slots, draws the row, then fills the slots in from the rect the row
/// actually took.
///
/// [`SelectMark::Tint`] returns neither: the icon and the label carry that one.
fn selection_shapes(
    cx: &Cx<'_>,
    rect: egui::Rect,
    mark: SelectMark,
) -> (Option<egui::Shape>, Option<egui::Shape>) {
    let (pill_wanted, bar_wanted) = mark_wants(mark);
    let g = mark_geometry(cx, rect);
    // **Translucent, not a solid plate**. An opaque `SurfaceVariant` pill reads as a card
    // that happens to be under the words; the accent at `fill_alpha` reads as the row being *lit*,
    // which is what a console rail does and what leaves the label legible on top.
    let fill = pill_wanted.then(|| {
        egui::Shape::rect_filled(
            g.pill,
            egui::CornerRadius::same(round_u8(g.pill_radius)),
            cx.theme
                .color(ColorRole::Primary)
                .gamma_multiply(cx.theme.control.fill_alpha),
        )
    });
    let bar = bar_wanted.then(|| {
        egui::Shape::rect_filled(
            g.bar,
            egui::CornerRadius::same(round_u8(g.bar_w * 0.5)),
            cx.theme.color(ColorRole::Primary),
        )
    });
    (fill, bar)
}

/// The marks partway through a fold — `k` runs from 0 (open) to 1 (folded) — for a row occupying
/// `rect` whose icon is centred at `icon_at`. Back to front, like [`selection_shapes`].
///
/// The pill fades out. The bar **melts into a disc** behind the icon: its rect, its corner and its
/// ink all tween from the bar's to the disc's, so a solid line beside the words becomes a soft
/// tinted circle behind the glyph in one motion rather than one mark being swapped for another.
/// A `Pill` with no bar melts the pill instead; `Tint` has nothing to melt.
///
/// The disc is a step darker than the open pill (`DISC_TINT` × `fill_alpha`): folded, it is the
/// only thing saying which entry is live, and the pill's wash was sized to sit under a label.
fn folding_marks(
    cx: &Cx<'_>,
    rect: egui::Rect,
    mark: SelectMark,
    k: f32,
    icon_at: egui::Pos2,
    band: (f32, f32),
) -> (Option<egui::Shape>, Option<egui::Shape>) {
    let (pill_wanted, bar_wanted) = mark_wants(mark);
    let g = mark_geometry(cx, rect);
    let primary = cx.theme.color(ColorRole::Primary);
    let fill_alpha = cx.theme.control.fill_alpha;
    let side = cx.theme.metrics.icon_size * 0.5;
    // As big as the band allows behind the icon, and never past the row's own inset from it.
    let room = (icon_at.x - band.0 - g.inset).min(band.1 - g.inset - icon_at.x) * 2.0;
    let d = (side * DISC_RATIO)
        .min(room)
        .min(rect.height() - 4.0)
        .max(0.0);
    let disc = egui::Rect::from_center_size(icon_at, egui::Vec2::splat(d));
    let disc_alpha = (fill_alpha * DISC_TINT).min(1.0);
    let pill = (pill_wanted && k < 1.0).then(|| {
        egui::Shape::rect_filled(
            g.pill,
            egui::CornerRadius::same(round_u8(g.pill_radius)),
            primary.gamma_multiply(fill_alpha * (1.0 - k)),
        )
    });
    let (from, from_radius, from_alpha) = if bar_wanted {
        (g.bar, g.bar_w * 0.5, 1.0)
    } else if pill_wanted {
        (g.pill, g.pill_radius, fill_alpha)
    } else {
        return (pill, None);
    };
    let melt = egui::Shape::rect_filled(
        from.lerp_towards(&disc, k),
        egui::CornerRadius::same(round_u8(egui::lerp(from_radius..=d * 0.5, k))),
        primary.gamma_multiply(egui::lerp(from_alpha..=disc_alpha, k)),
    );
    (pill, Some(melt))
}

/// A rail entry partway through a fold, folded (`k` = 1), or on an arm too narrow for words.
///
/// It draws rather than delegating to `ListRow` because a row lays its icon out against a *label
/// column* — and here the column is going: the icon slides from where the row had it to the
/// middle of the arm, the words fade over the first stretch of the fold, and the mark melts into
/// a disc behind the live icon ([`folding_marks`]). At `k` = 0 the icon and the words are exactly
/// where `ListRow` drew them, so the first frame of a fold is the row's own last frame.
fn folding_item(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    mark: SelectMark,
    icon: &IconRef,
    title: &str,
    selected: bool,
    fold: FoldFrame,
) -> Response {
    let k = fold.k;
    let m = &cx.theme.metrics;
    let height = m.row_height.max(m.touch_target);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let side = m.icon_size * 0.5;
    // Where `ListRow` centres the icon, and where the folded band does; in between, the tween.
    // With no band to go by - a rail that is simply narrow - the row is the band.
    let band = fold.band.unwrap_or((rect.left(), rect.right()));
    let row_x = rect.left() + m.content_inset + side * 0.5;
    let fold_x = f32::midpoint(band.0, band.1);
    let icon_at = egui::pos2(egui::lerp(row_x..=fold_x, k), rect.center().y);
    // Painted out to the band's edges, not the row's clip. The rows of a rail sit in a scroll
    // area whose clip starts where the arm does, a screen inset in from the glass, while the
    // disc is centred in the band that runs from the glass - on a panel with large glyphs it
    // reaches into that inset and was cut there (user report). The clip keeps its top and
    // bottom: rows scrolled out of the view stay out of it.
    let clip = ui.clip_rect();
    // `set_clip_rect`, not `with_clip_rect`: the latter only ever narrows.
    let mut painter = ui.painter().clone();
    painter.set_clip_rect(egui::Rect::from_min_max(
        egui::pos2(band.0.min(clip.left()), clip.top()),
        egui::pos2(band.1.max(clip.right()), clip.bottom()),
    ));
    if selected {
        let (pill, melt) = folding_marks(cx, rect, mark, k, icon_at, band);
        for shape in [pill, melt].into_iter().flatten() {
            painter.add(shape);
        }
    }
    let role = if selected {
        ColorRole::Primary
    } else {
        ColorRole::Muted
    };
    let style = crate::icons::IconStyle::sized(side).color(crate::icons::IconColor::Role(role));
    cx.icons.paint(
        &painter,
        egui::Rect::from_center_size(icon_at, egui::Vec2::splat(side)),
        icon,
        &style,
        cx.theme,
    );
    // The words, where the row had them, fading over the first stretch of the fold.
    let fade = 1.0 - (k / LABEL_FADE).min(1.0);
    if fade > 0.0 {
        let x = rect.left() + m.content_inset + side + cx.theme.components.list_row.pad;
        let ink = cx
            .theme
            .color(if selected {
                ColorRole::Primary
            } else {
                ColorRole::OnSurface
            })
            .gamma_multiply(fade);
        painter.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            title,
            egui::TextStyle::Body.resolve(ui.style()),
            ink,
        );
    }
    response
}

/// One entry of a left/right split's list, marked with [`SelectMark`]'s default.
///
/// [`list_item_with`] picks a different mark.
pub fn list_item(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    icon: IconRef,
    title: &str,
    selected: bool,
) -> Response {
    list_item_with(ui, cx, SelectMark::default(), icon, title, selected)
}

/// One entry of a left/right split's list, marked the way `mark` says.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::{icon, layout::{self, SelectMark}};
/// // A dense list: the bar alone, so a column of rows does not read as stripes.
/// let _ = layout::list_item_with(ui, cx, SelectMark::Bar, icon::HOME, "Home", true);
/// # }
/// ```
pub fn list_item_with(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    mark: SelectMark,
    icon: IconRef,
    title: &str,
    selected: bool,
) -> Response {
    // **A folding arm draws the fold, and a narrow arm drops the label by itself.** A folded rail
    // is still a rail: the icons and the selection mark have to stay readable, and only the words
    // have anywhere to go. Reading the fold (`Rail::show` leaves it under `FOLD_KEY` while its
    // rail closure runs) and the width here, rather than taking a `collapsed` flag, means a rail
    // built out of `list_item` calls needs no branch in the caller — and a rail that is simply
    // narrow gets the folded drawing, which is the same thing seen from the other side.
    let fold = ui
        .ctx()
        .data(|d| d.get_temp::<FoldFrame>(egui::Id::new(FOLD_KEY)));
    if let Some(f) = fold.filter(|f| f.k > 0.0) {
        return folding_item(ui, cx, mark, &icon, title, selected, f);
    }
    if ui.available_width() < cx.theme.metrics.icon_cell {
        let narrow = FoldFrame { k: 1.0, band: None };
        return folding_item(ui, cx, mark, &icon, title, selected, narrow);
    }
    // Two slots held open under the row, filled in once the row has told us how tall it is.
    let slots = selected.then(|| {
        [
            ui.painter().add(egui::Shape::Noop),
            ui.painter().add(egui::Shape::Noop),
        ]
    });
    let response = ListRow::new(title)
        .icon(icon)
        // **The selected row carries the accent; the rest are quiet.** A rail is a list of places,
        // and the one you are in is the only thing on it with a state — so it takes the colour and
        // the others take `Muted`, rather than every glyph being accented and the selection having
        // to find some other way to stand out.
        .icon_color(if selected {
            ColorRole::Primary
        } else {
            ColorRole::Muted
        })
        .title_role(if selected {
            ColorRole::Primary
        } else {
            ColorRole::OnSurface
        })
        // **No badge.** An earlier version put one behind every sidebar glyph, following One UI's
        // settings sidebar. An instrument rail is not that: its icons are bare line marks, and a
        // tinted disc behind each one competes with the pill that is already marking the selection.
        .icon_badge(false)
        // Nor bold. The accent says which row it is, and weight on top of colour says it twice.
        .strong(false)
        .chevron(false)
        .separator(false)
        .show(ui, &mut cx.widgets());
    if let Some([pill_slot, bar_slot]) = slots {
        let (pill, bar) = selection_shapes(cx, response.rect, mark);
        if let Some(pill) = pill {
            ui.painter().set(pill_slot, pill);
        }
        if let Some(bar) = bar {
            ui.painter().set(bar_slot, bar);
        }
    }
    response
}

/// **Text shrunk to fit a width** — for places with only a `Painter` to hand, such as inside a grid
/// cell.
///
/// `egui::Label::truncate` requires a `Ui`. But when a [`Grid`] callback draws inside a cell itself,
/// all it has is a `Painter`, and long text there simply intrudes on the neighbouring cell. The
/// reason it **shrinks** rather than truncating (`…`) is that labels on a device screen are usually
/// short and have to be seen whole at a glance — `Cash · Call…` reads as a fault, not as guidance.
///
/// **The measuring side is [`fit_size`].** This one only draws — to match the size across sibling
/// cells, settle one size with `fit_size` first and hand that font here.
///
/// ```no_run
/// # fn draw(ui: &egui::Ui, cx: &fairing::Cx<'_>, cell_rect: egui::Rect) {
/// use fairing::layout;
/// layout::fit_text(
///     ui.painter(),
///     cell_rect.center(),
///     egui::Align2::CENTER_CENTER,
///     "Ethiopia Yirgacheffe 200 g",
///     cell_rect.width() * 0.9,
///     egui::FontId::proportional(cx.theme.metrics.row_height * 0.3),
///     cx.theme.color(fairing::ColorRole::OnSurface),
/// );
/// # }
/// ```
pub fn fit_text(
    painter: &egui::Painter,
    at: egui::Pos2,
    align: egui::Align2,
    text: &str,
    max_width: f32,
    font: egui::FontId,
    color: egui::Color32,
) {
    let font = fit_size(painter, std::slice::from_ref(&text), max_width, font);
    painter.text(at, align, text, font, color);
}

/// Settle one size against the longest, **so that sibling cells use the same text size**.
///
/// [`fit_text`] shrinks each cell separately. Used as it stands on a control that reads as one set
/// (category chips, payment methods, a receipt choice), that gives **a different text size on every
/// tile** — in Korean all three fitted and it never showed, and it surfaced the moment the language
/// was switched to English (measured on the kiosk example).
///
/// A **product grid, conversely, is right to shrink per cell** — one tile is one product, and having
/// all sixteen shrink because one product has a long name is worse. Which it is, the app knows.
///
/// ```no_run
/// # fn draw(ui: &egui::Ui, base: egui::FontId, w: f32) {
/// use fairing::layout;
/// let labels = ["Credit / debit card", "Mobile pay", "Cash · call an assistant"];
/// // All three are drawn in this font — the size fitted to the longest, "Cash · call an assistant".
/// let font = layout::fit_size(ui.painter(), &labels, w, base);
/// # let _ = font;
/// # }
/// ```
#[must_use]
pub fn fit_size(
    painter: &egui::Painter,
    texts: &[&str],
    max_width: f32,
    base: egui::FontId,
) -> egui::FontId {
    let mut size = base.size;
    for text in texts {
        let width = painter
            .layout_no_wrap((*text).to_owned(), base.clone(), egui::Color32::WHITE)
            .size()
            .x;
        if width > max_width && width > 0.0 {
            size = size.min(base.size * max_width / width);
        }
    }
    egui::FontId {
        size: size.max(FIT_MIN_SIZE),
        family: base.family,
    }
}

/// Shrunk below this it cannot be read — it shrinks no further (and overflows instead).
const FIT_MIN_SIZE: f32 = 8.0;

/// A Wi-Fi strength of `0..=4` as a fan icon. Being parametric, it cannot be changed by swapping the
/// icon set (guide 09 §6's pitfall) — the colour is a role, so the palette does follow.
pub fn wifi_icon(cx: &Cx<'_>, painter: &egui::Painter, rect: egui::Rect, level: u8, off: bool) {
    let style = crate::icons::IconStyle::sized(rect.width()).param_style(cx.theme);
    crate::icons::parametric::wifi(painter, rect, level, off, &style);
}

/// The rounding for a slider's value display. The track's range is `f32`, so it is shown as an integer.
#[expect(
    clippy::cast_possible_truncation,
    reason = "for display, and the slider range is a device value so it never exceeds i64"
)]
fn rounded(value: f32) -> i64 {
    value.round() as i64
}

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
