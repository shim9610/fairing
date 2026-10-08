//! The icon compiler: `assets/icons/*.svg` → `crates/fairing-widgets/src/icons/generated.rs`.
//!
//! The output is deterministic: file names in ascending order, coordinates in the `f32` round-trip
//! representation, and `\n` for the line breaks.

use crate::path::Seg;
use crate::svg::{self, Icon};
use crate::util::{self, Error, Result};
use std::path::{Path, PathBuf};

/// The SVG source directory.
pub(crate) fn source_dir(root: &Path) -> PathBuf {
    root.join("assets").join("icons")
}

/// The generated file's path.
pub(crate) fn output_path(root: &Path) -> PathBuf {
    root.join("crates")
        .join("fairing-widgets")
        .join("src")
        .join("icons")
        .join("generated.rs")
}

/// Read the sources and build the icon list in name order. With no directory, the list is empty.
pub(crate) fn load(root: &Path) -> Result<Vec<Icon>> {
    let dir = source_dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .map_err(|err| Error::new(format!("cannot read {}: {err}", dir.display())))?;
    for entry in entries {
        let path = entry
            .map_err(|err| Error::new(format!("cannot read an entry of {}: {err}", dir.display())))?
            .path();
        if path.extension().is_some_and(|ext| ext == "svg") {
            files.push(path);
        }
    }
    files.sort();

    let mut icons: Vec<Icon> = Vec::new();
    for file in &files {
        let name = file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| Error::new(format!("cannot read the name of {}", file.display())))?;
        check_name(name)?;
        if icons.iter().any(|icon| icon.name == name) {
            return Err(Error::new(format!("the icon name is duplicated: {name}")));
        }
        let icon = svg::parse_icon(name, &util::read_file(file)?)?;
        validate(&icon)?;
        icons.push(icon);
    }
    icons.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(icons)
}

/// An icon name is an identifier: it starts with a lowercase letter or a digit and adds only `-`
/// and `_`. It goes into the generated code as a string literal, so a file name is not simply trusted.
fn check_name(name: &str) -> Result<()> {
    let first_ok = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let rest_ok = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'));
    if first_ok && rest_ok {
        return Ok(());
    }
    Err(Error::new(format!(
        "the icon name `{name}` is outside the identifier rule (`[a-z0-9][a-z0-9_-]*`)"
    )))
}

/// The coordinates a segment holds.
fn coords(seg: Seg) -> Vec<f32> {
    match seg {
        Seg::M(x1, y1) | Seg::L(x1, y1) => vec![x1, y1],
        Seg::Q(x1, y1, x2, y2) => vec![x1, y1, x2, y2],
        Seg::C(x1, y1, x2, y2, x3, y3) => vec![x1, y1, x2, y2, x3, y3],
        Seg::Z => Vec::new(),
    }
}

/// Catch values the generated code would not compile with (NaN, infinity) beforehand, and check a filled icon's convexity.
fn validate(icon: &Icon) -> Result<()> {
    let finite = icon
        .segs
        .iter()
        .all(|seg| coords(*seg).iter().all(|value| value.is_finite()));
    if !finite {
        return Err(Error::new(format!(
            "{}.svg produces a coordinate that is not finite",
            icon.name
        )));
    }
    check_convex_fill(icon)
}

/// The flattening subdivision count used for the check. The same as the runtime cap (16), so it looks at the densest case.
const CHECK_SUBDIVISIONS: usize = 16;

/// The cross-product tolerance for the convexity decision. The coordinates are on the 24-grid, so below this it counts as a straight line.
const CONVEX_EPSILON: f64 = 1.0e-3;

