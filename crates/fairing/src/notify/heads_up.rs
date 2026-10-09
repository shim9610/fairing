//! The heads-up banner (A6): shows a new notification briefly at the top before
//! it goes into the shade.
//!
//! `y: −H → 0` over 220 ms `CubicOut`; a 4 s hold (paused while held); leaving `0 → −H` over
//! 200 ms `CubicIn`. Dragging up is 1:1, and on release `dy < −H/3` or a fling past
//! `motion.fling_px_s` springs it
//! away, otherwise it returns. A tap runs the action and then leaves. Opening the shade absorbs
//! it immediately (it disappears with no exit).
//!
//! **Zero heap allocations per frame**: the banner holds the title and body
//! galleys and re-lays them out only when the content or the width changes (the string buffers
//! are reused too), and the icon is flattened once per size by the banner's [`IconCache`].

use super::hooks::{BannerView, HeadsUpCx, HeadsUpLayout, HeadsUpLayoutCx, HeadsUpPainter};
use super::model::{Level, Notification, NotificationId};
use crate::access::{Access, Gate};
use crate::i18n::Strings;
use crate::icons::{builtin, IconCache, IconRef, IconSet, IconStyle};
use crate::motion::{DragSpring, ReleaseRule, RubberBand};
use crate::theme::{ColorRole, MotionTokens, Theme};
use egui::{Color32, Rect};
use std::time::{Duration, Instant};

/// The release distance threshold (A6: `dy < −H/3`).
const SNAP_RATIO: f32 = 1.0 / 3.0;
/// The banner's widest, as a multiple of `metrics.toast_width` (it was a fixed 560 du).
const HEADS_UP_OVER_TOAST: f32 = 4.0 / 3.0;
/// The rubber band when pulled down (up is 1:1; down resists).
const DOWN_RUBBER: RubberBand = RubberBand {
    factor: 0.25,
    max: 16.0,
};
// The three dimensions (`pad` · `icon_gap` · `progress_h`) are `theme.components.heads_up`.

/// The title a banner shows for a notification whose gate the session fails — the same
/// wording (and string key) as the shade's redacted row.
const HIDDEN_NOTIFICATION: &str = "1 notification";

/// The heads-up phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum HeadsUpPhase {
    /// Entering.
    Entering,
    /// Holding (the timer).
    Holding,
    /// Being dragged.
    Dragging,
    /// Leaving (by tween or spring).
    Leaving,
}

/// The result of a heads-up.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HeadsUpAction {
    /// Tapped — the shell runs the notification's action.
    Tapped(NotificationId),
    /// Swiped up and dismissed.
    Swiped(NotificationId),
}

/// One heads-up banner (only ever one at a time).
#[doc(hidden)]
pub struct HeadsUp {
    current: Option<Banner>,
    height: f32,
    /// The icon polyline cache (no allocation on the render path).
    icons: IconCache,
    /// The integrator's banner, in place of the built-in one (rung 5).
    painter: Option<HeadsUpPainter>,
    /// The integrator's placement of the banner (rung 4).
    layout: Option<HeadsUpLayout>,
}

