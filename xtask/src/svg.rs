//! The minimal parser that reads `assets/icons/*.svg` ("the icon compiler").
//!
//! Not a general-purpose SVG parser. It handles only the subset the icon set actually uses:
//! `path` (`d`), `circle`, `ellipse`, `rect` (`rx`/`ry`), `line`, `polyline`, `polygon`, and the
//! `svg` and `g` that hold them.
//!
//! **Anything outside the subset is an error, not something ignored.** An element such as `use`,
//! `text` or `image`, or a `transform` attribute, stops the compile rather than producing an icon
//! quietly missing a piece. A shape **inside** a container such as `defs`, `clipPath`, `mask` or
//! `symbol` is not drawn on screen and so is skipped.
//! A `g`'s `fill` is inherited by its children (the Figma and Material export convention).

use crate::path::{self, Pt, Seg, KAPPA};
use crate::util::{self, Error, Result};

/// One icon. The coordinate system is the 24×24 design grid.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Icon {
    /// The file name with the extension taken off.
    pub(crate) name: String,
    /// The flattened path.
    pub(crate) segs: Vec<Seg>,
    /// Whether it is a filled icon. True where even one shape is not `fill="none"`.
    pub(crate) fill: bool,
}

/// The length of one side of the design grid.
const GRID: f64 = 24.0;

/// A container whose contents are not drawn on screen. The shapes inside are skipped.
const HIDDEN: [&str; 9] = [
    "defs", "clipPath", "mask", "symbol", "pattern", "marker", "metadata", "title", "desc",
];

/// The shape elements.
const SHAPES: [&str; 7] = [
    "path", "circle", "ellipse", "rect", "line", "polyline", "polygon",
];

/// Turn one SVG source into an icon.
pub(crate) fn parse_icon(name: &str, source: &str) -> Result<Icon> {
    let elements = scan(source);
    let root = elements
        .iter()
        .find(|element| element.name == "svg")
        .ok_or_else(|| Error::new(format!("{name}.svg has no <svg> root")))?;

    let mut segs: Vec<Seg> = Vec::new();
    let mut fill = false;
    for (index, element) in elements.iter().enumerate() {
        if hidden(&elements, index) {
            continue;
        }
        check_supported(element, name)?;
        let Some(mut produced) = shape_of(element, name)? else {
            continue;
        };
        if fillable(&element.name) && resolve_fill(&elements, index) {
            fill = true;
        }
        segs.append(&mut produced);
    }
    if segs.is_empty() {
        return Err(Error::new(format!(
            "found no shape to draw in {name}.svg (supported: path, circle, ellipse, rect, line, polyline, polygon)"
        )));
    }
    let (min_x, min_y, width, height) = view_box(root, name)?;
    fit_to_grid(&mut segs, min_x, min_y, width, height);
    Ok(Icon {
        name: name.to_owned(),
        segs,
        fill,
    })
}

fn fillable(name: &str) -> bool {
    matches!(name, "path" | "circle" | "ellipse" | "rect" | "polygon")
}

/// Whether an ancestor is a container that does not appear on screen.
fn hidden(elements: &[Element], index: usize) -> bool {
    let Some(element) = elements.get(index) else {
        return true;
    };
    element
        .parents
        .iter()
        .filter_map(|parent| elements.get(*parent))
        .any(|parent| HIDDEN.contains(&parent.name.as_str()))
}

/// An error where it is outside the supported subset. A dedicated parser does not throw away what it does not know.
fn check_supported(element: &Element, icon: &str) -> Result<()> {
    let known = element.name == "svg"
        || element.name == "g"
        || SHAPES.contains(&element.name.as_str())
        || HIDDEN.contains(&element.name.as_str());
    if !known {
        return Err(Error::new(format!(
            "<{}> in {icon}.svg is not supported (supported: svg, g, {})",
            element.name,
            SHAPES.join(", ")
        )));
    }
    if element.attr("transform").is_some() {
        return Err(Error::new(format!(
            "<{}> in {icon}.svg has a transform. Export an SVG with the coordinates already applied",
            element.name
        )));
    }
    Ok(())
}

/// Whether the value means "do not paint", such as `none` or `transparent` (case-insensitively).
fn is_no_paint(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value == "none" || value == "transparent"
}

