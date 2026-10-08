//! The `deps.allow` approval gate.

use crate::tree::{self, Pkg};
use crate::util::{self, Error, Result};
use crate::version::{Version, VersionReq};
use std::path::{Path, PathBuf};

/// The `cargo tree` output format. `{p}` = `name vX.Y.Z`, `{l}` = the licence expression.
const TREE_FORMAT: &str = "{p}|{l}";

const HEADER: &str = "\
# deps.allow — the approved dependency allowlist
#
# Every crate appearing in the tree (transitive dependencies included) has to be here. Without it the audit gate fails.
# Additions go only through the approval procedure in CONTRIBUTING.md (a PR review).
#
#   Check:   cargo xtask deps-check
#   Refresh: cargo xtask deps-check --write   (dumps the current tree; the existing lines' reasons are kept)
#
# Lines starting with '#' and blank lines are ignored. The column separator is two or more spaces, and
# only the last column (the reason) may contain two spaces within it.
#
#   name   version-req   license   reason / approved-by / date
#
# version-req: =X.Y.Z (exact) · ^X.Y[.Z] (caret; 0.x pins the minor) · ~X.Y[.Z] (pins the minor) · * (any)
#              With no prefix it is read as a caret. Even for the same crate, a different major (a
#              different minor for 0.x) is a separate line. `--write` pins only egui and eframe at
#              =X.Y.Z and writes the rest as carets — so that a transitive dependency's
#              patch update does not break the gate.
# license:     it has to match cargo tree's {l} value exactly as a string, after whitespace normalisation.
# reason:      leave the approver and the date with the reason, as `(approver, YYYY-MM-DD)`.
#              A line with no date, or with a `TODO` left in, counts as unapproved and the gate fails.
#
# Target triples: x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu (the union under --all-features)
";

/// One approval line.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) req: VersionReq,
    pub(crate) license: String,
    pub(crate) reason: String,
}

/// `deps.allow`'s path.
pub(crate) fn path(root: &Path) -> PathBuf {
    root.join("deps.allow")
}

/// Parse `deps.allow`'s body. It raises errors carrying the line number.
pub(crate) fn parse(text: &str) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let number = index + 1;
        let cols = util::split_columns(line, 4);
        let (Some(name), Some(req), Some(license)) = (cols.first(), cols.get(1), cols.get(2))
        else {
            return Err(Error::new(format!(
                "deps.allow:{number}: fewer than 3 columns (name / version-req / license / reason)"
            )));
        };
        let req = VersionReq::parse(req)
            .map_err(|err| Error::new(format!("deps.allow:{number}: {err}")))?;
        // Two spaces inside the licence column would break the column split. It is caught by bracket balance.
        if license.matches('(').count() != license.matches(')').count() {
            return Err(Error::new(format!(
                "deps.allow:{number}: the brackets in the licence column `{license}` do not balance (the column separator is two or more spaces)"
            )));
        }
        entries.push(Entry {
            name: (*name).to_owned(),
            req,
            license: util::normalize_ws(license),
            reason: cols
                .get(3)
                .map_or_else(String::new, |r| r.trim().to_owned()),
        });
    }
    Ok(entries)
}

/// Only these two direct dependencies are pinned exactly at `=X.Y.Z` (to control the MSRV
/// and API drift). Every other direct dependency and every transitive one is written as a caret.
const EXACT_PIN: [&str; 2] = ["egui", "eframe"];

/// The version-req `--write` puts on a new line.
fn default_req(name: &str, version: &Version) -> VersionReq {
    if EXACT_PIN.contains(&name) {
        VersionReq::exact(version)
    } else {
        VersionReq::caret(version)
    }
}

/// The position of the approval line whose name, version and licence all match.
fn find_matching(entries: &[Entry], pkg: &Pkg, version: &Version) -> Option<usize> {
    entries.iter().position(|entry| {
        entry.name == pkg.name && entry.req.matches(version) && entry.license == pkg.license
    })
}

/// The position of the first line whose name and version match. It shows what the licence differs from when it does.
fn find_by_version(entries: &[Entry], name: &str, version: &Version) -> Option<usize> {
    entries
        .iter()
        .position(|entry| entry.name == name && entry.req.matches(version))
}