impl std::fmt::Debug for HeadsUp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadsUp")
            .field("current", &self.current)
            .field("height", &self.height)
            .field("painter", &self.painter.is_some())
            .field("layout", &self.layout.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct Banner {
    id: NotificationId,
    title: String,
    body: String,
    icon: IconRef,
    level: Level,
    progress: Option<f32>,
    /// The notification's gate. Checked on every frame it is drawn, so a session that changes
    /// while the banner is up (a lock, a timeout) hides the content from then on.
    gate: Option<Gate>,
    y: DragSpring,
    phase: HeadsUpPhase,
    until: Instant,
    rect: Rect,
    /// Whether a finger is resting on the banner — the hold timer stops while it is (A6).
    pressed: bool,
    /// The title and body galley cache, and the wrap width they were laid out at.
    wrap: f32,
    /// The height the content measured to on the last frame, 0 until it has been drawn. The
    /// banner is drawn at the larger of this and `heads_up_height`: the token is a floor, not
    /// a box, and a two-line body used to run out of the bottom of it.
    needed: f32,
}

impl Banner {
    /// Overwrite the content (reusing the string buffers; the galleys are invalidated).
    fn fill(&mut self, notification: &Notification) {
        self.id = notification.id;
        self.title.clear();
        self.title.push_str(&notification.title);
        self.body.clear();
        self.body.push_str(&notification.body);
        self.icon = notification.shown_icon();
        self.level = notification.level;
        self.progress = notification.progress;
        self.gate.clone_from(&notification.gate);
        self.rect = Rect::NOTHING;
        self.pressed = false;
        self.needed = 0.0;
    }
}

impl HeadsUp {
    /// A banner of height `height` (`theme.metrics.heads_up_height`).
    #[must_use]
    pub fn new(height: f32) -> Self {
        Self {
            current: None,
            height: height.max(1.0),
            icons: IconCache::new(),
            painter: None,
            layout: None,
        }
    }

    /// Draw the banner with `painter` instead of the built-in one.
    pub(crate) fn set_painter(&mut self, painter: HeadsUpPainter) {
        self.painter = Some(painter);
    }

    /// Place the banner with `layout`.
    pub(crate) fn set_layout(&mut self, layout: HeadsUpLayout) {
        self.layout = Some(layout);
    }

    /// Raise a new notification (replacing one already up — the later one wins).
    pub fn show(&mut self, notification: &Notification, now: Instant, tokens: &MotionTokens) {
        let h = self.height;
        let until = now + tokens.heads_up_in.duration + tokens.heads_up_hold;
        // A banner already up is reused — the string and galley buffers stay, so there are fewer allocations.
        if let Some(banner) = &mut self.current {
            banner.fill(notification);
            banner.phase = HeadsUpPhase::Entering;
            banner.until = until;
            banner.y.set_range(-h, 0.0);
            // A banner already up comes down from the top again — the signal that a new notification arrived.
            banner.y.snap(-h);
            banner.y.to(0.0, tokens.heads_up_in);
            return;
        }
        let mut y = DragSpring::new(-h, -h, 0.0, DOWN_RUBBER);
        y.to(0.0, tokens.heads_up_in);
        self.current = Some(Banner {
            id: notification.id,
            title: notification.title.clone(),
            body: notification.body.clone(),
            icon: notification.shown_icon(),
            level: notification.level,
            progress: notification.progress,
            gate: notification.gate.clone(),
            y,
            phase: HeadsUpPhase::Entering,
            until,
            rect: Rect::NOTHING,
            pressed: false,
            wrap: 0.0,
            needed: 0.0,
        });
    }

    /// The notification on screen was updated in place: the banner takes the new content and
    /// keeps where it is, its phase and its timer. Another id is left alone.
    pub(crate) fn update(&mut self, notification: &Notification) {
        let Some(banner) = self.current.as_mut().filter(|b| b.id == notification.id) else {
            return;
        };
        let (rect, pressed, needed) = (banner.rect, banner.pressed, banner.needed);
        banner.fill(notification);
        banner.rect = rect;
        banner.pressed = pressed;
        banner.needed = needed;
    }

    /// The height the banner is drawn and travels at: the token, or more where the content
    /// measured to more.
    fn h(&self) -> f32 {
        let base = self.height;
        self.current.as_ref().map_or(base, |b| b.needed.max(base))
    }

    /// The shade opened: it disappears with no exit animation.
    pub(crate) fn absorb(&mut self) {
        self.current = None;
    }

    /// Start leaving (after a tap, or on the timer).
    pub fn dismiss(&mut self, tokens: &MotionTokens) {
        // The drawn height, not the token: a banner the content made taller leaves all the way.
        let h = self.h();
        if let Some(b) = &mut self.current {
            b.phase = HeadsUpPhase::Leaving;
            b.pressed = false;
            b.y.to(-h, tokens.heads_up_out);
        }
    }

    /// Begin a drag (a press on the banner).
    pub(crate) fn begin_drag(&mut self) {
        if let Some(b) = &mut self.current {
            b.phase = HeadsUpPhase::Dragging;
            b.y.begin();
        }
    }

    /// Mid-drag (`dy` is the vertical movement from the press point, negative upward). Up is 1:1, down is rubber-banded.
    pub fn drag(&mut self, dy: f32, vy: f32) {
        if let Some(b) = &mut self.current {
            b.phase = HeadsUpPhase::Dragging;
            b.y.drag(dy, vy);
        }
    }

    /// Release: `dy < −H/3` or a fling past `motion.fling_px_s` leaves, otherwise it returns.
    /// `true` if it was dismissed.
    pub fn release(&mut self, vy: f32, now: Instant, tokens: &MotionTokens) -> bool {
        let h = self.h();
        let hold = tokens.heads_up_hold;
        let spring = tokens.spring;
        let reduce = tokens.reduce;
        let Some(b) = &mut self.current else {
            return false;
        };
        b.pressed = false;
        // The fraction of the distance travelled upwards. Pulled down (the rubber band) it is 0.
        let progress = (-b.y.value() / h).clamp(0.0, 1.0);
        // The fling threshold is the motion token's, as every other release rule's is.
        let rule = ReleaseRule {
            snap_ratio: SNAP_RATIO,
            fling: tokens.fling_px_s,
        };
        if rule.confirm(progress, -vy) {
            b.phase = HeadsUpPhase::Leaving;
            if reduce {
                b.y.snap(-h);
            } else {
                b.y.release(-h, spring);
            }
            true
        } else {
            b.phase = HeadsUpPhase::Holding;
            b.until = now + hold;
            if reduce {
                b.y.snap(0.0);
            } else {
                b.y.release(0.0, spring);
            }
            false
        }
    }

    /// Frame stage 5. `true` while it is moving.
    pub fn tick(&mut self, dt: f32, now: Instant, tokens: &MotionTokens) -> bool {
        let h = self.h();
        let out = tokens.heads_up_out;
        let Some(b) = &mut self.current else {
            return false;
        };
        // Whether it was still on its way before this step: a leaving banner is removed only
        // after the frame that drew it at the end of its travel, so it slides all the way off
        // rather than vanishing from part way.
        let was_moving = b.y.is_animating();
        let moving = b.y.tick(dt);
        if b.phase == HeadsUpPhase::Entering && !moving {
            b.phase = HeadsUpPhase::Holding;
        }
        // `Dragging` catches none of the branches below — while it is being dragged the hold timer does not
        // run and the value is settled by the finger (A6).
        match b.phase {
            // The timer stops while a finger rests on it too. "The time left is held" is expressed by
            // pushing the expiry back by dt — no separate state is needed. Once the exit has begun, a
            // finger resting on it does not stop it going all the way out.
            HeadsUpPhase::Holding if b.pressed => {
                b.until += Duration::from_secs_f32(dt.clamp(0.0, crate::motion::MAX_DT));
            }
            HeadsUpPhase::Holding if now >= b.until => {
                b.phase = HeadsUpPhase::Leaving;
                b.y.to(-h, out);
                return true;
            }
            HeadsUpPhase::Leaving if !was_moving => {
                self.current = None;
                return false;
            }
            // Arrived: one more frame draws it there, and the next removes it.
            HeadsUpPhase::Leaving => return true,
            _ => {}
        }
        moving
    }

    /// The id of the notification on screen.
    #[must_use]
    pub fn visible(&self) -> Option<NotificationId> {
        self.current.as_ref().map(|b| b.id)
    }

    /// The phase.
    #[must_use]
    pub fn phase(&self) -> Option<HeadsUpPhase> {
        self.current.as_ref().map(|b| b.phase)
    }

    /// The banner's top y offset (−H..=0).
    #[must_use]
    pub fn y(&self) -> f32 {
        self.current.as_ref().map_or(-self.height, |b| b.y.value())
    }

    /// The banner's height.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.height
    }

    /// Last frame's rect.
    #[must_use]
    pub fn rect(&self) -> Option<Rect> {
        self.current
            .as_ref()
            .map(|b| b.rect)
            .filter(Rect::is_positive)
    }

    /// Whether a finger is on the banner (with the hold timer stopped).
    #[must_use]
    pub fn is_pressed(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|b| b.pressed || b.phase == HeadsUpPhase::Dragging)
    }

    /// Whether it is moving. A banner on its way out counts until it is gone.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|b| b.y.is_animating() || b.phase == HeadsUpPhase::Leaving)
    }

    /// The expiry to arm an idle repaint for. None while it is held (the timer is stopped).
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.current
            .as_ref()
            .filter(|b| b.phase == HeadsUpPhase::Holding && !b.pressed)
            .map(|b| b.until)
    }

    /// Where the banner rests, `h` tall: centred at the top of `screen`, a screen inset in from
    /// its sides and at most a third wider than a toast, or where the integrator's layout moves
    /// it. A layout sets the place and the width, not the height; a rect it leaves no width
    /// (`Rect::NOTHING`) keeps the banner out.
    fn at_rest(&mut self, screen: Rect, theme: &Theme, h: f32) -> Rect {
        let m = &theme.metrics;
        let width = (screen.width() - 2.0 * m.screen_inset)
            .clamp(m.touch_target, m.toast_width * HEADS_UP_OVER_TOAST);
        let mut place = Rect::from_min_size(
            egui::pos2(
                screen.center().x - width / 2.0,
                screen.min.y + m.screen_inset * 0.5,
            ),
            egui::vec2(width, h),
        );
        let Some(layout) = self.layout.as_mut() else {
            return place;
        };
        layout(
            &HeadsUpLayoutCx {
                screen,
                height: h,
                theme,
            },
            &mut place,
        );
        if place.width() > 0.0 {
            Rect::from_min_size(place.min, egui::vec2(place.width(), h))
        } else {
            Rect::NOTHING
        }
    }

    /// Stage 13: draw at the top (`Area(Order::Tooltip)`), or where the integrator's layout puts
    /// it, with the built-in banner or the integrator's painter. Taps and drags are
    /// taken here and returned. A notification whose gate `access` fails is drawn redacted, as
    /// the shade draws it: one line of "1 notification", the bell, no body and no progress.
    #[allow(clippy::too_many_arguments)] // What drawing the banner needs, the hooks' two included.
    pub(crate) fn ui(
        &mut self,
        ctx: &egui::Context,
        screen: Rect,
        theme: &Theme,
        tokens: &MotionTokens,
        now: Instant,
        icons: &mut IconSet,
        strings: &Strings,
        access: &Access,
    ) -> Option<HeadsUpAction> {
        let h = self.h();
        let id = self.current.as_ref()?.id;
        let y = self.y();
        let place = self.at_rest(screen, theme, h);
        let Self {
            current,
            icons: cache,
            painter,
            ..
        } = self;
        let banner = current.as_mut()?;
        // A rect the layout emptied keeps the banner off the screen, still timed.
        if !place.is_positive() {
            banner.rect = Rect::NOTHING;
            return None;
        }
        let rect = place.translate(egui::vec2(0.0, y));
        banner.rect = rect;
        let allowed = banner.gate.as_ref().is_none_or(|gate| access.allows(gate));
        let response = egui::Area::new(egui::Id::new("fairing.heads_up"))
            .order(egui::Order::Tooltip)
            .fixed_pos(rect.min)
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                if let Some(painter) = painter.as_mut() {
                    paint_with(painter, ui, rect, banner, allowed, theme, icons, strings);
                } else {
                    let hidden = (!allowed).then(|| strings.get(HIDDEN_NOTIFICATION));
                    paint_banner(ui, rect, banner, hidden, theme, cache);
                }
                ui.allocate_rect(rect, egui::Sense::click_and_drag())
            })
            .inner;
        // Measured taller than it was drawn: the spring's range widens to the new height, and
        // a banner still coming in is moved up by the difference so its bottom edge stays
        // where it was — the extra height arrives above the screen, not as a pop below it.
        let grown = banner.needed.max(self.height);
        if grown > h + 0.5 {
            banner.y.set_range(-grown, 0.0);
            if banner.phase == HeadsUpPhase::Entering {
                let value = banner.y.value() - (grown - h);
                banner.y.snap(value);
                banner.y.to(0.0, tokens.heads_up_in);
            }
        }
        banner.pressed = response.is_pointer_button_down_on();
        // Its drag — up and away — is its own, and it says so.
        crate::drag::claim_if_held(&response);
        if response.drag_started() {
            self.begin_drag();
        }
        if response.dragged() {
            let (dy, vy) = ctx.input(|i| {
                (
                    i.pointer
                        .press_origin()
                        .and_then(|o| i.pointer.interact_pos().map(|p| p.y - o.y))
                        .unwrap_or(0.0),
                    i.pointer.velocity().y,
                )
            });
            self.drag(dy, vy);
        }
        if response.drag_stopped() {
            let vy = ctx.input(|i| i.pointer.velocity().y);
            if self.release(vy, now, tokens) {
                return Some(HeadsUpAction::Swiped(id));
            }
        }
        if response.clicked() {
            self.dismiss(tokens);
            return Some(HeadsUpAction::Tapped(id));
        }
        None
    }
}

