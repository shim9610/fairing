//! `PatternPad` — a path drawn through a grid of dots, the other way into the unlock prompt.
//!
//! # The crate owns no secret
//!
//! Like [`PinPad`](super::PinPad) it borrows the caller's buffer — a `&mut Vec<u8>` of dot
//! numbers, counted row by row from 0 at the top left. The path goes there and nowhere else:
//! between frames the widget remembers whether a finger is drawing and where it last was, never
//! which dots it took.
//!
//! # A dot passed over is a dot taken
//!
//! A straight stroke from one dot to another through a third takes the third on the way, as every
//! pattern lock does — a finger cannot jump a dot it is dragged across
//! ([`PatternPad::passed_over`]). The stroke is followed through **every pointer position the
//! frame carried**, not the last one alone, so a quick stroke on a slow frame takes the dots it
//! crossed rather than the chord between two samples.
//!
//! # Too short is not wrong
//!
//! A path under [`PatternPad::min_points`] is not submitted: the response says `too_short`, and
//! the caller asks for more dots without spending an attempt on it.
//!
//! # Why the cells are as big as they are
//!
//! A cell — the square round one dot — is never under a touch target, and grows to `CELL_MAX`
//! targets where there is room. A dot takes the finger `HIT_RATIO` of a cell from its centre:
//! room for a glove, and still narrow enough that a stroke between two dots past a third's side
//! leaves the third alone.

use super::PatternPadLook;
use crate::cx::WidgetCx as Cx;
use crate::theme::{control_height, ColorRole};
use egui::emath::TSTransform;
use egui::{Pos2, Rect, Response, Sense, Shape, Stroke, Vec2};

/// The fewest dots on a side.
pub const MIN_GRID: u8 = 3;

/// The most dots on a side — twenty-five dots, every cell still a touch target on a panel the
/// size of a phone.
pub const MAX_GRID: u8 = 5;

/// A cell's side, in touch targets, at most.
const CELL_MAX: f32 = 2.0;

/// How far from its centre a dot takes the finger, over the cell's side.
const HIT_RATIO: f32 = 0.3;

/// A resting dot's diameter over `control.mark_size`.
const DOT_RATIO: f32 = 0.4;

/// A taken dot's diameter over a resting one's.
const TAKEN_GROW: f32 = 1.5;

/// The line's width over a resting dot's diameter.
const LINE_RATIO: f32 = 0.5;

/// The line's alpha — the dots it joins stay readable through it.
const LINE_ALPHA: f32 = 0.6;

/// The halo round a taken dot: its alpha, drawn out to the radius that took the finger.
const HALO_ALPHA: f32 = 0.16;

/// What the pad reported this frame.
#[derive(Debug)]
pub struct PatternPadResponse {
    /// The whole pad.
    pub response: Response,
    /// A dot was taken, or a new stroke cleared the old path.
    pub changed: bool,
    /// The finger lifted on a path of at least [`PatternPad::min_points`] dots — the caller's to
    /// take.
    pub submitted: bool,
    /// The finger lifted on a path too short to submit. The path is left in the buffer, so the
    /// caller can show it as it says why.
    pub too_short: bool,
    /// Where each dot was drawn, in dot order.
    centers: Vec<Pos2>,
}

impl PatternPadResponse {
    /// Where dot `dot` was drawn this frame — for a test drawing a pattern the way a finger
    /// would.
    #[must_use]
    pub fn dot_center(&self, dot: u8) -> Option<Pos2> {
        self.centers.get(usize::from(dot)).copied()
    }
}

/// A pattern pad: a square of dots, and the path a finger draws through them.
///
/// ```no_run
/// # fn ui(ui: &mut egui::Ui, cx: &mut fairing_widgets::WidgetCx<'_>, path: &mut Vec<u8>) {
/// use fairing_widgets::widgets::PatternPad;
///
/// let pad = PatternPad::new(path).grid(3).min_points(4).show(ui, cx);
/// if pad.submitted {
///     let drawn = std::mem::take(path);
///     // hand `drawn` to whatever checks it
/// #   let _ = drawn;
/// }
/// # }
/// ```
pub struct PatternPad<'a> {
    path: &'a mut Vec<u8>,
    grid: u8,
    min_points: u8,
    enabled: bool,
    show_path: bool,
    mark: Option<ColorRole>,
}