fn is_zero(value: &str) -> bool {
    value
        .trim()
        .parse::<f64>()
        .is_ok_and(|v| v.abs() < f64::EPSILON)
}

/// Take one attribute's value out of `style="fill: none; stroke: red"`.
fn style_value(style: &str, property: &str) -> Option<String> {
    style.split(';').find_map(|piece| {
        let (key, value) = piece.split_once(':')?;
        (key.trim().eq_ignore_ascii_case(property)).then(|| value.trim().to_owned())
    })
}

/// Whether this element settles the fill itself. `None` (inherit) where it does not.
fn fill_setting(element: &Element) -> Option<bool> {
    if let Some(style) = element.attr("style") {
        if style_value(style, "fill-opacity").is_some_and(|value| is_zero(&value)) {
            return Some(false);
        }
        if let Some(value) = style_value(style, "fill") {
            return Some(!is_no_paint(&value));
        }
    }
    if element.attr("fill-opacity").is_some_and(is_zero) {
        return Some(false);
    }
    element.attr("fill").map(|value| !is_no_paint(value))
}

/// Resolve `fill` by inheritance, element → ancestor → root. Where nobody settles it, it is filled,
/// per the SVG default.
fn resolve_fill(elements: &[Element], index: usize) -> bool {
    let Some(element) = elements.get(index) else {
        return true;
    };
    if let Some(decision) = fill_setting(element) {
        return decision;
    }
    for parent in element.parents.iter().rev() {
        if let Some(decision) = elements.get(*parent).and_then(fill_setting) {
            return decision;
        }
    }
    true
}

fn shape_of(element: &Element, icon: &str) -> Result<Option<Vec<Seg>>> {
    let segs = match element.name.as_str() {
        "path" => match element.attr("d") {
            Some(data) => path::parse_path(data)
                .map_err(|err| Error::new(format!("{icon}.svg <path>: {err}")))?,
            None => return Ok(None),
        },
        "circle" => {
            let radius = number(element, "r", 0.0, icon)?;
            ellipse(
                number(element, "cx", 0.0, icon)?,
                number(element, "cy", 0.0, icon)?,
                radius,
                radius,
            )
        }
        "ellipse" => ellipse(
            number(element, "cx", 0.0, icon)?,
            number(element, "cy", 0.0, icon)?,
            number(element, "rx", 0.0, icon)?,
            number(element, "ry", 0.0, icon)?,
        ),
        "rect" => rect(element, icon)?,
        "line" => vec![
            Seg::M(
                util::to_f32(number(element, "x1", 0.0, icon)?),
                util::to_f32(number(element, "y1", 0.0, icon)?),
            ),
            Seg::L(
                util::to_f32(number(element, "x2", 0.0, icon)?),
                util::to_f32(number(element, "y2", 0.0, icon)?),
            ),
        ],
        "polyline" => polyline(element, icon, false)?,
        "polygon" => polyline(element, icon, true)?,
        _ => return Ok(None),
    };
    if segs.is_empty() {
        return Ok(None);
    }
    Ok(Some(segs))
}

/// An ellipse (a circle included) as four cubic pieces.
fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<Seg> {
    if rx <= 0.0 || ry <= 0.0 {
        return Vec::new();
    }
    let (hx, hy) = (KAPPA * rx, KAPPA * ry);
    let mut out = vec![Seg::M(util::to_f32(cx + rx), util::to_f32(cy))];
    let quarters: [(Pt, Pt, Pt); 4] = [
        ((cx + rx, cy + hy), (cx + hx, cy + ry), (cx, cy + ry)),
        ((cx - hx, cy + ry), (cx - rx, cy + hy), (cx - rx, cy)),
        ((cx - rx, cy - hy), (cx - hx, cy - ry), (cx, cy - ry)),
        ((cx + hx, cy - ry), (cx + rx, cy - hy), (cx + rx, cy)),
    ];
    for (first, second, end) in quarters {
        out.push(cubic(first, second, end));
    }
    out.push(Seg::Z);
    out
}

fn cubic(first: Pt, second: Pt, end: Pt) -> Seg {
    Seg::C(
        util::to_f32(first.0),
        util::to_f32(first.1),
        util::to_f32(second.0),
        util::to_f32(second.1),
        util::to_f32(end.0),
        util::to_f32(end.1),
    )
}

