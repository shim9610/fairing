//! `WheelPicker` — a drum of **ordered** values, the centre row chosen.
//!
//! # Why this is not a `Dropdown`
//!
//! A list is for names: the operator reads it. A drum is for values with an order — an hour, a
//! quantity, a year — that the operator *feels*: a flick moves further than a drag, the nearest
//! row is always the one under the window, and the ends can be joined so 23 rolls into 00. A
//! dropdown promises a tap on a row; a drum promises that letting go always lands on a row.
//! Those are different contracts, which is why the dropdown survey put the wheel in
//! its own widget rather than in [`Opener`](super::Opener). Apple's guidance is the same: a
//! picker for ordered, predictable values, a table for a long list of names.
//!
//! # What it draws
//!
//! An odd number of rows, five by default, each a full control height so a gloved finger lands
//! on one. The **window** — the centre row — wears the control face, and the rows fade with
//! their distance from it, which is the drum's curvature drawn as ink rather than as geometry:
//! egui has no cylinder, and a row that shrinks as it turns away would also move its own hit
//! area. The value is set in the heading size, because a wheel is read from where the hand is,
//! not from where the eye is, and centred, because the drum is a column.
//!
//! # How it moves
//!
//! The drum's position is an [`Animated`] value in rows, kept in egui's own memory under the
//! widget's id, so a closure screen gets it too. A drag follows the finger 1:1; a release
//! projects the finger's velocity `FLING_SECS` ahead, rounds to the nearest row and settles
//! there on the theme's spring, with the settle decision scaled from pixels into rows. A tap on
//! a row that is not the centre turns the drum to it over `motion.switch`, which is what a
//! finger that missed the window meant. Without `wrap` the drum stops at its ends; with it the
//! position is unbounded and the row is the position modulo the count.
//!
//! The chosen index is **the row under the window right now**, so `changed()` fires as the drum
//! passes each row and not only when it settles — a clock that ticks as the hour wheel turns
//! beats one that jumps when it stops.
//!
//! A value the **caller** sets is the other way round: the drum turns to it (the short way on a
//! joined drum) and the value stays what the caller wrote, with no `changed()` for the rows
//! the turn passes. An index past the end is pulled onto the drum — the last row, or the same
//! row modulo the count when joined — and that correction is reported as a change.

use super::{WheelLook, WheelRow};
use crate::cx::WidgetCx as Cx;
use crate::motion::{Animated, Mode};
use crate::theme::ColorRole;
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Rect, Response, Sense, Vec2};

/// How far ahead a release projects the finger's velocity before it rounds to a row.
const FLING_SECS: f32 = 0.18;
/// The default number of visible rows. Odd, so the window is one row.
const ROWS: u8 = 5;
/// The dimmest a row gets at the drum's edge.
const EDGE_ALPHA: f32 = 0.15;

/// A drum of ordered values.
#[derive(Debug)]
pub struct WheelPicker<'a> {
    id_salt: &'a str,
    options: &'a [&'a str],
    selected: &'a mut usize,
    rows: u8,
    wrap: bool,
    width: Option<f32>,
    enabled: bool,
}

