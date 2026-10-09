//! The transition driver — the state machines and render mappings for home ↔ task (the A2 icon
//! zoom) and screen push/pop (A3). Pure functions, so headless unit testing is easy.
//!
//! The two mappings take their arguments differently: [`a2_open`] / `a2_close`
//! take a **raw `t`** and ease it inside (the windows are reckoned on the raw `t`), while
//! [`a3_push`] / [`a3_pop`] take an **already eased** value. The drawing is
//! `Workspace::draw_panes`'s — this file only produces values.

use crate::motion::{Animated, Easing, Spring, Tween};
use crate::theme::MotionTokens;
use crate::workspace::Instance;
use egui::Rect;
use std::time::Duration;

/// The fallback duration when there is no icon Rect (A2, "a fade plus a scale of 0.96 → 1,
/// 200 ms"). The same for opening and closing. `reduce` (a 0 ms tween) stays 0.
pub(crate) const FALLBACK_MS: u64 = 200;

/// The fallback scale's starting value (A2).
pub(crate) const FALLBACK_SCALE: f32 = 0.96;

/// The placeholder card's starting corner radius (px, A2 "12 → 0").
pub(crate) const CARD_RADIUS: f32 = 12.0;

/// How much the icon at the centre of the card grows by the end — A2's "48 → 96" as a ratio of
/// the desktop icon it zooms from (`metrics.icon_size`), so an icon of any size starts the
/// animation at its own size rather than jumping to 48 px on the first frame.
pub(crate) const CARD_ICON_GROW: f32 = 2.0;

/// The width of the shadow band down the A3 incoming layer's left edge (px, A3 "an 8 px shadow band").
pub(crate) const SHADOW_BAND_PX: f32 = 8.0;

/// The shadow band's four alpha steps — from the outside (left) inwards (the layer's edge). Four steps of 2 px.
pub(crate) const SHADOW_BAND_ALPHA: [f32; 4] = [0.04, 0.08, 0.14, 0.22];

/// On the fallback (no icon Rect) the tween's duration becomes [`FALLBACK_MS`]. The curve is
/// unchanged, and so is 0 ms (`reduce`).
#[must_use]
pub(crate) fn home_tween(icon_rect: Option<Rect>, tween: Tween) -> Tween {
    if icon_rect.is_some() || tween.duration.is_zero() {
        tween
    } else {
        Tween {
            duration: Duration::from_millis(FALLBACK_MS),
            easing: tween.easing,
        }
    }
}

/// A transition's driving value `t` is the **raw progress** (elapsed / duration) — [`Animated`]
/// runs linearly and the easing is applied once, by the render mapping (inside [`a2_open`]; for A3
/// `draw_panes` does `tokens.push.easing.apply(t)`). Handing `Animated::to` an easing tween as it
/// stands makes `value()` already eased, so it would be applied twice.
fn linear_driver(from: f32, tween: Tween) -> Animated<f32> {
    let mut t = Animated::new(from);
    t.to(
        1.0,
        Tween {
            duration: tween.duration,
            easing: Easing::Linear,
        },
    );
    t
}

/// The A2 state machine. `InTask` is expressed as `Idle` plus `WorkspaceView::Tasks`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[doc(hidden)]
pub enum HomeTransition {
    /// None.
    Idle,
    /// Home → task.
    Opening {
        /// `t ∈ [0, 1]`.
        t: Animated<f32>,
        /// The tapped icon's Rect. Without it, the fallback (a fade plus a scale).
        icon_rect: Option<Rect>,
    },
    /// Task → home.
    Closing {
        /// `t ∈ [0, 1]`.
        t: Animated<f32>,
        /// The icon Rect to return to.
        icon_rect: Option<Rect>,
        /// Where the screen starts from, where not its pane: a lift let go into home
        /// carries on from where the finger left it.
        from: Option<Rect>,
    },
}

