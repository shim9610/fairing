//! Calling `cargo tree` and parsing its lines (audit stages 5 · 6 · 10).

use crate::util::{self, Error, Result};
use std::path::{Path, PathBuf};

/// The host the audit runs on, and the device target it cross-checks.
pub(crate) const HOST_TARGET: &str = "x86_64-unknown-linux-gnu";
/// The device target (stage 12's cross build).
pub(crate) const CROSS_TARGET: &str = "aarch64-unknown-linux-gnu";
/// The targets audited. The same two as `deny.toml [graph].targets`.
pub(crate) const TARGETS: [&str; 2] = [HOST_TARGET, CROSS_TARGET];

/// One crate appearing in the tree.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Pkg {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) license: String,
    pub(crate) repository: String,
}

/// The raw information read from one `cargo tree` line. The source comment is held as it is, unclassified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) pkg: Pkg,
    /// What is inside the brackets of `name vX.Y.Z (…)`. `None` for a registry crate.
    pub(crate) source: Option<String>,
}

/// Parse one line of `cargo tree --prefix none -f "{p}|{l}[|{r}]"`.
///
/// - A trailing ` (*)` (cargo's duplicate marking) is removed.
/// - ` (proc-macro)` is not part of the name and is removed.
/// - What brackets are left are **the source**. A workspace member (a path), git, an alternative
///   registry and a `[patch]` replacement all come through here. Which it is, [`classify`] settles by
///   comparing against the set of member paths — passing "brackets mean a member" as before would let
///   a git or an out-of-tree path dependency bypass the approval gate whole.
pub(crate) fn parse_line(line: &str) -> Option<Row> {
    let trimmed = line.trim_end();
    let trimmed = trimmed.strip_suffix(" (*)").unwrap_or(trimmed).trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let mut fields = trimmed.splitn(3, '|');
    let package = fields.next()?.trim();
    let license = fields.next().unwrap_or("").trim();
    let repository = fields.next().unwrap_or("").trim();

    let package = package.replace(" (proc-macro)", "");
    let package = package.trim();
    let (head, source) = match package.split_once(" (") {
        Some((head, tail)) => (head, Some(tail.trim_end_matches(')').to_owned())),
        None => (package, None),
    };
    let (name, version) = head.trim().split_once(' ')?;
    let version = version.trim().strip_prefix('v')?;
    if name.is_empty() || version.is_empty() {
        return None;
    }
    Some(Row {
        pkg: Pkg {
            name: name.to_owned(),
            version: version.to_owned(),
            license: util::normalize_ws(license),
            repository: repository.to_owned(),
        },
        source,
    })
}

/// The root `Cargo.toml`'s `[workspace] members` as absolute paths.
pub(crate) fn member_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let text = util::read_file(&root.join("Cargo.toml"))?;
    let items = crate::tomlish::items(&text)?;
    let members = crate::tomlish::find(&items, "workspace", "members")
        .ok_or_else(|| Error::new("the root Cargo.toml has no `[workspace] members`"))?;
    let mut out = Vec::new();
    for entry in crate::tomlish::array_strings(&members.value)? {
        if entry.contains('*') {
            return Err(Error::new(format!(
                "the glob `{entry}` in `[workspace] members` is not supported: write the paths one at a time"
            )));
        }
        let dir = root.join(&entry);
        if !dir.join("Cargo.toml").is_file() {
            return Err(Error::new(format!(
                "`{entry}` in `[workspace] members` has no Cargo.toml"
            )));
        }
        out.push(dir.canonicalize().unwrap_or(dir));
    }
    Ok(out)
}

/// Whether a source comment is a workspace member's directory.
fn is_member(source: &str, root: &Path, members: &[PathBuf]) -> bool {
    let raw = Path::new(source);
    let full = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    };
    let full = full.canonicalize().unwrap_or(full);
    members.contains(&full)
}

/// Sort a line into an approval candidate (a registry crate), a member, or a violation.
/// On a violation it hands back a human-readable explanation.
fn classify(
    row: Row,
    root: &Path,
    members: &[PathBuf],
) -> std::result::Result<Option<Pkg>, String> {
    match row.source {
        None => Ok(Some(row.pkg)),
        Some(source) if is_member(&source, root, members) => Ok(None),
        Some(source) => Err(format!(
            "a source that is not allowed: {} v{} ({source}) — only crates.io is used. \
git, an alternative registry, an out-of-workspace path dependency and a [patch]/[replace] swap all catch here",
            row.pkg.name, row.pkg.version
        )),
    }
}

/// Run `cargo tree` for one target and get the crate list.
pub(crate) fn collect(root: &Path, format: &str, target: &str) -> Result<Vec<Pkg>> {
    let members = member_dirs(root)?;
    let cargo = util::cargo_bin();
    let args = [
        "tree",
        "--locked",
        "--workspace",
        "-e",
        "normal",
        "--prefix",
        "none",
        "--all-features",
        "-f",
        format,
        "--target",
        target,
    ];
    let stdout = util::capture(root, &cargo, &args)?;
    let mut out = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    for row in stdout.lines().filter_map(parse_line) {
        match classify(row, root, &members) {
            Ok(Some(pkg)) => out.push(pkg),
            Ok(None) => {}
            Err(message) => {
                if !bad.contains(&message) {
                    bad.push(message);
                }
            }
        }
    }
    if !bad.is_empty() {
        return Err(Error::new(format!("{target}:\n  {}", bad.join("\n  "))));
    }
    Ok(out)
}