impl std::fmt::Debug for PatternPad<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The path is a secret; only its length is anyone's business.
        f.debug_struct("PatternPad")
            .field("dots", &self.path.len())
            .field("grid", &self.grid)
            .field("min_points", &self.min_points)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl<'a> PatternPad<'a> {
    /// A pad drawing into `path`: three dots a side, and four to submit.
    pub fn new(path: &'a mut Vec<u8>) -> Self {
        Self {
            path,
            grid: MIN_GRID,
            min_points: 4,
            enabled: true,
            show_path: true,
            mark: None,
        }
    }

    /// Dots on a side, [`MIN_GRID`] to [`MAX_GRID`].
    #[must_use]
    pub fn grid(mut self, grid: u8) -> Self {
        self.grid = grid.clamp(MIN_GRID, MAX_GRID);
        self
    }

    /// The fewest dots a path submits with — four by default, as most pattern locks ask. Never
    /// under one, and never over the dots there are.
    #[must_use]
    pub fn min_points(mut self, min_points: u8) -> Self {
        self.min_points = min_points.max(1);
        self
    }

    /// Greyed and deaf — the authenticator's lockout.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw the path as the finger draws it (the default). Off, the dots stay at rest while the
    /// finger takes them — nothing on the glass for a glance over the shoulder to read, the
    /// pattern's counterpart of a shuffled keypad.
    #[must_use]
    pub fn show_path(mut self, show_path: bool) -> Self {
        self.show_path = show_path;
        self
    }

    /// The colour the path is drawn in, over `Primary` — `Danger` for one that was refused.
    #[must_use]
    pub fn mark(mut self, mark: Option<ColorRole>) -> Self {
        self.mark = mark;
        self
    }

    /// The dots strictly between `from` and `to` on the straight line joining them, from `from`
    /// on — the ones a stroke between the two crosses dead centre. Empty for neighbours, for a
    /// knight's move, and for a dot off the grid.
    #[must_use]
    pub fn passed_over(from: u8, to: u8, grid: u8) -> Vec<u8> {
        let grid = grid.clamp(MIN_GRID, MAX_GRID);
        let dots = grid * grid;
        if from >= dots || to >= dots || from == to {
            return Vec::new();
        }
        let g = i16::from(grid);
        let (r0, c0) = (i16::from(from) / g, i16::from(from) % g);
        let (r1, c1) = (i16::from(to) / g, i16::from(to) % g);
        let (dr, dc) = (r1 - r0, c1 - c0);
        // At most four steps on a side of five, so the conversion is exact.
        let steps = i16::try_from(gcd(dr.unsigned_abs(), dc.unsigned_abs())).unwrap_or(1);
        (1..steps)
            .filter_map(|k| {
                let (r, c) = (r0 + dr / steps * k, c0 + dc / steps * k);
                u8::try_from(r * g + c).ok()
            })
            .collect()
    }

    /// **The path a finger drawn through `dots` in order records**: a dot crossed on the way is
    /// taken where it is crossed, and a dot taken once is not taken again. A path equal to its
    /// own `as_drawn` is one a finger can draw — what a stored pattern is checked against.
    #[must_use]
    pub fn as_drawn(dots: &[u8], grid: u8) -> Vec<u8> {
        let grid = grid.clamp(MIN_GRID, MAX_GRID);
        let mut path = Vec::with_capacity(dots.len());
        for &dot in dots {
            take(&mut path, dot, grid);
        }
        path
    }

    /// **The size the pad takes in `room`** with `grid` dots a side, without drawing it — for a
    /// caller laying a card out around it. [`PatternPad::show`] allocates exactly this in
    /// `ui.available_size()`.
    #[must_use]
    pub fn measure(cx: &Cx<'_>, room: Vec2, grid: u8) -> Vec2 {
        Geometry::of(cx, room, grid.clamp(MIN_GRID, MAX_GRID)).size()
    }

    /// The fewest dots that submit, inside what the grid holds.
    fn min_points_in_grid(&self) -> usize {
        usize::from(self.min_points).clamp(1, usize::from(self.grid * self.grid))
    }

    /// Draw it.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> PatternPadResponse {
        let geometry = Geometry::of(cx, ui.available_size(), self.grid);
        let sense = if self.enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(geometry.size(), sense);
        // The path follows the finger, and says so: the rail's page swipe leaves this drag alone.
        crate::drag::claim_if_held(&response);
        let centers = geometry.centers(rect);
        let hit = geometry.cell * HIT_RATIO;
        let trace_id = response.id.with("pattern.trace");
        let mut trace: Trace = ui.data(|d| d.get_temp(trace_id)).unwrap_or_default();
        let before = self.path.len();
        let mut cleared = false;
        let (mut submitted, mut too_short) = (false, false);
        if self.enabled {
            let held = response.is_pointer_button_down_on();
            // The events are in screen space, and the pad may sit in a scaled or shaken layer —
            // the unlock card does.
            let to_local = ui
                .ctx()
                .layer_transform_from_global(ui.layer_id())
                .unwrap_or(TSTransform::IDENTITY);
            let pass = ui.input(Pass::read);
            if !trace.drawing && (held || response.clicked()) {
                let start = pass
                    .press
                    .map(|p| to_local * p)
                    .or_else(|| response.interact_pointer_pos());
                if let Some(start) = start {
                    cleared = !self.path.is_empty();
                    self.path.clear();
                    trace = Trace {
                        drawing: true,
                        last: start,
                    };
                    sweep(self.path, start, start, &centers, hit, self.grid);
                }
            }
            if trace.drawing {
                for point in pass.points.iter().map(|p| to_local * *p) {
                    sweep(self.path, trace.last, point, &centers, hit, self.grid);
                    trace.last = point;
                }
                if !held {
                    trace.drawing = false;
                    let len = self.path.len();
                    submitted = len >= self.min_points_in_grid();
                    too_short = !submitted && len > 0;
                }
            }
        } else {
            trace.drawing = false;
        }
        ui.data_mut(|d| d.insert_temp(trace_id, trace));
        let changed = cleared || self.path.len() != before;
        self.paint(ui, cx, response.id, (rect, &centers), hit, trace);
        PatternPadResponse {
            response,
            changed,
            submitted,
            too_short,
            centers,
        }
    }

    /// The halos, the line, then the dots over it — or a painter's drawing. `pad` is
    /// the pad's rect and `centers` its dots.
    fn paint(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        id: egui::Id,
        (pad, centers): (Rect, &[Pos2]),
        hit: f32,
        trace: Trace,
    ) {
        let theme = cx.theme;
        let motion = theme.motion;
        let alpha = if self.enabled {
            1.0
        } else {
            theme.control.disabled_alpha
        };
        let dot = theme.control.mark_size * DOT_RATIO;
        let rest = theme.color(ColorRole::Muted).gamma_multiply(alpha);
        let ink = theme
            .color(self.mark.unwrap_or(ColorRole::Primary))
            .gamma_multiply(alpha);
        let taken: Vec<f32> = (0u8..)
            .zip(centers)
            .map(|(i, _)| {
                let on = self.show_path && self.path.contains(&i);
                let tween = if on {
                    motion.press
                } else {
                    motion.press_release
                };
                cx.animate(
                    id.with(("pattern.dot", i)),
                    if on { 1.0 } else { 0.0 },
                    tween,
                )
            })
            .collect();
        if let Some(custom) = cx
            .painters
            .as_deref_mut()
            .and_then(|p| p.pattern_pad.as_mut())
        {
            // A path kept off the glass is kept from the painter too.
            let (path, finger): (&[u8], _) = if self.show_path {
                (self.path, trace.drawing.then_some(trace.last))
            } else {
                (&[], None)
            };
            custom(
                ui.painter(),
                &mut PatternPadLook {
                    rect: pad,
                    dots: centers,
                    dot,
                    reach: hit,
                    taken: &taken,
                    path,
                    finger,
                    mark: self.mark,
                    enabled: self.enabled,
                    theme,
                    icons: &mut *cx.icons,
                },
            );
            return;
        }
        let painter = ui.painter();
        for (centre, t) in centers.iter().zip(&taken) {
            if *t > 0.0 {
                painter.circle_filled(*centre, hit * t, ink.gamma_multiply(HALO_ALPHA * t));
            }
        }
        if self.show_path {
            let mut points: Vec<Pos2> = self
                .path
                .iter()
                .filter_map(|&d| centers.get(usize::from(d)).copied())
                .collect();
            if trace.drawing && !points.is_empty() {
                points.push(trace.last);
            }
            if points.len() > 1 {
                painter.add(Shape::line(
                    points,
                    Stroke::new(dot * LINE_RATIO, ink.gamma_multiply(LINE_ALPHA)),
                ));
            }
        }
        for (centre, t) in centers.iter().zip(&taken) {
            let r = dot / 2.0 * (1.0 + (TAKEN_GROW - 1.0) * t);
            painter.circle_filled(*centre, r, rest.lerp_to_gamma(ink, *t));
        }
    }
}

