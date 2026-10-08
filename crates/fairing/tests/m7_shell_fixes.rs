//! Regression tests for the shell's frame loop: what wakes an idle panel and what does not, the
//! command cap, the font guard, the theme toggle and the wall clock (fixes before 0.1.0).

use fairing::services::null::NullClock;
use fairing::services::Backend;
use fairing::settings::SettingValue;
use fairing::testing::{single_level_access, test_shell, Harness, FRAME_DT, FRAME_DT_F32};
use fairing::theme::{MotionTokens, Palette};
use fairing::time::{ClockFormat, WallTime};
use fairing::{
    screen_with, ColorRole, Cx, LaunchAction, Lifecycle, Screen, Services, Shell, ShellEvent, Theme,
};
use std::cell::RefCell;
use std::time::{Duration, Instant};

thread_local! {
    static LOGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// A logger that keeps what the shell logs on this thread.
struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        LOGS.with(|l| l.borrow_mut().push(format!("{}", record.args())));
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture;

fn capture_logs() {
    let _ = log::set_logger(&CAPTURE);
    log::set_max_level(log::LevelFilter::Warn);
    clear_logs();
}

fn clear_logs() {
    LOGS.with(|l| l.borrow_mut().clear());
}

/// How many lines logged on this thread contain `needle`.
fn logged(needle: &str) -> usize {
    LOGS.with(|l| l.borrow().iter().filter(|m| m.contains(needle)).count())
}

/// Run one idle frame by hand (no queued input) and hand back egui's repaint delay for it —
/// what a runner would wait before the next frame. Keeps the harness clock in step.
fn frame_delay(h: &mut Harness) -> Duration {
    let input = egui::RawInput {
        screen_rect: Some(h.screen_rect()),
        time: Some(h.time()),
        predicted_dt: FRAME_DT_F32,
        ..Default::default()
    };
    let ctx = h.ctx.clone();
    let shell = &mut h.shell;
    let out = ctx.run_ui(input, |ui| shell.frame(ui));
    h.sleep(FRAME_DT);
    let delay = out
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map_or(Duration::MAX, |v| v.repaint_delay);
    out.drop_without_applying_deltas();
    delay
}

/// The delay after egui's own two-frame rule has settled.
fn settled_delay(h: &mut Harness) -> Duration {
    for _ in 0..3 {
        let _ = frame_delay(h);
    }
    frame_delay(h)
}

fn reduced_motion() -> MotionTokens {
    MotionTokens::from_config(&fairing::config::MotionConfig {
        reduce: true,
        ..fairing::config::MotionConfig::default()
    })
}

/// Toggling an injected theme (equal sides of the palette pair) still moves egui's `Visuals`.
#[test]
fn injected_theme_toggle_reaches_egui_visuals() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    let injected = Theme {
        motion: reduced_motion(),
        ..Theme::light()
    };
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .theme(injected)
            .services(Services::null())
            .build(ctx)
    })?;
    h.frames(3);
    assert_eq!(h.ctx.theme(), egui::Theme::Light, "precondition: light");
    h.shell.set_theme_dark(true);
    h.frames(5);
    assert!(h.shell.theme().dark, "the shell's flag flipped");
    assert_eq!(
        h.ctx.theme(),
        egui::Theme::Dark,
        "egui's Visuals did not follow the dark flag"
    );
    Ok(())
}

/// A `[theme.palette]` override survives `set_theme_dark` both ways (guide 08 FAQ Q5).
#[test]
fn theme_switch_keeps_a_custom_palette() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    config
        .theme
        .palette
        .insert("primary".to_owned(), "#ff7a00".to_owned());
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config).services(Services::null()).build(ctx)
    })?;
    h.frames(2);
    let custom = h.shell.theme().color(ColorRole::Primary);
    assert_ne!(custom, Palette::dark().primary, "precondition: overridden");
    for dark in [false, true] {
        h.shell.set_theme_dark(dark);
        h.frames(3);
        assert_eq!(h.shell.theme().dark, dark, "precondition: switched");
        assert_eq!(
            h.shell.theme().color(ColorRole::Primary),
            custom,
            "the custom primary was lost on switching to dark = {dark}"
        );
    }
    Ok(())
}

