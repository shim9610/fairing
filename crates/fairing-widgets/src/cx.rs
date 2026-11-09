//! [`WidgetCx`] supplies theme, icon, animation and painting context to controls.

use crate::icons::IconSet;
use crate::motion::{AnimationStore, Tween};
use crate::theme::Theme;
use crate::widgets::WidgetPainters;

/// Context supplied to a widget.
///
/// Public fields allow simultaneous borrows of separate resources, such as mutable
/// icons and a shared theme. The context contains widget resources rather than the
/// shell registry, device services or application access state. The shell supplies
/// one through `Cx::widgets()`.
pub struct WidgetCx<'a> {
    /// The resolved theme — palette, metrics, component tokens, motion.
    pub theme: &'a Theme,
    /// The icon set, for drawing an [`IconRef`](crate::icons::IconRef).
    pub icons: &'a mut IconSet,
    /// The animation state. Reached through [`WidgetCx::animate`] rather than directly.
    pub anims: &'a mut AnimationStore,
    /// The id namespace an animation is scoped to, so two instances of one screen animate apart.
    pub anim_scope: egui::Id,
    /// The frame number, for the store's own eviction.
    pub frame: u64,
    /// How much of the bottom of the pane something is lying over — the on-screen keyboard,
    /// while it is up. A widget that opens a panel keeps the panel clear of it (`Dropdown`).
    /// Zero where nothing is.
    pub inset_bottom: f32,
    /// Optional painter bundle for built-in widgets. A supported widget uses its
    /// registered kind-specific callback; otherwise it uses its built-in drawing.
    pub painters: Option<&'a mut WidgetPainters>,
}

impl WidgetCx<'_> {
    /// An animated value that eases from where it was towards `target`.
    ///
    /// The same call the shell's `Cx::animate` makes, scoped the same way.
    pub fn animate(&mut self, id: egui::Id, target: f32, tween: Tween) -> f32 {
        self.anims
            .animate(id.with(self.anim_scope), target, tween, self.frame)
    }

    /// Borrow it again for a nested call, without giving up this one.
    pub fn reborrow(&mut self) -> WidgetCx<'_> {
        WidgetCx {
            theme: self.theme,
            icons: &mut *self.icons,
            anims: &mut *self.anims,
            anim_scope: self.anim_scope,
            frame: self.frame,
            inset_bottom: self.inset_bottom,
            painters: self.painters.as_deref_mut(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WidgetCx;
    use crate::icons::IconSet;
    use crate::motion::AnimationStore;
    use crate::theme::Theme;
    use crate::widgets::WidgetPainters;

    /// A context borrowed again for a nested call keeps the painters: a widget drawn
    /// through `reborrow` is drawn the way its kind is painted, not the built-in way.
    #[test]
    fn a_reborrowed_context_keeps_the_painters() {
        let theme = Theme::light();
        let (mut icons, mut anims) = (IconSet::new(), AnimationStore::new());
        let mut painters = WidgetPainters::new();
        let mut cx = WidgetCx {
            theme: &theme,
            icons: &mut icons,
            anims: &mut anims,
            anim_scope: egui::Id::unique("test"),
            frame: 0,
            inset_bottom: 0.0,
            painters: Some(&mut painters),
        };
        assert!(cx.reborrow().painters.is_some());
    }
}
