//! **The split shade** — `[overlay] layout = "split"`.
//!
//! A wide screen does not pull one shade from anywhere along its top: the left of the edge opens
//! the notifications and the right the controls, each a panel of its own filling the height.
//! The panel is picked once, by where the pull (or the status bar tap) began, and does not
//! change on the way down. Each panel holds only its own; while one is open, a pull or a
//! bar tap on the other side brings the other in, as a first open would, while what is left of the
//! first dissolves. A split shade has no stop on the way down — each panel already does one thing.

#![cfg(feature = "overlay")]

use fairing::notify::{Notification, NotificationId};
use fairing::overlay::OverlayPanel;
use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx, Shell};

/// A shell with a split shade, drawn as `reveal`, on a screen of `size`.
fn shell(
    reveal: &str,
    size: (f32, f32),
    tune: impl FnOnce(&mut fairing::ShellConfig),
) -> fairing::Result<Harness> {
    let mut config = single_level_access();
    "split".clone_into(&mut config.overlay.layout);
    reveal.clone_into(&mut config.overlay.reveal);
    tune(&mut config);
    Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(config).build(ctx)?;
        shell.add(screen("s", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            fairing::layout::page(ui, cx, "s", |ui, cx| {
                fairing::layout::note(ui, cx, "body");
            });
        }));
        shell.launch(fairing::LaunchAction::open("s"));
        Ok(shell)
    })
    .map(|h| {
        let mut h = h.with_size(size.0, size.1);
        // Past the launch transition, which holds taps off while it runs.
        h.frames(40);
        h
    })
}

fn need<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| fairing::Error::Config(format!("{what} is missing")))
}

/// Pull from the top edge at `x` down to `to_y`, hold still, let go and settle.
fn pull_at(h: &mut Harness, x: f32, to_y: f32) {
    h.press(egui::pos2(x, 2.0));
    h.frames(1);
    for i in 1..=10 {
        #[expect(clippy::cast_precision_loss, reason = "ten steps")]
        let t = i as f32 / 10.0;
        h.move_to(egui::pos2(x, 2.0 + (to_y - 2.0) * t));
        h.frames(1);
    }
    for _ in 0..6 {
        h.move_to(egui::pos2(x, to_y));
        h.frames(1);
    }
    h.release(egui::pos2(x, to_y));
    h.frames(40);
}

/// Shut it with a tap low on the screen, away from any panel.
fn shut(h: &mut Harness) {
    let screen = h.screen_rect();
    let panel = h
        .shell
        .overlay()
        .frame()
        .panel
        .unwrap_or(egui::Rect::NOTHING);
    let x = if panel.center().x < screen.center().x {
        screen.max.x - 24.0
    } else {
        screen.min.x + 24.0
    };
    h.tap(egui::pos2(x, screen.max.y - 24.0));
    h.frames(40);
}

/// A point on the status bar at `share` of its width.
fn bar_at(h: &Harness, share: f32) -> fairing::Result<egui::Pos2> {
    let status = need(h.shell.layout().status, "the status bar")?;
    Ok(egui::pos2(
        status.min.x + status.width() * share,
        status.center().y,
    ))
}

/// The words drawn this frame, joined — to see what a panel holds.
fn words(h: &mut Harness) -> String {
    let mut out = String::new();
    for c in h.frame_shapes() {
        if let egui::Shape::Text(t) = &c.shape {
            out.push_str(t.galley.text());
            out.push(' ');
        }
    }
    out
}

/// **The left of the edge opens the notifications, the right the controls** — and each draws only
/// its own half: the list and no tiles, or the tiles and no list.
#[test]
fn a_left_pull_opens_the_notifications_and_a_right_pull_the_controls() -> fairing::Result<()> {
    let mut h = shell("curtain", (1280.0, 800.0), |_| {})?;
    let screen = h.screen_rect();
    assert_eq!(
        h.shell.overlay().panels(),
        [OverlayPanel::Notifications, OverlayPanel::ControlCenter]
    );
    pull_at(&mut h, screen.width() * 0.25, screen.height() * 0.8);
    let o = h.shell.overlay();
    assert!(o.is_open(), "a pull from the left opens the shade");
    assert_eq!(o.panel(), OverlayPanel::Notifications);
    assert!(o.list_rect().is_some(), "the notifications draw their list");
    assert!(
        o.tile_rect("tile.wifi").is_none(),
        "and no tiles: {:?}",
        o.tile_rect("tile.wifi")
    );
    shut(&mut h);
    assert!(
        h.shell.overlay().is_closed(),
        "a tap below the panel shuts it"
    );
    pull_at(&mut h, screen.width() * 0.75, screen.height() * 0.8);
    let o = h.shell.overlay();
    assert!(o.is_open(), "a pull from the right opens the shade");
    assert_eq!(o.panel(), OverlayPanel::ControlCenter);
    assert!(
        o.tile_rect("tile.wifi").is_some(),
        "the controls draw the tiles"
    );
    assert!(o.list_rect().is_none(), "and no list");
    Ok(())
}

