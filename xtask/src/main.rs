//! `cargo xtask` — the dependency audit gates and the generated files.
//!
//! This crate has no dependencies (std alone) — so that the audit tool does not grow what it audits.
//!
//! ```text
//! cargo xtask audit [--strict]     stages 0 + 13
//! cargo xtask integrity            build-input integrity (stage 0)
//! cargo xtask deps-check [--write] the deps.allow approval gate
//! cargo xtask dup-check            duplicate versions (against deny.toml [bans].skip)
//! cargo xtask sync-check           the blocking/sync ban (stage 13)
//! cargo xtask licenses [--check]   THIRD_PARTY.md
//! cargo xtask icons [--check]      assets/icons/*.svg → icons/generated.rs
//! ```

// xtask is a CLI a person reads, so it writes to stdout/stderr directly. The library side logs only.
#![allow(clippy::print_stdout, clippy::print_stderr)]
// Quite apart from [lints] workspace = true, the crate pins it down itself.
#![forbid(unsafe_code)]

mod audit;
mod deps;
mod dup;
mod icons;
mod integrity;
mod licenses;
mod path;
mod svg;
mod sync;
mod tomlish;
mod tree;
mod util;
mod version;

use std::process::ExitCode;
use util::{Error, Result};

fn main() -> ExitCode {
    match dispatch() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("xtask: {err}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch() -> Result<bool> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        usage();
        return Ok(false);
    };
    let rest = args.get(1..).unwrap_or(&[]);
    match command.as_str() {
        "help" | "--help" | "-h" => {
            usage();
            Ok(true)
        }
        "audit" => {
            let flags = check_flags(rest, &["--strict"], command)?;
            let root = util::workspace_root()?;
            Ok(audit::run(&root, has(flags, "--strict")))
        }
        "deps-check" => {
            let flags = check_flags(rest, &["--write"], command)?;
            let root = util::workspace_root()?;
            if has(flags, "--write") {
                deps::write(&root)?;
                Ok(true)
            } else {
                deps::check(&root)
            }
        }
        "integrity" => {
            check_flags(rest, &[], command)?;
            let root = util::workspace_root()?;
            integrity::check(&root)
        }
        "dup-check" => {
            check_flags(rest, &[], command)?;
            let root = util::workspace_root()?;
            dup::check(&root)
        }
        "sync-check" => {
            check_flags(rest, &[], command)?;
            let root = util::workspace_root()?;
            sync::check(&root)
        }
        "licenses" => {
            let flags = check_flags(rest, &["--check"], command)?;
            let root = util::workspace_root()?;
            if has(flags, "--check") {
                licenses::check(&root)
            } else {
                licenses::write(&root)?;
                Ok(true)
            }
        }
        "icons" => {
            let flags = check_flags(rest, &["--check"], command)?;
            let root = util::workspace_root()?;
            icons::run(&root, has(flags, "--check"))
        }
        other => {
            eprintln!("xtask: unknown command `{other}`");
            usage();
            Ok(false)
        }
    }
}

/// Check that only the allowed flags came. The argument parsing uses std alone (zero dependencies).
fn check_flags<'a>(rest: &'a [String], allowed: &[&str], command: &str) -> Result<&'a [String]> {
    for arg in rest {
        if !allowed.contains(&arg.as_str()) {
            let expected = if allowed.is_empty() {
                "none".to_owned()
            } else {
                allowed.join(" | ")
            };
            return Err(Error::new(format!(
                "`{command}` got an unknown argument `{arg}` (flags taken: {expected})"
            )));
        }
    }
    Ok(rest)
}

fn has(flags: &[String], name: &str) -> bool {
    flags.iter().any(|flag| flag == name)
}

fn usage() {
    println!(
        "\
cargo xtask <command> [flags]

  audit [--strict]      Run stages 0 + 13. Exit code 1 if any of them fails.
                        The gates that do not compile run first and the compiling stages after.
                        --strict also fails on cargo-audit/cargo-deny missing or at the wrong version (CI).
  integrity             Build-input integrity (stage 0): .cargo/config.toml · vendor/ ·
                        [patch]/[replace] · the members' lints · Cargo.lock's provenance.
  deps-check [--write]  Compare every crate in the tree against deps.allow.
                        --write refreshes deps.allow from the current tree (keeping the existing reasons).
  dup-check             Check that any duplicate version is inside deny.toml [bans].skip.
  sync-check            The blocking/sync ban: it looks for lock types under
                        crates/**/src and blocking calls under crates/fairing/src.
  licenses [--check]    Generate THIRD_PARTY.md. --check only looks at whether it is up to date.
  icons [--check]       assets/icons/*.svg → crates/fairing-widgets/src/icons/generated.rs.
                        --check only looks at whether it is up to date.
  help                  This help.

Target triples: x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu"
    );
}
