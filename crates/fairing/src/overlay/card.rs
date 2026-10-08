//! **The card reveal** — the shade as a floating card that *materialises*, the way One
//! UI 7 and 8 open the quick panel.
//!
//! A curtain ([`super::Overlay::ui`]'s first mapping, A1) hangs the panel from the top
//! edge and draws it down: the content is pinned to the top and the pull reveals it. Nothing in
//! it moves except the edge, so the eye reads "a cloth being lifted off something that was
//! already there". A card is the opposite: it is already its final size at its final place, and
//! what the pull drives is **how far it has arrived** —
//!
//! - it **fades in** from nothing (the plate and, lagging behind, the content);
//! - it **comes down** a short way ([`crate::theme::ShadeMetrics::card_drop`]) — a direction, not a
//!   journey;
//! - its **edge sharpens**: the plate starts as a soft blob (an [`egui::epaint::Shadow`]-feathered
//!   silhouette as wide as its corner) and tightens to a corner as it lands.
//!
//! Those three together are what reads as "it came down fast from above" — the blur standing in
//! for motion blur, the drop giving the direction, the fade making it an arrival rather than a
//! reveal. The page behind is not dimmed: there is no scrim, only a catch for the tap outside.
//!
//! The numbers were measured off a One UI 8 recording at 1432 × 1080: a card 0.445 of the width,
//! corners at 9 % of the card's width, content inset 6 %, side inset 1 %, a top gap of about one
//! corner radius under the status bar, a drop of 4–6 % of the height, the open 0.34 s and the close
//! 0.04 s. The proportions are the tokens' defaults; the width is `[overlay] card_width_ratio`.
//!
//! The geometry and the arrival curve live here as plain functions so they can be tested without a
//! shell; the drawing is [`super::Overlay::ui`]'s.
// UI geometry: pixel values crossing to u8 corner radii. The loss is meaningless in this range, so
// the cast lints are lifted for the whole file (the workspace's pedantic lints stay).
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use crate::theme::{Metrics, ShadeMetrics};
use egui::{pos2, vec2, Rect};

/// How the shade comes into view (`[overlay] reveal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OverlayReveal {
    /// A sheet hung from the top edge, drawn down over the page (A1). The default.
    #[default]
    Curtain,
    /// A floating card that fades in, comes down a short way and sharpens into place.
    Card,
}

impl OverlayReveal {
    /// From a config string. `None` for a name it does not know.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "curtain" => Some(Self::Curtain),
            "card" => Some(Self::Card),
            _ => None,
        }
    }
}

/// Which side of a wide screen a card rests on (`[overlay] card_anchor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardAnchor {
    /// The side the pull started from; the middle when there was no finger.
    #[default]
    Press,
    /// Always the left.
    Left,
    /// Always the right.
    Right,
    /// Always the middle.
    Center,
}

impl CardAnchor {
    /// From a config string. `None` for a name it does not know.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "press" => Some(Self::Press),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "center" | "centre" => Some(Self::Center),
            _ => None,
        }
    }
}

/// The side a pull started from, latched on the frame the card first shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CardSide {
    /// The left half of the screen.
    Left,
    /// The right half.
    Right,
}

impl CardSide {
    /// Which half of `screen` `x` is in.
    pub(super) fn of(x: f32, screen: Rect) -> Self {
        if x < screen.center().x {
            Self::Left
        } else {
            Self::Right
        }
    }
}

/// The card's lengths, resolved to du from [`ShadeMetrics`]'s multiples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CardStyle {
    /// The inset from the screen's sides and bottom (`shade.pad`).
    pub inset: f32,
    /// The gap under the status bar (`shade.card_gap`).
    pub gap: f32,
    /// The corner radius (`shade.card_corner`).
    pub corner: f32,
    /// How much further in than a curtain's `pad` the content sits (`shade.card_inset − shade.pad`,
    /// never negative).
    pub extra: f32,
    /// How far above its rest the card arrives from (`shade.card_drop`).
    pub drop: f32,
}