fn line_to(point: Pt) -> Seg {
    Seg::L(util::to_f32(point.0), util::to_f32(point.1))
}

fn rect(element: &Element, icon: &str) -> Result<Vec<Seg>> {
    let x = number(element, "x", 0.0, icon)?;
    let y = number(element, "y", 0.0, icon)?;
    let width = number(element, "width", 0.0, icon)?;
    let height = number(element, "height", 0.0, icon)?;
    if width <= 0.0 || height <= 0.0 {
        return Ok(Vec::new());
    }
    let (rx, ry) = corner_radii(element, icon, width, height)?;
    if rx <= 0.0 || ry <= 0.0 {
        return Ok(vec![
            Seg::M(util::to_f32(x), util::to_f32(y)),
            line_to((x + width, y)),
            line_to((x + width, y + height)),
            line_to((x, y + height)),
            Seg::Z,
        ]);
    }
    Ok(rounded_rect(x, y, width, height, rx, ry))
}

/// Settle `rx` / `ry` by the SVG rules: with only one, the other matches it, and neither exceeds half the side.
fn corner_radii(element: &Element, icon: &str, width: f64, height: f64) -> Result<(f64, f64)> {
    let horizontal = if element.attr("rx").is_some() {
        Some(number(element, "rx", 0.0, icon)?)
    } else {
        None
    };
    let vertical = if element.attr("ry").is_some() {
        Some(number(element, "ry", 0.0, icon)?)
    } else {
        None
    };
    let (rx, ry) = match (horizontal, vertical) {
        (Some(first), Some(second)) => (first, second),
        (Some(only), None) | (None, Some(only)) => (only, only),
        (None, None) => (0.0, 0.0),
    };
    Ok((rx.clamp(0.0, width / 2.0), ry.clamp(0.0, height / 2.0)))
}

fn rounded_rect(x: f64, y: f64, width: f64, height: f64, rx: f64, ry: f64) -> Vec<Seg> {
    let (hx, hy) = (KAPPA * rx, KAPPA * ry);
    let (right, bottom) = (x + width, y + height);
    vec![
        Seg::M(util::to_f32(x + rx), util::to_f32(y)),
        line_to((right - rx, y)),
        cubic((right - rx + hx, y), (right, y + ry - hy), (right, y + ry)),
        line_to((right, bottom - ry)),
        cubic(
            (right, bottom - ry + hy),
            (right - rx + hx, bottom),
            (right - rx, bottom),
        ),
        line_to((x + rx, bottom)),
        cubic(
            (x + rx - hx, bottom),
            (x, bottom - ry + hy),
            (x, bottom - ry),
        ),
        line_to((x, y + ry)),
        cubic((x, y + ry - hy), (x + rx - hx, y), (x + rx, y)),
        Seg::Z,
    ]
}

fn polyline(element: &Element, icon: &str, close: bool) -> Result<Vec<Seg>> {
    let Some(text) = element.attr("points") else {
        return Ok(Vec::new());
    };
    let points = path::parse_points(text)
        .map_err(|err| Error::new(format!("{icon}.svg <{}>: {err}", element.name)))?;
    let Some(first) = points.first() else {
        return Ok(Vec::new());
    };
    let mut out = vec![Seg::M(util::to_f32(first.0), util::to_f32(first.1))];
    for point in points.iter().skip(1) {
        out.push(line_to(*point));
    }
    if close {
        out.push(Seg::Z);
    }
    Ok(out)
}

fn number(element: &Element, key: &str, default: f64, icon: &str) -> Result<f64> {
    let Some(text) = element.attr(key) else {
        return Ok(default);
    };
    let cleaned = text.trim().trim_end_matches("px").trim();
    cleaned.parse::<f64>().map_err(|_| {
        Error::new(format!(
            "cannot read {key}=\"{text}\" of <{}> in {icon}.svg as a number",
            element.name
        ))
    })
}

