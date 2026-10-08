//! The duplicate-version gate.
//!
//! It gathers the two targets' duplicates with `cargo tree -d` and then takes out the versions
//! registered with a reason in `deny.toml [bans].skip`. Two or more of the same name left is a failure.

use crate::tomlish;
use crate::util::{self, Error, Result};
use crate::version::{Version, VersionReq};
use std::path::Path;

/// One entry of `deny.toml [bans].skip` (`name` or `name@req`).
#[derive(Debug, Clone)]
pub(crate) struct Skip {
    pub(crate) name: String,
    pub(crate) req: Option<VersionReq>,
}

impl Skip {
    fn covers(&self, name: &str, version: &Version) -> bool {
        self.name == name && self.req.as_ref().is_none_or(|req| req.matches(version))
    }
}

/// Gather `deny.toml`'s `[bans].skip` entries.
///
/// It uses the [`crate::tomlish`] scanner rather than a line-based heuristic. A `#` or `]` inside a
/// reason string, or several entries on one line, does not make an entry quietly disappear (which
/// would make dup-check fail falsely). It takes both the `skip = [ … ]` and the `[[bans.skip]]` forms,
/// and cargo-deny's old `{ name = …, version = … }` spelling is an error rather than something ignored.
pub(crate) fn parse_skip(text: &str) -> Result<Vec<Skip>> {
    let items = tomlish::items(text)?;
    let mut specs: Vec<String> = Vec::new();
    for item in &items {
        if item.section == "bans" && item.key == "skip" {
            for element in tomlish::array_elements(&item.value)? {
                specs.push(spec_of(&element, item.line)?);
            }
        } else if item.section == "bans.skip" {
            reject_legacy_key(&item.key, item.line)?;
            if item.key == "crate" {
                specs.push(quoted(&item.value, item.line)?);
            }
        }
    }
    specs.iter().map(|spec| parse_spec(spec)).collect()
}

fn reject_legacy_key(key: &str, line: usize) -> Result<()> {
    if matches!(key, "name" | "version") {
        return Err(Error::new(format!(
            "deny.toml:{line}: the old `{key} = …` spelling in `[bans].skip` is not used. Write it as `crate = \"name@req\"`"
        )));
    }
    Ok(())
}

fn quoted(value: &str, line: usize) -> Result<String> {
    tomlish::unquote(value)
        .ok_or_else(|| Error::new(format!("deny.toml:{line}: `{value}` is not a string")))
}

/// Take the crate specifier out of one element of the `skip` array.
/// It takes both the `"name@req"` and the `{ crate = "name@req", reason = "…" }` forms.
fn spec_of(element: &str, line: usize) -> Result<String> {
    if let Some(text) = tomlish::unquote(element) {
        return Ok(text);
    }
    let table = tomlish::inline_table(element)
        .map_err(|err| Error::new(format!("deny.toml:{line}: {err}")))?;
    for (key, _) in &table {
        reject_legacy_key(key, line)?;
    }
    let entry = table
        .iter()
        .find(|(key, _)| key == "crate")
        .ok_or_else(|| Error::new(format!("deny.toml:{line}: `{element}` has no `crate` key")))?;
    quoted(&entry.1, line)
}

fn parse_spec(spec: &str) -> Result<Skip> {
    match spec.split_once('@') {
        Some((name, req)) => Ok(Skip {
            name: name.trim().to_owned(),
            req: Some(VersionReq::parse(req).map_err(|err| {
                Error::new(format!(
                    "`{spec}` in deny.toml `[bans].skip`: {err}. xtask reads only this syntax (`=X.Y.Z` `^X.Y` `~X.Y` `*`)"
                ))
            })?),
        }),
        None => Ok(Skip {
            name: spec.trim().to_owned(),
            req: None,
        }),
    }
}

/// Take only the root lines (`name vX.Y.Z`) out of `cargo tree -d`'s output.
/// The lines below start with a tree glyph or whitespace and so are filtered out.
pub(crate) fn parse_dup_output(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let first = line.chars().next();
        if !first.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let cleaned = line.trim_end().replace(" (proc-macro)", "");
        let cleaned = cleaned.strip_suffix(" (*)").unwrap_or(&cleaned);
        let Some((name, rest)) = cleaned.split_once(' ') else {
            continue;
        };
        let version = rest.split_whitespace().next().unwrap_or("");
        let Some(version) = version.strip_prefix('v') else {
            continue;
        };
        let pair = (name.to_owned(), version.to_owned());
        if !out.contains(&pair) {
            out.push(pair);
        }
    }
    out
}

