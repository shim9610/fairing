//! `Dropdown` selects an option from a list.
//!
//! `Trigger` selects the closed control's appearance; `Opener` selects how the
//! options are presented. These are independent settings. Supported openers include
//! an anchored list, grid, sheet and searchable list.
//!
//! The open panel is placed within the calling `Ui`'s clip rectangle and accounts
//! for `WidgetCx::inset_bottom`. Anchored panels can open above the trigger when
//! space below is insufficient. Long lists scroll and long labels are truncated.
//! An outside press dismisses the panel and is consumed within its bound.
//!
//! The panel uses the trigger's styling and opens over `motion.switch`.
//! `Dropdown::hints` adds display-only trailing text; it does not bind shortcuts.
//! Search input takes focus when the field is tapped. Choose the presentation and
//! option count after checking the available pane size and intended input device.
//! Layout, input handling and scrolling build on
//! [egui `Ui`](https://docs.rs/egui/0.37.0/egui/struct.Ui.html) and
//! [`ScrollArea`](https://docs.rs/egui/0.37.0/egui/containers/scroll_area/struct.ScrollArea.html).

mod painting;
use painting::{
    bottom_corners, chevron, chevron_at, divider, edge_fades, icon_at, paint_panel, paint_skin,
    paint_words, row_corners, text_width, top_corners, truncated,
};

mod popup;
mod trigger_paint;

use super::{DropdownLook, DropdownPart};
use crate::cx::WidgetCx as Cx;
use crate::icons::{builtin, IconColor, IconRef, IconStyle};
use crate::theme::{card_radius, ColorRole, Elevation};
use crate::unit::round_u8;
use egui::text::{LayoutJob, TextWrapping};
use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

/// The most of the available height an open list may take before it scrolls.
const LIST_FRACTION: f32 = 0.6;
/// The most of the pane a sheet may rise to. More than a list: a sheet is modal and the content
/// under it is dimmed, so there is nothing behind it the operator is still reading.
const SHEET_FRACTION: f32 = 0.7;
/// How deep the fade at a cut edge of a scrolling list is, as a fraction of a row.
const FADE_ROWS: f32 = 0.6;
/// The most rows any opener lays out. A list past a few dozen rows is not a dropdown, and the
/// cap keeps the arithmetic in a range an `f32` counts exactly.
const MAX_ROWS: u16 = 64;
/// The most of a row a hint may take before it is cut, so a long hint never squeezes the option
/// it belongs to.
const HINT_FRACTION: f32 = 0.4;
/// What is shown in the `Search` list when nothing matches, unless [`Dropdown::no_match`] says
/// otherwise.
const NO_MATCH: &str = "No match";
/// The layer the shield and the panel draw in.
///
/// **Middle, not Foreground.** The pane a screen draws in is `Background`, the shade, its scrim
/// and the gesture edges are `Foreground`, a toast and a heads-up are `Tooltip` — and the
/// on-screen keyboard is `Middle`. In `Foreground` the shield lay *over* the keyboard, so every
/// key a search was typed on was a tap that closed the search instead: the demo tour found it,
/// with a query that never arrived. In `Middle` the keyboard, which comes up after the search
/// field takes focus and so is the newer area, lies over the shield; the shade still lies over
/// everything here, which is right — a pull-down is not a tap outside.
const LAYER: egui::Order = egui::Order::Middle;
/// Maximum grid height as a fraction of the available pane.
const GRID_FRACTION: f32 = 0.7;

/// The closed control — what the finger sees before the list is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Trigger {
    /// The value on a button face, with a chevron.
    #[default]
    Button,
    /// A form field: the [`Dropdown::label`] floats small over the value, the way a
    /// [`TextField`](super::TextField)'s would.
    Field(FieldLook),
    /// The value in running text, with no box — only the chevron says it opens.
    Inline,
    /// Caption over a value in the heading text style.
    Tile,
    /// A chip — a filter in a toolbar. Wears the selected chip's face while it is open.
    Chip,
}

/// How a [`Trigger::Field`] is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldLook {
    /// A recessed box with a hairline round it — `TextField`'s own face.
    #[default]
    Outlined,
    /// A tinted box, square at the bottom, with the line under it.
    Filled,
    /// The line alone.
    Underlined,
}

