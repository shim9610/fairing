//! The bar painter hooks — the integrator draws the status bar or the nav bar **wholesale**.
//!
//! What the shell keeps: the bar rect (layout and edge zones), the gate decision, tap events on
//! the bar itself, and visibility per the chrome policy (`Hide` / `Overlay`). What the painter
//! is handed is **the drawing, and nothing else**. With a painter installed the built-in render
//! (`StatusBar::ui` / `NavBar::ui`) is never called.
//!
//! ```no_run
//! # use fairing::{BarKind, ShellConfig, Shell};
//! # let ctx = egui::Context::default();
//! let shell = Shell::builder(ShellConfig::default())
//!     .status_bar_painter(|ui: &mut egui::Ui, bar: &mut fairing::BarCx<'_>| {
//!         let color = bar.cx.theme.color(fairing::ColorRole::OnSurface);
//!         ui.painter().rect_filled(bar.rect, 0.0, bar.cx.theme.color(fairing::ColorRole::Surface));
//!         for item in bar.items() {
//!             let _ = (item.id, item.live());
//!         }
//!         let _ = (color, BarKind::Status);
//!     })
//!     .build(&ctx)?;
//! # let _ = shell;
//! # Ok::<(), fairing::Error>(())
//! ```

use super::Slot;
use crate::screen::Cx;
use egui::Rect;

/// Which bar is being drawn. The status bar and the nav bar share one painter type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    /// The status bar.
    Status,
    /// The nav bar.
    Nav,
}

/// One bar item. An element of the list the shell hands over **with the gate already decided** —
/// the painter chooses the layout and the drawing, and takes "who may see this" exactly as the
/// shell (and `[access.gates]`) decided it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarItem<'a> {
    /// The declaration id (`"status.clock"`, `"back"`, an integrator id, …).
    pub id: &'a str,
    /// The status bar slot. Nav bar items have no slot, so it is `None`.
    pub slot: Option<Slot>,
    /// Whether the declaration and the config left it on (`StatusItemSpec::enabled`, `NavBar::item_enabled`).
    pub enabled: bool,
    /// Whether it passed its gate.
    pub allowed: bool,
}

impl BarItem<'_> {
    /// Whether it may be drawn and pressed (`enabled && allowed`).
    #[must_use]
    pub fn live(&self) -> bool {
        self.enabled && self.allowed
    }
}

/// The shell-side storage form of a [`BarItem`]. A reusable buffer whose values are overwritten
/// each frame, so the render path does not grow its heap use.
#[derive(Debug, Default, Clone)]
pub(crate) struct BarItemRow {
    pub(crate) id: String,
    pub(crate) slot: Option<Slot>,
    pub(crate) enabled: bool,
    pub(crate) allowed: bool,
}

/// A writer that overwrites the item buffer from the front. Its `Drop` trims the leftover rows (keeping the capacity).
pub(crate) struct RowSink<'a> {
    rows: &'a mut Vec<BarItemRow>,
    next: usize,
}

impl<'a> RowSink<'a> {
    pub(crate) fn new(rows: &'a mut Vec<BarItemRow>) -> Self {
        Self { rows, next: 0 }
    }

    pub(crate) fn push(&mut self, id: &str, slot: Option<Slot>, enabled: bool, allowed: bool) {
        if let Some(row) = self.rows.get_mut(self.next) {
            // `String::clear` plus `push_str` does not allocate while the capacity holds.
            row.id.clear();
            row.id.push_str(id);
            row.slot = slot;
            row.enabled = enabled;
            row.allowed = allowed;
        } else {
            self.rows.push(BarItemRow {
                id: id.to_owned(),
                slot,
                enabled,
                allowed,
            });
        }
        self.next += 1;
    }
}

impl Drop for RowSink<'_> {
    fn drop(&mut self) {
        self.rows.truncate(self.next);
    }
}

/// The context a bar painter receives. `cx` is **the same** [`Cx`] screens and status items get,
/// so the backend snapshots (clock, Wi-Fi, battery), the theme, the icons and the `shell` handle
/// are all there as they are.
///
/// Taps are taken by the painter's own widgets and asked of the shell through
/// [`Cx::shell`](crate::ShellHandle) (`back()`, `home()`, `launch(..)`) — the shell re-checks
/// the gate at that point.
pub struct BarCx<'a> {
    /// Whether this is the status bar or the nav bar.
    pub kind: BarKind,
    /// The bar rect the shell decided. Drawing outside it is clipped.
    pub rect: Rect,
    /// Whether back has anything to do (the nav bar). Always `false` for a status bar painter.
    pub back_enabled: bool,
    /// The same handle a screen closure receives.
    pub cx: Cx<'a>,
    items: &'a [BarItemRow],
}

impl<'a> BarCx<'a> {
    /// Only the shell constructs one.
    pub(crate) fn new(
        kind: BarKind,
        rect: Rect,
        back_enabled: bool,
        items: &'a [BarItemRow],
        cx: Cx<'a>,
    ) -> Self {
        Self {
            kind,
            rect,
            back_enabled,
            cx,
            items,
        }
    }

    /// This bar's items (in config order). No allocation.
    pub fn items(&self) -> impl Iterator<Item = BarItem<'_>> + '_ {
        self.items.iter().map(|row| BarItem {
            id: &row.id,
            slot: row.slot,
            enabled: row.enabled,
            allowed: row.allowed,
        })
    }

    /// The items in one slot (the status bar). Nav bar items have no slot, so nothing comes out.
    pub fn slot(&self, slot: Slot) -> impl Iterator<Item = BarItem<'_>> + '_ {
        self.items().filter(move |item| item.slot == Some(slot))
    }

    /// Find one by id.
    #[must_use]
    pub fn item(&self, id: &str) -> Option<BarItem<'_>> {
        self.items().find(|item| item.id == id)
    }

    /// The item count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no items at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl std::fmt::Debug for BarCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BarCx")
            .field("kind", &self.kind)
            .field("rect", &self.rect)
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}

/// The callback that draws a whole bar. Given through
/// [`crate::shell::ShellBuilder::status_bar_painter`] and
/// [`crate::shell::ShellBuilder::nav_bar_painter`].
pub type BarPainter = Box<dyn FnMut(&mut egui::Ui, &mut BarCx<'_>)>;

#[cfg(test)]
mod tests {
    use super::{BarItemRow, RowSink, Slot};

    /// The writer overwrites from the front and trims only the leftover rows — the capacity (the `String` buffers) stays.
    #[test]
    fn row_sink_reuses_the_buffer() {
        let mut rows: Vec<BarItemRow> = Vec::new();
        {
            let mut sink = RowSink::new(&mut rows);
            sink.push("a", Some(Slot::Left), true, true);
            sink.push("b", Some(Slot::Right), true, false);
        }
        assert_eq!(rows.len(), 2);
        let capacity = rows.capacity();
        let a_capacity = rows.first().map(|row| row.id.capacity());
        {
            let mut sink = RowSink::new(&mut rows);
            sink.push("c", None, false, true);
        }
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.first().map(|row| row.id.as_str()), Some("c"));
        assert_eq!(rows.first().map(|row| row.slot), Some(None));
        assert_eq!(
            rows.capacity(),
            capacity,
            "it does not take the row buffer again"
        );
        assert_eq!(
            rows.first().map(|row| row.id.capacity()),
            a_capacity,
            "the id string buffer is reused too"
        );
    }
}