impl<'a> WheelPicker<'a> {
    /// The values in their order, and the index of the chosen one. The salt names the drum.
    #[must_use]
    pub fn new(id_salt: &'a str, options: &'a [&'a str], selected: &'a mut usize) -> Self {
        Self {
            id_salt,
            options,
            selected,
            rows: ROWS,
            wrap: false,
            width: None,
            enabled: true,
        }
    }

    /// How many rows show. Made odd, so the window is one row; at least one.
    #[must_use]
    pub const fn rows(mut self, rows: u8) -> Self {
        self.rows = if rows.is_multiple_of(2) {
            rows.saturating_add(1)
        } else {
            rows
        };
        self
    }

    /// Join the ends: past the last value comes the first. For an hour, a minute, a month.
    #[must_use]
    pub const fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// The width (du). Without it, all the remaining width.
    #[must_use]
    pub const fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Enabled. Disabled it senses `hover()` and is faded.
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw it. On a frame the row under the window changed, `response.changed()`.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let row_h = crate::theme::control_height(&cx.theme.metrics, &cx.theme.control);
        let rows = self.rows.max(1);
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let (rect, mut response) = ui.allocate_exact_size(
            Vec2::new(width, row_h * f32::from(rows)),
            if self.enabled {
                Sense::click_and_drag()
            } else {
                Sense::hover()
            },
        );
        // The drum follows the finger, and says so: the rail's page swipe leaves this drag alone.
        crate::drag::claim_if_held(&response);
        let count = self.options.len();
        if count == 0 {
            return response;
        }
        // An index past the end — a stale one, or a "nothing chosen" sentinel — is pulled onto
        // the drum: the last row on an open drum, the same row modulo the count on a joined one.
        let wanted = self.onto_drum(*self.selected, count);
        if wanted != *self.selected {
            *self.selected = wanted;
            response.mark_changed();
        }
        let id = ui.id().with(self.id_salt);
        let mut drum: Drum = ui.data(|d| d.get_temp::<Drum>(id)).unwrap_or(Drum {
            pos: Animated::new(as_f32(wanted)),
            caller: false,
        });

        // The caller moved the value while the drum was at rest, or while it was still turning
        // to the caller's last value: turn to it, the short way round on a joined drum.
        let resting = drum.pos.mode() == Mode::Idle || drum.caller;
        if resting && index_at(drum.pos.target(), count, self.wrap) != wanted {
            let target = self.turn_to(drum.pos.value(), wanted, count);
            drum.pos.to(target, cx.theme.motion.switch);
            drum.caller = true;
        }
        if self.enabled && self.steer(ui, cx, &response, rect, row_h, &mut drum.pos) {
            // The finger took over: from here the rows passed are the operator's choice.
            drum.caller = false;
        }
        let dt = ui.input(|i| i.stable_dt);
        if drum.pos.tick(dt) {
            ui.ctx().request_repaint();
        }

        // A turn to the caller's value keeps that value: the rows it passes on the way are not
        // changes the operator made.
        if !drum.caller {
            let index = index_at(drum.pos.value(), count, self.wrap);
            if index != *self.selected {
                *self.selected = index;
                response.mark_changed();
            }
        }
        if drum.pos.mode() == Mode::Idle {
            drum.caller = false;
        }
        ui.data_mut(|d| d.insert_temp(id, drum));
        self.paint(ui, cx, rect, row_h, drum.pos.value());
        response
    }

    /// `index` as a row of this drum: held to the last row when open, rolled round when joined.
    fn onto_drum(&self, index: usize, count: usize) -> usize {
        if self.wrap {
            index.checked_rem(count).unwrap_or(0)
        } else {
            index.min(count.saturating_sub(1))
        }
    }

    /// The drum position that shows row `to`, starting from position `pos`: the row itself on an
    /// open drum, the nearer way round on a joined one (23 to 00 is one row, not twenty-three).
    fn turn_to(&self, pos: f32, to: usize, count: usize) -> f32 {
        if !self.wrap {
            return as_f32(to);
        }
        let from = index_at(pos, count, true);
        let ahead = if to >= from {
            to - from
        } else {
            count - (from - to)
        };
        let steps = if ahead <= count / 2 {
            as_f32(ahead)
        } else {
            -as_f32(count - ahead)
        };
        pos.round() + steps
    }

    /// The rows within reach of the window at drum position `pos`: one row past the half shown
    /// on each side, so a row turning in is drawn before it shows.
    fn rows_at(&self, rect: Rect, row_h: f32, pos: f32) -> Vec<WheelRow> {
        let half = self.rows.max(1) / 2;
        let reach = f32::from(half) + 1.0;
        let count = self.options.len();
        let first = (pos - reach).floor();
        let mut rows = Vec::new();
        // At most `2 · reach + 1` rows are in reach. Counting them, rather than stepping a float
        // until it passes the far side, ends even where adding one no longer moves the float.
        for step in 0..u16::from(half).saturating_mul(2).saturating_add(3) {
            let k = first + f32::from(step);
            if k > pos + reach {
                break;
            }
            if let Some(index) = Row::at(k, count, self.wrap) {
                let y = (k - pos).mul_add(row_h, rect.center().y);
                rows.push(WheelRow {
                    index,
                    rect: Rect::from_center_size(
                        egui::pos2(rect.center().x, y),
                        Vec2::new(rect.width(), row_h),
                    ),
                    distance: (k - pos).abs(),
                });
            }
        }
        rows
    }

