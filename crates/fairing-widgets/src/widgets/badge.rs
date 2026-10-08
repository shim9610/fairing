//! `CountBadge` — a count or a dot, inline or laid over a corner.
//!
//! # The bug this element was written to close
//!
//! The dock has drawn a badge since M1: a `Danger` pill with the digits in `OnPrimary`. Measured,
//! `OnPrimary` on `Danger` is **2.79** in base dark — a number the operator is meant to *read*,
//! 38 % under the 4.5 a word needs. Crossing the other way is no better: `Surface` on `Primary` is
//! 3.81 in the same palette.
//!
//! So the ink is **per tone**, and it is a measurement rather than a preference:
//! [`Alert`](BadgeTone::Alert) is `Danger` written in `Surface` (6.59 / 4.78 / 6.09 / 5.80) and
//! [`Neutral`](BadgeTone::Neutral) is `Primary` written in `OnPrimary` (4.83 / 5.82 / 9.37 / 6.08).
//! Both clear 4.5 in every shipped palette. Taking the dock's badge through this element changes
//! its render — the digits go from white to near-black in three of the four palettes — and that
//! change is the fix.
//!
//! There is no `Warning` tone, and it was measured before being refused: `Surface` on `Warning` is
//! 4.42 in abyss light, 0.08 under the floor. Two tones, not three.
//!
//! # Why the digits are drawn in cells
//!
//! A badge must not breathe as the number changes, and laying the string out as one run cannot
//! give that: with proportional figures `1` has a narrower advance than `8`, so a run-laid badge
//! changes width between `11` and `88` as well as between `9` and `10`. Each glyph is therefore
//! centred in a cell as wide as a `0`, and a badge has exactly three widths in its whole life — a
//! circle at one digit, a pill at two, and `99+` at three.
//!
//! Neither Material nor Fluent states this rule. It is what "it must not jitter" actually requires.
//!
//! # Why an overlaid badge wears a halo and an inline one does not
//!
//! Laid over an icon, a badge has to read as punched through it, so it carries a `Surface` ring
//! outside the pill — 3.81–16.18 against the icon colours it lands on. Inline there is nothing
//! behind it to separate from, and the same ring on a card would be `Surface` on `SurfaceVariant`
//! at 1.18: a smudge.
//!
//! # Why `paint_over` returns a rect and not a `Response`
//!
//! It allocates nothing. The host already owns the rect and already has a `Response` — a badge that
//! allocated one would either steal the host's tap or need its own touch target beside it, and a
//! badge is not a target. What it returns is the rect it occupied, halo included, so a caller can
//! keep clear of it.

use super::BadgeLook;
use crate::cx::WidgetCx as Cx;
use crate::theme::{ColorRole, Theme};
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

/// What a badge says.
///
/// Borrowed rather than owned: a dock repaints every frame and a badge must not allocate to be
/// drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeValue<'a> {
    /// A number. Over `control.badge_max` it reads `99+`.
    Count(u32),
    /// A mark with no number — "something, and it does not matter how many".
    Dot,
    /// Short text, already in the caller's locale.
    Text(&'a str),
}

/// Which measured fill-and-ink pair a badge uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeTone {
    /// Something is wrong or waiting — `Danger` written in `Surface`.
    #[default]
    Alert,
    /// A count with no verdict attached — a cart, a queue length. `Primary` written in `OnPrimary`.
    Neutral,
}

impl BadgeTone {
    /// The pill's fill.
    #[must_use]
    pub fn fill(self) -> ColorRole {
        match self {
            Self::Alert => ColorRole::Danger,
            Self::Neutral => ColorRole::Primary,
        }
    }

    /// The digits. **Per tone, and measured** — see the module doc.
    #[must_use]
    pub fn ink(self) -> ColorRole {
        match self {
            Self::Alert => ColorRole::Surface,
            Self::Neutral => ColorRole::OnPrimary,
        }
    }
}

/// Which corner of the host an overlaid badge is centred on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeAnchor {
    /// The trailing top corner — where a dock icon and a navigation item both wear one.
    #[default]
    TopEnd,
    /// The leading top corner.
    TopStart,
    /// The trailing bottom corner.
    BottomEnd,
    /// The leading bottom corner.
    BottomStart,
}

