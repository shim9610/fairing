//! The declaration registry — the store behind `shell.add(decl)` and `shell.remove(id)`.
//!
//! Adding the same id again replaces it. Removing takes the related icons and items with it and
//! closes open screens (ending an instance is done by `Shell` on the next frame, when it sees
//! the registry changed).

use super::{ActionDecl, DesktopPlacement, Screen, ScreenDecl, ScreenSource};
use crate::chrome::NavItemDecl;
use crate::chrome::StatusItemDecl;
#[cfg(feature = "overlay")]
use crate::overlay::TileDecl;

/// The unit of `shell.add` (Decl).
#[derive(Debug)]
pub enum Decl {
    /// A screen.
    Screen(ScreenDecl),
    /// An action icon.
    Action(ActionDecl),
    /// A status bar item.
    StatusItem(StatusItemDecl),
    /// A nav bar item.
    NavItem(NavItemDecl),
    /// A quick-settings tile (feature `overlay`).
    #[cfg(feature = "overlay")]
    Tile(TileDecl),
}

#[cfg(feature = "overlay")]
impl From<TileDecl> for Decl {
    fn from(value: TileDecl) -> Self {
        Self::Tile(value)
    }
}

impl From<ScreenDecl> for Decl {
    fn from(value: ScreenDecl) -> Self {
        Self::Screen(value)
    }
}

impl From<ActionDecl> for Decl {
    fn from(value: ActionDecl) -> Self {
        Self::Action(value)
    }
}

impl From<StatusItemDecl> for Decl {
    fn from(value: StatusItemDecl) -> Self {
        Self::StatusItem(value)
    }
}

impl From<NavItemDecl> for Decl {
    fn from(value: NavItemDecl) -> Self {
        Self::NavItem(value)
    }
}

impl Decl {
    /// id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Screen(d) => &d.id,
            Self::Action(d) => &d.id,
            Self::StatusItem(d) => &d.id,
            Self::NavItem(d) => &d.id,
            #[cfg(feature = "overlay")]
            Self::Tile(d) => d.id(),
        }
    }

    /// The kind.
    #[must_use]
    pub fn kind(&self) -> DeclKind {
        match self {
            Self::Screen(_) => DeclKind::Screen,
            Self::Action(_) => DeclKind::Action,
            Self::StatusItem(_) => DeclKind::StatusItem,
            Self::NavItem(_) => DeclKind::NavItem,
            #[cfg(feature = "overlay")]
            Self::Tile(_) => DeclKind::Tile,
        }
    }
}

/// The kind of a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    /// A screen.
    Screen,
    /// An action.
    Action,
    /// A status bar item.
    StatusItem,
    /// A nav bar item.
    NavItem,
    /// A quick-settings tile.
    Tile,
}

/// The declaration store. Preserves insertion order (which is `.desktop()`'s auto-placement order).
#[derive(Debug, Default)]
#[doc(hidden)]
pub struct Registry {
    screens: Vec<ScreenDecl>,
    actions: Vec<ActionDecl>,
    status_items: Vec<StatusItemDecl>,
    nav_items: Vec<NavItemDecl>,
    #[cfg(feature = "overlay")]
    tiles: Vec<TileDecl>,
    dirty: bool,
}