    /// The finger's say: a drag follows it, a release settles on a row, a tap turns to a row.
    /// Whether the finger moved the drum this frame.
    fn steer(
        &self,
        ui: &egui::Ui,
        cx: &Cx<'_>,
        response: &Response,
        rect: Rect,
        row_h: f32,
        drum: &mut Animated<f32>,
    ) -> bool {
        let count = self.options.len();
        let mut moved = false;
        let last = as_f32(count.saturating_sub(1));
        if response.dragged() {
            // Up moves the drum to later rows, the way a page scrolls.
            let mut pos = drum.value() - response.drag_delta().y / row_h;
            if !self.wrap {
                pos = pos.clamp(0.0, last);
            }
            let velocity = -ui.input(|i| i.pointer.velocity().y) / row_h;
            drum.drag(pos, velocity);
            moved = true;
        }
        if response.drag_stopped() {
            let target = landing(drum.value(), drum.velocity(), count, self.wrap);
            // The settle decision is in px; the value is in rows.
            drum.release_scaled(target, cx.theme.motion.spring, 1.0 / row_h);
            moved = true;
        }
        if response.clicked() {
            if let Some(at) = response.interact_pointer_pos() {
                let k = ((at.y - rect.center().y) / row_h).round();
                let mut target = drum.value().round() + k;
                if !self.wrap {
                    target = target.clamp(0.0, last);
                }
                if (target - drum.value()).abs() > 0.01 {
                    drum.to(target, cx.theme.motion.switch);
                    moved = true;
                }
            }
        }
        moved
    }

