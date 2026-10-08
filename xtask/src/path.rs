//! The parser for an SVG `path`'s `d` attribute, and the arc (A) → cubic Bézier conversion.
//!
//! The commands supported: `M L H V C S Q T A Z`, absolute and relative. The coordinate separator may
//! be a space or a comma, and it takes the run-together spellings that use only a sign or a decimal
//! point (`10-5`, `.5.5`) and the arc spellings with the flags attached (`a5 5 0 015 5`).

use crate::util::{self, Error, Result};
use std::f64::consts::{PI, TAU};

/// The segment the generated code uses. The same shape as `Seg` in `crates/fairing/src/icons`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Seg {
    /// A move.
    M(f32, f32),
    /// A line.
    L(f32, f32),
    /// A quadratic Bézier (the control point, the endpoint).
    Q(f32, f32, f32, f32),
    /// A cubic Bézier (two control points, the endpoint).
    C(f32, f32, f32, f32, f32, f32),
    /// Close the subpath.
    Z,
}

/// A point on the plane.
pub(crate) type Pt = (f64, f64);

/// The factor `4/3 * (sqrt(2) - 1)` for approximating a circle or a rounded rectangle's corner with cubics.
pub(crate) const KAPPA: f64 = 0.552_284_749_830_793_9;

/// Turn one `d` attribute into a segment list.
pub(crate) fn parse_path(data: &str) -> Result<Vec<Seg>> {
    let mut lexer = Lexer::new(data);
    let mut state = State::new();
    let mut last: Option<char> = None;
    loop {
        lexer.skip_separators();
        if lexer.eof() {
            break;
        }
        let command = match lexer.take_command() {
            Some(letter) => letter,
            None => match last {
                // Where only coordinates carry on, the previous command repeats. After an M/m it is an L/l.
                Some('M') => 'L',
                Some('m') => 'l',
                Some(letter) if !matches!(letter, 'Z' | 'z') => letter,
                _ => {
                    return Err(Error::new(format!(
                        "the path data has no command character: `{data}`"
                    )))
                }
            },
        };
        if last.is_none() && !matches!(command, 'M' | 'm') {
            return Err(Error::new(format!(
                "the path data has to start with an M/m: `{data}`"
            )));
        }
        execute(&mut state, &mut lexer, command, data)?;
        last = Some(command);
    }
    Ok(state.segs)
}

struct State {
    segs: Vec<Seg>,
    cur: Pt,
    start: Pt,
    cubic_ctrl: Option<Pt>,
    quad_ctrl: Option<Pt>,
}

impl State {
    fn new() -> Self {
        Self {
            segs: Vec::new(),
            cur: (0.0, 0.0),
            start: (0.0, 0.0),
            cubic_ctrl: None,
            quad_ctrl: None,
        }
    }

    fn move_to(&mut self, to: Pt) {
        self.segs
            .push(Seg::M(util::to_f32(to.0), util::to_f32(to.1)));
        self.cur = to;
        self.start = to;
        self.cubic_ctrl = None;
        self.quad_ctrl = None;
    }

    fn line_to(&mut self, to: Pt) {
        self.segs
            .push(Seg::L(util::to_f32(to.0), util::to_f32(to.1)));
        self.cur = to;
        self.cubic_ctrl = None;
        self.quad_ctrl = None;
    }

    fn quad_to(&mut self, ctrl: Pt, to: Pt) {
        self.segs.push(Seg::Q(
            util::to_f32(ctrl.0),
            util::to_f32(ctrl.1),
            util::to_f32(to.0),
            util::to_f32(to.1),
        ));
        self.cur = to;
        self.quad_ctrl = Some(ctrl);
        self.cubic_ctrl = None;
    }

    fn cubic_to(&mut self, first: Pt, second: Pt, to: Pt) {
        self.segs.push(Seg::C(
            util::to_f32(first.0),
            util::to_f32(first.1),
            util::to_f32(second.0),
            util::to_f32(second.1),
            util::to_f32(to.0),
            util::to_f32(to.1),
        ));
        self.cur = to;
        self.cubic_ctrl = Some(second);
        self.quad_ctrl = None;
    }