impl HomeTransition {
    /// Whether it is running.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// Start opening. Mid-`Closing` it continues from `t` in reverse (the A2 interruption rule).
    /// With no `icon_rect` the duration is [`FALLBACK_MS`] (`home_tween`).
    pub fn open(&mut self, icon_rect: Option<Rect>, tween: Tween) {
        let start = match *self {
            Self::Closing { t, .. } => 1.0 - t.value(),
            _ => 0.0,
        };
        let t = linear_driver(start, home_tween(icon_rect, tween));
        *self = Self::Opening { t, icon_rect };
    }

    /// Start closing. Mid-`Opening` it continues from `t` in reverse. With no `icon_rect` the duration is [`FALLBACK_MS`].
    pub fn close(&mut self, icon_rect: Option<Rect>, tween: Tween) {
        let start = match *self {
            Self::Opening { t, .. } => 1.0 - t.value(),
            _ => 0.0,
        };
        let t = linear_driver(start, home_tween(icon_rect, tween));
        *self = Self::Closing {
            t,
            icon_rect,
            from: None,
        };
    }

    /// Start closing from `from` rather than the pane — a lifted screen let go into home.
    /// It always starts at the beginning: the screen is where the finger left it.
    pub(crate) fn close_from(&mut self, icon_rect: Option<Rect>, from: Rect, tween: Tween) {
        let t = linear_driver(0.0, home_tween(icon_rect, tween));
        *self = Self::Closing {
            t,
            icon_rect,
            from: Some(from),
        };
    }

    /// Advance. On finishing it returns to `Idle` and hands back `true` (once, on the frame it finished).
    pub fn tick(&mut self, dt: f32) -> bool {
        match self {
            Self::Idle => false,
            Self::Opening { t, .. } | Self::Closing { t, .. } => {
                if t.tick(dt) {
                    false
                } else {
                    *self = Self::Idle;
                    true
                }
            }
        }
    }

    /// Finish at once (a new launch mid-transition, or reduce).
    pub fn finish(&mut self) {
        *self = Self::Idle;
    }

    /// The current `t` (raw progress, before easing).
    #[must_use]
    pub fn t(&self) -> f32 {
        match self {
            Self::Idle => 1.0,
            Self::Opening { t, .. } | Self::Closing { t, .. } => t.value(),
        }
    }

    /// This transition's icon Rect (the fallback path without one).
    #[must_use]
    pub fn icon_rect(&self) -> Option<Rect> {
        match self {
            Self::Idle => None,
            Self::Opening { icon_rect, .. } | Self::Closing { icon_rect, .. } => *icon_rect,
        }
    }
}

/// The A2 render mapping (as for opening, A2 "the render mapping").
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct A2Mapping {
    /// The desktop layer's scale (pivoted at its centre).
    pub(crate) desktop_scale: f32,
    /// The desktop's opacity.
    pub(crate) desktop_opacity: f32,
    /// The placeholder card's Rect.
    pub(crate) card_rect: Rect,
    /// The card's corner radius.
    pub(crate) card_radius: f32,
    /// The icon at the centre of the card, as a multiple of the desktop icon's size: `1` at the
    /// icon's own size, [`CARD_ICON_GROW`] at the end of the opening.
    pub(crate) icon_grow: f32,
    /// The card icon's opacity.
    pub(crate) icon_opacity: f32,
    /// The real screen's opacity (0 = not drawn while `t < 0.5`).
    pub(crate) screen_opacity: f32,
}

/// The A2 fallback mapping — for when there is no icon Rect (A2, "a fade plus a scale of
/// 0.96 → 1, 200 ms"). There is no card; the real screen layer is faded and scaled whole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct A2Fallback {
    /// The desktop layer's scale (pivoted at its centre).
    pub(crate) desktop_scale: f32,
    /// The desktop's opacity.
    pub(crate) desktop_opacity: f32,
    /// The screen layer's scale (pivoted at its centre).
    pub(crate) screen_scale: f32,
    /// The screen's opacity.
    pub(crate) screen_opacity: f32,
}

fn window(t: f32, from: f32, to: f32) -> f32 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

