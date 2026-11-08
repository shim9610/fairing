//! Compiler fixtures: the analyzer must distinguish these actual Rust call paths.
mod access {
    pub mod protected_dispatch {
        pub fn invoke<R>(allowed: bool, callback: impl FnOnce() -> R) -> Option<R> {
            if allowed {
                Some(callback())
            } else {
                None
            }
        }
    }
}
pub fn guarded_only(callback: impl FnOnce()) {
    access::protected_dispatch::invoke(true, callback);
}
pub fn direct_bypass(callback: impl FnOnce()) {
    callback();
}
pub fn bypass_after_a_guard(first: impl FnOnce(), second: impl FnOnce()) {
    access::protected_dispatch::invoke(true, first);
    second();
}
pub fn pointer_bypass(callback: fn()) {
    callback();
}
pub trait Renderer {
    fn draw(&mut self);
}
pub fn virtual_bypass(callback: &mut dyn Renderer) {
    callback.draw();
}
pub fn unreachable_callback(callback: impl FnOnce()) {
    if false {
        callback();
    }
}
