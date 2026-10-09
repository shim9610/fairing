//! `Dropdown` — a closed list, and the one hard question about where it draws.
//!
//! # The gap
//!
//! Searching this crate for "dropdown" or "combo" returned nothing. The only route to a choice was
//! `layout::choice_rows` on a pushed screen — which costs a **whole navigation** for a three-item
//! setting, and the instrument-panel reference has two of them side by side on one card.
//!
//! # Where the list draws, and why
//!
//! This is a shell: something else already owns the status bar, the nav bar, the on-screen
//! keyboard, a shade that pulls down and the gesture edge zones. A popup is a layering decision
//! before it is a widget decision, and there were three candidates.
//!
//! **An in-place expansion** that pushes the rows below it down is what a settings list does, and
//! it is the cheapest — no new layer, nothing to clip. It loses because the list lives in a scroll
//! area: opening a six-option list shoves everything under it off a seven-inch panel, and the
//! operator loses the row they were reading.
//!
//! **A sheet rising from the bottom edge** is what a phone does, and it is the most comfortable for
//! a gloved hand. It lost the first round on machinery — it needs a surface of its own, and it has
//! to negotiate with the keyboard, the shade and the edge zones — and came back as
//! [`Opener::Sheet`] once the bound it rises in was settled: the caller's own pane, under the bars
//! and out of the edge zones by construction.
//!
//! **An area anchored to the trigger** is the default, with two rules that make it safe on a
//! small panel. It **flips above** the trigger when there is not room below, so a dropdown on the
//! last row of a screen does not open off the bottom. And it is clipped to — and placed inside —
//! **the calling `Ui`'s own clip rect**, which inside a screen is the screen's pane: so the list
//! cannot draw over the nav bar or the status bar, and the widget needs to know nothing about
//! either. The bound comes from where it was called, not from the window.
//!
//! # Why the list still scrolls, and when to stop using this
//!
//! Every row is a full `touch_target`, which on a gloved panel is about 13 mm — so a 480 du pane
//! holds about eight of them and no more. The list caps itself at `LIST_FRACTION` of the bound and
//! scrolls past that, but a list you have to scroll is a list you cannot see, and **past about eight
//! options the control wanted was `choice_rows` on a screen of its own** — or [`Opener::Search`].
//! The docs say so rather than the type refusing, because the caller knows their panel and this
//! does not.
//!
//! # Two axes, not one enum
//!
//! A survey of the shipped systems (Material 3's exposed dropdown and menus, Apple's pop-up
//! buttons and pickers, Ant's four `Select` variants, Carbon's inline and fluid dropdowns) gave
//! eight closed forms and six ways of opening, and every pairing of the two is a thing somebody
//! ships. So the closed control and the open one are chosen **separately**:
//!
//! | [`Trigger`] | Where it belongs |
//! |---|---|
//! | `Button` (default) | A toolbar, a card — the value on a button face |
//! | `Field(Outlined · Filled · Underlined)` | A form, beside `TextField` and `NumberField`: a floating label over the value |
//! | `Inline` | The value in running text — "Lock mode PDH ▾" — with no box of its own |
//! | `Tile` | The middle of a lying-L console: a small caption over a value read from across the bench |
//! | `Chip` | A filter in a toolbar |
//!
//! | [`Opener`] | Where it belongs |
//! |---|---|
//! | `Anchored` (default) | Five to about twelve options |
//! | `Grid` | Up to about nine **short** options — units, presets — as finger-sized cells: the best for a glove |
//! | `Sheet` | A phone's habit: rises from the bottom of the pane behind a scrim |
//! | `Search` | Past about twelve: the trigger becomes a search field, the matches under it |
//!
//! The count rule that came out of the survey, for the caller: up to four is a
//! [`SegmentedControl`](super::SegmentedControl); up to six a radio list or `choice_rows`; five
//! to twelve this; past that `Search` or a screen of its own. A value with an **order** — a
//! quantity, an hour — is not a dropdown at all but a drum, and that is its own widget.
//!
//! # The hint at the right of a row
//!
//! A row may carry a second, right-aligned text in the `Muted` role — a keyboard shortcut is the
//! usual thing, a unit or a count the next — set by [`Dropdown::hints`]. On a bench with a keypad
//! it is the difference between a list you read and a list you learn: macOS draws its key
//! equivalents there, Material 3 its trailing supporting text. The hint is **shown, not bound**:
//! which key does what is the shell's, and a hint that lied about it would be worse than none. A
//! grid cell has no room for one and leaves it out.
//!
//! # The open panel is the trigger, unrolled
//!
//! The first build gave every opener a panel of its own — an opaque card in the list ground, the
//! card radius, a floating shadow — whatever the trigger looked like, and set it under the trigger
//! as a second shape. A filled field opened into a card; a chip opened into the same card; a
//! search opened into that card docked to the top of the pane, nowhere near the chip that was
//! tapped. Each open control read as a different widget laid over the page (user report).
//!
//! Now the panel **wears the trigger's own container** and the two make **one silhouette**: the
//! trigger's ground, its edge, its radius and its underline run round the whole of it, the
//! trigger is drawn again as the panel's first row — the same words, the chevron turned over, a
//! tap on it closing the panel as a tap on the trigger always did — and the rows unroll under it
//! (or over it, where there is no room below) inside that one outline. A button opens into an
//! edged list in its own radius; an outlined field into its focus ring, grown; a filled field
//! keeps its rounded top and its line, now under the last row; a chip stays the accent-filled
//! head of a plain list; a tile's card holds the grid.
//!
//! `Search` is the same panel with the head turned into the field: the label stays where the
//! style keeps it, the query is typed where the value was — the value itself is the field's
//! hint, so what is chosen now stays readable while another is looked for — and the search glyph
//! takes the chevron's place. The field takes no focus on opening: the list is there
//! to be read first, and the keyboard comes when the field is tapped. The keyboard lies over the bottom of the pane and is not pushed,
//! so the bound the list is placed in stops at the keyboard's top
//! ([`WidgetCx::inset_bottom`](crate::WidgetCx::inset_bottom)): the list opens above the trigger
//! where the keys leave no room below, and a trigger the keys would cover is scrolled clear of
//! them by its own page. Docking the search to the top edge kept it clear of the keys too, but as
//! a different thing in a different place; this keeps it the control that was tapped.
//!
//! # What the second look found
//!
//! Rendered on the seven-inch demo and driven from the headless harness, the first build had
//! five faults. The list could not be scrolled by a pointer at all — egui's `ScrollArea`
//! drag-scrolls on touch alone by default, and the crate's own page had already chosen
//! `ScrollSource::ALL` for a device driven by a trackball; the list now does the same.
//! A long option ran over the chevron on a narrow trigger, and over the list's edge: both are
//! truncated with an ellipsis now, and the list takes the width of its **widest option** (never
//! narrower than the trigger, never wider than the bound) so the truncation is the last resort.
//! A tap outside the list closed it *and* landed on whatever was under the finger — on a bench
//! that is a "Start" button — so an invisible shield now lies under the open list across the
//! bound and eats that tap, the way a scrim does without the dimming. The pressed row's square
//! fill poked out of the list's rounded corners; the first and last rows round theirs. And a cut
//! list said nothing about being cut: it fades at the edge that has more behind it. The list also
//! unrolls from the trigger over `motion.switch` rather than appearing whole.
//!
//! # Why the open list is `Elevation::Floating`
//!
//! It is the crate's first genuine floating thing: content scrolls under it, and the elevation
//! token's own doc reserves that level for exactly this. It is also the only level that is
//! *identified* — the rim clears WCAG 1.4.11's 3.0 against the page, where a card's stays
//! decorative — which is right, because a list lying over content has to be told from it.

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
/// The most of the available height a grid may take: more than a list, because its rows are
/// few and a gloved hand wants every cell in sight.
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
    /// A caption over a value set large enough to read from across the bench.
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
        let id = ui.id().with(self.id_salt);
        let mut open = ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
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
            open = !open;
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
                Pick::Dismissed => open = false,
                Pick::At(index) => {
                    open = false;
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
            ui.data_mut(|d| d.remove_temp::<String>(id.with("query")));
        }
        ui.data_mut(|d| d.insert_temp(id, open));
        response
    }

    /// Hand `part` at `rect` to the dropdown painter, where there is one, and say
    /// whether it drew it. `ground` is the panel's: it goes in as the built-in one and comes back
    /// as the painter's, for the fades of a list that scrolls.
    fn painted(
        &self,
        painter: &egui::Painter,
        cx: &mut Cx<'_>,
        part: DropdownPart,
        rect: Rect,
        ground: &mut Color32,
    ) -> bool {
        let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.dropdown.as_mut()) else {
            return false;
        };
        let (text, hint) = match part {
            DropdownPart::Option { index, .. } => (
                self.options.get(index).copied().unwrap_or_default(),
                self.hint(index),
            ),
            _ => (self.shown(), None),
        };
        let mut look = DropdownLook {
            part,
            rect,
            trigger: self.trigger,
            opener: self.opener,
            label: self.label,
            text,
            hint,
            enabled: self.enabled,
            ground: *ground,
            theme: cx.theme,
            icons: &mut *cx.icons,
        };
        custom(painter, &mut look);
        *ground = look.ground;
        true
    }

    /// The open panel's ground — the skin, or a painter's — and the colour it is, which the list
    /// fades into. `shown` is what shows of it, `unroll` how far it has opened.
    fn ground(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        skin: &Skin,
        shown: Rect,
        unroll: f32,
    ) -> Color32 {
        let mut ground = skin.ground;
        let part = DropdownPart::Panel {
            shown,
            unroll,
            close: None,
        };
        if !self.painted(ui.painter(), cx, part, skin.rect, &mut ground) {
            paint_skin(ui.painter(), cx, skin);
        }
        ground
    }

    /// The chosen option's text, or an em dash where the index has run off the end.
    fn shown(&self) -> &str {
        self.options.get(*self.selected).copied().unwrap_or("—")
    }

    /// The hint on row `index`, where there is one and it says something.
    fn hint(&self, index: usize) -> Option<&str> {
        self.hints.get(index).copied().filter(|h| !h.is_empty())
    }

    /// How many rows any opener lays out.
    fn rows(&self) -> u16 {
        u16::try_from(self.options.len())
            .unwrap_or(u16::MAX)
            .min(MAX_ROWS)
    }

    // ── The trigger ────────────────────────────────────────────────────────────────────────

    /// The closed control's size. A button, a field and a tile take the width they are given; a
    /// chip and an inline value take what their words need.
    fn trigger_size(&self, ui: &egui::Ui, t: &Tokens) -> Vec2 {
        let avail = ui.available_width();
        match self.trigger {
            Trigger::Button => Vec2::new(avail, t.target),
            Trigger::Field(_) => {
                let h = if self.label.is_some() {
                    t.target + t.small_h
                } else {
                    t.target
                };
                Vec2::new(avail, h)
            }
            Trigger::Tile => {
                let inner = t.small_h + t.line_gap + t.heading_h;
                Vec2::new(avail, t.tile.max(t.inset.mul_add(2.0, inner)))
            }
            Trigger::Inline => {
                let words = self.words_width(ui, t, &t.body);
                let w = t.inset.mul_add(0.5, words) + t.gap + t.mark + t.inset * 0.5;
                Vec2::new(w.min(avail), t.target)
            }
            Trigger::Chip => {
                let words = self.words_width(ui, t, &t.button);
                // A chip's side padding is its radius, as `Chip` has it.
                let w = t.control_radius.mul_add(2.0, words) + t.gap + t.mark;
                Vec2::new(w.min(avail), t.target)
            }
        }
    }

    /// The width of the label (where there is one) and the value, in `font`, with the gap
    /// between them.
    fn words_width(&self, ui: &egui::Ui, t: &Tokens, font: &egui::FontId) -> f32 {
        let value = text_width(ui, self.shown(), font);
        self.label
            .map_or(value, |l| text_width(ui, l, font) + t.gap + value)
    }

    fn paint_trigger(&self, ui: &mut egui::Ui, cx: &mut Cx<'_>, t: &Tokens, face: Face, ink: Ink) {
        match self.trigger {
            Trigger::Button => self.paint_button(ui, cx, t, face, ink),
            Trigger::Field(look) => self.paint_field(ui, cx, t, face, ink, look),
            Trigger::Inline => self.paint_inline(ui, cx, t, face, ink),
            Trigger::Tile => self.paint_tile(ui, cx, t, face, ink),
            Trigger::Chip => self.paint_chip(ui, cx, t, face, ink),
        }
    }

    /// The current value on a button face, and a chevron that turns over when it is open.
    fn paint_button(&self, ui: &mut egui::Ui, cx: &mut Cx<'_>, t: &Tokens, face: Face, ink: Ink) {
        if face.container {
            let radius = CornerRadius::same(round_u8(t.control_radius));
            let painter = ui.painter();
            painter.rect_filled(face.rect, radius, ink.face.pressed_if(face.pressed, ink));
            painter.rect_stroke(
                face.rect,
                radius,
                Stroke::new(t.stroke_edge, ink.edge),
                StrokeKind::Inside,
            );
        }
        let words = Words {
            at: Pos2::new(face.rect.min.x + t.inset, face.rect.center().y),
            gap: t.gap,
            max_width: t.inset.mul_add(-1.5, face.rect.width()) - t.mark - t.gap,
            font: &t.body,
            label: self.label,
            value: self.shown(),
            value_colour: ink.label,
        };
        paint_words(ui, words, ink);
        chevron(ui, cx, t, face, ink.chevron);
    }

    /// A form field: the label floats small at the top, the value sits under it.
    fn paint_field(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        face: Face,
        ink: Ink,
        look: FieldLook,
    ) {
        let rect = face.rect;
        let radius = round_u8(t.control_radius);
        let (fill, corners) = match look {
            FieldLook::Outlined => (ink.field, CornerRadius::same(radius)),
            // Material's filled field: rounded where it meets the page, square where it meets
            // its own line.
            FieldLook::Filled => (
                ink.face,
                CornerRadius {
                    nw: radius,
                    ne: radius,
                    sw: 0,
                    se: 0,
                },
            ),
            FieldLook::Underlined => (Color32::TRANSPARENT, CornerRadius::ZERO),
        };
        if face.container {
            let painter = ui.painter();
            if fill != Color32::TRANSPARENT || face.pressed {
                painter.rect_filled(rect, corners, fill.pressed_if(face.pressed, ink));
            }
            // Open, the edge is the focus ring: an open field is a field being written to.
            let edge = if face.open {
                Stroke::new(t.stroke_mark, ink.focus)
            } else {
                Stroke::new(t.stroke_edge, ink.outline)
            };
            match look {
                FieldLook::Outlined => {
                    painter.rect_stroke(rect, corners, edge, StrokeKind::Inside);
                }
                FieldLook::Filled | FieldLook::Underlined => {
                    let y = rect.max.y - edge.width * 0.5;
                    painter
                        .line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], edge);
                }
            }
        }
        let pad = t.inset * 0.5;
        let max_width = t.inset.mul_add(-1.5, rect.width()) - t.mark - t.gap;
        let mut band = rect;
        if let Some(label) = self.label {
            let galley = truncated(ui, label, t.small.clone(), ink.hint, max_width);
            ui.painter().galley(
                Pos2::new(rect.min.x + t.inset, rect.min.y + pad),
                galley,
                ink.hint,
            );
            band.min.y = rect.min.y + pad + t.small_h;
        }
        let words = Words {
            at: Pos2::new(rect.min.x + t.inset, band.center().y),
            gap: t.gap,
            max_width,
            font: &t.body,
            label: None,
            value: self.shown(),
            value_colour: ink.label,
        };
        paint_words(ui, words, ink);
        chevron(ui, cx, t, face, ink.chevron);
    }

    /// The value in running text: no face, only the pressed tint while it is held.
    fn paint_inline(&self, ui: &mut egui::Ui, cx: &mut Cx<'_>, t: &Tokens, face: Face, ink: Ink) {
        if face.pressed && face.container {
            let radius = CornerRadius::same(round_u8(t.control_radius));
            ui.painter().rect_filled(face.rect, radius, ink.pressed);
        }
        let pad = t.inset * 0.5;
        let words = Words {
            at: Pos2::new(face.rect.min.x + pad, face.rect.center().y),
            gap: t.gap,
            max_width: pad.mul_add(-2.0, face.rect.width()) - t.mark - t.gap,
            font: &t.body,
            label: self.label,
            value: self.shown(),
            value_colour: ink.label,
        };
        paint_words(ui, words, ink);
        chevron_at(
            ui,
            cx,
            t,
            Pos2::new(face.rect.max.x - pad - t.mark * 0.5, face.rect.center().y),
            face.open,
            ink.chevron,
        );
    }

    /// A caption over a value set in the heading size, on a raised tile.
    fn paint_tile(&self, ui: &mut egui::Ui, cx: &mut Cx<'_>, t: &Tokens, face: Face, ink: Ink) {
        let rect = face.rect;
        if face.container {
            let radius = CornerRadius::same(round_u8(t.card_radius));
            crate::theme::paint_elevation(ui.painter(), cx.theme, rect, radius, Elevation::Raised);
            let painter = ui.painter();
            painter.rect_filled(rect, radius, ink.face.pressed_if(face.pressed, ink));
            painter.rect_stroke(
                rect,
                radius,
                Stroke::new(t.stroke_edge, ink.edge),
                StrokeKind::Inside,
            );
        }
        let max_width = t.inset.mul_add(-1.5, rect.width()) - t.mark - t.gap;
        if let Some(label) = self.label {
            let galley = truncated(ui, label, t.small.clone(), ink.hint, max_width);
            ui.painter().galley(
                Pos2::new(rect.min.x + t.inset, rect.min.y + t.inset),
                galley,
                ink.hint,
            );
        }
        let value = truncated(ui, self.shown(), t.heading.clone(), ink.label, max_width);
        let size = value.size();
        ui.painter().galley(
            Pos2::new(rect.min.x + t.inset, rect.max.y - t.inset - size.y),
            value,
            ink.label,
        );
        chevron(ui, cx, t, face, ink.chevron);
    }

    /// A chip: the unselected chip's face closed, the selected chip's face while it is open.
    fn paint_chip(&self, ui: &mut egui::Ui, cx: &mut Cx<'_>, t: &Tokens, face: Face, ink: Ink) {
        let radius = face
            .corners
            .unwrap_or_else(|| CornerRadius::same(round_u8(t.control_radius)));
        let (fill, text) = if face.open {
            (ink.mark, ink.on_mark)
        } else {
            (ink.face, ink.label)
        };
        // An open chip's accent face is its content — the head of the open panel wears it too.
        if face.container || face.open {
            let painter = ui.painter();
            painter.rect_filled(face.rect, radius, fill.pressed_if(face.pressed, ink));
            if !face.open {
                painter.rect_stroke(
                    face.rect,
                    radius,
                    Stroke::new(t.stroke_edge, ink.edge),
                    StrokeKind::Inside,
                );
            }
        }
        let pad = t.control_radius;
        let words = Words {
            at: Pos2::new(face.rect.min.x + pad, face.rect.center().y),
            gap: t.gap,
            max_width: pad.mul_add(-2.0, face.rect.width()) - t.mark - t.gap,
            font: &t.button,
            label: self.label,
            value: self.shown(),
            value_colour: text,
        };
        // An open chip's muted label would vanish on the accent: the whole line takes the
        // accent's own text colour.
        let mut ink = ink;
        if face.open {
            ink.hint = ink.on_mark;
        }
        paint_words(ui, words, ink);
        chevron_at(
            ui,
            cx,
            t,
            Pos2::new(face.rect.max.x - pad - t.mark * 0.5, face.rect.center().y),
            face.open,
            if face.open { ink.on_mark } else { ink.chevron },
        );
    }

    // ── The openers ────────────────────────────────────────────────────────────────────────

    /// The widest row's text, in the body face: the option and, where it has one, its hint.
    fn widest_row(&self, ui: &egui::Ui, t: &Tokens) -> f32 {
        self.options
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let option = text_width(ui, o, &t.body);
                self.hint(i)
                    .map_or(option, |h| option + t.gap + text_width(ui, h, &t.body))
            })
            .fold(0.0, f32::max)
    }

    /// The width an anchored list wants: as wide as the widest row asks, within the bound — the
    /// trigger's width is a floor, not the list's size, and an ellipsis is the last resort rather
    /// than the first.
    fn list_width(&self, ui: &egui::Ui, t: &Tokens, place: &Placement) -> f32 {
        let text_x = t.inset + t.mark + t.gap;
        (self.widest_row(ui, t) + text_x + t.inset)
            .max(place.trigger.width())
            .min(place.bounds.width())
    }

    /// **The look the open panel wears** — the trigger's own container, so the panel and the
    /// trigger make one silhouette. `list` is where the rows go, under or over the
    /// trigger.
    fn skin(&self, t: &Tokens, ink: Ink, trigger: Rect, list: Rect) -> Skin {
        let below = list.min.y >= trigger.max.y - 0.5;
        let radius = round_u8(match self.trigger {
            Trigger::Tile => t.card_radius,
            _ => t.control_radius,
        });
        let all = CornerRadius::same(radius);
        let top = CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        };
        let edge = Stroke::new(t.stroke_edge, ink.edge);
        // An open field is a field being written to: its edge is the focus ring, and the ring
        // runs round the whole of what it opened into.
        let focus = Stroke::new(t.stroke_mark, ink.focus);
        let (corners, ground, stroke, underline) = match self.trigger {
            Trigger::Button | Trigger::Tile => (all, ink.list, Some(edge), None),
            Trigger::Field(FieldLook::Outlined) => (all, ink.field, Some(focus), None),
            // Material's filled field: rounded where it meets the page, square where it meets
            // its own line — and the line is now under the last row.
            Trigger::Field(FieldLook::Filled) => (top, ink.list, None, Some(focus)),
            Trigger::Field(FieldLook::Underlined) => {
                (CornerRadius::ZERO, ink.list, None, Some(focus))
            }
            Trigger::Inline | Trigger::Chip => (all, ink.list, None, None),
        };
        let (head, tail) = if below {
            (top_corners(corners), bottom_corners(corners))
        } else {
            (bottom_corners(corners), top_corners(corners))
        };
        Skin {
            rect: trigger.union(list),
            corners,
            ground,
            stroke,
            underline,
            head,
            tail,
            below,
        }
    }

    /// The trigger drawn again as the panel's first row, over the page's own copy: the same
    /// words on the panel's ground, the chevron turned over, and a tap on it closes the panel as
    /// a tap on the trigger always did. Returns whether it was tapped.
    fn head(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        open: &Open,
    ) -> bool {
        let Open { skin, trigger, ink } = *open;
        let rect = skin.head_rect(trigger);
        let response = ui.interact(rect, id.with("head"), Sense::click());
        let pressed = response.is_pointer_button_down_on();
        let head = DropdownPart::Trigger {
            open: true,
            pressed,
            head: true,
        };
        if self.painted(ui.painter(), cx, head, rect, &mut skin.ground.clone()) {
            return response.clicked();
        }
        if pressed && !matches!(self.trigger, Trigger::Chip) {
            ui.painter().rect_filled(rect, skin.head, ink.pressed);
        }
        let face = Face {
            rect,
            open: true,
            pressed,
            container: false,
            corners: Some(skin.head),
        };
        self.paint_trigger(ui, cx, t, face, ink);
        divider(ui.painter(), t, &skin, rect, ink);
        response.clicked()
    }

    /// The head as a search field: the label where the style keeps it, the query typed where the
    /// value was — the value itself is the field's hint, so what is chosen now stays readable
    /// while another is looked for — and the search glyph in the chevron's place.
    fn search_head(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        open: &Open,
        query: &mut String,
    ) {
        let Open { skin, trigger, ink } = *open;
        let rect = skin.head_rect(trigger);
        let chip = matches!(self.trigger, Trigger::Chip);
        let head = DropdownPart::Trigger {
            open: true,
            pressed: false,
            head: true,
        };
        // A painter draws the head; the query is typed into the `TextEdit` over it either way.
        let painted = self.painted(ui.painter(), cx, head, rect, &mut skin.ground.clone());
        if chip && !painted {
            ui.painter().rect_filled(rect, skin.head, ink.mark);
        }
        let text = if chip { ink.on_mark } else { ink.label };
        let slot = self.value_slot(ui, t, rect, (ink, !painted));
        if !painted {
            icon_at(
                ui,
                cx,
                &builtin::SEARCH,
                Pos2::new(slot.mark_x, slot.line.center().y),
                t.mark,
                if chip { ink.on_mark } else { ink.chevron },
            );
        }
        let edit = Rect::from_min_max(
            Pos2::new(slot.left, slot.line.min.y),
            Pos2::new(slot.mark_x - t.mark * 0.5 - t.gap, slot.line.max.y),
        );
        search_edit(ui, t, id, edit, query, self.shown(), text);
        if !painted {
            divider(ui.painter(), t, &skin, rect, ink);
        }
    }

    /// Where the value goes on a head of this trigger's style, with the label painted where the
    /// style keeps it — `paint` false leaves it to a painter: the line the value sits on, the x
    /// it starts at, and the centre of the mark at the right.
    fn value_slot(
        &self,
        ui: &egui::Ui,
        t: &Tokens,
        rect: Rect,
        (ink, paint): (Ink, bool),
    ) -> ValueSlot {
        let chip = matches!(self.trigger, Trigger::Chip);
        let hint = if chip { ink.on_mark } else { ink.hint };
        let (pad, font) = match self.trigger {
            Trigger::Inline => (t.inset * 0.5, &t.body),
            Trigger::Chip => (t.control_radius, &t.button),
            Trigger::Button | Trigger::Field(_) | Trigger::Tile => (t.inset, &t.body),
        };
        let mark_x = match self.trigger {
            Trigger::Inline | Trigger::Chip => rect.max.x - pad - t.mark * 0.5,
            Trigger::Button | Trigger::Field(_) | Trigger::Tile => {
                rect.max.x - t.inset * 0.5 - t.mark * 0.5
            }
        };
        let max_width = pad.mul_add(-2.0, rect.width()) - t.mark - t.gap;
        let mut left = rect.min.x + pad;
        let line = match self.trigger {
            Trigger::Field(_) => {
                let top = t.inset * 0.5;
                let mut band = rect;
                if let Some(label) = self.label {
                    if paint {
                        let galley = truncated(ui, label, t.small.clone(), hint, max_width);
                        ui.painter()
                            .galley(Pos2::new(left, rect.min.y + top), galley, hint);
                    }
                    band.min.y = rect.min.y + top + t.small_h;
                }
                band
            }
            Trigger::Tile => {
                if let Some(label) = self.label.filter(|_| paint) {
                    let galley = truncated(ui, label, t.small.clone(), hint, max_width);
                    ui.painter()
                        .galley(Pos2::new(left, rect.min.y + t.inset), galley, hint);
                }
                Rect::from_min_max(
                    Pos2::new(rect.min.x, rect.max.y - t.inset - t.heading_h),
                    Pos2::new(rect.max.x, rect.max.y - t.inset),
                )
            }
            Trigger::Button | Trigger::Inline | Trigger::Chip => {
                if let Some(label) = self.label {
                    let galley = truncated(ui, label, font.clone(), hint, max_width * 0.5);
                    let size = galley.size();
                    if paint {
                        ui.painter().galley(
                            Pos2::new(left, rect.center().y - size.y * 0.5),
                            galley,
                            hint,
                        );
                    }
                    left += size.x + t.gap;
                }
                rect
            }
        };
        ValueSlot { line, left, mark_x }
    }

    /// The anchored list. Returns what the frame decided.
    fn list(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        place: Placement,
        ink: Ink,
    ) -> Pick {
        let wanted = t.target * f32::from(self.rows());
        let width = self.list_width(ui, t, &place);
        let (rows, scrolls) = place_list(place.trigger, place.bounds, wanted, width, LIST_FRACTION);
        let skin = self.skin(t, ink, place.trigger, rows);
        let open = Open {
            skin,
            trigger: place.trigger,
            ink,
        };

        // The shield first, so the panel is the one on top.
        let shield_tapped = shield(ui, id, place.bounds, None);
        let shown = place
            .bounds
            .intersect(skin.unrolled(place.trigger, place.unroll));
        let mut pick = Pick::None;
        let last = self.options.len().saturating_sub(1);
        panel(ui, id, skin.rect, shown, |ui| {
            let ground = self.ground(ui, cx, &skin, shown, place.unroll);
            if self.head(ui, cx, t, id, &open) {
                pick = Pick::Dismissed;
            }
            let out = scroll_rows(ui, id, rows, scrolls, |ui| {
                for index in 0..self.options.len() {
                    let slot = RowSlot {
                        width: rows.width(),
                        index,
                        corners: row_corners(index, last, skin.tail),
                    };
                    if self.row(ui, cx, t, slot, ink) {
                        pick = Pick::At(index);
                    }
                }
            });
            if scrolls {
                edge_fades(ui.painter(), rows, ground, t.target * FADE_ROWS, out);
            }
        });
        dismissed(ui, pick, shield_tapped, &place)
    }

    /// The grid: finger-sized cells in a near-square block, anchored like the list.
    fn grid(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        place: Placement,
        ink: Ink,
    ) -> Pick {
        let widest = self
            .options
            .iter()
            .map(|o| text_width(ui, o, &t.body))
            .fold(0.0, f32::max);
        let cell_w = t.gap.mul_add(2.0, widest).max(t.target);
        let mut shape = GridShape::fit(self.rows(), cell_w, t.target, t.gap, place.bounds.width());
        // Under a trigger wider than the block, the cells widen to fill it: the block then lines
        // up with what opened it instead of hanging off its left end.
        shape.stretch_to(place.trigger.width());
        let width = shape.width().min(place.bounds.width());
        let (rows, scrolls) = place_list(
            place.trigger,
            place.bounds,
            shape.height(),
            width,
            GRID_FRACTION,
        );
        let skin = self.skin(t, ink, place.trigger, rows);
        let open = Open {
            skin,
            trigger: place.trigger,
            ink,
        };

        let shield_tapped = shield(ui, id, place.bounds, None);
        let shown = place
            .bounds
            .intersect(skin.unrolled(place.trigger, place.unroll));
        let mut pick = Pick::None;
        panel(ui, id, skin.rect, shown, |ui| {
            let ground = self.ground(ui, cx, &skin, shown, place.unroll);
            if self.head(ui, cx, t, id, &open) {
                pick = Pick::Dismissed;
            }
            let out = scroll_rows(ui, id, rows, scrolls, |ui| {
                let (block, _) =
                    ui.allocate_exact_size(Vec2::new(rows.width(), shape.height()), Sense::hover());
                for index in 0..self.options.len().min(usize::from(MAX_ROWS)) {
                    let cell = shape.cell(block.min, index);
                    if self.grid_cell(ui, cx, t, id, (cell, index), ink) {
                        pick = Pick::At(index);
                    }
                }
            });
            if scrolls {
                edge_fades(ui.painter(), rows, ground, t.target * FADE_ROWS, out);
            }
        });
        dismissed(ui, pick, shield_tapped, &place)
    }

    /// One cell: the chosen one wears the accent, the rest the control face. Returns whether it
    /// was picked.
    fn grid_cell(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        (cell, index): (Rect, usize),
        ink: Ink,
    ) -> bool {
        let response = ui.interact(cell, id.with(("cell", index)), Sense::click());
        let on = index == *self.selected;
        let part = DropdownPart::Option {
            index,
            selected: on,
            pressed: response.is_pointer_button_down_on(),
            cell: true,
        };
        if self.painted(ui.painter(), cx, part, cell, &mut ink.list.clone()) {
            return response.clicked();
        }
        let radius = CornerRadius::same(round_u8(t.control_radius));
        let fill = if on { ink.mark } else { ink.face };
        let painter = ui.painter();
        painter.rect_filled(
            cell,
            radius,
            fill.pressed_if(response.is_pointer_button_down_on(), ink),
        );
        if !on {
            painter.rect_stroke(
                cell,
                radius,
                Stroke::new(t.stroke_edge, ink.edge),
                StrokeKind::Inside,
            );
        }
        let colour = if on { ink.on_mark } else { ink.label };
        let galley = truncated(
            ui,
            self.options.get(index).copied().unwrap_or_default(),
            t.body.clone(),
            colour,
            t.gap.mul_add(-2.0, cell.width()),
        );
        let size = galley.size();
        ui.painter()
            .galley(cell.center() - size * 0.5, galley, colour);
        response.clicked()
    }

    /// The sheet: full width at the bottom of the pane, a scrim over the rest.
    fn sheet(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        place: Placement,
        ink: Ink,
    ) -> Pick {
        let bounds = place.bounds;
        let wanted = t.target * (f32::from(self.rows()) + 1.0);
        let height = wanted.min(bounds.height() * SHEET_FRACTION).max(0.0);
        let scrolls = height + 0.5 < wanted;
        let rect = Rect::from_min_max(Pos2::new(bounds.min.x, bounds.max.y - height), bounds.max);
        let k = place.unroll.clamp(0.0, 1.0);
        // Slid up from below the pane's edge rather than clipped: a sheet moves as one thing.
        let shown = rect.translate(Vec2::new(0.0, height * (1.0 - k)));
        let radius = round_u8(t.card_radius);
        let corners = CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        };

        let shield_tapped = shield(ui, id, bounds, Some(ink.scrim.gamma_multiply(k)));
        let mut pick = Pick::None;
        panel(ui, id, shown, bounds, |ui| {
            let mut ground = ink.list;
            let part = DropdownPart::Panel {
                shown,
                unroll: k,
                close: Some(sheet_close(shown, t)),
            };
            let painted = self.painted(ui.painter(), cx, part, shown, &mut ground);
            if !painted {
                paint_panel(ui.painter(), cx, shown, corners, ink.list);
            }
            if self.sheet_header(ui, cx, t, id, (shown, painted), ink) {
                pick = Pick::Dismissed;
            }
            let body =
                Rect::from_min_max(Pos2::new(shown.min.x, shown.min.y + t.target), shown.max);
            let out = scroll_rows(ui, id, body, scrolls, |ui| {
                for index in 0..self.options.len() {
                    let slot = RowSlot {
                        width: body.width(),
                        index,
                        corners: CornerRadius::ZERO,
                    };
                    if self.row(ui, cx, t, slot, ink) {
                        pick = Pick::At(index);
                    }
                }
            });
            if scrolls {
                edge_fades(ui.painter(), body, ground, t.target * FADE_ROWS, out);
            }
        });
        dismissed(ui, pick, shield_tapped, &place)
    }

    /// The sheet's first row: its title at the left, a close button at the right — drawn here
    /// unless a painter drew the sheet (`painted`), pressed either way. Returns whether the close
    /// was pressed.
    fn sheet_header(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        (sheet, painted): (Rect, bool),
        ink: Ink,
    ) -> bool {
        let header = Rect::from_min_size(sheet.min, Vec2::new(sheet.width(), t.target));
        let close = sheet_close(sheet, t);
        let response = ui.interact(close, id.with("close"), Sense::click());
        if painted {
            return response.clicked();
        }
        if response.is_pointer_button_down_on() {
            ui.painter().rect_filled(
                close,
                CornerRadius::same(round_u8(t.control_radius)),
                ink.pressed,
            );
        }
        if let Some(label) = self.label {
            let galley = truncated(
                ui,
                label,
                t.button.clone(),
                ink.label,
                header.width() - t.inset - t.target,
            );
            let size = galley.size();
            ui.painter().galley(
                Pos2::new(header.min.x + t.inset, header.center().y - size.y * 0.5),
                galley,
                ink.label,
            );
        }
        icon_at(ui, cx, &builtin::CLOSE, close.center(), t.mark, ink.chevron);
        // The line under the header: the title is not a row.
        let y = header.max.y - t.hairline * 0.5;
        ui.painter().line_segment(
            [Pos2::new(header.min.x, y), Pos2::new(header.max.x, y)],
            Stroke::new(t.hairline, ink.outline),
        );
        response.clicked()
    }

    /// The search: the head turned into a field, the matches under it in the same panel.
    fn search(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        place: Placement,
        ink: Ink,
    ) -> Pick {
        let query_id = id.with("query");
        let mut query = ui.data(|d| d.get_temp::<String>(query_id).unwrap_or_default());
        let matches = self.matches(&query);
        let shown_rows = u16::try_from(matches.len())
            .unwrap_or(u16::MAX)
            .clamp(1, MAX_ROWS);
        let wanted = t.target * f32::from(shown_rows);
        let width = self.list_width(ui, t, &place);
        let (rows, scrolls) = place_list(place.trigger, place.bounds, wanted, width, LIST_FRACTION);
        let skin = self.skin(t, ink, place.trigger, rows);
        let open = Open {
            skin,
            trigger: place.trigger,
            ink,
        };

        let shield_tapped = shield(ui, id, place.bounds, None);
        let shown = place
            .bounds
            .intersect(skin.unrolled(place.trigger, place.unroll));
        let mut pick = Pick::None;
        let last = matches.len().saturating_sub(1);
        panel(ui, id, skin.rect, shown, |ui| {
            let ground = self.ground(ui, cx, &skin, shown, place.unroll);
            self.search_head(ui, cx, t, id, &open, &mut query);
            let out = scroll_rows(ui, id, rows, scrolls, |ui| {
                if matches.is_empty() {
                    no_match_row(ui, t, rows.width(), ink, self.no_match);
                }
                for (n, index) in matches.iter().copied().enumerate() {
                    let slot = RowSlot {
                        width: rows.width(),
                        index,
                        corners: row_corners(n, last, skin.tail),
                    };
                    if self.row(ui, cx, t, slot, ink) {
                        pick = Pick::At(index);
                    }
                }
            });
            if scrolls {
                edge_fades(ui.painter(), rows, ground, t.target * FADE_ROWS, out);
            }
        });
        ui.data_mut(|d| d.insert_temp(query_id, query));
        dismissed(ui, pick, shield_tapped, &place)
    }

    /// The options that contain `query`, case-insensitively, in their own order. An empty query
    /// matches everything.
    fn matches(&self, query: &str) -> Vec<usize> {
        let needle = query.trim().to_lowercase();
        self.options
            .iter()
            .enumerate()
            .filter(|(_, o)| needle.is_empty() || o.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .take(usize::from(MAX_ROWS))
            .collect()
    }

    /// One option: a tick where it is the chosen one, the text, and its hint at the right.
    /// Returns whether it was picked.
    fn row(&self, ui: &mut egui::Ui, cx: &mut Cx<'_>, t: &Tokens, slot: RowSlot, ink: Ink) -> bool {
        let RowSlot {
            width,
            index,
            corners,
        } = slot;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, t.target), Sense::click());
        let on = index == *self.selected;
        let part = DropdownPart::Option {
            index,
            selected: on,
            pressed: response.is_pointer_button_down_on(),
            cell: false,
        };
        if self.painted(ui.painter(), cx, part, rect, &mut ink.list.clone()) {
            return response.clicked();
        }
        if response.is_pointer_button_down_on() {
            // The first and last rows round the corners they share with the list, so the
            // pressed fill does not poke out of it.
            ui.painter().rect_filled(rect, corners, ink.pressed);
        }
        if on {
            icon_at(
                ui,
                cx,
                &builtin::CHECK,
                Pos2::new(rect.min.x + t.inset + t.mark * 0.5, rect.center().y),
                t.mark,
                ink.mark,
            );
        }
        // Every row leaves room for the tick whether it wears one or not, so the list does not
        // shuffle sideways as the choice moves.
        let text_x = rect.min.x + t.inset + t.mark + t.gap;
        let mut right = rect.max.x - t.inset;
        if let Some(hint) = self.hint(index) {
            let galley = truncated(ui, hint, t.body.clone(), ink.hint, width * HINT_FRACTION);
            let size = galley.size();
            right -= size.x;
            ui.painter().galley(
                Pos2::new(right, rect.center().y - size.y * 0.5),
                galley,
                ink.hint,
            );
            right -= t.gap;
        }
        let colour = if on { ink.mark } else { ink.label };
        let galley = truncated(
            ui,
            self.options.get(index).copied().unwrap_or_default(),
            t.body.clone(),
            colour,
            right - text_x,
        );
        let size = galley.size();
        ui.painter().galley(
            Pos2::new(text_x, rect.center().y - size.y * 0.5),
            galley,
            colour,
        );
        response.clicked()
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

