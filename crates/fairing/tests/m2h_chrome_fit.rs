//! **What the chrome says has to fit inside what it draws** — the notification row, the heads-up
//! banner and the toast stack, sized by their words.

use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx, LaunchAction, Notification, NotificationId, Shell};

fn shell() -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("s", |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            ui.label("page");
        }));
        shell.launch(LaunchAction::open("s"));
        Ok(shell)
    })?;
    h.frames(10);
    Ok(h)
}

/// The rect a text shape covers, for the text `wanted`.
fn text_rect(h: &mut Harness, wanted: &str) -> Option<egui::Rect> {
    for c in h.frame_shapes() {
        if let egui::Shape::Text(t) = c.shape {
            if t.galley.job.text == wanted {
                return Some(t.galley.rect.translate(t.pos.to_vec2()));
            }
        }
    }
    None
}

/// The smallest filled rect on the frame that holds `inner` whole — the card behind a text.
fn card_around(h: &mut Harness, inner: egui::Rect) -> Option<egui::Rect> {
    let mut best: Option<egui::Rect> = None;
    for c in h.frame_shapes() {
        if let egui::Shape::Rect(r) = c.shape {
            if r.fill.a() > 0
                && r.rect.contains_rect(inner)
                && best.is_none_or(|b| r.rect.area() < b.area())
            {
                best = Some(r.rect);
            }
        }
    }
    best
}

/// **A notification row is as tall as its two lines**: the body sits inside the card, not on
/// its bottom edge.
#[test]
fn a_two_line_notification_row_holds_its_body() -> fairing::Result<()> {
    let mut h = shell()?;
    h.shell.notify(
        Notification::new(NotificationId::of("s3"), "Sensor 3 offline").body("check wiring"),
    );
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(60);
    let title = text_rect(&mut h, "Sensor 3 offline");
    let body = text_rect(&mut h, "check wiring");
    let (Some(title), Some(body)) = (title, body) else {
        return Err(fairing::Error::Runner(format!(
            "the row's text is not on the frame: {title:?} {body:?}"
        )));
    };
    let card = card_around(&mut h, title).unwrap_or(egui::Rect::NOTHING);
    assert!(card.width() > 0.0, "no card behind the title at {title:?}");
    assert!(
        card.max.y >= body.max.y - 0.5,
        "the body ends at {} and the card at {}: the second line runs out of the card",
        body.max.y,
        card.max.y
    );
    Ok(())
}

/// **The heads-up banner grows to its body**, rather than drawing the second line out of its
/// bottom edge.
#[test]
fn a_two_line_heads_up_grows_to_its_body() -> fairing::Result<()> {
    let mut h = shell()?;
    h.shell.notify(
        Notification::new(NotificationId::of("job"), "Job finished").body("3 files exported"),
    );
    h.frames(60);
    let title = text_rect(&mut h, "Job finished");
    let body = text_rect(&mut h, "3 files exported");
    let (Some(title), Some(body)) = (title, body) else {
        return Err(fairing::Error::Runner(format!(
            "the banner's text is not on the frame: {title:?} {body:?}"
        )));
    };
    let banner = card_around(&mut h, title).unwrap_or(egui::Rect::NOTHING);
    assert!(banner.width() > 0.0, "no banner behind the title");
    assert!(
        banner.max.y >= body.max.y - 0.5,
        "the body ends at {} and the banner at {}: the second line runs out of the banner",
        body.max.y,
        banner.max.y
    );
    Ok(())
}

/// **The same toast twice shows once.**
#[test]
fn the_same_toast_twice_shows_once() -> fairing::Result<()> {
    let mut h = shell()?;
    h.shell.toast("Saved");
    h.shell.toast("Saved");
    h.frames(30);
    h.shell.toast("Saved");
    h.frames(30);
    let mut count = 0;
    for c in h.frame_shapes() {
        if let egui::Shape::Text(t) = c.shape {
            if t.galley.job.text == "Saved" {
                count += 1;
            }
        }
    }
    assert_eq!(count, 1, "\"Saved\" is on the frame {count} times");
    Ok(())
}