/// The coordinate system. With no `viewBox`, `width`/`height` stand in; with neither, it is an error.
fn view_box(root: &Element, icon: &str) -> Result<(f64, f64, f64, f64)> {
    if let Some(text) = root.attr("viewBox") {
        let values = path::parse_points(text).map_err(|_| {
            Error::new(format!(
                "cannot read viewBox=\"{text}\" in {icon}.svg as four numbers"
            ))
        })?;
        let (Some(origin), Some(size)) = (values.first(), values.get(1)) else {
            return Err(Error::new(format!(
                "viewBox=\"{text}\" in {icon}.svg is missing values"
            )));
        };
        if size.0 <= 0.0 || size.1 <= 0.0 {
            return Err(Error::new(format!(
                "the size of viewBox=\"{text}\" in {icon}.svg is 0 or below"
            )));
        }
        return Ok((origin.0, origin.1, size.0, size.1));
    }
    let width = number(root, "width", 0.0, icon)?;
    let height = number(root, "height", 0.0, icon)?;
    if width > 0.0 && height > 0.0 {
        return Ok((0.0, 0.0, width, height));
    }
    Err(Error::new(format!(
        "{icon}.svg has neither a viewBox nor width/height. Export it with viewBox=\"0 0 24 24\""
    )))
}

/// Where the coordinate system is not 24×24, it is scaled uniformly and centred in the 24-grid.
fn fit_to_grid(segs: &mut [Seg], min_x: f64, min_y: f64, width: f64, height: f64) {
    let identity = min_x.abs() < f64::EPSILON
        && min_y.abs() < f64::EPSILON
        && (width - GRID).abs() < f64::EPSILON
        && (height - GRID).abs() < f64::EPSILON;
    if identity {
        return;
    }
    let scale = GRID / width.max(height);
    let offset_x = (GRID - width * scale) / 2.0 - min_x * scale;
    let offset_y = (GRID - height * scale) / 2.0 - min_y * scale;
    apply(segs, scale, offset_x, offset_y);
}

fn apply(segs: &mut [Seg], scale: f64, offset_x: f64, offset_y: f64) {
    let map_x = |value: f32| util::to_f32(f64::from(value) * scale + offset_x);
    let map_y = |value: f32| util::to_f32(f64::from(value) * scale + offset_y);
    for seg in &mut *segs {
        *seg = match *seg {
            Seg::M(x, y) => Seg::M(map_x(x), map_y(y)),
            Seg::L(x, y) => Seg::L(map_x(x), map_y(y)),
            Seg::Q(cx, cy, x, y) => Seg::Q(map_x(cx), map_y(cy), map_x(x), map_y(y)),
            Seg::C(ax, ay, bx, by, x, y) => Seg::C(
                map_x(ax),
                map_y(ay),
                map_x(bx),
                map_y(by),
                map_x(x),
                map_y(y),
            ),
            Seg::Z => Seg::Z,
        };
    }
}

/// One opening tag and its ancestors.
#[derive(Debug, Clone)]
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    /// The indices of the ancestor elements, in order from the outside.
    parents: Vec<usize>,
}

impl Element {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// Walk the document's elements in order, recording **the nesting** along with them.
/// Comments and declarations are skipped.
fn scan(source: &str) -> Vec<Element> {
    let chars: Vec<char> = source.chars().collect();
    let mut cursor = Cursor { chars, pos: 0 };
    let mut out: Vec<Element> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    while cursor.seek('<') {
        cursor.pos += 1;
        match cursor.peek() {
            Some('!') => {
                if cursor.matches("!--") {
                    cursor.skip_past("-->");
                } else {
                    cursor.skip_past(">");
                }
            }
            Some('?') => cursor.skip_past("?>"),
            Some('/') => {
                cursor.pos += 1;
                let name = cursor.name();
                cursor.skip_past(">");
                // Close down to the innermost element that matches.
                if let Some(depth) = stack
                    .iter()
                    .rposition(|index| out.get(*index).is_some_and(|el| el.name == name))
                {
                    stack.truncate(depth);
                }
            }
            Some(_) => {
                let name = cursor.name();
                let (attrs, self_closing) = cursor.attributes();
                if name.is_empty() {
                    continue;
                }
                let index = out.len();
                out.push(Element {
                    name,
                    attrs,
                    parents: stack.clone(),
                });
                if !self_closing {
                    stack.push(index);
                }
            }
            None => break,
        }
    }
    out
}

struct Cursor {
    chars: Vec<char>,
    pos: usize,
}

impl Cursor {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn seek(&mut self, target: char) -> bool {
        while let Some(current) = self.peek() {
            if current == target {
                return true;
            }
            self.pos += 1;
        }
        false
    }

