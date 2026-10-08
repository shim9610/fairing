//! `PinPad` — the digits of a PIN, on keys a gloved finger lands on.
//!
//! # Why it is a widget and not a corner of the prompt
//!
//! The shell's unlock prompt draws one, and so can an integrator: in `routing` mode the login
//! screen is theirs, and the keypad is the part of it nobody should have to draw twice.
//!
//! # The crate owns no secret
//!
//! Like [`TextField`](super::TextField) it borrows the caller's `&mut String`. The digits go into
//! that buffer and nowhere else — no animation entry and no egui memory holds them — so the
//! caller decides how long they live and in what type.
//!
//! # Why the order is the caller's
//!
//! A shuffled layout is fresh **each time the prompt opens** — not each frame, and not
//! each press: a keypad that moves under the finger cannot be used. The widget cannot tell
//! "opened" from "drawn again", so the caller holds the order ([`PinPad::order`]) and makes a new
//! one when it opens ([`PinPad::shuffled`]).
//!
//! # The bottom row is a row like the others
//!
//! Erase, 0 and OK — three keys with faces, the same size as every key above them. A first
//! version left the bottom left empty and drew erase as a bare glyph, and the grid read as three
//! rows and a stray: the one row with a hole in it was the row the eye checks last. Erase and OK
//! dim, and stop listening, while there is nothing to erase or submit.
//!
//! # Auto-submit, and how long a PIN may be
//!
//! With a known length ([`PinPad::len`]) the last digit submits by itself — nobody looks for the
//! OK key — and the dots say how many are left; OK still submits early, and the authenticator
//! says what it thinks of a short PIN. With an unknown one the dots only count what went in, and
//! [`PinPad::max_len`] caps how many digits the pad takes (sixteen when it is not set).
//!
//! # Why the keys are as tall as they are
//!
//! A key is `KEY_RATIO` touch targets tall, and shrinks to fit the height it is given but never
//! below one target. The PIN is typed with the device held at arm's length or mounted on a
//! machine, often in a glove, and a miss costs a wrong-PIN shake and an attempt off the limit —
//! the one keypad where a mistyped key has a price.

use super::{PinKey, PinPadLook, PinPart};
use crate::cx::WidgetCx as Cx;
use crate::icons::{IconColor, IconStyle};
use crate::theme::{control_height, ColorRole};
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

use super::button::ButtonKind;

/// A key's height in touch targets, before it shrinks to fit.
const KEY_RATIO: f32 = 1.25;

/// The widest a key may be over its height — past this a phone-width keypad on a wide panel
/// turns into three bars.
const MAX_ASPECT: f32 = 1.8;

/// A dot's diameter as a fraction of `control.mark_size`.
const DOT_RATIO: f32 = 0.45;

/// The dot row's height in dot diameters — the dots and the air above and below them.
const DOT_ROW: f32 = 3.0;

/// The digits in key order when nothing is shuffled: the telephone layout, 1 at the top left
/// and 0 under 8.
pub const PHONE_ORDER: [u8; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];

/// The most digits the pad ever takes — the dot row has room for this many. It is also the cap
/// when neither [`PinPad::len`] nor [`PinPad::max_len`] says otherwise.
pub const MAX_DIGITS: usize = 16;

/// What a key on the pad does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PadKey {
    /// A digit, `0..=9`.
    Digit(u8),
    /// Take the last digit off.
    Erase,
    /// Submit what is there.
    Ok,
}

/// The twelve keys, row by row: the digits in `order`, then erase · the tenth digit · OK — the
/// same bottom row whatever the length, so a thumb that learnt where erase is keeps it.
fn keys(order: [u8; 10]) -> [PadKey; 12] {
    let [k1, k2, k3, k4, k5, k6, k7, k8, k9, k0] = order.map(PadKey::Digit);
    [
        k1,
        k2,
        k3,
        k4,
        k5,
        k6,
        k7,
        k8,
        k9,
        PadKey::Erase,
        k0,
        PadKey::Ok,
    ]
}

/// Whether `order` holds each digit exactly once.
fn is_permutation(order: [u8; 10]) -> bool {
    let mut seen = [false; 10];
    for d in order {
        match seen.get_mut(usize::from(d)) {
            Some(slot) if !*slot => *slot = true,
            _ => return false,
        }
    }
    true
}