/// Whether the reason column looks like an approval record: the reason, the approver and the date.
fn is_approved(reason: &str) -> bool {
    !reason.is_empty() && !reason.contains("TODO") && has_date(reason)
}

/// Whether a `YYYY-MM-DD` is in it.
fn has_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = |start: usize, count: usize| -> bool {
        (0..count).all(|offset| bytes.get(start + offset).is_some_and(u8::is_ascii_digit))
    };
    (0..bytes.len()).any(|index| {
        digits(index, 4)
            && bytes.get(index + 4) == Some(&b'-')
            && digits(index + 5, 2)
            && bytes.get(index + 7) == Some(&b'-')
            && digits(index + 8, 2)
    })
}

fn version_of(pkg: &Pkg) -> Result<Version> {
    Version::parse(&pkg.version)
        .map_err(|err| Error::new(format!("cannot read {}'s version: {err}", pkg.name)))
}

/// Stage 6: compare the tree against `deps.allow`. `true` on a pass.
pub(crate) fn check(root: &Path) -> Result<bool> {
    let packages = tree::collect_union(root, TREE_FORMAT)?;
    let file = path(root);
    if !file.exists() {
        return Err(Error::new(format!(
            "{} is missing. Build the initial allowlist with `cargo xtask deps-check --write` and get it approved in a PR",
            file.display()
        )));
    }
    let entries = parse(&util::read_file(&file)?)?;
    let mut used = vec![false; entries.len()];
    let mut violations: Vec<String> = Vec::new();

    for pkg in &packages {
        let version = version_of(pkg)?;
        if let Some(index) = find_matching(&entries, pkg, &version) {
            if let Some(slot) = used.get_mut(index) {
                *slot = true;
            }
            continue;
        }
        match find_by_version(&entries, &pkg.name, &version).and_then(|index| entries.get(index)) {
            Some(entry) => violations.push(format!(
                "needs approval: {} v{} ({}) ← cargo tree -i {}   [the allowlist's licence: {}]",
                pkg.name, pkg.version, pkg.license, pkg.name, entry.license
            )),
            None => violations.push(format!(
                "needs approval: {} v{} ({}) ← cargo tree -i {}",
                pkg.name, pkg.version, pkg.license, pkg.name
            )),
        }
    }

    // A line existing is not an approval. It needs a reason, an approver and a date.
    for (entry, hit) in entries.iter().zip(used.iter()) {
        if *hit && !is_approved(&entry.reason) {
            violations.push(format!(
                "unapproved line: {} {} — write `a description (approver, YYYY-MM-DD)` in the reason column (CONTRIBUTING.md)",
                entry.name, entry.req
            ));
        }
    }

    report(&packages, &entries, &used, &violations);
    Ok(violations.is_empty())
}

fn report(packages: &[Pkg], entries: &[Entry], used: &[bool], violations: &[String]) {
    println!(
        "{} crates in the tree, {} lines in the allowlist",
        packages.len(),
        entries.len()
    );
    for (entry, hit) in entries.iter().zip(used.iter()) {
        if !hit {
            println!(
                "  (note) an allowlist line not in the tree: {} {}",
                entry.name, entry.req
            );
        }
    }
    if violations.is_empty() {
        println!("Every crate is in deps.allow.");
        return;
    }
    for line in violations {
        println!("{line}");
    }
    println!(
        "{} are not approved. Go through the approval procedure in CONTRIBUTING.md and then refresh with `cargo xtask deps-check --write`.",
        violations.len()
    );
}

/// `--write`: build or refresh `deps.allow` from the current tree. The existing lines' reasons are kept.
pub(crate) fn write(root: &Path) -> Result<()> {
    let packages = tree::collect_union(root, TREE_FORMAT)?;
    let file = path(root);
    let existing = if file.exists() {
        parse(&util::read_file(&file)?)?
    } else {
        Vec::new()
    };

    let mut rows: Vec<[String; 4]> = Vec::new();
    let mut used = vec![false; existing.len()];
    let mut added = 0_usize;
    for pkg in &packages {
        let version = version_of(pkg)?;
        let (matched, req, reason) = resolve_row(&existing, pkg, &version);
        match matched {
            Some(index) => {
                if let Some(slot) = used.get_mut(index) {
                    *slot = true;
                }
            }
            None => added += 1,
        }
        rows.push([pkg.name.clone(), req, pkg.license.clone(), reason]);
    }

    util::write_file(&file, &render(&rows))?;
    let dropped = used.iter().filter(|hit| !**hit).count();
    println!(
        "refreshed {}: {} lines ({added} new, {dropped} gone)",
        file.display(),
        rows.len()
    );
    if added > 0 {
        println!(
            "the new entries' reason is `TODO approve`. Fill in the PR template's dependency checklist and write `(approver, YYYY-MM-DD)`. \nUntil then `cargo xtask deps-check` fails."
        );
    }
    Ok(())
}