impl CardStyle {
    /// Resolve against the theme.
    pub(super) fn new(m: &Metrics, s: ShadeMetrics) -> Self {
        let u = m.corner_radius;
        Self {
            inset: u * s.pad,
            gap: u * s.card_gap,
            corner: u * s.card_corner,
            extra: (u * (s.card_inset - s.pad)).max(0.0),
            drop: u * s.card_drop,
        }
    }

    /// The corner, for every corner.
    pub(super) fn corners(&self) -> egui::CornerRadius {
        egui::CornerRadius::same(self.corner.round().clamp(0.0, 255.0) as u8)
    }
}

/// Past this share of the usable width a card would leave only a sliver of page beside it, and
/// it takes the whole width instead — a phone's shade rather than a tablet's.
const FILL_PAST: f32 = 0.75;

/// **Where a card of height `height` rests** on `screen` (the overlay's screen: under a visible
/// status bar).
///
/// The width is `width_ratio` of the screen or what the tile row needs (`min_width`, one touch
/// target a tile), whichever is more. Where that would cover more than [`FILL_PAST`] of the width
/// there is no page worth keeping beside it: the card takes the whole width less two insets, the
/// way a phone's shade does, and the anchor stops mattering. `side` is the latched pull side for
/// [`CardAnchor::Press`]; `None` rests in the middle.
pub(super) fn rest(
    screen: Rect,
    height: f32,
    width_ratio: f32,
    min_width: f32,
    anchor: CardAnchor,
    side: Option<CardSide>,
    style: &CardStyle,
) -> Rect {
    let widest = (screen.width() - style.inset * 2.0).max(1.0);
    let need = (screen.width() * width_ratio).max(min_width);
    let w = if need > widest * FILL_PAST {
        widest
    } else {
        need
    };
    let side = match anchor {
        CardAnchor::Left => Some(CardSide::Left),
        CardAnchor::Right => Some(CardSide::Right),
        CardAnchor::Center => None,
        CardAnchor::Press => side,
    };
    let x = match side {
        Some(CardSide::Left) => screen.min.x + style.inset,
        Some(CardSide::Right) => screen.max.x - style.inset - w,
        None => screen.center().x - w * 0.5,
    };
    Rect::from_min_size(pos2(x, screen.min.y + style.gap), vec2(w, height))
}

/// How much of the arrival the content waits out before it starts to show (a share of the
/// progress). The plate comes first and the content resolves inside it — that is what makes a
/// faint blob read as "something arriving" rather than "a transparent panel".
const CONTENT_LAG: f32 = 0.35;

/// The soft plate's share of the plate's own alpha while it is still a blob.
const SOFT_ALPHA: f32 = 0.6;

/// **How far a card has arrived** at progress `p` (0 = not there, 1 = at rest).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Arrival {
    /// How far above its rest it still is, as a share of the drop (1 → 0).
    pub rise: f32,
    /// The plate's opacity (0 → 1).
    pub plate: f32,
    /// The content's opacity (0 → 1, starting after [`CONTENT_LAG`]).
    pub content: f32,
    /// The soft edge's width as a share of the corner radius (1 → 0), and the soft plate's
    /// opacity as a share of the plate's.
    pub soft: f32,
}

/// The arrival at `p`. Everything is monotonic in `p` and lands exactly at `p = 1`.
///
/// The curve's shape comes from what drives `p`: the pull's share of the stop, trailed by
/// [`follow`] on the way in — so a flick plays as an ease-out over about three [`LAG_S`] and a slow
/// pull tracks the finger. Nothing here adds an easing of its own.
pub(super) fn arrival(p: f32) -> Arrival {
    let p = p.clamp(0.0, 1.0);
    Arrival {
        rise: 1.0 - p,
        plate: p,
        content: ((p - CONTENT_LAG) / (1.0 - CONTENT_LAG)).clamp(0.0, 1.0),
        soft: 1.0 - p,
    }
}

/// **How long a card takes to catch the pull up** (s) — the time constant of its arrival on the
/// way in.
///
/// Tied 1:1 to the finger, a flick covers the stop in a frame or two and the whole arrival —
/// the fade, the drop, the edge sharpening — happens where nobody can see it (the first real
/// capture showed exactly that). One UI plays it over about 0.34 s however fast the hand was;
/// three time constants of 0.1 s is the same. On a slow pull the card is never more than a tenth
/// of a second behind the finger, which is not felt. On the way out it does not trail at all: a
/// card leaving is gone at the speed it is sent.
pub(super) const LAG_S: f32 = 0.1;