/// The opening mapping. **`t` is the raw progress** and the geometry goes through
/// `tokens.home_open.easing` inside (the A2 test: at `t = 0.5` the card ==
/// `lerp(icon, pane, CubicOut(0.5))`). The windows are reckoned on the raw `t`. A3's [`a3_push`] /
/// [`a3_pop`] take the opposite — an **eased** value. Do not mix the two conventions.
///
/// The card: Rect `lerp(icon, pane, s)`, corners `12 → 0`, the icon `1× → 2×` its size (all at
/// `s = ease(t)`), with the icon fading out over `t ∈ [0.5, 0.8]`. The real screen runs from
/// `t ≥ 0.5` at `(t − 0.5) / 0.5`. The card's colour (the declaration's `background`, or surface)
/// and the drawing are `Workspace`'s. With no icon Rect it is `a2_fallback_open` instead of this
/// function.
#[must_use]
pub(crate) fn a2_open(
    t: f32,
    icon_rect: Rect,
    pane_rect: Rect,
    tokens: &MotionTokens,
) -> A2Mapping {
    let s = tokens.home_open.easing.apply(t);
    A2Mapping {
        desktop_scale: 1.0 + (tokens.desktop_scale - 1.0) * s,
        desktop_opacity: 1.0 - window(t, 0.0, 0.6),
        card_rect: icon_rect.lerp_towards(&pane_rect, s),
        card_radius: CARD_RADIUS * (1.0 - s),
        icon_grow: 1.0 + (CARD_ICON_GROW - 1.0) * s,
        icon_opacity: 1.0 - window(t, 0.5, 0.8),
        screen_opacity: window(t, 0.5, 1.0),
    }
}

/// The closing mapping (A2, "the render mapping (closing)"). The argument convention is
/// [`a2_open`]'s (a raw `t`). Symmetric with opening: the screen `1 → 0` (`t ∈ [0, 0.4]`), the card
/// `pane → icon`, the corners `0 → 12`, the icon `2× → 1×` fading in over `t ∈ [0.5, 0.8]`, and the
/// desktop scaling `0.92 → 1` with its opacity over `t ∈ [0.3, 1]`. With no icon Rect,
/// [`a2_fallback_close`].
#[must_use]
pub(crate) fn a2_close(
    t: f32,
    icon_rect: Rect,
    pane_rect: Rect,
    tokens: &MotionTokens,
) -> A2Mapping {
    let s = tokens.home_close.easing.apply(t);
    A2Mapping {
        desktop_scale: tokens.desktop_scale + (1.0 - tokens.desktop_scale) * s,
        desktop_opacity: window(t, 0.3, 1.0),
        card_rect: pane_rect.lerp_towards(&icon_rect, s),
        card_radius: CARD_RADIUS * s,
        icon_grow: CARD_ICON_GROW - (CARD_ICON_GROW - 1.0) * s,
        icon_opacity: window(t, 0.5, 0.8),
        screen_opacity: 1.0 - window(t, 0.0, 0.4),
    }
}

/// The fallback opening mapping (a raw `t`, eased inside by `tokens.home_open.easing`). The screen
/// scales `0.96 → 1` at an opacity of `0 → 1`; the desktop is as in [`a2_open`].
#[must_use]
pub(crate) fn a2_fallback_open(t: f32, tokens: &MotionTokens) -> A2Fallback {
    let s = tokens.home_open.easing.apply(t);
    A2Fallback {
        desktop_scale: 1.0 + (tokens.desktop_scale - 1.0) * s,
        desktop_opacity: 1.0 - window(t, 0.0, 0.6),
        screen_scale: FALLBACK_SCALE + (1.0 - FALLBACK_SCALE) * s,
        screen_opacity: s,
    }
}

/// The fallback closing mapping (a raw `t`, eased inside by `tokens.home_close.easing`). The screen
/// scales `1 → 0.96` at an opacity of `1 → 0`; the desktop is as in [`a2_close`].
#[must_use]
pub(crate) fn a2_fallback_close(t: f32, tokens: &MotionTokens) -> A2Fallback {
    let s = tokens.home_close.easing.apply(t);
    A2Fallback {
        desktop_scale: tokens.desktop_scale + (1.0 - tokens.desktop_scale) * s,
        desktop_opacity: window(t, 0.3, 1.0),
        screen_scale: 1.0 - (1.0 - FALLBACK_SCALE) * s,
        screen_opacity: 1.0 - s,
    }
}

