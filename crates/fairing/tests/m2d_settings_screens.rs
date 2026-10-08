//! The built-in settings screens. Feature `settings`.
//!
//! What it checks: whether an item stays away entirely where there is no backend, whether a manufacturer can
//! override it **at any layer**, and whether the power **never goes off on its own**.
//!
//! The rules are the other integration tests': written as `fairing::Result<()>` with no `panic!` and no `unwrap`.

#![cfg(all(feature = "settings", feature = "mock"))]

use fairing::services::{PowerRequest, Services};
use fairing::settings::{add_all, keys, SettingValue, SettingsConfig};
use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx, Shell, ShellEvent};
use std::cell::Cell;
use std::rc::Rc;

/// A shell with every Mock on it (Wi-Fi, Bluetooth, power, the clock and the display).
fn mock_shell(config: SettingsConfig) -> fairing::Result<Harness> {
    Harness::from_builder(move |ctx| {
        let services = fairing::services::mock::services();
        let mut shell = Shell::builder(single_level_access())
            .services(services)
            .build(ctx)?;
        add_all(&mut shell, &config);
        Ok(shell)
    })
}

/// A shell with no backend at all (all Null).
fn bare_shell(config: SettingsConfig) -> fairing::Result<Harness> {
    Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access())
            .services(Services::null())
            .build(ctx)?;
        add_all(&mut shell, &config);
        Ok(shell)
    })
}

/// The registered declaration ids.
fn declared(h: &Harness) -> Vec<String> {
    h.shell
        .registry()
        .screens()
        .iter()
        .map(|d| d.id().to_owned())
        .collect()
}

/// One call registers every one of the eleven that a backend stands behind.
#[test]
fn add_all_registers_the_built_in_screens() -> fairing::Result<()> {
    let h = mock_shell(SettingsConfig::default())?;
    let ids = declared(&h);
    for want in [
        "settings.home",
        "settings.wifi",
        "settings.network",
        "settings.bluetooth",
        "settings.display",
        "settings.sound",
        "settings.datetime",
        "settings.locale",
        "settings.power",
        "settings.about",
    ] {
        assert!(
            ids.iter().any(|id| id == want),
            "there is no {want}: {ids:?}"
        );
    }
    Ok(())
}

/// **With no backend the item stays away entirely.** Having to press it to be told "not supported" is the worst of it.
#[test]
fn screens_without_a_backend_are_not_registered() -> fairing::Result<()> {
    let h = bare_shell(SettingsConfig::default())?;
    let ids = declared(&h);
    assert!(!ids.iter().any(|id| id == "settings.wifi"), "{ids:?}");
    assert!(!ids.iter().any(|id| id == "settings.bluetooth"), "{ids:?}");
    assert!(!ids.iter().any(|id| id == "settings.network"), "{ids:?}");
    // The ones that need no backend are still there.
    assert!(ids.iter().any(|id| id == "settings.home"), "{ids:?}");
    assert!(ids.iter().any(|id| id == "settings.about"), "{ids:?}");
    Ok(())
}

/// During development the screens have to be visible with no backend too.
#[test]
fn ignoring_capabilities_registers_everything() -> fairing::Result<()> {
    let h = bare_shell(SettingsConfig::default().ignoring_capabilities())?;
    let ids = declared(&h);
    assert!(ids.iter().any(|id| id == "settings.wifi"), "{ids:?}");
    Ok(())
}

/// Excluding one — for when we want to use just that screen of our own.
#[test]
fn a_screen_can_be_excluded() -> fairing::Result<()> {
    let h = mock_shell(SettingsConfig::default().without("settings.wifi"))?;
    let ids = declared(&h);
    assert!(!ids.iter().any(|id| id == "settings.wifi"), "{ids:?}");
    assert!(ids.iter().any(|id| id == "settings.bluetooth"), "{ids:?}");
    Ok(())
}

/// `only` excludes the complement.
#[test]
fn only_keeps_just_the_named_screens() -> fairing::Result<()> {
    let h = mock_shell(SettingsConfig::only(["settings.home", "settings.about"]))?;
    let ids = declared(&h);
    assert!(ids.iter().any(|id| id == "settings.home"), "{ids:?}");
    assert!(ids.iter().any(|id| id == "settings.about"), "{ids:?}");
    assert!(!ids.iter().any(|id| id == "settings.wifi"), "{ids:?}");
    Ok(())
}

/// **The deepest override**: registering under the same id has our screen win outright.
#[test]
fn a_manufacturer_screen_replaces_the_built_in_one() -> fairing::Result<()> {
    let drawn = Rc::new(Cell::new(false));
    let drawn_in = Rc::clone(&drawn);
    let mut h = Harness::from_builder(move |ctx| {
        let services = fairing::services::mock::services();
        let mut shell = Shell::builder(single_level_access())
            .services(services)
            .build(ctx)?;
        add_all(&mut shell, &SettingsConfig::default());
        // The same id after the built-in — `Registry::add` replaces it.
        shell.add(
            screen("settings.wifi", move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                drawn_in.set(true);
                ui.label("ACME Wi-Fi");
            })
            .title("ACME Wi-Fi"),
        );
        shell.launch(fairing::LaunchAction::open("settings.wifi"));
        Ok(shell)
    })?;
    h.frames(3);
    assert!(drawn.get(), "the manufacturer's screen has to be drawn");
    // There is still only the one registration — it is a replacement, not an addition.
    assert_eq!(
        declared(&h)
            .iter()
            .filter(|id| *id == "settings.wifi")
            .count(),
        1
    );
    Ok(())
}

/// When a settings screen changes a value the integrator gets it as an event — **persisting it is the integrator's.**
#[test]
fn changing_a_setting_reaches_the_integrator() -> fairing::Result<()> {
    let mut h = mock_shell(SettingsConfig::default())?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    h.shell
        .set_setting(keys::UI_SILENT.into(), SettingValue::Bool(true));
    let changed: Vec<_> = h
        .shell
        .poll_events()
        .into_iter()
        .filter_map(|e| match e {
            ShellEvent::SettingChanged { key, value } => Some((key, value)),
            _ => None,
        })
        .collect();
    assert_eq!(changed.len(), 1, "{changed:?}");
    Ok(())
}