/// Hand one banner to the integrator's painter — redacted where the session fails its gate.
#[allow(clippy::too_many_arguments)] // The painter's context, built from what `HeadsUp::ui` holds.
fn paint_with(
    painter: &mut HeadsUpPainter,
    ui: &mut egui::Ui,
    rect: Rect,
    banner: &mut Banner,
    allowed: bool,
    theme: &Theme,
    icons: &mut IconSet,
    strings: &Strings,
) {
    let Banner {
        id,
        title,
        body,
        icon,
        level,
        progress,
        pressed,
        needed,
        ..
    } = banner;
    let view = if allowed {
        BannerView {
            id: *id,
            title,
            body,
            icon,
            level: *level,
            progress: *progress,
            pressed: *pressed,
        }
    } else {
        BannerView {
            id: *id,
            title: strings.get(HIDDEN_NOTIFICATION),
            body: "",
            icon: &builtin::BELL,
            level: Level::Info,
            progress: None,
            pressed: *pressed,
        }
    };
    painter(
        ui,
        &mut HeadsUpCx::new(rect, view, theme, icons, strings, needed),
    );
}

/// One banner: the background, icon, title, body and progress. Galley- and polyline-cached, so no allocation.
/// With `hidden` (a gated notification the session may not read) that line is the title, and
/// the body, the progress and the notification's own icon and accent are left out.
fn paint_banner(
    ui: &egui::Ui,
    rect: Rect,
    banner: &mut Banner,
    hidden: Option<&str>,
    theme: &Theme,
    icons: &mut IconCache,
) {
    let painter = ui.painter();
    let metrics = theme.metrics;
    painter.rect_filled(
        rect,
        metrics.corner_radius,
        theme.color(ColorRole::SurfaceVariant),
    );
    painter.rect_stroke(
        rect,
        metrics.corner_radius,
        egui::Stroke::new(
            theme.control.stroke_hairline,
            theme.color(ColorRole::Outline),
        ),
        egui::StrokeKind::Inside,
    );
    // The in-chrome icon size rule (half of `icon_size` = 24 px) — the same as a panel tile's.
    let m = theme.components.heads_up;
    let icon_size = metrics.icon_size * 0.5;
    let accent = theme.color(if hidden.is_some() {
        ColorRole::Muted
    } else {
        banner.level.role()
    });
    let inset = metrics.content_inset;
    let text_x = rect.min.x + inset + icon_size + m.icon_gap;
    let wrap = (rect.max.x - inset - text_x).max(metrics.touch_target * 0.5);
    banner.wrap = wrap;
    // The galleys are not held across frames — a growing atlas throws the UVs out.
    let title = painter.layout(
        hidden.map_or_else(|| banner.title.clone(), str::to_owned),
        egui::TextStyle::Button.resolve(ui.style()),
        Color32::PLACEHOLDER,
        wrap,
    );
    let progress = if hidden.is_some() {
        None
    } else {
        banner.progress
    };
    let body = (hidden.is_none() && !banner.body.is_empty()).then(|| {
        painter.layout(
            banner.body.clone(),
            egui::TextStyle::Body.resolve(ui.style()),
            Color32::PLACEHOLDER,
            wrap,
        )
    });
    // **The icon centres on the text block, and the block is measured first.**
    //
    // It used to be pinned near the top of the banner at `rect.min.y + pad + 8.0` — a bare literal
    // that happened to land near the middle of a one-line title and left the icon sitting visibly
    // high on a two-line one, which is the defect as reported. Measuring the galleys before drawing
    // the icon is the whole fix; the gap between the two lines is `control.line_gap` rather than
    // another literal `4.0`.
    let line_gap = theme.control.line_gap;
    let title_h = title.size().y;
    let block_h = title_h + body.as_ref().map_or(0.0, |g| line_gap + g.size().y);
    // What this content needs top to bottom; the banner grows to it on the next frame.
    let progress_h = progress.map_or(0.0, |_| line_gap + m.progress_h + m.pad * 0.5);
    banner.needed = inset + block_h + progress_h + inset;
    let mut y = rect.min.y + inset;
    let icon = if hidden.is_some() {
        &builtin::BELL
    } else {
        &banner.icon
    };
    if let IconRef::Builtin(name) = icon {
        if let Some(def) = crate::icons::find(name) {
            let style = IconStyle::sized(icon_size);
            let icon_rect = Rect::from_center_size(
                egui::pos2(rect.min.x + inset + icon_size / 2.0, y + block_h / 2.0),
                egui::vec2(icon_size, icon_size),
            );
            crate::icons::paint(painter, icon_rect, def, accent, style.stroke_px(), icons);
        }
    }
    painter.galley(
        egui::pos2(text_x, y),
        title,
        theme.color(ColorRole::OnSurface),
    );
    y += title_h + line_gap;
    if let Some(galley) = body {
        painter.galley(egui::pos2(text_x, y), galley, theme.color(ColorRole::Muted));
    }
    if let Some(progress) = progress {
        let track = Rect::from_min_size(
            egui::pos2(text_x, rect.max.y - m.pad * 0.5 - m.progress_h),
            egui::vec2(rect.max.x - m.pad - text_x, m.progress_h),
        );
        // Through the widget's own painter rather than two `rect_filled` calls of its own. The
        // pair it replaces drew the track in `Outline`, which measures 1.15-1.65 against these
        // grounds: an empty banner bar was **invisible in all four shipped palettes**.
        crate::widgets::paint_bar(painter, theme, track, progress, ColorRole::Primary);
    }
}

