//! Public-API regression for untyped hooks entering the protected facade.
#![allow(
    clippy::panic,
    clippy::unwrap_used,
    reason = "Security regressions deliberately fail if an unmediated hook runs"
)]
use fairing::{egui, shell::ShellBuilder, Error, Shell, ShellConfig};
use std::{cell::Cell, rc::Rc};

fn builder() -> ShellBuilder {
    let mut config = ShellConfig::default();
    config.access.session_tokens = true;
    Shell::builder(config)
}

type HookInstaller = fn(ShellBuilder) -> ShellBuilder;

#[test]
fn all_untyped_extension_families_are_rejected_before_execution() {
    let ctx = egui::Context::default();
    let cases: &[(&str, HookInstaller)] = &[
        ("status_bar_painter", |b| {
            b.status_bar_painter(|_, _| panic!("unguarded"))
        }),
        ("status_bar_layout", |b| {
            b.status_bar_layout(|_, _| panic!("unguarded"))
        }),
        ("nav_bar_painter", |b| {
            b.nav_bar_painter(|_, _| panic!("unguarded"))
        }),
        ("nav_bar_layout", |b| {
            b.nav_bar_layout(|_, _| panic!("unguarded"))
        }),
        ("toast_painter", |b| {
            b.toast_painter(|_, _| panic!("unguarded"))
        }),
        ("toast_layout", |b| {
            b.toast_layout(|_, _| panic!("unguarded"))
        }),
        ("heads_up_painter", |b| {
            b.heads_up_painter(|_, _| panic!("unguarded"))
        }),
        ("heads_up_layout", |b| {
            b.heads_up_layout(|_, _| panic!("unguarded"))
        }),
        ("lock_screen_painter", |b| {
            b.lock_screen_painter(|_, _| panic!("unguarded"))
        }),
        ("unlock_prompt_painter", |b| {
            b.unlock_prompt_painter(|_, _| panic!("unguarded"))
        }),
        ("recents_card_painter", |b| {
            b.recents_card_painter(|_, _| panic!("unguarded"))
        }),
        ("recents_ground_painter", |b| {
            b.recents_ground_painter(|_, _| panic!("unguarded"))
        }),
        ("slot_painter", |b| {
            b.slot_painter(|_, _| panic!("unguarded"))
        }),
        ("gesture_handle_painter", |b| {
            b.gesture_handle_painter(|_, _| panic!("unguarded"))
        }),
        ("image_loader", |b| {
            b.image_loader(|_, _| panic!("unguarded"))
        }),
        ("wallpaper", |b| {
            b.wallpaper(fairing::desktop::Wallpaper::Painter(Box::new(|_, _| {
                panic!("unguarded")
            })))
        }),
        ("widget_painters", |b| {
            b.widget_painters(
                fairing::widgets::WidgetPainters::new().button(|_, _| panic!("unguarded")),
            )
        }),
        #[cfg(feature = "osk")]
        ("osk_key_layout", |b| {
            b.osk_key_layout(|_, _| panic!("unguarded"))
        }),
        #[cfg(feature = "osk")]
        ("osk_key_painter", |b| {
            b.osk_key_painter(|_, _| panic!("unguarded"))
        }),
        #[cfg(feature = "overlay")]
        ("shade_tile_layout", |b| {
            b.shade_tile_layout(|_, _| panic!("unguarded"))
        }),
        #[cfg(feature = "overlay")]
        ("shade_tile_painter", |b| {
            b.shade_tile_painter(|_, _| panic!("unguarded"))
        }),
        #[cfg(feature = "overlay")]
        ("shade_panel_painter", |b| {
            b.shade_panel_painter(|_, _| panic!("unguarded"))
        }),
    ];
    for (name, install) in cases {
        let result = install(builder()).build_protected(&ctx);
        assert!(
            matches!(result, Err(Error::Config(ref message)) if message.contains(name)),
            "accepted {name}"
        );
    }
}

#[test]
fn inactive_and_nested_wallpaper_branches_cannot_smuggle_a_painter() {
    use fairing::desktop::Wallpaper;
    let calls = Rc::new(Cell::new(0));
    let captured = Rc::clone(&calls);
    let wallpaper = Wallpaper::themed(
        Wallpaper::Fixed(egui::Color32::BLACK),
        Wallpaper::themed(
            Wallpaper::Fixed(egui::Color32::WHITE),
            Wallpaper::ThemedPainter(Box::new(move |_, _, _| captured.set(captured.get() + 1))),
        ),
    );
    assert!(builder()
        .wallpaper(wallpaper)
        .build_protected(&egui::Context::default())
        .is_err());
    assert_eq!(calls.get(), 0);
    assert!(builder()
        .wallpaper(Wallpaper::themed(
            Wallpaper::Fixed(egui::Color32::BLACK),
            Wallpaper::Fixed(egui::Color32::WHITE),
        ))
        .build_protected(&egui::Context::default())
        .is_ok());
}

#[test]
fn legacy_customization_remains_explicitly_available() {
    let calls = Rc::new(Cell::new(0));
    let captured = Rc::clone(&calls);
    let ctx = egui::Context::default();
    let mut shell = builder()
        .status_bar_painter(move |_, _| captured.set(captured.get() + 1))
        .build(&ctx)
        .unwrap();
    for _ in 0..3 {
        ctx.run_ui(egui::RawInput::default(), |ui| shell.frame(ui))
            .drop_without_applying_deltas();
    }
    assert!(calls.get() > 0);
}
