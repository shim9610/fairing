//! Regression tests for the status bar: its crossfades asking for frames and the measuring of
//! integrator items (fixes before 0.1.0).

use fairing::services::mock::{AudioMsg, MockAudio};
use fairing::services::AudioSnapshot;
use fairing::testing::{single_level_access, test_shell, Harness};
use fairing::{status_item, Cx, Services, Slot};

/// The volume icon's crossfade keeps the shell animating (and so asking for frames).
#[test]
fn volume_crossfade_keeps_the_shell_animating() -> fairing::Result<()> {
    let audio = MockAudio::new(10);
    let control = audio.control();
    let services = Services::builder()
        .clock(fairing::services::null::NullClock)
        .audio(audio)
        .build();
    let mut cfg = single_level_access();
    cfg.status_bar.right = vec!["status.volume".to_owned()];
    let mut h = Harness::new(cfg, services)?;
    h.frames(10);
    assert!(
        h.shell.status_bar().item_rect("status.volume").is_some(),
        "precondition: volume drawn"
    );
    assert!(!h.shell.is_animating(), "precondition: idle");
    control.send(AudioMsg::Volume(AudioSnapshot {
        level: 90,
        muted: false,
    }));
    h.frames(2);
    assert!(
        h.shell.is_animating(),
        "the volume icon is mid-crossfade but the shell reports nothing animating"
    );
    Ok(())
}

fn add_tiny(shell: &mut fairing::Shell) {
    shell.add(status_item(
        "tiny",
        Slot::Right,
        |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            ui.label("x");
        },
    ));
}

/// A small integrator item that collapses on the 72 px estimate is measured and comes back where
/// its real width fits (guide 03 §1.4).
#[test]
fn small_custom_status_item_is_measured_after_a_collapse() -> fairing::Result<()> {
    // Measure everything on a wide bar first.
    let mut wide = test_shell(single_level_access(), add_tiny)?;
    wide.frames(4);
    let sb = wide.shell.status_bar();
    let ids = [
        "status.clock",
        "status.user",
        "status.notifications",
        "tiny",
    ];
    let mut total = 0.0;
    let mut tiny_w = 0.0;
    let mut rects = Vec::new();
    for id in ids {
        if let Some(r) = sb.item_rect(id) {
            total += r.width();
            rects.push(r);
            if id == "tiny" {
                tiny_w = r.width();
            }
        }
    }
    // The gaps between the items as the bar laid them, not the bar's spacing constant copied.
    rects.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    let gaps: f32 = rects
        .iter()
        .zip(rects.iter().skip(1))
        .map(|(a, b)| b.min.x - a.max.x)
        .sum();
    assert!(
        tiny_w > 0.0 && tiny_w < 40.0,
        "precondition: tiny is small ({tiny_w})"
    );
    let pad = wide.shell.theme().metrics.status_edge_pad;
    let needed = total + gaps + 2.0 * pad;
    // Room for its real width plus a margin, but not for the 72 px guess.
    let width = needed + (72.0 - tiny_w) * 0.5;
    let mut cfg = single_level_access();
    cfg.motion.reduce = true;
    let mut h = Harness::new(cfg, Services::null())?.with_size(width, 600.0);
    add_tiny(&mut h.shell);
    h.frames(10);
    assert!(
        h.shell.status_bar().item_rect("tiny").is_some(),
        "bar {width} px wide has room for the {tiny_w} px item (needs {needed}), but it stays collapsed"
    );
    Ok(())
}