/// How the options open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Opener {
    /// A list hanging off the trigger — flipped above it where there is no room below.
    #[default]
    Anchored,
    /// Finger-sized cells in a near-square grid, anchored the same way. Hints are left out.
    Grid,
    /// A sheet rising from the bottom of the pane behind a scrim, with a close button.
    Sheet,
    /// The trigger turned into a search field, the matching options under it.
    Search,
}

/// A closed list of options.
#[derive(Debug)]
pub struct Dropdown<'a> {
    id_salt: &'a str,
    options: &'a [&'a str],
    hints: &'a [&'a str],
    selected: &'a mut usize,
    label: Option<&'a str>,
    trigger: Trigger,
    opener: Opener,
    enabled: bool,
    no_match: &'a str,
}

impl<'a> Dropdown<'a> {
    /// The options, and the index of the chosen one. The salt names the open state.
    #[must_use]
    pub fn new(id_salt: &'a str, options: &'a [&'a str], selected: &'a mut usize) -> Self {
        Self {
            id_salt,
            options,
            hints: &[],
            selected,
            label: None,
            trigger: Trigger::Button,
            opener: Opener::Anchored,
            enabled: true,
            no_match: NO_MATCH,
        }
    }

    /// What a search shows when nothing matches — `"No match"` unless given. A widget has no
    /// string table, so on a panel in another language the words are the caller's: on a fairing
    /// screen, `cx.strings.get(fairing::i18n::labels::NO_MATCH)`.
    #[must_use]
    pub const fn no_match(mut self, text: &'a str) -> Self {
        self.no_match = text;
        self
    }

    /// A right-aligned, muted second text per row — the `i`-th hint sits on the `i`-th row. A
    /// keyboard shortcut is the usual thing; an empty string, or a slice shorter than the
    /// options, leaves the rest blank. **Shown, not bound**: the key itself is the shell's to
    /// handle.
    #[must_use]
    pub const fn hints(mut self, hints: &'a [&'a str]) -> Self {
        self.hints = hints;
        self
    }

    /// What the value is *of*. A field floats it over the value and a tile captions the value
    /// with it; a button, a chip and an inline value put it in front, muted. A sheet titles
    /// itself with it.
    #[must_use]
    pub const fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// The closed form.
    #[must_use]
    pub const fn trigger(mut self, trigger: Trigger) -> Self {
        self.trigger = trigger;
        self
    }

    /// How the options open.
    #[must_use]
    pub const fn opener(mut self, opener: Opener) -> Self {
        self.opener = opener;
        self
    }

    /// Enabled. Disabled it senses `hover()`, so the tap falls through.
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw it. On the frame a new option was picked, `response.changed()`.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let t = Tokens::read(ui, cx);
        let id = ui.scope_id().with(self.id_salt);
        let mut open = crate::interaction::load::<bool>(
            ui.ctx(),
            id,
            self.enabled && ui.is_enabled(),
            crate::interaction::Lifetime::Mounted,
        )
        .unwrap_or(false);
        let was_open = open;
        // The bound the list is placed inside and clipped to: where this was called, not the
        // window. Inside a screen that is the screen's own pane — less whatever lies over the
        // bottom of it, which while a search is typed is the keyboard.
        let mut bounds = ui.clip_rect();
        bounds.max.y = (bounds.max.y - cx.inset_bottom).max(bounds.min.y);