    /// The window, then every row within reach of it, faded with its distance — a painter's
    /// drawing where one is given, the built-in one where not. Either way it is clipped
    /// to the drum.
    fn paint(&self, ui: &egui::Ui, cx: &mut Cx<'_>, rect: Rect, row_h: f32, pos: f32) {
        let painter = ui.painter().with_clip_rect(rect);
        let centre = rect.center();
        let window = Rect::from_center_size(centre, Vec2::new(rect.width(), row_h));
        if let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.wheel.as_mut()) {
            let rows = self.rows_at(rect, row_h, pos);
            let count = self.options.len();
            custom(
                &painter,
                &mut WheelLook {
                    rect,
                    window,
                    options: self.options,
                    rows: &rows,
                    selected: index_at(pos, count, self.wrap),
                    wrap: self.wrap,
                    enabled: self.enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            return;
        }
        let ink = Ink::of(cx, self.enabled);
        painter.rect_filled(
            window,
            CornerRadius::same(round_u8(cx.theme.metrics.control_radius)),
            ink.window,
        );
        let font = egui::FontId::proportional(cx.theme.metrics.type_scale.heading);
        let reach = f32::from(self.rows.max(1) / 2) + 1.0;
        for row in self.rows_at(rect, row_h, pos) {
            let alpha = (1.0 - row.distance / reach).clamp(EDGE_ALPHA, 1.0);
            let text = self.options.get(row.index).copied().unwrap_or_default();
            painter.text(
                row.rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                font.clone(),
                ink.label.gamma_multiply(alpha),
            );
        }
    }
}

/// The drum's memory between frames, in egui's memory under the widget's id.
#[derive(Debug, Clone, Copy)]
struct Drum {
    /// The position in rows.
    pos: Animated<f32>,
    /// Turning to a value the caller set, not one the finger chose.
    caller: bool,
}

/// A row's index for a drum position: the position modulo the count with `wrap`, and only the
/// positions that are rows without it.
struct Row;

impl Row {
    fn at(k: f32, count: usize, wrap: bool) -> Option<usize> {
        if count == 0 {
            return None;
        }
        if wrap {
            Some(index_at(k, count, true))
        } else if k >= 0.0 && k <= as_f32(count.saturating_sub(1)) {
            Some(index_at(k, count, false))
        } else {
            None
        }
    }
}

/// The count as a float, exactly. A drum past sixty-five thousand rows is not a drum.
fn as_f32(n: usize) -> f32 {
    u16::try_from(n).map_or(f32::MAX, f32::from)
}

/// The row under the window for a drum position.
fn index_at(pos: f32, count: usize, wrap: bool) -> usize {
    if count == 0 {
        return 0;
    }
    let n = as_f32(count);
    let k = pos.round();
    let k = if wrap {
        k.rem_euclid(n)
    } else {
        k.clamp(0.0, n - 1.0)
    };
    // In range by the line above; the cast is the only way back to an index.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = k as usize;
    index.min(count - 1)
}

/// Where a released drum lands: the finger's velocity projected `FLING_SECS` ahead, rounded to
/// a row, and kept to the drum's ends where it has them.
fn landing(pos: f32, velocity: f32, count: usize, wrap: bool) -> f32 {
    let projected = velocity.mul_add(FLING_SECS, pos).round();
    if wrap {
        projected
    } else {
        projected.clamp(0.0, as_f32(count.saturating_sub(1)))
    }
}

/// The colours one frame paints with, faded once at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    window: Color32,
    label: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, enabled: bool) -> Self {
        let tint = cx.theme.color(ColorRole::OnSurface);
        let ink = Self {
            window: tint.gamma_multiply(cx.theme.control.fill_alpha),
            label: tint,
        };
        if enabled {
            ink
        } else {
            let a = cx.theme.control.disabled_alpha;
            Self {
                window: ink.window.gamma_multiply(a),
                label: ink.label.gamma_multiply(a),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{index_at, landing, Row};

    /// **The row under the window is the nearest one**, held to the ends without `wrap` and
    /// rolled round with it.
    #[test]
    fn the_window_shows_the_nearest_row_and_the_ends_join_only_when_asked() {
        assert_eq!(index_at(2.4, 10, false), 2);
        assert_eq!(index_at(2.6, 10, false), 3);
        assert_eq!(
            index_at(-3.0, 10, false),
            0,
            "past the start without wrap is the start"
        );
        assert_eq!(
            index_at(40.0, 10, false),
            9,
            "past the end without wrap is the end"
        );
        assert_eq!(
            index_at(-1.0, 24, true),
            23,
            "one before 00 is 23 on a joined drum"
        );
        assert_eq!(
            index_at(24.0, 24, true),
            0,
            "one after 23 is 00 on a joined drum"
        );
        assert_eq!(index_at(5.0, 0, true), 0, "an empty drum is not a panic");
    }

    /// **A flick carries further than a drag**, and a slow release lands on the nearest row.
    #[test]
    fn a_release_lands_on_a_row_where_the_velocity_points() {
        assert!((landing(2.3, 0.0, 10, false) - 2.0).abs() < f32::EPSILON);
        assert!(
            landing(2.0, 20.0, 10, false) >= 5.0,
            "twenty rows a second carries past the next row"
        );
        assert!(
            (landing(8.0, 40.0, 10, false) - 9.0).abs() < f32::EPSILON,
            "a flick past the end stops at the end"
        );
        assert!(
            landing(22.0, 20.0, 24, true) > 23.0,
            "a joined drum rolls on past its last row"
        );
    }

    /// **Only real rows are drawn** on an open drum; a joined one always has a row.
    #[test]
    fn rows_past_the_ends_are_left_out_unless_the_drum_is_joined() {
        assert_eq!(Row::at(-1.0, 10, false), None);
        assert_eq!(Row::at(10.0, 10, false), None);
        assert_eq!(Row::at(9.0, 10, false), Some(9));
        assert_eq!(Row::at(-1.0, 10, true), Some(9));
    }
}