/// What the pad reported this frame.
#[derive(Debug)]
pub struct PinPadResponse {
    /// The whole pad.
    pub response: Response,
    /// A digit went in or came out.
    pub changed: bool,
    /// The PIN is complete — the last digit of a known length, or the OK key — and is the
    /// caller's to take.
    pub submitted: bool,
    /// Where each key was drawn, in key order.
    keys: [(PadKey, Rect); 12],
}

impl PinPadResponse {
    /// Where the key for `digit` was drawn this frame — wherever a shuffle put it. For a test
    /// driving a login screen the way a finger would.
    #[must_use]
    pub fn digit_rect(&self, digit: u8) -> Option<Rect> {
        self.keys
            .iter()
            .find(|(key, _)| *key == PadKey::Digit(digit))
            .map(|(_, rect)| *rect)
    }
}

/// A PIN keypad with its dot row.
///
/// ```no_run
/// # fn ui(ui: &mut egui::Ui, cx: &mut fairing_widgets::WidgetCx<'_>, pin: &mut String) {
/// use fairing_widgets::widgets::PinPad;
///
/// let pad = PinPad::new(pin).len(4).show(ui, cx);
/// if pad.submitted {
///     let entered = std::mem::take(pin);
///     // hand `entered` to whatever checks it
/// #   let _ = entered;
/// }
/// # }
/// ```
pub struct PinPad<'a> {
    pin: &'a mut String,
    len: u8,
    max_len: u8,
    order: [u8; 10],
    enabled: bool,
    keyboard: bool,
}