/// **As a card, each panel rests on its own side** — the notifications on the left, the controls
/// on the right — and it is the panel that decides, not which half of the screen the finger was
/// in: with the divide moved to 0.7, a pull just right of the middle still opens the
/// notifications, on the left.
#[test]
fn a_split_card_rests_on_its_panels_side() -> fairing::Result<()> {
    let mut h = shell("card", (1280.0, 800.0), |_| {})?;
    let screen = h.screen_rect();
    pull_at(&mut h, screen.width() * 0.3, screen.height() * 0.8);
    let notes = need(h.shell.overlay().frame().panel, "the notifications card")?;
    assert_eq!(h.shell.overlay().panel(), OverlayPanel::Notifications);
    assert!(
        notes.min.x < screen.width() * 0.1 && notes.center().x < screen.center().x,
        "the notifications rest on the left: {notes:?}"
    );
    shut(&mut h);
    pull_at(&mut h, screen.width() * 0.8, screen.height() * 0.8);
    let controls = need(h.shell.overlay().frame().panel, "the controls card")?;
    assert_eq!(h.shell.overlay().panel(), OverlayPanel::ControlCenter);
    assert!(
        controls.max.x > screen.width() * 0.9 && controls.center().x > screen.center().x,
        "the controls rest on the right: {controls:?}"
    );

    let mut h = shell("card", (1280.0, 800.0), |c| c.overlay.split_ratio = 0.7)?;
    pull_at(&mut h, screen.width() * 0.6, screen.height() * 0.8);
    assert_eq!(
        h.shell.overlay().panel(),
        OverlayPanel::Notifications,
        "left of a 0.7 divide is the notifications"
    );
    let card = need(h.shell.overlay().frame().panel, "the card")?;
    assert!(
        card.min.x < screen.width() * 0.1 && card.center().x < screen.center().x,
        "and they rest on the left whatever half the finger was in: {card:?}"
    );
    Ok(())
}

/// **A split panel fills the height** — from under the status bar down to the bottom of
/// the content area, a card keeping its inset above it: each half of a wide screen is a column of
/// its own, and a panel stopping where its content did read as a box floating in it. Both panels
/// are the same height, however little either holds.
#[test]
fn split_panels_fill_the_height() -> fairing::Result<()> {
    let mut h = shell("card", (1280.0, 800.0), |_| {})?;
    let screen = h.screen_rect();
    let content = h.shell.layout().content;
    pull_at(&mut h, screen.width() * 0.8, screen.height() * 0.8);
    let controls = need(h.shell.overlay().frame().panel, "the controls card")?;
    let inset = controls.min.x.min(screen.max.x - controls.max.x);
    assert!(
        (controls.max.y - (content.max.y - inset)).abs() < 1.5,
        "the controls run down to the content's bottom less their inset: bottom {} against {} - {inset}",
        controls.max.y,
        content.max.y
    );
    assert!(
        controls.min.y < content.min.y + inset * 2.0,
        "from just under the status bar: top {} against {}",
        controls.min.y,
        content.min.y
    );
    shut(&mut h);
    pull_at(&mut h, screen.width() * 0.2, screen.height() * 0.8);
    let notes = need(h.shell.overlay().frame().panel, "the notifications card")?;
    assert!(
        (notes.height() - controls.height()).abs() < 1.0,
        "the empty notifications are as tall as the controls: {} against {}",
        notes.height(),
        controls.height()
    );

    // A curtain fills it too: the whole content area, past the unified shade's limit.
    let mut h = shell("curtain", (1280.0, 800.0), |_| {})?;
    pull_at(&mut h, screen.width() * 0.8, screen.height() * 0.95);
    let o = h.shell.overlay();
    assert!(o.is_open());
    assert!(
        (o.height() - content.height()).abs() < 1.0,
        "a split curtain is the content area's height: {} against {}",
        o.height(),
        content.height()
    );
    Ok(())
}