/// A manual-mode keyboard up and at rest lets the panel idle (no debounce wake every frame).
#[cfg(feature = "osk")]
#[test]
fn manual_osk_at_rest_idles() -> fairing::Result<()> {
    use fairing::screen::{ChromePolicy, OskMode};
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            fairing::screen("manual", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("manual");
            })
            .chrome(ChromePolicy {
                osk: OskMode::Manual,
                ..ChromePolicy::default()
            }),
        );
    })?;
    h.shell.launch(LaunchAction::open("manual"));
    h.frames(3);
    h.shell.osk_mut().toggle();
    h.frames(120);
    assert!(
        h.shell.osk().is_shown(),
        "precondition: the manual keyboard is up"
    );
    assert!(!h.shell.is_animating(), "precondition: at rest");
    let delay = settled_delay(&mut h);
    assert!(
        delay > Duration::from_secs(1),
        "a resting manual keyboard kept a short wake scheduled; delay = {delay:?}"
    );
    Ok(())
}

/// A custom backend whose `next_wake` is stuck in the past is named in the log by its type.
#[test]
fn stale_wake_of_a_custom_backend_is_reported() -> fairing::Result<()> {
    struct StuckBackend(Option<Instant>);
    impl Backend for StuckBackend {
        fn poll_at(&mut self, now: Instant) {
            // Remembers the first frame's time and never moves on.
            self.0.get_or_insert(now);
        }
        fn next_wake(&self) -> Option<Instant> {
            self.0
        }
    }
    capture_logs();
    let mut config = single_level_access();
    config.motion.reduce = true;
    let services = Services::builder()
        .clock(NullClock)
        .custom(StuckBackend(None))
        .build();
    let mut h = Harness::new(config, services)?;
    h.frames(300);
    assert!(
        logged("StuckBackend") > 0 && logged("next_wake in the past") > 0,
        "a stuck custom backend was not named in the log"
    );
    Ok(())
}

/// Fonts replaced between `build` and the first frame are bound again (said once in the log),
/// and the panel draws and goes idle instead of spinning blank.
#[test]
fn fonts_replaced_before_first_frame_recover_and_are_logged_once() -> fairing::Result<()> {
    capture_logs();
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.ctx.set_fonts(egui::FontDefinitions::default());
    // What `build` itself logged (the missing bold face, the assumed density) is not the point.
    clear_logs();
    h.frames(120);
    assert!(
        h.shell.frame_no() > 0,
        "the panel never drew its first frame"
    );
    assert_eq!(
        logged("font definitions were replaced"),
        1,
        "the font replacement is said exactly once"
    );
    let delay = settled_delay(&mut h);
    assert!(
        delay > Duration::from_secs(1),
        "the panel kept repainting; delay = {delay:?}"
    );
    Ok(())
}

/// Exactly `MAX_COMMANDS_PER_FRAME` (256) queued commands are not "over the limit".
#[test]
fn exactly_256_commands_are_not_over_the_limit() -> fairing::Result<()> {
    capture_logs();
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(10);
    let handle = h.shell.handle();
    for _ in 0..256 {
        handle.set_badge("nothing-here", None);
    }
    clear_logs();
    let _ = frame_delay(&mut h); // the wake's own immediate frame
    let second = frame_delay(&mut h);
    assert_eq!(
        logged("went over the per-frame limit"),
        0,
        "nothing was left in the queue, yet the cap warning fired"
    );
    assert!(
        second > Duration::from_secs(1),
        "nothing was left, so no immediate repaint is owed; delay = {second:?}"
    );
    Ok(())
}

/// 257 queued commands still run the one past the cap on the next frame, with the warning.
#[test]
fn commands_past_the_cap_run_next_frame() -> fairing::Result<()> {
    capture_logs();
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(10);
    let handle = h.shell.handle();
    for _ in 0..256 {
        handle.set_badge("nothing-here", None);
    }
    handle.toast("the last one");
    clear_logs();
    let first = frame_delay(&mut h);
    assert_eq!(
        logged("went over the per-frame limit"),
        1,
        "the cap warning"
    );
    assert!(
        first < Duration::from_millis(50),
        "the rest are owed a frame"
    );
    assert_eq!(h.shell.toasts().visible().len(), 0, "the 257th waits");
    h.frames(3);
    assert_eq!(h.shell.toasts().visible().len(), 1, "the 257th ran");
    Ok(())
}