#[cfg(test)]
mod tests {
    use super::{HeadsUp, HeadsUpPhase};
    use crate::notify::{Notification, NotificationId};
    use crate::theme::MotionTokens;
    use std::time::{Duration, Instant};

    fn note() -> Notification {
        Notification::new(NotificationId::of("n"), "Title").body("Body")
    }

    /// A banner that has finished entering and stands at `y = 0` (the starting point for the drag rules).
    fn settled(height: f32, tokens: &MotionTokens) -> (HeadsUp, Instant) {
        let mut now = Instant::now();
        let mut hu = HeadsUp::new(height);
        hu.show(&note(), now, tokens);
        for _ in 0..16 {
            now += Duration::from_millis(16);
            hu.tick(0.016, now, tokens);
        }
        (hu, now)
    }

    /// The A6 entry curve: `−H → 0` over 220 ms `CubicOut`, then `Holding`.
    #[test]
    fn enter_then_hold_then_leave() {
        let t = MotionTokens::default();
        let mut now = Instant::now();
        let mut hu = HeadsUp::new(88.0);
        hu.show(&note(), now, &t);
        assert_eq!(hu.phase(), Some(HeadsUpPhase::Entering));
        assert!((hu.y() + 88.0).abs() < 1e-6, "it starts at −H");
        // The entrance finishes after 220 ms.
        for _ in 0..14 {
            now += Duration::from_millis(16);
            hu.tick(0.016, now, &t);
        }
        assert_eq!(hu.phase(), Some(HeadsUpPhase::Holding));
        assert!(hu.y().abs() < 1e-3, "y = {}", hu.y());
        assert!(hu.next_deadline().is_some());
        // After holding for 4 s the exit tween runs, and it is gone 200 ms later.
        for _ in 0..((4000 + 400) / 16) {
            now += Duration::from_millis(16);
            hu.tick(0.016, now, &t);
        }
        assert_eq!(
            hu.visible(),
            None,
            "once the hold and the exit are done there is nothing"
        );
    }