/// Whether a finger is drawing on the pad, and where it was last seen — all the pad keeps
/// between frames.
#[derive(Debug, Clone, Copy, Default)]
struct Trace {
    drawing: bool,
    last: Pos2,
}

/// The pointer this pass, in the order it happened: where the last press went down, and every
/// position after it — each move, and the release.
#[derive(Debug, Default)]
struct Pass {
    press: Option<Pos2>,
    points: Vec<Pos2>,
}

impl Pass {
    fn read(input: &egui::InputState) -> Self {
        let mut pass = Self::default();
        for event in &input.events {
            match event {
                egui::Event::PointerMoved(pos) => pass.points.push(*pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    ..
                } => {
                    if *pressed {
                        pass.press = Some(*pos);
                        pass.points.clear();
                    } else {
                        pass.points.push(*pos);
                    }
                }
                _ => {}
            }
        }
        pass
    }
}

/// The pad's lengths in a given room — read by [`PatternPad::measure`] and
/// [`PatternPad::show`] alike.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    cell: f32,
    grid: u8,
}

impl Geometry {
    fn of(cx: &Cx<'_>, room: Vec2, grid: u8) -> Self {
        let theme = cx.theme;
        let target = control_height(&theme.metrics, &theme.control);
        let side = if room.y.is_finite() {
            room.x.min(room.y)
        } else {
            room.x
        };
        let cell = (side / f32::from(grid)).clamp(target, target * CELL_MAX);
        Self { cell, grid }
    }

