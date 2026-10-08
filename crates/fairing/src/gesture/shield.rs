//! The gesture shield: a transparent full-screen
//! `Area(Order::Foreground)` from the moment an edge gesture is recognised until it is released.
//! It stops the widgets underneath from receiving the pointer.
//!
//! egui 0.36.1 only hit-tests layers registered in `Areas::order` (`hit_test.rs`), so within the
//! same `Foreground` this Area — **registered later** — sits above the shade panel and the
//! status bar overlay. A widget that already holds the drag (`drag_id`) keeps it until release,
//! so the shield only blocks **new** hits — which is why it is not raised inside the slop (taps
//! pass through).
//!
//! Blocking input during a transition (the A2/A3 tweens) uses the same function; M1 had only
//! `interactable(false)`.
//!
//! **Why no tap leaks on the release frame** (confirmed against egui 0.36.1 `interaction.rs`):
//! the engine returns to `Idle` on the release frame and does not draw the shield that frame,
//! but egui's `clicked()` only fires when `potential_click_id` (the widget hit on the press
//! frame) is hit again on the release frame and `!is_decidedly_dragging()` (it moved less than
//! 6 px). The shield goes up only past the slop (12 px), so a press that had a shield is already
//! "decidedly dragging" and no widget is clicked on release — the status bar's
//! `tap_opens_shade` never closes the shade it just opened
//! (`m2_shade::release_past_snap_opens_and_settles_within_450ms` pins this). A tap inside the
//! slop passing through with no shield is the contract to begin with.

use egui::{Rect, Sense};

/// The shield `Area`'s id name.
pub(crate) const AREA_ID: &str = "fairing.gesture.shield";

/// Draw a transparent shield over `rect` this frame. The returned `Response` shows the input it
/// absorbed (usually ignored).
#[must_use]
pub(crate) fn show(ctx: &egui::Context, rect: Rect) -> egui::Response {
    egui::Area::new(egui::Id::new(AREA_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .constrain(false)
        .fade_in(false)
        .interactable(true)
        .sense(Sense::click_and_drag())
        .show(ctx, |ui| ui.allocate_rect(rect, Sense::click_and_drag()))
        .inner
}

#[cfg(test)]
mod tests {
    use super::AREA_ID;
    use crate::desktop::DesktopView;
    use crate::services::{null::NullClock, Services};
    use crate::testing::{single_level_access, Harness};
    use crate::{screen, Cx, Error, LaunchAction, Result, Shell};
    use std::cell::Cell;
    use std::rc::Rc;

    /// The screen width of a 1024 × 600 reference panel.
    const W: f32 = 1024.0;

    /// A `reduce = false` shell on the Null clock, so the tweens run.
    fn animated(setup: impl FnOnce(&mut Shell)) -> Result<Harness> {
        let services = Services::builder().clock(NullClock).build();
        let mut h = Harness::new(single_level_access(), services)?;
        setup(&mut h.shell);
        Ok(h)
    }

    /// Until the transition is over (3 s at most).
    fn run_until_idle(h: &mut Harness) -> Result<()> {
        for _ in 0..180 {
            h.frame();
            if !h.shell.is_animating() {
                return Ok(());
            }
        }
        Err(Error::Config(
            "the transition did not end inside 3 s".to_owned(),
        ))
    }

    /// From where it is, it drags for `frames` frames at `step` px/frame (never letting go). The
    /// last position.
    fn drag_by(h: &mut Harness, from: egui::Pos2, step: egui::Vec2, frames: usize) -> egui::Pos2 {
        let mut pos = from;
        for _ in 0..frames {
            pos += step;
            h.move_to(pos);
            h.frame();
        }
        pos
    }

    /// Whether the shield layer is covering this place.
    fn shielded(h: &Harness, pos: egui::Pos2) -> bool {
        h.ctx.layer_id_at(pos)
            == Some(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new(AREA_ID),
            ))
    }

    /// During an A2/A3 **tween** the shield covers the screen and the widgets below take no
    /// taps. The gesture back, whose finger is driving the value, puts no shield up (it is the input itself).
    #[test]
    fn shield_blocks_taps_during_push_and_gesture_back_tween() -> Result<()> {
        let hits = Rc::new(Cell::new(0u32));
        let counter = Rc::clone(&hits);
        // **Where the button lands, recorded rather than assumed.** This used to tap a literal
        // `(20, 60)`, which was inside the button only while `screen_inset` was frozen at 12 du. It
        // resolves with the finger since the control vocabulary's adoption step 4 — 20.47 at the gloved
        // default — so the literal fell outside and the test failed for a reason that had nothing to do
        // with the shield it is about.
        let button = Rc::new(Cell::new(egui::Rect::NOTHING));
        let seen = Rc::clone(&button);
        let mut h = animated(move |sh| {
            let counter = Rc::clone(&counter);
            let seen = Rc::clone(&seen);
            sh.add(screen("a", move |ui: &mut egui::Ui, _: &mut Cx| {
                let r = ui.button("hit");
                seen.set(r.rect);
                if r.clicked() {
                    counter.set(counter.get() + 1);
                }
            }));
            sh.add(screen("b", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("b");
            }));
        })?;
        let center = egui::pos2(W / 2.0, 300.0);

        // Home: no shield.
        h.frames(2);
        assert!(!shielded(&h, center));
        assert_eq!(
            h.ctx.layer_id_at(center),
            Some(DesktopView::layer_id()),
            "at home the desktop is on top"
        );

        // Mid-A2-open tween → a shield.
        h.shell.handle().launch(LaunchAction::open("a"));
        h.frames(3);
        assert!(h.shell.is_animating() && shielded(&h, center));
        run_until_idle(&mut h)?;
        assert!(!shielded(&h, center), "it comes away once it is over");

        // With no tween a tap at the same place goes through (the baseline).
        h.tap(button.get().center());
        let baseline = hits.get();
        assert_eq!(baseline, 1, "once the tween is over the button is pressed");

        // A tap during the A3 push tween → refused.
        h.shell.handle().launch(LaunchAction::open("b"));
        h.frames(2);
        assert!(
            h.shell.is_animating() && shielded(&h, center),
            "the push tween"
        );
        h.press(button.get().center());
        h.frame();
        h.release(button.get().center());
        h.frame();
        h.frame();
        assert_eq!(hits.get(), baseline, "no click leaks during a tween");
        run_until_idle(&mut h)?;

        // The gesture back: while the finger is running, the workspace puts no shield up — the gesture engine puts
        // its own up (the same Area).
        let from = egui::pos2(4.0, 300.0);
        h.press(from);
        h.frame();
        let end = drag_by(&mut h, from, egui::vec2(15.0, 0.0), 6);
        assert!(
            shielded(&h, center),
            "the engine puts the gesture shield up"
        );
        h.release(end);
        h.frame();
        run_until_idle(&mut h)?;
        assert!(!shielded(&h, center));
        Ok(())
    }
}
