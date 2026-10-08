//! `cargo xtask audit` — it runs the audit stages and prints a result table.
//!
//! **The order of execution is not the table's numbering.** The checks that do not compile
//! (0 · 13 · 5 · 6 · 10 · 11 · 7 · 8) run first and the compiling stages (1–4 · 12) after. Otherwise
//! an unapproved crate's `build.rs` and proc-macros would run before the approval gate reaches its
//! decision. The table's numbers are fixed stage identifiers, and the result table is sorted
//! back into numeric order to show.
//!
//! Where 0 (integrity) fails, the compiling stages do not run at all — in that state the source
//! being compiled may differ from what `Cargo.toml` / `Cargo.lock` says.
//!
//! 7 and 8 are installed plugins (`cargo-audit`, `cargo-deny`) rather than built into cargo. Absent,
//! they print installation guidance and are skipped, and under `--strict` (CI) they count as failures.
//! 9 (`cargo update --dry-run`) only reports and never counts as a failure.
//!
//! A stage that calls cargo passes `--locked`. Where a PR's stale lock file is quietly reinterpreted
//! on the runner, the tree audited and the tree committed differ.

use crate::util::{self, Result};
use crate::{deps, dup, icons, integrity, licenses, sync, tree};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Pass,
    Fail,
    Skipped,
    Report,
}

impl Outcome {
    fn tag(&self) -> &'static str {
        match *self {
            Self::Pass => "ok",
            Self::Fail => "FAIL",
            Self::Skipped => "SKIP",
            Self::Report => "note",
        }
    }
}

struct Step {
    number: usize,
    title: &'static str,
    command: String,
    outcome: Outcome,
}

const TOTAL: usize = 13;

/// The whole audit. `false` if any of them fails.
pub(crate) fn run(root: &Path, strict: bool) -> bool {
    let cargo = util::cargo_bin();
    println!(
        "cargo xtask audit — stages 0 + {TOTAL}, root {}",
        root.display()
    );
    println!(
        "Order of execution: the gates that do not compile (0·13·5·6·10·11·7·8) → compiling (1·2·3·4·12) → reporting (9)."
    );
    if strict {
        println!("--strict: a missing plugin and a version mismatch count as failures too.");
    }
    let mut steps = gates(root, strict);
    if steps
        .iter()
        .any(|step| step.number == 0 && step.outcome == Outcome::Fail)
    {
        println!(
            "\nIntegrity (0) failed. The compiling stages (1·2·3·4·12) will not run — \n\
             compiling in that state is running code Cargo.toml does not speak for."
        );
    } else {
        steps.extend(builds(root, &cargo));
    }
    steps.push(report(
        9,
        "make updates visible",
        root,
        &cargo,
        &["update", "--dry-run"],
    ));
    steps.sort_by_key(|step| step.number);
    summarize(&steps);
    !steps.iter().any(|step| step.outcome == Outcome::Fail)
}

/// The stages that do not compile: integrity · blocking/sync · duplicate versions · the allowlist ·
/// the notices file · the generated files · the two plugins.
///
/// 13 (blocking/sync) comes right after 0. It is only a text scan and so is cheap, and on a tree that
/// has taken a lock in it is better to say so before spending time on the compiling stages.
fn gates(root: &Path, strict: bool) -> Vec<Step> {
    vec![
        internal(0, "integrity", "xtask integrity", || integrity::check(root)),
        internal(13, "blocking/sync", "xtask sync-check", || {
            sync::check(root)
        }),
        internal(5, "duplicate versions", "xtask dup-check", || {
            dup::check(root)
        }),
        internal(6, "allowlist", "xtask deps-check", || deps::check(root)),
        internal(10, "notices file", "xtask licenses --check", || {
            licenses::check(root)
        }),
        internal(11, "generated files", "xtask icons --check", || {
            icons::run(root, true)
        }),
        plugin(
            7,
            "vulnerability advisories",
            root,
            "audit",
            "cargo-audit",
            &["audit", "--deny", "warnings"],
            strict,
        ),
        plugin(
            8,
            "licences · sources · bans",
            root,
            "deny",
            "cargo-deny",
            &["deny", "check"],
            strict,
        ),
    ]
}