    fn size(self) -> Vec2 {
        Vec2::splat(self.cell * f32::from(self.grid))
    }

    /// Each dot's centre in `rect`, row by row.
    fn centers(self, rect: Rect) -> Vec<Pos2> {
        let n = self.grid;
        (0..n * n)
            .map(|i| {
                let (row, col) = (f32::from(i / n), f32::from(i % n));
                rect.min + Vec2::new(col + 0.5, row + 0.5) * self.cell
            })
            .collect()
    }
}

fn gcd(mut a: u16, mut b: u16) -> u16 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Take `dot` onto `path` — and first any dot not yet taken that the stroke to it crosses.
fn take(path: &mut Vec<u8>, dot: u8, grid: u8) {
    if dot >= grid * grid || path.contains(&dot) {
        return;
    }
    if let Some(&last) = path.last() {
        for between in PatternPad::passed_over(last, dot, grid) {
            if !path.contains(&between) {
                path.push(between);
            }
        }
    }
    path.push(dot);
}

/// Take every dot the stroke from `a` to `b` comes within `hit` of, in the order it reaches them.
fn sweep(path: &mut Vec<u8>, a: Pos2, b: Pos2, centers: &[Pos2], hit: f32, grid: u8) {
    let d = b - a;
    let len_sq = d.length_sq();
    let mut crossed: Vec<(f32, u8)> = (0u8..)
        .zip(centers)
        .filter_map(|(i, c)| {
            let t = if len_sq > 0.0 {
                ((*c - a).dot(d) / len_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            ((a + d * t).distance(*c) <= hit).then_some((t, i))
        })
        .collect();
    crossed.sort_by(|x, y| x.0.total_cmp(&y.0));
    for (_, dot) in crossed {
        take(path, dot, grid);
    }
}

#[cfg(test)]
mod tests {
    use super::{sweep, PatternPad};
    use egui::{pos2, Pos2};

    /// A 3 × 3 grid of cell 100, dot `i` at the centre of its cell.
    fn centers() -> Vec<Pos2> {
        (0..9_u8)
            .map(|i| {
                pos2(
                    f32::from(i % 3) * 100.0 + 50.0,
                    f32::from(i / 3) * 100.0 + 50.0,
                )
            })
            .collect()
    }

    #[test]
    fn a_straight_line_passes_over_the_dots_on_it() {
        assert_eq!(PatternPad::passed_over(0, 2, 3), vec![1]);
        assert_eq!(PatternPad::passed_over(0, 8, 3), vec![4]);
        assert_eq!(PatternPad::passed_over(6, 0, 3), vec![3]);
        assert_eq!(PatternPad::passed_over(2, 6, 3), vec![4]);
        assert!(
            PatternPad::passed_over(0, 5, 3).is_empty(),
            "a knight's move"
        );
        assert!(PatternPad::passed_over(0, 1, 3).is_empty(), "neighbours");
        assert!(PatternPad::passed_over(0, 9, 3).is_empty(), "off the grid");
        assert_eq!(PatternPad::passed_over(0, 15, 4), vec![5, 10]);
        assert_eq!(PatternPad::passed_over(0, 24, 5), vec![6, 12, 18]);
        // Two down and four across on a side of five crosses the one dot halfway.
        assert_eq!(PatternPad::passed_over(0, 14, 5), vec![7]);
    }

    #[test]
    fn as_drawn_takes_what_a_stroke_crosses_and_nothing_twice() {
        assert_eq!(PatternPad::as_drawn(&[0, 2, 8], 3), vec![0, 1, 2, 5, 8]);
        assert_eq!(
            PatternPad::as_drawn(&[1, 0, 2], 3),
            vec![1, 0, 2],
            "over a dot already taken"
        );
        assert_eq!(PatternPad::as_drawn(&[0, 0, 1, 9], 3), vec![0, 1]);
    }

    #[test]
    fn a_stroke_takes_the_dots_it_crosses_in_order() {
        let c = centers();
        let mut path = Vec::new();
        // One sample at each end of the diagonal: the middle is taken all the same.
        sweep(&mut path, pos2(50.0, 50.0), pos2(250.0, 250.0), &c, 30.0, 3);
        assert_eq!(path, vec![0, 4, 8]);
        // A knight's move goes between its neighbours and takes neither.
        let mut path = Vec::new();
        sweep(&mut path, pos2(50.0, 50.0), pos2(250.0, 150.0), &c, 30.0, 3);
        assert_eq!(path, vec![0, 5]);
        // An L in two samples takes the corner it turned at.
        let mut path = Vec::new();
        sweep(&mut path, pos2(50.0, 50.0), pos2(250.0, 50.0), &c, 30.0, 3);
        sweep(
            &mut path,
            pos2(250.0, 50.0),
            pos2(250.0, 250.0),
            &c,
            30.0,
            3,
        );
        assert_eq!(path, vec![0, 1, 2, 5, 8]);
    }

    #[test]
    fn its_debug_shows_the_count_and_not_the_dots() {
        let mut path = vec![4, 7, 1];
        let pad = PatternPad::new(&mut path);
        let printed = format!("{pad:?}");
        assert!(printed.contains("dots: 3") && !printed.contains("[4, 7, 1]"));
    }
}
