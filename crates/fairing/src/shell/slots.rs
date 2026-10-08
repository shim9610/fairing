//! Slots for the subsystems a feature can remove (the overlay and the OSK).
//! With the feature on they wrap the real thing; with it off the same set of methods does
//! nothing — so that the frame loop in `shell/mod.rs` reads as one piece, with no `cfg`.

use crate::screen::CxParts;
use crate::theme::MotionTokens;
use egui::Rect;
use std::time::{Duration, Instant};

#[cfg(feature = "overlay")]
pub(crate) use with_overlay::OverlaySlot;
#[cfg(not(feature = "overlay"))]
pub(crate) use without_overlay::OverlaySlot;

#[cfg(feature = "osk")]
pub(crate) use with_osk::OskSlot;
#[cfg(not(feature = "osk"))]
pub(crate) use without_osk::OskSlot;

/// What the overlay slot returns in stage 11 (the same with the feature either way).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct OverlayOut {
    /// The action for the shell to handle.
    pub action: Option<OverlaySlotAction>,
    /// The panel's **revealed** rect (the visible part of the curtain) — used to clip the status row.
    pub panel: Option<Rect>,
    /// The status row inside the panel (status bar hidden, panel visible). It is in panel
    /// content coordinates (pinned to the top), so before the curtain is fully down only its
    /// intersection with `panel` shows.
    pub status_row: Option<Rect>,
    /// The peeked status row.
    pub peek: Option<Rect>,
}

/// A feature-independent action (a copy of `overlay::PanelAction`). With the feature off nobody
/// constructs one, but the shell's stage 14 branch stays a single piece.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(feature = "overlay"), allow(dead_code))]
pub(crate) enum OverlaySlotAction {
    Launch(crate::screen::LaunchAction),
    TileLocked(String),
    TileLongPressed(String),
    NotificationTapped(crate::notify::NotificationId),
    Dismiss(crate::notify::NotificationId),
    ClearAll,
}

#[cfg(feature = "overlay")]
mod with_overlay {
    use super::{OverlayOut, OverlaySlotAction};
    use crate::config::OverlayConfig;
    use crate::gesture::Gesture;
    use crate::notify::NotificationCenter;
    use crate::overlay::{Overlay, OverlayInput, PanelAction, TileDecl};
    use crate::screen::CxParts;
    use crate::theme::MotionTokens;
    use egui::Rect;
    use std::time::{Duration, Instant};

    /// The real overlay.
    #[derive(Debug)]
    pub(crate) struct OverlaySlot(pub Overlay);