/// Flatten a segment list into per-subpath point lists. The same rule as `crates/fairing`'s `flatten`
/// (a drawing command after a `Z` carries on from the subpath's starting point).
// The subdivision count is 16, so `usize → f64` is lossless (the reason for the exception to the
// workspace's pedantic cast lints).
#[allow(clippy::cast_precision_loss)]
fn subpaths(segs: &[Seg]) -> Vec<Vec<(f64, f64)>> {
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    let mut cursor = (0.0_f64, 0.0_f64);
    let mut pending: Option<(f64, f64)> = None;
    let flush = |current: &mut Vec<(f64, f64)>, out: &mut Vec<Vec<(f64, f64)>>| {
        if current.len() >= 3 {
            out.push(std::mem::take(current));
        } else {
            current.clear();
        }
    };
    for seg in segs {
        // Before a drawing command, the starting point from just after a `Z` is restored.
        if !matches!(seg, Seg::M(..) | Seg::Z) && current.is_empty() {
            if let Some(start) = pending.take() {
                cursor = start;
                current.push(start);
            }
        }
        match *seg {
            Seg::M(x, y) => {
                flush(&mut current, &mut out);
                pending = None;
                cursor = (f64::from(x), f64::from(y));
                current.push(cursor);
            }
            Seg::L(x, y) => {
                cursor = (f64::from(x), f64::from(y));
                current.push(cursor);
            }
            Seg::Q(cx, cy, x, y) => {
                let p0 = cursor;
                let p1 = (f64::from(cx), f64::from(cy));
                let p2 = (f64::from(x), f64::from(y));
                for step in 1..=CHECK_SUBDIVISIONS {
                    let t = step as f64 / CHECK_SUBDIVISIONS as f64;
                    let u = 1.0 - t;
                    current.push((
                        u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
                        u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
                    ));
                }
                cursor = p2;
            }
            Seg::C(ax, ay, bx, by, x, y) => {
                let p0 = cursor;
                let p1 = (f64::from(ax), f64::from(ay));
                let p2 = (f64::from(bx), f64::from(by));
                let p3 = (f64::from(x), f64::from(y));
                for step in 1..=CHECK_SUBDIVISIONS {
                    let t = step as f64 / CHECK_SUBDIVISIONS as f64;
                    let u = 1.0 - t;
                    current.push((
                        u * u * u * p0.0
                            + 3.0 * u * u * t * p1.0
                            + 3.0 * u * t * t * p2.0
                            + t * t * t * p3.0,
                        u * u * u * p0.1
                            + 3.0 * u * u * t * p1.1
                            + 3.0 * u * t * t * p2.1
                            + t * t * t * p3.1,
                    ));
                }
                cursor = p3;
            }
            Seg::Z => {
                pending = current.first().copied();
                if let Some(start) = pending {
                    cursor = start;
                }
                flush(&mut current, &mut out);
            }
        }
    }
    flush(&mut current, &mut out);
    out
}

/// Whether a closed polygon is convex. Every neighbouring pair of edges has to have the same cross-product sign.
fn is_convex(points: &[(f64, f64)]) -> bool {
    let count = points.len();
    if count < 4 {
        return true;
    }
    let mut sign = 0.0_f64;
    for index in 0..count {
        let (Some(a), Some(b), Some(c)) = (
            points.get(index),
            points.get((index + 1) % count),
            points.get((index + 2) % count),
        ) else {
            return true;
        };
        let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
        if cross.abs() <= CONVEX_EPSILON {
            continue;
        }
        if sign == 0.0 {
            sign = cross;
        } else if sign.signum() != cross.signum() {
            return false;
        }
    }
    true
}

/// A filled icon's subpaths have to be convex.
///
/// `epaint`'s `fill_closed_path` is a triangle fan from point 0 and so fills a concave polygon wrongly.
/// The design settled on "no runtime triangulation · pre-decomposed into convex pieces at the xtask
/// stage", so it is stopped here, fail-closed. A stroked icon (`fill = false`) is not a candidate.
fn check_convex_fill(icon: &Icon) -> Result<()> {
    if !icon.fill {
        return Ok(());
    }
    for (index, points) in subpaths(&icon.segs).iter().enumerate() {
        if !is_convex(points) {
            return Err(Error::new(format!(
                "filled subpath {index} of {}.svg is concave. Split it into convex pieces and export \
                 that, or draw it as a fill=\"none\" stroke (there is no runtime triangulation)",
                icon.name
            )));
        }
    }
    Ok(())
}

/// An `f32` as a Rust float literal. It uses the shortest representation that round-trips.
fn number(value: f32) -> String {
    let printed = format!("{value}");
    let mut text = if printed == "-0" {
        "0".to_owned()
    } else {
        printed
    };
    if !text.contains(['.', 'e', 'E']) {
        text.push_str(".0");
    }
    text
}

