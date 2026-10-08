//! The toast queue (A6): `max_visible` at once, bottom centre (above the nav
//! bar).
//!
//! Entering, y goes `24 → 0` and opacity `0 → 1` over 160 ms `CubicOut`; it holds for
//! `duration`; leaving, opacity goes `1 → 0` over 200 ms. A new one lifts the existing ones by
//! `components.toast.lift_step` over 120 ms. A tap leaves immediately.
//! The slot spacing (16 px) is larger than that lift, so the boxes do not touch as they rise.
//!
//! **Zero heap allocations per frame**: [`ActiveToast`] holds the text galley
//! and re-lays it out only when the content or the width changes, and the icon polylines are
//! flattened once per size by the queue's [`IconCache`]. A drawing frame only stamps the galley
//! and the polylines down.

use super::hooks::{kept, ToastCx, ToastLayout, ToastLayoutCx, ToastPainter};
use super::model::{Level, Toast};
use crate::i18n::Strings;
use crate::icons::{IconCache, IconRef, IconSet, IconStyle};
use crate::motion::Animated;
use crate::theme::{ColorRole, MotionTokens, Theme, ToastMetrics};
use egui::{Color32, Rect};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

// The six dimensions (`enter_offset` · `lift_step` · `stack_gap` · `pad_x` · `icon_gap` · `accent_w`)
// are `theme.components.toast` rather than file constants. As constants, an integrator
// wanting to change one toast padding would have to climb to rung 5 of the override ladder and redraw
// the whole card with a `toast_painter` — a painter for a padding. Rung 2 keeps it a token.

/// The phase of a toast on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum ToastPhase {
    /// Entering.
    Entering,
    /// Holding.
    Holding,
    /// Leaving.
    Leaving,
}

/// A toast on screen.
#[derive(Debug)]
#[doc(hidden)]
pub struct ActiveToast {
    /// The content.
    pub toast: Toast,
    /// The enter/leave progress (0 → 1).
    pub t: Animated<f32>,
    /// How far it has been lifted (px).
    pub lift: Animated<f32>,
    /// The phase.
    pub phase: ToastPhase,
    /// When the hold expires. `None` for a hold too long for the clock to represent (a
    /// `Duration::MAX` "until tapped"): it never expires on its own.
    pub until: Option<Instant>,
    /// Last frame's rect.
    pub rect: Rect,
    /// The text galley cache — it is not re-laid out while the width is unchanged.
    /// Last frame's text height (du). Holding the galley itself would misalign the UVs when the
    /// atlas grows, so **only the height** is kept — the row height being one frame late is fine.
    text_h: f32,
    /// The wrap width the galley was built at.
    wrap: f32,
    /// Pushed again while showing: the hold starts over on the next tick.
    renew: bool,
    /// The height a painter asked for. `None` with the built-in card.
    height: Option<f32>,
}

impl ActiveToast {
    /// The vertical offset: the entry y offset plus the lift (upward is negative).
    #[must_use]
    fn offset_y(&self, m: ToastMetrics) -> f32 {
        (1.0 - self.t.value().clamp(0.0, 1.0)).mul_add(m.enter_offset, -self.lift.value())
    }

    /// This frame's opacity.
    #[must_use]
    fn opacity(&self) -> f32 {
        self.t.value().clamp(0.0, 1.0)
    }
}

/// A tap on a toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastAction {
    /// Tapped → leave immediately (by index).
    Tapped(usize),
}

/// The gap between the stack's lowest toast and the bottom of its anchor.
const STACK_MARGIN: f32 = 16.0;

/// The toast queue.
#[doc(hidden)]
pub struct ToastQueue {
    queue: VecDeque<Toast>,
    visible: Vec<ActiveToast>,
    max_visible: usize,
    default_duration: Duration,
    /// The icon polyline cache (no allocation on the render path).
    icons: IconCache,
    /// The integrator's card, in place of the built-in one (rung 5).
    painter: Option<ToastPainter>,
    /// The integrator's placement of the stack (rung 4).
    layout: Option<ToastLayout>,
    /// This frame's resting rects — the stack before the entry's rise and the lift. A buffer
    /// whose values are overwritten each frame.
    rests: Vec<Rect>,
}

