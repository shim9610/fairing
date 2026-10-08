//! A minimal, dependency-free TOML scanner (shared by 's xtask stages).
//!
//! Not a complete parser. It walks only the subset the audit gates actually read — table headers,
//! `key = value`, arrays and inline tables — **respecting string boundaries**.
//! Every check that reads `deny.toml`, `Cargo.toml`, `Cargo.lock` or `.cargo/config.toml` uses this
//! one module. That is so a line-based heuristic cannot be fooled by a `#` or a `]` inside a string
//! and quietly lose an entry.
//!
//! Syntax it does not support (multi-line strings) is an error rather than something ignored (fail-closed).

use crate::util::{Error, Result};

/// One scanned `key = value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Item {
    /// The name of the table it belongs to. The top level is an empty string, and `[a.b]` is `"a.b"`.
    pub(crate) section: String,
    /// The 0-based index of a `[[a.b]]` repeated table. 0 for an ordinary table.
    pub(crate) table_index: usize,
    /// The key (with the quotes stripped).
    pub(crate) key: String,
    /// The value's text (with comments removed). A multi-line array keeps its newlines.
    pub(crate) value: String,
    /// The line number the value started on (1-based).
    pub(crate) line: usize,
}

impl Item {
    /// Where the value is a basic string, hand it back with the quotes stripped.
    pub(crate) fn string(&self) -> Option<String> {
        unquote(&self.value)
    }
}

/// The minimal state that follows being inside or outside a string.
#[derive(Default)]
struct Quotes {
    mark: Option<char>,
    escaped: bool,
}

impl Quotes {
    /// Eat one character and say whether that character is **a syntax character outside a string**.
    fn outside(&mut self, ch: char) -> bool {
        if let Some(mark) = self.mark {
            if self.escaped {
                self.escaped = false;
            } else if mark == '"' && ch == '\\' {
                self.escaped = true;
            } else if ch == mark {
                self.mark = None;
            }
            return false;
        }
        if ch == '"' || ch == '\'' {
            self.mark = Some(ch);
            return false;
        }
        true
    }
}

/// Cut a `#` comment outside a string off a line.
pub(crate) fn strip_comment(line: &str) -> &str {
    let mut quotes = Quotes::default();
    for (index, ch) in line.char_indices() {
        if quotes.outside(ch) && ch == '#' {
            return line.get(..index).unwrap_or("");
        }
    }
    line
}