/// Where both fit, neither is cut. Where they do not, the value keeps what it needs up to half
/// the room and the muted label is cut to the rest — the value is what the control is for.
fn paint_words(ui: &egui::Ui, words: Words<'_>, ink: Ink) {
    let mut x = words.at.x;
    let mut room = words.max_width;
    if let Some(label) = words.label {
        let label_w = text_width(ui, label, words.font);
        let value_w = text_width(ui, words.value, words.font);
        let value_take = value_w
            .min((room - label_w - words.gap).max(room * 0.5))
            .min(room);
        let label_room = (room - value_take - words.gap).max(0.0);
        let galley = truncated(ui, label, words.font.clone(), ink.hint, label_room);
        let size = galley.size();
        ui.painter()
            .galley(Pos2::new(x, words.at.y - size.y * 0.5), galley, ink.hint);
        let step = size.x + words.gap;
        x += step;
        room -= step;
    }
    let galley = truncated(
        ui,
        words.value,
        words.font.clone(),
        words.value_colour,
        room,
    );
    let size = galley.size();
    ui.painter().galley(
        Pos2::new(x, words.at.y - size.y * 0.5),
        galley,
        words.value_colour,
    );
}

/// The chevron at the trigger's right edge, half an inset in: a glyph carries its own optical
/// padding where a word does not, and Material's exposed dropdown pads its trailing icon 12 dp
/// against the text's 16. On a narrow field the difference is a value shown whole.
fn chevron(ui: &egui::Ui, cx: &mut Cx<'_>, t: &Tokens, face: Face, colour: Color32) {
    chevron_at(
        ui,
        cx,
        t,
        Pos2::new(
            face.rect.max.x - t.inset * 0.5 - t.mark * 0.5,
            face.rect.center().y,
        ),
        face.open,
        colour,
    );
}