/// **Each panel holds only its own**: the notifications nothing of the controls and the
/// controls nothing of the notifications — no header naming the other panel, no tiles under the
/// list, no list under the tiles.
#[test]
fn a_split_panel_holds_only_its_own() -> fairing::Result<()> {
    let mut h = shell("card", (1280.0, 800.0), |_| {})?;
    let screen = h.screen_rect();
    for i in 0..2 {
        h.shell.notify(Notification::new(
            NotificationId::of(&format!("n{i}")),
            format!("Notice {i}"),
        ));
    }
    h.frames(2);
    pull_at(&mut h, screen.width() * 0.2, screen.height() * 0.8);
    let notes = words(&mut h);
    assert!(
        notes.contains("Notice 0"),
        "the notifications show: {notes}"
    );
    assert!(
        !notes.contains("Controls") && !notes.contains("Wi-Fi"),
        "and nothing of the controls: {notes}"
    );
    shut(&mut h);
    pull_at(&mut h, screen.width() * 0.8, screen.height() * 0.8);
    let controls = words(&mut h);
    assert!(controls.contains("Wi-Fi"), "the controls show: {controls}");
    assert!(
        !controls.contains("Notice 0") && !controls.contains("Notifications"),
        "and nothing of the notifications: {controls}"
    );
    Ok(())
}

/// **A pull on the other side crosses to its panel**: with the notifications open, a
/// pull from the right of the top edge brings the controls in under the finger, as a first open
/// would, while what is left of the notifications dissolves where it stood.
#[test]
fn pulling_the_other_side_crosses_to_its_panel() -> fairing::Result<()> {
    let mut h = shell("card", (1280.0, 800.0), |_| {})?;
    let screen = h.screen_rect();
    pull_at(&mut h, screen.width() * 0.2, screen.height() * 0.8);
    assert_eq!(h.shell.overlay().panel(), OverlayPanel::Notifications);
    let left = need(h.shell.overlay().frame().panel, "the notifications card")?;
    // A pull from the right, held part-way.
    let x = screen.width() * 0.8;
    h.press(egui::pos2(x, 2.0));
    h.frames(1);
    for i in 1..=4 {
        #[expect(clippy::cast_precision_loss, reason = "four steps")]
        let y = 2.0 + 30.0 * i as f32;
        h.move_to(egui::pos2(x, y));
        h.frames(1);
    }
    assert_eq!(
        h.shell.overlay().panel(),
        OverlayPanel::ControlCenter,
        "the pull on the right crossed to the controls"
    );
    let (from, t) = need(h.shell.overlay().crossing(), "the crossing")?;
    assert_eq!(from, OverlayPanel::Notifications);
    assert!(t < 1.0, "the notifications are still going: {t}");
    let shapes = h.frame_shapes();
    let right = need(h.shell.overlay().frame().panel, "the controls card")?;
    assert!(
        right.max.x > screen.width() * 0.9 && right.center().x > screen.center().x,
        "the controls come in on the right: {right:?}"
    );
    let fading_on = |r: egui::Rect| {
        shapes.iter().any(|c| match &c.shape {
            egui::Shape::Rect(s) => {
                s.blur_width == 0.0
                    && s.fill.a() > 0
                    && s.fill.a() < 255
                    && (s.rect.min.x - r.min.x).abs() < 1.0
                    && (s.rect.max.x - r.max.x).abs() < 1.0
            }
            _ => false,
        })
    };
    assert!(
        fading_on(left),
        "the notifications card is dissolving on the left"
    );
    assert!(
        fading_on(right),
        "the controls card is arriving on the right"
    );
    // Let go well down: it opens, and the crossing is over.
    h.move_to(egui::pos2(x, screen.height() * 0.8));
    h.frames(6);
    h.release(egui::pos2(x, screen.height() * 0.8));
    h.frames(40);
    assert!(h.shell.overlay().is_open());
    assert_eq!(h.shell.overlay().panel(), OverlayPanel::ControlCenter);
    assert!(
        h.shell.overlay().crossing().is_none(),
        "the dissolve is over"
    );
    assert!(h.shell.overlay().tile_rect("tile.wifi").is_some());
    Ok(())
}

