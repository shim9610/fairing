//! Chrome — the shell around the content. M1: the status bar ([`StatusBar`]) and
//! the button-style nav bar ([`NavBar`]). The shade, toasts and the OSK are M2.
//!
//! Both bars can be **replaced wholesale** through a [`BarPainter`].

pub(crate) mod bar;
mod guard;
mod nav_bar;
mod status_bar;

pub use bar::{BarCx, BarItem, BarKind, BarPainter};
#[doc(hidden)]
pub use guard::PolicyDriver;
pub use guard::EMERGENCY_GATE;
pub use nav_bar::{
    nav_item, NavAction, NavBar, NavItem, NavItemDecl, NavLayout, NavLayoutCx, NavStyle,
};
pub use status_bar::{
    status_item, LabelPos, Slot, StatusBar, StatusBarAction, StatusItem, StatusItemDecl,
    StatusItemSpec, StatusLayout, StatusLayoutCx, BUILTIN_IDS,
};