impl Registry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add. If the id exists (whatever its kind) it is taken out and replaced. Returns the
    /// declaration that was replaced.
    ///
    /// This is **the one place declarations are validated**. The builder does not
    /// force an order (`.icon()` may follow `.desktop()`), so only `add`, which sees the finished
    /// declaration, can judge:
    /// - `.desktop()` or `.dock()` with no icon → a warning (it registers, but no icon appears).
    /// - The same id replaced with a different kind (`Screen` → `Action`, say) → a warning (the
    ///   sign of an unintended id collision).
    ///
    /// A new declaration always goes on the end — so even after a replacement the insertion
    /// order (which is `.desktop()`'s auto-placement order) is "by most recent `add`".
    pub fn add(&mut self, decl: Decl) -> Option<Decl> {
        warn_unusable_icon_placement(&decl);
        if let Some(previous) = self.kind(decl.id()) {
            let kind = decl.kind();
            if previous != kind {
                log::warn!(
                    "add: replacing `{}` ({previous:?} -> {kind:?}) - check that the ids were not meant to differ",
                    decl.id()
                );
            }
        }
        let replaced = self.remove(decl.id());
        match decl {
            Decl::Screen(d) => self.screens.push(d),
            Decl::Action(d) => self.actions.push(d),
            Decl::StatusItem(d) => self.status_items.push(d),
            Decl::NavItem(d) => self.nav_items.push(d),
            #[cfg(feature = "overlay")]
            Decl::Tile(d) => self.tiles.push(d),
        }
        self.dirty = true;
        replaced
    }

    /// Remove by id (whatever the kind).
    pub fn remove(&mut self, id: &str) -> Option<Decl> {
        if let Some(i) = self.screens.iter().position(|d| d.id == id) {
            self.dirty = true;
            return Some(Decl::Screen(self.screens.remove(i)));
        }
        if let Some(i) = self.actions.iter().position(|d| d.id == id) {
            self.dirty = true;
            return Some(Decl::Action(self.actions.remove(i)));
        }
        if let Some(i) = self.status_items.iter().position(|d| d.id == id) {
            self.dirty = true;
            return Some(Decl::StatusItem(self.status_items.remove(i)));
        }
        if let Some(i) = self.nav_items.iter().position(|d| d.id == id) {
            self.dirty = true;
            return Some(Decl::NavItem(self.nav_items.remove(i)));
        }
        #[cfg(feature = "overlay")]
        if let Some(i) = self.tiles.iter().position(|d| d.id() == id) {
            self.dirty = true;
            return Some(Decl::Tile(self.tiles.remove(i)));
        }
        None
    }

    /// Look up the kind.
    #[must_use]
    pub fn kind(&self, id: &str) -> Option<DeclKind> {
        if self.screens.iter().any(|d| d.id == id) {
            Some(DeclKind::Screen)
        } else if self.actions.iter().any(|d| d.id == id) {
            Some(DeclKind::Action)
        } else if self.status_items.iter().any(|d| d.id == id) {
            Some(DeclKind::StatusItem)
        } else if self.nav_items.iter().any(|d| d.id == id) {
            Some(DeclKind::NavItem)
        } else if self.tile_index(id).is_some() {
            Some(DeclKind::Tile)
        } else {
            None
        }
    }

    /// A screen declaration.
    #[must_use]
    pub fn screen(&self, id: &str) -> Option<&ScreenDecl> {
        self.screens.iter().find(|d| d.id == id)
    }

    /// A screen declaration (mutable — for calling the factory).
    pub(crate) fn screen_mut(&mut self, id: &str) -> Option<&mut ScreenDecl> {
        self.screens.iter_mut().find(|d| d.id == id)
    }

    /// An action declaration (mutable — for running it).
    pub(crate) fn action_mut(&mut self, id: &str) -> Option<&mut ActionDecl> {
        self.actions.iter_mut().find(|d| d.id == id)
    }

    /// **Lend out** the screen a resident declaration owns, for exactly one call.
    ///
    /// The screen leaves the declaration and comes back through [`Registry::put_resident`]. Taking
    /// it rather than handing out a `&mut` is what makes **nesting** safe: while a screen is being
    /// drawn its slot is empty, so a screen that reaches for itself — directly or round a cycle —
    /// gets `None` and draws nothing, where a shared cell would have panicked and a plain `&mut`
    /// would not have compiled at all.
    ///
    /// `None` where the id is gone, the declaration is a factory one, or the screen is already out.
    pub(crate) fn take_resident(&mut self, id: &str) -> Option<Box<dyn Screen>> {
        match &mut self.screens.iter_mut().find(|d| d.id() == id)?.source {
            ScreenSource::Resident(screen) => screen.take(),
            ScreenSource::Factory(_) => None,
        }
    }

    /// Give back what [`Registry::take_resident`] lent out.
    ///
    /// Where the declaration has gone in the meantime (`shell.remove(id)` from inside the screen's
    /// own `ui`) the screen is dropped — the declaration it belonged to no longer exists.
    pub(crate) fn put_resident(&mut self, id: &str, screen: Box<dyn Screen>) {
        if let Some(decl) = self.screens.iter_mut().find(|d| d.id() == id) {
            if let ScreenSource::Resident(slot) = &mut decl.source {
                *slot = Some(screen);
            }
        }
    }

    /// Whether a screen is declared under this id. The list a screen draws is filtered on it, so an
    /// entry never points at something that is not there.
    #[must_use]
    pub fn has_screen(&self, id: &str) -> bool {
        self.screens.iter().any(|d| d.id() == id)
    }

    /// Whether a resident declaration under this id **has its screen in hand right now** — so
    /// [`Registry::take_resident`] would answer with it.
    ///
    /// [`ScreenDecl::is_resident`] is not the same question: it stays true while the screen is out
    /// on loan, which is exactly when it cannot be lent again.
    #[must_use]
    pub(crate) fn has_resident(&self, id: &str) -> bool {
        matches!(
            self.screen(id).map(|d| &d.source),
            Some(ScreenSource::Resident(Some(_)))
        )
    }

    /// Every screen, in insertion order.
    #[must_use]
    pub fn screens(&self) -> &[ScreenDecl] {
        &self.screens
    }

    /// Every action, in insertion order.
    #[must_use]
    pub fn actions(&self) -> &[ActionDecl] {
        &self.actions
    }

    /// The status bar items (mutable — for rendering).
    pub(crate) fn status_items_mut(&mut self) -> &mut [StatusItemDecl] {
        &mut self.status_items
    }

    /// The status bar items.
    #[must_use]
    pub fn status_items(&self) -> &[StatusItemDecl] {
        &self.status_items
    }

    /// The nav bar items (mutable — for rendering).
    pub(crate) fn nav_items_mut(&mut self) -> &mut [NavItemDecl] {
        &mut self.nav_items
    }

    /// The tile declarations (feature `overlay`).
    #[cfg(feature = "overlay")]
    #[must_use]
    pub fn tiles(&self) -> &[TileDecl] {
        &self.tiles
    }

    #[cfg(feature = "overlay")]
    fn tile_index(&self, id: &str) -> Option<usize> {
        self.tiles.iter().position(|d| d.id() == id)
    }

    #[cfg(not(feature = "overlay"))]
    #[allow(clippy::unused_self)]
    fn tile_index(&self, _id: &str) -> Option<usize> {
        None
    }

    /// The nav bar items.
    #[must_use]
    pub fn nav_items(&self) -> &[NavItemDecl] {
        &self.nav_items
    }

    /// Whether it changed since the last check. Reading it clears it (it triggers a desktop rebuild).
    pub(crate) fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}