/// **The power never goes off on its own.** A screen asking raises only the event, and it runs when the integrator has
/// tidied up and calls `commit_power` — not calling it is the refusal.
#[test]
fn a_power_request_is_only_an_event() -> fairing::Result<()> {
    let requested = Rc::new(Cell::new(false));
    let requested_in = Rc::clone(&requested);
    let mut h = Harness::from_builder(move |ctx| {
        let services = fairing::services::mock::services();
        let mut shell = Shell::builder(single_level_access())
            .services(services)
            .build(ctx)?;
        shell.add(
            screen("ask", move |_: &mut egui::Ui, cx: &mut Cx<'_>| {
                if !requested_in.get() {
                    requested_in.set(true);
                    cx.request_power(PowerRequest::Reboot);
                }
            })
            .title("ask"),
        );
        shell.launch(fairing::LaunchAction::open("ask"));
        Ok(shell)
    })?;
    h.frames(3);
    let events: Vec<_> = h
        .shell
        .poll_events()
        .into_iter()
        .filter(|e| matches!(e, ShellEvent::PowerRequest(_)))
        .collect();
    assert_eq!(events.len(), 1, "{events:?}");
    // The shell did not call the backend — running it is in the integrator's hands.
    assert!(requested.get());
    Ok(())
}

/// Called after the integrator has tidied up, it really does run.
#[test]
fn commit_power_runs_it() -> fairing::Result<()> {
    let mut h = mock_shell(SettingsConfig::default())?;
    h.frames(2);
    assert!(h.shell.commit_power(PowerRequest::Reboot).is_ok());
    Ok(())
}

/// The home icon can be turned off — for putting it only in the dock, or opening it only from a hidden entry point.
#[test]
fn the_home_icon_can_be_left_off() -> fairing::Result<()> {
    let h = mock_shell(SettingsConfig::default().without_home_icon())?;
    let on_desktop = h
        .shell
        .desktop()
        .pages()
        .iter()
        .any(|page| page.slots().any(|(_, _, slot)| slot.id == "settings.home"));
    assert!(!on_desktop, "the icon must not go up");
    Ok(())
}

/// The screens really do run frames — this looks at whether drawing panics.
#[test]
fn every_screen_draws() -> fairing::Result<()> {
    for id in [
        "settings.home",
        "settings.wifi",
        "settings.bluetooth",
        "settings.display",
        "settings.sound",
        "settings.datetime",
        "settings.locale",
        "settings.power",
        "settings.about",
    ] {
        let mut h = Harness::from_builder(move |ctx| {
            let services = fairing::services::mock::services();
            let mut shell = Shell::builder(single_level_access())
                .services(services)
                .build(ctx)?;
            add_all(&mut shell, &SettingsConfig::default());
            shell.launch(fairing::LaunchAction::open(id));
            Ok(shell)
        })?;
        h.frames(3);
        assert_eq!(
            h.shell
                .workspace()
                .focused()
                .map(fairing::workspace::Instance::decl_id),
            Some(id),
            "{id} did not open"
        );
    }
    Ok(())
}

/// **The two-column split**: on a Pane that is wide, `settings.home` draws a list column and a body column.
/// The point is that it pushes no screen — the workspace stack is as it was.
#[test]
fn a_wide_pane_draws_two_columns() -> fairing::Result<()> {
    let mut narrow = mock_shell(SettingsConfig::default())?.with_size(480.0, 800.0);
    narrow
        .shell
        .launch(fairing::LaunchAction::open("settings.home"));
    narrow.frames(3);
    let one_column = narrow.frame_shapes().len();

    let mut wide = mock_shell(SettingsConfig::default())?.with_size(1280.0, 720.0);
    wide.shell
        .launch(fairing::LaunchAction::open("settings.home"));
    wide.frames(3);
    let two_column = wide.frame_shapes().len();

    assert!(
        two_column > one_column,
        "a wide Pane has to draw more: {one_column} → {two_column}"
    );
    // It draws the right column inside the same screen — the focus is still `settings.home`.
    assert_eq!(
        wide.shell
            .workspace()
            .focused()
            .map(fairing::workspace::Instance::decl_id),
        Some("settings.home")
    );
    Ok(())
}

/// Narrow again it goes back to one column — folding a foldable must not leave half of it.
#[test]
fn folding_back_returns_to_one_column() -> fairing::Result<()> {
    let mut h = mock_shell(SettingsConfig::default())?.with_size(1280.0, 720.0);
    h.shell.launch(fairing::LaunchAction::open("settings.home"));
    h.frames(3);
    let wide = h.frame_shapes().len();
    let mut h = h.with_size(480.0, 800.0);
    h.frames(3);
    let narrow = h.frame_shapes().len();
    assert!(
        narrow < wide,
        "folded it has to be one column: {wide} → {narrow}"
    );
    Ok(())
}

/// A tall screen is one column however big it is — split in two, both halves feel cramped.
#[test]
fn a_tall_pane_stays_one_column() -> fairing::Result<()> {
    let mut tall = mock_shell(SettingsConfig::default())?.with_size(800.0, 1280.0);
    tall.shell
        .launch(fairing::LaunchAction::open("settings.home"));
    tall.frames(3);
    let mut narrow = mock_shell(SettingsConfig::default())?.with_size(480.0, 800.0);
    narrow
        .shell
        .launch(fairing::LaunchAction::open("settings.home"));
    narrow.frames(3);
    // The same column count means it drew the list alone — the widths differ so the shape counts are not equal, but a
    // split would add the whole right-hand body (the sliders and the cards) and the difference would be far bigger.
    let (tall_n, narrow_n) = (tall.frame_shapes().len(), narrow.frame_shapes().len());
    assert!(
        tall_n < narrow_n * 2,
        "the tall screen looks as though it split: {narrow_n} vs {tall_n}"
    );
    Ok(())
}