    fn close(&mut self) {
        self.segs.push(Seg::Z);
        self.cur = self.start;
        self.cubic_ctrl = None;
        self.quad_ctrl = None;
    }

    /// The reflection of the previous cubic control point. Where the previous was not a cubic, the current point as it is.
    fn reflected_cubic(&self) -> Pt {
        reflect(self.cubic_ctrl, self.cur)
    }

    /// The reflection of the previous quadratic control point.
    fn reflected_quad(&self) -> Pt {
        reflect(self.quad_ctrl, self.cur)
    }
}

fn reflect(previous: Option<Pt>, cur: Pt) -> Pt {
    previous.map_or(cur, |prev| {
        (
            2.0f64.mul_add(cur.0, -prev.0),
            2.0f64.mul_add(cur.1, -prev.1),
        )
    })
}

fn execute(state: &mut State, lexer: &mut Lexer, command: char, data: &str) -> Result<()> {
    let relative = command.is_ascii_lowercase();
    match command.to_ascii_uppercase() {
        'M' => {
            let to = point(lexer, state.cur, relative, data)?;
            state.move_to(to);
        }
        'L' => {
            let to = point(lexer, state.cur, relative, data)?;
            state.line_to(to);
        }
        'H' => {
            let value = number(lexer, data)?;
            let x = if relative { state.cur.0 + value } else { value };
            state.line_to((x, state.cur.1));
        }
        'V' => {
            let value = number(lexer, data)?;
            let y = if relative { state.cur.1 + value } else { value };
            state.line_to((state.cur.0, y));
        }
        'C' => {
            let first = point(lexer, state.cur, relative, data)?;
            let second = point(lexer, state.cur, relative, data)?;
            let to = point(lexer, state.cur, relative, data)?;
            state.cubic_to(first, second, to);
        }
        'S' => {
            let second = point(lexer, state.cur, relative, data)?;
            let to = point(lexer, state.cur, relative, data)?;
            let first = state.reflected_cubic();
            state.cubic_to(first, second, to);
        }
        'Q' => {
            let ctrl = point(lexer, state.cur, relative, data)?;
            let to = point(lexer, state.cur, relative, data)?;
            state.quad_to(ctrl, to);
        }
        'T' => {
            let to = point(lexer, state.cur, relative, data)?;
            let ctrl = state.reflected_quad();
            state.quad_to(ctrl, to);
        }
        'A' => arc(state, lexer, relative, data)?,
        'Z' => state.close(),
        other => {
            return Err(Error::new(format!(
                "unsupported path command `{other}` (supported: M L H V C S Q T A Z)"
            )))
        }
    }
    Ok(())
}

fn arc(state: &mut State, lexer: &mut Lexer, relative: bool, data: &str) -> Result<()> {
    let rx = number(lexer, data)?;
    let ry = number(lexer, data)?;
    let rotation = number(lexer, data)?;
    let large = flag(lexer, data)?;
    let sweep = flag(lexer, data)?;
    let to = point(lexer, state.cur, relative, data)?;

    let same =
        (to.0 - state.cur.0).abs() < f64::EPSILON && (to.1 - state.cur.1).abs() < f64::EPSILON;
    if same {
        return Ok(());
    }
    if rx.abs() < f64::EPSILON || ry.abs() < f64::EPSILON {
        state.line_to(to);
        return Ok(());
    }
    for (first, second, end) in arc_to_cubics(state.cur, rx, ry, rotation, large, sweep, to) {
        state.cubic_to(first, second, end);
    }
    Ok(())
}