/// The `(inherited line, version-req, reason)` a crate will use.
///
/// An existing approval is inherited only where the name, the version **and the licence** all match.
/// Attaching the old approver and date to a line whose licence changed would leave an invalid approval
/// record in the file.
fn resolve_row(
    existing: &[Entry],
    pkg: &Pkg,
    version: &Version,
) -> (Option<usize>, String, String) {
    if let Some(index) = find_matching(existing, pkg, version) {
        if let Some(entry) = existing.get(index) {
            return (Some(index), entry.req.to_string(), entry.reason.clone());
        }
    }
    let reason = find_by_version(existing, &pkg.name, version)
        .and_then(|index| existing.get(index))
        .map_or_else(
            || "TODO approve".to_owned(),
            |entry| {
                format!(
                    "TODO approve (licence changed: {} → {})",
                    entry.license, pkg.license
                )
            },
        );
    (None, default_req(&pkg.name, version).to_string(), reason)
}

/// Align the columns and build the file's body. For the same input it is always the same output.
fn render(rows: &[[String; 4]]) -> String {
    let width = |index: usize| -> usize {
        rows.iter()
            .filter_map(|row| row.get(index))
            .map(String::len)
            .max()
            .unwrap_or(0)
    };
    let (name_w, req_w, license_w) = (width(0), width(1), width(2));
    let mut out = String::from(HEADER);
    out.push('\n');
    for row in rows {
        let (Some(name), Some(req), Some(license), Some(reason)) =
            (row.first(), row.get(1), row.get(2), row.get(3))
        else {
            continue;
        };
        let line = format!("{name:name_w$}  {req:req_w$}  {license:license_w$}  {reason}");
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{default_req, is_approved, parse, render, resolve_row};
    use crate::tree::Pkg;
    use crate::util::{Error, Result};
    use crate::version::Version;

    fn pkg(name: &str, version: &str, license: &str) -> Pkg {
        Pkg {
            name: name.to_owned(),
            version: version.to_owned(),
            license: license.to_owned(),
            repository: String::new(),
        }
    }

    const SAMPLE: &str = "\
# a comment
egui                   =0.36.1       MIT OR Apache-2.0                UI (owner, 2026-09-02)

accesskit              ^0.24         MIT OR Apache-2.0                a required egui dependency
epaint_default_fonts   =0.36.1       (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0   the default fonts
winnow                 ^1            MIT OR Apache-2.0
";

    #[test]
    fn parses_columns_comments_and_blank_lines() -> Result<()> {
        let entries = parse(SAMPLE)?;
        assert_eq!(entries.len(), 4);
        let first = entries.first().ok_or_else(|| Error::new("an empty list"))?;
        assert_eq!(first.name, "egui");
        assert_eq!(first.req.to_string(), "=0.36.1");
        assert_eq!(first.license, "MIT OR Apache-2.0");
        assert_eq!(first.reason, "UI (owner, 2026-09-02)");
        Ok(())
    }

    #[test]
    fn license_with_spaces_stays_in_one_column() -> Result<()> {
        let entries = parse(SAMPLE)?;
        let fonts = entries
            .iter()
            .find(|entry| entry.name == "epaint_default_fonts")
            .ok_or_else(|| Error::new("epaint_default_fonts is missing"))?;
        assert_eq!(
            fonts.license,
            "(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0"
        );
        assert_eq!(fonts.reason, "the default fonts");
        Ok(())
    }

    #[test]
    fn reason_is_optional() -> Result<()> {
        let entries = parse(SAMPLE)?;
        let winnow = entries
            .iter()
            .find(|entry| entry.name == "winnow")
            .ok_or_else(|| Error::new("winnow is missing"))?;
        assert_eq!(winnow.reason, "");
        assert!(winnow.req.matches(&Version::parse("1.4.0")?));
        assert!(!winnow.req.matches(&Version::parse("2.0.0")?));
        Ok(())
    }

    #[test]
    fn rejects_short_rows() {
        assert!(parse("egui  =0.36.1\n").is_err());
    }

    #[test]
    fn rejects_bad_requirements() {
        assert!(parse("egui  >=0.36  MIT  x\n").is_err());
    }

    #[test]
    fn new_lines_use_the_design_version_rule() -> Result<()> {
        // Only egui and eframe are pinned exactly.
        assert_eq!(
            default_req("egui", &Version::parse("0.36.1")?).to_string(),
            "=0.36.1"
        );
        assert_eq!(
            default_req("eframe", &Version::parse("0.36.1")?).to_string(),
            "=0.36.1"
        );
        assert_eq!(
            default_req("accesskit", &Version::parse("0.24.1")?).to_string(),
            "^0.24"
        );
        assert_eq!(
            default_req("winnow", &Version::parse("1.0.4")?).to_string(),
            "^1"
        );
        Ok(())
    }

    #[test]
    fn a_license_change_is_a_new_entry() -> Result<()> {
        let existing = parse("foo  ^1  MIT  UI (owner, 2026-09-02)\n")?;
        let version = Version::parse("1.0.0")?;
        let (matched, req, reason) =
            resolve_row(&existing, &pkg("foo", "1.0.0", "GPL-3.0"), &version);
        assert_eq!(matched, None, "an old approval must not be inherited");
        assert_eq!(req, "^1");
        assert!(reason.contains("TODO approve"), "{reason}");
        assert!(reason.contains("MIT → GPL-3.0"), "{reason}");
        // With the same licence, the reason is kept as it is.
        let (kept, _, same) = resolve_row(
            &existing,
            &pkg("foo", "1.4.0", "MIT"),
            &Version::parse("1.4.0")?,
        );
        assert_eq!(kept, Some(0));
        assert_eq!(same, "UI (owner, 2026-09-02)");
        Ok(())
    }

    #[test]
    fn a_second_line_with_another_license_is_reachable() -> Result<()> {
        let existing = parse(
            "foo  ^1  MIT  a (owner, 2026-09-02)\nfoo  ^1  Apache-2.0  b (owner, 2026-09-02)\n",
        )?;
        let version = Version::parse("1.5.0")?;
        let (matched, _, reason) =
            resolve_row(&existing, &pkg("foo", "1.5.0", "Apache-2.0"), &version);
        assert_eq!(matched, Some(1));
        assert_eq!(reason, "b (owner, 2026-09-02)");
        Ok(())
    }

    #[test]
    fn approval_needs_a_reason_an_approver_and_a_date() {
        assert!(is_approved("UI (shim9610, 2026-09-02)"));
        assert!(!is_approved(""));
        assert!(!is_approved("TODO approve"));
        assert!(!is_approved("internal to egui"));
        assert!(!is_approved("approved (shim9610)"));
    }

    #[test]
    fn unbalanced_license_parentheses_are_rejected() {
        assert!(parse("name  =1.0.0  (MIT OR  Apache-2.0)  r\n").is_err());
    }

    #[test]
    fn rendering_is_deterministic_and_reparses() -> Result<()> {
        let rows = vec![
            [
                "egui".to_owned(),
                "=0.36.1".to_owned(),
                "MIT OR Apache-2.0".to_owned(),
                "UI".to_owned(),
            ],
            [
                "log".to_owned(),
                "^0.4".to_owned(),
                "MIT OR Apache-2.0".to_owned(),
                String::new(),
            ],
        ];
        let first = render(&rows);
        assert_eq!(first, render(&rows));
        let entries = parse(&first)?;
        assert_eq!(entries.len(), 2);
        assert!(first.contains("egui  =0.36.1  MIT OR Apache-2.0  UI"));
        assert!(first.contains("log   ^0.4     MIT OR Apache-2.0"));
        assert!(!first.lines().any(|line| line.ends_with(' ')));
        Ok(())
    }
}