/// The A3 state machine. The back gesture is [`StackTransition::DraggingBack`] (M2, A3).
#[derive(Debug)]
#[doc(hidden)]
pub enum StackTransition {
    /// None.
    Idle,
    /// Pushing (the incoming one = the top of the stack).
    Pushing {
        /// `t ∈ [0, 1]`.
        t: Animated<f32>,
    },
    /// Popping. The outgoing instance is already off the stack and lives here.
    Popping {
        /// `t ∈ [0, 1]`.
        t: Animated<f32>,
        /// The outgoing instance (dropped after `Destroyed` at the end of the transition). Boxed because of the size difference.
        outgoing: Option<Box<Instance>>,
    },
    /// The back gesture (M2, A3): a drag from the left edge. `p ∈ [0, 1]` = the pop progress,
    /// 1:1 with the finger. Before the release (`confirmed == None`) the outgoing one is still at
    /// the top of the stack. Confirmed, it comes off the stack into `outgoing` and `p` springs to
    /// 1; cancelled, it springs to 0 and then `Idle`. The rendering uses `a3_pop(p)` **without
    /// easing** (s = p).
    ///
    /// If a finger takes hold again mid-cancel, `StackTransition::regrab_back` puts `confirmed`
    /// back to `None` and continues the drag **from the current `p`** (A3 interruption and
    /// re-entry).
    DraggingBack {
        /// `p ∈ [0, 1]`.
        p: Animated<f32>,
        /// The outgoing instance, once confirmed.
        outgoing: Option<Box<Instance>>,
        /// The release's outcome. `None` means the finger is leading.
        confirmed: Option<bool>,
    },
}

impl StackTransition {
    /// Whether it is running.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// Start a push. `t` is the raw progress (the easing is `draw_panes`'s, through `tokens.push.easing`).
    pub fn push(&mut self, tween: Tween) {
        let t = linear_driver(0.0, tween);
        *self = Self::Pushing { t };
    }

    /// Start a pop. `t` is the raw progress (the easing is `tokens.pop.easing`).
    pub fn pop(&mut self, outgoing: Instance, tween: Tween) {
        let t = linear_driver(0.0, tween);
        *self = Self::Popping {
            t,
            outgoing: Some(Box::new(outgoing)),
        };
    }

    /// Start the back gesture (`p = 0`, following the finger).
    pub(crate) fn begin_back(&mut self) {
        let mut p = Animated::new(0.0);
        p.drag(0.0, 0.0);
        *self = Self::DraggingBack {
            p,
            outgoing: None,
            confirmed: None,
        };
    }

    /// Take hold again mid-cancel (A3, "taking hold again during a cancelled gesture continues
    /// from the current p"). On catching it, it hands back the `p` at that point. After a
    /// confirmation (`Some(true)`) the stack has already changed and cannot be undone, so it
    /// catches **only while cancelling**.
    pub(crate) fn regrab_back(&mut self) -> Option<f32> {
        let Self::DraggingBack {
            p,
            outgoing: None,
            confirmed,
        } = self
        else {
            return None;
        };
        if *confirmed != Some(false) {
            return None;
        }
        *confirmed = None;
        let value = p.value();
        p.drag(value, 0.0);
        Some(value)
    }

    /// Gesture progress: `p = clamp(dx / W, 0, 1)`, `v_p = v_x / W`.
    pub(crate) fn drag_back(&mut self, p_value: f32, v_p: f32) {
        if let Self::DraggingBack {
            p, confirmed: None, ..
        } = self
        {
            p.drag(p_value.clamp(0.0, 1.0), v_p);
        }
    }