/// **A status bar tap on the other side crosses; on its own side it shuts** — the tap
/// is the non-gesture way across, now that the panels carry no header naming each other.
#[test]
fn a_bar_tap_on_the_other_side_crosses_and_on_its_own_side_shuts() -> fairing::Result<()> {
    let mut h = shell("curtain", (1280.0, 800.0), |_| {})?;
    h.tap(bar_at(&h, 0.35)?);
    h.frames(40);
    assert!(h.shell.overlay().is_open());
    assert_eq!(h.shell.overlay().panel(), OverlayPanel::Notifications);
    h.tap(bar_at(&h, 0.65)?);
    h.frames(2);
    assert!(
        !h.shell.overlay().is_closed(),
        "a tap on the other side does not shut it"
    );
    assert_eq!(
        h.shell.overlay().panel(),
        OverlayPanel::ControlCenter,
        "it crosses to the controls"
    );
    h.frames(40);
    assert!(h.shell.overlay().is_open(), "and they open");
    h.tap(bar_at(&h, 0.65)?);
    h.frames(40);
    assert!(
        h.shell.overlay().is_closed(),
        "a tap on the open panel's own side shuts it"
    );
    Ok(())
}

/// **A split shade has no stop on the way down**: with `two_step` on as well, a
/// pull still opens the panel the whole way.
#[test]
fn split_has_no_stop_on_the_way_down() -> fairing::Result<()> {
    let mut h = shell("curtain", (1280.0, 800.0), |c| c.overlay.two_step = true)?;
    let screen = h.screen_rect();
    for i in 0..6 {
        h.shell.notify(Notification::new(
            NotificationId::of(&format!("n{i}")),
            format!("Notice {i}"),
        ));
    }
    h.frames(2);
    pull_at(&mut h, screen.width() * 0.2, screen.height() * 0.6);
    let o = h.shell.overlay();
    assert!(o.is_open(), "it opened");
    assert!(
        (o.y() - o.height()).abs() < 1.0,
        "the whole way, not to a stop: y {} of {}",
        o.y(),
        o.height()
    );
    Ok(())
}

/// **A status bar tap opens the panel on its side** — the tap is the non-gesture way in, and
/// where it lands picks the panel as a pull's start would.
#[test]
fn a_status_bar_tap_opens_the_panel_on_its_side() -> fairing::Result<()> {
    let mut h = shell("curtain", (1280.0, 800.0), |_| {})?;
    let status = need(h.shell.layout().status, "the status bar")?;
    h.tap(egui::pos2(
        status.min.x + status.width() * 0.35,
        status.center().y,
    ));
    h.frames(40);
    assert!(
        h.shell.overlay().is_open(),
        "a tap on the bar opens the shade"
    );
    assert_eq!(h.shell.overlay().panel(), OverlayPanel::Notifications);
    h.shell.handle().toggle_overlay();
    h.frames(40);
    assert!(h.shell.overlay().is_closed());
    h.tap(egui::pos2(
        status.min.x + status.width() * 0.65,
        status.center().y,
    ));
    h.frames(40);
    assert!(h.shell.overlay().is_open());
    assert_eq!(h.shell.overlay().panel(), OverlayPanel::ControlCenter);
    Ok(())
}

