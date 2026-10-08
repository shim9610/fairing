//! [`WidgetCx`] — what a control needs, and nothing else.

use crate::icons::IconSet;
use crate::motion::{AnimationStore, Tween};
use crate::theme::Theme;
use crate::widgets::WidgetPainters;

/// The context a widget is drawn with.
///
/// # Why it is a struct of fields and not a trait
///
/// A trait with `theme()` and `icons()` methods will not compile against this crate's own code:
/// [`ListRow`](crate::widgets::ListRow) does
/// `cx.icons.paint(painter, rect, icon, &style, cx.theme)`, borrowing `icons` mutably and `theme`
/// shared at the same moment. That is legal only because they are **disjoint fields**; behind
/// methods each call borrows the whole context and the second one is rejected. So the shape is
/// forced: plain public fields, and `animate` reaches its state through a field of its own.
///
/// # Why it is not the shell's `Cx`
///
/// Measured before the element layer was split out, every widget in this crate reached
/// outside itself for exactly three things — the theme, the icons, and one animation call. It
/// never needed the screen registry, the services, the access gate or the pane. Taking only these
/// three is what keeps a control from growing a dependency on the shell by accident; the shell's
/// own `Cx` lends one of these with `Cx::widgets()`.
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
    /// The widget painters: a widget whose kind has one is drawn by it, the rest draw
    /// themselves. `None` draws every widget the built-in way.
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
            anim_scope: egui::Id::new("test"),
            frame: 0,
            inset_bottom: 0.0,
            painters: Some(&mut painters),
        };
        assert!(cx.reborrow().painters.is_some());
    }
}