impl std::fmt::Debug for ToastQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToastQueue")
            .field("queue", &self.queue)
            .field("visible", &self.visible)
            .field("max_visible", &self.max_visible)
            .field("default_duration", &self.default_duration)
            .field("painter", &self.painter.is_some())
            .field("layout", &self.layout.is_some())
            .finish_non_exhaustive()
    }
}

impl ToastQueue {
    /// `max_visible` at once, with a default display time.
    #[must_use]
    pub fn new(max_visible: usize, default_duration: Duration) -> Self {
        Self {
            queue: VecDeque::new(),
            visible: Vec::new(),
            max_visible: max_visible.max(1),
            default_duration,
            icons: IconCache::new(),
            painter: None,
            layout: None,
            rests: Vec::new(),
        }
    }

    /// Draw each toast with `painter` instead of the built-in card.
    pub(crate) fn set_painter(&mut self, painter: ToastPainter) {
        self.painter = Some(painter);
    }

    /// Place the stack with `layout`.
    pub(crate) fn set_layout(&mut self, layout: ToastLayout) {
        self.layout = Some(layout);
    }

    /// Queue one (shown on the next `tick` if there is room).
    ///
    /// **The same words again do not stack.** A toast that repeats one already showing renews
    /// that one instead — its hold starts over, and if it was on its way out it comes back — and
    /// one that repeats a toast still waiting is dropped. Two "Saved" cards one under the other
    /// say nothing the first did not, and were the mess the demo showed.
    pub fn push(&mut self, toast: Toast) {
        let same = |other: &Toast| other.text == toast.text && other.level == toast.level;
        if let Some(shown) = self.visible.iter_mut().find(|i| same(&i.toast)) {
            shown.renew = true;
            return;
        }
        if self.queue.iter().any(same) {
            return;
        }
        self.queue.push_back(toast);
    }

    /// Frame stage 5: fill slots, expire, animate. `true` while something is moving.
    pub fn tick(&mut self, dt: f32, now: Instant, tokens: &MotionTokens, m: ToastMetrics) -> bool {
        let mut animating = false;
        while self.visible.len() < self.max_visible {
            let Some(toast) = self.queue.pop_front() else {
                break;
            };
            let duration = if toast.duration.is_zero() {
                self.default_duration
            } else {
                toast.duration
            };
            let mut t = Animated::new(0.0);
            t.to(1.0, tokens.toast_in);
            self.visible.push(ActiveToast {
                toast,
                t,
                lift: Animated::new(0.0),
                phase: ToastPhase::Entering,
                until: now
                    .checked_add(tokens.toast_in.duration)
                    .and_then(|t| t.checked_add(duration)),
                rect: Rect::NOTHING,
                text_h: 0.0,
                wrap: 0.0,
                renew: false,
                height: None,
            });
        }
        // A toast pushed again while showing: the hold starts over, and one that was leaving
        // turns round.
        for item in &mut self.visible {
            if !item.renew {
                continue;
            }
            item.renew = false;
            let duration = if item.toast.duration.is_zero() {
                self.default_duration
            } else {
                item.toast.duration
            };
            item.until = now.checked_add(duration);
            if item.phase == ToastPhase::Leaving {
                item.phase = ToastPhase::Entering;
                item.t.to(1.0, tokens.toast_in);
            }
        }
        // How far it is lifted is settled again every frame as "the number of toasts that came in after
        // me × 8 px" — a new one raises it, and it comes back down by itself as the ones before it go (A6).
        let count = self.visible.len();
        for (index, item) in self.visible.iter_mut().enumerate() {
            let later = u16::try_from(count - 1 - index).unwrap_or(u16::MAX);
            let lift = m.lift_step * f32::from(later);
            if (item.lift.target() - lift).abs() > f32::EPSILON {
                item.lift.to(lift, tokens.toast_shift);
            }
            animating |= item.t.tick(dt);
            animating |= item.lift.tick(dt);
            match item.phase {
                ToastPhase::Entering if !item.t.is_animating() => item.phase = ToastPhase::Holding,
                ToastPhase::Holding if item.until.is_some_and(|until| now >= until) => {
                    item.phase = ToastPhase::Leaving;
                    item.t.to(0.0, tokens.toast_out);
                    animating = true;
                }
                _ => {}
            }
        }
        self.visible
            .retain(|item| item.phase != ToastPhase::Leaving || item.t.is_animating());
        animating
    }