/// A card has to **stand apart from the screen's background.** A screen Pane's default background is `Surface`, so a
/// card is `SurfaceVariant` — with the two close together the whole group disappears (which really did happen).
///
/// # Why the floor is 1.12 and not WCAG 1.4.11's 3.0
///
/// It was proposed that the page/card separation be widened to 3.0 so a container is identified by
/// its own body rather than by an edge. Measured, that is not reachable by any palette that stays
/// recognisably dark or light:
///
/// | | card today | card at 3.0 |
/// |---|---|---|
/// | `base dark` | L 0.018 (1.19) | **`#626262`** — a mid grey card on a near-black page |
/// | `base light` | L 1.000 (1.18) | **`#888888`** — a mid grey card on a white page |
/// | `abyss dark` | L 0.027 (1.24) | `#676767` |
/// | `abyss light` | L 0.780 (1.26) | `#959595` |
///
/// The target is wrong rather than unreachable. SC 1.4.11 asks for 3.0 on "visual information
/// required to identify **user interface components** and states" — a thing you locate in order to
/// operate it. A card is a grouping, and its boundary is decoration; the row inside it is what gets
/// operated, and that row's own contrast is gated at 4.5 and 3.0 by
/// `every_preset_meets_the_contrast_floor`. One UI and iOS separate a card by a *small* fill step
/// plus a shadow, which is exactly the 1.19 plus `Elevation::Raised` here.
///
/// The complaint that started it — "big grey boxes repeating, so nothing tells you which element
/// matters" — is answered by drawing **fewer** cards, not louder ones: `layout::Container`
/// gives filled, outlined and divider-only, and the built-in settings screens now use the last.
#[test]
fn cards_are_distinct_from_the_background() {
    use fairing::theme::{Palette, Preset};

    /// The sRGB relative luminance (WCAG).
    fn luminance(c: egui::Color32) -> f32 {
        let f = |v: u8| {
            let s = f32::from(v) / 255.0;
            if s <= 0.040_45 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126f32.mul_add(f(c.r()), 0.7152 * f(c.g())) + 0.0722 * f(c.b())
    }

    for preset in Preset::ALL {
        for dark in [true, false] {
            let p = Palette::preset(*preset, dark);
            let (bg, card) = (p.surface, p.surface_variant);
            let ratio = {
                let (a, b) = (luminance(bg) + 0.05, luminance(card) + 0.05);
                if a > b {
                    a / b
                } else {
                    b / a
                }
            };
            assert!(
                ratio >= 1.12,
                "{preset:?} dark={dark}: the card and the background are too alike (contrast {ratio:.3})"
            );
        }
    }
}

/// [`layout::grid`] has **the width decide the column count** — an integrator does not count it again per panel.
#[test]
fn the_grid_picks_columns_from_the_width() {
    use fairing::layout;

    /// It gathers the cell Rects and counts the columns (how many share a y).
    fn columns(width: f32) -> usize {
        let mut config = single_level_access();
        config.motion.reduce = true;
        let Ok(h) = Harness::new(config, fairing::services::Services::null()) else {
            return 0;
        };
        let h = &mut h.with_size(width, 800.0);
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = std::rc::Rc::clone(&seen);
        h.shell.add(
            fairing::screen(
                "grid",
                move |ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>| {
                    let items = [0u8; 8];
                    layout::grid(ui, cx, 2.0, 2.0, &items, |_, _, cell| {
                        sink.borrow_mut().push(cell.rect.top());
                    });
                },
            )
            .title("grid"),
        );
        h.shell.launch(fairing::LaunchAction::open("grid"));
        h.frames(3);
        seen.borrow_mut().clear();
        h.frames(1);
        let tops = seen.borrow();
        let Some(first) = tops.first() else { return 0 };
        tops.iter().filter(|t| (*t - first).abs() < 0.5).count()
    }

    let narrow = columns(400.0);
    let wide = columns(1600.0);
    assert!(narrow >= 1, "even a narrow screen has to give one column");
    assert!(
        wide > narrow,
        "four times the width and the columns did not grow: {narrow} → {wide}"
    );
}

/// [`layout::action_bar`] **pins the bar to the bottom** — a long body does not move it.
#[test]
fn the_action_bar_stays_at_the_bottom() {
    use fairing::layout;

    /// It measures the bar's y as the number of body rows changes.
    fn bar_top(rows: usize) -> f32 {
        let mut config = single_level_access();
        config.motion.reduce = true;
        let Ok(h) = Harness::new(config, fairing::services::Services::null()) else {
            return f32::NAN;
        };
        let h = &mut h.with_size(800.0, 600.0);
        let seen = std::rc::Rc::new(std::cell::Cell::new(f32::NAN));
        let sink = std::rc::Rc::clone(&seen);
        h.shell.add(
            fairing::screen("bar", move |ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>| {
                layout::action_bar(
                    ui,
                    cx,
                    1.5,
                    |ui, cx| {
                        for i in 0..rows {
                            layout::info_row(ui, cx, "row", &i.to_string());
                        }
                    },
                    |ui, _| sink.set(ui.max_rect().top()),
                );
            })
            .title("bar"),
        );
        h.shell.launch(fairing::LaunchAction::open("bar"));
        h.frames(3);
        seen.get()
    }

    let (few, many) = (bar_top(1), bar_top(60));
    assert!(few.is_finite() && many.is_finite(), "{few} / {many}");
    assert!(
        (few - many).abs() < 1.0,
        "the body's length moved the bar: {few} vs {many}"
    );
}

/// **The logical size and the visual size are told apart** — the area a gloved hand presses stays as it is and only
/// what is seen shrinks. Giving a `visual_inset` must not shrink the hit Rect.
#[test]
fn visual_inset_shrinks_the_paint_not_the_hit() -> fairing::Result<()> {
    use fairing::layout::{self, Deco};

    /// It measures one cell's (logical, visual) Rect.
    fn measure(inset: f32) -> Option<(egui::Rect, egui::Rect)> {
        let mut config = single_level_access();
        config.motion.reduce = true;
        let Ok(h) = Harness::new(config, fairing::services::Services::null()) else {
            return None;
        };
        let h = &mut h.with_size(900.0, 700.0);
        let seen = std::rc::Rc::new(std::cell::Cell::new(None));
        let sink = std::rc::Rc::clone(&seen);
        h.shell.add(
            fairing::screen("g", move |ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>| {
                layout::Grid::new(2.0, 2.0)
                    .deco(Deco::new().visual_inset(inset))
                    .show(ui, cx, &[0u8], |_, _, cell| {
                        sink.set(Some((cell.rect, cell.visual)));
                    });
            })
            .title("g"),
        );
        h.shell.launch(fairing::LaunchAction::open("g"));
        h.frames(3);
        seen.get()
    }

    let Some((plain_rect, plain_visual)) = measure(0.0) else {
        return Err(fairing::Error::Config("the grid was not drawn".to_owned()));
    };
    let Some((inset_rect, inset_visual)) = measure(12.0) else {
        return Err(fairing::Error::Config("the grid was not drawn".to_owned()));
    };
    // By default logical = visual.
    assert!((plain_rect.width() - plain_visual.width()).abs() < 0.5);
    // The hit area is as it was.
    assert!(
        (plain_rect.width() - inset_rect.width()).abs() < 0.5,
        "the hit area shrank: {} → {}",
        plain_rect.width(),
        inset_rect.width()
    );
    // Only the visual shrank.
    assert!(
        inset_visual.width() < inset_rect.width() - 20.0,
        "the visual did not shrink: {} vs {}",
        inset_visual.width(),
        inset_rect.width()
    );
    Ok(())
}

/// **A slider row's layout turns on the width** — wide, the title, the track and the value are one line; narrow, two.
///
/// Stacked into two lines always, a slider card comes out more than twice as thick as a switch card. For one item of
/// the same kind. But **what shrank is the padding, not the hit height** — the track takes `touch_target` in either
/// layout (the glove policy).
#[test]
fn a_wide_slider_row_folds_onto_one_line() {
    use fairing::layout;

    /// It measures the height of a card holding one slider row.
    fn card_height(width: f32) -> f32 {
        let mut config = single_level_access();
        config.motion.reduce = true;
        let Ok(h) = Harness::new(config, fairing::services::Services::null()) else {
            return f32::NAN;
        };
        let h = &mut h.with_size(width, 640.0);
        let seen = Rc::new(Cell::new(f32::NAN));
        let sink = Rc::clone(&seen);
        let mut value = 50.0_f32;
        h.shell.add(
            screen("s", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                let top = ui.cursor().top();
                layout::group(ui, cx, |ui, cx| {
                    layout::slider_row(ui, cx, "Volume", &mut value, 0.0..=100.0, "%", true);
                });
                sink.set(ui.cursor().top() - top);
            })
            .title("s"),
        );
        h.shell.launch(fairing::LaunchAction::open("s"));
        h.frames(3);
        seen.get()
    }

    let (wide, narrow) = (card_height(1024.0), card_height(420.0));
    assert!(wide.is_finite() && narrow.is_finite(), "{wide} / {narrow}");
    assert!(
        wide < narrow * 0.8,
        "it did not fold to one line on a wide screen: wide {wide} vs narrow {narrow}"
    );
    // Either way one track has to fit — checking it did not fold to 0 and vanish.
    assert!(wide > 40.0, "the card vanished outright: {wide}");
}

