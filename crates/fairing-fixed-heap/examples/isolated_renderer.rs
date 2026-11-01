//! Headless real-egui subprocess fault fixture. stdout belongs exclusively to IPC.
use fairing::egui;
use fairing_process::{Error, RenderToken, Worker};
use std::time::{Duration, Instant};

fn exhaust() -> Result<Vec<Vec<u8>>, &'static str> {
    let mut buffers = Vec::new();
    buffers.try_reserve_exact(1024).map_err(|_| "bootstrap")?;
    buffers.resize_with(1024, Vec::<u8>::new);
    for buffer in &mut buffers {
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
                return Ok(buffers);
            }
            bytes /= 2;
        }
    }
    Err("not exhausted")
}

fn raw_packet(
    worker: &Worker,
    kind: u8,
    context: u64,
    sequence: u64,
    payload: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;
    let mut header = [0u8; 40];
    header
        .get_mut(..4)
        .ok_or(std::io::ErrorKind::InvalidInput)?
        .copy_from_slice(b"FRP1");
    *header.get_mut(4).ok_or(std::io::ErrorKind::InvalidInput)? = 1;
    *header.get_mut(5).ok_or(std::io::ErrorKind::InvalidInput)? = kind;
    header
        .get_mut(8..16)
        .ok_or(std::io::ErrorKind::InvalidInput)?
        .copy_from_slice(&worker.generation().to_le_bytes());
    header
        .get_mut(16..24)
        .ok_or(std::io::ErrorKind::InvalidInput)?
        .copy_from_slice(&context.to_le_bytes());
    header
        .get_mut(24..32)
        .ok_or(std::io::ErrorKind::InvalidInput)?
        .copy_from_slice(&sequence.to_le_bytes());
    header
        .get_mut(32..36)
        .ok_or(std::io::ErrorKind::InvalidInput)?
        .copy_from_slice(
            &u32::try_from(payload.len())
                .map_err(|_| std::io::ErrorKind::InvalidInput)?
                .to_le_bytes(),
        );
    let mut output = std::io::stdout().lock();
    output.write_all(&header)?;
    output.write_all(payload)?;
    output.flush()
}

#[allow(
    clippy::too_many_lines,
    reason = "isolated fault-injection fixture, not a production renderer"
)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Must precede egui/font initialization and application model parsing.
    let mut worker = Worker::connect()?;
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "start-hang" {
        loop {
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
    if mode == "oversize" {
        use std::io::Write;
        let mut header = [0u8; 40];
        header
            .get_mut(..4)
            .ok_or("header")?
            .copy_from_slice(b"FRP1");
        *header.get_mut(4).ok_or("header")? = 1;
        *header.get_mut(5).ok_or("header")? = 5;
        header
            .get_mut(8..16)
            .ok_or("header")?
            .copy_from_slice(&worker.generation().to_le_bytes());
        header
            .get_mut(16..24)
            .ok_or("header")?
            .copy_from_slice(&worker.context().to_le_bytes());
        header
            .get_mut(32..36)
            .ok_or("header")?
            .copy_from_slice(&4097u32.to_le_bytes());
        std::io::stdout().write_all(&header)?;
        std::io::stdout().flush()?;
        loop {
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
    let ctx = egui::Context::default();
    let mut handled = 0usize;
    let mut token: Option<RenderToken> = None;
    let mut pending = None;
    let mut flood = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    while !worker.is_closed() && Instant::now() < deadline {
        if let Some(frame) = pending.take() {
            match worker.try_commit(frame) {
                Ok(()) => {}
                Err(error) if error.kind() == Error::Full => {
                    pending = Some(error.into_frame());
                }
                Err(error) => return Err(error.into()),
            }
        }
        if pending.is_some() {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        if let Some(snapshot) = worker.try_snapshot() {
            if mode == "partial" {
                use std::io::Write;
                std::io::stdout().write_all(b"FRP1\x01\x05\x00")?;
                std::io::stdout().flush()?;
                loop {
                    std::thread::park_timeout(Duration::from_secs(1));
                }
            }
            if mode == "replay" || mode == "bad-context" {
                raw_packet(
                    &worker,
                    5,
                    worker.context(),
                    snapshot.token().sequence(),
                    &[],
                )?;
                let mut payload = [0u8; 16];
                payload
                    .get_mut(..8)
                    .ok_or("payload")?
                    .copy_from_slice(&snapshot.token().sequence().to_le_bytes());
                payload
                    .get_mut(8..)
                    .ok_or("payload")?
                    .copy_from_slice(snapshot.bytes());
                let context = if mode == "bad-context" {
                    worker.context().checked_add(1).ok_or("context")?
                } else {
                    worker.context()
                };
                raw_packet(&worker, 6, context, 1, &payload)?;
                raw_packet(&worker, 6, context, 1, &payload)?;
                loop {
                    std::thread::park_timeout(Duration::from_secs(1));
                }
            }
            let input = egui::RawInput::default();
            let mut output = ctx.run_ui(input, |ui| {
                ui.label("Independent renderer");
                let _ = ui.button("Intent");
            });
            let shapes = std::mem::take(&mut output.shapes);
            output.drop_without_applying_deltas();
            let first_generation = worker.generation() == 1;
            if first_generation && handled > 0 && mode == "ui-oom-once" {
                let input = egui::RawInput::default();
                let ballast = exhaust()?;
                let output = ctx.run_ui(input, |ui| {
                    ui.label("Independent renderer");
                });
                output.drop_without_applying_deltas();
                drop(ballast);
                return Err("UI unexpectedly survived exhaustion".into());
            }
            if first_generation && handled > 0 && mode == "tessellate-oom-once" {
                let ballast = exhaust()?;
                let shapes = ctx.tessellate(shapes, 1.0);
                std::hint::black_box(shapes);
                drop(ballast);
                return Err("tessellation unexpectedly survived exhaustion".into());
            }
            if first_generation && handled > 0 && mode == "hang-once" {
                loop {
                    std::thread::park_timeout(Duration::from_secs(1));
                }
            }
            std::hint::black_box(ctx.tessellate(shapes, 1.0));
            token = Some(snapshot.token());
            let mut frame = worker.frame(token)?;
            // Echo bytes only; the work engine independently checks the application schema.
            frame.try_action(snapshot.bytes())?;
            pending = Some(frame);
            handled += 1;
            flood = mode == "flood";
        } else if flood {
            let mut frame = worker.frame(token)?;
            for _ in 0..fairing_process::MAX_FRAME_ACTIONS {
                frame.try_action(b"flood")?;
            }
            pending = Some(frame);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}
