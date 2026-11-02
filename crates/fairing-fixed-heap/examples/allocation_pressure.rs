//! Headless allocator-lab/queue/pool trial; no GPU or physical board is needed.
#![allow(
    clippy::print_stdout,
    reason = "allocation trial reports its verified mode"
)]

// Actual product source; no copy of the blur implementation in the failure probe.
#[path = "../../fairing/src/overlay/backdrop.rs"]
mod backdrop;

use fairing::{inbox::Inbox, memory::FixedPool, LaunchAction, Shell, ShellConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "global")]
    if std::env::args().any(|arg| arg == "--infallible-exhaust") {
        // Expected to abort only in verification/memory.py's isolated negative control.
        let exhausted = Vec::<u8>::with_capacity(fairing_fixed_heap::HEAP_BYTES + 1);
        std::hint::black_box(exhausted);
        return Err("infallible allocation escaped the fixed arena".into());
    }
    #[cfg(feature = "global")]
    {
        let mut too_large = Vec::<u8>::new();
        if too_large
            .try_reserve_exact(fairing_fixed_heap::HEAP_BYTES + 1)
            .is_ok()
        {
            return Err("oversized allocation escaped the fixed arena".into());
        }
    }
    let (tx, inbox) = Inbox::<u64>::bounded(8)?;
    let mut objects = FixedPool::<u64, 8>::new()?;
    #[cfg(feature = "global")]
    let before = fairing_fixed_heap::try_stats().ok_or("allocator busy")?;
    for value in 0..10_000 {
        tx.try_send_with(|| value)?;
        let key = objects
            .try_insert(inbox.try_recv()?)
            .map_err(|_| "object pool full")?;
        if objects.remove(key) != Some(value) {
            return Err("object pool lost a value".into());
        }
    }
    #[cfg(feature = "global")]
    {
        let after = fairing_fixed_heap::try_stats().ok_or("allocator busy")?;
        if before.total_allocations != after.total_allocations {
            return Err("fixed queue/object reuse allocated during steady state".into());
        }
        // Cumulative requests exceed the arena, exercising reuse rather than a bump-only pool.
        for _ in 0..(fairing_fixed_heap::HEAP_BYTES / 65_536 + 1) * 2 {
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
    #[cfg(feature = "global")]
    {
        bounded_display_pressure()?;
        recoverable_pressure()?;
        let stats = fairing_fixed_heap::try_stats().ok_or("allocator busy")?;
        if stats.claimed_bytes > stats.capacity {
            return Err("allocator grew past arena".into());
        }
        println!("allocator-lab: capacity={} claimed={} live={} allocations={}; 10000 queue/pool operations allocated zero times",
            stats.capacity, stats.claimed_bytes, stats.allocated_bytes, stats.total_allocations);
    }
    #[cfg(not(feature = "global"))]
    println!("system heap: bounded queue and fixed object pool checks passed");
    Ok(())
}

/// Exhaust the real diagnostic arena, exercise fallible display paths and resume after release.
#[cfg(feature = "global")]
fn recoverable_pressure() -> Result<(), Box<dyn std::error::Error>> {
    use fairing::{
        inbox::TryBuildSendError,
        notify::{
            AdmissionError, Notification, NotificationCenter, NotificationId, Toast, ToastQueue,
        },
    };
    use std::time::{Duration, Instant};
    let text = "x".repeat(fairing::notify::MAX_TOAST_TEXT_BYTES);
    let theme = fairing::Theme::default();
    let now = Instant::now();
    let (work, receive) = Inbox::<u64>::bounded(2)?;
    let (display, display_rx) = Inbox::<Toast>::bounded(2)?;
    let mut center = NotificationCenter::new(1);
    center.try_push(Notification::new(NotificationId(1), "retained").persistent())?;
    let next = Notification::new(NotificationId(2), "rejected ownership");
    let mut toasts = ToastQueue::new(1, Duration::from_secs(1)).with_pending_capacity(2);
    toasts.try_push_text("waiting")?;
    let mut ballast: Vec<Vec<u8>> = Vec::new();
    ballast.try_reserve_exact(1024)?;
    ballast.resize_with(1024, Vec::new);
    let mut minimum_failed = false;
    'fill: for buffer in &mut ballast {
        let mut size = fairing_fixed_heap::try_stats()
            .ok_or("allocator busy")?
            .available_bytes
            .max(2)
            / 2;
        loop {
            if buffer.try_reserve_exact(size).is_ok() {
                break;
            }
            if size <= 1 {
                minimum_failed = true;
                break 'fill;
            }
            size /= 2;
        }
    }
    // No formatting, logging or boxed error conversion while the arena is exhausted.
    let failed_text = matches!(Toast::try_new(&text), Err(AdmissionError::AllocationFailed));
    let failed_factory = matches!(
        display.try_send_fallible_with(|| Toast::try_new(&text)),
        Err(TryBuildSendError::Build(AdmissionError::AllocationFailed))
    );
    let returned_slot = display.stats().outstanding == 0;
    let retained = center.try_push(next);
    let rejected = retained
        .as_ref()
        .is_err_and(|error| error.kind() == AdmissionError::Full);
    let continued = work.try_send(42).is_ok() && receive.try_recv() == Ok(42);
    let _ = toasts.tick(0.0, now, &theme.motion, theme.components.toast);
    let deferred = toasts.visible().is_empty()
        && toasts.pending() == 1
        && toasts.next_deadline() == now.checked_add(Duration::from_millis(100));
    let _ = toasts.tick(
        0.0,
        now + Duration::from_millis(50),
        &theme.motion,
        theme.components.toast,
    );
    let no_early_retry = toasts.visible().is_empty() && toasts.pending() == 1;
    let updated = center
        .try_push(Notification::new(NotificationId(1), String::new()).persistent())
        .is_ok();
    drop(ballast);
    if !(updated
        && minimum_failed
        && failed_text
        && failed_factory
        && returned_slot
        && rejected
        && continued
        && deferred
        && no_early_retry)
    {
        return Err("recoverable pressure path failed".into());
    }
    display.try_send_fallible_with(|| Toast::try_new("recovered"))?;
    if display_rx.try_recv()?.text != "recovered" {
        return Err("factory did not recover".into());
    }
    let _ = toasts.tick(
        0.0,
        now + Duration::from_millis(100),
        &theme.motion,
        theme.components.toast,
    );
    if toasts.visible().len() != 1 || toasts.pending() != 0 {
        return Err("deferred display did not resume".into());
    }
    println!("recoverable-pressure: allocation refused; bounded work continued; deferred display resumed");
    Ok(())
}

