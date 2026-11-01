//! Headless fixed-heap/queue/pool trial; no GPU or physical board is needed.
#![allow(
    clippy::print_stdout,
    reason = "allocation trial reports its verified mode"
)]

use fairing::{inbox::Inbox, memory::FixedPool, LaunchAction, Shell, ShellConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "fixed-heap")]
    if std::env::args().any(|arg| arg == "--infallible-exhaust") {
        // Expected to abort only in verification/memory.py's isolated negative control.
        let exhausted = Vec::<u8>::with_capacity(fairing::fixed_heap::HEAP_BYTES + 1);
        std::hint::black_box(exhausted);
        return Err("infallible allocation escaped the fixed arena".into());
    }
    #[cfg(feature = "fixed-heap")]
    {
        let mut too_large = Vec::<u8>::new();
        if too_large
            .try_reserve_exact(fairing::fixed_heap::HEAP_BYTES + 1)
            .is_ok()
        {
            return Err("oversized allocation escaped the fixed arena".into());
        }
    }
    let (tx, inbox) = Inbox::<u64>::bounded(8)?;
    let mut objects = FixedPool::<u64, 8>::new()?;
    #[cfg(feature = "fixed-heap")]
    let before = fairing::fixed_heap::try_stats().ok_or("allocator busy")?;
    for value in 0..10_000 {
        tx.try_send_with(|| value)?;
        let key = objects
            .try_insert(inbox.try_recv()?)
            .map_err(|_| "object pool full")?;
        if objects.remove(key) != Some(value) {
            return Err("object pool lost a value".into());
        }
    }
    #[cfg(feature = "fixed-heap")]
    {
        let after = fairing::fixed_heap::try_stats().ok_or("allocator busy")?;
        if before.total_allocations != after.total_allocations {
            return Err("fixed queue/object reuse allocated during steady state".into());
        }
        // Cumulative requests exceed the arena, exercising reuse rather than a bump-only pool.
        for _ in 0..(fairing::fixed_heap::HEAP_BYTES / 65_536 + 1) * 2 {
            let mut bytes = Vec::<u8>::new();
            bytes.try_reserve_exact(65_536)?;
            bytes.resize(65_536, 7);
            std::hint::black_box(&bytes);
        }
    }
    // Exercise variable-sized GUI allocations inside the same arena as the pools.
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
    #[cfg(feature = "fixed-heap")]
    {
        let stats = fairing::fixed_heap::try_stats().ok_or("allocator busy")?;
        if stats.claimed_bytes > stats.capacity {
            return Err("allocator grew past arena".into());
        }
        println!("fixed-heap: capacity={} claimed={} live={} allocations={}; 10000 queue/pool operations allocated zero times",
            stats.capacity, stats.claimed_bytes, stats.allocated_bytes, stats.total_allocations);
    }
    #[cfg(not(feature = "fixed-heap"))]
    println!("system heap: bounded queue and fixed object pool checks passed");
    Ok(())
}
