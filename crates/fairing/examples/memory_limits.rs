//! Headless bounded queue/pool trial; no GPU or physical board is needed.
#![allow(
    clippy::print_stdout,
    reason = "allocation trial reports its verified mode"
)]

use fairing::{inbox::Inbox, memory::FixedPool, LaunchAction, Shell, ShellConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (tx, inbox) = Inbox::<u64>::bounded(8)?;
    let mut objects = FixedPool::<u64, 8>::new()?;

    for value in 0..10_000 {
        tx.try_send_with(|| value)?;
        let key = objects
            .try_insert(inbox.try_recv()?)
            .map_err(|_| "object pool full")?;
        if objects.remove(key) != Some(value) {
            return Err("object pool lost a value".into());
        }
    }

    // Exercise the headless GUI separately from the allocation-free queue/pool loop.
    let ctx = fairing::egui::Context::default();
    let mut cfg = ShellConfig::default();
    cfg.access.levels = vec!["device".into()];
    let mut shell = Shell::builder(cfg)
        .command_queue_capacity(8)
        .max_commands_per_update(2)
        .build(&ctx)?;
    let handle = shell.handle();
    for frame in 0..120_u32 {
        handle.try_send_with(|| {
            fairing::shell::Command::Launch(LaunchAction::Set(
                "device.frame".into(),
                fairing::SettingValue::Int(i64::from(frame)),
            ))
        })?;
        let out = ctx.run_ui(
            fairing::egui::RawInput {
                time: Some(f64::from(frame) / 60.0),
                ..Default::default()
            },
            |ui| shell.frame(ui),
        );
        out.drop_without_applying_deltas();
        let _ = shell.poll_events();
    }

    println!("system heap: bounded queue and fixed object pool checks passed");
    Ok(())
}
