//! The `deps.allow` `version-req` syntax: `=X.Y.Z` · `^X.Y[.Z]` · `~X.Y[.Z]` · `*`.
//!
//! `=` means **one exact version with all three places written out**. Its meaning parts company with
//! cargo's `=1.2` (= `>=1.2.0, <1.3.0`), so an `=` short of a place is a parse error.
//! With no prefix it is read as a caret. A caret follows cargo's rule: it pins the first non-zero
//! place from the left (= `0.x` pins the minor, everything else the major).
//! Only `0.0.z` follows cargo in pinning the patch as well (for an allowlist, the narrower is safer than the looser).

use crate::util::{Error, Result};
use std::fmt;

/// A minimal version type holding `X.Y.Z[-pre]`. Build metadata (`+...`) is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version {
    pub(crate) major: u64,
    pub(crate) minor: u64,
    pub(crate) patch: u64,
    pub(crate) pre: String,
}

impl Version {
    /// Whether it is a version carrying a pre-release tag.
    pub(crate) fn is_pre(&self) -> bool {
        !self.pre.is_empty()
    }

    fn triple(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }

    /// It takes `1`, `1.2`, `1.2.3`, `1.2.3-rc.1` and `1.2.3+meta`.
    /// An unstated place is 0. The second value is the number of places stated (1..=3).
    pub(crate) fn parse_counted(text: &str) -> Result<(Self, u8)> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(Error::new("an empty version string"));
        }
        let without_build = trimmed.split_once('+').map_or(trimmed, |(head, _)| head);
        let (core, pre) = without_build
            .split_once('-')
            .map_or((without_build, ""), |(head, tail)| (head, tail));
        let mut parts = [0_u64; 3];
        let mut count = 0_u8;
        for (index, piece) in core.split('.').enumerate() {
            if index >= 3 {
                return Err(Error::new(format!("too many version places: `{trimmed}`")));
            }
            let value = piece.parse::<u64>().map_err(|_| {
                Error::new(format!(
                    "`{piece}` in the version `{trimmed}` is not a number"
                ))
            })?;
            if let Some(slot) = parts.get_mut(index) {
                *slot = value;
            }
            count += 1;
        }
        let version = Self {
            major: parts.first().copied().unwrap_or(0),
            minor: parts.get(1).copied().unwrap_or(0),
            patch: parts.get(2).copied().unwrap_or(0),
            pre: pre.to_owned(),
        };
        Ok((version, count))
    }

    /// The place-count-free version of `parse_counted`.
    pub(crate) fn parse(text: &str) -> Result<Self> {
        Version::parse_counted(text).map(|(version, _)| version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Any,
    Exact,
    Caret,
    Tilde,
}

/// A `deps.allow` version requirement. It keeps the original text so it round-trips when written back to the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VersionReq {
    text: String,
    op: Op,
    base: Version,
    comps: u8,
}

impl VersionReq {
    /// Parse a requirement string.
    pub(crate) fn parse(text: &str) -> Result<Self> {
        let trimmed = text.trim();
        if trimmed == "*" {
            return Ok(Self {
                text: trimmed.to_owned(),
                op: Op::Any,
                base: Version {
                    major: 0,
                    minor: 0,
                    patch: 0,
                    pre: String::new(),
                },
                comps: 0,
            });
        }
        let (op, rest) = match trimmed.chars().next() {
            Some('=') => (Op::Exact, trimmed.get(1..).unwrap_or_default()),
            Some('^') => (Op::Caret, trimmed.get(1..).unwrap_or_default()),
            Some('~') => (Op::Tilde, trimmed.get(1..).unwrap_or_default()),
            Some('<' | '>') => {
                return Err(Error::new(format!(
                    "unsupported version requirement `{trimmed}`: only `=` `^` `~` `*` are used"
                )));
            }
            _ => (Op::Caret, trimmed),
        };
        let (base, comps) = Version::parse_counted(rest)?;
        // In cargo `=1.2` is `>=1.2.0, <1.3.0`, but the `deps.allow` syntax uses only `=X.Y.Z` (one exact
        // version). A spelling whose two meanings part company is not accepted.
        if op == Op::Exact && comps < 3 {
            return Err(Error::new(format!(
                "`{trimmed}`: `=` has to have all three places written out (`=X.Y.Z`). To mean a range, use `^`/`~`"
            )));
        }
        // A caret or tilde does not accept a pre-release (see `matches` below). But where the
        // requirement's own base is a pre-release, it becomes a dead line that cannot even accept itself.
        if matches!(op, Op::Caret | Op::Tilde) && base.is_pre() {
            return Err(Error::new(format!(
                "`{trimmed}`: a pre-release tag is approved only as `=X.Y.Z-pre`"
            )));
        }
        Ok(Self {
            text: trimmed.to_owned(),
            op,
            base,
            comps,
        })
    }