/// The union of the two targets, de-duplicated by `(name, version)` and returned in name and version order.
pub(crate) fn collect_union(root: &Path, format: &str) -> Result<Vec<Pkg>> {
    let mut all: Vec<Pkg> = Vec::new();
    for target in TARGETS {
        for pkg in collect(root, format, target)? {
            let known = all
                .iter()
                .any(|seen| seen.name == pkg.name && seen.version == pkg.version);
            if !known {
                all.push(pkg);
            }
        }
    }
    all.sort_by_key(sort_key);
    Ok(all)
}

fn sort_key(pkg: &Pkg) -> (String, Vec<u64>, String) {
    let numeric = pkg
        .version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|piece| !piece.is_empty())
        .filter_map(|piece| piece.parse::<u64>().ok())
        .collect();
    (pkg.name.clone(), numeric, pkg.version.clone())
}

#[cfg(test)]
mod tests {
    use super::{classify, parse_line, Pkg, Row};
    use std::path::{Path, PathBuf};

    fn row(name: &str, version: &str, license: &str, source: Option<&str>) -> Row {
        Row {
            pkg: Pkg {
                name: name.to_owned(),
                version: version.to_owned(),
                license: license.to_owned(),
                repository: String::new(),
            },
            source: source.map(str::to_owned),
        }
    }

    #[test]
    fn parses_a_plain_line() {
        assert_eq!(
            parse_line("egui v0.36.1|MIT OR Apache-2.0"),
            Some(row("egui", "0.36.1", "MIT OR Apache-2.0", None))
        );
    }

    #[test]
    fn strips_the_duplicate_marker() {
        assert_eq!(
            parse_line("log v0.4.28|MIT OR Apache-2.0 (*)"),
            Some(row("log", "0.4.28", "MIT OR Apache-2.0", None))
        );
    }

    #[test]
    fn keeps_proc_macro_crates() {
        assert_eq!(
            parse_line("serde_derive v1.0.228 (proc-macro)|MIT OR Apache-2.0"),
            Some(row("serde_derive", "1.0.228", "MIT OR Apache-2.0", None))
        );
    }

    #[test]
    fn keeps_the_source_annotation() {
        assert_eq!(
            parse_line("fairing v0.1.0 (/home/u/fairing-dev/crates/fairing)|MIT OR Apache-2.0"),
            Some(row(
                "fairing",
                "0.1.0",
                "MIT OR Apache-2.0",
                Some("/home/u/fairing-dev/crates/fairing")
            ))
        );
        assert_eq!(
            parse_line("evil v1.0.0 (https://github.com/x/y#13d88c7b)|MIT"),
            Some(row(
                "evil",
                "1.0.0",
                "MIT",
                Some("https://github.com/x/y#13d88c7b")
            ))
        );
        assert_eq!(
            parse_line("my-macro v0.1.0 (proc-macro) (crates/my-macro)|MIT"),
            Some(row("my-macro", "0.1.0", "MIT", Some("crates/my-macro")))
        );
    }

    #[test]
    fn only_real_member_paths_are_skipped() {
        let root = Path::new("/repo");
        let members: Vec<PathBuf> = vec![PathBuf::from("/repo/crates/fairing")];
        assert_eq!(
            classify(
                row("fairing", "0.0.1", "MIT", Some("/repo/crates/fairing")),
                root,
                &members
            ),
            Ok(None)
        );
        // git, an alternative registry and an out-of-workspace path are all violations.
        for source in [
            "https://github.com/x/y#abcdef",
            "registry `alt`",
            "/tmp/evil-log",
        ] {
            let verdict = classify(row("log", "0.4.34", "MIT", Some(source)), root, &members);
            assert!(verdict.is_err(), "{source} passed");
        }
    }

    #[test]
    fn registry_crates_are_approval_targets() {
        let root = Path::new("/repo");
        let outcome = classify(row("log", "0.4.34", "MIT", None), root, &[]);
        assert_eq!(
            outcome.map(|pkg| pkg.map(|p| p.name)),
            Ok(Some("log".to_owned()))
        );
    }

    #[test]
    fn normalizes_license_whitespace() {
        assert_eq!(
            parse_line("unicode-ident v1.0.20|(MIT OR  Apache-2.0) AND Unicode-3.0"),
            Some(row(
                "unicode-ident",
                "1.0.20",
                "(MIT OR Apache-2.0) AND Unicode-3.0",
                None
            ))
        );
    }

    #[test]
    fn keeps_the_repository_field() {
        let parsed = parse_line("log v0.4.28|MIT OR Apache-2.0|https://github.com/rust-lang/log");
        assert_eq!(
            parsed.map(|row| row.pkg.repository),
            Some("https://github.com/rust-lang/log".to_owned())
        );
    }

    #[test]
    fn ignores_blank_and_malformed_lines() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("   "), None);
        assert_eq!(parse_line("no-version|MIT"), None);
    }

    #[test]
    fn missing_license_becomes_empty() {
        assert_eq!(
            parse_line("weird v1.0.0|"),
            Some(row("weird", "1.0.0", "", None))
        );
    }
}