/// The difference between the `[` / `{` and `]` / `}` counts outside strings. At 0 the value ends there.
fn depth_of(text: &str) -> i32 {
    let mut quotes = Quotes::default();
    let mut depth = 0_i32;
    for ch in text.chars() {
        if !quotes.outside(ch) {
            continue;
        }
        match ch {
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// Split only on a `delim` outside strings and brackets. Empty pieces are dropped.
pub(crate) fn split_top(text: &str, delim: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quotes = Quotes::default();
    let mut depth = 0_i32;
    let mut start = 0_usize;
    for (index, ch) in text.char_indices() {
        if !quotes.outside(ch) {
            continue;
        }
        match ch {
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            _ if ch == delim && depth == 0 => {
                if let Some(piece) = text.get(start..index) {
                    let piece = piece.trim();
                    if !piece.is_empty() {
                        out.push(piece);
                    }
                }
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    if let Some(piece) = text.get(start..) {
        let piece = piece.trim();
        if !piece.is_empty() {
            out.push(piece);
        }
    }
    out
}

/// Split into `key = value` at the first `=` outside a string.
fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let mut quotes = Quotes::default();
    let mut depth = 0_i32;
    for (index, ch) in line.char_indices() {
        if !quotes.outside(ch) {
            continue;
        }
        match ch {
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            '=' if depth == 0 => {
                let key = line.get(..index)?.trim();
                let value = line.get(index + 1..)?.trim();
                return Some((key, value));
            }
            _ => {}
        }
    }
    None
}

/// Strip the quotes off a basic or literal string. `None` where it is not a string.
pub(crate) fn unquote(text: &str) -> Option<String> {
    let trimmed = text.trim();
    for mark in ['"', '\''] {
        if let Some(inner) = trimmed
            .strip_prefix(mark)
            .and_then(|r| r.strip_suffix(mark))
        {
            if mark == '\'' {
                return Some(inner.to_owned());
            }
            return Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"));
        }
    }
    None
}

/// The list of a `[ ... ]` array's top-level elements, as text.
pub(crate) fn array_elements(value: &str) -> Result<Vec<String>> {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| Error::new(format!("not an array: `{trimmed}`")))?;
    Ok(split_top(inner, ',')
        .into_iter()
        .map(|piece| piece.trim().trim_end_matches(',').trim().to_owned())
        .filter(|piece| !piece.is_empty())
        .collect())
}

/// A string array (`["a", "b"]`).
pub(crate) fn array_strings(value: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for element in array_elements(value)? {
        let text = unquote(&element).ok_or_else(|| {
            Error::new(format!(
                "an array element that is not a string: `{element}`"
            ))
        })?;
        out.push(text);
    }
    Ok(out)
}

/// The `(key, value text)` list of an inline table `{ k = v, ... }`.
pub(crate) fn inline_table(value: &str) -> Result<Vec<(String, String)>> {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| Error::new(format!("not an inline table: `{trimmed}`")))?;
    let mut out = Vec::new();
    for pair in split_top(inner, ',') {
        let Some((key, item)) = split_key_value(pair) else {
            return Err(Error::new(format!("`{pair}` has no `=`")));
        };
        let key = unquote(key).unwrap_or_else(|| key.to_owned());
        out.push((key, item.to_owned()));
    }
    Ok(out)
}

fn bump(counters: &mut Vec<(String, usize)>, name: &str) -> usize {
    if let Some(slot) = counters.iter_mut().find(|(key, _)| key == name) {
        slot.1 += 1;
        return slot.1;
    }
    counters.push((name.to_owned(), 0));
    0
}

fn header(trimmed: &str, line: usize) -> Result<Option<(String, bool)>> {
    if let Some(rest) = trimmed.strip_prefix("[[") {
        let name = rest.strip_suffix("]]").ok_or_else(|| {
            Error::new(format!("the `[[...]]` header on line {line} is not closed"))
        })?;
        return Ok(Some((name.trim().to_owned(), true)));
    }
    if let Some(rest) = trimmed.strip_prefix('[') {
        // Only where it is a header rather than a value (= the line ends with `]`).
        if let Some(name) = rest.strip_suffix(']') {
            return Ok(Some((name.trim().to_owned(), false)));
        }
        return Err(Error::new(format!(
            "the `[...]` header on line {line} is not closed"
        )));
    }
    Ok(None)
}

/// Walk the whole document as a list of `Item`s.
pub(crate) fn items(text: &str) -> Result<Vec<Item>> {
    if text.contains("\"\"\"") || text.contains("'''") {
        return Err(Error::new(
            "a multi-line string (`\"\"\"` / `'''`) is not handled by this scanner",
        ));
    }
    let mut out: Vec<Item> = Vec::new();
    let mut section = String::new();
    let mut counters: Vec<(String, usize)> = Vec::new();
    let mut index = 0_usize;
    let mut pending: Option<(String, String, usize)> = None;

    for (offset, raw) in text.lines().enumerate() {
        let number = offset + 1;
        let line = strip_comment(raw);
        let trimmed = line.trim();
        if let Some((key, mut buffer, start)) = pending.take() {
            buffer.push('\n');
            buffer.push_str(trimmed);
            if depth_of(&buffer) == 0 {
                out.push(Item {
                    section: section.clone(),
                    table_index: index,
                    key,
                    value: buffer.trim().to_owned(),
                    line: start,
                });
            } else {
                pending = Some((key, buffer, start));
            }
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if let Some((name, repeated)) = header(trimmed, number)? {
            index = if repeated {
                bump(&mut counters, &name)
            } else {
                0
            };
            section = name;
            continue;
        }
        let Some((key, value)) = split_key_value(trimmed) else {
            continue;
        };
        let key = unquote(key).unwrap_or_else(|| key.to_owned());
        if depth_of(value) == 0 {
            out.push(Item {
                section: section.clone(),
                table_index: index,
                key,
                value: value.to_owned(),
                line: number,
            });
        } else {
            pending = Some((key, value.to_owned(), number));
        }
    }
    if let Some((key, _, start)) = pending {
        return Err(Error::new(format!(
            "the `{key}` value on line {start} is not closed"
        )));
    }
    Ok(out)
}

/// Every table header name (with no de-duplication, in the order they appear).
pub(crate) fn sections(text: &str) -> Result<Vec<String>> {
    if text.contains("\"\"\"") || text.contains("'''") {
        return Err(Error::new(
            "a multi-line string (`\"\"\"` / `'''`) is not handled by this scanner",
        ));
    }
    let mut out = Vec::new();
    for (offset, raw) in text.lines().enumerate() {
        let trimmed = strip_comment(raw).trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((name, _)) = header(trimmed, offset + 1)? {
            out.push(name);
        }
    }
    Ok(out)
}

/// Find `section`'s `key` value (the first one).
pub(crate) fn find<'a>(items: &'a [Item], section: &str, key: &str) -> Option<&'a Item> {
    items
        .iter()
        .find(|item| item.section == section && item.key == key)
}

#[cfg(test)]
mod tests {
    use super::{
        array_strings, find, inline_table, items, sections, split_top, strip_comment, unquote,
    };
    use crate::util::{Error, Result};

    #[test]
    fn comments_inside_strings_survive() {
        assert_eq!(
            strip_comment(r#"a = "see #42"  # a real comment"#).trim(),
            r#"a = "see #42""#
        );
        assert_eq!(strip_comment("# all comment"), "");
        assert_eq!(strip_comment("plain = 1"), "plain = 1");
    }

    #[test]
    fn brackets_inside_strings_do_not_end_arrays() -> Result<()> {
        let text = "[bans]\nskip = [\n  { crate = \"calloop@0.13\", reason = \"tracked in [bans] note\" },\n  { crate = \"rustix@0.38\" },\n]\n";
        let parsed = items(text)?;
        let skip = find(&parsed, "bans", "skip").ok_or_else(|| Error::new("skip is missing"))?;
        assert!(skip.value.contains("rustix@0.38"));
        Ok(())
    }

    #[test]
    fn splits_only_at_top_level() {
        let pieces = split_top("{ a = 1, b = [2, 3] }, { c = \"x,y\" }", ',');
        assert_eq!(pieces, vec!["{ a = 1, b = [2, 3] }", "{ c = \"x,y\" }"]);
    }

    #[test]
    fn reads_repeated_tables() -> Result<()> {
        let text = "[[bans.skip]]\ncrate = \"a@1\"\n\n[[bans.skip]]\ncrate = \"b@2\"\n";
        let parsed = items(text)?;
        let indexes: Vec<usize> = parsed.iter().map(|item| item.table_index).collect();
        assert_eq!(indexes, vec![0, 1]);
        Ok(())
    }

    #[test]
    fn reads_arrays_and_inline_tables() -> Result<()> {
        let parsed = items("[workspace]\nmembers = [\n  \"crates/fairing\",\n  \"xtask\",\n]\n")?;
        let members = find(&parsed, "workspace", "members").ok_or_else(|| Error::new("missing"))?;
        assert_eq!(
            array_strings(&members.value)?,
            vec!["crates/fairing", "xtask"]
        );
        let table = inline_table("{ crate = \"calloop@0.13\", reason = \"x = y\" }")?;
        assert_eq!(table.first().map(|(k, _)| k.as_str()), Some("crate"));
        assert_eq!(
            table.first().and_then(|(_, v)| unquote(v)),
            Some("calloop@0.13".to_owned())
        );
        Ok(())
    }

    #[test]
    fn lists_section_headers() -> Result<()> {
        assert_eq!(
            sections("[alias]\nxtask = \"run\"\n[source.crates-io]\nreplace-with = \"v\"\n")?,
            vec!["alias", "source.crates-io"]
        );
        Ok(())
    }

    #[test]
    fn multiline_strings_are_rejected() {
        assert!(items("a = \"\"\"x\"\"\"\n").is_err());
    }

    #[test]
    fn unterminated_values_are_rejected() {
        assert!(items("[bans]\nskip = [\n { crate = \"a\" },\n").is_err());
    }
}
