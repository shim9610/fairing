//! The integrity check — it runs **before compiling**.
//!
//! The approval gates (`deps.allow`, `cargo-deny`) look at the tree `Cargo.toml` / `Cargo.lock`
//! speaks for. There are a few channels that can part that tree from **the code actually compiled**,
//! and neither gate can see them:
//!
//! - `.cargo/config.toml`'s `[source]` replacement plus `vendor/` — Cargo.toml and Cargo.lock do not
//!   change by a byte while the source compiled does.
//! - `[patch]` / `[replace]` — a different source is slotted in under the same name and version.
//! - `.cargo/config.toml`'s `[build] rustc-wrapper`, `[env]` and `[target.*.runner]` — they run
//!   arbitrary code on the runner.
//! - A member dropping `[lints] workspace = true` leaves `unsafe_code = "forbid"` unapplied to that crate.
//! - `[target.'cfg(...)'.dependencies]` — a dependency outside the two audited targets appears in
//!   neither `cargo tree` nor `cargo deny`.
//!
//! And one fact that lives in prose as well as in the manifest: the minimum Rust version. The
//! crates and CI read `rust-version`; the README, the contributing guide and the getting-started
//! table quote it, and this checks they quote the same number.
//!
//! All of it is fail-closed: anything it does not know is not passed.

use crate::tomlish;
use crate::util::{self, Error, Result};
use std::path::{Path, PathBuf};

/// How `Cargo.lock` spells the crates.io registry.
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// The only table allowed in `.cargo/config.toml`.
const ALLOWED_CONFIG_SECTION: &str = "alias";

/// Stage 0's body. `true` on a pass.
pub(crate) fn check(root: &Path) -> Result<bool> {
    let mut problems: Vec<String> = Vec::new();
    let members = crate::tree::member_dirs(root)?;

    cargo_config(root, &members, &mut problems)?;
    vendor_dir(root, &mut problems);
    manifests(root, &members, &mut problems)?;
    lockfile(root, &members, &mut problems)?;
    msrv_in_prose(root, &mut problems)?;

    println!(
        "integrity: .cargo/config.toml · vendor/ · [patch]/[replace] · the members' lints · Cargo.lock's provenance · the MSRV the docs quote ({} members)",
        members.len()
    );
    if problems.is_empty() {
        println!("The build inputs are exactly what Cargo.toml and Cargo.lock speak for.");
        return Ok(true);
    }
    for problem in &problems {
        println!("integrity violation: {problem}");
    }
    Ok(false)
}