impl BadgeAnchor {
    /// The point on `host` a badge at this anchor is centred on.
    fn on(self, host: Rect) -> egui::Pos2 {
        match self {
            Self::TopEnd => host.right_top(),
            Self::TopStart => host.left_top(),
            Self::BottomEnd => host.right_bottom(),
            Self::BottomStart => host.left_bottom(),
        }
    }
}

/// A count badge.
#[derive(Debug)]
pub struct CountBadge<'a> {
    value: BadgeValue<'a>,
    tone: BadgeTone,
    /// A du override of `control.badge_h`.
    height: Option<f32>,
    enabled: bool,
    /// Draw `Count(0)` instead of drawing nothing.
    show_zero: bool,
    /// Take the pill's space even when nothing is drawn.
    reserve: bool,
}

impl<'a> CountBadge<'a> {
    /// A badge showing `value`.
    #[must_use]
    pub fn new(value: BadgeValue<'a>) -> Self {
        Self {
            value,
            tone: BadgeTone::default(),
            height: None,
            enabled: true,
            show_zero: false,
            reserve: false,
        }
    }

    /// A count.
    #[must_use]
    pub fn count(n: u32) -> Self {
        Self::new(BadgeValue::Count(n))
    }

    /// A dot.
    #[must_use]
    pub fn dot() -> Self {
        Self::new(BadgeValue::Dot)
    }

    /// Which fill-and-ink pair.
    #[must_use]
    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self
    }

    /// Override the height (du). `control.badge_h` by default.
    ///
    /// The default is an overlay's size — 16 du at the floor, a third of a fingertip. A kiosk badge
    /// read across a room is bigger: the reference kiosk's inline count measures 0.53 T, about
    /// 1.6× this, and that is what this is for.
    #[must_use]
    pub fn height(mut self, du: f32) -> Self {
        self.height = Some(du.max(0.0));
        self
    }

    /// Enabled.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw a zero count instead of drawing nothing.
    ///
    /// Off by default, which is iOS's rule and the one the dock already followed. The reference
    /// kiosks do the opposite — they show `0` beside the basket, because there the badge is part of
    /// a button's label and a button that loses half its content when empty reflows. Both are
    /// right for their case, so it is a flag.
    #[must_use]
    pub fn show_zero(mut self, show: bool) -> Self {
        self.show_zero = show;
        self
    }

    /// Take the pill's width even when nothing is drawn, so a host row keeps its shape as the count
    /// crosses zero.
    #[must_use]
    pub fn reserve(mut self, reserve: bool) -> Self {
        self.reserve = reserve;
        self
    }

    /// Whether anything is drawn at all.
    fn visible(&self) -> bool {
        !matches!(self.value, BadgeValue::Count(0) if !self.show_zero)
    }

    /// The pill's size, without drawing it.
    #[must_use]
    pub fn measure(&self, painter: &egui::Painter, theme: &Theme) -> Vec2 {
        let h = self.height.unwrap_or(theme.control.badge_h);
        if matches!(self.value, BadgeValue::Dot) {
            return Vec2::splat(h * theme.control.badge_dot_ratio);
        }
        let text = badge_text(self.value, theme.control.badge_max);
        Vec2::new(pill_width(painter, theme, &text, h), h)
    }

    /// Draw it in the layout, as a thing of its own.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let size = self.measure(ui.painter(), cx.theme);
        let visible = self.visible();
        let take = if visible || self.reserve {
            size
        } else {
            Vec2::ZERO
        };
        let (rect, response) = ui.allocate_exact_size(take, Sense::hover());
        if !visible {
            return response;
        }
        let at = Rect::from_center_size(rect.center(), size);
        if let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.badge.as_mut()) {
            let text = badge_text(self.value, cx.theme.control.badge_max);
            custom(
                ui.painter(),
                &mut BadgeLook {
                    rect: at,
                    value: self.value,
                    text: &text,
                    tone: self.tone,
                    enabled: self.enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            return response;
        }
        let ink = Ink::of(cx.theme, self.tone, false, self.enabled);
        paint(ui.painter(), cx.theme, at, self.value, ink);
        response
    }

    /// Draw it over a rect the caller already owns.
    ///
    /// Allocates nothing — see the module doc. Returns the rect it occupied, halo included.
    #[must_use]
    pub fn paint_over(
        self,
        painter: &egui::Painter,
        theme: &Theme,
        host: Rect,
        anchor: BadgeAnchor,
    ) -> Rect {
        if !self.visible() {
            return Rect::NOTHING;
        }
        let size = self.measure(painter, theme);
        let at = Rect::from_center_size(anchor.on(host), size);
        let ink = Ink::of(theme, self.tone, true, self.enabled);
        paint(painter, theme, at, self.value, ink);
        at.expand(theme.control.stroke_mark)
    }
}