impl std::fmt::Debug for PinPad<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The buffer is a secret; only its length is anyone's business.
        f.debug_struct("PinPad")
            .field("digits", &self.pin.len())
            .field("len", &self.len)
            .field("max_len", &self.max_len)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl<'a> PinPad<'a> {
    /// A pad typing into `pin`. Anything in it that is not a digit is left alone and counted as
    /// a dot like the rest — the buffer is the caller's.
    pub fn new(pin: &'a mut String) -> Self {
        Self {
            pin,
            len: 0,
            max_len: 0,
            order: PHONE_ORDER,
            enabled: true,
            keyboard: false,
        }
    }

    /// How many digits the PIN has: the last one submits by itself and the dot row shows the
    /// ones still to come. `0` (the default) means unknown — the dots count what went in and OK
    /// submits.
    #[must_use]
    pub fn len(mut self, len: u8) -> Self {
        self.len = u8::try_from(usize::from(len).min(MAX_DIGITS)).unwrap_or(0);
        self
    }

    /// The most digits the pad takes while the length is unknown — what a device that allows
    /// four to eight digits sets to 8. `0` (the default) is [`MAX_DIGITS`]; a known
    /// [`PinPad::len`] is its own cap.
    #[must_use]
    pub fn max_len(mut self, max_len: u8) -> Self {
        self.max_len = u8::try_from(usize::from(max_len).min(MAX_DIGITS)).unwrap_or(0);
        self
    }

    /// The digits in key order — top left to bottom middle, ten of them, each once.
    /// [`PinPad::shuffled`] makes one; anything that is not a permutation of `0..=9` is ignored
    /// for the telephone layout, since a pad missing a digit could lock someone out.
    #[must_use]
    pub fn order(mut self, order: [u8; 10]) -> Self {
        if is_permutation(order) {
            self.order = order;
        }
        self
    }

    /// Greyed and deaf — the authenticator's lockout (A9: a disabled tint, no animation).
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Also take the digit keys, Backspace and Enter from a hardware keyboard — a panel with a
    /// keypad beside the glass. Off by default, so two pads on one screen do not both type.
    #[must_use]
    pub fn keyboard(mut self, keyboard: bool) -> Self {
        self.keyboard = keyboard;
        self
    }

    /// A digit order from `seed`, every digit once (Fisher–Yates over xorshift64*).
    ///
    /// The seed is the caller's: what this protects against is a smudge pattern and a glance
    /// over the shoulder, not someone predicting a random number, so any per-opening value will
    /// do — `std::collections::hash_map::RandomState` hashing a counter is what the shell uses.
    #[must_use]
    pub fn shuffled(seed: u64) -> [u8; 10] {
        let mut state = seed | 1;
        let mut next = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_F491_4F6C_DD1D)
        };
        let mut order = PHONE_ORDER;
        for i in (1..order.len()).rev() {
            // `i + 1` is at most 10, so both conversions are lossless.
            let bound = u64::try_from(i + 1).unwrap_or(1);
            let j = usize::try_from(next() % bound).unwrap_or(0);
            order.swap(i, j);
        }
        order
    }

    /// The cap on what the buffer takes.
    fn capacity(&self) -> usize {
        match (self.len, self.max_len) {
            (0, 0) => MAX_DIGITS,
            (0, max) => usize::from(max),
            (len, _) => usize::from(len),
        }
    }

    /// One key's effect on the buffer. Returns `(changed, submitted)`.
    fn press(&mut self, key: PadKey) -> (bool, bool) {
        match key {
            PadKey::Digit(d) if self.pin.len() < self.capacity() => {
                self.pin.push(char::from(b'0' + d.min(9)));
                let complete = self.len > 0 && self.pin.len() == usize::from(self.len);
                (true, complete)
            }
            PadKey::Erase if !self.pin.is_empty() => {
                self.pin.pop();
                (true, false)
            }
            PadKey::Ok => (false, !self.pin.is_empty()),
            _ => (false, false),
        }
    }

    /// **The size the pad takes in `room`**, without drawing it — for a caller laying a card out
    /// around it. [`PinPad::show`] allocates exactly this in `ui.available_size()`.
    #[must_use]
    pub fn measure(cx: &Cx<'_>, room: Vec2) -> Vec2 {
        Geometry::of(cx, room).size()
    }

    /// Draw it.
    pub fn show(mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> PinPadResponse {
        let geometry = Geometry::of(cx, ui.available_size());
        let Geometry {
            key_w,
            key_h,
            gap,
            dot,
            dots_h,
        } = geometry;
        let (rect, response) = ui.allocate_exact_size(geometry.size(), Sense::hover());

        let mut changed = false;
        let mut submitted = false;
        let grid_top = rect.min.y + dots_h + gap;
        let mut drawn = [(PadKey::Ok, Rect::NOTHING); 12];
        for (i, key) in keys(self.order).into_iter().enumerate() {
            // Twelve keys in three columns: both casts are of indices below 12.
            #[allow(clippy::cast_precision_loss)]
            let (col, row) = ((i % 3) as f32, (i / 3) as f32);
            let at = Rect::from_min_size(
                egui::pos2(
                    rect.min.x + col * (key_w + gap),
                    grid_top + row * (key_h + gap),
                ),
                Vec2::new(key_w, key_h),
            );
            if let Some(slot) = drawn.get_mut(i) {
                *slot = (key, at);
            }
            let id = response.id.with(("pin.key", i));
            // Erase and OK have nothing to do on an empty buffer, and say so by dimming.
            let live = self.enabled && (matches!(key, PadKey::Digit(_)) || !self.pin.is_empty());
            let sense = if live { Sense::click() } else { Sense::hover() };
            let key_response = ui.interact(at, id, sense);
            let pressed = live && key_response.is_pointer_button_down_on();
            let motion = cx.theme.motion;
            let tween = if pressed {
                motion.press
            } else {
                motion.press_release
            };
            let press = cx.animate(id.with("press"), if pressed { 1.0 } else { 0.0 }, tween);
            let state = KeyState {
                pressed,
                press,
                live,
            };
            draw_key(ui, cx, at, key, (state, self.enabled));
            if key_response.clicked() {
                let (c, s) = self.press(key);
                changed |= c;
                submitted |= s;
            }
        }
        if self.keyboard && self.enabled {
            let typed: Vec<PadKey> = ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|event| match event {
                        // A held key's auto-repeat is not a second press.
                        egui::Event::Key {
                            key,
                            pressed: true,
                            repeat: false,
                            ..
                        } => hardware_key(*key),
                        _ => None,
                    })
                    .collect()
            });
            for key in typed {
                let (c, s) = self.press(key);
                changed |= c;
                submitted |= s;
            }
        }
        self.draw_dots(ui, cx, rect, (dots_h, dot));
        PinPadResponse {
            response,
            changed,
            submitted,
            keys: drawn,
        }
    }
}