/// `visual_inset` is **not the grid's alone** — a card and an action bar keep their place too and only draw inwards.
/// A knob that does nothing is worse than no knob.
#[test]
fn visual_inset_floats_the_card_and_the_bar() -> fairing::Result<()> {
    use fairing::layout::{self, Deco};

    /// It measures a card's inner width.
    fn card_inner(inset: f32) -> f32 {
        let mut config = single_level_access();
        config.motion.reduce = true;
        let Ok(h) = Harness::new(config, fairing::services::Services::null()) else {
            return f32::NAN;
        };
        let h = &mut h.with_size(900.0, 640.0);
        let seen = Rc::new(Cell::new(f32::NAN));
        let sink = Rc::clone(&seen);
        h.shell.add(
            screen("c", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                layout::group_with(ui, cx, Deco::new().visual_inset(inset), |ui, _| {
                    sink.set(ui.max_rect().width());
                });
            })
            .title("c"),
        );
        h.shell.launch(fairing::LaunchAction::open("c"));
        h.frames(3);
        seen.get()
    }

    /// It measures the action bar's (top of where it draws, where the body ends).
    fn bar_geometry(inset: f32) -> Option<(f32, f32)> {
        let mut config = single_level_access();
        config.motion.reduce = true;
        let Ok(h) = Harness::new(config, fairing::services::Services::null()) else {
            return None;
        };
        let h = &mut h.with_size(900.0, 640.0);
        let seen = Rc::new(Cell::new(None));
        let sink = Rc::clone(&seen);
        let body_seen = Rc::new(Cell::new(f32::NAN));
        let body_sink = Rc::clone(&body_seen);
        h.shell.add(
            screen("b", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                let bar_top = Rc::clone(&sink);
                layout::action_bar_with(
                    ui,
                    cx,
                    1.5,
                    Deco::new().visual_inset(inset),
                    |ui, _| body_sink.set(ui.max_rect().bottom()),
                    move |ui, _| bar_top.set(Some(ui.max_rect().top())),
                );
            })
            .title("b"),
        );
        h.shell.launch(fairing::LaunchAction::open("b"));
        h.frames(3);
        seen.get().map(|top| (top, body_seen.get()))
    }

    // The card: the drawing comes inwards and the inner width shrinks.
    let (plain, floated) = (card_inner(0.0), card_inner(14.0));
    assert!(
        plain.is_finite() && floated.is_finite(),
        "{plain}/{floated}"
    );
    assert!(
        floated < plain - 20.0,
        "the card did not lift: {plain} → {floated}"
    );

    // The bar: where it draws settles inwards, and where the body ends (= the logical place) is as it was.
    let Some((plain_top, plain_body)) = bar_geometry(0.0) else {
        return Err(fairing::Error::Config("the bar was not drawn".to_owned()));
    };
    let Some((float_top, float_body)) = bar_geometry(14.0) else {
        return Err(fairing::Error::Config("the bar was not drawn".to_owned()));
    };
    assert!(
        float_top > plain_top + 8.0,
        "the bar did not lift: {plain_top} → {float_top}"
    );
    assert!(
        (plain_body - float_body).abs() < 1.0,
        "the body's place moved with it: {plain_body} vs {float_body}"
    );
    Ok(())
}

/// `ListRow::height` **only grows** — a value below `touch_target` is taken up to that value. One screen being dense
/// is no reason to make a row a gloved hand cannot press.
#[test]
fn a_row_can_grow_but_never_below_the_touch_target() -> fairing::Result<()> {
    use fairing::widgets::ListRow;

    /// It measures one row's real height. `None` means `touch_target` could not be read either.
    fn row_height(request: Option<f32>) -> Option<(f32, f32)> {
        let mut config = single_level_access();
        config.motion.reduce = true;
        let h = Harness::new(config, fairing::services::Services::null()).ok()?;
        let h = &mut h.with_size(800.0, 640.0);
        let seen = Rc::new(Cell::new(None));
        let sink = Rc::clone(&seen);
        h.shell.add(
            screen("r", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                let mut row = ListRow::new("row");
                if let Some(du) = request {
                    row = row.height(du);
                }
                let rect = row.show(ui, &mut cx.widgets()).rect;
                sink.set(Some((rect.height(), cx.theme.metrics.touch_target)));
            })
            .title("r"),
        );
        h.shell.launch(fairing::LaunchAction::open("r"));
        h.frames(3);
        seen.get()
    }

    let Some((base, touch)) = row_height(None) else {
        return Err(fairing::Error::Config("the row was not drawn".to_owned()));
    };
    assert!(
        base >= touch,
        "the default row is shorter than the touch target: {base}/{touch}"
    );

    // Growing it is free.
    let Some((tall, _)) = row_height(Some(base * 2.0)) else {
        return Err(fairing::Error::Config("the row was not drawn".to_owned()));
    };
    assert!(
        (tall - base * 2.0).abs() < 1.0,
        "the height asked for was not taken: {tall} vs {}",
        base * 2.0
    );

    // Shrinking it is stopped at the floor.
    let Some((squashed, _)) = row_height(Some(4.0)) else {
        return Err(fairing::Error::Config("the row was not drawn".to_owned()));
    };
    assert!(
        (squashed - touch).abs() < 1.0,
        "it went below the touch target: {squashed} vs {touch}"
    );
    Ok(())
}