/// Go through the centre parameterisation and split the arc into cubic Béziers of 90° or less.
/// It hands back a list of `(control point 1, control point 2, endpoint)`, the last endpoint being `to`.
pub(crate) fn arc_to_cubics(
    from: Pt,
    rx: f64,
    ry: f64,
    rotation_deg: f64,
    large: bool,
    sweep: bool,
    to: Pt,
) -> Vec<(Pt, Pt, Pt)> {
    let phi = rotation_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();
    let dx = (from.0 - to.0) / 2.0;
    let dy = (from.1 - to.1) / 2.0;
    let px = cos_phi.mul_add(dx, sin_phi * dy);
    let py = (-sin_phi).mul_add(dx, cos_phi * dy);

    let mut rx = rx.abs();
    let mut ry = ry.abs();
    let lambda = (px * px) / (rx * rx) + (py * py) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }

    let denominator = (rx * rx).mul_add(py * py, ry * ry * px * px);
    let numerator = (rx * rx).mul_add(ry * ry, -((rx * rx).mul_add(py * py, ry * ry * px * px)));
    let mut coefficient = if denominator > 0.0 {
        (numerator.max(0.0) / denominator).sqrt()
    } else {
        0.0
    };
    if large == sweep {
        coefficient = -coefficient;
    }
    let cpx = coefficient * rx * py / ry;
    let cpy = -coefficient * ry * px / rx;
    let ellipse = Ellipse {
        cx: cos_phi.mul_add(cpx, -(sin_phi * cpy)) + f64::midpoint(from.0, to.0),
        cy: sin_phi.mul_add(cpx, cos_phi * cpy) + f64::midpoint(from.1, to.1),
        rx,
        ry,
        cos_phi,
        sin_phi,
    };

    let ux = (px - cpx) / rx;
    let uy = (py - cpy) / ry;
    let vx = (-px - cpx) / rx;
    let vy = (-py - cpy) / ry;
    let theta = angle_between(1.0, 0.0, ux, uy);
    let mut delta = angle_between(ux, uy, vx, vy);
    if !sweep && delta > 0.0 {
        delta -= TAU;
    }
    if sweep && delta < 0.0 {
        delta += TAU;
    }
    ellipse.subdivide(theta, delta)
}

struct Ellipse {
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    cos_phi: f64,
    sin_phi: f64,
}

impl Ellipse {
    fn point(&self, angle: f64) -> Pt {
        let (sin_a, cos_a) = angle.sin_cos();
        (
            (self.rx * cos_a).mul_add(self.cos_phi, -(self.ry * sin_a * self.sin_phi)) + self.cx,
            (self.rx * cos_a).mul_add(self.sin_phi, self.ry * sin_a * self.cos_phi) + self.cy,
        )
    }

    /// The parametric derivative (the tangent vector).
    fn tangent(&self, angle: f64) -> Pt {
        let (sin_a, cos_a) = angle.sin_cos();
        (
            (-self.rx * sin_a).mul_add(self.cos_phi, -(self.ry * cos_a * self.sin_phi)),
            (-self.rx * sin_a).mul_add(self.sin_phi, self.ry * cos_a * self.cos_phi),
        )
    }

    fn subdivide(&self, theta: f64, delta: f64) -> Vec<(Pt, Pt, Pt)> {
        let count = util::to_count((delta.abs() / (PI / 2.0)).ceil()).max(1);
        let step = delta / util::count_to_f64(count);
        let kappa = 4.0 / 3.0 * (step / 4.0).tan();
        let mut out = Vec::with_capacity(count);
        let mut angle = theta;
        for _ in 0..count {
            let next = angle + step;
            let start = self.point(angle);
            let end = self.point(next);
            let start_tangent = self.tangent(angle);
            let end_tangent = self.tangent(next);
            out.push((
                (
                    kappa.mul_add(start_tangent.0, start.0),
                    kappa.mul_add(start_tangent.1, start.1),
                ),
                (
                    (-kappa).mul_add(end_tangent.0, end.0),
                    (-kappa).mul_add(end_tangent.1, end.1),
                ),
                end,
            ));
            angle = next;
        }
        out
    }
}

fn angle_between(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let dot = ux.mul_add(vx, uy * vy);
    let length = ux.mul_add(ux, uy * uy).sqrt() * vx.mul_add(vx, vy * vy).sqrt();
    if length <= 0.0 {
        return 0.0;
    }
    let angle = (dot / length).clamp(-1.0, 1.0).acos();
    if ux.mul_add(vy, -(uy * vx)) < 0.0 {
        -angle
    } else {
        angle
    }
}

fn point(lexer: &mut Lexer, cur: Pt, relative: bool, data: &str) -> Result<Pt> {
    let x = number(lexer, data)?;
    let y = number(lexer, data)?;
    if relative {
        Ok((cur.0 + x, cur.1 + y))
    } else {
        Ok((x, y))
    }
}