/// Warn about a declaration that asked for the desktop or the dock with no icon. It fires once
/// per `add` — the desktop rebuild runs on every `dirty` frame, so warning there would pile the
/// log up.
fn warn_unusable_icon_placement(decl: &Decl) {
    let (id, has_icon, wants_slot) = match decl {
        Decl::Screen(d) => (
            &d.id,
            d.icon.is_some(),
            d.desktop != DesktopPlacement::None || d.dock,
        ),
        Decl::Action(d) => (
            &d.id,
            d.icon.is_some(),
            d.desktop != DesktopPlacement::None || d.dock,
        ),
        // Top-bar and nav-bar items use no icon slot.
        Decl::StatusItem(_) | Decl::NavItem(_) => return,
        #[cfg(feature = "overlay")]
        Decl::Tile(_) => return,
    };
    if wants_slot && !has_icon {
        log::warn!("`{id}`: no icon, so it does not go on the desktop or dock - add `.icon(..)`");
    }
}

#[cfg(test)]
mod tests {
    use super::{DeclKind, Registry};
    use crate::icons::IconRef;
    use crate::screen::{action, screen};

    fn noop_screen(id: &str) -> super::ScreenDecl {
        screen(id, |ui: &mut egui::Ui, _: &mut crate::screen::Cx<'_>| {
            ui.label("x");
        })
    }

    /// Insertion order is `.desktop()`'s auto-placement order.
    #[test]
    fn add_preserves_insertion_order() {
        let mut registry = Registry::new();
        for id in ["a", "b", "c"] {
            let _ = registry.add(noop_screen(id).into());
        }
        let ids: Vec<&str> = registry
            .screens()
            .iter()
            .map(super::ScreenDecl::id)
            .collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    /// Adding the same id again replaces it. The replaced declaration comes back and the new one goes to the end.
    #[test]
    fn add_replaces_same_id_and_moves_it_to_the_back() {
        let mut registry = Registry::new();
        for id in ["a", "b"] {
            let _ = registry.add(noop_screen(id).into());
        }
        let replaced = registry.add(noop_screen("a").title("new a").into());
        assert!(replaced.is_some_and(|d| d.id() == "a"));
        let ids: Vec<&str> = registry
            .screens()
            .iter()
            .map(super::ScreenDecl::id)
            .collect();
        assert_eq!(ids, ["b", "a"]);
        assert_eq!(
            registry.screen("a").map(super::ScreenDecl::label),
            Some("new a")
        );
        assert_eq!(registry.screens().len(), 2);
    }

    /// A replacement may change the kind (with a warning) — a lookup returns the new kind.
    #[test]
    fn add_can_change_kind_of_an_existing_id() {
        let mut registry = Registry::new();
        let _ = registry.add(noop_screen("a").into());
        assert_eq!(registry.kind("a"), Some(DeclKind::Screen));
        let replaced = registry.add(action("a", |_| {}).into());
        assert!(replaced.is_some_and(|d| d.kind() == DeclKind::Screen));
        assert_eq!(registry.kind("a"), Some(DeclKind::Action));
        assert!(registry.screens().is_empty());
        assert_eq!(registry.actions().len(), 1);
    }

    /// It registers even with no icon (it can be opened from code or a deep link). Only a warning.
    #[test]
    fn desktop_without_icon_still_registers() {
        let mut registry = Registry::new();
        let _ = registry.add(noop_screen("a").desktop().dock().into());
        assert_eq!(registry.kind("a"), Some(DeclKind::Screen));
        assert!(registry.screen("a").is_some_and(|d| d.icon_ref().is_none()));
        // With an icon, the same result and no warning.
        let _ = registry.add(
            noop_screen("b")
                .icon(IconRef::Builtin("gauge"))
                .desktop()
                .into(),
        );
        assert!(registry.screen("b").is_some_and(|d| d.icon_ref().is_some()));
    }

    /// `take_dirty` is raised only by add and remove, and reading it lowers it.
    #[test]
    fn dirty_is_set_by_add_and_remove_only() {
        let mut registry = Registry::new();
        assert!(!registry.take_dirty());
        let _ = registry.add(noop_screen("a").into());
        assert!(registry.take_dirty());
        assert!(!registry.take_dirty());
        assert!(registry.remove("nope").is_none());
        assert!(
            !registry.take_dirty(),
            "removing an id that is not there does not raise dirty"
        );
        assert!(registry.remove("a").is_some());
        assert!(registry.take_dirty());
    }
}