    /// The hold timer stops while it is held (A6).
    #[test]
    fn holding_timer_pauses_while_pressed() {
        let t = MotionTokens::default();
        let mut now = Instant::now();
        let mut hu = HeadsUp::new(88.0);
        hu.show(&note(), now, &t);
        for _ in 0..14 {
            now += Duration::from_millis(16);
            hu.tick(0.016, now, &t);
        }
        let deadline = hu.next_deadline();
        if let Some(b) = hu.current.as_mut() {
            b.pressed = true;
        }
        for _ in 0..300 {
            now += Duration::from_millis(16);
            hu.tick(0.016, now, &t);
        }
        assert_eq!(
            hu.phase(),
            Some(HeadsUpPhase::Holding),
            "it does not go away while held"
        );
        assert!(
            hu.next_deadline().is_none(),
            "no expiry is scheduled while held"
        );
        if let Some(b) = hu.current.as_mut() {
            b.pressed = false;
        }
        let moved = hu.next_deadline().zip(deadline).is_some_and(|(a, b)| a > b);
        assert!(
            moved,
            "the expiry time moved back by as long as it was held"
        );
    }

    /// Dragged more than `H/3` upward and released, it leaves; short of that, it returns (A6).
    #[test]
    fn release_rules_match_a6() {
        let t = MotionTokens::default();
        let (mut hu, now) = settled(90.0, &t);
        hu.begin_drag();
        hu.drag(-40.0, -100.0);
        assert!((hu.y() + 40.0).abs() < 1e-3, "1:1 tracking: {}", hu.y());
        assert!(hu.release(-100.0, now, &t), "it passed H/3 = 30");
        assert_eq!(hu.phase(), Some(HeadsUpPhase::Leaving));

        let (mut hu, now) = settled(90.0, &t);
        hu.begin_drag();
        hu.drag(-10.0, 0.0);
        assert!(
            !hu.release(0.0, now, &t),
            "short on both distance and velocity → it comes back"
        );
        assert_eq!(hu.phase(), Some(HeadsUpPhase::Holding));

        // The velocity rule is the motion token's fling threshold, as every other release is.
        let fast = -(t.fling_px_s + 100.0);
        let (mut hu, now) = settled(90.0, &t);
        hu.begin_drag();
        hu.drag(-5.0, fast);
        assert!(
            hu.release(fast, now, &t),
            "velocity {fast} past the fling threshold → it leaves"
        );
        let slow = -(t.fling_px_s - 100.0);
        let (mut hu, now) = settled(90.0, &t);
        hu.begin_drag();
        hu.drag(-5.0, slow);
        assert!(
            !hu.release(slow, now, &t),
            "velocity {slow} short of the fling threshold → it comes back"
        );
    }