        let (rect, mut response) = ui.allocate_exact_size(
            self.trigger_size(ui, &t),
            if self.enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        if response.clicked() {
            let operation = if open {
                crate::interaction::rules::Operation::Cancel
            } else {
                crate::interaction::rules::Operation::Start
            };
            open = crate::interaction::rules::step(crate::interaction::snapshot(open), operation)
                .active;
        }
        let ink = Ink::of(cx, self.enabled);
        let face = Face {
            rect,
            open,
            pressed: response.is_pointer_button_down_on(),
            container: true,
            corners: None,
        };
        let closed = DropdownPart::Trigger {
            open,
            pressed: face.pressed,
            head: false,
        };
        if !self.painted(ui.painter(), cx, closed, rect, &mut ink.list.clone()) {
            self.paint_trigger(ui, cx, &t, face, ink);
        }

        if open && self.enabled {
            // A trigger the keyboard has come up over is scrolled clear of it by its own page,
            // to the top of the view so the rows have the most room under it.
            if cx.inset_bottom > 0.0 && rect.max.y > bounds.max.y {
                ui.scroll_to_rect(rect, Some(egui::Align::Min));
            }
            // Seeded at 0 on the frame it opens, so the list unrolls rather than appears.
            if !was_open {
                let _ = cx.animate(id.with("unroll"), 0.0, cx.theme.motion.switch);
            }
            let unroll = cx.animate(id.with("unroll"), 1.0, cx.theme.motion.switch);
            let place = Placement {
                trigger: rect,
                bounds,
                unroll,
            };
            let picked = match self.opener {
                Opener::Anchored => self.list(ui, cx, &t, id, place, ink),
                Opener::Grid => self.grid(ui, cx, &t, id, place, ink),
                Opener::Sheet => self.sheet(ui, cx, &t, id, place, ink),
                Opener::Search => self.search(ui, cx, &t, id, place, ink),
            };
            match picked {
                Pick::None => {}
                Pick::Dismissed => {
                    open = crate::interaction::rules::step(
                        crate::interaction::snapshot(open),
                        crate::interaction::rules::Operation::Cancel,
                    )
                    .active;
                }
                Pick::At(index) => {
                    open = crate::interaction::rules::step(
                        crate::interaction::snapshot(open),
                        crate::interaction::rules::Operation::Finish,
                    )
                    .active;
                    if index != *self.selected {
                        *self.selected = index;
                        response.mark_changed();
                    }
                }
            }
        }
        let open = open && self.enabled;
        if !open {
            // A closed search forgets what was typed: reopening it shows the whole list again.
            let _ = crate::interaction::take::<String>(ui.ctx(), id.with("query"));
            crate::interaction::forget_focus(ui.ctx(), id.with("field"));
            crate::interaction::retire_area(ui.ctx(), egui::LayerId::new(LAYER, id.with("shield")));
            crate::interaction::retire_area(ui.ctx(), egui::LayerId::new(LAYER, id.with("list")));
        }
        crate::interaction::store(ui.ctx(), id, open);
        response
    }
}

// ── Shared pieces ──────────────────────────────────────────────────────────────────────────

/// The tokens one frame draws with, read once.
struct Tokens {
    /// A row's, a cell's and a trigger's height: the control height, floored at the finger.
    target: f32,
    inset: f32,
    gap: f32,
    line_gap: f32,
    /// The chevron, the tick and the close glyph.
    mark: f32,
    control_radius: f32,
    card_radius: f32,
    /// A tile's least height.
    tile: f32,
    stroke_edge: f32,
    stroke_mark: f32,
    hairline: f32,
    body: egui::FontId,
    small: egui::FontId,
    heading: egui::FontId,
    /// A chip's and a sheet title's face: a choice, not body copy — as `Chip` has it.
    button: egui::FontId,
    small_h: f32,
    heading_h: f32,
}

impl Tokens {
    fn read(ui: &egui::Ui, cx: &Cx<'_>) -> Self {
        let m = &cx.theme.metrics;
        let c = &cx.theme.control;
        let body = egui::FontId::proportional(m.type_scale.body);
        let small = egui::FontId::proportional(m.type_scale.small);
        let heading = egui::FontId::proportional(m.type_scale.heading);
        let button = egui::TextStyle::Button.resolve(ui.style());
        let small_h = ui.fonts_mut(|f| f.row_height(&small));
        let heading_h = ui.fonts_mut(|f| f.row_height(&heading));
        Self {
            target: crate::theme::control_height(m, c),
            inset: m.content_inset,
            gap: c.gap,
            line_gap: c.line_gap,
            mark: c.icon,
            control_radius: m.control_radius,
            card_radius: card_radius(m, c),
            tile: m.tile_size,
            stroke_edge: c.stroke_edge,
            stroke_mark: c.stroke_mark,
            hairline: c.stroke_hairline,
            body,
            small,
            heading,
            button,
            small_h,
            heading_h,
        }
    }
}

/// The trigger's rect and state on this frame.
#[derive(Debug, Clone, Copy)]
struct Face {
    rect: Rect,
    open: bool,
    pressed: bool,
    /// Whether to paint the container — the fill, the edge, the elevation. `false` for the
    /// panel's head, which lies on the panel's own skin and paints only its words.
    container: bool,
    /// The corners a skin lets the head round, where it is one; a closed trigger rounds its own.
    corners: Option<CornerRadius>,
}