/// **A crossing never draws a card taller than it rests.** When the panels were as tall as what
/// they held, a panel crossed to for the first time came up at the limit for the frame that
/// measured it — as tall as the screen for a frame in the console. The panels now share one height,
/// and this keeps any frame of a crossing (here by a tap on the bar's other side)
/// from overshooting it.
#[test]
fn a_crossing_never_draws_a_card_taller_than_it_rests() -> fairing::Result<()> {
    let mut h = shell("card", (1280.0, 800.0), |_| {})?;
    let screen = h.screen_rect();
    pull_at(&mut h, screen.width() * 0.2, screen.height() * 0.8);
    let bar = bar_at(&h, 0.8)?;
    // A tap on the bar's other side, pressed and let go by hand: `tap` runs a frame past the
    // release, and that frame is the first the controls are drawn in.
    h.press(bar);
    h.frame();
    h.release(bar);
    let mut seen = Vec::new();
    for _ in 0..20 {
        let shapes = h.frame_shapes();
        for c in &shapes {
            if let egui::Shape::Rect(r) = &c.shape {
                // A plate on the right half: the controls card coming up.
                if r.blur_width == 0.0
                    && r.fill.a() > 0
                    && r.rect.min.x > screen.center().x - 80.0
                    && r.rect.max.x > screen.width() * 0.9
                    && r.rect.height() > 40.0
                {
                    seen.push(r.rect.height());
                }
            }
        }
    }
    let rest = need(h.shell.overlay().frame().panel, "the controls card")?.height();
    assert!(
        !seen.is_empty(),
        "the controls card was drawn while crossing"
    );
    let tallest = seen.iter().copied().fold(0.0_f32, f32::max);
    assert!(
        tallest <= rest + 1.0,
        "no frame showed the controls taller than they rest: tallest {tallest} against {rest}"
    );
    Ok(())
}

/// **A crossing never blinks.** The card being crossed from is seen in every frame of its
/// dissolve: egui lays a new `Area` out unseen on its first frame, and with the outgoing panel's
/// `Area` made afresh at each crossing, the old card vanished for that frame before it started to
/// fade (user-visible in the console capture).
#[test]
fn a_crossing_never_blinks() -> fairing::Result<()> {
    let mut h = shell("card", (1280.0, 800.0), |_| {})?;
    let screen = h.screen_rect();
    pull_at(&mut h, screen.width() * 0.2, screen.height() * 0.8);
    let bar = bar_at(&h, 0.8)?;
    h.press(bar);
    h.frame();
    h.release(bar);
    // A card's plate on the left: round-cornered and tall — not the status bar's band.
    let old_card = |r: &egui::epaint::RectShape| {
        r.blur_width == 0.0
            && r.fill.a() > 0
            && r.rect.height() > 100.0
            && r.corner_radius.nw > 10
            && r.rect.min.x < screen.width() * 0.1
    };
    let mut checked = 0;
    for frame in 0..20 {
        let shapes = h.frame_shapes();
        let Some((_, t)) = h.shell.overlay().crossing() else {
            continue;
        };
        if t > 0.9 {
            continue;
        }
        checked += 1;
        let shown = shapes.iter().any(|c| match &c.shape {
            egui::Shape::Rect(r) => old_card(r),
            _ => false,
        });
        assert!(
            shown,
            "frame {frame} of the crossing (t {t}) lost the card being crossed from"
        );
    }
    assert!(checked > 0, "the crossing was seen");
    Ok(())
}

/// **A pull whose first move lands with the press still opens**. On a slow frame — the
/// console's software-rendered first frames, a kiosk under load — or a quick flick on a touch
/// digitizer, the finger comes down on the top edge and is past the edge zone before the frame
/// that sees it runs. The shade read that as a press below the edge and let the whole pull go:
/// one console capture run in seven lost its first pull exactly so. Measured from where the
/// finger came down, it opens, on the side it came down on, and the card arrives as from any
/// other pull.
#[test]
fn a_pull_whose_first_move_lands_with_the_press_still_opens() -> fairing::Result<()> {
    let mut h = shell("card", (1280.0, 800.0), |_| {})?;
    h.press(egui::pos2(300.0, 2.0));
    h.move_to(egui::pos2(300.0, 48.0));
    h.frames(1);
    let mut y = 48.0_f32;
    let mut seen = false;
    while y < 650.0 {
        y = (y + 46.0).min(650.0);
        h.move_to(egui::pos2(300.0, y));
        h.frames(1);
        seen |= h.shell.overlay().frame().panel.is_some();
    }
    assert!(seen, "the card showed under the finger on the way down");
    h.release(egui::pos2(300.0, 650.0));
    h.frames(40);
    assert!(h.shell.overlay().is_open(), "and the pull opened the shade");
    assert_eq!(
        h.shell.overlay().panel(),
        OverlayPanel::Notifications,
        "on the side the finger came down on"
    );
    Ok(())
}