/// Closer than this the card is where the pull says — under one step of an 8-bit opacity.
const SETTLED: f32 = 1.0 / 255.0;

/// The arrival as drawn this frame: `shown` moved towards `target` over `dt` seconds.
///
/// Rising, it closes the gap exponentially at [`LAG_S`] and snaps once within [`SETTLED`], so it
/// lands exactly rather than forever approaching. Falling, or under `reduce`, it is the target.
pub(super) fn follow(shown: f32, target: f32, dt: f32, reduce: bool) -> f32 {
    if reduce || target <= shown {
        return target;
    }
    let next = shown + (target - shown) * (1.0 - (-dt.max(0.0) / LAG_S).exp());
    if target - next < SETTLED {
        target
    } else {
        next
    }
}

/// The soft plate's colour: the plate's, at [`SOFT_ALPHA`] of the plate's opacity.
pub(super) fn soft_fill(plate: egui::Color32, arrival: Arrival) -> egui::Color32 {
    plate.gamma_multiply(arrival.plate * SOFT_ALPHA)
}

#[cfg(test)]
mod tests {
    use super::{arrival, follow, rest, CardAnchor, CardSide, CardStyle, OverlayReveal, LAG_S};
    use egui::{pos2, Rect};

    fn style() -> CardStyle {
        CardStyle {
            inset: 16.0,
            gap: 8.0,
            corner: 48.0,
            extra: 16.0,
            drop: 36.0,
        }
    }

    /// A wide screen: the share of the width, on the side pulled from, under the status bar.
    #[test]
    fn a_wide_screen_rests_the_card_on_the_side_pulled_from() {
        let screen = Rect::from_min_max(pos2(0.0, 40.0), pos2(1280.0, 800.0));
        let s = style();
        let left = rest(
            screen,
            600.0,
            0.46,
            200.0,
            CardAnchor::Press,
            Some(CardSide::Left),
            &s,
        );
        let right = rest(
            screen,
            600.0,
            0.46,
            200.0,
            CardAnchor::Press,
            Some(CardSide::Right),
            &s,
        );
        let middle = rest(screen, 600.0, 0.46, 200.0, CardAnchor::Press, None, &s);
        assert!((left.width() - 1280.0 * 0.46).abs() < 0.01);
        assert!((left.min.x - 16.0).abs() < 0.01, "{left:?}");
        assert!((right.max.x - (1280.0 - 16.0)).abs() < 0.01, "{right:?}");
        assert!((middle.center().x - 640.0).abs() < 0.01, "{middle:?}");
        assert!((left.min.y - 48.0).abs() < 0.01 && (left.height() - 600.0).abs() < 0.01);
        // A fixed anchor ignores the side.
        let fixed = rest(
            screen,
            600.0,
            0.46,
            200.0,
            CardAnchor::Right,
            Some(CardSide::Left),
            &s,
        );
        assert_eq!(fixed, right);
    }

    /// A narrow screen: as wide as the screen allows, whatever the side; and never narrower than
    /// the tile row needs.
    #[test]
    fn a_narrow_screen_gives_the_card_the_whole_width() {
        let screen = Rect::from_min_max(pos2(0.0, 40.0), pos2(480.0, 800.0));
        let s = style();
        let card = rest(
            screen,
            600.0,
            0.46,
            400.0,
            CardAnchor::Press,
            Some(CardSide::Left),
            &s,
        );
        assert!((card.width() - (480.0 - 32.0)).abs() < 0.01, "{card:?}");
        assert!((card.min.x - 16.0).abs() < 0.01 && (card.max.x - 464.0).abs() < 0.01);
        let other = rest(
            screen,
            600.0,
            0.46,
            400.0,
            CardAnchor::Press,
            Some(CardSide::Right),
            &s,
        );
        assert_eq!(
            card, other,
            "the side cannot matter where the card fills the width"
        );
        // The row's need wins over a share too narrow for it…
        let wide = Rect::from_min_max(pos2(0.0, 40.0), pos2(1280.0, 800.0));
        let roomy = rest(wide, 600.0, 0.2, 500.0, CardAnchor::Center, None, &s);
        assert!((roomy.width() - 500.0).abs() < 0.01, "{roomy:?}");
        // …until it would leave only a sliver beside it, and then it takes the whole width.
        let crowded = rest(wide, 600.0, 0.2, 1000.0, CardAnchor::Center, None, &s);
        assert!(
            (crowded.width() - (1280.0 - 32.0)).abs() < 0.01,
            "{crowded:?}"
        );
    }

