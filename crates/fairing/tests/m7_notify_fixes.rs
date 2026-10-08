//! Regression tests for the heads-up banner, toasts and the notification model (fixes before
//! 0.1.0).

use fairing::access::Subject;
use fairing::testing::{access_config, single_level_access, test_shell, Harness};
use fairing::{LaunchAction, Level, Notification, NotificationId, Services, Toast};
use std::time::Duration;

fn missing(what: &str) -> fairing::Error {
    fairing::Error::Config(format!("could not find {what}"))
}

/// Every string drawn this frame.
fn drawn_text(h: &mut Harness) -> String {
    fn walk(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                out.push_str(text.galley.text());
                out.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = String::new();
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, &mut out);
    }
    out
}

fn animated() -> fairing::Result<Harness> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    Ok(h)
}

/// A gated notification the session fails is redacted in the heads-up banner, as in the shade.
#[test]
fn gated_notification_is_redacted_in_the_heads_up() -> fairing::Result<()> {
    let mut cfg = access_config(&["viewer", "admin"], Some("bottom"));
    cfg.access
        .gates
        .insert("secret".to_owned(), "admin".to_owned());
    let mut h = test_shell(cfg, |_| {})?;
    h.frames(2);
    h.shell.notify(
        Notification::new(NotificationId::of("secret"), "TopSecretTitle")
            .body("TopSecretBody")
            .gate("secret"),
    );
    h.frames(2);
    assert_eq!(
        h.shell.heads_up().visible(),
        Some(NotificationId::of("secret")),
        "precondition: the banner is up"
    );
    let text = drawn_text(&mut h);
    assert!(
        !text.contains("TopSecretTitle") && !text.contains("TopSecretBody"),
        "the heads-up drew gated content:\n{text}"
    );
    assert!(
        text.contains("1 notification"),
        "the redacted line is drawn:\n{text}"
    );
    Ok(())
}

/// A banner raised while the session could read it is redacted once the panel locks over it.
#[test]
fn heads_up_is_redacted_when_the_lock_screen_comes_up() -> fairing::Result<()> {
    let mut cfg = access_config(&["viewer", "operator", "maintainer"], Some("top"));
    cfg.access.pin_table.pins = [("maintainer".to_owned(), "9876".to_owned())]
        .into_iter()
        .collect();
    cfg.access
        .gates
        .insert("secret".to_owned(), "maintainer".to_owned());
    let mut h = test_shell(cfg, |_| {})?;
    h.frames(2);
    h.shell.handle().set_subject(Subject {
        level: Level(2),
        ..Subject::default()
    });
    h.frames(2);
    h.shell.notify(
        Notification::new(NotificationId::of("secret"), "TopSecretTitle")
            .body("TopSecretBody")
            .gate("secret"),
    );
    h.frames(2);
    let text = drawn_text(&mut h);
    assert!(
        text.contains("TopSecretTitle"),
        "precondition: the maintainer reads it:\n{text}"
    );
    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    assert!(h.shell.lock_screen_visible(), "precondition: locked");
    assert_eq!(
        h.shell.heads_up().visible(),
        Some(NotificationId::of("secret")),
        "precondition: the banner is still up"
    );
    let text = drawn_text(&mut h);
    assert!(
        !text.contains("TopSecretTitle") && !text.contains("TopSecretBody"),
        "the banner over the lock screen drew gated content:\n{text}"
    );
    Ok(())
}

/// A banner up for a notification that is then updated in place shows the new content.
#[test]
fn heads_up_shows_the_updated_content() -> fairing::Result<()> {
    let mut h = animated()?;
    let id = NotificationId::of("job");
    h.shell
        .notify(Notification::new(id, "Starting").body("zero"));
    h.run_for(0.3);
    h.shell
        .notify(Notification::new(id, "Running").body("half"));
    h.frames(2);
    assert_eq!(
        h.shell.heads_up().visible(),
        Some(id),
        "precondition: banner up"
    );
    let text = drawn_text(&mut h);
    assert!(
        text.contains("Running") && !text.contains("Starting"),
        "the banner shows stale content while the centre holds the update:\n{text}"
    );
    Ok(())
}

/// A tapped banner the content made taller than `heads_up_height` slides all the way off.
#[test]
fn tapped_tall_heads_up_slides_fully_away() -> fairing::Result<()> {
    let mut h = animated()?;
    let body = "line one of a long body that wraps and wraps across the banner width, \
                line two of a long body that wraps and wraps across the banner width, \
                line three of a long body that wraps and wraps across the banner width";
    h.shell
        .notify(Notification::new(NotificationId::of("tall"), "Tall").body(body));
    h.run_for(0.5);
    let base = h.shell.heads_up().height();
    let rect = h
        .shell
        .heads_up()
        .rect()
        .ok_or_else(|| missing("banner rect"))?;
    assert!(
        rect.height() > base + 10.0,
        "precondition: grew ({} vs {base})",
        rect.height()
    );
    let rest_top = rect.min.y;
    h.press(rect.center());
    h.frame();
    h.release(rect.center());
    let mut last = rect;
    for _ in 0..60 {
        h.frame();
        match h.shell.heads_up().rect() {
            Some(r) => last = r,
            None => break,
        }
    }
    assert_eq!(h.shell.heads_up().visible(), None, "precondition: gone");
    assert!(
        last.max.y <= rest_top + 0.5,
        "the last frame of the exit still showed {:.1} px of the banner below its resting top {rest_top}",
        last.max.y - rest_top
    );
    Ok(())
}

/// A toast with `Duration::MAX` ("until tapped") neither panics the frame nor leaves by itself.
#[test]
fn toast_with_huge_duration_does_not_panic() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(2);
    h.shell.toast(Toast::new("sticky").duration(Duration::MAX));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.frames(3)));
    assert!(r.is_ok(), "a toast with Duration::MAX panicked the frame");
    h.run_for(30.0);
    assert_eq!(h.shell.toasts().visible().len(), 1, "it holds until tapped");
    Ok(())
}

/// The same through the handle and through a renewal (the same words pushed again).
#[test]
fn renewed_toast_with_huge_duration_does_not_panic() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(2);
    let handle = h.shell.handle();
    handle.toast(Toast::new("pinned").duration(Duration::MAX));
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        h.frames(3);
        handle.toast(Toast::new("pinned").duration(Duration::MAX));
        h.frames(3);
    }));
    assert!(
        ran.is_ok(),
        "renewing a Duration::MAX toast panicked the frame"
    );
    assert_eq!(h.shell.toasts().visible().len(), 1);
    Ok(())
}

/// `Notification::progress(NaN)` stays inside the documented 0..=1.
#[test]
fn nan_progress_is_kept_in_range() -> fairing::Result<()> {
    let total = 0.0_f32;
    let n = Notification::new(NotificationId::of("job"), "Job").progress(0.0 / total);
    let p = n.progress.ok_or_else(|| missing("progress"))?;
    assert!(
        (0.0..=1.0).contains(&p),
        "progress(NaN) stored {p}, outside the documented 0..=1"
    );
    Ok(())
}