/// Same-process, actual exhausted-arena checks for bounded display admission and CPU rendering.
#[cfg(feature = "global")]
#[allow(
    clippy::too_many_lines,
    reason = "real-pressure integration fixture keeps the exhausted interval explicit"
)]
fn bounded_display_pressure() -> Result<(), Box<dyn std::error::Error>> {
    use fairing::memory::{
        display_warning_tile, paint_display_warning, paint_image_preview, DisplayWindow,
    };
    use fairing::notify::{AdmissionError, OverflowPolicy, ToastQueue};
    use std::time::Duration;
    let samples: Vec<u32> = (0..10000).collect();
    let source = vec![128u8; 512 * 512 * 4];
    let screenshot = egui::ColorImage::new([16, 16], vec![egui::Color32::WHITE; 256]);
    let old_text = "o".repeat(4096);
    let new_text = "n".repeat(4096);
    let oversized = "x".repeat(4097);
    let mut heads_up = fairing::notify::HeadsUp::new(88.0);
    let banner_note =
        fairing::notify::Notification::new(fairing::notify::NotificationId(50), new_text.clone());
    let banner_tokens = fairing::Theme::default().motion;
    let banner_now = std::time::Instant::now();
    let mut window = DisplayWindow::<u32, 32>::new();
    let mut pixels = [0u8; 64 * 64 * 4];
    let mut warning = [0u8; 96 * 12 * 4];
    let mut recent = ToastQueue::new(1, Duration::from_secs(1))
        .with_pending_capacity(1)
        .with_overflow_policy(OverflowPolicy::KeepLatest);
    recent.try_push_text(&old_text)?;
    let mut empty = ToastQueue::new(1, Duration::from_secs(1))
        .with_pending_capacity(1)
        .with_overflow_policy(OverflowPolicy::KeepLatest);
    let context = egui::Context::default();
    let mut shell = fairing::Shell::builder(fairing::testing::single_level_access())
        .command_queue_capacity(2)
        .build(&context)?;
    let handle = shell.handle();
    let (work, receive) = fairing::inbox::Inbox::<u32>::bounded(2)?;
    let mut ballast: Vec<Vec<u8>> = Vec::new();
    ballast.try_reserve_exact(1024)?;
    ballast.resize_with(1024, Vec::new);
    let mut exhausted = false;
    'fill: for buffer in &mut ballast {
        let mut bytes = fairing_fixed_heap::try_stats()
            .ok_or("allocator busy")?
            .available_bytes
            .max(2)
            / 2;
        loop {
            if buffer.try_reserve_exact(bytes).is_ok() {
                break;
            }
            if bytes <= 1 {
                exhausted = true;
                break 'fill;
            }
            bytes /= 2;
        }
    }
    // From here to ballast release: no formatting/boxing or new fixture allocation.
    let before = fairing_fixed_heap::try_stats().map(|s| s.total_allocations);
    let status = window.extend(&samples);
    let clipped = status.discarded == 9968 && window.recent(4).copied().eq(9996..10000);
    let preview = paint_image_preview(&source, [512, 512], &mut pixels, [64, 64], 4096)
        .is_ok_and(fairing::memory::DisplayStatus::is_reduced);
    let warned = paint_display_warning(&mut warning, [96, 12], 96 * 12).is_ok()
        && warning
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p == &[255, 208, 64, 255]);
    let (_, prepared) = display_warning_tile();
    let prepared_matches = warning.as_chunks::<4>().0 == prepared;
    let omitted = empty.try_push_text(&new_text) == Err(AdmissionError::AllocationFailed)
        && empty.take_display_status().allocation_failed;
    shell.toast(new_text.as_str());
    let local_omitted = shell.take_display_status().allocation_failed;
    handle.toast(&new_text);
    let convenience_omitted = local_omitted
        && shell.take_display_status().allocation_failed
        && handle.queue_stats().outstanding == 0;
    heads_up.show(&banner_note, banner_now, &banner_tokens);
    let banner_omitted =
        heads_up.visible().is_none() && heads_up.take_display_status().allocation_failed;
    let blur_omitted = matches!(
        backdrop::frost(&screenshot),
        Err(backdrop::FrostError::AllocationFailed)
    );
    let mut continued = true;
    for i in 0..10000u32 {
        continued &= work.try_send(i).is_ok() && receive.try_recv() == Ok(i);
    }
    let after = fairing_fixed_heap::try_stats().map(|s| s.total_allocations);
    let zero_allocations = before.is_some() && before == after;
    // The oldest pending text is actually freed before constructing its replacement.
    let replaced = recent.try_push_text(&new_text).is_ok()
        && recent.pending() == 1
        && recent.take_display_status().discarded == 1;
    let rejected_oversize = recent.try_push_text(&oversized) == Err(AdmissionError::TooLarge)
        && recent.pending() == 1
        && recent.take_display_status().discarded == 1;
    drop(ballast);
    if !(exhausted
        && clipped
        && preview
        && warned
        && prepared_matches
        && omitted
        && convenience_omitted
        && blur_omitted
        && banner_omitted
        && continued
        && zero_allocations
        && replaced
        && rejected_oversize)
    {
        return Err("bounded display pressure invariant failed".into());
    }
    println!("bounded-display: actual one-byte exhaustion; oldest text freed/replaced; preview clipped; warning raster painted; failed banner/blur/text omitted; 10000 work commands continued; zero allocations in clipped preview/warning/work interval");
    Ok(())
}