    fn matches(&self, needle: &str) -> bool {
        needle
            .chars()
            .enumerate()
            .all(|(offset, expected)| self.chars.get(self.pos + offset) == Some(&expected))
    }

    fn skip_past(&mut self, needle: &str) {
        while self.pos < self.chars.len() {
            if self.matches(needle) {
                self.pos += needle.chars().count();
                return;
            }
            self.pos += 1;
        }
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn name(&mut self) -> String {
        let mut out = String::new();
        while let Some(current) = self.peek() {
            if current.is_alphanumeric() || matches!(current, '-' | '_' | ':' | '.') {
                out.push(current);
                self.pos += 1;
            } else {
                break;
            }
        }
        out
    }

    /// The attribute list and "whether it is a self-closing tag".
    fn attributes(&mut self) -> (Vec<(String, String)>, bool) {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => return (out, true),
                Some('>') => {
                    self.pos += 1;
                    return (out, false);
                }
                Some('/') => {
                    self.skip_past(">");
                    return (out, true);
                }
                Some(_) => {}
            }
            let key = self.name();
            if key.is_empty() {
                self.pos += 1;
                continue;
            }
            self.skip_ws();
            let value = if self.peek() == Some('=') {
                self.pos += 1;
                self.skip_ws();
                self.value()
            } else {
                String::new()
            };
            out.push((key, value));
        }
    }

    fn value(&mut self) -> String {
        let quote = match self.peek() {
            Some(c @ ('"' | '\'')) => {
                self.pos += 1;
                Some(c)
            }
            _ => None,
        };
        let mut out = String::new();
        while let Some(current) = self.peek() {
            let done = match quote {
                Some(mark) => current == mark,
                None => current.is_whitespace() || current == '>' || current == '/',
            };
            if done {
                if quote.is_some() {
                    self.pos += 1;
                }
                break;
            }
            out.push(current);
            self.pos += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_icon, scan};
    use crate::path::Seg;
    use crate::util::Result;

    const OUTLINE: &str = r#"<?xml version="1.0"?>
<!-- the lucide family -->
<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"
     fill="none" stroke="currentColor" stroke-width="2">
  <path d="M5 12h14"/>
  <path d = 'M12 5v14' />
</svg>
"#;

    const FILLED: &str = r#"<svg viewBox="0 0 24 24">
  <circle cx="12" cy="12" r="6"/>
  <rect x="2" y="2" width="8" height="4" rx="2"/>
  <line x1="0" y1="0" x2="4" y2="4"/>
  <polyline points="1,1 2,2 3,1"/>
  <polygon points="10 10 14 10 12 14"/>
</svg>"#;

    #[test]
    fn reads_attributes_with_either_quote_style() {
        let elements = scan(OUTLINE);
        let names: Vec<&str> = elements.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["svg", "path", "path"]);
        assert_eq!(elements.get(2).and_then(|e| e.attr("d")), Some("M12 5v14"));
    }

    #[test]
    fn outline_icons_are_not_filled() -> Result<()> {
        let icon = parse_icon("plus", OUTLINE)?;
        assert!(!icon.fill);
        assert_eq!(icon.name, "plus");
        assert_eq!(
            icon.segs,
            vec![
                Seg::M(5.0, 12.0),
                Seg::L(19.0, 12.0),
                Seg::M(12.0, 5.0),
                Seg::L(12.0, 19.0),
            ]
        );
        Ok(())
    }

    #[test]
    fn shapes_become_segments() -> Result<()> {
        let icon = parse_icon("mix", FILLED)?;
        assert!(
            icon.fill,
            "with no fill attribute it is filled, per the SVG default"
        );
        // circle: M + C×4 + Z, rect(rx): M + (L,C)×4 + Z, line: M+L,
        // polyline: M+L×2, polygon: M+L×2+Z
        assert_eq!(icon.segs.len(), 6 + 10 + 2 + 3 + 4);
        assert_eq!(icon.segs.first(), Some(&Seg::M(18.0, 12.0)));
        assert_eq!(icon.segs.last(), Some(&Seg::Z));
        Ok(())
    }

