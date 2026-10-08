//! **The smallest complete app**: one screen on the desktop, the built-in
//! settings, a status bar item and a nav bar item, in a window.
//!
//! ```text
//! cargo run -p fairing --example hello --features runner-x11
//! ```
//!
//! It is the README's quick start as a file you can run, and it stands alone — no `mod common`,
//! unlike the other examples — so it is the one to copy into a new project.

use fairing::prelude::*;

/// The shell explains itself in the log: a missing bold face, a panel size it had to assume, a nav
/// item with no slot. A real app plugs in its own logger; this one prints warnings to stderr.
struct Stderr;

impl log::Log for Stderr {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static LOGGER: Stderr = Stderr;

fn main() -> fairing::Result<()> {
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Warn));

    // A device reads its file: `ShellConfig::load("/etc/myapp/fairing.toml")?`, defaults if it is
    // missing. This example sets the one key it needs in code instead.
    let mut config = ShellConfig::default();
    // A nav item is drawn only where this list names it.
    config.nav_bar.items = vec!["back".to_owned(), "home".to_owned(), "kbd".to_owned()];

    // A window for the desktop; `Options::default()` is the fullscreen device build.
    let options = fairing::runner::Options {
        fullscreen: false,
        title: "hello".to_owned(),
        size: Some((1024.0, 600.0)),
    };
    fairing::runner::run_shell(options, move |ctx| {
        let mut shell = Shell::builder(config)
            .services(Services::builder().build()) // the real clock, null everything else
            .physical_mm(154.0, 86.0) // the panel's visible area: millimetres become real
            .build(ctx)?;

        // `settings.home` and the screens under it. Wi-Fi, Bluetooth, network and power join
        // only when a backend has them.
        fairing::settings::add_all(&mut shell, &fairing::settings::SettingsConfig::default());

        shell.add(
            screen("dashboard", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                ui.heading("Dashboard");
                if ui.button("Settings").clicked() {
                    cx.open("settings.home");
                }
                if ui.button("Say hello").clicked() {
                    cx.shell.toast("Hello");
                }
            })
            .title("Dashboard")
            .icon(icon::GAUGE)
            .desktop(),
        );
        shell.add(status_item("temp", Slot::Right, |ui, _cx| {
            ui.label("36.5 °C");
        }));
        shell.add(nav_item("kbd", |ui, _cx| {
            let _ = ui.button("⌨");
        }));
        shell.remove("status.bluetooth"); // built-ins go away by id

        Ok(shell)
    })
}