    impl OverlaySlot {
        pub(crate) fn from_config(
            cfg: &OverlayConfig,
            dismiss_button: bool,
            tokens: &MotionTokens,
        ) -> Self {
            Self(Overlay::from_config(cfg, dismiss_button, tokens))
        }
        pub(crate) fn is_closed(&self) -> bool {
            self.0.is_closed()
        }
        pub(crate) fn is_animating(&self) -> bool {
            self.0.is_animating()
        }
        pub(crate) fn status_bar_opacity(&self) -> f32 {
            self.0.status_bar_opacity()
        }
        pub(crate) fn open(&mut self, tokens: &MotionTokens) {
            self.0.open(tokens);
        }
        pub(crate) fn close(&mut self, tokens: &MotionTokens) {
            self.0.close(tokens);
        }
        pub(crate) fn bar_tapped(&mut self, tokens: &MotionTokens) -> bool {
            self.0.bar_tapped(tokens)
        }
        pub(crate) fn mark_tiles_dirty(&mut self) {
            self.0.mark_tiles_dirty();
        }
        pub(crate) fn peek_remaining(&mut self, now: Instant) -> Option<Duration> {
            self.0.peek_remaining(now)
        }
        /// Pin the status row's Area (`status_area_id`) directly above the panel on every pass (z-order).
        pub(crate) fn pin_status_row_above_panel(ctx: &egui::Context, status_area_id: &str) {
            ctx.set_sublayer(
                egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new(crate::overlay::PANEL_AREA_ID),
                ),
                egui::LayerId::new(egui::Order::Foreground, egui::Id::new(status_area_id)),
            );
        }
        #[allow(clippy::too_many_arguments)]
        pub(crate) fn update(
            &mut self,
            gesture: Option<Gesture>,
            screen: Rect,
            content: Rect,
            status_hidden: bool,
            status_height: f32,
            allowed: bool,
            now: Instant,
            dt: f32,
            tokens: &MotionTokens,
        ) -> bool {
            self.0.update(
                &OverlayInput {
                    gesture,
                    screen,
                    content,
                    status_hidden,
                    status_height,
                    allowed,
                    now,
                    dt,
                },
                tokens,
            )
        }
        pub(crate) fn ui(
            &mut self,
            ctx: &egui::Context,
            parts: &mut CxParts<'_>,
            screen: Rect,
            center: &NotificationCenter,
            tiles: &[TileDecl],
        ) -> OverlayOut {
            let action = self
                .0
                .ui(ctx, parts, screen, center, tiles)
                .map(|a| match a {
                    PanelAction::Launch(l) => OverlaySlotAction::Launch(l),
                    PanelAction::TileLocked { id } => OverlaySlotAction::TileLocked(id),
                    PanelAction::TileLongPressed { id } => OverlaySlotAction::TileLongPressed(id),
                    PanelAction::NotificationTapped(id) => {
                        OverlaySlotAction::NotificationTapped(id)
                    }
                    PanelAction::Dismiss(id) => OverlaySlotAction::Dismiss(id),
                    PanelAction::ClearAll => OverlaySlotAction::ClearAll,
                });
            let frame = self.0.frame();
            OverlayOut {
                action,
                panel: frame.panel,
                status_row: frame.status_row,
                peek: frame.peek,
            }
        }
    }
}

#[cfg(not(feature = "overlay"))]
#[allow(clippy::unused_self)] // It keeps the same method shape as the real slot (so the shell reads as one piece).
mod without_overlay {
    use super::OverlayOut;
    use crate::config::OverlayConfig;
    use crate::gesture::Gesture;
    use crate::notify::NotificationCenter;
    use crate::screen::CxParts;
    use crate::theme::MotionTokens;
    use egui::Rect;
    use std::time::{Duration, Instant};

    /// No overlay (feature `overlay` off). A top pull and `OpenOverlay` do nothing.
    #[derive(Debug, Default)]
    pub(crate) struct OverlaySlot;

    impl OverlaySlot {
        pub(crate) fn from_config(
            _cfg: &OverlayConfig,
            _dismiss_button: bool,
            _tokens: &MotionTokens,
        ) -> Self {
            Self
        }
        pub(crate) fn is_closed(&self) -> bool {
            true
        }
        pub(crate) fn is_animating(&self) -> bool {
            false
        }
        pub(crate) fn status_bar_opacity(&self) -> f32 {
            1.0
        }
        pub(crate) fn open(&mut self, _tokens: &MotionTokens) {
            log::info!("the `overlay` feature is off, so the shade cannot open");
        }
        pub(crate) fn close(&mut self, _tokens: &MotionTokens) {}
        pub(crate) fn bar_tapped(&mut self, _tokens: &MotionTokens) -> bool {
            false
        }
        pub(crate) fn mark_tiles_dirty(&mut self) {}
        pub(crate) fn peek_remaining(&mut self, _now: Instant) -> Option<Duration> {
            None
        }
        pub(crate) fn pin_status_row_above_panel(_ctx: &egui::Context, _status_area_id: &str) {}
        #[allow(clippy::too_many_arguments, clippy::unused_self)]
        pub(crate) fn update(
            &mut self,
            _gesture: Option<Gesture>,
            _screen: Rect,
            _content: Rect,
            _status_hidden: bool,
            _status_height: f32,
            _allowed: bool,
            _now: Instant,
            _dt: f32,
            _tokens: &MotionTokens,
        ) -> bool {
            false
        }
        #[allow(clippy::unused_self)]
        pub(crate) fn ui(
            &mut self,
            _ctx: &egui::Context,
            _parts: &mut CxParts<'_>,
            _screen: Rect,
            _center: &NotificationCenter,
            _tiles: &[()],
        ) -> OverlayOut {
            OverlayOut::default()
        }
    }
}