impl PinPad<'_> {
    /// The dot row over the keys in `rect`, `(height, dot)` its band and its dots' size — by the
    /// pad's painter where there is one, the built-in way where not.
    fn draw_dots(&self, ui: &egui::Ui, cx: &mut Cx<'_>, rect: Rect, (height, dot): (f32, f32)) {
        if let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.pin_pad.as_mut()) {
            let band = Rect::from_min_size(rect.min, Vec2::new(rect.width(), height));
            custom(
                ui.painter(),
                &mut PinPadLook {
                    part: PinPart::Dots {
                        entered: self.pin.len(),
                        length: (self.len > 0).then_some(self.len),
                    },
                    rect: band,
                    drawn: band,
                    enabled: self.enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
        } else {
            paint_dots(
                ui,
                cx,
                rect,
                height,
                dot,
                self.pin.len(),
                self.len,
                self.enabled,
            );
        }
    }
}

/// One key at `at`, `(state, enabled)` its own state and the pad's — by the pad's painter where
/// there is one, the built-in way where not.
fn draw_key(
    ui: &egui::Ui,
    cx: &mut Cx<'_>,
    at: Rect,
    key: PadKey,
    (state, enabled): (KeyState, bool),
) {
    if let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.pin_pad.as_mut()) {
        custom(
            ui.painter(),
            &mut PinPadLook {
                part: PinPart::Key {
                    key: key.public(),
                    live: state.live,
                    pressed: state.pressed,
                    press: state.press,
                },
                rect: at,
                drawn: at.expand(cx.theme.control.press_grow * state.press),
                enabled,
                theme: cx.theme,
                icons: &mut *cx.icons,
            },
        );
    } else {
        paint_key(ui, cx, at, key, state);
    }
}

/// The pad's lengths in a given room — one function, read by [`PinPad::measure`] and
/// [`PinPad::show`] alike, so a card laid out around the pad and the pad itself agree.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    key_w: f32,
    key_h: f32,
    gap: f32,
    dot: f32,
    dots_h: f32,
}

impl Geometry {
    fn of(cx: &Cx<'_>, room: Vec2) -> Self {
        let theme = cx.theme;
        let gap = theme.control.gap;
        let target = control_height(&theme.metrics, &theme.control);
        let dot = theme.control.mark_size * DOT_RATIO;
        let dots_h = dot * DOT_ROW;
        let mut key_h = target * KEY_RATIO;
        if room.y.is_finite() {
            key_h = key_h.min(((room.y - dots_h - gap * 4.0) / 4.0).max(target));
        }
        let key_w = ((room.x - gap * 2.0) / 3.0).clamp(target, key_h * MAX_ASPECT);
        Self {
            key_w,
            key_h,
            gap,
            dot,
            dots_h,
        }
    }

    fn size(self) -> Vec2 {
        Vec2::new(
            self.key_w * 3.0 + self.gap * 2.0,
            self.dots_h + self.gap + self.key_h * 4.0 + self.gap * 3.0,
        )
    }
}

/// A hardware key → the pad key it stands for.
fn hardware_key(key: egui::Key) -> Option<PadKey> {
    use egui::Key;
    let digit = match key {
        Key::Num0 => 0,
        Key::Num1 => 1,
        Key::Num2 => 2,
        Key::Num3 => 3,
        Key::Num4 => 4,
        Key::Num5 => 5,
        Key::Num6 => 6,
        Key::Num7 => 7,
        Key::Num8 => 8,
        Key::Num9 => 9,
        Key::Backspace => return Some(PadKey::Erase),
        Key::Enter => return Some(PadKey::Ok),
        _ => return None,
    };
    Some(PadKey::Digit(digit))
}

/// A key's state this frame: a finger on it, its press tween, and whether it can be pressed.
#[derive(Debug, Clone, Copy)]
struct KeyState {
    pressed: bool,
    press: f32,
    live: bool,
}

impl PadKey {
    /// The key as a painter is told it.
    const fn public(self) -> PinKey {
        match self {
            Self::Digit(d) => PinKey::Digit(d),
            Self::Erase => PinKey::Erase,
            Self::Ok => PinKey::Ok,
        }
    }
}