fn number(lexer: &mut Lexer, data: &str) -> Result<f64> {
    lexer
        .take_number()
        .ok_or_else(|| Error::new(format!("the path data is short of numbers: `{data}`")))
}

fn flag(lexer: &mut Lexer, data: &str) -> Result<bool> {
    lexer
        .take_flag()
        .ok_or_else(|| Error::new(format!("an arc (A) flag has to be 0 or 1: `{data}`")))
}

/// The tokeniser the `d` attribute and the `points` attribute share.
pub(crate) struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    /// Build a tokeniser from a string.
    pub(crate) fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Skip whitespace and commas.
    pub(crate) fn skip_separators(&mut self) {
        while self.peek().is_some_and(|c| c.is_whitespace() || c == ',') {
            self.pos += 1;
        }
    }

    /// Whether there is nothing more to read.
    pub(crate) fn eof(&mut self) -> bool {
        self.skip_separators();
        self.peek().is_none()
    }

    fn take_command(&mut self) -> Option<char> {
        let letter = self.peek()?;
        if letter.is_ascii_alphabetic() {
            self.pos += 1;
            Some(letter)
        } else {
            None
        }
    }

    fn take_flag(&mut self) -> Option<bool> {
        self.skip_separators();
        match self.peek()? {
            '0' => {
                self.pos += 1;
                Some(false)
            }
            '1' => {
                self.pos += 1;
                Some(true)
            }
            _ => None,
        }
    }

    /// Read one number. It takes exponent notation and a leading sign or decimal point.
    pub(crate) fn take_number(&mut self) -> Option<f64> {
        self.skip_separators();
        let begin = self.pos;
        if matches!(self.peek(), Some('+' | '-')) {
            self.pos += 1;
        }
        let mut digits = self.take_digits();
        if self.peek() == Some('.') {
            self.pos += 1;
            digits |= self.take_digits();
        }
        if !digits {
            self.pos = begin;
            return None;
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let mark = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            if !self.take_digits() {
                self.pos = mark;
            }
        }
        let text: String = self.chars.get(begin..self.pos)?.iter().collect();
        text.parse::<f64>().ok()
    }

    fn take_digits(&mut self) -> bool {
        let begin = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        self.pos > begin
    }
}