fn seg_literal(seg: Seg) -> String {
    match seg {
        Seg::M(x, y) => format!("Seg::M({}, {})", number(x), number(y)),
        Seg::L(x, y) => format!("Seg::L({}, {})", number(x), number(y)),
        Seg::Q(cx, cy, x, y) => format!(
            "Seg::Q({}, {}, {}, {})",
            number(cx),
            number(cy),
            number(x),
            number(y)
        ),
        Seg::C(ax, ay, bx, by, x, y) => format!(
            "Seg::C({}, {}, {}, {}, {}, {})",
            number(ax),
            number(ay),
            number(bx),
            number(by),
            number(x),
            number(y)
        ),
        Seg::Z => "Seg::Z".to_owned(),
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The generated file's body. For the same icon list it is always the same string.
pub(crate) fn render(icons: &[Icon]) -> String {
    let mut out = String::from(
        "// @generated by cargo xtask icons - do not edit\n\
         // Source: assets/icons/*.svg · regenerate: `cargo xtask icons` · check: `cargo xtask icons --check`\n\
         // Coordinates are the 24x24 design grid.\n\
         // The geometry is a Lucide 1.39.0 subset (ISC; some icons from Feather, MIT).\n\
         // Both licence texts are in this crate's LICENSE-lucide.\n\n\
         // These coordinates come out of shapes by machine, so they do not follow the literal\n\
         // rules meant for humans to read. One circle or arc is enough to trip the workspace\n\
         // lints (clippy pedantic, -D warnings) here, so the whole generated file is exempt.\n\
         #![allow(\n\
         \x20   clippy::unreadable_literal,\n\
         \x20   clippy::approx_constant,\n\
         \x20   clippy::excessive_precision\n\
         )]\n\n",
    );
    if icons.iter().all(|icon| icon.segs.is_empty()) {
        out.push_str("use super::IconDef;\n\n");
    } else {
        out.push_str("use super::{IconDef, Seg};\n\n");
    }
    out.push_str("/// The built-in icon table, generated from the source SVGs. Sorted by name.\n");
    // Audit stage 1 is `cargo fmt --all --check` and stage 11 is `xtask icons --check`. Where rustfmt
    // folds a short `segs: &[..]` onto one line, the two gates break each other, so it is stopped from
    // touching the table. The indentation has this generator as its single source.
    out.push_str("#[rustfmt::skip]\n");
    if icons.is_empty() {
        out.push_str("pub const ICONS: &[IconDef] = &[];\n");
        return out;
    }
    out.push_str("pub const ICONS: &[IconDef] = &[\n");
    for icon in icons {
        out.push_str("    IconDef {\n        name: \"");
        out.push_str(&escape(&icon.name));
        out.push_str("\",\n        segs: &[\n");
        for seg in &icon.segs {
            out.push_str("            ");
            out.push_str(&seg_literal(*seg));
            out.push_str(",\n");
        }
        out.push_str("        ],\n        fill: ");
        out.push_str(if icon.fill { "true" } else { "false" });
        out.push_str(",\n    },\n");
    }
    out.push_str("];\n");
    out
}

/// Stage 11's body. With `check` it touches no file and only looks at whether it is up to date.
pub(crate) fn run(root: &Path, check: bool) -> Result<bool> {
    let icons = load(root)?;
    let expected = render(&icons);
    let file = output_path(root);
    if !check {
        util::write_file(&file, &expected)?;
        println!(
            "refreshed {}: {} icons (from {})",
            file.display(),
            icons.len(),
            source_dir(root).display()
        );
        return Ok(true);
    }
    if !file.exists() {
        // Even at zero icons, `icons/mod.rs` declares `mod generated;`, so with no file it would not
        // compile in the first place. "Passing where neither exists" was never a possible state.
        println!(
            "{} is missing. Generate it with `cargo xtask icons`.",
            file.display()
        );
        return Ok(false);
    }
    if util::read_file(&file)? == expected {
        println!("{} is up to date ({} icons).", file.display(), icons.len());
        return Ok(true);
    }
    println!(
        "{} differs from the sources. Refresh it with `cargo xtask icons`.",
        file.display()
    );
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{check_name, number, render, validate};
    use crate::path::Seg;
    use crate::svg::Icon;

    fn icon(name: &str, segs: Vec<Seg>, fill: bool) -> Icon {
        Icon {
            name: name.to_owned(),
            segs,
            fill,
        }
    }

    #[test]
    fn formats_floats_as_valid_rust_literals() {
        assert_eq!(number(12.0), "12.0");
        assert_eq!(number(-0.0), "0.0");
        assert_eq!(number(0.5), "0.5");
        assert_eq!(number(-3.25), "-3.25");
    }

    #[test]
    fn renders_the_expected_shape() {
        let icons = vec![
            icon("minus", vec![Seg::M(5.0, 12.0), Seg::L(19.0, 12.0)], false),
            icon(
                "dot",
                vec![Seg::C(1.0, 2.0, 3.0, 4.0, 5.0, 6.0), Seg::Z],
                true,
            ),
        ];
        let text = render(&icons);
        assert!(text.starts_with("// @generated by cargo xtask icons - do not edit\n"));
        assert!(text.contains("use super::{IconDef, Seg};"));
        assert!(text.contains("pub const ICONS: &[IconDef] = &[\n"));
        assert!(text.contains("        name: \"minus\",\n"));
        assert!(text.contains("            Seg::M(5.0, 12.0),\n"));
        assert!(text.contains("            Seg::C(1.0, 2.0, 3.0, 4.0, 5.0, 6.0),\n"));
        assert!(text.contains("        fill: true,\n"));
        assert!(text.ends_with("];\n"));
    }

    /// `#[rustfmt::skip]` goes on the table so that audit stage 1 (`cargo fmt --all --check`) and stage
    /// 11 (`icons --check`) do not break each other.
    #[test]
    fn the_table_is_fenced_off_from_rustfmt() {
        for icons in [
            Vec::new(),
            vec![icon(
                "minus",
                vec![Seg::M(5.0, 12.0), Seg::L(19.0, 12.0)],
                false,
            )],
        ] {
            let text = render(&icons);
            assert!(
                text.contains("#[rustfmt::skip]\npub const ICONS:"),
                "{text}"
            );
        }
    }

    #[test]
    fn empty_input_still_compiles() {
        let text = render(&[]);
        assert!(text.contains("use super::IconDef;"));
        assert!(!text.contains("Seg"));
        assert!(text.ends_with("pub const ICONS: &[IconDef] = &[];\n"));
    }

    #[test]
    fn generated_files_carry_the_lint_exception() {
        // Circle and arc coordinates (15.3137, 3.1416, say) trip unreadable_literal and approx_constant.
        // Audit stage 2 is -D warnings, so without an exception it fails the moment an icon comes in.
        let text = render(&[icon(
            "dot",
            vec![Seg::C(18.0, 15.3137, 15.3137, 18.0, 12.0, 18.0), Seg::Z],
            true,
        )]);
        assert!(text.contains("#![allow("), "{text}");
        assert!(text.contains("clippy::unreadable_literal"));
        assert!(text.contains("clippy::approx_constant"));
        assert!(text.contains("clippy::excessive_precision"));
        // An inner attribute has to come before any item.
        assert!(
            matches!(
                (text.find("#!["), text.find("use super::")),
                (Some(allow), Some(first_use)) if allow < first_use
            ),
            "{text}"
        );
    }

    #[test]
    fn icon_names_follow_the_identifier_rule() {
        assert!(check_name("wifi").is_ok());
        assert!(check_name("battery-low").is_ok());
        assert!(check_name("arrow_up2").is_ok());
        assert!(check_name("Wifi").is_err());
        assert!(check_name("bad name").is_err());
        assert!(check_name("-lead").is_err());
        assert!(check_name("").is_err());
        assert!(check_name("quote\"inside").is_err());
    }

    #[test]
    fn rendering_is_deterministic() {
        let icons = vec![icon("a", vec![Seg::M(1.0, 1.0), Seg::Z], false)];
        assert_eq!(render(&icons), render(&icons));
    }

    /// A convex fill passes and a concave fill is fail-closed.
    #[test]
    fn filled_icons_must_be_convex() {
        let square = vec![
            Seg::M(4.0, 4.0),
            Seg::L(20.0, 4.0),
            Seg::L(20.0, 20.0),
            Seg::L(4.0, 20.0),
            Seg::Z,
        ];
        assert!(validate(&icon("square", square.clone(), true)).is_ok());

        // An L shape — the sign flips at the inner corner.
        let ell = vec![
            Seg::M(4.0, 4.0),
            Seg::L(12.0, 4.0),
            Seg::L(12.0, 12.0),
            Seg::L(20.0, 12.0),
            Seg::L(20.0, 20.0),
            Seg::L(4.0, 20.0),
            Seg::Z,
        ];
        assert!(validate(&icon("ell", ell.clone(), true)).is_err());
        // The same shape is not a candidate where the icon is stroked.
        assert!(validate(&icon("ell", ell, false)).is_ok());
    }

    /// A circle (four cubic pieces) has to be judged convex even after flattening.
    #[test]
    fn a_flattened_circle_counts_as_convex() {
        let kappa = 0.552_284_8_f32 * 10.0;
        let circle = vec![
            Seg::M(22.0, 12.0),
            Seg::C(22.0, 12.0 + kappa, 12.0 + kappa, 22.0, 12.0, 22.0),
            Seg::C(12.0 - kappa, 22.0, 2.0, 12.0 + kappa, 2.0, 12.0),
            Seg::C(2.0, 12.0 - kappa, 12.0 - kappa, 2.0, 12.0, 2.0),
            Seg::C(12.0 + kappa, 2.0, 22.0, 12.0 - kappa, 22.0, 12.0),
            Seg::Z,
        ];
        assert!(validate(&icon("circle", circle, true)).is_ok());
    }

    /// A star is concave — one broken subpath out of several has to be caught.
    #[test]
    fn one_concave_subpath_fails_the_whole_icon() {
        let mut segs = vec![Seg::M(2.0, 2.0), Seg::L(8.0, 2.0), Seg::L(8.0, 8.0), Seg::Z];
        segs.extend([
            Seg::M(12.0, 4.0),
            Seg::L(14.0, 10.0),
            Seg::L(20.0, 12.0),
            Seg::L(14.0, 14.0),
            Seg::L(12.0, 20.0),
            Seg::L(10.0, 14.0),
            Seg::L(4.0, 12.0),
            Seg::L(10.0, 10.0),
            Seg::Z,
        ]);
        assert!(validate(&icon("star", segs, true)).is_err());
    }
}