/// Pick out the duplicates that are still a problem because no `skip` covers them.
/// It hands back `(the crate's name, the versions left)`.
pub(crate) fn unresolved(
    dups: &[(String, String)],
    skips: &[Skip],
) -> Result<Vec<(String, Vec<String>)>> {
    let mut names: Vec<String> = Vec::new();
    for (name, _) in dups {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    names.sort();

    let mut out = Vec::new();
    for name in names {
        let mut kept: Vec<String> = Vec::new();
        for (candidate, version_text) in dups {
            if candidate != &name {
                continue;
            }
            let version = Version::parse(version_text)?;
            if !skips.iter().any(|skip| skip.covers(&name, &version)) {
                kept.push(version_text.clone());
            }
        }
        if kept.len() >= 2 {
            out.push((name, kept));
        }
    }
    Ok(out)
}

/// Stage 5's body. `true` on a pass.
pub(crate) fn check(root: &Path) -> Result<bool> {
    let deny = util::read_file(&root.join("deny.toml"))?;
    let skips = parse_skip(&deny)?;
    let cargo = util::cargo_bin();

    let mut dups: Vec<(String, String)> = Vec::new();
    for target in crate::tree::TARGETS {
        let args = [
            "tree",
            "--locked",
            "--workspace",
            "-e",
            "normal",
            "-d",
            "--all-features",
            "--target",
            target,
        ];
        let stdout = util::capture(root, &cargo, &args)?;
        for pair in parse_dup_output(&stdout) {
            if !dups.contains(&pair) {
                dups.push(pair);
            }
        }
    }

    let bad = unresolved(&dups, &skips)?;
    let mut names: Vec<&str> = dups.iter().map(|(name, _)| name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    println!(
        "duplicate versions: {} crates / {} version entries, {} entries in deny.toml [bans].skip",
        names.len(),
        dups.len(),
        skips.len()
    );
    if bad.is_empty() {
        println!("No duplicates outside skip.");
        return Ok(true);
    }
    for (name, versions) in &bad {
        println!(
            "duplicate versions (unapproved): {name} → {}   ← cargo tree -i {name}",
            versions.join(", ")
        );
    }
    println!("Register it with a reason in deny.toml [bans].skip, or unify the versions.");
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{parse_dup_output, parse_skip, unresolved};
    use crate::util::Result;

    /// The failure cases the review turned up: a `#`, `]` or `crate` inside a string, several entries
    /// on one line, and a plain string used as an array element.
    const TRICKY: &str = r#"
[bans]
skip = [
    { reason = "see #42", crate = "calloop@0.13" },
    { crate = "rustix@0.38", reason = "tracked in [bans] note" },
    { reason = "crate drift", crate = "syn@2" },
    { crate = "a@1" }, { crate = "b@2" },
    "thiserror@1",
]
"#;

    const DENY: &str = r#"
[advisories]
ignore = []

[bans]
multiple-versions = "deny"
skip = [
    { crate = "calloop@0.13", reason = "winit 0.30 ↔ sctk 0.20" },
    { crate = "rustix@0.38", reason = "the same" },
    { crate = "syn@2", reason = "a proc-macro generation change" },
]
skip-tree = []

[sources]
allow-git = []
"#;

    const TREE_D: &str = "\
calloop v0.13.0
├── smithay-client-toolkit v0.19.2
│   └── smithay-clipboard v0.7.2
└── winit v0.30.12

calloop v0.14.4
└── winit v0.31.0

syn v1.0.109 (proc-macro)
└── old-derive v0.1.0

syn v2.0.111
└── serde_derive v1.0.228 (proc-macro)
";

    #[test]
    fn quoting_and_layout_do_not_lose_entries() -> Result<()> {
        let skips = parse_skip(TRICKY)?;
        let names: Vec<&str> = skips.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["calloop", "rustix", "syn", "a", "b", "thiserror"]
        );
        Ok(())
    }

    #[test]
    fn reads_array_of_tables() -> Result<()> {
        let text =
            "[[bans.skip]]\ncrate = \"calloop@0.13\"\n\n[[bans.skip]]\ncrate = \"rustix@0.38\"\n";
        let names: Vec<String> = parse_skip(text)?.into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["calloop", "rustix"]);
        Ok(())
    }

    #[test]
    fn legacy_name_version_keys_are_an_error() {
        assert!(
            parse_skip("[bans]\nskip = [ { name = \"calloop\", version = \"0.13\" } ]\n").is_err()
        );
    }

    #[test]
    fn unsupported_version_ranges_are_reported() {
        assert!(parse_skip("[bans]\nskip = [ { crate = \"calloop@>=0.13\" } ]\n").is_err());
    }

    #[test]
    fn reads_only_the_bans_skip_list() -> Result<()> {
        let skips = parse_skip(DENY)?;
        let names: Vec<&str> = skips.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["calloop", "rustix", "syn"]);
        Ok(())
    }

    #[test]
    fn reads_root_lines_only() {
        let dups = parse_dup_output(TREE_D);
        assert_eq!(
            dups,
            vec![
                ("calloop".to_owned(), "0.13.0".to_owned()),
                ("calloop".to_owned(), "0.14.4".to_owned()),
                ("syn".to_owned(), "1.0.109".to_owned()),
                ("syn".to_owned(), "2.0.111".to_owned()),
            ]
        );
    }

    #[test]
    fn skipped_versions_resolve_the_duplicate() -> Result<()> {
        let skips = parse_skip(DENY)?;
        let dups = parse_dup_output(TREE_D);
        // calloop 0.13 is skipped → only 0.14 is left, so it is not a duplicate.
        // syn 2 is skipped, but 1.0.109 is left, so again there is only the one.
        assert_eq!(unresolved(&dups, &skips)?, Vec::new());
        Ok(())
    }

    #[test]
    fn unskipped_duplicates_fail() -> Result<()> {
        let skips = parse_skip("[bans]\nskip = []\n")?;
        let dups = parse_dup_output(TREE_D);
        let bad = unresolved(&dups, &skips)?;
        let names: Vec<&str> = bad.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["calloop", "syn"]);
        Ok(())
    }

    #[test]
    fn a_skip_without_version_covers_everything() -> Result<()> {
        let skips = parse_skip("[bans]\nskip = [ { crate = \"calloop\" } ]\n")?;
        let dups = parse_dup_output(TREE_D);
        let bad = unresolved(&dups, &skips)?;
        let names: Vec<&str> = bad.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["syn"]);
        Ok(())
    }

    #[test]
    fn ignores_skip_lists_of_other_sections() -> Result<()> {
        let text = "[licenses]\nskip = [ { crate = \"nope@1\" } ]\n[bans]\nskip = []\n";
        assert!(parse_skip(text)?.is_empty());
        Ok(())
    }
}