/// A garbage `utc_secs` with a positive offset neither overflows nor panics `WallTime`.
#[test]
fn walltime_with_huge_utc_secs_does_not_overflow() {
    let t = WallTime {
        utc_secs: u64::MAX,
        offset_min: 540,
    };
    assert_eq!(t.local_secs(), i64::MAX, "saturates");
    let civil = std::panic::catch_unwind(|| t.civil());
    assert!(civil.is_ok(), "WallTime::civil overflowed");
    let formatted = std::panic::catch_unwind(|| t.format(ClockFormat::Hm));
    assert!(formatted.is_ok(), "WallTime::format overflowed");
}

/// A toast waiting behind full slots does not keep the panel rendering every frame.
#[test]
fn waiting_toast_does_not_render_every_frame() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.notify.max_visible = 1;
    let mut h = test_shell(config, |_| {})?;
    h.frames(5);
    h.shell
        .toast(fairing::Toast::new("first").duration(Duration::from_secs(30)));
    h.shell.toast(fairing::Toast::new("second"));
    h.frames(60);
    assert_eq!(h.shell.toasts().visible().len(), 1, "precondition: one up");
    assert_eq!(h.shell.toasts().pending(), 1, "precondition: one waiting");
    assert!(!h.shell.is_animating(), "precondition: nothing moves");
    let delay = settled_delay(&mut h);
    assert!(
        delay > Duration::from_secs(1),
        "the next thing due is the first toast's hold (~29 s away); delay = {delay:?}"
    );
    Ok(())
}

/// A stopped screen's `evict_after` arms a wake, so it is evicted on a panel nobody touches.
#[test]
fn evict_after_fires_on_an_idle_panel() -> fairing::Result<()> {
    struct Big;
    impl Screen for Big {
        fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
            ui.label("big");
        }
    }
    let mut config = single_level_access();
    config.status_bar.enabled = false;
    let mut h = test_shell(config, |sh| {
        sh.add(screen_with("big", || Big).evict_after(Duration::from_secs(2)));
    })?;
    h.shell.launch(LaunchAction::open("big"));
    h.frames(10);
    h.shell.home();
    h.frames(10);
    assert!(
        h.shell.workspace().find("big").is_some(),
        "precondition: stopped, not yet evicted"
    );
    let delay = settled_delay(&mut h);
    assert!(
        delay <= Duration::from_secs(3),
        "an eviction is due in ~2 s but the next frame was scheduled in {delay:?}"
    );
    Ok(())
}

/// A request a screen makes from `on_lifecycle(Destroyed)` on eviction is carried out on the
/// same frame, not stranded until the next input.
#[test]
fn request_from_destroyed_on_eviction_is_not_stranded() -> fairing::Result<()> {
    struct Saver;
    impl Screen for Saver {
        fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
            ui.label("saver");
        }
        fn on_lifecycle(&mut self, ev: Lifecycle, cx: &mut Cx<'_>) {
            if ev == Lifecycle::Destroyed {
                cx.set_setting("app.saved", SettingValue::Bool(true));
            }
        }
    }
    let mut config = single_level_access();
    config.status_bar.enabled = false;
    let mut h = test_shell(config, |sh| {
        sh.add(screen_with("saver", || Saver).evict_after(Duration::from_secs(1)));
    })?;
    h.shell.launch(LaunchAction::open("saver"));
    h.frames(10);
    h.shell.home();
    h.frames(10);
    let _ = h.shell.poll_events();
    let _ = settled_delay(&mut h);
    // One frame after the eviction is due, as a single wake gives.
    h.sleep(5.0);
    let _ = frame_delay(&mut h);
    let events = h.shell.poll_events();
    let evicted = events
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenClosed { id, .. } if id == "saver"));
    let saved = events.iter().any(
        |e| matches!(e, ShellEvent::SettingChanged { key, .. } if key.0.as_ref() == "app.saved"),
    );
    assert!(evicted, "precondition: the eviction ran on the woken frame");
    assert!(saved, "the Destroyed handler's request was not carried out");
    Ok(())
}