/// One key: the button family's dress — a digit and erase are `Normal` keys, OK the one
/// `Primary`.
fn paint_key(ui: &egui::Ui, cx: &mut Cx<'_>, at: Rect, key: PadKey, state: KeyState) {
    let KeyState {
        pressed,
        press,
        live: enabled,
    } = state;
    let theme = cx.theme;
    let drawn = at.expand(theme.control.press_grow * press);
    let tint = |c: Color32| {
        if enabled {
            c
        } else {
            c.gamma_multiply(theme.control.disabled_alpha)
        }
    };
    let kind = match key {
        PadKey::Ok => ButtonKind::Primary,
        _ => ButtonKind::Normal,
    };
    let dress = kind.dress();
    let radius = CornerRadius::same(round_u8(theme.metrics.control_radius));
    let mut face = theme.color(dress.face);
    if dress.translucent {
        face = face.gamma_multiply(theme.control.fill_alpha);
    }
    if pressed {
        // `blend`'s receiver is the layer behind (see `BigButton`).
        face = face.blend(theme.color(ColorRole::Pressed));
    }
    ui.painter().rect_filled(drawn, radius, tint(face));
    if let Some(edge) = dress.edge {
        ui.painter().rect_stroke(
            drawn,
            radius,
            Stroke::new(theme.control.stroke_edge, tint(theme.color(edge))),
            StrokeKind::Inside,
        );
    }
    let ink = tint(theme.color(dress.label));
    match key {
        PadKey::Digit(d) => {
            let font = theme.strong(theme.metrics.type_scale.heading);
            let galley =
                ui.painter()
                    .layout_no_wrap(char::from(b'0' + d.min(9)).to_string(), font, ink);
            let size = galley.size();
            ui.painter()
                .galley(drawn.center() - size / 2.0, galley, ink);
        }
        PadKey::Erase => {
            let side = theme.control.icon;
            paint_backspace(
                ui.painter(),
                Rect::from_center_size(drawn.center(), Vec2::splat(side)),
                Stroke::new(theme.control.stroke_mark, ink),
            );
        }
        PadKey::Ok => {
            let side = theme.control.icon;
            let style = IconStyle::sized(side).color(IconColor::Fixed(ink));
            cx.icons.paint(
                ui.painter(),
                Rect::from_center_size(drawn.center(), Vec2::splat(side)),
                &crate::icons::builtin::CHECK,
                &style,
                theme,
            );
        }
    }
}

/// The backspace mark — a tag pointing left with a cross in it — drawn rather than taken from the
/// icon set, which has a back arrow and no backspace: an arrow on a keypad reads as "back a
/// screen", not "take a digit off".
fn paint_backspace(painter: &egui::Painter, at: Rect, stroke: Stroke) {
    let (c, w, h) = (at.center(), at.width() * 0.5, at.height() * 0.36);
    let shoulder = c.x - w * 0.45;
    let outline = vec![
        egui::pos2(c.x - w, c.y),
        egui::pos2(shoulder, c.y - h),
        egui::pos2(c.x + w, c.y - h),
        egui::pos2(c.x + w, c.y + h),
        egui::pos2(shoulder, c.y + h),
    ];
    painter.add(egui::Shape::closed_line(outline, stroke));
    let x = egui::pos2(c.x + w * 0.22, c.y);
    let arm = h * 0.5;
    painter.line_segment([x + Vec2::new(-arm, -arm), x + Vec2::new(arm, arm)], stroke);
    painter.line_segment([x + Vec2::new(-arm, arm), x + Vec2::new(arm, -arm)], stroke);
}