    /// Build a requirement allowing one exact version only (`=X.Y.Z`).
    pub(crate) fn exact(version: &Version) -> Self {
        let text = format!("={version}");
        Self {
            text,
            op: Op::Exact,
            base: version.clone(),
            comps: 3,
        }
    }

    /// Build the default caret form.
    ///
    /// 1.0 and above becomes `^X`, `0.Y.z` becomes `^0.Y`, and `0.0.z` and a pre-release are exact pins.
    /// A patch update does not break the gate, and only a minor (for 0.x) or major update shows up as an
    /// approval diff.
    pub(crate) fn caret(version: &Version) -> Self {
        if version.is_pre() || (version.major == 0 && version.minor == 0) {
            return Self::exact(version);
        }
        let (text, minor, comps) = if version.major == 0 {
            (format!("^0.{}", version.minor), version.minor, 2)
        } else {
            (format!("^{}", version.major), 0, 1)
        };
        Self {
            text,
            op: Op::Caret,
            base: Version {
                major: version.major,
                minor,
                patch: 0,
                pre: String::new(),
            },
            comps,
        }
    }

    /// Whether this requirement accepts a version.
    pub(crate) fn matches(&self, version: &Version) -> bool {
        match self.op {
            Op::Any => true,
            Op::Exact => *version == self.base,
            // A pre-release is not approved by a caret or tilde. It has to be stated with `=`.
            Op::Caret | Op::Tilde => {
                !version.is_pre()
                    && version.triple() >= self.base.triple()
                    && version.triple() < self.upper_bound()
            }
        }
    }

