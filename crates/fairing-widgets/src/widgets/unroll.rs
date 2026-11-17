//! `unroll` animates an expandable body over `motion.switch`.
//!
//! The body is laid out in a child `Ui` and clipped to the currently exposed band.
//! Its measured height drives the expansion, and following content is placed below
//! that band. The caller supplies the body and expanded state. Layout and clipping
//! use egui's `Ui` mechanisms.

use crate::cx::WidgetCx as Cx;
use egui::{Pos2, Rect, Sense, Vec2};

/// A body about to be laid out under a row this frame: its eased height is known, its natural
/// height from last frame is known, and the caller lays the body out into it with
/// [`Unroll::show`].
#[derive(Debug, Clone, Copy)]
pub struct Unroll {
    id: egui::Id,
    /// How far open, 0 to 1, eased.
    k: f32,
    /// What the body took last frame.
    natural: f32,
    /// The body's left inset from the row's left edge.
    indent: f32,
}

/// What a body took this frame.
#[derive(Debug, Clone, Copy)]
pub struct Unrolled {
    /// The band shown this frame — the body's full width and its eased height.
    pub rect: Rect,
    /// The body's whole height as laid out this frame, whatever is shown of it.
    pub natural: f32,
    /// How far open, 0 to 1.
    pub k: f32,
    /// Fully open and the height has stopped changing: nothing more will move.
    pub settled: bool,
}

/// Start a body under the row just drawn. `None` while the body is closed and its band has
/// closed all the way, so the caller lays nothing out; otherwise the returned value is to be
/// given the body with [`Unroll::show`] — on the same frame, right where the cursor is.
///
/// `open` is the row's state; the height eases towards it over `motion.switch`. `indent` is
/// how far in from the row's left edge the body starts — one icon column, on a settings row.
pub fn unroll(
    ui: &mut egui::Ui,
    cx: &mut Cx<'_>,
    id: egui::Id,
    open: bool,
    indent: f32,
) -> Option<Unroll> {
    let anim = id.with("unroll");
    let was = ui.data(|d| d.get_temp::<bool>(id.with("was_open")).unwrap_or(false));
    if open && !was {
        // Seeded closed on the frame it opens, so the body unrolls rather than appears.
        let _ = cx.animate(anim, 0.0, cx.theme.motion.switch);
    }
    ui.data_mut(|d| d.insert_temp(id.with("was_open"), open));
    let k = cx
        .animate(anim, if open { 1.0 } else { 0.0 }, cx.theme.motion.switch)
        .clamp(0.0, 1.0);
    if !open && k <= 0.001 {
        return None;
    }
    let natural = ui
        .data(|d| d.get_temp::<f32>(id.with("natural")))
        .unwrap_or(0.0);
    Some(Unroll {
        id,
        k,
        natural,
        indent,
    })
}

impl Unroll {
    /// Lay the body out under the cursor, clipped to the band shown this frame, and advance the
    /// caller's cursor by the band. The body `Ui` runs top-down from the indent and is as tall as
    /// it needs; a scroll area does not belong inside it.
    pub fn show(self, ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) -> Unrolled {
        let top_left = ui.cursor().min;
        let width = ui.available_width();
        let shown = (self.natural * self.k).max(0.0);
        let band = Rect::from_min_size(top_left, Vec2::new(width, shown));
        let clip = ui.clip_rect().intersect(band);
        let body_rect = Rect::from_min_max(
            Pos2::new(top_left.x + self.indent, top_left.y),
            Pos2::new(top_left.x + width, f32::INFINITY),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(clip);
        add(&mut child);
        let used = (child.min_rect().max.y - top_left.y).max(0.0);
        ui.data_mut(|d| d.insert_temp(self.id.with("natural"), used));
        let moved = (used - self.natural).abs() > 0.5;
        if moved {
            ui.ctx().request_repaint();
        }
        // The band is what the caller's layout sees; the body beyond it is under the clip.
        let _ = ui.allocate_rect(band, Sense::hover());
        Unrolled {
            rect: band,
            natural: used,
            k: self.k,
            settled: self.k >= 0.999 && !moved,
        }
    }
}