#[cfg(feature = "osk")]
mod with_osk {
    use crate::config::OskConfig;
    use crate::icons::IconSet;
    use crate::osk::{Osk, OskAction};
    use crate::screen::OskMode;
    use crate::theme::{MotionTokens, Theme};
    use egui::Rect;
    use std::time::Instant;

    /// The real OSK.
    #[derive(Debug)]
    pub(crate) struct OskSlot(pub Osk);

    impl OskSlot {
        pub(crate) fn from_config(cfg: &OskConfig) -> Self {
            Self(Osk::from_config(cfg))
        }
        pub(crate) fn inset_bottom(&self) -> f32 {
            self.0.inset_bottom()
        }
        pub(crate) fn is_shown(&self) -> bool {
            self.0.is_shown()
        }
        /// The fully extended height — where the keys stop once they are up.
        pub(crate) fn height(&self) -> f32 {
            self.0.height()
        }
        pub(crate) fn is_animating(&self) -> bool {
            self.0.is_animating()
        }
        pub(crate) fn hide(&mut self) {
            self.0.hide();
        }
        pub(crate) fn toggle(&mut self) {
            self.0.toggle();
        }
        pub(crate) fn raise(&mut self, above: bool) {
            self.0.raise(above);
        }
        pub(crate) fn set_max_key(&mut self, du: f32) {
            self.0.set_max_key(du);
        }
        pub(crate) fn update(
            &mut self,
            wants: bool,
            mode: OskMode,
            screen_height: f32,
            now: Instant,
            dt: f32,
            tokens: &MotionTokens,
        ) -> bool {
            self.0.update(wants, mode, screen_height, now, dt, tokens)
        }
        pub(crate) fn ui(
            &mut self,
            ctx: &egui::Context,
            osk_rect: Option<Rect>,
            theme: &Theme,
            icons: &mut IconSet,
        ) -> bool {
            let Some(rect) = osk_rect else {
                return false;
            };
            matches!(
                self.0.ui(ctx, rect, theme, icons),
                Some(OskAction::HideRequested)
            )
        }
    }
}

#[cfg(not(feature = "osk"))]
#[allow(clippy::unused_self)] // It keeps the same method shape as the real slot.
mod without_osk {
    use crate::config::OskConfig;
    use crate::icons::IconSet;
    use crate::screen::OskMode;
    use crate::theme::{MotionTokens, Theme};
    use egui::Rect;
    use std::time::Instant;

    /// No OSK (feature `osk` off) — there is a hardware keyboard.
    #[derive(Debug, Default)]
    pub(crate) struct OskSlot;

    impl OskSlot {
        pub(crate) fn from_config(_cfg: &OskConfig) -> Self {
            Self
        }
        pub(crate) fn inset_bottom(&self) -> f32 {
            0.0
        }
        pub(crate) fn is_shown(&self) -> bool {
            false
        }
        pub(crate) fn height(&self) -> f32 {
            0.0
        }
        pub(crate) fn is_animating(&self) -> bool {
            false
        }
        pub(crate) fn hide(&mut self) {}
        pub(crate) fn toggle(&mut self) {}
        pub(crate) fn raise(&mut self, _above: bool) {}
        pub(crate) fn set_max_key(&mut self, _du: f32) {}
        #[allow(clippy::unused_self)]
        pub(crate) fn update(
            &mut self,
            _wants: bool,
            _mode: OskMode,
            _screen_height: f32,
            _now: Instant,
            _dt: f32,
            _tokens: &MotionTokens,
        ) -> bool {
            false
        }
        #[allow(clippy::unused_self)]
        pub(crate) fn ui(
            &mut self,
            _ctx: &egui::Context,
            _osk_rect: Option<Rect>,
            _theme: &Theme,
            _icons: &mut IconSet,
        ) -> bool {
            false
        }
    }
}

/// Checks that both slots share a signature (types only, at compile time).
#[allow(dead_code)]
fn _assert_slots(
    _: &OverlaySlot,
    _: &OskSlot,
    _: &mut CxParts<'_>,
    _: &MotionTokens,
    _: Rect,
    _: Instant,
    _: Duration,
) {
}