    /// The arrival: everything monotonic, the content waiting for the plate, all landing at 1.
    #[test]
    fn the_arrival_lands_with_the_content_behind_the_plate() {
        let start = arrival(0.0);
        assert!(start.plate.abs() < f32::EPSILON && start.content.abs() < f32::EPSILON);
        assert!((start.rise - 1.0).abs() < f32::EPSILON && (start.soft - 1.0).abs() < f32::EPSILON);
        let early = arrival(0.2);
        assert!(
            early.plate > 0.0 && early.content.abs() < f32::EPSILON,
            "{early:?}"
        );
        let mut last = start;
        for i in 1..=20 {
            #[expect(clippy::cast_precision_loss, reason = "twenty steps")]
            let a = arrival(i as f32 / 20.0);
            assert!(a.plate >= last.plate && a.content >= last.content, "{a:?}");
            assert!(a.rise <= last.rise && a.soft <= last.soft, "{a:?}");
            last = a;
        }
        let end = arrival(1.0);
        assert!((end.plate - 1.0).abs() < f32::EPSILON && (end.content - 1.0).abs() < f32::EPSILON);
        assert!(end.rise.abs() < f32::EPSILON && end.soft.abs() < f32::EPSILON);
        assert_eq!(arrival(1.5), end, "it is clamped past the end");
    }

    /// The trailing arrival: a flick's jump to the end is spread over about three time
    /// constants, lands exactly, never overshoots; the way out and `reduce` do not trail.
    #[test]
    fn a_flick_still_shows_the_arrival_and_lands_exactly() {
        let dt = 1.0 / 60.0;
        let mut shown = 0.0;
        let mut frames = 0;
        while shown < 1.0 && frames < 600 {
            let next = follow(shown, 1.0, dt, false);
            assert!(next > shown && next <= 1.0, "{shown} -> {next}");
            shown = next;
            frames += 1;
        }
        #[expect(clippy::cast_precision_loss, reason = "a few dozen frames")]
        let took = frames as f32 * dt;
        assert!(
            took > LAG_S * 2.0 && took < LAG_S * 8.0,
            "the arrival is spread over a few time constants: {took} s"
        );
        assert!((shown - 1.0).abs() < f32::EPSILON, "it lands exactly");
        // A third of the way in after one time constant's worth of frames, give or take.
        let mut early = 0.0;
        for _ in 0..6 {
            early = follow(early, 1.0, dt, false);
        }
        assert!(early > 0.5 && early < 0.75, "after 0.1 s: {early}");
        assert!(
            (follow(0.8, 0.2, dt, false) - 0.2).abs() < f32::EPSILON,
            "out: at once"
        );
        assert!(
            (follow(0.0, 1.0, dt, true) - 1.0).abs() < f32::EPSILON,
            "reduce: at once"
        );
    }

    /// The config names.
    #[test]
    fn the_names_parse() {
        assert_eq!(OverlayReveal::parse("card"), Some(OverlayReveal::Card));
        assert_eq!(
            OverlayReveal::parse(" Curtain "),
            Some(OverlayReveal::Curtain)
        );
        assert_eq!(OverlayReveal::parse("sheet"), None);
        assert_eq!(CardAnchor::parse("centre"), Some(CardAnchor::Center));
        assert_eq!(CardAnchor::parse("press"), Some(CardAnchor::Press));
        assert_eq!(CardAnchor::parse("top"), None);
    }
}
