//! **The info popover** a long press brings up over a desktop icon (v1).
//!
//! It says what an icon is before anyone opens it: the icon and its title, the level it needs —
//! where that is more than the lowest — and the description its declaration gives
//! ([`ScreenDecl::description`](crate::screen::ScreenDecl::description)). It stands over the icon
//! and points at it; where there is no room above, it hangs below.
//!
//! **Any press puts it away, and that press does nothing else** — it does not launch the icon
//! under the finger, turn the page or reach a bar. A popover the next tap could see through would
//! launch whatever lay under a finger that only meant to close it.

use super::{DesktopCtx, IconSlot};
use crate::i18n::tr_key;
use crate::icons::{builtin, IconStyle};
use crate::motion::Animated;
use crate::theme::{ColorRole, Elevation, MotionTokens, Theme};
use egui::emath::TSTransform;
use egui::epaint::Shadow;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, Galley, Pos2, Rect, Sense, Shape, Stroke, StrokeKind};
use std::sync::Arc;

/// The scale the card grows from as it comes and shrinks to as it goes, about the caret's tip —
/// so it seems to come out of the icon. A purely visual ratio.
const OPEN_SCALE: f32 = 0.92;

/// What a long press on a desktop icon does — `[desktop] long_press`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LongPressMode {
    /// The info popover.
    #[default]
    Info,
    /// Nothing drawn: the press is the integrator's, through `ShellEvent::IconLongPressed`.
    Quiet,
}

impl LongPressMode {
    /// `"info"` or `"none"`. Anything else warns and is `"info"`.
    pub(crate) fn from_config(name: &str) -> Self {
        match name {
            "info" => Self::Info,
            "none" => Self::Quiet,
            other => {
                log::warn!(
                    "[desktop] long_press = \"{other}\" is neither info nor none - using info"
                );
                Self::Info
            }
        }
    }
}

/// The popover: which icon, where that icon is, and how far in or out it has come.
#[derive(Debug)]
pub(crate) struct IconInfo {
    /// The icon it is about, while it is up or going.
    id: Option<String>,
    /// The icon's cell as last drawn — what the caret points at.
    anchor: Rect,
    /// Whether the icon was drawn this frame. An icon that stops being drawn — its page turned
    /// away, a screen opened over it, a hidden icon's gate closed — takes the popover with it.
    seen: bool,
    /// Presence: 0 → 1 coming, 1 → 0 going.
    t: Animated<f32>,
    /// Going: it takes no presses any more, and it is dropped once `t` is back at 0.
    closing: bool,
    /// The card as last drawn.
    card: Option<Rect>,
}

impl IconInfo {
    pub(crate) fn new() -> Self {
        Self {
            id: None,
            anchor: Rect::NOTHING,
            seen: false,
            t: Animated::new(0.0),
            closing: false,
            card: None,
        }
    }

    /// Bring it up for `id`, whose cell is `anchor`.
    pub(crate) fn open(&mut self, id: &str, anchor: Rect, tokens: &MotionTokens) {
        if self.id.as_deref() != Some(id) {
            self.id = Some(id.to_owned());
            self.t.snap(0.0);
        }
        self.anchor = anchor;
        self.seen = true;
        self.closing = false;
        self.t.to(1.0, tokens.toast_in);
    }

    /// Put it away: it fades out where it is (at once under `reduce`).
    pub(crate) fn close(&mut self, tokens: &MotionTokens) {
        if self.id.is_none() || self.closing {
            return;
        }
        self.closing = true;
        self.t.to(0.0, tokens.toast_out);
        if !self.t.is_animating() {
            self.drop_now();
        }
    }

    /// Gone this frame, with no fade.
    pub(crate) fn drop_now(&mut self) {
        self.id = None;
        self.closing = false;
        self.seen = false;
        self.t.snap(0.0);
        self.card = None;
    }

    /// Whether it is up and taking presses — not gone and not on its way out.
    pub(crate) fn is_open(&self) -> bool {
        self.id.is_some() && !self.closing
    }

    /// The icon it is up for, while it is open.
    pub(crate) fn id(&self) -> Option<&str> {
        self.id.as_deref().filter(|_| !self.closing)
    }