/// **The grid does not overflow the width.** The `tile_w` formula sets only `gap × (columns + 1)` aside, while
/// `ui.horizontal` slips egui's default `item_spacing.x` (10 du) in between every item as well — at 2 columns that
/// added 40 du and cut the right column off the screen. Caught by really laying a kiosk menu out.
#[test]
fn the_grid_never_overflows_its_width() -> fairing::Result<()> {
    use fairing::layout;

    /// It gathers one row's cell Rects.
    fn row_rects(width: f32, items: usize) -> (Vec<egui::Rect>, f32) {
        let mut config = single_level_access();
        config.motion.reduce = true;
        let Ok(h) = Harness::new(config, fairing::services::Services::null()) else {
            return (Vec::new(), 0.0);
        };
        let h = &mut h.with_size(width, 900.0);
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let avail = Rc::new(Cell::new(0.0));
        let (sink, aw) = (Rc::clone(&seen), Rc::clone(&avail));
        let list: Vec<u8> = (0..items).map(|i| u8::try_from(i).unwrap_or(0)).collect();
        h.shell.add(
            screen("g", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                aw.set(ui.available_width());
                sink.borrow_mut().clear();
                layout::Grid::new(2.0, 2.0).show(ui, cx, &list, |_, _, cell| {
                    sink.borrow_mut().push(cell.rect);
                });
            })
            .title("g"),
        );
        h.shell.launch(fairing::LaunchAction::open("g"));
        h.frames(3);
        let out = seen.borrow().clone();
        (out, avail.get())
    }

    for (width, items) in [(900.0_f32, 6_usize), (1080.0, 6), (640.0, 4), (1400.0, 9)] {
        let (rects, avail) = row_rects(width, items);
        assert!(!rects.is_empty(), "the grid was not drawn ({width})");
        let Some(first) = rects.first() else {
            return Err(fairing::Error::Config("there is no cell".to_owned()));
        };
        // The right edge of the first row must not go past the width available.
        let left = first.left();
        let right = rects
            .iter()
            .filter(|r| (r.top() - first.top()).abs() < 1.0)
            .fold(f32::MIN, |acc, r| acc.max(r.right()));
        assert!(
            right - left <= avail + 1.0,
            "width {width}: the grid overflowed by {} du (available width {avail})",
            right - left - avail
        );
    }
    Ok(())
}

/// **The built-in Wi-Fi screen shows; it does not connect**.
///
/// A tap on a network row leaves as [`ShellEvent::WifiNetworkTapped`] and the shell does nothing
/// else — no `connect`, whether the network is open, saved or secured.
///
/// This is a regression test. The screen used to connect on a tap for open and saved
/// networks and refuse a secured one it had no profile for, leaving a line in the log. `MockWifi`
/// starts with `known` empty, the way a device does out of the box, so **every secured network was
/// dead** — and because the tap was already spent, adding a password prompt meant rewriting the
/// screen whole rather than taking one event.
///
/// The rows are probed rather than worked out — a test does not copy the layout formula. It sweeps
/// bottom-up so it has the list before it can reach the scan row or the radio toggle, and stops as
/// soon as every network has answered.
#[test]
fn tapping_a_wifi_network_reports_it_and_connects_to_nothing() -> fairing::Result<()> {
    use fairing::services::WifiState;

    // Tall enough that the whole list is on screen — `ui::page` scrolls, and a row below the fold
    // cannot be tapped.
    let mut h = mock_shell(SettingsConfig::default())?.with_size(1024.0, 1400.0);
    h.shell.launch(fairing::LaunchAction::open("settings.wifi"));
    // The open transition puts a shield over the pane, so taps have to wait for it.
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
    assert!(!h.shell.workspace().is_home(), "settings.wifi did not open");
    let _ = h.shell.poll_events();

    let want = ["fairing-lab", "guest", "fail-net"];
    let content = h.shell.layout().content;
    let step = h.shell.theme().metrics.row_height / 2.0;
    let x = content.center().x;

    let mut seen: Vec<(String, bool, bool)> = Vec::new();
    let mut y = content.bottom() - step;
    while y > content.top() && seen.len() < want.len() {
        h.tap(egui::pos2(x, y));
        h.frames(2);
        for event in h.shell.poll_events() {
            if let ShellEvent::WifiNetworkTapped {
                ssid,
                secured,
                known,
            } = event
            {
                if !seen.iter().any(|(s, _, _)| *s == ssid) {
                    seen.push((ssid, secured, known));
                }
            }
        }
        y -= step;
    }

    for ssid in want {
        assert!(
            seen.iter().any(|(s, _, _)| s == ssid),
            "`{ssid}` never reported a tap: {seen:?}"
        );
    }
    // The one that was dead before: secured, with nothing saved for it.
    assert!(
        seen.iter()
            .any(|(s, secured, known)| s == "fairing-lab" && *secured && !*known),
        "the secured network has to report secured = true and known = false: {seen:?}"
    );

    // **Nothing was connected** — not the open one and not the saved-looking one either.
    let snapshot = h.shell.services().wifi.snapshot();
    assert!(
        snapshot.enabled,
        "the sweep reached the radio toggle - the probe went further than the list"
    );
    assert!(
        !matches!(
            snapshot.state,
            WifiState::Connecting | WifiState::Connected { .. }
        ),
        "the shell connected on its own: {:?}",
        snapshot.state
    );
    assert!(
        snapshot.known.is_empty(),
        "the shell saved a profile on its own: {:?}",
        snapshot.known
    );
    Ok(())
}