/// A label (muted, optional) and a value on one line, cut with an ellipsis where they would
/// run past `max_width`.
#[derive(Clone, Copy)]
struct Words<'w> {
    /// The left end, at the line's vertical centre.
    at: Pos2,
    max_width: f32,
    /// Between the label and the value: the control's own text gap.
    gap: f32,
    font: &'w egui::FontId,
    label: Option<&'w str>,
    value: &'w str,
    value_colour: Color32,
}

/// Where an open list is anchored, and how far it has unrolled.
#[derive(Debug, Clone, Copy)]
struct Placement {
    trigger: Rect,
    bounds: Rect,
    unroll: f32,
}

/// One row's place in a list.
#[derive(Debug, Clone, Copy)]
struct RowSlot {
    width: f32,
    /// The option's index — not the row's, which in a searched list is a different number.
    index: usize,
    corners: CornerRadius,
}

/// The block a grid lays its cells in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GridShape {
    cols: u16,
    rows: u16,
    cell: Vec2,
    gap: f32,
}

impl GridShape {
    /// Near-square: as many columns as the square root of the count asks, cut to what fits in
    /// `max_width` — and never fewer than one.
    fn fit(n: u16, cell_w: f32, cell_h: f32, gap: f32, max_width: f32) -> Self {
        let n = n.max(1);
        let mut wanted: u16 = 1;
        while wanted.saturating_mul(wanted) < n {
            wanted += 1;
        }
        let mut fits: u16 = 1;
        while f32::from(fits + 1).mul_add(cell_w + gap, gap) <= max_width && fits < n {
            fits += 1;
        }
        let cols = wanted.min(fits).min(n).max(1);
        let rows = n.div_ceil(cols);
        Self {
            cols,
            rows,
            cell: Vec2::new(cell_w, cell_h),
            gap,
        }
    }

    /// Widen the cells so the block spans `width`, where it is narrower than that.
    fn stretch_to(&mut self, width: f32) {
        if self.width() < width {
            let cols = f32::from(self.cols);
            self.cell.x = (width - self.gap * (cols + 1.0)) / cols;
        }
    }

    /// The block's width: the cells, a gap between each pair and one at each end.
    fn width(&self) -> f32 {
        let cols = f32::from(self.cols);
        cols.mul_add(self.cell.x, (cols + 1.0) * self.gap)
    }

    /// The block's height, the same way.
    fn height(&self) -> f32 {
        let rows = f32::from(self.rows);
        rows.mul_add(self.cell.y, (rows + 1.0) * self.gap)
    }

    /// The `i`-th cell, counting along the rows, in a block whose corner is at `origin`.
    fn cell(&self, origin: Pos2, i: usize) -> Rect {
        let cols = usize::from(self.cols.max(1));
        let col = u16::try_from(i % cols).unwrap_or(u16::MAX);
        let row = u16::try_from(i / cols).unwrap_or(u16::MAX);
        let x = f32::from(col).mul_add(self.cell.x + self.gap, origin.x + self.gap);
        let y = f32::from(row).mul_add(self.cell.y + self.gap, origin.y + self.gap);
        Rect::from_min_size(Pos2::new(x, y), self.cell)
    }
}

/// **The open panel is the trigger, unrolled**: the one silhouette the trigger and the
/// panel make, and the container it wears — the trigger's own.
#[derive(Debug, Clone, Copy)]
struct Skin {
    /// The whole silhouette: the trigger and the rows as one rect.
    rect: Rect,
    corners: CornerRadius,
    ground: Color32,
    /// The edge round the whole silhouette, where the trigger has one.
    stroke: Option<Stroke>,
    /// A line along the silhouette's bottom — a filled or an underlined field's.
    underline: Option<Stroke>,
    /// The corners on the head's side of the silhouette, for the head's own face and its pressed
    /// tint.
    head: CornerRadius,
    /// The corners at the rows' end, for the end row's pressed fill.
    tail: CornerRadius,
    /// Whether the rows hang under the head, or stand over it where there was no room below.
    below: bool,
}