    /// Pulled downward the rubber band holds it — the banner does not go off the bottom.
    #[test]
    fn downward_drag_is_rubber_banded() {
        let t = MotionTokens::default();
        let (mut hu, now) = settled(88.0, &t);
        hu.begin_drag();
        hu.drag(200.0, 400.0);
        assert!(
            hu.y() <= 16.0,
            "the rubber-band ceiling is 16 px: {}",
            hu.y()
        );
        assert!(
            !hu.release(0.0, now, &t),
            "it cannot be swept away downwards"
        );
    }

    /// Opening the shade absorbs it with no exit.
    #[test]
    fn absorb_removes_the_banner_at_once() {
        let t = MotionTokens::default();
        let now = Instant::now();
        let mut hu = HeadsUp::new(88.0);
        hu.show(&note(), now, &t);
        hu.absorb();
        assert_eq!(hu.visible(), None);
        assert!(!hu.is_animating() && hu.next_deadline().is_none());
    }

    /// A new notification replacing a banner on screen reuses the buffers and restarts the curve.
    #[test]
    fn second_notification_replaces_the_banner() {
        let t = MotionTokens::default();
        let mut now = Instant::now();
        let mut hu = HeadsUp::new(88.0);
        hu.show(&note(), now, &t);
        for _ in 0..14 {
            now += Duration::from_millis(16);
            hu.tick(0.016, now, &t);
        }
        hu.show(
            &Notification::new(NotificationId::of("m"), "second"),
            now,
            &t,
        );
        assert_eq!(hu.visible(), Some(NotificationId::of("m")));
        assert_eq!(hu.phase(), Some(HeadsUpPhase::Entering));
        assert!((hu.y() + 88.0).abs() < 1e-6, "from −H again");
    }
}