    /// The icon it is about, while it is up or on its way out.
    pub(crate) fn subject(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// The card as last drawn, while it is open.
    pub(crate) fn card(&self) -> Option<Rect> {
        self.card.filter(|_| self.is_open())
    }

    /// Whether it is coming or going.
    pub(crate) fn is_animating(&self) -> bool {
        self.id.is_some() && self.t.is_animating()
    }

    /// Frame stage 5.
    pub(crate) fn tick(&mut self, dt: f32) {
        if self.id.is_none() {
            return;
        }
        self.t.tick(dt);
        if self.closing && !self.t.is_animating() {
            self.drop_now();
        }
    }

    /// The icon `id` was drawn in `cell` this frame.
    pub(crate) fn see(&mut self, id: &str, cell: Rect) {
        if self.id.as_deref() == Some(id) {
            self.anchor = cell;
            self.seen = true;
        }
    }

    /// Draw it (frame stage 9, after everything the desktop draws). `root` is the whole screen —
    /// the shield takes every press there — and `bounds` what the card keeps inside: the space
    /// between the bars. `slot` is the icon it is about, `None` where it is gone.
    pub(crate) fn ui(
        &mut self,
        ctx: &egui::Context,
        root: Rect,
        bounds: Rect,
        slot: Option<&IconSlot>,
        cx: &mut DesktopCtx<'_>,
    ) {
        let seen = std::mem::take(&mut self.seen);
        if self.id.is_none() {
            return;
        }
        let Some(slot) = slot else {
            // Its declaration went: nothing is left to say anything about.
            self.drop_now();
            return;
        };
        if !seen {
            self.close(&cx.theme.motion);
            if self.id.is_none() {
                return;
            }
        }
        if self.is_open() {
            shield(ctx, root);
        }
        let t = self.t.value().clamp(0.0, 1.0);
        let anchor = self.anchor;
        let id = egui::Id::new("fairing.desktop.info");
        let card = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(root.min)
            .constrain(false)
            .fade_in(false)
            .interactable(false)
            .show(ctx, |ui| {
                let card = lay_out(ui.painter(), anchor, bounds, slot, cx);
                let scale = OPEN_SCALE + (1.0 - OPEN_SCALE) * t;
                ctx.set_transform_layer(ui.layer_id(), about(card.tip, scale));
                ui.set_opacity(t);
                paint(ui.painter(), &card, slot, cx);
                card.rect
            })
            .inner;
        self.card = Some(card);
    }
}

/// Every press while the popover is up lands here — over the desktop, the bars, everything the
/// shell drew below it — so nothing under it sees the press that puts it away.
fn shield(ctx: &egui::Context, root: Rect) {
    egui::Area::new(egui::Id::new("fairing.desktop.info.shield"))
        .order(egui::Order::Foreground)
        .fixed_pos(root.min)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let _ = ui.allocate_rect(root, Sense::click_and_drag());
        });
}

/// A scale by `s` about `at`.
fn about(at: Pos2, s: f32) -> TSTransform {
    TSTransform::from_translation(at.to_vec2())
        * TSTransform::from_scaling(s)
        * TSTransform::from_translation(-at.to_vec2())
}

/// The card's pieces, laid out for this frame.
struct Card {
    /// The card.
    rect: Rect,
    /// Where the caret points: the middle of the icon cell's top edge, or its bottom edge when the
    /// card hangs below.
    tip: Pos2,
    /// The card hangs below the icon (the caret is on its top edge).
    below: bool,
    /// The icon beside the title.
    icon: Rect,
    /// Whether the session may open it — the icon and the padlock are drawn as the desktop draws
    /// them.
    allowed: bool,
    /// The title.
    title: (Pos2, Arc<Galley>),
    /// The level row: the padlock and the words beside it.
    level: Option<(Rect, Pos2, Arc<Galley>)>,
    /// The description.
    description: Option<(Pos2, Arc<Galley>)>,
}

/// The level an icon needs, to be shown — where levels mean something, and where it is more than
/// the lowest (everyone has that, so saying it says nothing) or more than the session has.
fn needed_level(slot: &IconSlot, cx: &DesktopCtx<'_>, allowed: bool) -> Option<String> {
    let access = cx.access;
    if access.levels_open() {
        return None;
    }
    let key = access.hint(&slot.gate_name())?;
    let table = access.table();
    let lowest = table.get(table.bottom()).map(|def| def.label.as_str());
    (!allowed || lowest != Some(key.as_str())).then_some(key)
}