    /// The release: confirmed, it takes `outgoing` (the top pulled off the stack) and springs to 1; cancelled, to 0.
    pub(crate) fn release_back(
        &mut self,
        confirm: bool,
        outgoing: Option<Instance>,
        spring: Spring,
        reduce: bool,
        width: f32,
    ) {
        if let Self::DraggingBack {
            p,
            outgoing: slot,
            confirmed,
        } = self
        {
            *confirmed = Some(confirm);
            *slot = outgoing.map(Box::new);
            let target = if confirm { 1.0 } else { 0.0 };
            if reduce {
                p.snap(target);
            } else {
                // `p = dx / W` is a normalised value, so the settle test (0.5 px · 10 px/s) is pinched by
                // `1 / W` — otherwise it settles on the first tick and A3's "a Spring on release" is never seen.
                p.release_scaled(target, spring, 1.0 / width.max(1.0));
            }
        }
    }

    /// The back gesture's release outcome (`None` = still the finger's).
    #[must_use]
    pub fn back_confirmed(&self) -> Option<bool> {
        match self {
            Self::DraggingBack { confirmed, .. } => *confirmed,
            _ => None,
        }
    }

    /// Advance. On the finishing frame it hands back `Some(outgoing)` (a pop, or a confirmed
    /// gesture) or `Some(None)` (a push, or a cancelled gesture).
    pub fn tick(&mut self, dt: f32) -> Option<Option<Instance>> {
        match self {
            Self::Idle => None,
            Self::DraggingBack {
                p,
                outgoing,
                confirmed,
            } => {
                if confirmed.is_none() || p.tick(dt) {
                    None
                } else {
                    let out = outgoing.take().map(|b| *b);
                    *self = Self::Idle;
                    Some(out)
                }
            }
            Self::Pushing { t } => {
                if t.tick(dt) {
                    None
                } else {
                    *self = Self::Idle;
                    Some(None)
                }
            }
            Self::Popping { t, outgoing } => {
                if t.tick(dt) {
                    None
                } else {
                    let out = outgoing.take().map(|b| *b);
                    *self = Self::Idle;
                    Some(out)
                }
            }
        }
    }

    /// Finish at once. On a pop it hands the outgoing instance back.
    pub fn finish(&mut self) -> Option<Instance> {
        let out = match self {
            Self::Popping { outgoing, .. } | Self::DraggingBack { outgoing, .. } => {
                outgoing.take().map(|b| *b)
            }
            _ => None,
        };
        *self = Self::Idle;
        out
    }

    /// The current `t` (raw progress, before easing).
    #[must_use]
    pub fn t(&self) -> f32 {
        match self {
            Self::Idle => 1.0,
            Self::Pushing { t } | Self::Popping { t, .. } => t.value(),
            Self::DraggingBack { p, .. } => p.value(),
        }
    }
}

/// The A3 render mapping (A3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct A3Mapping {
    /// The incoming (upper) layer's x offset.
    pub(crate) x_in: f32,
    /// The outgoing (lower) layer's x offset.
    pub(crate) x_out: f32,
    /// The lower layer's dim alpha.
    pub(crate) dim: f32,
    /// The strength of the shadow band down the incoming layer's left edge (`0 → 1`, multiplied
    /// into [`SHADOW_BAND_ALPHA`]). It is 0 while the incoming layer is off-screen (`x_in = W`), so
    /// the band does not jump on a transition's first and last frames.
    pub(crate) shadow: f32,
}

impl A3Mapping {
    /// No transition (offsets, dim and shadow all 0).
    pub(crate) const IDLE: Self = Self {
        x_in: 0.0,
        x_out: 0.0,
        dim: 0.0,
        shadow: 0.0,
    };
}

/// The push mapping. `eased` is the **already eased** progress (`tokens.push.easing.apply(t)`) —
/// `Workspace::draw_panes` works it out beforehand and hands it over. Handing a raw `t` in still
/// gives a monotonic 0→1, so it does not show; be careful (a different convention from
/// [`a2_open`]).
///
/// The incoming layer at `x = (1 − s)·W`, the outgoing at `x = −parallax·W·s`, the dim at `dim·s`,
/// and the shadow band's strength at `s` (= how much of the incoming layer is on screen).
#[must_use]
pub(crate) fn a3_push(eased: f32, width: f32, tokens: &MotionTokens) -> A3Mapping {
    let s = eased.clamp(0.0, 1.0);
    A3Mapping {
        x_in: (1.0 - s) * width,
        x_out: -tokens.parallax * width * s,
        dim: tokens.dim * s,
        shadow: s,
    }
}