    /// Leave immediately (from a tap, say).
    pub fn dismiss(&mut self, index: usize, tokens: &MotionTokens) {
        if let Some(item) = self.visible.get_mut(index) {
            if item.phase == ToastPhase::Leaving {
                return;
            }
            item.phase = ToastPhase::Leaving;
            item.t.to(0.0, tokens.toast_out);
        }
    }

    /// The toasts on screen (from the bottom up).
    #[must_use]
    pub fn visible(&self) -> &[ActiveToast] {
        &self.visible
    }

    /// How many are queued.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Whether the next `tick` brings a waiting toast in: one is queued and a slot is free. A
    /// toast waiting behind full slots moves only when one of them leaves, which its hold's
    /// deadline or its exit animation already wakes the shell for.
    #[must_use]
    pub(crate) fn has_room_for_next(&self) -> bool {
        !self.queue.is_empty() && self.visible.len() < self.max_visible
    }

    /// Whether anything is moving.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.visible
            .iter()
            .any(|i| i.t.is_animating() || i.lift.is_animating())
    }

    /// The next expiry to arm an idle repaint for (while a toast is holding).
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.visible
            .iter()
            .filter(|i| i.phase == ToastPhase::Holding)
            .filter_map(|i| i.until)
            .min()
    }

    /// The room one toast needs under the top of `anchor`: the floor of its height and the margin
    /// below it. The shell keeps at least this much when the keyboard narrows the anchor.
    pub(crate) fn room(theme: &Theme) -> f32 {
        theme.metrics.widget_height + STACK_MARGIN
    }

    /// Stage 13: draw at the bottom centre (`Area(Order::Tooltip)`), or where the integrator's
    /// layout puts them, with the built-in card or the integrator's painter. `anchor` is
    /// the content between the bars, ending above the keyboard while it is up.
    pub(crate) fn ui(
        &mut self,
        ctx: &egui::Context,
        anchor: Rect,
        theme: &Theme,
        icons: &mut IconSet,
        strings: &Strings,
    ) -> Option<ToastAction> {
        if self.visible.is_empty() {
            return None;
        }
        let mut action = None;
        let width = theme
            .metrics
            .toast_width
            .min(anchor.width() - 24.0)
            .max(96.0);
        // The same rule as a panel tile (half of `icon_size` = 24 px) — the in-chrome icon size.
        let icon_size = theme.metrics.icon_size * 0.5;
        let m = theme.components.toast;
        // `content_inset`, the one token every row-like thing uses, rather than a toast-only
        // `pad_x`. A toast that starts its text at a different distance from the edge than a list
        // row does is the same defect the settings screens had, just on a smaller surface.
        let inset = theme.metrics.content_inset;
        let text_x = inset + icon_size + m.icon_gap;
        let wrap = (width - text_x - inset).max(24.0);
        // The stack at rest: from the bottom centre up, the oldest first.
        let mut bottom = anchor.max.y - STACK_MARGIN;
        self.rests.clear();
        for item in &self.visible {
            let height = Self::row_height(item, theme);
            self.rests.push(Rect::from_min_size(
                egui::pos2(anchor.center().x - width / 2.0, bottom - height),
                egui::vec2(width, height),
            ));
            bottom -= height + m.stack_gap;
        }
        let Self {
            visible,
            icons: cache,
            painter,
            layout,
            rests,
            ..
        } = self;
        if let Some(layout) = layout.as_mut() {
            layout(
                &ToastLayoutCx {
                    area: anchor,
                    theme,
                },
                rests,
            );
        }
        for (index, item) in visible.iter_mut().enumerate() {
            // A rect the layout emptied keeps its toast off the screen, still timed.
            let Some(place) = rests.get(index).copied().and_then(kept) else {
                item.rect = Rect::NOTHING;
                continue;
            };
            let opacity = item.opacity();
            let rect = place.translate(egui::vec2(0.0, item.offset_y(m)));
            item.rect = rect;
            let response = egui::Area::new(egui::Id::new(("fairing.toast", index)))
                .order(egui::Order::Tooltip)
                .fixed_pos(rect.min)
                .constrain(false)
                .fade_in(false)
                .show(ctx, |ui| {
                    ui.set_opacity(opacity);
                    match painter.as_mut() {
                        Some(painter) => {
                            let ActiveToast { toast, height, .. } = item;
                            painter(
                                ui,
                                &mut ToastCx::new(rect, toast, theme, icons, strings, height),
                            );
                        }
                        None => paint_toast(ui, rect, item, theme, cache, icon_size, wrap),
                    }
                    ui.allocate_rect(rect, egui::Sense::click())
                })
                .inner;
            if response.clicked() {
                action = Some(ToastAction::Tapped(index));
            }
        }
        if let Some(ToastAction::Tapped(index)) = action {
            // A tap turns it to the exit where it stands — the shell calls `dismiss` on the same frame, but
            // calling twice does not restart the curve (it is already `Leaving`).
            self.dismiss(index, &theme.motion);
        }
        action
    }

    /// The row height: what a painter asked for, or the text height plus its padding — at least
    /// `widget_height` either way.
    fn row_height(item: &ActiveToast, theme: &Theme) -> f32 {
        item.height
            .unwrap_or(item.text_h + 24.0)
            .max(theme.metrics.widget_height)
    }
}