/// The drawn string. A count over `max` becomes `<max>+`.
#[must_use]
pub fn badge_text(value: BadgeValue<'_>, max: u16) -> String {
    match value {
        BadgeValue::Count(n) if n > u32::from(max) => format!("{max}+"),
        BadgeValue::Count(n) => n.to_string(),
        BadgeValue::Text(t) => t.to_owned(),
        BadgeValue::Dot => String::new(),
    }
}

/// How many digit cells a string occupies.
/// How many digit cells a string occupies. `f32`, because every use of it is a width: a
/// badge that reached the precision limit of an `f32` count would be about a metre wide.
fn cells(text: &str) -> u16 {
    u16::try_from(text.chars().count()).unwrap_or(u16::MAX)
}

/// The pill's width for `text` at height `h`.
///
/// `cells × cell` and not the string's own advance — see the module doc on jitter. A `0` is the
/// reference glyph because it is the widest digit in almost every face and the one a tabular
/// figure is cut to.
fn pill_width(painter: &egui::Painter, theme: &Theme, text: &str, h: f32) -> f32 {
    let font = egui::FontId::proportional(h * theme.control.badge_text_ratio);
    let cell = painter
        .layout_no_wrap("0".to_owned(), font, Color32::PLACEHOLDER)
        .rect
        .width();
    let inner = f32::from(cells(text)) * cell + 2.0 * theme.control.badge_pad;
    inner.max(h)
}

/// The colours one frame paints with, dimmed once at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    pill: Color32,
    digits: Color32,
    /// Transparent for an inline badge.
    halo: Color32,
}

impl Ink {
    fn of(theme: &Theme, tone: BadgeTone, overlaid: bool, enabled: bool) -> Self {
        let ink = Self {
            pill: theme.color(tone.fill()),
            digits: theme.color(tone.ink()),
            halo: if overlaid {
                theme.color(ColorRole::Surface)
            } else {
                Color32::TRANSPARENT
            },
        };
        if enabled {
            ink
        } else {
            let a = theme.control.disabled_alpha;
            Self {
                pill: ink.pill.gamma_multiply(a),
                digits: ink.digits.gamma_multiply(a),
                halo: ink.halo.gamma_multiply(a),
            }
        }
    }
}