/// Read a `points` attribute (`"1,2 3,4"`) as a point list.
pub(crate) fn parse_points(text: &str) -> Result<Vec<Pt>> {
    let mut lexer = Lexer::new(text);
    let mut out = Vec::new();
    while !lexer.eof() {
        let (Some(x), Some(y)) = (lexer.take_number(), lexer.take_number()) else {
            return Err(Error::new(format!(
                "the points value does not pair up: `{text}`"
            )));
        };
        out.push((x, y));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{arc_to_cubics, parse_path, parse_points, Lexer, Pt, Seg};
    use crate::util::{Error, Result};

    fn near(left: f32, right: f32) -> bool {
        (left - right).abs() < 1e-3
    }

    fn near64(left: f64, right: f64) -> bool {
        (left - right).abs() < 1e-6
    }

    #[test]
    fn tokenizes_mixed_separators() {
        let mut lexer = Lexer::new("10-5 .5.5, 1e2 -3.5e-1");
        let read: Vec<Option<f64>> = (0..6).map(|_| lexer.take_number()).collect();
        assert_eq!(
            read,
            vec![
                Some(10.0),
                Some(-5.0),
                Some(0.5),
                Some(0.5),
                Some(100.0),
                Some(-0.35)
            ]
        );
        assert!(lexer.eof());
    }

    #[test]
    fn absolute_and_relative_agree() -> Result<()> {
        let absolute = parse_path("M 2 3 L 6 3 L 6 9 Z")?;
        let relative = parse_path("m2,3l4,0l0,6z")?;
        assert_eq!(absolute, relative);
        assert_eq!(
            absolute,
            vec![Seg::M(2.0, 3.0), Seg::L(6.0, 3.0), Seg::L(6.0, 9.0), Seg::Z]
        );
        Ok(())
    }

    #[test]
    fn implicit_lineto_follows_moveto() -> Result<()> {
        assert_eq!(
            parse_path("M1 1 2 2 3 3")?,
            vec![Seg::M(1.0, 1.0), Seg::L(2.0, 2.0), Seg::L(3.0, 3.0)]
        );
        Ok(())
    }

    #[test]
    fn horizontal_and_vertical_become_lines() -> Result<()> {
        assert_eq!(
            parse_path("M0 0 H10 V10 h-4 v-4")?,
            vec![
                Seg::M(0.0, 0.0),
                Seg::L(10.0, 0.0),
                Seg::L(10.0, 10.0),
                Seg::L(6.0, 10.0),
                Seg::L(6.0, 6.0),
            ]
        );
        Ok(())
    }

    #[test]
    fn smooth_commands_reflect_the_control_point() -> Result<()> {
        let segs = parse_path("M0 0 C1 1 2 2 3 3 S5 5 6 6")?;
        assert_eq!(segs.get(2), Some(&Seg::C(4.0, 4.0, 5.0, 5.0, 6.0, 6.0)));
        let quad = parse_path("M0 0 Q1 2 2 0 T4 0")?;
        assert_eq!(quad.get(2), Some(&Seg::Q(3.0, -2.0, 4.0, 0.0)));
        Ok(())
    }

    #[test]
    fn close_returns_to_the_subpath_start() -> Result<()> {
        let segs = parse_path("M2 2 L6 2 Z l0 4")?;
        assert_eq!(segs.last(), Some(&Seg::L(2.0, 6.0)));
        Ok(())
    }

    #[test]
    fn arc_flags_may_be_glued_to_the_next_number() -> Result<()> {
        let glued = parse_path("M2 12a10 10 0 0120 0")?;
        let spaced = parse_path("M2 12 a10 10 0 0 1 20 0")?;
        assert_eq!(glued, spaced);
        let Some(Seg::C(_, _, _, _, x, y)) = glued.last() else {
            return Err(Error::new("the arc was not turned into cubics"));
        };
        assert!(near(*x, 22.0) && near(*y, 12.0), "the endpoint {x},{y}");
        Ok(())
    }

    #[test]
    fn arc_endpoints_match_the_request() {
        let cases: [(Pt, f64, f64, f64, bool, bool, Pt); 4] = [
            ((2.0, 12.0), 10.0, 10.0, 0.0, false, true, (22.0, 12.0)),
            ((2.0, 12.0), 10.0, 10.0, 0.0, true, false, (22.0, 12.0)),
            ((4.0, 4.0), 6.0, 3.0, 30.0, true, true, (16.0, 10.0)),
            ((1.0, 1.0), 0.5, 0.5, 0.0, false, false, (2.0, 2.0)),
        ];
        for (from, rx, ry, rot, large, sweep, to) in cases {
            let parts = arc_to_cubics(from, rx, ry, rot, large, sweep, to);
            assert!(!parts.is_empty(), "the split is empty");
            let Some((_, _, end)) = parts.last() else {
                continue;
            };
            assert!(
                near64(end.0, to.0) && near64(end.1, to.1),
                "the endpoint does not line up: {end:?} != {to:?}"
            );
            assert!(parts.len() <= 4, "a piece over 90°: {}", parts.len());
        }
    }

    #[test]
    fn parsing_is_deterministic() -> Result<()> {
        let data = "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zm0 3.5.5.5";
        assert_eq!(parse_path(data)?, parse_path(data)?);
        Ok(())
    }

    #[test]
    fn rejects_broken_data() {
        assert!(
            parse_path("L 1 2").is_err(),
            "starting without an M is an error"
        );
        assert!(parse_path("M1").is_err());
        assert!(parse_path("M0 0 A10 10 0 5 1 10 10").is_err());
        assert!(parse_path("M0 0 X1 1").is_err());
    }

    #[test]
    fn reads_point_lists() -> Result<()> {
        assert_eq!(
            parse_points("1,2 3 4\n5,6")?,
            vec![(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)]
        );
        assert!(parse_points("1,2 3").is_err());
        Ok(())
    }
}