    /// The exclusive upper bound.
    fn upper_bound(&self) -> (u64, u64, u64) {
        let Version {
            major,
            minor,
            patch,
            ..
        } = self.base;
        match self.op {
            Op::Tilde => {
                if self.comps >= 2 {
                    (major, minor + 1, 0)
                } else {
                    (major + 1, 0, 0)
                }
            }
            _ => {
                if major != 0 {
                    (major + 1, 0, 0)
                } else if self.comps < 2 {
                    (1, 0, 0)
                } else if minor != 0 || self.comps < 3 {
                    (0, minor + 1, 0)
                } else {
                    (0, 0, patch + 1)
                }
            }
        }
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::{Version, VersionReq};
    use crate::util::Result;

    fn accepts(req: &str, version: &str) -> Result<bool> {
        Ok(VersionReq::parse(req)?.matches(&Version::parse(version)?))
    }

    #[test]
    fn exact_requires_identical_version() -> Result<()> {
        assert!(accepts("=0.36.1", "0.36.1")?);
        assert!(!accepts("=0.36.1", "0.36.2")?);
        assert!(!accepts("=0.36.1", "0.37.0")?);
        Ok(())
    }

    #[test]
    fn caret_pins_major_for_one_and_above() -> Result<()> {
        assert!(accepts("^1", "1.99.3")?);
        assert!(!accepts("^1", "2.0.0")?);
        assert!(accepts("^1.2.3", "1.2.4")?);
        assert!(!accepts("^1.2.3", "1.2.2")?);
        Ok(())
    }

    #[test]
    fn caret_pins_minor_for_zero_major() -> Result<()> {
        assert!(accepts("^0.24", "0.24.9")?);
        assert!(!accepts("^0.24", "0.25.0")?);
        assert!(!accepts("^0.24.3", "0.25.0")?);
        assert!(accepts("^0.24.3", "0.24.30")?);
        Ok(())
    }

    #[test]
    fn caret_without_prefix_is_the_default() -> Result<()> {
        assert!(accepts("1", "1.4.0")?);
        assert!(!accepts("1", "2.0.0")?);
        assert!(accepts("0.4", "0.4.22")?);
        assert!(!accepts("0.4", "0.5.0")?);
        Ok(())
    }

    #[test]
    fn tilde_pins_minor() -> Result<()> {
        assert!(accepts("~1.2", "1.2.9")?);
        assert!(!accepts("~1.2", "1.3.0")?);
        assert!(accepts("~1.2.3", "1.2.3")?);
        assert!(!accepts("~1.2.3", "1.2.2")?);
        assert!(accepts("~1", "1.9.0")?);
        assert!(!accepts("~1", "2.0.0")?);
        Ok(())
    }

    #[test]
    fn star_accepts_everything() -> Result<()> {
        assert!(accepts("*", "0.0.1")?);
        assert!(accepts("*", "123.4.5")?);
        Ok(())
    }

    #[test]
    fn prerelease_needs_an_exact_requirement() -> Result<()> {
        assert!(!accepts("^1.0.0", "1.1.0-rc.1")?);
        assert!(accepts("=1.1.0-rc.1", "1.1.0-rc.1")?);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_operators() {
        assert!(VersionReq::parse(">=1.0").is_err());
        assert!(VersionReq::parse("<2").is_err());
    }

    #[test]
    fn exact_requires_all_three_components() {
        // cargo reads `=1.2` as a range. A spelling whose meanings part company is not accepted at all.
        assert!(VersionReq::parse("=1.2").is_err());
        assert!(VersionReq::parse("=1").is_err());
        assert!(VersionReq::parse("=1.2.3").is_ok());
    }

    #[test]
    fn caret_and_tilde_reject_a_prerelease_base() {
        assert!(VersionReq::parse("^1.0.0-rc.1").is_err());
        assert!(VersionReq::parse("~1.0.0-rc.1").is_err());
        assert!(VersionReq::parse("=1.0.0-rc.1").is_ok());
    }

    #[test]
    fn caret_default_follows_the_design_rule() -> Result<()> {
        let caret = |text: &str| -> Result<String> {
            Ok(VersionReq::caret(&Version::parse(text)?).to_string())
        };
        assert_eq!(caret("1.0.4")?, "^1");
        assert_eq!(caret("2.13.1")?, "^2");
        assert_eq!(caret("0.36.1")?, "^0.36");
        assert_eq!(caret("0.4.28")?, "^0.4");
        // 0.0.z and a pre-release have no room to widen, so they are exact pins.
        assert_eq!(caret("0.0.3")?, "=0.0.3");
        assert_eq!(caret("1.0.0-rc.1")?, "=1.0.0-rc.1");
        // A requirement built has to accept itself.
        for text in ["1.0.4", "0.36.1", "0.0.3", "1.0.0-rc.1"] {
            let version = Version::parse(text)?;
            assert!(VersionReq::caret(&version).matches(&version), "{text}");
        }
        Ok(())
    }

    #[test]
    fn requirement_text_round_trips() -> Result<()> {
        let req = VersionReq::parse(" ^0.36 ")?;
        assert_eq!(req.to_string(), "^0.36");
        let exact = VersionReq::exact(&Version::parse("1.2.3")?);
        assert_eq!(exact.to_string(), "=1.2.3");
        Ok(())
    }

    #[test]
    fn version_parsing_drops_build_metadata() -> Result<()> {
        let version = Version::parse("1.2.3+abc")?;
        assert_eq!(version.to_string(), "1.2.3");
        let pre = Version::parse("1.2.3-rc.1")?;
        assert!(pre.is_pre());
        assert_eq!(pre.to_string(), "1.2.3-rc.1");
        assert!(Version::parse("1.x.3").is_err());
        Ok(())
    }
}
