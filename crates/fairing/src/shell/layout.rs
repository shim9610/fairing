//! Screen geometry. Computed once per frame.

use crate::screen::{BarMode, ChromePolicy};
use egui::Rect;

/// The frame's layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The status bar rect. `None` when hidden. With `Overlay` it is `Some` but the content does not shrink.
    pub status: Option<Rect>,
    /// The nav bar rect.
    pub nav: Option<Rect>,
    /// The content (the workspace). `Screen::ui` receives only this rect. **With a rail it is
    /// narrowed by that much** — a screen not covering the rail is the point of a rail.
    pub content: Rect,
    /// The icon rail's rect. `None` when it is off.
    pub rail: Option<Rect>,
    /// OSK (M2).
    pub osk: Option<Rect>,
    /// The edge zones `[top, bottom, left, right]` (M2 gestures). They remain even when hidden.
    pub edge_zones: [Rect; 4],
    /// Whether the status bar overlaps the content.
    pub status_overlay: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            status: None,
            nav: None,
            content: Rect::NOTHING,
            rail: None,
            osk: None,
            edge_zones: [Rect::NOTHING; 4],
            status_overlay: false,
        }
    }
}

/// The layout's input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LayoutInput {
    /// The whole screen.
    pub(crate) screen: Rect,
    /// The focused screen's chrome policy.
    pub(crate) policy: ChromePolicy,
    /// The status bar's height, if it is on.
    pub(crate) status_height: Option<f32>,
    /// The nav bar's height, if it is on.
    pub(crate) nav_height: Option<f32>,
    /// The edge zone width.
    pub(crate) edge_px: f32,
    /// The OSK's height (M2).
    pub(crate) osk_height: Option<f32>,
    /// The icon rail — which side, and how wide. `None` when it is off.
    pub(crate) rail: Option<RailInput>,
}

/// The rail's layout input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RailInput {
    /// `true` for the left.
    pub(crate) left: bool,
    /// A fraction of the content width (0..1).
    pub(crate) fraction: f32,
    /// The floor — one icon cell has to fit.
    pub(crate) min_width: f32,
}

/// The edge zones `[top, bottom, left, right]`. `edge_px` at the screen's edge
/// always remains, whether or not a bar is hidden, and a **visible** status or nav bar makes the
/// whole bar a zone (A1: the top is `max(edge_px, top_bar)` and the bottom is
/// symmetric, `max(edge_px, bottom_bar)`; a hidden bar passes 0). Split out because
/// the gesture engine (stage 5) needs it before the layout.
#[must_use]
pub(crate) fn edge_zones(screen: Rect, edge_px: f32, top_bar: f32, bottom_bar: f32) -> [Rect; 4] {
    let e = edge_px;
    let top = e.max(top_bar);
    let bottom = e.max(bottom_bar);
    [
        Rect::from_min_size(screen.min, egui::vec2(screen.width(), top)),
        Rect::from_min_size(
            egui::pos2(screen.min.x, screen.max.y - bottom),
            egui::vec2(screen.width(), bottom),
        ),
        Rect::from_min_size(screen.min, egui::vec2(e, screen.height())),
        Rect::from_min_size(
            egui::pos2(screen.max.x - e, screen.min.y),
            egui::vec2(e, screen.height()),
        ),
    ]
}