    #[test]
    fn rect_without_radius_is_four_lines() -> Result<()> {
        let icon = parse_icon(
            "box",
            r#"<svg viewBox="0 0 24 24"><rect x="2" y="4" width="20" height="16"/></svg>"#,
        )?;
        assert_eq!(
            icon.segs,
            vec![
                Seg::M(2.0, 4.0),
                Seg::L(22.0, 4.0),
                Seg::L(22.0, 20.0),
                Seg::L(2.0, 20.0),
                Seg::Z,
            ]
        );
        Ok(())
    }

    #[test]
    fn style_fill_none_wins() -> Result<()> {
        let icon = parse_icon(
            "ring",
            r#"<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" style="fill: none; stroke: red"/></svg>"#,
        )?;
        assert!(!icon.fill);
        Ok(())
    }

    #[test]
    fn other_view_boxes_are_scaled_into_the_grid() -> Result<()> {
        let icon = parse_icon(
            "wide",
            r#"<svg viewBox="0 0 48 48"><path d="M0 0L48 48"/></svg>"#,
        )?;
        assert_eq!(icon.segs, vec![Seg::M(0.0, 0.0), Seg::L(24.0, 24.0)]);
        Ok(())
    }

    #[test]
    fn empty_documents_are_an_error() {
        assert!(parse_icon("nothing", "<svg viewBox=\"0 0 24 24\"></svg>").is_err());
    }

    #[test]
    fn shapes_inside_defs_clip_paths_and_masks_are_skipped() -> Result<()> {
        for container in ["defs", "clipPath", "mask", "symbol"] {
            let source = format!(
                "<svg viewBox=\"0 0 24 24\"><{container}><rect width=\"24\" height=\"24\"/></{container}><path d=\"M5 12h14\" fill=\"none\"/></svg>"
            );
            let icon = parse_icon("clip", &source)?;
            assert_eq!(
                icon.segs,
                vec![Seg::M(5.0, 12.0), Seg::L(19.0, 12.0)],
                "a shape inside <{container}> leaked out"
            );
            assert!(
                !icon.fill,
                "the rectangle inside <{container}> changed the fill decision"
            );
        }
        Ok(())
    }

    #[test]
    fn a_group_fill_is_inherited() -> Result<()> {
        let icon = parse_icon(
            "outline",
            r#"<svg viewBox="0 0 24 24"><g fill="none" stroke="currentColor"><path d="M5 12h14"/></g></svg>"#,
        )?;
        assert!(!icon.fill);
        // Where a child settles it again, the child wins.
        let icon = parse_icon(
            "mixed",
            r#"<svg viewBox="0 0 24 24"><g fill="none"><path d="M5 12h14" fill="currentColor"/></g></svg>"#,
        )?;
        assert!(icon.fill);
        Ok(())
    }

    #[test]
    fn transparent_and_zero_opacity_are_not_filled() -> Result<()> {
        for attrs in [
            r#"fill="NONE""#,
            r#"fill="transparent""#,
            r#"fill-opacity="0""#,
            r#"style="fill-opacity: 0""#,
        ] {
            let source = format!(
                "<svg viewBox=\"0 0 24 24\"><circle cx=\"12\" cy=\"12\" r=\"6\" {attrs}/></svg>"
            );
            assert!(!parse_icon("ring", &source)?.fill, "{attrs}");
        }
        Ok(())
    }

    #[test]
    fn ellipses_become_four_cubics() -> Result<()> {
        let icon = parse_icon(
            "oval",
            r#"<svg viewBox="0 0 24 24"><ellipse cx="12" cy="12" rx="5" ry="3"/></svg>"#,
        )?;
        assert_eq!(icon.segs.len(), 6);
        assert_eq!(icon.segs.first(), Some(&Seg::M(17.0, 12.0)));
        Ok(())
    }