/// The compiling stages: formatting · lints · tests · docs · the target build.
fn builds(root: &Path, cargo: &str) -> Vec<Step> {
    let target = tree::TARGETS
        .get(1)
        .copied()
        .unwrap_or("aarch64-unknown-linux-gnu");
    vec![
        external(
            1,
            "formatting",
            root,
            cargo,
            &["fmt", "--all", "--check"],
            &[],
        ),
        external(
            2,
            "lints",
            root,
            cargo,
            &[
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
            &[],
        ),
        external(
            3,
            "tests",
            root,
            cargo,
            &["test", "--locked", "--workspace", "--all-features"],
            &[],
        ),
        external(
            4,
            "docs",
            root,
            cargo,
            &[
                "doc",
                "--locked",
                "--workspace",
                "--no-deps",
                "--all-features",
            ],
            &[("RUSTDOCFLAGS", "-D warnings")],
        ),
        external(
            12,
            "target build",
            root,
            cargo,
            &[
                "check",
                "--locked",
                "-p",
                "fairing",
                "--features",
                "runner",
                "--target",
                target,
            ],
            &[],
        ),
    ]
}

fn banner(number: usize, title: &str, command: &str) {
    println!("\n── [{number:>2}/{TOTAL}] {title} · {command}");
}

/// The names kept so the result table can explain the compiling stages that were skipped.
const BUILD_STEPS: [usize; 5] = [1, 2, 3, 4, 12];

/// One external-command stage.
fn external(
    number: usize,
    title: &'static str,
    root: &Path,
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Step {
    let command = util::display_command(program, args);
    banner(number, title, &command);
    let outcome = match util::run(root, program, args, envs) {
        Ok(true) => Outcome::Pass,
        Ok(false) => Outcome::Fail,
        Err(err) => {
            println!("error: {err}");
            Outcome::Fail
        }
    };
    Step {
        number,
        title,
        command,
        outcome,
    }
}

/// A stage that runs inside xtask. It takes the work as a closure so the banner can be printed first.
fn internal<F>(number: usize, title: &'static str, command: &str, work: F) -> Step
where
    F: FnOnce() -> Result<bool>,
{
    banner(number, title, command);
    let outcome = match work() {
        Ok(true) => Outcome::Pass,
        Ok(false) => Outcome::Fail,
        Err(err) => {
            println!("error: {err}");
            Outcome::Fail
        }
    };
    Step {
        number,
        title,
        command: command.to_owned(),
        outcome,
    }
}

/// A stage that only prints and never counts as a failure (9).
fn report(number: usize, title: &'static str, root: &Path, program: &str, args: &[&str]) -> Step {
    let command = util::display_command(program, args);
    banner(number, title, &command);
    if let Err(err) = util::run(root, program, args, &[]) {
        println!("(report only) could not run: {err}");
    }
    Step {
        number,
        title,
        command,
        outcome: Outcome::Report,
    }
}

/// An installed-plugin stage (7 · 8).
fn plugin(
    number: usize,
    title: &'static str,
    root: &Path,
    subcommand: &str,
    crate_name: &str,
    args: &[&str],
    strict: bool,
) -> Step {
    let cargo = util::cargo_bin();
    let command = util::display_command(&cargo, args);
    banner(number, title, &command);
    let pinned = pinned_version(root, crate_name);
    let install = pinned.as_ref().map_or_else(
        || format!("cargo install {crate_name} --locked"),
        |version| format!("cargo install {crate_name} --locked --version {version}"),
    );

    let Some(found) = util::plugin_version(root, subcommand) else {
        println!("{crate_name} is not installed. To install: {install}");
        return Step {
            number,
            title,
            command,
            outcome: if strict {
                Outcome::Fail
            } else {
                Outcome::Skipped
            },
        };
    };
    let mut mismatch = false;
    if let Some(version) = &pinned {
        if &found != version {
            mismatch = true;
            println!(
                "warning: the installed {crate_name} {found} differs from {version} in xtask/tools.lock. To match it: {install}"
            );
        }
    }
    let outcome = match util::run(root, &cargo, args, &[]) {
        Ok(true) if mismatch && strict => Outcome::Fail,
        Ok(true) => Outcome::Pass,
        Ok(false) => Outcome::Fail,
        Err(err) => {
            println!("error: {err}");
            Outcome::Fail
        }
    };
    Step {
        number,
        title,
        command,
        outcome,
    }
}

/// The versions `xtask/tools.lock` pins, the single source for tool versions.
fn pinned_version(root: &Path, crate_name: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join("xtask").join("tools.lock")).ok()?;
    parse_tools_lock(&text)
        .into_iter()
        .find(|(name, _)| name == crate_name)
        .map(|(_, version)| version)
}

/// Read `tools.lock` as a list of `(crate, version)`.
pub(crate) fn parse_tools_lock(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        if let (Some(name), Some(version)) = (parts.next(), parts.next()) {
            out.push((name.to_owned(), version.to_owned()));
        }
    }
    out
}

fn summarize(steps: &[Step]) {
    println!("\n── Results");
    println!("  #  state  stage · command");
    for step in steps {
        println!(
            "  {:>2} {:<5} {} · {}",
            step.number,
            step.outcome.tag(),
            step.title,
            step.command
        );
    }
    let failed: Vec<usize> = steps
        .iter()
        .filter(|step| step.outcome == Outcome::Fail)
        .map(|step| step.number)
        .collect();
    let skipped: Vec<usize> = steps
        .iter()
        .filter(|step| step.outcome == Outcome::Skipped)
        .map(|step| step.number)
        .collect();
    if !skipped.is_empty() {
        println!(
            "\nStages skipped: {} (the plugin is not installed. CI runs --strict, so it fails there)",
            join(&skipped)
        );
    }
    let missing: Vec<usize> = BUILD_STEPS
        .into_iter()
        .filter(|number| !steps.iter().any(|step| step.number == *number))
        .collect();
    if !missing.is_empty() {
        println!(
            "Compiling stages not run: {} (integrity failed)",
            join(&missing)
        );
    }
    if failed.is_empty() {
        println!("\nAll passed.");
    } else {
        println!("\nStages failed: {}", join(&failed));
    }
}

fn join(numbers: &[usize]) -> String {
    numbers
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::{parse_tools_lock, Outcome};

    #[test]
    fn reads_the_tools_lock() {
        let text = "# a comment\n\ncargo-audit 0.22.2\ncargo-deny 0.20.2\n";
        assert_eq!(
            parse_tools_lock(text),
            vec![
                ("cargo-audit".to_owned(), "0.22.2".to_owned()),
                ("cargo-deny".to_owned(), "0.20.2".to_owned()),
            ]
        );
    }

    #[test]
    fn outcome_tags_are_stable() {
        assert_eq!(Outcome::Pass.tag(), "ok");
        assert_eq!(Outcome::Fail.tag(), "FAIL");
        assert_eq!(Outcome::Skipped.tag(), "SKIP");
        assert_eq!(Outcome::Report.tag(), "note");
    }
}