impl Skin {
    /// The head's rect: the trigger's band, across the silhouette's width.
    fn head_rect(&self, trigger: Rect) -> Rect {
        Rect::from_min_size(
            Pos2::new(self.rect.min.x, trigger.min.y),
            Vec2::new(self.rect.width(), trigger.height()),
        )
    }

    /// The part of the silhouette shown while it unrolls from the trigger: the head whole from the
    /// first frame, the rows through a clip that grows away from it over `motion.switch`.
    fn unrolled(&self, trigger: Rect, unroll: f32) -> Rect {
        let k = unroll.clamp(0.0, 1.0);
        let r = self.rect;
        if self.below {
            let tail = r.max.y - trigger.max.y;
            Rect::from_min_max(r.min, Pos2::new(r.max.x, trigger.max.y + tail * k))
        } else {
            let tail = trigger.min.y - r.min.y;
            Rect::from_min_max(Pos2::new(r.min.x, trigger.min.y - tail * k), r.max)
        }
    }
}

/// What an opener hands its head: the skin, the trigger the head lies on, and the ink.
#[derive(Debug, Clone, Copy)]
struct Open {
    skin: Skin,
    trigger: Rect,
    ink: Ink,
}

/// Where a head's value goes — see `Dropdown::value_slot`.
#[derive(Debug, Clone, Copy)]
struct ValueSlot {
    /// The line the value sits on, full width.
    line: Rect,
    /// Where the value starts: after the padding, and after the label where it is on the line.
    left: f32,
    /// The centre of the mark at the right — the chevron's, or the search glyph's.
    mark_x: f32,
}

/// The floating layer an opener draws in, clipped to `clip`.
///
/// **Placed by this widget and never moved by egui.** An area is constrained to the screen by
/// default, and constrained it is dragged to wherever its *remembered* size fits — a sheet
/// starting below the pane's edge on purpose would be hauled up, and a panel whose first,
/// invisible sizing frame laid every row out (a scroll area shows everything in that pass)
/// would be hauled up for good, its rows then reaching down to where they were told to be and
/// keeping the oversize alive frame after frame. The headless harness found it: a sheet whose
/// layer reached a hundred pixels over its own top and took the tap that should have closed it.
/// So the constraint is off, and the sizing pass lays nothing out: the size is known.
///
/// **The panel is the shield's sublayer**, so it lies right over the shield every frame. Drawing
/// it second only decided the order the first time: egui raises a layer a press comes down on,
/// and the tap outside that closes the list comes down on the shield. When both came back the
/// shield was the one on top, and every row tapped after the reopen closed the list instead —
/// on the bench, a choice that never took. A drag outside, which leaves the list open, raised
/// the shield over rows still showing.
fn panel(ui: &egui::Ui, id: egui::Id, rect: Rect, clip: Rect, add: impl FnOnce(&mut egui::Ui)) {
    ui.ctx().set_sublayer(
        egui::LayerId::new(LAYER, id.with("shield")),
        egui::LayerId::new(LAYER, id.with("list")),
    );
    egui::Area::new(id.with("list"))
        .order(LAYER)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(clip);
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());
            if !ui.is_sizing_pass() {
                add(ui);
            }
        });
}

/// A sheet's close button: the last target-sized square of its first row.
fn sheet_close(sheet: Rect, t: &Tokens) -> Rect {
    Rect::from_min_size(
        Pos2::new(sheet.max.x - t.target, sheet.min.y),
        Vec2::splat(t.target),
    )
}

/// The rows, in a scroll area the height of `rect`. Returns where the scroll sits and how tall
/// the content is, for the fades.
fn scroll_rows(
    ui: &mut egui::Ui,
    id: egui::Id,
    rect: Rect,
    scrolls: bool,
    add: impl FnOnce(&mut egui::Ui),
) -> (f32, f32) {
    // Every source, as `layout::page` has it: egui's default drag-scrolls on touch
    // alone, and a panel driven by a trackball or a mouse could not scroll the list at all — the
    // headless harness found it, driving a pointer.
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(id.with("scroll"))
        .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
        .max_height(rect.height());
    if !scrolls {
        scroll = scroll.auto_shrink([false, false]);
    }
    let out = ui
        .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            scroll.show(ui, |ui| {
                // Rows abut: a gap between them is a gap in the list's own ground, and the
                // pressed fill would leave it unfilled.
                ui.spacing_mut().item_spacing.y = 0.0;
                add(ui);
            })
        })
        .inner;
    (out.state.clamped_offset().y, out.content_size.y)
}