/// Lay the card out over `anchor`, inside `bounds`.
fn lay_out(
    painter: &egui::Painter,
    anchor: Rect,
    bounds: Rect,
    slot: &IconSlot,
    cx: &DesktopCtx<'_>,
) -> Card {
    let theme = cx.theme;
    let m = theme.metrics;
    let p = theme.components.popover;
    let pad = m.content_inset;
    let gap = theme.control.line_gap;
    let radius = m.corner_radius;
    let area = bounds.shrink(m.screen_inset);
    // The widest it may be, and never so narrow that the caret has no straight edge to sit on.
    let least = 2.0 * (p.caret + radius);
    let widest = p.max_width.min(area.width()).max(least);
    let inner = (widest - 2.0 * pad).max(1.0);
    let icon_size = m.icon_size * 0.5;
    let allowed = match &slot.gate {
        Some(gate) => cx.access.allows(gate),
        None => cx.access.allows_name(&slot.id),
    };

    let title = painter.layout(
        cx.strings.get(&slot.label).to_owned(),
        theme.strong(m.type_scale.button),
        Color32::PLACEHOLDER,
        (inner - icon_size - p.icon_gap).max(1.0),
    );
    let lock = m.desktop_lock_size;
    let level = needed_level(slot, cx, allowed).map(|key| {
        level_galley(
            painter,
            theme,
            cx.strings.get(tr_key!("Required level")),
            cx.strings.get(&key),
            allowed,
            (inner - lock - p.icon_gap).max(1.0),
        )
    });
    let description = slot.description.as_ref().map(|key| {
        painter.layout(
            cx.strings.get(key).to_owned(),
            egui::FontId::proportional(m.type_scale.body),
            Color32::PLACEHOLDER,
            inner,
        )
    });

    let title_h = title.size().y.max(icon_size);
    let mut content_w = icon_size + p.icon_gap + title.size().x;
    let mut content_h = title_h;
    if let Some(galley) = &level {
        content_w = content_w.max(lock + p.icon_gap + galley.size().x);
        content_h += gap + galley.size().y.max(lock);
    }
    if let Some(galley) = &description {
        content_w = content_w.max(galley.size().x);
        content_h += gap + galley.size().y;
    }
    let size = egui::vec2(
        (content_w.min(inner) + 2.0 * pad).clamp(least, widest),
        content_h + 2.0 * pad,
    );
    let (rect, tip, below) = place(size, anchor, area, p.caret, radius);

    let x = rect.left() + pad;
    let mut y = rect.top() + pad;
    let icon = Rect::from_min_size(
        egui::pos2(x, y + (title_h - icon_size) / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    let title_pos = egui::pos2(
        x + icon_size + p.icon_gap,
        y + (title_h - title.size().y) / 2.0,
    );
    y += title_h;
    let level = level.map(|galley| {
        y += gap;
        let row_h = galley.size().y.max(lock);
        let lock_rect = Rect::from_min_size(
            egui::pos2(x, y + (row_h - lock) / 2.0),
            egui::vec2(lock, lock),
        );
        let at = egui::pos2(x + lock + p.icon_gap, y + (row_h - galley.size().y) / 2.0);
        y += row_h;
        (lock_rect, at, galley)
    });
    let description = description.map(|galley| {
        y += gap;
        (egui::pos2(x, y), galley)
    });
    Card {
        rect,
        tip,
        below,
        icon,
        allowed,
        title: (title_pos, title),
        level,
        description,
    }
}

/// Where a card of `size` goes over `anchor`, inside `area`: above the icon where it fits, below
/// where there is more room there; centred on it, clamped in. The caret's tip stays on the straight
/// part of the edge, clear of the corners. Returns the card, the tip, and whether it hangs below.
fn place(
    size: egui::Vec2,
    anchor: Rect,
    area: Rect,
    caret: f32,
    radius: f32,
) -> (Rect, Pos2, bool) {
    let above_room = anchor.top() - caret - area.top();
    let below_room = area.bottom() - anchor.bottom() - caret;
    let below = size.y > above_room && below_room > above_room;
    let top = if below {
        anchor.bottom() + caret
    } else {
        anchor.top() - caret - size.y
    };
    let top = top.clamp(area.top(), (area.bottom() - size.y).max(area.top()));
    let left = (anchor.center().x - size.x / 2.0)
        .clamp(area.left(), (area.right() - size.x).max(area.left()));
    let rect = Rect::from_min_size(egui::pos2(left, top), size);
    let caret_x = anchor.center().x.clamp(
        rect.left() + radius + caret,
        (rect.right() - radius - caret).max(rect.left() + radius + caret),
    );
    let tip = if below {
        egui::pos2(caret_x, rect.top() - caret)
    } else {
        egui::pos2(caret_x, rect.bottom() + caret)
    };
    (rect, tip, below)
}

/// "Required level · Maintenance": the words in the caption's grey, the level in the strong face
/// — in the warning colour where the session falls short of it.
fn level_galley(
    painter: &egui::Painter,
    theme: &Theme,
    words: &str,
    level: &str,
    allowed: bool,
    wrap: f32,
) -> Arc<Galley> {
    let size = theme.metrics.type_scale.small;
    let muted = theme.color(ColorRole::Muted);
    let mut job = LayoutJob::default();
    job.append(
        words,
        0.0,
        TextFormat::simple(egui::FontId::proportional(size), muted),
    );
    job.append(
        " · ",
        0.0,
        TextFormat::simple(egui::FontId::proportional(size), muted),
    );
    job.append(
        level,
        0.0,
        TextFormat::simple(
            theme.strong(size),
            theme.color(if allowed {
                ColorRole::OnSurface
            } else {
                ColorRole::Warning
            }),
        ),
    );
    job.wrap.max_width = wrap;
    painter.layout_job(job)
}

/// The card, its caret and what is on it.
fn paint(painter: &egui::Painter, card: &Card, slot: &IconSlot, cx: &mut DesktopCtx<'_>) {
    let theme = cx.theme;
    let m = theme.metrics;
    let caret = theme.components.popover.caret;
    let radius = m.corner_radius;
    let fill = theme.color(ColorRole::SurfaceVariant);
    let ink = theme.elevate(Elevation::Floating);
    let corners = egui::CornerRadius::from(radius);
    if ink.shadow != Shadow::NONE {
        painter.add(ink.shadow.as_shape(card.rect, corners));
    }
    painter.rect_filled(card.rect, corners, fill);
    // The caret's base sits inside the card's edge — past the rim and its feathering — so the
    // rim breaks where the caret joins and runs down its sides instead.
    let edge = if card.below {
        card.rect.top()
    } else {
        card.rect.bottom()
    };
    let into = if card.below { 1.0 } else { -1.0 } * (ink.rim.width + 1.0);
    let base = [
        egui::pos2(card.tip.x - caret, edge + into),
        egui::pos2(card.tip.x + caret, edge + into),
    ];
    if ink.rim != Stroke::NONE {
        painter.rect_stroke(card.rect, corners, ink.rim, StrokeKind::Inside);
    }
    painter.add(Shape::convex_polygon(
        vec![base[0], card.tip, base[1]],
        fill,
        Stroke::NONE,
    ));
    if ink.rim != Stroke::NONE {
        let sides = [
            egui::pos2(card.tip.x - caret, edge),
            card.tip,
            egui::pos2(card.tip.x + caret, edge),
        ];
        painter.add(Shape::line(sides.to_vec(), ink.rim));
    }

    let style = IconStyle {
        stroke: Some(m.desktop_icon_stroke),
        ..IconStyle::sized(card.icon.width()).enabled(card.allowed)
    };
    let _ = cx
        .icons
        .paint(painter, card.icon, &slot.icon, &style, theme);
    let (at, galley) = &card.title;
    painter.galley(*at, Arc::clone(galley), theme.color(ColorRole::OnSurface));
    if let Some((lock, at, galley)) = &card.level {
        let (icon, role) = if card.allowed {
            (&builtin::UNLOCK, ColorRole::Muted)
        } else {
            (&builtin::LOCK, ColorRole::Warning)
        };
        let style = IconStyle::sized(lock.width()).color(crate::icons::IconColor::Role(role));
        let _ = cx.icons.paint(painter, *lock, icon, &style, theme);
        painter.galley(*at, Arc::clone(galley), theme.color(ColorRole::Muted));
    }
    if let Some((at, galley)) = &card.description {
        painter.galley(*at, Arc::clone(galley), theme.color(ColorRole::Muted));
    }
}

#[cfg(test)]
mod tests {
    use super::{IconInfo, LongPressMode};
    use crate::theme::MotionTokens;
    use egui::Rect;

    #[test]
    fn the_config_words_are_info_and_none() {
        assert_eq!(LongPressMode::from_config("info"), LongPressMode::Info);
        assert_eq!(LongPressMode::from_config("none"), LongPressMode::Quiet);
        assert_eq!(
            LongPressMode::from_config("menu"),
            LongPressMode::Info,
            "an unknown word is the default"
        );
    }

    /// It comes in over `toast_in` and goes over `toast_out`; once out it is gone, and under
    /// `reduce` both are at once.
    #[test]
    fn it_comes_in_and_goes_out() {
        let tokens = MotionTokens::default();
        let cell = Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(96.0, 96.0));
        let mut info = IconInfo::new();
        info.open("app", cell, &tokens);
        assert!(info.is_open() && info.is_animating());
        for _ in 0..60 {
            info.tick(1.0 / 60.0);
        }
        assert!(info.is_open() && !info.is_animating());
        assert_eq!(info.id(), Some("app"));
        info.close(&tokens);
        assert!(!info.is_open(), "on its way out it takes no presses");
        assert_eq!(info.id(), None);
        assert!(info.is_animating());
        for _ in 0..60 {
            info.tick(1.0 / 60.0);
        }
        assert!(!info.is_animating());

        let reduced = MotionTokens {
            toast_in: crate::motion::Tween::instant(),
            toast_out: crate::motion::Tween::instant(),
            ..tokens
        };
        info.open("app", cell, &reduced);
        assert!(info.is_open() && !info.is_animating());
        info.close(&reduced);
        assert!(!info.is_open() && !info.is_animating());
    }
}
