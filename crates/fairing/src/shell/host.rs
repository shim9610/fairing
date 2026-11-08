//! Sealed host operations shared by ordinary and protected shells.
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::super::Shell {}
    impl Sealed for super::super::ProtectedShell {}
}

/// A library-owned shell accepted by runners and headless hosts.
///
/// Only `Shell` and `ProtectedShell` implement this contract. It provides no
/// conversion between registration modes and cannot be implemented downstream.
///
/// ```compile_fail
/// struct External(fairing::Shell);
/// impl fairing::shell::HostedShell for External {
///     fn is_faulted(&self) -> bool { self.0.is_faulted() }
///     fn quarantine(&mut self) { self.0.quarantine(); }
///     fn now(&self) -> std::time::Instant { self.0.now() }
///     fn logic(&mut self, ctx: &fairing::egui::Context) { self.0.logic(ctx); }
///     fn logic_with(&mut self, ctx: &fairing::egui::Context, app: &mut dyn std::any::Any) {
///         self.0.logic_with(ctx, app);
///     }
///     fn frame(&mut self, ui: &mut fairing::egui::Ui) { self.0.frame(ui); }
///     fn frame_with(&mut self, ui: &mut fairing::egui::Ui, app: &mut dyn std::any::Any) {
///         self.0.frame_with(ui, app);
///     }
///     fn poll_events(&mut self) -> Vec<fairing::ShellEvent> { self.0.poll_events() }
/// }
/// ```
pub trait HostedShell: sealed::Sealed {
    /// Forward the corresponding shell `is_faulted` operation.
    fn is_faulted(&self) -> bool;
    /// Forward the corresponding shell `quarantine` operation.
    fn quarantine(&mut self);
    /// Forward the corresponding shell `now` operation.
    fn now(&self) -> std::time::Instant;
    /// Forward the corresponding shell `logic` operation.
    fn logic(&mut self, ctx: &egui::Context);
    /// Forward the corresponding shell `logic_with` operation.
    fn logic_with(&mut self, ctx: &egui::Context, app: &mut dyn std::any::Any);
    /// Forward the corresponding shell `frame` operation.
    fn frame(&mut self, ui: &mut egui::Ui);
    /// Forward the corresponding shell `frame_with` operation.
    fn frame_with(&mut self, ui: &mut egui::Ui, app: &mut dyn std::any::Any);
    /// Forward the corresponding shell `poll_events` operation.
    fn poll_events(&mut self) -> Vec<super::ShellEvent>;
}
impl HostedShell for super::Shell {
    fn is_faulted(&self) -> bool {
        super::Shell::is_faulted(self)
    }
    fn quarantine(&mut self) {
        super::Shell::quarantine(self);
    }
    fn now(&self) -> std::time::Instant {
        super::Shell::now(self)
    }
    fn logic(&mut self, ctx: &egui::Context) {
        super::Shell::logic(self, ctx);
    }
    fn logic_with(&mut self, ctx: &egui::Context, app: &mut dyn std::any::Any) {
        super::Shell::logic_with(self, ctx, app);
    }
    fn frame(&mut self, ui: &mut egui::Ui) {
        super::Shell::frame(self, ui);
    }
    fn frame_with(&mut self, ui: &mut egui::Ui, app: &mut dyn std::any::Any) {
        super::Shell::frame_with(self, ui, app);
    }
    fn poll_events(&mut self) -> Vec<super::ShellEvent> {
        super::Shell::poll_events(self)
    }
}
impl HostedShell for super::ProtectedShell {
    fn is_faulted(&self) -> bool {
        super::ProtectedShell::is_faulted(self)
    }
    fn quarantine(&mut self) {
        super::ProtectedShell::quarantine(self);
    }
    fn now(&self) -> std::time::Instant {
        super::ProtectedShell::now(self)
    }
    fn logic(&mut self, ctx: &egui::Context) {
        super::ProtectedShell::logic(self, ctx);
    }
    fn logic_with(&mut self, ctx: &egui::Context, app: &mut dyn std::any::Any) {
        super::ProtectedShell::logic_with(self, ctx, app);
    }
    fn frame(&mut self, ui: &mut egui::Ui) {
        super::ProtectedShell::frame(self, ui);
    }
    fn frame_with(&mut self, ui: &mut egui::Ui, app: &mut dyn std::any::Any) {
        super::ProtectedShell::frame_with(self, ui, app);
    }
    fn poll_events(&mut self) -> Vec<super::ShellEvent> {
        super::ProtectedShell::poll_events(self)
    }
}
