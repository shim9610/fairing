//! Shared utilities: the error type, finding the workspace root, running external commands, splitting columns.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// An xtask error. With no external dependency, it carries only a human-readable message.
#[derive(Debug, Clone)]
pub(crate) struct Error {
    message: String,
}

impl Error {
    /// Build an error from a message.
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::new(err.to_string())
    }
}

/// xtask's own result type.
pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Find the workspace root.
///
/// The first choice is the parent of the compile-time `CARGO_MANIFEST_DIR` (= the repository root);
/// failing that, it walks up from the current directory looking for a `Cargo.toml` with a `[workspace]`.
pub(crate) fn workspace_root() -> Result<PathBuf> {
    if let Some(dir) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        if is_workspace_root(dir) {
            return Ok(dir.to_path_buf());
        }
    }
    let mut cur = std::env::current_dir()?;
    loop {
        if is_workspace_root(&cur) {
            return Ok(cur);
        }
        if !cur.pop() {
            break;
        }
    }
    Err(Error::new(
        "could not find the workspace root: no Cargo.toml with a `[workspace]` in any parent path",
    ))
}

fn is_workspace_root(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("Cargo.toml")).is_ok_and(|text| {
        text.lines()
            .any(|l| l.trim_start().starts_with("[workspace]"))
    })
}

/// The `cargo` binary to run. The `CARGO` environment variable cargo handed over takes priority.
pub(crate) fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn build_command(root: &Path, program: &str, args: &[&str], envs: &[(&str, &str)]) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir(root);
    cmd.args(args);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd
}

/// Make the command line into one human-readable line.
pub(crate) fn display_command(program: &str, args: &[&str]) -> String {
    let mut out = String::from(program);
    for arg in args {
        out.push(' ');
        if arg.contains(' ') || arg.contains('|') {
            out.push('"');
            out.push_str(arg);
            out.push('"');
        } else {
            out.push_str(arg);
        }
    }
    out
}

/// Run a command and hand its stdout back as a string. On a failure, an error including the stderr.
pub(crate) fn capture(root: &Path, program: &str, args: &[&str]) -> Result<String> {
    let mut cmd = build_command(root, program, args, &[]);
    cmd.stdin(Stdio::null());
    let output = cmd.output().map_err(|err| {
        Error::new(format!(
            "cannot run: {}\ncause: {err}",
            display_command(program, args)
        ))
    })?;
    if !output.status.success() {
        return Err(Error::new(format!(
            "the command failed ({}): {}\n--- stderr ---\n{}",
            exit_text(output.status.code()),
            display_command(program, args),
            String::from_utf8_lossy(&output.stderr).trim_end()
        )));
    }
    String::from_utf8(output.stdout).map_err(|err| {
        Error::new(format!(
            "{}'s output is not UTF-8: {err}",
            display_command(program, args)
        ))
    })
}

/// Run a command and let its output flow through as it is. `true` on success.
pub(crate) fn run(
    root: &Path,
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<bool> {
    let mut cmd = build_command(root, program, args, envs);
    cmd.stdin(Stdio::null());
    let status = cmd.status().map_err(|err| {
        Error::new(format!(
            "cannot run: {}\ncause: {err}",
            display_command(program, args)
        ))
    })?;
    Ok(status.success())
}

fn exit_text(code: Option<i32>) -> String {
    code.map_or_else(
        || "terminated by a signal".to_owned(),
        |c| format!("exit code {c}"),
    )
}

/// Check whether a plugin is installed, and its version, with `cargo <sub> --version`.
/// `None` where it is not installed.
pub(crate) fn plugin_version(root: &Path, subcommand: &str) -> Option<String> {
    let mut cmd = build_command(root, &cargo_bin(), &[subcommand, "--version"], &[]);
    cmd.stdin(Stdio::null());
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    version_from_output(&String::from_utf8(output.stdout).ok()?)
}

/// Take just the version out of the first line of `cargo <sub> --version`.
/// `cargo audit` prints its name twice, as `cargo-audit-audit 0.22.2`.
pub(crate) fn version_from_output(text: &str) -> Option<String> {
    text.lines()
        .next()?
        .split_whitespace()
        .next_back()
        .map(str::to_owned)
}

/// Read a file. On a failure, an error including the path.
pub(crate) fn read_file(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .map_err(|err| Error::new(format!("cannot read {}: {err}", path.display())))
}

/// Write a file. Where the parent directory does not exist, it is made.
pub(crate) fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| Error::new(format!("cannot make {}: {err}", parent.display())))?;
    }
    std::fs::write(path, contents)
        .map_err(|err| Error::new(format!("cannot write to {}: {err}", path.display())))
}