/// One toast row: the background, the severity icon and the text. Galley- and polyline-cached, so no allocation.
fn paint_toast(
    ui: &egui::Ui,
    rect: Rect,
    item: &mut ActiveToast,
    theme: &Theme,
    icons: &mut IconCache,
    icon_size: f32,
    wrap: f32,
) {
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        theme.metrics.corner_radius,
        theme.color(ColorRole::SurfaceVariant),
    );
    // The severity band — used with the icon so that colour is not the only distinction (`level`). An
    // info toast has no band and keeps the body colour — colour on every toast buries the warnings and errors.
    let m = theme.components.toast;
    // As in the measuring pass above: one content inset, shared with every row-like thing.
    let inset = theme.metrics.content_inset;
    let accent = theme.color(item.toast.level.role());
    if item.toast.level != Level::Info {
        let bar = Rect::from_min_size(rect.left_top(), egui::vec2(m.accent_w, rect.height()));
        painter.rect_filled(bar, m.accent_w / 2.0, accent);
    }
    // The icon is borrowed — an `IconRef::Glyph` is not cloned each frame. An integrator-
    // registered icon (`Custom` · `Texture`) needs an `IconSet` to resolve, so it is skipped here.
    let name = match &item.toast.icon {
        Some(IconRef::Builtin(name)) => Some(*name),
        Some(_) => None,
        None => match item.toast.level.icon() {
            IconRef::Builtin(name) => Some(name),
            _ => None,
        },
    };
    if let Some(def) = name.and_then(crate::icons::find) {
        let style = IconStyle::sized(icon_size);
        let icon_rect = Rect::from_center_size(
            egui::pos2(rect.min.x + inset + icon_size / 2.0, rect.center().y),
            egui::vec2(icon_size, icon_size),
        );
        crate::icons::paint(painter, icon_rect, def, accent, style.stroke_px(), icons);
    }
    item.wrap = wrap;
    // The galleys are not held across frames — a growing atlas throws the UVs out.
    let galley = painter.layout(
        item.toast.text.clone(),
        egui::TextStyle::Body.resolve(ui.style()),
        Color32::PLACEHOLDER,
        wrap,
    );
    let text_x = rect.min.x + inset + icon_size + m.icon_gap;
    let pos = egui::pos2(text_x, rect.center().y - galley.size().y / 2.0);
    item.text_h = galley.size().y;
    painter.galley(pos, galley, theme.color(ColorRole::OnSurface));
}

#[cfg(test)]
mod tests {
    use super::{ToastPhase, ToastQueue};
    use crate::notify::{Level, Toast};
    use crate::theme::MotionTokens;
    use std::time::{Duration, Instant};

    fn tokens() -> MotionTokens {
        MotionTokens::default()
    }

    fn toast_metrics() -> crate::theme::ToastMetrics {
        crate::theme::ComponentMetrics::default().toast
    }