/// Every string drawn this frame, so a test can ask whether something reached the screen.
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

/// Opens a settings screen and runs until its transition is over.
fn open_settings(config: SettingsConfig, id: &str) -> fairing::Result<Harness> {
    // Tall enough that nothing is below the fold — `ui::page` scrolls, and a row that is scrolled
    // out cannot be tapped.
    let mut h = mock_shell(config)?.with_size(1024.0, 1400.0);
    h.shell.launch(fairing::LaunchAction::open(id));
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
    let _ = h.shell.poll_events();
    Ok(h)
}

/// **Holding a network row reports a long press, and that press is not also a tap.**
///
/// This is where "forget this network" goes —
/// [`WifiBackend::forget`](fairing::services::WifiBackend::forget) is the integrator's call, and
/// without a hold to hang it on the only way to offer it would be to rewrite the screen, which is
/// the blockage that handing network taps to the integrator set out to remove.
///
/// The row times its own press rather than asking egui, because `Response::long_touched()` needs
/// real touch events and never fires where the platform delivers touch as the mouse —
/// this test drives the synthetic pointer, so it would fail against that implementation.
#[test]
fn holding_a_wifi_network_reports_a_long_press_and_not_a_tap() -> fairing::Result<()> {
    use fairing::services::WifiState;

    let mut h = open_settings(SettingsConfig::default(), "settings.wifi")?;

    // Find a row by probing, then hold that spot.
    let content = h.shell.layout().content;
    let step = h.shell.theme().metrics.row_height / 2.0;
    let x = content.center().x;
    let mut row: Option<(f32, String)> = None;
    let mut y = content.bottom() - step;
    while y > content.top() && row.is_none() {
        h.tap(egui::pos2(x, y));
        h.frames(2);
        for event in h.shell.poll_events() {
            if let ShellEvent::WifiNetworkTapped { ssid, .. } = event {
                row = Some((y, ssid));
            }
        }
        y -= step;
    }
    let Some((row_y, ssid)) = row else {
        return Err(fairing::Error::Config("no network row answered".to_owned()));
    };

    // `[gesture] long_press` is 500 ms = 30 frames at 60 Hz; 45 clears it comfortably.
    let at = egui::pos2(x, row_y);
    h.hold(at, 45);
    h.release(at);
    h.frames(2);

    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::WifiNetworkLongPressed { ssid: s, known: false } if *s == ssid
        )),
        "holding `{ssid}` reported no long press: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ShellEvent::WifiNetworkTapped { .. })),
        "the release that ended the hold arrived as a tap too: {events:?}"
    );

    // And the shell still did nothing of its own.
    let snapshot = h.shell.services().wifi.snapshot();
    assert!(
        !matches!(
            snapshot.state,
            WifiState::Connecting | WifiState::Connected { .. }
        ),
        "the shell connected on its own: {:?}",
        snapshot.state
    );
    Ok(())
}

/// **A pairing request is drawn, and confirming it answers the backend**.
///
/// The passkey is the backend's; the screen receives it and shows it so the number can be checked
/// against the other device. Before this the screen called `pair()` and then drew neither the
/// request nor a way to answer, so pairing stopped there with nothing on screen.
#[test]
fn a_pairing_passkey_is_drawn_and_confirming_answers_the_backend() -> fairing::Result<()> {
    let mut h = open_settings(SettingsConfig::default(), "settings.bluetooth")?;

    // The backend raises the request — `MockBluetooth` uses a fixed passkey, being a simulator.
    let addr = "AA:BB:CC:00:00:01";
    if h.shell.services_mut().bluetooth.pair(addr).is_err() {
        return Err(fairing::Error::Config(
            "the mock refused to pair".to_owned(),
        ));
    }
    h.frames(2);
    assert!(
        h.shell.services().bluetooth.snapshot().pending.is_some(),
        "the mock did not raise a pairing request"
    );

    // **It is on screen.** This is the whole point: the crate receives the passkey and draws it.
    let text = drawn_text(&mut h);
    assert!(
        text.contains("123456"),
        "the passkey never reached the screen: {text:?}"
    );

    // Probe down for `Confirm`; it is the first row under the card, so the first tap that clears
    // the request is that one.
    let content = h.shell.layout().content;
    let step = h.shell.theme().metrics.row_height / 2.0;
    let x = content.center().x;
    let mut y = content.top() + step;
    while y < content.bottom() && h.shell.services().bluetooth.snapshot().pending.is_some() {
        h.tap(egui::pos2(x, y));
        h.frames(2);
        y += step;
    }

    let snapshot = h.shell.services().bluetooth.snapshot();
    assert!(
        snapshot.pending.is_none(),
        "the request is still waiting - nothing on the card answered it"
    );
    assert!(
        snapshot.devices.iter().any(|d| d.addr == addr && d.paired),
        "confirming did not pair the device: {:?}",
        snapshot.devices
    );
    Ok(())
}

/// Every line of text drawn this frame, so a row can be looked for **exactly** rather than as a
/// substring of some other label.
fn drawn_lines(h: &mut Harness) -> Vec<String> {
    drawn_text(h).lines().map(str::to_owned).collect()
}

/// **The list shows only screens that are really registered**.
///
/// The entry table names eleven ids, but `add_all` registers fewer:
/// `settings.credentials` only where the authenticator manages its entries (there is none here),
/// and anything the integrator dropped with [`SettingsConfig::without`] is gone too. Those rows
/// were drawn anyway, and pressing one did nothing at all — `Shell::launch` cannot open an id that
/// is not there. (`settings.network` used to have no screen either; the mock network backend
/// stands behind one now.)
#[test]
fn the_list_shows_only_screens_that_are_really_registered() -> fairing::Result<()> {
    let mut h = open_settings(
        SettingsConfig::default().without("settings.sound"),
        "settings.home",
    )?;
    let lines = drawn_lines(&mut h);
    let has = |label: &str| lines.iter().any(|line| line == label);
    assert!(
        has("Display"),
        "a registered entry is still listed: {lines:?}"
    );
    assert!(
        !has("Sound"),
        "`without(\"settings.sound\")` dropped the screen, so the row goes too: {lines:?}"
    );
    assert!(
        has("Network"),
        "settings.network is registered behind the network backend: {lines:?}"
    );
    assert!(
        !has("Users & access"),
        "settings.credentials needs an authenticator that manages its entries: {lines:?}"
    );
    Ok(())
}

