use super::{Overlay, SHOT_PATIENCE};
use egui::{Color32, ColorImage, Context, RawInput, Rect, ScreenshotCallback, Vec2};
use std::sync::Arc;

fn overlay() -> Overlay {
    let cfg = crate::config::OverlayConfig {
        reveal: "card".into(),
        card_glass: 0.7,
        ..crate::config::OverlayConfig::default()
    };
    Overlay::from_config(&cfg, true, &crate::theme::Theme::default().motion)
}

fn frame(
    ctx: &Context,
    overlay: &mut Overlay,
    first: bool,
    surface: Color32,
    size: f32,
) -> Option<ScreenshotCallback> {
    let output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(size))),
            ..RawInput::default()
        },
        |_| overlay.frost_backdrop(ctx, first, surface),
    );
    let callback = output
        .viewport_output
        .values()
        .flat_map(|v| &v.commands)
        .find_map(|command| {
            if let egui::ViewportCommand::Screenshot(callback) = command {
                Some(callback.clone())
            } else {
                None
            }
        });
    output.drop_without_applying_deltas();
    callback
}

fn request(ctx: &Context, overlay: &mut Overlay) -> crate::Result<ScreenshotCallback> {
    frame(ctx, overlay, true, Color32::BLACK, 64.0)
        .ok_or_else(|| crate::Error::Config("test screenshot was not requested".into()))
}

fn image() -> Arc<ColorImage> {
    Arc::new(ColorImage::new([8, 8], vec![Color32::WHITE; 64]))
}

#[test]
fn a_closed_cards_reply_cannot_complete_a_new_cards_request() -> crate::Result<()> {
    let ctx = Context::default();
    let mut overlay = overlay();
    let old = request(&ctx, &mut overlay)?;
    overlay.reset_panel_state();
    let current = request(&ctx, &mut overlay)?;
    old.complete(image());
    frame(&ctx, &mut overlay, false, Color32::BLACK, 64.0);
    assert!(overlay.backdrop.is_none());
    current.complete(image());
    frame(&ctx, &mut overlay, false, Color32::BLACK, 64.0);
    assert!(overlay.backdrop.is_some());
    Ok(())
}

#[test]
fn a_timed_out_reply_is_discarded_and_a_fresh_request_still_works() -> crate::Result<()> {
    let ctx = Context::default();
    let mut overlay = overlay();
    let old = request(&ctx, &mut overlay)?;
    for _ in 0..SHOT_PATIENCE {
        frame(&ctx, &mut overlay, false, Color32::BLACK, 64.0);
    }
    assert!(overlay.pending_shot.is_none());
    old.complete(image());
    frame(&ctx, &mut overlay, false, Color32::BLACK, 64.0);
    assert!(overlay.backdrop.is_none());
    request(&ctx, &mut overlay)?.complete(image());
    frame(&ctx, &mut overlay, false, Color32::BLACK, 64.0);
    assert!(overlay.backdrop.is_some());
    Ok(())
}

#[test]
fn theme_and_viewport_changes_invalidate_pending_captures() -> crate::Result<()> {
    for resized in [false, true] {
        let ctx = Context::default();
        let mut overlay = overlay();
        let pending = request(&ctx, &mut overlay)?;
        pending.complete(image());
        let (surface, size) = if resized {
            (Color32::BLACK, 96.0)
        } else {
            (Color32::WHITE, 64.0)
        };
        frame(&ctx, &mut overlay, false, surface, size);
        assert!(overlay.backdrop.is_none());
        assert!(overlay.pending_shot.is_none());
    }
    Ok(())
}

#[test]
fn a_screenshot_callback_wakes_an_idle_context() -> crate::Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let ctx = Context::default();
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&wakes);
    ctx.set_request_repaint_callback(move |_| {
        count.fetch_add(1, Ordering::Relaxed);
    });
    let mut overlay = overlay();
    let pending = request(&ctx, &mut overlay)?;
    for _ in 0..3 {
        frame(&ctx, &mut overlay, false, Color32::BLACK, 64.0);
    }
    let before = wakes.load(Ordering::Relaxed);
    pending.complete(image());
    assert!(wakes.load(Ordering::Relaxed) > before);
    frame(&ctx, &mut overlay, false, Color32::BLACK, 64.0);
    assert!(overlay.backdrop.is_some());
    Ok(())
}