/// Draw the search text editor without a separate frame.
/// Opening does not request editor focus; tapping the field requests it.
fn search_edit(
    ui: &mut egui::Ui,
    t: &Tokens,
    id: egui::Id,
    edit: Rect,
    query: &mut String,
    hint: &str,
    colour: Color32,
) {
    let field = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(edit)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(query)
                        .id(id.with("field"))
                        .font(t.body.clone())
                        .text_color(colour)
                        .hint_text(hint)
                        .frame(egui::Frame::NONE)
                        .desired_width(edit.width()),
                )
            },
        )
        .inner;
    // Selecting the query is a drag the field owns, and it says so.
    crate::drag::claim_if_held(&field);
}

/// The one row a search with nothing found shows. Not a row that can be picked.
fn no_match_row(ui: &mut egui::Ui, t: &Tokens, width: f32, ink: Ink, text: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, t.target), Sense::hover());
    let galley = truncated(
        ui,
        text,
        t.body.clone(),
        ink.hint,
        t.inset.mul_add(-2.0, width),
    );
    let size = galley.size();
    ui.painter().galley(
        Pos2::new(
            rect.min.x + t.inset + t.mark + t.gap,
            rect.center().y - size.y * 0.5,
        ),
        galley,
        ink.hint,
    );
}

/// The shield: everything under the list, across the bound, takes the tap that closes it — so
/// closing a list never presses what lay beneath the finger. Painted with `dim` where the opener
/// is modal. Returns whether it was tapped.
fn shield(ui: &egui::Ui, id: egui::Id, bounds: Rect, dim: Option<Color32>) -> bool {
    egui::Area::new(id.with("shield"))
        .order(LAYER)
        .fixed_pos(bounds.min)
        .constrain_to(bounds)
        .show(ui.ctx(), |ui| {
            if let Some(colour) = dim {
                ui.painter().rect_filled(bounds, CornerRadius::ZERO, colour);
            }
            ui.allocate_exact_size(bounds.size(), Sense::click())
                .1
                .clicked()
        })
        .inner
}

/// What the frame decided once the opener has had the press: a pick stands; otherwise a tap on
/// the shield, anywhere outside the bound but the trigger, or `Escape` closes it.
///
/// **A press on something lying over the content is not a tap outside.** The on-screen
/// keyboard draws in this widget's own order and over the pane's bottom; where a caller's clip
/// ends above the keys, a press on one must not read as a press beyond the bound. So a press
/// that egui puts on a layer at or above this one is left alone, and only a press on the
/// content beyond the bound — another pane, the desktop — closes the list. (The keyboard's
/// keys reaching the search at all is the keyboard's doing: it claims its panel as a widget
/// and keeps its layer on top of the order, see `osk`.)
fn dismissed(ui: &egui::Ui, pick: Pick, shield_tapped: bool, place: &Placement) -> Pick {
    if !matches!(pick, Pick::None) {
        return pick;
    }
    let ctx = ui.ctx();
    // Where the press went down — one that came down on the list and slid off in the same frame
    // pressed the list.
    let pressed_at = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| crate::drag::press_point(i).or(i.pointer.interact_pos()))
            .flatten()
    });
    let outside_bound = pressed_at.is_some_and(|p| {
        !place.bounds.contains(p)
            && !place.trigger.contains(p)
            && ctx.layer_id_at(p).is_none_or(|layer| layer.order < LAYER)
    });
    let escaped = ui.ctx().input(|i| i.key_pressed(egui::Key::Escape));
    if shield_tapped || outside_bound || escaped {
        Pick::Dismissed
    } else {
        Pick::None
    }
}

/// What one frame of an open list decided.
#[derive(Debug, Clone, Copy)]
enum Pick {
    /// Still open.
    None,
    /// A tap outside closed it.
    Dismissed,
    /// An option was chosen.
    At(usize),
}