/// The dot row: a filled dot per digit in, and — with a known length — a ring per digit to come.
#[allow(clippy::too_many_arguments)] // The row's geometry and state, each one value.
fn paint_dots(
    ui: &egui::Ui,
    cx: &Cx<'_>,
    rect: Rect,
    height: f32,
    dot: f32,
    entered: usize,
    len: u8,
    enabled: bool,
) {
    let theme = cx.theme;
    let slots = if len > 0 { usize::from(len) } else { entered }.min(MAX_DIGITS);
    if slots == 0 {
        return;
    }
    // Sixteen at most, so the cast is exact.
    #[allow(clippy::cast_precision_loss)]
    let count = slots as f32;
    let step = (dot * 2.0).min(rect.width() / count);
    let r = (dot / 2.0).min(step * 0.4);
    let y = rect.min.y + height / 2.0;
    let x0 = rect.center().x - step * (count - 1.0) / 2.0;
    let alpha = if enabled {
        1.0
    } else {
        theme.control.disabled_alpha
    };
    let filled = theme.color(ColorRole::OnSurface).gamma_multiply(alpha);
    let ring = theme.color(ColorRole::ControlEdge).gamma_multiply(alpha);
    for i in 0..slots {
        #[allow(clippy::cast_precision_loss)]
        let centre = egui::pos2(x0 + step * i as f32, y);
        if i < entered {
            ui.painter().circle_filled(centre, r, filled);
        } else {
            ui.painter()
                .circle_stroke(centre, r, Stroke::new(theme.control.stroke_mark, ring));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_permutation, keys, PadKey, PinPad, PHONE_ORDER};

    #[test]
    fn the_bottom_row_is_a_full_row_of_keys() {
        let row = keys(PHONE_ORDER);
        assert_eq!(
            row.get(9..),
            Some(&[PadKey::Erase, PadKey::Digit(0), PadKey::Ok][..])
        );
        assert_eq!(row.first(), Some(&PadKey::Digit(1)));
    }

    #[test]
    fn max_len_caps_an_unknown_length_and_len_wins_over_it() {
        let mut pin = String::new();
        let mut pad = PinPad::new(&mut pin).max_len(6);
        for _ in 0..10 {
            pad.press(PadKey::Digit(3));
        }
        assert_eq!(pin.len(), 6);
        let mut pin = String::new();
        let mut pad = PinPad::new(&mut pin).len(4).max_len(8);
        for _ in 0..10 {
            pad.press(PadKey::Digit(3));
        }
        assert_eq!(pin.len(), 4, "a known length is its own cap");
        let mut pin = String::new();
        let mut pad = PinPad::new(&mut pin).max_len(200);
        for _ in 0..40 {
            pad.press(PadKey::Digit(3));
        }
        assert_eq!(pin.len(), super::MAX_DIGITS, "never past the dot row");
    }

    #[test]
    fn a_known_length_submits_on_its_last_digit_and_takes_no_more() {
        let mut pin = String::new();
        let mut pad = PinPad::new(&mut pin).len(3);
        assert_eq!(pad.press(PadKey::Ok), (false, false), "nothing to submit");
        assert_eq!(pad.press(PadKey::Digit(4)), (true, false));
        assert_eq!(pad.press(PadKey::Ok), (false, true), "OK submits early");
        assert_eq!(pad.press(PadKey::Digit(2)), (true, false));
        assert_eq!(pad.press(PadKey::Digit(7)), (true, true));
        assert_eq!(pad.press(PadKey::Digit(1)), (false, false), "full");
        assert_eq!(pad.press(PadKey::Erase), (true, false));
        assert_eq!(pin, "42");
    }

    #[test]
    fn without_a_length_ok_submits_anything_but_nothing() {
        let mut pin = String::new();
        let mut pad = PinPad::new(&mut pin);
        assert_eq!(pad.press(PadKey::Ok), (false, false), "nothing to submit");
        assert_eq!(pad.press(PadKey::Erase), (false, false));
        pad.press(PadKey::Digit(9));
        assert_eq!(pad.press(PadKey::Ok), (false, true));
        for _ in 0..40 {
            pad.press(PadKey::Digit(1));
        }
        assert_eq!(pin.len(), super::MAX_DIGITS);
    }

    #[test]
    fn a_shuffle_is_every_digit_once_and_depends_on_its_seed() {
        let mut layouts = std::collections::BTreeSet::new();
        for seed in 0..64_u64 {
            let order = PinPad::shuffled(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            assert!(is_permutation(order), "{order:?}");
            layouts.insert(order);
        }
        assert!(
            layouts.len() > 32,
            "only {} layouts from 64 seeds",
            layouts.len()
        );
    }

    #[test]
    fn an_order_that_is_not_a_permutation_is_ignored() {
        let mut pin = String::new();
        let pad = PinPad::new(&mut pin).order([1, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(pad.order, PHONE_ORDER);
        let mut pin = String::new();
        let pad = PinPad::new(&mut pin).order([0, 9, 8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(pad.order, [0, 9, 8, 7, 6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn its_debug_shows_the_count_and_not_the_digits() {
        let mut pin = "8642".to_owned();
        let pad = PinPad::new(&mut pin);
        let printed = format!("{pad:?}");
        assert!(!printed.contains("8642") && printed.contains("digits: 4"));
    }
}