/// The grid precision icon coordinates are folded to. On the 24-grid, 1e-4 is invisible.
const COORD_STEP: f64 = 10_000.0;

/// Narrow an `f64` coordinate to an `f32`.
///
/// It is rounded to 1e-4 before the narrowing. That stops both the floating-point noise the arc (A) →
/// cubic conversion produces, such as `-2.4e-8`, being written into the generated file as
/// `-0.000000024234437`, and a difference in libm implementations changing the last digit and making
/// `icons --check` come out differently per platform.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn to_f32(value: f64) -> f32 {
    if !value.is_finite() {
        return value as f32;
    }
    ((value * COORD_STEP).round() / COORD_STEP) as f32
}

/// Use an `f64` of 0 or above as a count (an arc's subdivision count). A negative or a NaN becomes 0.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn to_count(value: f64) -> usize {
    if value.is_finite() && value > 0.0 {
        value as usize
    } else {
        0
    }
}

/// A count as an `f64`. An icon arc's subdivision count is small, so it is lossless.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn count_to_f64(value: usize) -> f64 {
    value as f64
}

/// Reduce runs of whitespace to one space and trim both ends. Used for comparing licence expressions.
pub(crate) fn normalize_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Take two or more spaces (or a tab) as the column separator and split into at most `max` columns.
/// The last column is everything left (two spaces within it are kept).
pub(crate) fn split_columns(line: &str, max: usize) -> Vec<&str> {
    let mut cols: Vec<&str> = Vec::new();
    let mut rest = line.trim();
    while cols.len() + 1 < max && !rest.is_empty() {
        let Some((gap_start, gap_end)) = find_gap(rest) else {
            break;
        };
        let (Some(head), Some(tail)) = (rest.get(..gap_start), rest.get(gap_end..)) else {
            break;
        };
        cols.push(head);
        rest = tail.trim_start();
    }
    if !rest.is_empty() {
        cols.push(rest);
    }
    cols
}

/// The byte range of a whitespace run that acts as a column separator (two or more spaces, or containing a tab).
fn find_gap(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes.get(i), Some(b' ' | b'\t')) {
            let start = i;
            let mut has_tab = false;
            while let Some(&b) = bytes.get(i) {
                if b == b'\t' {
                    has_tab = true;
                } else if b != b' ' {
                    break;
                }
                i += 1;
            }
            if has_tab || i - start >= 2 {
                return Some((start, i));
            }
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{normalize_ws, split_columns, to_f32, version_from_output};

    #[test]
    fn coordinates_are_snapped_to_the_grid_precision() {
        // The noise the arc conversion produces becomes 0.
        assert!(to_f32(-2.423_443_7e-8).abs() < f32::EPSILON);
        assert!((to_f32(15.313_708_498_984_76) - 15.3137).abs() < 1e-6);
        assert!((to_f32(12.0) - 12.0).abs() < f32::EPSILON);
        assert!(to_f32(f64::INFINITY).is_infinite());
        assert!(to_f32(f64::NAN).is_nan());
    }

    #[test]
    fn splits_on_two_or_more_spaces() {
        let cols = split_columns(
            "egui  =0.36.1  MIT OR Apache-2.0  UI (owner, 2026-09-02)",
            4,
        );
        assert_eq!(
            cols,
            vec![
                "egui",
                "=0.36.1",
                "MIT OR Apache-2.0",
                "UI (owner, 2026-09-02)"
            ]
        );
    }

    #[test]
    fn keeps_wide_gaps_inside_last_column() {
        let cols = split_columns("a  b  c  d    e", 4);
        assert_eq!(cols, vec!["a", "b", "c", "d    e"]);
    }

    #[test]
    fn single_space_is_not_a_separator() {
        let cols = split_columns("a b  c", 4);
        assert_eq!(cols, vec!["a b", "c"]);
    }

    #[test]
    fn tab_is_a_separator() {
        let cols = split_columns("a\tb\tc", 4);
        assert_eq!(cols, vec!["a", "b", "c"]);
    }

    #[test]
    fn reads_plugin_versions() {
        assert_eq!(
            version_from_output("cargo-audit-audit 0.22.2\n"),
            Some("0.22.2".to_owned())
        );
        assert_eq!(
            version_from_output("cargo-deny 0.20.2\nthe next line"),
            Some("0.20.2".to_owned())
        );
        assert_eq!(version_from_output(""), None);
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(normalize_ws("  MIT   OR\tApache-2.0 "), "MIT OR Apache-2.0");
    }
}
