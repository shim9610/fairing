//! Edges. Zone hit-testing and direction vectors.

use egui::{Pos2, Rect, Vec2};

/// A screen edge. The order matches [`crate::shell::Layout::edge_zones`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    /// Top (the shade).
    Top,
    /// Bottom (home and recents — `NavStyle::Gesture`, M6).
    Bottom,
    /// Left (gesture back).
    Left,
    /// Right.
    Right,
}

impl Edge {
    /// All four, in zone-array order.
    pub const ALL: [Self; 4] = [Self::Top, Self::Bottom, Self::Left, Self::Right];

    /// The `edge_zones` index.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Top => 0,
            Self::Bottom => 1,
            Self::Left => 2,
            Self::Right => 3,
        }
    }

    /// The unit vector pointing **into** the screen (the axis progress is measured on).
    #[must_use]
    pub fn inward(self) -> Vec2 {
        match self {
            Self::Top => Vec2::new(0.0, 1.0),
            Self::Bottom => Vec2::new(0.0, -1.0),
            Self::Left => Vec2::new(1.0, 0.0),
            Self::Right => Vec2::new(-1.0, 0.0),
        }
    }

    /// The unit vector **along** the edge — the axis a slide is measured on: rightward for the top
    /// and bottom edges, downward for the left and right ones.
    #[must_use]
    pub fn along(self) -> Vec2 {
        match self {
            Self::Top | Self::Bottom => Vec2::new(1.0, 0.0),
            Self::Left | Self::Right => Vec2::new(0.0, 1.0),
        }
    }

    /// From a config string (`"left"`, …).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

/// A bitmask of edges (block lists and allow lists).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeMask(u8);

impl EdgeMask {
    /// None.
    pub const NONE: Self = Self(0);
    /// All of them.
    pub const ALL: Self = Self(0b1111);

    /// This mask plus `edge`.
    #[must_use]
    pub fn with(self, edge: Edge) -> Self {
        Self(self.0 | (1 << edge.index()))
    }

    /// Whether it contains one.
    #[must_use]
    pub fn contains(self, edge: Edge) -> bool {
        self.0 & (1 << edge.index()) != 0
    }

    /// Whether it is empty.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// The edge zone `pos` falls in. Where they overlap (a corner), `Top` and `Bottom` come before
/// left and right — the shade wins over back.
#[must_use]
pub fn edge_at(zones: &[Rect; 4], pos: Pos2) -> Option<Edge> {
    Edge::ALL
        .into_iter()
        .find(|edge| zones.get(edge.index()).is_some_and(|z| z.contains(pos)))
}

/// Whether it is inside the top-corner square (`corner_px` on a side) — where the emergency gesture lives.
#[must_use]
pub fn in_top_corner(screen: Rect, pos: Pos2, corner_px: f32) -> bool {
    let c = corner_px.max(1.0);
    let left = Rect::from_min_size(screen.min, Vec2::splat(c));
    let right = Rect::from_min_size(egui::pos2(screen.max.x - c, screen.min.y), Vec2::splat(c));
    left.contains(pos) || right.contains(pos)
}

#[cfg(test)]
mod tests {
    use super::{edge_at, in_top_corner, Edge, EdgeMask};
    use egui::{pos2, Rect};

    fn zones() -> [Rect; 4] {
        crate::shell::compute_layout(&crate::shell::LayoutInput {
            screen: Rect::from_min_max(pos2(0.0, 0.0), pos2(1024.0, 600.0)),
            policy: crate::screen::ChromePolicy::default(),
            status_height: Some(32.0),
            nav_height: Some(56.0),
            edge_px: 24.0,
            osk_height: None,
            rail: None,
        })
        .edge_zones
    }

    #[test]
    fn corner_prefers_top_over_left() {
        let z = zones();
        assert_eq!(edge_at(&z, pos2(10.0, 8.0)), Some(Edge::Top));
        assert_eq!(edge_at(&z, pos2(10.0, 300.0)), Some(Edge::Left));
        assert_eq!(edge_at(&z, pos2(500.0, 300.0)), None);
        assert!(in_top_corner(
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1024.0, 600.0)),
            pos2(1010.0, 10.0),
            64.0
        ));
    }

    #[test]
    fn mask_bits() {
        let m = EdgeMask::NONE.with(Edge::Left);
        assert!(m.contains(Edge::Left) && !m.contains(Edge::Top) && !m.is_empty());
        assert!(EdgeMask::ALL.contains(Edge::Right));
    }
}
