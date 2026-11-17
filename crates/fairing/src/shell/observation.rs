//! Host observations without component or registration access.

/// A read-only view of shell state. It provides no backend, component or legacy-shell loan.
///
/// ```compile_fail
/// fn escape<'a>(view: fairing::shell::ShellView<'a>) -> &'a fairing::Shell {
///     view.shell
/// }
/// ```
#[derive(Clone, Copy)]
pub struct ShellView<'a> {
    pub(super) shell: &'a super::Shell,
}
impl<'a> ShellView<'a> {
    /// Observe the current token/policy decision without dispatching work.
    #[must_use]
    pub fn allows(&self, gate: &crate::Gate) -> bool {
        self.shell.access().allows(gate)
    }
    /// Registered screen identifiers in declaration order.
    pub fn screen_ids(&self) -> impl Iterator<Item = &'a str> {
        self.shell
            .registry
            .screens()
            .iter()
            .map(crate::screen::ScreenDecl::id)
    }
    /// Whether a screen identifier is registered.
    #[must_use]
    pub fn has_screen(&self, id: &str) -> bool {
        self.shell.registry.has_screen(id)
    }
    /// Whether the declaration retains a resident screen body across opens.
    #[must_use]
    pub fn screen_is_resident(&self, id: &str) -> Option<bool> {
        self.shell
            .registry
            .screen(id)
            .map(crate::screen::ScreenDecl::is_resident)
    }
    /// A navigation item's current hit rectangle, without a bar component loan.
    #[must_use]
    pub fn nav_item_rect(&self, item: &crate::chrome::NavItem) -> Option<egui::Rect> {
        self.shell.nav_bar().item_rect(item)
    }
    /// The current read-only token capability, if attached.
    #[must_use]
    pub fn session_token(&self) -> Option<&'a crate::access::SessionToken> {
        self.shell.access().session_token()
    }
    /// Current layout geometry; this value model is a supported host contract.
    #[must_use]
    pub fn layout(&self) -> &super::Layout {
        self.shell.layout()
    }
    /// Current monotonic observation time.
    #[must_use]
    pub fn now(&self) -> std::time::Instant {
        self.shell.now()
    }
    /// Whether normal dispatch is quarantined.
    #[must_use]
    pub fn is_faulted(&self) -> bool {
        self.shell.is_faulted()
    }
    /// Whether a credential prompt is visible.
    #[must_use]
    pub fn unlock_prompt_visible(&self) -> bool {
        self.shell.unlock_prompt_visible()
    }
    /// Whether the lock screen is visible.
    #[must_use]
    pub fn lock_screen_visible(&self) -> bool {
        self.shell.lock_screen_visible()
    }
    /// The current immutable settings values, without backend mutation access.
    #[must_use]
    pub fn settings(&self) -> &crate::settings::SettingsView {
        self.shell.settings()
    }
}