    #[test]
    fn unsupported_elements_and_transforms_are_errors() {
        for source in [
            r##"<svg viewBox="0 0 24 24"><use href="#p"/><path d="M5 12h14"/></svg>"##,
            r#"<svg viewBox="0 0 24 24"><text x="1" y="2">a</text><path d="M5 12h14"/></svg>"#,
            r#"<svg viewBox="0 0 24 24"><path d="M5 12h14" transform="rotate(90 12 12)"/></svg>"#,
            r#"<svg viewBox="0 0 24 24"><g transform="translate(1 1)"><path d="M5 12h14"/></g></svg>"#,
        ] {
            assert!(parse_icon("bad", source).is_err(), "{source}");
        }
    }

    #[test]
    fn width_and_height_stand_in_for_a_missing_view_box() -> Result<()> {
        let icon = parse_icon(
            "wide",
            r#"<svg width="48" height="48"><path d="M0 0L48 48"/></svg>"#,
        )?;
        assert_eq!(icon.segs, vec![Seg::M(0.0, 0.0), Seg::L(24.0, 24.0)]);
        assert!(parse_icon("nogrid", r#"<svg><path d="M0 0L1 1"/></svg>"#).is_err());
        Ok(())
    }

    #[test]
    fn arc_noise_is_snapped_away() -> Result<()> {
        let icon = parse_icon(
            "arc",
            r#"<svg viewBox="0 0 24 24"><path d="M0 0 a1 1 0 00-5-5" fill="none"/></svg>"#,
        )?;
        // The endpoint has to be (-5, -5) with no noise such as -2.4e-8 left behind.
        let last = icon.segs.last().copied();
        assert!(
            matches!(last, Some(Seg::C(_, _, _, _, x, y)) if (x - -5.0).abs() < 1e-4 && (y - -5.0).abs() < 1e-4),
            "{last:?}"
        );
        for seg in &icon.segs {
            if let Seg::C(a, b, c, d, e, f) = *seg {
                for value in [a, b, c, d, e, f] {
                    assert!(
                        value == 0.0 || value.abs() > 1e-4,
                        "noise was left: {value}"
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn parsing_is_deterministic() -> Result<()> {
        assert_eq!(parse_icon("mix", FILLED)?, parse_icon("mix", FILLED)?);
        Ok(())
    }

    /// Exactly the shape `lucide-static@1.39.0` actually exports (`assets/icons/wifi.svg`).
    /// It has a licence comment, multi-line attributes, `stroke-linecap`, a relative `m`, `h.01` and a
    /// relative arc `a` all at once. All three are inside the supported subset, so it has to pass with no
    /// reinforcement of the parser.
    const LUCIDE_WIFI: &str = r#"<!-- @license lucide-static v1.39.0 - ISC -->
<svg
  class="lucide lucide-wifi"
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M12 20h.01" />
  <path d="M2 8.82a15 15 0 0 1 20 0" />
  <path d="M5 12.859a10 10 0 0 1 14 0" />
  <path d="M8.5 16.429a5 5 0 0 1 7 0" />
</svg>
"#;

    #[test]
    fn a_real_lucide_export_parses() -> Result<()> {
        let icon = parse_icon("wifi", LUCIDE_WIFI)?;
        assert!(!icon.fill, "it has to be a stroked icon");
        // A one-point subpath (`h.01`) plus 3 arcs.
        assert_eq!(icon.segs.first(), Some(&Seg::M(12.0, 20.0)));
        assert_eq!(icon.segs.get(1), Some(&Seg::L(12.01, 20.0)));
        assert_eq!(
            icon.segs
                .iter()
                .filter(|seg| matches!(seg, Seg::M(..)))
                .count(),
            4
        );
        assert!(icon.segs.iter().any(|seg| matches!(seg, Seg::C(..))));
        // The arcs do not go outside the grid.
        for seg in &icon.segs {
            if let Seg::C(ax, ay, bx, by, x, y) = *seg {
                for value in [ax, ay, bx, by, x, y] {
                    assert!((-1.0..=25.0).contains(&value), "{value}");
                }
            }
        }
        assert_eq!(parse_icon("wifi", LUCIDE_WIFI)?, icon);
        Ok(())
    }

    #[test]
    fn bad_numbers_are_reported() {
        let parsed = parse_icon(
            "bad",
            r#"<svg viewBox="0 0 24 24"><circle cx="a" cy="1" r="2"/></svg>"#,
        );
        assert!(parsed.is_err());
    }
}