    /// A6: two visible, the third queued. A new one lifts the previous by 8 px.
    #[test]
    fn two_visible_and_the_older_one_lifts() {
        let t = tokens();
        let now = Instant::now();
        let mut q = ToastQueue::new(2, Duration::from_secs(3));
        q.push(Toast::new("a"));
        q.tick(0.0, now, &t, toast_metrics());
        assert_eq!(q.visible().len(), 1);
        assert!(q.visible().first().is_some_and(|i| i.lift.target() == 0.0));
        q.push(Toast::new("b"));
        q.push(Toast::new("c"));
        q.tick(1.0 / 60.0, now, &t, toast_metrics());
        assert_eq!(q.visible().len(), 2, "two at a time at most");
        assert_eq!(q.pending(), 1, "the third one waits");
        assert!(
            q.visible()
                .first()
                .is_some_and(|i| (i.lift.target() - toast_metrics().lift_step).abs() < 1e-6),
            "the toast that came first sits 8 px higher"
        );
    }

    /// The 160 ms `CubicOut` entry: at frame 5 (83 ms), `t = 1 − (1 − 0.52)³`.
    #[test]
    fn enter_curve_matches_a6() {
        let t = tokens();
        let now = Instant::now();
        let mut q = ToastQueue::new(2, Duration::from_secs(3));
        q.push(Toast::new("a").level(Level::Warning));
        for _ in 0..5 {
            q.tick(1.0 / 60.0, now, &t, toast_metrics());
        }
        let elapsed: f32 = 5.0 / 60.0;
        let s = (elapsed / 0.16).clamp(0.0, 1.0);
        let expected = 1.0 - (1.0 - s).powi(3);
        let value = q.visible().first().map_or(0.0, |i| i.t.value());
        assert!((value - expected).abs() < 1e-3, "{value} vs {expected}");
        assert_eq!(
            q.visible().first().map(|i| i.phase),
            Some(ToastPhase::Entering)
        );
    }

    /// Past the hold it leaves → the queued one comes in as room appears. After the last, it is idle.
    #[test]
    fn expiry_makes_room_for_the_queued_toast() {
        let t = tokens();
        let mut now = Instant::now();
        let mut q = ToastQueue::new(1, Duration::from_millis(100));
        q.push(Toast::new("a"));
        q.push(Toast::new("b"));
        q.tick(0.0, now, &t, toast_metrics());
        assert_eq!(q.pending(), 1);
        let deadline = q.next_deadline();
        assert!(
            deadline.is_none(),
            "no expiry is scheduled while entering (an animation is running)"
        );
        for _ in 0..40 {
            now += Duration::from_millis(16);
            q.tick(0.016, now, &t, toast_metrics());
        }
        assert_eq!(
            q.visible().first().map(|i| i.toast.text.as_str()),
            Some("b"),
            "the first toast withdrew and the waiting one came in"
        );
        for _ in 0..40 {
            now += Duration::from_millis(16);
            q.tick(0.016, now, &t, toast_metrics());
        }
        assert!(q.visible().is_empty() && q.pending() == 0);
        assert!(!q.is_animating() && q.next_deadline().is_none(), "idle");
    }

    /// A tap (`dismiss`) switches it to leaving immediately, and calling it twice does not restart the curve.
    #[test]
    fn dismiss_is_idempotent() {
        let t = tokens();
        let now = Instant::now();
        let mut q = ToastQueue::new(2, Duration::from_secs(3));
        q.push(Toast::new("a"));
        q.tick(0.0, now, &t, toast_metrics());
        q.dismiss(0, &t);
        let after = q.visible().first().map(|i| i.t.value());
        q.dismiss(0, &t);
        assert_eq!(
            q.visible().first().map(|i| i.phase),
            Some(ToastPhase::Leaving)
        );
        assert_eq!(q.visible().first().map(|i| i.t.value()), after);
    }

    /// Under `reduce` the tweens are 0 ms, so entering and leaving finish on that frame.
    #[test]
    fn reduce_settles_immediately() {
        let t = MotionTokens::from_config(&crate::config::MotionConfig {
            reduce: true,
            ..crate::config::MotionConfig::default()
        });
        let now = Instant::now();
        let mut q = ToastQueue::new(2, Duration::from_secs(3));
        q.push(Toast::new("a"));
        q.tick(0.0, now, &t, toast_metrics());
        assert_eq!(
            q.visible().first().map(|i| i.phase),
            Some(ToastPhase::Holding)
        );
        assert_eq!(q.visible().first().map(|i| i.t.value()), Some(1.0));
        assert!(!q.is_animating());
    }
}