/// Frame stage 6: computing the layout.
#[must_use]
pub(crate) fn compute_layout(input: &LayoutInput) -> Layout {
    let screen = input.screen;
    let mut top = screen.min.y;
    let mut bottom = screen.max.y;
    let mut status = None;
    let mut status_overlay = false;
    if let Some(h) = input.status_height {
        match input.policy.status_bar {
            BarMode::Show => {
                status = Some(Rect::from_min_size(
                    screen.min,
                    egui::vec2(screen.width(), h),
                ));
                top += h;
            }
            BarMode::Overlay => {
                status = Some(Rect::from_min_size(
                    screen.min,
                    egui::vec2(screen.width(), h),
                ));
                status_overlay = true;
            }
            BarMode::Hide => {}
        }
    }
    let mut nav = None;
    if let Some(h) = input.nav_height {
        if input.policy.nav_bar != BarMode::Hide {
            nav = Some(Rect::from_min_size(
                egui::pos2(screen.min.x, screen.max.y - h),
                egui::vec2(screen.width(), h),
            ));
            bottom -= h;
        }
    }
    let osk = input.osk_height.map(|h| {
        Rect::from_min_size(
            egui::pos2(screen.min.x, bottom - h),
            egui::vec2(screen.width(), h),
        )
    });
    let full = Rect::from_min_max(
        egui::pos2(screen.min.x, top),
        egui::pos2(screen.max.x, bottom.max(top)),
    );
    // The rail is taken **out of the content** — below the status bar and the nav bar. Crossing the bars,
    // the bars would be drawn over the rail and the icons cut off.
    let (rail, content) = match input.rail {
        Some(r) if full.width() > r.min_width * 2.0 => {
            let w = (full.width() * r.fraction).max(r.min_width);
            // The content must not become narrower than the rail — the screen is the main body.
            let w = w.min(full.width() * 0.5);
            if r.left {
                (
                    Some(Rect::from_min_max(
                        full.min,
                        egui::pos2(full.min.x + w, full.max.y),
                    )),
                    Rect::from_min_max(egui::pos2(full.min.x + w, full.min.y), full.max),
                )
            } else {
                (
                    Some(Rect::from_min_max(
                        egui::pos2(full.max.x - w, full.min.y),
                        full.max,
                    )),
                    Rect::from_min_max(full.min, egui::pos2(full.max.x - w, full.max.y)),
                )
            }
        }
        // Where there is not the width for a rail it is ignored even when turned on — on a 480 px panel a
        // rail plus a screen leaves both unusable.
        _ => (None, full),
    };
    let edge_zones = edge_zones(
        screen,
        input.edge_px,
        status.map_or(0.0, |r| r.height()),
        nav.map_or(0.0, |r| r.height()),
    );
    Layout {
        status,
        nav,
        content,
        rail,
        osk,
        edge_zones,
        status_overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::{compute_layout, LayoutInput};
    use crate::screen::ChromePolicy;
    use egui::{pos2, Rect};

    fn input(policy: ChromePolicy) -> LayoutInput {
        LayoutInput {
            screen: Rect::from_min_max(pos2(0.0, 0.0), pos2(1024.0, 600.0)),
            policy,
            status_height: Some(32.0),
            nav_height: Some(56.0),
            edge_px: 24.0,
            osk_height: None,
            rail: None,
        }
    }

    #[test]
    fn bars_shrink_content() {
        let l = compute_layout(&input(ChromePolicy::default()));
        assert!((l.content.min.y - 32.0).abs() < f32::EPSILON);
        assert!((l.content.max.y - 544.0).abs() < f32::EPSILON);
        assert!(l.status.is_some() && l.nav.is_some());
    }

    /// A1: a visible status bar makes all of itself (32 > 24) the top edge zone, and the nav bar (56) the bottom one.
    #[test]
    fn visible_bars_widen_edge_zones() {
        let l = compute_layout(&input(ChromePolicy::default()));
        assert!((l.edge_zones[0].height() - 32.0).abs() < f32::EPSILON);
        assert!((l.edge_zones[1].height() - 56.0).abs() < f32::EPSILON);
        assert!((l.edge_zones[1].max.y - 600.0).abs() < f32::EPSILON);
        assert!((l.edge_zones[2].width() - 24.0).abs() < f32::EPSILON);
    }

    #[test]
    fn fullscreen_keeps_edge_zones() {
        let l = compute_layout(&input(ChromePolicy::fullscreen()));
        assert!(l.status.is_none() && l.nav.is_none());
        assert!((l.content.height() - 600.0).abs() < f32::EPSILON);
        assert!((l.edge_zones[0].height() - 24.0).abs() < f32::EPSILON);
    }
}