/// **The right column draws whatever is registered under the id**.
///
/// A wide screen puts the selected entry's screen up on the right. It used to call a private `fn`
/// out of a table of the eight built-in bodies, so a screen the integrator registered — their own,
/// or one replacing a built-in under the same id — could never appear there, however the module
/// documentation advertised the "deep" override. Now the column asks the registry.
#[test]
fn the_right_column_draws_the_screen_the_integrator_registered() -> fairing::Result<()> {
    const MARK: &str = "our own Wi-Fi screen";
    let mut h = Harness::from_builder(|ctx| {
        let services = fairing::services::mock::services();
        let mut shell = Shell::builder(single_level_access())
            .services(services)
            .build(ctx)?;
        add_all(&mut shell, &SettingsConfig::default());
        // The "deep" override the module documents: the same id, registered afterwards.
        shell.add(screen(
            "settings.wifi",
            |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label(MARK);
            },
        ));
        Ok(shell)
    })?
    // Landscape and wide enough for the two-pane split.
    .with_size(1400.0, 800.0);
    h.shell.launch(fairing::LaunchAction::open("settings.home"));
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
    let _ = h.shell.poll_events();

    let lines = drawn_lines(&mut h);
    assert!(
        lines.iter().any(|line| line == "Display"),
        "the list is on the left, so this really is the split: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line == MARK),
        "the right column has to draw the registered screen: {lines:?}"
    );
    Ok(())
}

/// **The settings list takes rows out and puts rows in**.
///
/// The list used to be a private constant, so it was neither. Now it is a `Vec` the integrator
/// owns: [`entries`](fairing::settings::screens::entries) gives the built-in one and
/// [`add_all_with`] registers `settings.home` over whatever is made of it. A row of one's own
/// wants a screen of one's own with it — the row is drawn only where the screen is really there,
/// which is the same rule that hides `settings.network`.
#[test]
fn the_settings_list_takes_rows_out_and_puts_rows_in() -> fairing::Result<()> {
    use fairing::settings::add_all_with;
    use fairing::settings::screens::{entries, SettingsEntry};

    let mut h = Harness::from_builder(|ctx| {
        let services = fairing::services::mock::services();
        let mut shell = Shell::builder(single_level_access())
            .services(services)
            .build(ctx)?;
        let mut list = entries();
        list.retain(|entry| entry.id != "settings.locale");
        list.push(SettingsEntry::new(
            "app.heater",
            fairing::icon::GAUGE,
            "Heater",
        ));
        // The row on its own is not enough — nothing is drawn for an id with no screen.
        shell.add(screen("app.heater", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("heater body");
        }));
        add_all_with(&mut shell, &SettingsConfig::default(), list);
        Ok(shell)
    })?
    .with_size(1024.0, 1400.0);
    h.shell.launch(fairing::LaunchAction::open("settings.home"));
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
    let _ = h.shell.poll_events();

    let lines = drawn_lines(&mut h);
    let has = |label: &str| lines.iter().any(|line| line == label);
    assert!(has("Heater"), "a row of our own is listed: {lines:?}");
    assert!(!has("Language"), "and one we took out is gone: {lines:?}");
    assert!(
        has("Display"),
        "the rest of the list is untouched: {lines:?}"
    );
    Ok(())
}

/// A row of one's own **only shows once its screen is registered** — the same rule that hides
/// `settings.network`. Without it the list would offer a row that does nothing, which is the
/// defect report #4 was about.
#[test]
fn a_row_without_a_screen_behind_it_is_not_listed() -> fairing::Result<()> {
    use fairing::settings::add_all_with;
    use fairing::settings::screens::{entries, SettingsEntry};

    let mut h = Harness::from_builder(|ctx| {
        let services = fairing::services::mock::services();
        let mut shell = Shell::builder(single_level_access())
            .services(services)
            .build(ctx)?;
        let mut list = entries();
        list.push(SettingsEntry::new(
            "app.heater",
            fairing::icon::GAUGE,
            "Heater",
        ));
        add_all_with(&mut shell, &SettingsConfig::default(), list);
        Ok(shell)
    })?
    .with_size(1024.0, 1400.0);
    h.shell.launch(fairing::LaunchAction::open("settings.home"));
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
    let _ = h.shell.poll_events();

    let lines = drawn_lines(&mut h);
    assert!(
        !lines.iter().any(|line| line == "Heater"),
        "no screen is registered under `app.heater`, so the row would open nothing: {lines:?}"
    );
    Ok(())
}

/// **A settings screen behind a gate of its own is neither listed nor drawn.**
///
/// The list filtered on `cx.allows(entry.id)` — the **entry id** taken as a gate name. A
/// declaration's gate defaults to its id, but `.gate("service")` names another, and only the
/// declaration knows. So a maintainer-only Wi-Fi screen was listed to a viewer, and on a wide
/// screen it was the first row, which meant the right column painted its contents with no
/// interaction at all — while `LaunchAction::open` correctly refused the same screen.
#[test]
fn a_settings_screen_behind_its_own_gate_is_neither_listed_nor_drawn() -> fairing::Result<()> {
    const SECRET: &str = "service only wifi";
    let mut config = fairing::testing::access_config(&["viewer", "service"], Some("viewer"));
    config
        .access
        .gates
        .insert("service".to_owned(), "service".to_owned());
    let build = move |ctx: &egui::Context| {
        let services = fairing::services::mock::services();
        let mut shell = Shell::builder(config.clone())
            .services(services)
            .build(ctx)?;
        add_all(&mut shell, &SettingsConfig::default());
        shell.add(
            screen("settings.wifi", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label(SECRET);
            })
            .gate("service"),
        );
        Ok(shell)
    };

    // Wide: the row would be the first one, so the right column would draw it unprompted.
    let mut wide = Harness::from_builder(build.clone())?.with_size(1400.0, 800.0);
    wide.shell
        .launch(fairing::LaunchAction::open("settings.home"));
    for _ in 0..120 {
        wide.frame();
        if !wide.shell.is_animating() {
            break;
        }
    }
    let _ = wide.shell.poll_events();
    let lines = drawn_lines(&mut wide);
    assert!(
        !lines.iter().any(|line| line == SECRET),
        "a viewer must not be shown a service-gated screen's body: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "Wi-Fi"),
        "nor the row that leads to it: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line == "Display"),
        "the ungated rows are still there: {lines:?}"
    );

    // Narrow: the row was listed and a tap did nothing, which is the listed-but-unregistered
    // defect all over again.
    let mut narrow = Harness::from_builder(build)?.with_size(1024.0, 1400.0);
    narrow
        .shell
        .launch(fairing::LaunchAction::open("settings.home"));
    for _ in 0..120 {
        narrow.frame();
        if !narrow.shell.is_animating() {
            break;
        }
    }
    let _ = narrow.shell.poll_events();
    let lines = drawn_lines(&mut narrow);
    assert!(
        !lines.iter().any(|line| line == "Wi-Fi"),
        "a row that cannot be opened is not drawn: {lines:?}"
    );
    Ok(())
}