/// It looks at every `.cargo/config[.toml]` in the repository.
fn cargo_config(root: &Path, members: &[PathBuf], problems: &mut Vec<String>) -> Result<()> {
    let mut dirs = vec![root.to_path_buf()];
    dirs.extend(members.iter().cloned());
    for dir in dirs {
        for name in ["config.toml", "config"] {
            let file = dir.join(".cargo").join(name);
            if !file.is_file() {
                continue;
            }
            let text = util::read_file(&file)?;
            let shown = display(root, &file);
            for section in tomlish::sections(&text)? {
                if section != ALLOWED_CONFIG_SECTION {
                    problems.push(format!(
                        "`[{section}]` in {shown} — only `[alias]` goes here. \
A source replacement, a rustc-wrapper, env or a runner bypasses the audit gates whole"
                    ));
                }
            }
            for item in tomlish::items(&text)? {
                if item.section.is_empty() {
                    problems.push(format!(
                        "the top-level key `{}` at {shown}:{}",
                        item.key, item.line
                    ));
                } else if item.key == "xtask" && item.string().as_deref() != Some("run -p xtask --")
                {
                    problems.push(format!(
                        "the `xtask` alias at {shown}:{} is not `run -p xtask --`: {}",
                        item.line, item.value
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The prose that quotes the minimum Rust version — the README's badge and sentence, the
/// contributing guide, the getting-started table — says what `Cargo.toml`'s `rust-version`
/// says. The crates and CI read the manifest; a sentence cannot, so this reads both and
/// compares, and a bump that forgets a sentence fails here.
fn msrv_in_prose(root: &Path, problems: &mut Vec<String>) -> Result<()> {
    let manifest = util::read_file(&root.join("Cargo.toml"))?;
    let Some(msrv) = tomlish::items(&manifest)?
        .into_iter()
        .find(|item| item.key == "rust-version")
        .and_then(|item| item.string())
    else {
        problems.push("Cargo.toml has no `rust-version`".to_owned());
        return Ok(());
    };
    for (file, needle) in [
        ("README.md", format!("rust-{msrv}%2B")),
        ("README.md", format!("minimum Rust version is {msrv}")),
        ("CONTRIBUTING.md", format!("minimum Rust version is {msrv}")),
        (
            "docs/guide/01-getting-started.md",
            format!("| Rust | {msrv} or newer"),
        ),
    ] {
        let text = util::read_file(&root.join(file))?;
        if !text.contains(&needle) {
            problems.push(format!(
                "{file} does not say the minimum Rust version is {msrv} (`{needle}` is not in it) — Cargo.toml's `rust-version` is the one source; update the sentence"
            ));
        }
    }
    Ok(())
}

fn vendor_dir(root: &Path, problems: &mut Vec<String>) {
    if root.join("vendor").exists() {
        problems.push(
            "there is a vendor/. Vendoring changes only the source compiled while leaving Cargo.lock as it is"
                .to_owned(),
        );
    }
}

/// Every manifest in the workspace.
fn manifest_paths(root: &Path, members: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = vec![root.join("Cargo.toml")];
    out.extend(members.iter().map(|dir| dir.join("Cargo.toml")));
    out
}

fn manifests(root: &Path, members: &[PathBuf], problems: &mut Vec<String>) -> Result<()> {
    for file in manifest_paths(root, members) {
        let text = util::read_file(&file)?;
        let shown = display(root, &file);
        let is_root = file == root.join("Cargo.toml");
        for section in tomlish::sections(&text)? {
            if section == "patch" || section.starts_with("patch.") || section == "replace" {
                problems.push(format!(
                    "`[{section}]` in {shown} — it changes the source while leaving the name and version as they are"
                ));
            }
            if section.starts_with("target.") && section.ends_with("dependencies") {
                problems.push(format!(
                    "`[{section}]` in {shown} — a dependency outside the audited targets \
(x86_64/aarch64-unknown-linux-gnu) appears in neither deps.allow nor cargo-deny"
                ));
            }
        }
        let items = tomlish::items(&text)?;
        if is_root {
            let forbidden = tomlish::find(&items, "workspace.lints.rust", "unsafe_code")
                .and_then(tomlish::Item::string);
            if forbidden.as_deref() != Some("forbid") {
                problems.push(format!(
                    "`[workspace.lints.rust] unsafe_code` in {shown} is not `forbid`"
                ));
            }
            internal_versions(&items, &shown, problems)?;
        } else {
            let on = tomlish::find(&items, "lints", "workspace").map(|item| item.value.as_str());
            if on != Some("true") {
                problems.push(format!(
                    "{shown} has no `[lints] workspace = true` — the workspace lints, \
unsafe_code = \"forbid\" among them, are not applied to this crate"
                ));
            }
        }
    }
    Ok(())
}

/// Whether the version of a workspace-internal crate in `[workspace.dependencies]` matches
/// `[workspace.package]`. `wildcards = "deny"` means the version cannot be dropped, so the value lives
/// in two places — and a mismatch blows up at the release tag. The root is the single source.
fn internal_versions(
    items: &[tomlish::Item],
    shown: &str,
    problems: &mut Vec<String>,
) -> Result<()> {
    let Some(package_version) =
        tomlish::find(items, "workspace.package", "version").and_then(tomlish::Item::string)
    else {
        problems.push(format!("{shown} has no `[workspace.package] version`"));
        return Ok(());
    };
    for item in items
        .iter()
        .filter(|item| item.section == "workspace.dependencies")
    {
        if !item.value.trim_start().starts_with('{') {
            continue;
        }
        let table = tomlish::inline_table(&item.value)?;
        let is_path = table.iter().any(|(key, _)| key == "path");
        if !is_path {
            continue;
        }
        let version = table
            .iter()
            .find(|(key, _)| key == "version")
            .and_then(|(_, value)| tomlish::unquote(value));
        if version.as_deref() != Some(package_version.as_str()) {
            problems.push(format!(
                "the `[workspace.dependencies] {}` version in {shown} differs from `[workspace.package] version = \"{package_version}\"`: {}",
                item.key,
                version.unwrap_or_else(|| "(none)".to_owned())
            ));
        }
    }
    Ok(())
}

/// Whether every package in `Cargo.lock` is either crates.io or a workspace member.
fn lockfile(root: &Path, members: &[PathBuf], problems: &mut Vec<String>) -> Result<()> {
    let file = root.join("Cargo.lock");
    if !file.is_file() {
        problems.push("there is no Cargo.lock. The lock file is committed".to_owned());
        return Ok(());
    }
    let names = member_names(members)?;
    let items = tomlish::items(&util::read_file(&file)?)?;
    let count = items
        .iter()
        .filter(|item| item.section == "package" && item.key == "name")
        .count();
    for index in 0..count {
        let field = |key: &str| -> Option<String> {
            items
                .iter()
                .find(|item| {
                    item.section == "package" && item.table_index == index && item.key == key
                })
                .and_then(tomlish::Item::string)
        };
        let Some(name) = field("name") else {
            continue;
        };
        let source = field("source");
        if source.as_deref() == Some(CRATES_IO) {
            continue;
        }
        if source.is_none() && names.contains(&name) {
            continue;
        }
        problems.push(format!(
            "the source of `{name}` in Cargo.lock is neither crates.io nor a workspace member: {}",
            source.unwrap_or_else(|| "(none)".to_owned())
        ));
    }
    Ok(())
}

fn member_names(members: &[PathBuf]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for dir in members {
        let file = dir.join("Cargo.toml");
        let items = tomlish::items(&util::read_file(&file)?)?;
        let name = tomlish::find(&items, "package", "name")
            .and_then(tomlish::Item::string)
            .ok_or_else(|| Error::new(format!("{} has no [package] name", file.display())))?;
        out.push(name);
    }
    Ok(out)
}

fn display(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::CRATES_IO;
    use crate::tomlish;
    use crate::util::Result;

    #[test]
    fn a_source_replacement_shows_up_as_a_section() -> Result<()> {
        let text = "[alias]\nxtask = \"run -p xtask --\"\n\n[source.crates-io]\nreplace-with = \"vendored-sources\"\n";
        let sections = tomlish::sections(text)?;
        assert!(sections.iter().any(|name| name.starts_with("source")));
        assert!(sections.iter().any(|name| name == "alias"));
        Ok(())
    }

    #[test]
    fn a_patched_package_loses_its_registry_source() -> Result<()> {
        let text = "[[package]]\nname = \"log\"\nversion = \"0.4.34\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
        let items = tomlish::items(text)?;
        let sources: Vec<Option<String>> = (0..2)
            .map(|index| {
                items
                    .iter()
                    .find(|item| {
                        item.section == "package"
                            && item.table_index == index
                            && item.key == "source"
                    })
                    .and_then(tomlish::Item::string)
            })
            .collect();
        assert_eq!(sources, vec![None, Some(CRATES_IO.to_owned())]);
        Ok(())
    }
}
