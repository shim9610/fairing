//! Minimal engine with a separate visible UI: `work_engine /path/to/process_renderer [seconds]`.
#![allow(clippy::print_stdout, reason = "standalone example summary")]
use fairing_process::{Config, Error, Host, WorkerCommand};
use std::time::{Duration, Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("expected renderer executable path")?;
    let seconds = std::env::args().nth(2).map_or(Ok(30u64), |s| s.parse())?;
    let mut display = Host::new(
        WorkerCommand::new(path).inherit_stderr(),
        Config {
            // Example only: GPU driver mappings may need a larger virtual address budget.
            address_space_bytes: 1024 * 1024 * 1024,
            ..Config::default()
        },
    )?;
    let until = Instant::now()
        .checked_add(Duration::from_secs(seconds))
        .ok_or("duration")?;
    let mut ticks = 0u64;
    let mut reported_pid = 0;
    let mut acknowledged = 0u64;
    while Instant::now() < until {
        ticks = ticks.checked_add(1).ok_or("counter")?;
        let pid = display.stats().child_pid;
        if pid != 0 && pid != reported_pid {
            println!("renderer_pid={pid}");
            reported_pid = pid;
        }
        match display.try_publish(&ticks.to_le_bytes()) {
            Ok(_) | Err(Error::Full | Error::Disconnected) => {}
            Err(error) => return Err(error.into()),
        }
        while let Some(intent) = display.try_action() {
            // Explicit demo schema validation; real device operations must also re-authorize.
            if intent.bytes().first() != Some(&1) {
                continue;
            }
            let Ok(bytes) = <[u8; 8]>::try_from(intent.bytes().get(1..).unwrap_or_default()) else {
                continue;
            };
            let observed = u64::from_le_bytes(bytes);
            if observed <= ticks {
                acknowledged = observed;
            }
        }
        // A device engine schedules acquisition/control here, independently of egui.
        std::thread::sleep(Duration::from_millis(20));
    }
    let stats = display.stats();
    println!("work_ticks={ticks} acknowledged={acknowledged} renderer={stats:?}");
    Ok(())
}