/// Where an anchored list goes: under the trigger where there is room, above it where there is
/// not, never taller than `fraction` of the bound, and never outside it. The second value is
/// whether it had to be cut short.
fn place_list(trigger: Rect, bounds: Rect, wanted: f32, width: f32, fraction: f32) -> (Rect, bool) {
    let cap = bounds.height() * fraction;
    let below = bounds.max.y - trigger.max.y;
    let above = trigger.min.y - bounds.min.y;
    let height = wanted.min(cap).min(below.max(above)).max(0.0);
    let scrolls = height + 0.5 < wanted;
    let top = if height <= below {
        trigger.max.y
    } else {
        // Flipped: a dropdown on the last row of a screen must not open off the bottom.
        trigger.min.y - height
    };
    // Left-aligned with the trigger, and slid left rather than cut when it is wider than the
    // room to the right of it.
    let width = width.max(trigger.width()).min(bounds.width());
    let left = trigger.min.x.min(bounds.max.x - width).max(bounds.min.x);
    (
        Rect::from_min_size(egui::pos2(left, top), Vec2::new(width, height)),
        scrolls,
    )
}

/// The colours one frame paints with, faded **once** at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    /// A button's, a chip's, a tile's and a filled field's ground.
    face: Color32,
    /// A control's edge.
    edge: Color32,
    label: Color32,
    /// A hint, a floating label, a caption.
    hint: Color32,
    chevron: Color32,
    /// The open list's own ground.
    list: Color32,
    /// The chosen row's tick and text, the chosen cell's and an open chip's face.
    mark: Color32,
    /// Text on `mark`.
    on_mark: Color32,
    /// An outlined field's recessed ground — `TextField`'s.
    field: Color32,
    /// A field's edge and a header's line — an input's boundary, not a control's.
    outline: Color32,
    /// An open field's edge.
    focus: Color32,
    pressed: Color32,
    scrim: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, enabled: bool) -> Self {
        let tint = cx.theme.color(ColorRole::OnSurface);
        let ink = Self {
            face: tint.gamma_multiply(cx.theme.control.fill_alpha),
            edge: cx.theme.color(ColorRole::ControlEdge),
            label: tint,
            hint: cx.theme.color(ColorRole::Muted),
            chevron: cx.theme.color(ColorRole::Muted),
            // Opaque, and not the trigger's tint: a list lying over content must not let the
            // content show through it.
            list: cx.theme.color(ColorRole::SurfaceVariant),
            mark: cx.theme.color(ColorRole::Primary),
            on_mark: cx.theme.color(ColorRole::OnPrimary),
            field: cx.theme.color(ColorRole::Background),
            outline: cx.theme.color(ColorRole::Outline),
            focus: cx.theme.color(ColorRole::Focus),
            pressed: cx.theme.color(ColorRole::Pressed),
            scrim: cx.theme.color(ColorRole::Scrim),
        };
        if enabled {
            ink
        } else {
            let a = cx.theme.control.disabled_alpha;
            Self {
                face: ink.face.gamma_multiply(a),
                edge: ink.edge.gamma_multiply(a),
                label: ink.label.gamma_multiply(a),
                hint: ink.hint.gamma_multiply(a),
                chevron: ink.chevron.gamma_multiply(a),
                mark: ink.mark.gamma_multiply(a),
                outline: ink.outline.gamma_multiply(a),
                ..ink
            }
        }
    }
}

/// The pressed tint laid over a face while it is held.
trait PressedIf {
    fn pressed_if(self, pressed: bool, ink: Ink) -> Self;
}

