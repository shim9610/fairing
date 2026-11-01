//! Work-engine fixture: deliberately has no egui or diagnostic allocator dependency.
#![allow(clippy::print_stdout, reason = "machine-readable diagnostic report")]
use fairing_process::{Config, Error, Host, State, WorkerCommand};
use std::time::{Duration, Instant};

fn memory(pid: u32) -> (u64, u64, u64) {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let read = |key: &str| {
        status
            .lines()
            .find_map(|line| {
                line.strip_prefix(key)?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
            .unwrap_or(0)
    };
    (read("VmRSS:"), read("VmSize:"), read("Threads:"))
}
#[allow(
    clippy::too_many_lines,
    reason = "bounded integration fixture and metrics, not product code"
)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let worker_path = std::env::args_os().nth(1).ok_or("worker path")?;
    let mode = std::env::args().nth(2).ok_or("mode")?;
    let command = WorkerCommand::new(worker_path).arg(&mode)?;
    let mut host = Host::new(
        command,
        Config {
            address_space_bytes: 128 * 1024 * 1024,
            startup_timeout: Duration::from_millis(700),
            snapshot_timeout: Duration::from_millis(400),
            restart_delay: Duration::from_millis(40),
            max_restarts: if mode == "start-hang"
                || mode == "oversize"
                || mode == "flood"
                || mode == "partial"
                || mode == "replay"
                || mode == "bad-context"
            {
                1
            } else {
                3
            },
        },
    )?;
    let start = Instant::now();
    let deadline = start + Duration::from_secs(8);
    let mut ticks = 0u64;
    let mut failed_ticks = 0u64;
    let mut accepted = 0u64;
    let mut recovered = false;
    let mut initial_generation = 0;
    let mut first_failure = None;
    let mut recovery_us = 0;
    let mut max_parent_rss = 0;
    let mut max_child_rss = 0;
    let mut max_child_virtual = 0;
    let mut max_parent_threads = 0;
    let mut max_child_threads = 0;
    let mut max_api_us = 0;
    let mut max_rtt_us = 0;
    let mut stale_actions = 0;
    let mut invalidated = false;
    let mut previous_sent = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .ok_or("clock")?;
    while Instant::now() < deadline {
        // Stands for critical work scheduled independently of UI frame callbacks.
        ticks += 1;
        let stats = host.stats();
        if stats.failures > 0 && first_failure.is_none() {
            first_failure = Some(Instant::now());
        }
        if matches!(
            stats.state,
            State::Starting | State::Stopping | State::Backoff | State::Quarantined
        ) {
            failed_ticks += 1;
        }
        if stats.generation != 0 && initial_generation == 0 {
            initial_generation = stats.generation;
        }
        if previous_sent.elapsed() >= Duration::from_millis(20) && stats.state != State::Quarantined
        {
            let now_us = u64::try_from(start.elapsed().as_micros()).map_err(|_| "clock")?;
            let call = Instant::now();
            match host.try_publish(&now_us.to_le_bytes()) {
                Ok(_) => previous_sent = Instant::now(),
                Err(Error::Full) => {}
                Err(error) => return Err(error.into()),
            }
            max_api_us = max_api_us.max(call.elapsed().as_micros());
        }
        if mode != "flood" {
            while let Some(action) = host.try_action() {
                if action.context() != host.stats().context {
                    stale_actions += 1;
                }
                // Application schema and authorization: accept exactly our own timestamp echo.
                let data: [u8; 8] = action
                    .bytes()
                    .try_into()
                    .map_err(|_| "invalid app intent")?;
                let sent = u64::from_le_bytes(data);
                let elapsed = u64::try_from(start.elapsed().as_micros()).map_err(|_| "clock")?;
                if sent > elapsed {
                    return Err("future timestamp".into());
                }
                max_rtt_us = max_rtt_us.max(elapsed - sent);
                accepted += 1;
                if action.generation() > initial_generation {
                    recovered = true;
                    recovery_us = first_failure.map_or(0, |at| at.elapsed().as_micros());
                }
            }
        }
        if mode == "context" && accepted >= 2 && !invalidated {
            // Discard queued old-session actions immediately, before the actor processes the wake.
            host.invalidate_context()?;
            invalidated = true;
            if host.try_action().is_some() {
                return Err("old-context action escaped".into());
            }
        }
        if ticks.is_multiple_of(10) {
            let (rss, _, threads) = memory(std::process::id());
            max_parent_rss = max_parent_rss.max(rss);
            max_parent_threads = max_parent_threads.max(threads);
            let (rss, virtual_bytes, threads) = memory(stats.child_pid);
            max_child_rss = max_child_rss.max(rss);
            max_child_virtual = max_child_virtual.max(virtual_bytes);
            max_child_threads = max_child_threads.max(threads);
        }
        let done = match mode.as_str() {
            "normal" => accepted >= 20,
            "context" => recovered && invalidated && accepted >= 4,
            "ui-oom-once" | "tessellate-oom-once" | "hang-once" => recovered && accepted >= 4,
            "start-hang" | "oversize" | "flood" | "partial" | "replay" | "bad-context" => {
                stats.state == State::Quarantined
            }
            _ => return Err("unknown fixture mode".into()),
        };
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let stats = host.stats();
    if ticks == 0 || accepted == 0 && stats.state != State::Quarantined || stale_actions != 0 {
        return Err("work/action invariant".into());
    }
    if mode == "bad-context" && accepted != 0 {
        return Err("wrong-context intent reached engine".into());
    }
    if mode == "replay" && accepted > stats.starts {
        return Err("duplicate intent reached engine".into());
    }
    match mode.as_str() {
        "normal" if accepted >= 20 && stats.failures == 0 => {}
        "context" if recovered && invalidated && stats.failures == 0 => {}
        "ui-oom-once" | "tessellate-oom-once"
            if recovered && stats.failures == 1 && stats.last_exit_signal == 6 => {}
        "hang-once"
            if recovered
                && stats.failures == 1
                && stats.last_fault == fairing_process::Fault::Timeout => {}
        "start-hang" | "partial"
            if stats.state == State::Quarantined
                && stats.starts == 2
                && stats.last_fault == fairing_process::Fault::Timeout => {}
        "oversize" | "replay" | "bad-context"
            if stats.state == State::Quarantined
                && stats.starts == 2
                && stats.last_fault == fairing_process::Fault::Protocol => {}
        "flood"
            if stats.state == State::Quarantined
                && stats.starts == 2
                && stats.last_fault == fairing_process::Fault::ActionOverflow => {}
        _ => return Err(format!("isolation fixture failed: {mode} {stats:?}").into()),
    }
    println!("{{\"mode\":\"{mode}\",\"ticks\":{ticks},\"unavailable_ticks\":{failed_ticks},\"accepted\":{accepted},\"starts\":{},\"failures\":{},\"last_signal\":{},\"recovered\":{recovered},\"recovery_us\":{recovery_us},\"parent_rss_kib\":{max_parent_rss},\"child_rss_kib\":{max_child_rss},\"child_virtual_kib\":{max_child_virtual},\"parent_threads\":{max_parent_threads},\"child_threads\":{max_child_threads},\"max_api_us\":{max_api_us},\"max_rtt_us\":{max_rtt_us}}}", stats.starts, stats.failures, stats.last_exit_signal);
    let pid = stats.child_pid;
    let dropping = Instant::now();
    drop(host);
    if dropping.elapsed() > Duration::from_millis(100) {
        return Err("host drop waited".into());
    }
    let cleanup_deadline = Instant::now() + Duration::from_secs(2);
    while pid != 0 && std::path::Path::new(&format!("/proc/{pid}")).exists() {
        if Instant::now() >= cleanup_deadline {
            return Err("child not reaped".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
