//! **Placing and drawing the keyboard's keys yourself** — rungs 4 and 5 of the override ladder.
//!
//! What the keys *are* — their labels, what they type, the faces — is the layout's
//! ([`OskLayout::Custom`](super::OskLayout::Custom) for a set of your own). Where they go and how
//! they look are these two hooks. Either way the shell keeps the keyboard: when it shows and
//! slides, the hit areas (each key's rect and half the gap round it), the press, typing into the
//! focused field, the composition, ⇧ and its lock, and the face changes.
//!
//! ```no_run
//! # fn main() -> fairing::Result<()> {
//! # let (ctx, config): (egui::Context, fairing::ShellConfig) = todo!();
//! use fairing::osk::{OskKeyCx, OskKeyLayoutCx};
//!
//! let shell = fairing::Shell::builder(config)
//!     // One field to a screen, so no focus keys: ⏮ and ⏭ go, and the space bar takes their room.
//!     .osk_key_layout(|keys: &OskKeyLayoutCx<'_>, rects: &mut [egui::Rect]| {
//!         let (Some(prev), Some(space), Some(next)) =
//!             (keys.index_of("⏮"), keys.index_of(" "), keys.index_of("⏭"))
//!         else {
//!             return;
//!         };
//!         let (Some(&left), Some(&right)) = (rects.get(prev), rects.get(next)) else {
//!             return;
//!         };
//!         if let Some(rect) = rects.get_mut(space) {
//!             *rect = egui::Rect::from_min_max(left.min, right.max);
//!         }
//!         for gone in [prev, next] {
//!             if let Some(rect) = rects.get_mut(gone) {
//!                 *rect = egui::Rect::NOTHING;
//!             }
//!         }
//!     })
//!     // Flat keys with a hairline round them.
//!     .osk_key_painter(|painter: &egui::Painter, key: &mut OskKeyCx<'_>| {
//!         let theme = key.theme;
//!         let ink = theme.color(fairing::ColorRole::OnSurface);
//!         painter.rect_stroke(
//!             key.rect,
//!             2.0,
//!             egui::Stroke::new(1.0, ink),
//!             egui::StrokeKind::Inside,
//!         );
//!         painter.text(
//!             key.rect.center(),
//!             egui::Align2::CENTER_CENTER,
//!             key.label,
//!             egui::FontId::proportional(theme.metrics.type_scale.button),
//!             ink,
//!         );
//!     })
//!     .build(&ctx)?;
//! # let _ = shell;
//! # Ok(())
//! # }
//! ```

use super::layouts::{KeyAction, KeyDef, KeyFace};
use crate::icons::IconSet;
use crate::theme::Theme;
use egui::Rect;

/// What a key layout places the face's keys with (rung 4).
///
/// The rects come placed the built-in way — row by row, each key as wide as its span, the rows
/// centred — **one per key, in reading order** (the first row left to right, then the next). A
/// rect the layout moves takes the key's hit area with it; a rect it empties (`Rect::NOTHING`)
/// leaves the key out, neither drawn nor pressed. The panel slides up from below, and `panel` is
/// where it is this frame: placing keys relative to it keeps them riding with it.
pub struct OskKeyLayoutCx<'a> {
    /// The keyboard at its full height, where it is this frame.
    pub panel: Rect,
    /// The face on show (`0` is the lowercase face of the built-in qwerty).
    pub face_index: usize,
    /// The theme — `metrics.osk_key_gap` is the gap the built-in layout leaves.
    pub theme: &'a Theme,
    face: &'a KeyFace,
}

impl<'a> OskKeyLayoutCx<'a> {
    pub(crate) fn new(panel: Rect, face_index: usize, face: &'a KeyFace, theme: &'a Theme) -> Self {
        Self {
            panel,
            face_index,
            theme,
            face,
        }
    }

    /// How many keys the face has — one rect each.
    #[must_use]
    pub fn len(&self) -> usize {
        self.face.rows.iter().map(|row| row.keys.len()).sum()
    }

    /// Whether the face has no keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Key `index` in reading order: its row, its place in the row, and its definition.
    #[must_use]
    pub fn key(&self, index: usize) -> Option<(usize, usize, &KeyDef)> {
        self.face
            .rows
            .iter()
            .enumerate()
            .flat_map(|(r, row)| row.keys.iter().enumerate().map(move |(k, key)| (r, k, key)))
            .nth(index)
    }

    /// The index of the first key labelled `label` (`"a"`, `" "`, `"⌫"`, …).
    #[must_use]
    pub fn index_of(&self, label: &str) -> Option<usize> {
        self.face
            .rows
            .iter()
            .flat_map(|row| row.keys.iter())
            .position(|key| key.label == label)
    }
}

impl std::fmt::Debug for OskKeyLayoutCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OskKeyLayoutCx")
            .field("panel", &self.panel)
            .field("face_index", &self.face_index)
            .field("keys", &self.len())
            .finish_non_exhaustive()
    }
}

/// The callback that places the keys. Given through
/// [`crate::shell::ShellBuilder::osk_key_layout`].
pub type OskKeyLayout = Box<dyn FnMut(&OskKeyLayoutCx<'_>, &mut [Rect])>;

/// What a key painter draws one key with (rung 5).
///
/// The key is the painter's whole — its face and its label. The panel behind the keys is the
/// shell's (`ColorRole::Surface`), and so are the press and what the key does.
pub struct OskKeyCx<'a> {
    /// The key as drawn this frame — shrunk while it is pressed, by the shell's press scale.
    pub rect: Rect,
    /// Its label: `"a"`, `"⇧"`, `"⌫"`, `"123"`, … The built-in keyboard draws ⇧ ⌫ ↵ ✓ ▾ and the
    /// language key as icons; a painter decides for itself.
    pub label: &'a str,
    /// What it does.
    pub action: &'a KeyAction,
    /// Whether a finger is on it.
    pub pressed: bool,
    /// Whether this is ⇧ held locked (caps lock) — the built-in keyboard draws it in the accent.
    pub locked: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

impl std::fmt::Debug for OskKeyCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OskKeyCx")
            .field("rect", &self.rect)
            .field("label", &self.label)
            .field("pressed", &self.pressed)
            .field("locked", &self.locked)
            .finish_non_exhaustive()
    }
}

/// The callback that draws one key. Given through
/// [`crate::shell::ShellBuilder::osk_key_painter`].
pub type OskKeyPainter = Box<dyn FnMut(&egui::Painter, &mut OskKeyCx<'_>)>;