/// A chevron that turns over when the list is open — drawn, not rotated: the crate ships both
/// glyphs, and turning one over would have to guess a pivot that the icon's own bounding box
/// does not carry.
fn chevron_at(ui: &egui::Ui, cx: &mut Cx<'_>, t: &Tokens, at: Pos2, open: bool, colour: Color32) {
    let icon = if open {
        builtin::CHEVRON_UP
    } else {
        builtin::CHEVRON_DOWN
    };
    icon_at(ui, cx, &icon, at, t.mark, colour);
}

/// One glyph, `side` square, centred on `at`.
fn icon_at(ui: &egui::Ui, cx: &mut Cx<'_>, icon: &IconRef, at: Pos2, side: f32, colour: Color32) {
    // Sized to the glyph it draws: the stroke follows the size, and `default()` is 24 whatever
    // the glyph is.
    let style = IconStyle::sized(side).color(IconColor::Fixed(colour));
    let rect = Rect::from_center_size(at, Vec2::splat(side));
    cx.icons.paint(ui.painter(), rect, icon, &style, cx.theme);
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

/// The top two corners of `c`, the bottom two square.
const fn top_corners(c: CornerRadius) -> CornerRadius {
    CornerRadius {
        nw: c.nw,
        ne: c.ne,
        sw: 0,
        se: 0,
    }
}

/// The bottom two corners of `c`, the top two square.
const fn bottom_corners(c: CornerRadius) -> CornerRadius {
    CornerRadius {
        nw: 0,
        ne: 0,
        sw: c.sw,
        se: c.se,
    }
}

/// The skin itself: its elevation, its ground, its edge and its line.
fn paint_skin(painter: &egui::Painter, cx: &Cx<'_>, skin: &Skin) {
    crate::theme::paint_elevation(
        painter,
        cx.theme,
        skin.rect,
        skin.corners,
        Elevation::Floating,
    );
    painter.rect_filled(skin.rect, skin.corners, skin.ground);
    if let Some(stroke) = skin.stroke {
        painter.rect_stroke(skin.rect, skin.corners, stroke, StrokeKind::Inside);
    }
    if let Some(line) = skin.underline {
        let y = skin.rect.max.y - line.width * 0.5;
        painter.line_segment(
            [Pos2::new(skin.rect.min.x, y), Pos2::new(skin.rect.max.x, y)],
            line,
        );
    }
}

/// The hairline between the head and the rows, on whichever side the rows are: the head is not a
/// row.
fn divider(painter: &egui::Painter, t: &Tokens, skin: &Skin, head: Rect, ink: Ink) {
    let y = if skin.below {
        head.max.y - t.hairline * 0.5
    } else {
        head.min.y + t.hairline * 0.5
    };
    painter.line_segment(
        [Pos2::new(head.min.x, y), Pos2::new(head.max.x, y)],
        Stroke::new(t.hairline, ink.outline),
    );
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
fn panel(ui: &egui::Ui, id: egui::Id, rect: Rect, clip: Rect, add: impl FnOnce(&mut egui::Ui)) {
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

/// The panel's own body: its elevation and its opaque ground.
fn paint_panel(
    painter: &egui::Painter,
    cx: &Cx<'_>,
    rect: Rect,
    corners: CornerRadius,
    ground: Color32,
) {
    crate::theme::paint_elevation(painter, cx.theme, rect, corners, Elevation::Floating);
    painter.rect_filled(rect, corners, ground);
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
    (out.state.offset.y, out.content_size.y)
}

/// egui's own single-line edit with no frame of its own — the head is its frame — in `edit`,
/// showing `hint` while nothing is typed.
///
/// **It takes no focus of its own**. The list opens to be read first, the way any
/// dropdown does; the keyboard comes when the field is tapped. Focus on opening raised the
/// keyboard over the lower half of the pane the moment the list appeared — a search box, not a
/// dropdown that can also be searched.
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

/// The corners a row's pressed fill rounds: the ones it shares with the list.
fn row_corners(index: usize, last: usize, radius: CornerRadius) -> CornerRadius {
    match (index == 0, index == last) {
        (true, true) => radius,
        (true, false) => CornerRadius {
            nw: radius.nw,
            ne: radius.ne,
            sw: 0,
            se: 0,
        },
        (false, true) => CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius.sw,
            se: radius.se,
        },
        (false, false) => CornerRadius::ZERO,
    }
}

/// The cut edges of a scrolling list say so: a fade of the list's own ground at whichever end
/// has more rows behind it. `scroll` is the offset and the content height.
fn edge_fades(
    painter: &egui::Painter,
    rect: Rect,
    ground: Color32,
    depth: f32,
    scroll: (f32, f32),
) {
    let (offset, content_h) = scroll;
    if offset > 1.0 {
        fade(
            painter,
            Rect::from_min_size(rect.min, Vec2::new(rect.width(), depth)),
            ground,
            true,
        );
    }
    if offset + rect.height() + 1.0 < content_h {
        fade(
            painter,
            Rect::from_min_size(
                Pos2::new(rect.min.x, rect.max.y - depth),
                Vec2::new(rect.width(), depth),
            ),
            ground,
            false,
        );
    }
}

/// The width of `text` on one line in `font`.
fn text_width(ui: &egui::Ui, text: &str, font: &egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::PLACEHOLDER)
        .size()
        .x
}

/// One line, cut with an ellipsis where it would run past `max_width`.
fn truncated(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    colour: Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = TextWrapping::truncate_at_width(max_width.max(8.0));
    ui.painter().layout_job(job)
}

/// A band of `colour` fading to clear — solid at the top when `from_top`, at the bottom when
/// not — as a two-triangle mesh, the way the chip lane fades.
fn fade(painter: &egui::Painter, band: Rect, colour: Color32, from_top: bool) {
    if band.height() <= 0.0 || band.width() <= 0.0 {
        return;
    }
    let clear = Color32::TRANSPARENT;
    let (top, bottom) = if from_top {
        (colour, clear)
    } else {
        (clear, colour)
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(band.left_top(), top);
    mesh.colored_vertex(band.right_top(), top);
    mesh.colored_vertex(band.right_bottom(), bottom);
    mesh.colored_vertex(band.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
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