/// The pill, its halo and its digits.
fn paint(painter: &egui::Painter, theme: &Theme, at: Rect, value: BadgeValue<'_>, ink: Ink) {
    if matches!(value, BadgeValue::Dot) {
        let r = at.width() * 0.5;
        if ink.halo.a() > 0 {
            // R11: a circle's stroke sits **outside** its path, so the halo lands on `[r, r + w]`
            // with no radius correction — the dot's own extent is untouched.
            painter.circle_stroke(
                at.center(),
                r,
                Stroke::new(theme.control.stroke_mark, ink.halo),
            );
        }
        painter.circle_filled(at.center(), r, ink.pill);
        return;
    }
    let radius = CornerRadius::same(round_u8(at.height() * 0.5));
    if ink.halo.a() > 0 {
        let w = theme.control.stroke_mark;
        painter.rect_stroke(
            at,
            radius,
            Stroke::new(w, ink.halo),
            // Outside, so the halo is a ring around the pill rather than a bite out of it.
            StrokeKind::Outside,
        );
    }
    painter.rect_filled(at, radius, ink.pill);
    let text = badge_text(value, theme.control.badge_max);
    let font = egui::FontId::proportional(at.height() * theme.control.badge_text_ratio);
    let n = f32::from(cells(&text).max(1));
    let cell = (at.width() - 2.0 * theme.control.badge_pad) / n;
    let left = at.center().x - cell * n * 0.5;
    for (slot, ch) in (0_u16..).zip(text.chars()) {
        let centre = egui::pos2(left + cell * (f32::from(slot) + 0.5), at.center().y);
        painter.text(
            centre,
            egui::Align2::CENTER_CENTER,
            ch,
            font.clone(),
            ink.digits,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{badge_text, cells, BadgeTone, BadgeValue};
    use crate::theme::{contrast, Palette, Preset};

    /// **The digits are readable on every tone in every palette** — the defect this element was
    /// written to close. The dock shipped `OnPrimary` on `Danger` at 2.79 in base dark, and a
    /// number the operator has to read is text, gated at 4.5.
    #[test]
    fn a_badges_digits_clear_the_text_floor_in_every_palette() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                let (name, mode) = (preset.as_str(), if dark { "dark" } else { "light" });
                for tone in [BadgeTone::Alert, BadgeTone::Neutral] {
                    let got = contrast(p.get(tone.ink()), p.get(tone.fill()));
                    assert!(
                        got >= 4.5,
                        "{name} {mode}: {tone:?} digits measure {got:.2} on their own pill"
                    );
                }
            }
        }
    }

    /// **And the pair the dock used would still fail**, so the reason for the per-tone ink cannot
    /// be quietly tidied away into one colour later.
    #[test]
    fn the_crossed_pairs_still_fail_which_is_why_the_ink_is_per_tone() {
        let p = Palette::preset(Preset::Base, true);
        let shipped = contrast(p.on_primary, p.danger);
        assert!(
            shipped < 4.5,
            "`OnPrimary` on `Danger` now measures {shipped:.2} in base dark — if that has changed, \
             this element's per-tone ink needs re-arguing"
        );
        let crossed = contrast(p.surface, p.primary);
        assert!(
            crossed < 4.5,
            "`Surface` on `Primary` measures {crossed:.2}"
        );
    }

    /// **A badge's pill reads on both grounds**, which is the 3.0 a filled shape needs.
    #[test]
    fn a_badges_pill_reads_on_the_page_and_on_a_card() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                for tone in [BadgeTone::Alert, BadgeTone::Neutral] {
                    for (label, ground) in [("page", p.surface), ("card", p.surface_variant)] {
                        let got = contrast(p.get(tone.fill()), ground);
                        assert!(
                            got >= 3.0,
                            "{} {}: a {tone:?} pill measures {got:.2} on the {label}",
                            preset.as_str(),
                            if dark { "dark" } else { "light" }
                        );
                    }
                }
            }
        }
    }

    /// `Warning` as a third tone, measured and refused — 0.08 under the floor in one palette.
    #[test]
    fn warning_is_not_a_third_tone_and_the_number_says_why() {
        let p = Palette::preset(Preset::Abyss, false);
        let got = contrast(p.surface, p.warning);
        assert!(
            got < 4.5,
            "`Surface` on `Warning` now measures {got:.2} in abyss light — a third tone may have \
             become possible, and the module doc says it is not"
        );
    }

    /// The overflow rule, and that a badge's cell count has exactly three values.
    #[test]
    fn a_count_over_the_maximum_reads_as_the_maximum_and_a_plus() {
        assert_eq!(badge_text(BadgeValue::Count(0), 99), "0");
        assert_eq!(badge_text(BadgeValue::Count(9), 99), "9");
        assert_eq!(badge_text(BadgeValue::Count(99), 99), "99");
        assert_eq!(badge_text(BadgeValue::Count(100), 99), "99+");
        assert_eq!(badge_text(BadgeValue::Count(u32::MAX), 99), "99+");
        assert_eq!(badge_text(BadgeValue::Dot, 99), "");
        assert_eq!(badge_text(BadgeValue::Text("new"), 99), "new");
        // Three widths in a badge's whole life, which is what "it must not jitter" needs.
        let mut seen: Vec<u16> = (0_u32..=250)
            .map(|n| cells(&badge_text(BadgeValue::Count(n), 99)))
            .collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen,
            vec![1_u16, 2, 3],
            "a count badge has more than three widths"
        );
    }

    /// A raised maximum is still a maximum, and a `u16` one cannot overflow the format.
    #[test]
    fn the_maximum_is_a_token_and_not_a_hard_coded_ninety_nine() {
        assert_eq!(badge_text(BadgeValue::Count(500), 999), "500");
        assert_eq!(badge_text(BadgeValue::Count(1000), 999), "999+");
        assert_eq!(badge_text(BadgeValue::Count(2), 1), "1+");
    }
}
