//! Validate the diagnostic binary's fixed arena size before compiling it.
#![forbid(unsafe_code)]
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=FAIRING_HEAP_BYTES");
    // Development budget for the workspace's parallel renderer tests. Diagnostic
    // runs can select a different measured budget.
    let bytes =
        env::var("FAIRING_HEAP_BYTES").map_or(Ok(512 * 1024 * 1024), |s| s.parse::<usize>())?;
    if !(128 * 1024..=1024 * 1024 * 1024).contains(&bytes) {
        return Err("FAIRING_HEAP_BYTES must be 128 KiB..=1 GiB".into());
    }
    let digits = bytes.to_string();
    let mut literal = String::new();
    for (index, digit) in digits.char_indices() {
        if index != 0 && (digits.len() - index) % 3 == 0 {
            literal.push('_');
        }
        literal.push(digit);
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    fs::write(out.join("heap_size.rs"), format!("/// Fixed arena byte budget selected at build time.\npub const HEAP_BYTES: usize = {literal};\n"))?;
    Ok(())
}