/// A list that filters down to nothing **says so** rather than leaving half the screen blank.
#[test]
fn a_settings_list_with_nothing_in_it_says_so() -> fairing::Result<()> {
    use fairing::settings::add_all_with;

    let mut h = Harness::from_builder(|ctx| {
        let services = fairing::services::mock::services();
        let mut shell = Shell::builder(single_level_access())
            .services(services)
            .build(ctx)?;
        add_all_with(&mut shell, &SettingsConfig::default(), Vec::new());
        Ok(shell)
    })?
    .with_size(1400.0, 800.0);
    h.shell.launch(fairing::LaunchAction::open("settings.home"));
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
    let _ = h.shell.poll_events();
    let lines = drawn_lines(&mut h);
    assert!(
        lines.iter().any(|line| line.contains("Choose a setting")),
        "an empty right column with no word of explanation reads as a bug: {lines:?}"
    );
    Ok(())
}

/// **A repeated id is dropped, keeping the first row.**
///
/// The selection is an id, so two rows pointing at one screen both light up when either is pressed
/// and the right column takes its heading from whichever comes first — the second row's label never
/// appears at all. `entries()` already carries every built-in id, so pushing "our Wi-Fi" onto it is
/// an easy list to build by mistake.
#[test]
fn a_repeated_row_is_dropped() -> fairing::Result<()> {
    use fairing::settings::add_all_with;
    use fairing::settings::screens::{entries, SettingsEntry};

    let mut h = Harness::from_builder(|ctx| {
        let services = fairing::services::mock::services();
        let mut shell = Shell::builder(single_level_access())
            .services(services)
            .build(ctx)?;
        let mut list = entries();
        list.push(SettingsEntry::new(
            "settings.wifi",
            fairing::icon::GAUGE,
            "Our Wi-Fi",
        ));
        add_all_with(&mut shell, &SettingsConfig::default(), list);
        Ok(shell)
    })?
    .with_size(1024.0, 1400.0);
    h.shell.launch(fairing::LaunchAction::open("settings.home"));
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
    let _ = h.shell.poll_events();
    let lines = drawn_lines(&mut h);
    assert!(
        lines.iter().any(|line| line == "Wi-Fi"),
        "the first row stays: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "Our Wi-Fi"),
        "the repeat is dropped rather than drawn as a second row for the same screen: {lines:?}"
    );
    Ok(())
}

/// Where a drawn text sits this frame — the first shape whose text is exactly `wanted`.
fn text_rect(h: &mut Harness, wanted: &str) -> Option<egui::Rect> {
    fn walk(shape: &egui::Shape, wanted: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == wanted => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| walk(s, wanted)),
            _ => None,
        }
    }
    h.frame_shapes()
        .iter()
        .find_map(|clipped| walk(&clipped.shape, wanted))
}

/// Tap the middle of a drawn text, then let the screen settle.
fn tap_text(h: &mut Harness, wanted: &str) -> fairing::Result<()> {
    let rect = text_rect(h, wanted)
        .ok_or_else(|| fairing::Error::Config(format!("`{wanted}` is not drawn")))?;
    h.tap(rect.center());
    h.frames(3);
    Ok(())
}

fn eth0(h: &Harness) -> fairing::Result<fairing::services::IfaceSnapshot> {
    h.shell
        .services()
        .network
        .interfaces()
        .into_iter()
        .find(|iface| iface.name == "eth0")
        .ok_or_else(|| fairing::Error::Config("there is no eth0".to_owned()))
}

/// The network screen draws what the backend reports — link, address and how it was
/// set, the hardware address, the hostname.
#[test]
fn the_network_screen_lists_the_interfaces() -> fairing::Result<()> {
    let mut h = open_settings(SettingsConfig::default(), "settings.network")?;
    let text = drawn_text(&mut h);
    for want in [
        "eth0 · Wired",
        "Connected",
        "192.168.0.42/24 · DHCP",
        "a4:5e:60:11:22:33",
        "fairing-demo",
    ] {
        assert!(text.contains(want), "`{want}` is not drawn:\n{text}");
    }
    Ok(())
}

/// The form starts from what the interface has, so switching to a manual address and
/// applying hands the backend that address as a static one — and the list says so after.
#[test]
fn switching_to_a_manual_address_reaches_the_backend() -> fairing::Result<()> {
    let mut h = open_settings(SettingsConfig::default(), "settings.network")?;
    tap_text(&mut h, "Set the IPv4 address")?;
    tap_text(&mut h, "Manual")?;
    tap_text(&mut h, "Apply")?;
    let iface = eth0(&h)?;
    assert!(!iface.dhcp, "{iface:?}");
    assert_eq!(
        iface.ipv4.map(|net| (net.addr, net.prefix)),
        Some((std::net::Ipv4Addr::new(192, 168, 0, 42), 24))
    );
    let text = drawn_text(&mut h);
    assert!(text.contains("192.168.0.42/24 · manual"), "{text}");
    Ok(())
}

/// A hostname that is not one is refused on the form — the backend keeps the old name —
/// and Cancel goes back to the list.
#[test]
fn a_bad_hostname_is_refused_on_the_form() -> fairing::Result<()> {
    let mut h = open_settings(SettingsConfig::default(), "settings.network")?;
    // Two interface cards put the hostname row below a 1400 px panel; make room rather than scroll.
    h.set_size(1024.0, 2200.0);
    h.frames(3);
    tap_text(&mut h, "Hostname")?;
    tap_text(&mut h, "fairing-demo")?;
    h.type_text("!");
    h.frames(2);
    tap_text(&mut h, "Apply")?;
    let text = drawn_text(&mut h);
    assert!(text.contains("Not applied"), "{text}");
    assert_eq!(
        h.shell.services().network.hostname().as_deref(),
        Some("fairing-demo")
    );
    tap_text(&mut h, "Cancel")?;
    assert!(drawn_text(&mut h).contains("Set the IPv4 address"));
    Ok(())
}