impl PressedIf for Color32 {
    fn pressed_if(self, pressed: bool, ink: Ink) -> Self {
        if pressed {
            // `blend`'s receiver is the layer **behind**.
            self.blend(ink.pressed)
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GridShape, LIST_FRACTION};
    use egui::{pos2, Rect};

    fn bounds() -> Rect {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 480.0))
    }

    fn place(trigger: Rect, bounds: Rect, wanted: f32, width: f32) -> (Rect, bool) {
        super::place_list(trigger, bounds, wanted, width, LIST_FRACTION)
    }

    /// **With room below, the list hangs off the trigger.**
    #[test]
    fn a_list_with_room_below_opens_downwards() {
        let trigger = Rect::from_min_max(pos2(0.0, 40.0), pos2(400.0, 88.0));
        let (rect, scrolls) = place(trigger, bounds(), 144.0, trigger.width());
        assert!(!scrolls);
        assert!(
            (rect.min.y - trigger.max.y).abs() < 1e-3,
            "it did not hang off the trigger"
        );
        assert!(bounds().contains_rect(rect), "it escaped its bound");
    }

    /// **On the last row of a screen it flips above**, rather than opening off the bottom — the
    /// failure that makes an anchored popup unusable on a short panel.
    #[test]
    fn a_list_at_the_bottom_flips_above_the_trigger() {
        let trigger = Rect::from_min_max(pos2(0.0, 420.0), pos2(400.0, 468.0));
        let (rect, _) = place(trigger, bounds(), 144.0, trigger.width());
        assert!(rect.max.y <= trigger.min.y + 1e-3, "it did not flip");
        assert!(bounds().contains_rect(rect), "it escaped its bound");
    }

    /// **A list longer than its bound is cut and scrolls**, and it still never escapes.
    #[test]
    fn a_long_list_is_cut_to_its_bound_and_says_so() {
        let trigger = Rect::from_min_max(pos2(0.0, 40.0), pos2(400.0, 88.0));
        let (rect, scrolls) = place(trigger, bounds(), 2000.0, trigger.width());
        assert!(
            scrolls,
            "a list twice the panel did not report that it was cut"
        );
        assert!(rect.height() <= bounds().height() * super::LIST_FRACTION + 1e-3);
        assert!(bounds().contains_rect(rect));
    }

    /// A degenerate bound does not produce an inside-out rect for epaint to swallow.
    #[test]
    fn a_bound_with_no_room_gives_an_empty_list_rather_than_a_negative_one() {
        let tight = Rect::from_min_max(pos2(0.0, 100.0), pos2(400.0, 100.0));
        let trigger = Rect::from_min_max(pos2(0.0, 100.0), pos2(400.0, 100.0));
        let (rect, _) = place(trigger, tight, 144.0, trigger.width());
        assert!(rect.height() >= 0.0, "the list came out inside out");
    }

    /// **A grid is near-square**: nine cells are three by three, six are three by two, and every
    /// cell is where its index says.
    #[test]
    fn a_grid_is_near_square_and_counts_its_cells_along_the_rows() {
        let nine = GridShape::fit(9, 60.0, 48.0, 8.0, 1000.0);
        assert_eq!((nine.cols, nine.rows), (3, 3));
        let six = GridShape::fit(6, 60.0, 48.0, 8.0, 1000.0);
        assert_eq!((six.cols, six.rows), (3, 2));
        let four = GridShape::fit(4, 60.0, 48.0, 8.0, 1000.0);
        assert_eq!((four.cols, four.rows), (2, 2));
        let fifth = six.cell(pos2(0.0, 0.0), 4);
        assert!(
            (fifth.min.x - (8.0 + 60.0 + 8.0)).abs() < 1e-3
                && (fifth.min.y - (8.0 + 48.0 + 8.0)).abs() < 1e-3,
            "the fifth of six cells is the second on the second row: {fifth:?}"
        );
    }

    /// **A grid never outgrows its bound**: cut to the columns that fit, it grows down instead,
    /// and it never has fewer than one column.
    #[test]
    fn a_grid_is_cut_to_the_columns_that_fit_and_never_to_none() {
        let narrow = GridShape::fit(9, 60.0, 48.0, 8.0, 150.0);
        assert_eq!(
            narrow.cols, 2,
            "two 60-wide cells fit in 150 with gaps, not three"
        );
        assert_eq!(narrow.rows, 5);
        assert!(narrow.width() <= 150.0);
        let tight = GridShape::fit(9, 60.0, 48.0, 8.0, 10.0);
        assert_eq!(
            tight.cols, 1,
            "a bound too narrow for one cell still gets one column"
        );
        assert_eq!(tight.rows, 9);
    }

    /// **Under a wide trigger the cells stretch** to span it; under a narrow one they keep their
    /// own width.
    #[test]
    fn cells_stretch_to_a_wider_trigger_and_not_to_a_narrower_one() {
        let mut shape = GridShape::fit(4, 60.0, 48.0, 8.0, 1000.0);
        let own = shape.width();
        shape.stretch_to(own - 50.0);
        assert!(
            (shape.width() - own).abs() < 1e-3,
            "a narrower trigger shrank the cells"
        );
        shape.stretch_to(400.0);
        assert!(
            (shape.width() - 400.0).abs() < 1e-3,
            "the block did not span the trigger"
        );
    }
}
