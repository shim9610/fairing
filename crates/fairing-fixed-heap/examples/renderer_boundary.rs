//! Isolated negative probes for stock egui allocation boundaries. Never a production allocator.
#![allow(clippy::print_stdout, reason = "diagnostic subprocess protocol")]
use fairing::egui;
use std::io::Write;

fn exhaust() -> Result<Vec<Vec<u8>>, &'static str> {
    let mut ballast = Vec::new();
    ballast
        .try_reserve_exact(1024)
        .map_err(|_| "ballast bootstrap failed")?;
    ballast.resize_with(1024, Vec::<u8>::new);
    for buffer in &mut ballast {
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
                return Ok(ballast);
            }
            bytes /= 2;
        }
    }
    Err("arena did not reach exhaustion")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stage = std::env::args().nth(1).unwrap_or_default();
    if stage == "mesh-clear" {
        let mut mesh = egui::epaint::Mesh::default();
        mesh.vertices.try_reserve_exact(4)?;
        mesh.indices.try_reserve_exact(6)?;
        mesh.vertices.push(egui::epaint::Vertex::default());
        let before = mesh.vertices.capacity();
        mesh.clear();
        println!(
            "mesh-clear: before={before} after={}",
            mesh.vertices.capacity()
        );
        return Ok(());
    }
    if stage != "ui" && stage != "tessellate" {
        return Err("expected ui, tessellate or mesh-clear".into());
    }
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.label("Prewarmed text");
        let _ = ui.button("Prepared button");
    });
    let shapes = std::mem::take(&mut output.shapes);
    output.drop_without_applying_deltas();
    let input = egui::RawInput::default();
    // Initialize and flush stdout before exhaustion. Application inputs are prepared already.
    println!("boundary-enter: {stage}");
    std::io::stdout().flush()?;
    let ballast = exhaust()?;
    if stage == "ui" {
        // No caller-owned String construction/formatting in this closure.
        let next = ctx.run_ui(input, |ui| {
            ui.label("Prewarmed text");
        });
        next.drop_without_applying_deltas();
    } else {
        let next = ctx.tessellate(shapes, 1.0);
        std::hint::black_box(next);
    }
    drop(ballast);
    println!("boundary-returned: {stage}");
    Ok(())
}