/// The pop mapping = the push's `1 − eased`. `eased` is the **already eased** progress (`tokens.pop.easing`).
#[must_use]
pub(crate) fn a3_pop(eased: f32, width: f32, tokens: &MotionTokens) -> A3Mapping {
    a3_push(1.0 - eased, width, tokens)
}

/// The clear-top cross-fade mapping: the opacity of the top screen being cleared,
/// `1 → 0`. `t` is the **raw** progress and the easing (`tokens.clear_top.easing`) is applied once,
/// here — the screen below stays put, so there is no movement and no dim.
///
/// The driving value is held separately by `Workspace` rather than being a [`StackTransition`]
/// variant: **several** instances are being cleared, so they do not fit in the one
/// `Popping.outgoing` slot, and with no movement there is no need for an [`A3Mapping`] either.
#[must_use]
pub(crate) fn clear_top_alpha(t: f32, tokens: &MotionTokens) -> f32 {
    1.0 - tokens.clear_top.easing.apply(t.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::{
        a2_close, a2_fallback_close, a2_fallback_open, a2_open, a3_pop, a3_push, home_tween,
        A3Mapping, HomeTransition, StackTransition, CARD_ICON_GROW, CARD_RADIUS, FALLBACK_MS,
        FALLBACK_SCALE,
    };
    use crate::motion::{Easing, Tween};
    use crate::theme::MotionTokens;
    use egui::{pos2, Rect};
    use std::time::Duration;

    fn icon() -> Rect {
        Rect::from_min_size(pos2(100.0, 200.0), egui::vec2(96.0, 96.0))
    }

    fn pane() -> Rect {
        Rect::from_min_size(pos2(0.0, 32.0), egui::vec2(1024.0, 512.0))
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// The A2 test: at t = 0.5 the card == `lerp(icon, pane, CubicOut(0.5) = 0.875)`.
    #[test]
    fn a2_card_follows_eased_t() {
        let tokens = MotionTokens::default();
        let m = a2_open(0.5, icon(), pane(), &tokens);
        let s = Easing::CubicOut.apply(0.5);
        assert!(close(s, 0.875));
        let expected = icon().lerp_towards(&pane(), s);
        assert!(close(m.card_rect.min.x, expected.min.x));
        assert!(close(m.card_rect.min.y, expected.min.y));
        assert!(close(m.card_rect.max.x, expected.max.x));
        assert!(close(m.card_rect.max.y, expected.max.y));
        assert!(
            m.screen_opacity.abs() < 1e-6,
            "t = 0.5 is still before the screen"
        );
        assert!(close(m.card_radius, CARD_RADIUS * (1.0 - s)));
        assert!(close(m.icon_grow, 1.0 + (CARD_ICON_GROW - 1.0) * s));
        assert!(close(
            m.desktop_scale,
            1.0 + (tokens.desktop_scale - 1.0) * s
        ));
    }

    /// A2's own figures, written in rather than worked out again from the formula — a derived
    /// formula flips along with the implementation and passes even when the interpolation direction
    /// or the argument order is the wrong way round. icon = (100,200)~(196,296), pane =
    /// (0,32)~(1024,544) and CubicOut(0.5) = 0.875, so min = (12.5, 53.0), max = (920.5, 513.0).
    /// The figures also pin that the raw t goes in and is eased once, inside: eased twice,
    /// the card would be at CubicOut(0.875) instead.
    #[test]
    fn a2_open_matches_the_spec_figures() {
        let m = a2_open(0.5, icon(), pane(), &MotionTokens::default());
        assert!(
            (m.card_rect.min - pos2(12.5, 53.0)).length() < 1e-3,
            "{:?}",
            m.card_rect
        );
        assert!(
            (m.card_rect.max - pos2(920.5, 513.0)).length() < 1e-3,
            "{:?}",
            m.card_rect
        );
        assert!(m.screen_opacity.abs() < 1e-6);
    }

    /// The opening windows are reckoned on the raw t: the desktop [0, 0.6], the icon [0.5, 0.8], the screen [0.5, 1].
    #[test]
    fn a2_open_windows_use_raw_t() {
        let tokens = MotionTokens::default();
        let at = |t: f32| a2_open(t, icon(), pane(), &tokens);
        assert!(close(at(0.0).desktop_opacity, 1.0));
        assert!(close(at(0.3).desktop_opacity, 0.5));
        assert!(close(at(0.6).desktop_opacity, 0.0));
        assert!(close(at(0.5).icon_opacity, 1.0));
        assert!(close(at(0.65).icon_opacity, 0.5));
        assert!(close(at(0.8).icon_opacity, 0.0));
        assert!(close(at(0.75).screen_opacity, 0.5));
        assert!(close(at(1.0).screen_opacity, 1.0));
        let end = at(1.0);
        assert!(close(end.card_rect.min.x, pane().min.x) && close(end.card_radius, 0.0));
        assert!(close(end.icon_grow, CARD_ICON_GROW));
    }

    /// Closing is symmetric with opening: at the end the card == the icon, the desktop scale 1, the screen 0.
    #[test]
    fn a2_close_is_symmetric() {
        let tokens = MotionTokens::default();
        let start = a2_close(0.0, icon(), pane(), &tokens);
        assert!(close(start.screen_opacity, 1.0));
        assert!(close(start.desktop_scale, tokens.desktop_scale));
        assert!(close(start.desktop_opacity, 0.0));
        assert!(close(start.card_rect.width(), pane().width()));
        assert!(close(start.icon_grow, CARD_ICON_GROW));
        let mid = a2_close(0.2, icon(), pane(), &tokens);
        assert!(
            close(mid.screen_opacity, 0.5),
            "the screen goes away over [0, 0.4]"
        );
        let end = a2_close(1.0, icon(), pane(), &tokens);
        assert!(close(end.screen_opacity, 0.0));
        assert!(close(end.desktop_scale, 1.0) && close(end.desktop_opacity, 1.0));
        assert!(
            close(end.card_rect.min.x, icon().min.x) && close(end.card_rect.max.y, icon().max.y)
        );
        assert!(close(end.card_radius, CARD_RADIUS) && close(end.icon_grow, 1.0));
        assert!(close(end.icon_opacity, 1.0));
    }

    /// The fallback: the screen scaling 0.96 → 1 at an opacity of 0 → 1 (opening) / the reverse (closing).
    #[test]
    fn a2_fallback_fades_and_scales() {
        let tokens = MotionTokens::default();
        let o0 = a2_fallback_open(0.0, &tokens);
        let o1 = a2_fallback_open(1.0, &tokens);
        assert!(close(o0.screen_scale, FALLBACK_SCALE) && close(o0.screen_opacity, 0.0));
        assert!(close(o1.screen_scale, 1.0) && close(o1.screen_opacity, 1.0));
        assert!(close(o1.desktop_scale, tokens.desktop_scale));
        let c0 = a2_fallback_close(0.0, &tokens);
        let c1 = a2_fallback_close(1.0, &tokens);
        assert!(close(c0.screen_scale, 1.0) && close(c0.screen_opacity, 1.0));
        assert!(close(c1.screen_scale, FALLBACK_SCALE) && close(c1.screen_opacity, 0.0));
        assert!(close(c1.desktop_scale, 1.0) && close(c1.desktop_opacity, 1.0));
    }

    /// The fallback duration is 200 ms — `reduce` (0 ms) and the icon case are unchanged.
    #[test]
    fn home_tween_picks_fallback_duration() {
        let tween = Tween::cubic_out(Duration::from_millis(240));
        assert_eq!(home_tween(Some(icon()), tween), tween);
        let fallback = home_tween(None, tween);
        assert_eq!(fallback.duration, Duration::from_millis(FALLBACK_MS));
        assert_eq!(fallback.easing, tween.easing);
        assert_eq!(home_tween(None, Tween::instant()), Tween::instant());

        let mut home = HomeTransition::Idle;
        home.open(None, tween);
        // 200 ms: 12 ticks (1/60) finish it; at 240 ms it would not have.
        for _ in 0..11 {
            assert!(!home.tick(1.0 / 60.0));
        }
        assert!(home.tick(1.0 / 60.0 + 1e-4), "it ends on the 12th tick");
        assert!(!home.is_active());
    }

    /// The driving value is the raw progress: even given an easing tween, `t()` is elapsed / duration (the mapping applies the easing).
    #[test]
    fn transition_t_is_raw_progress() {
        let tween = Tween::cubic_out(Duration::from_millis(220));
        let mut stack = StackTransition::Idle;
        stack.push(tween);
        for _ in 0..7 {
            assert!(stack.tick(1.0 / 60.0).is_none());
        }
        assert!(close(stack.t(), 7.0 / 60.0 / 0.22), "t = {}", stack.t());
        let mut home = HomeTransition::Idle;
        home.open(Some(icon()), Tween::cubic_out(Duration::from_millis(240)));
        for _ in 0..3 {
            home.tick(1.0 / 60.0);
        }
        assert!(close(home.t(), 3.0 / 60.0 / 0.24), "t = {}", home.t());
    }

    /// The A2 interruption rule: a `close` mid-`Opening` continues from `1 − t` (and the same the other way).
    #[test]
    fn home_transition_reverses_from_current_t() {
        let tween = Tween::cubic_out(Duration::from_millis(240));
        let mut home = HomeTransition::Idle;
        home.open(Some(icon()), tween);
        for _ in 0..6 {
            home.tick(1.0 / 60.0);
        }
        let t = home.t();
        assert!(t > 0.0 && t < 1.0);
        home.close(Some(icon()), tween);
        assert!(matches!(home, HomeTransition::Closing { .. }));
        assert!(close(home.t(), 1.0 - t));
        assert_eq!(home.icon_rect(), Some(icon()));
        home.open(None, tween);
        assert!(matches!(home, HomeTransition::Opening { .. }));
        assert!(close(home.t(), t));
        assert_eq!(home.icon_rect(), None);
        home.finish();
        assert!(!home.is_active() && close(home.t(), 1.0));
    }

    /// The A3 test: at frame 7 (≈117 ms), `x_in == (1 − CubicOut(0.53))·W ± 1 px`.
    #[test]
    fn a3_push_at_frame_seven() {
        let tokens = MotionTokens::default();
        let eased = Easing::CubicOut.apply(7.0 / 60.0 / 0.22);
        let m = a3_push(eased, 1024.0, &tokens);
        assert!((m.x_in - (1.0 - eased) * 1024.0).abs() < 1.0);
        assert!(close(m.x_out, -tokens.parallax * 1024.0 * eased));
        assert!(close(m.dim, tokens.dim * eased));
        assert!(close(m.shadow, eased));
    }

    /// A pop is `1 − s`; the shadow band is 0 at both ends, so it does not jump.
    #[test]
    fn a3_pop_mirrors_push_and_shadow_vanishes_offscreen() {
        let tokens = MotionTokens::default();
        let p = a3_pop(0.25, 1024.0, &tokens);
        let q = a3_push(0.75, 1024.0, &tokens);
        assert!(close(p.x_in, q.x_in) && close(p.x_out, q.x_out));
        assert!(close(p.dim, q.dim) && close(p.shadow, q.shadow));
        assert!(
            close(a3_push(0.0, 1024.0, &tokens).shadow, 0.0),
            "the push starts off screen"
        );
        assert!(
            close(a3_pop(1.0, 1024.0, &tokens).shadow, 0.0),
            "the pop ends off screen"
        );
        assert!(close(a3_push(1.0, 1024.0, &tokens).x_in, 0.0));
        assert!(close(a3_push(1.0, 1024.0, &tokens).dim, tokens.dim));
        assert!(
            close(a3_push(0.0, 1024.0, &tokens).x_out, A3Mapping::IDLE.x_out),
            "with s = 0 the layer below stays put"
        );
        assert!(
            close(a3_push(2.0, 1024.0, &tokens).x_in, 0.0),
            "outside the range it is clipped"
        );
    }
}
