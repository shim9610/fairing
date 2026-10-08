//! The string table: every word the crate draws goes through [`Strings`], which
//! looks it up in the active locale's table.
//!
//! **The English text is the key** ([`LabelKey`]). A key with no entry shows as written, so an
//! untranslated string reads as English rather than as nothing, and a screen of yours that never
//! heard of translation reads as it always did. English needs no table; Korean is built in. Any
//! other language — or a change to a built-in entry — comes in as [`Translations`] through
//! [`ShellBuilder::translations`](crate::ShellBuilder::translations).
//!
//! The locale starts as `[shell] locale` and follows the `ui.locale` setting while the shell runs:
//! `settings.locale` sets it, and the next frame is in the new language.
//!
//! Inside the crate a built-in string is marked where it is written — `tr!(strings, "Brightness")`
//! looks it up there and then, `tr_key!("Wi-Fi")` is a key looked up where it is drawn. The tests
//! read those marks, and the constants in the `labels` modules, out of the source and check that
//! the Korean table has every one of them and nothing besides.

use std::collections::HashMap;

mod ko;

/// A label key: the English text, and what is shown where the active locale's
/// table has no entry for it.
pub type LabelKey = String;

/// A built-in string, looked up in the active table: `tr!(strings, "Brightness")`.
#[cfg_attr(
    not(feature = "settings"),
    expect(
        unused_macros,
        reason = "the settings screens are what looks a literal up on the spot"
    )
)]
macro_rules! tr {
    ($strings:expr, $key:literal) => {
        $strings.get($key)
    };
}
#[cfg_attr(
    not(feature = "settings"),
    expect(
        unused_imports,
        reason = "the settings screens are what looks a literal up on the spot"
    )
)]
pub(crate) use tr;

/// A built-in string's key, looked up where it is drawn: `tr_key!("Wi-Fi")`. It is the literal
/// itself — the mark is what the tests read to check the Korean table has every key.
macro_rules! tr_key {
    ($key:literal) => {
        $key
    };
}
pub(crate) use tr_key;

/// The keys of the words the widgets draw. A widget has no string table, so its words come from
/// whoever places it — on a screen, through `cx.strings`; the built-in tables have these too:
///
/// ```no_run
/// use fairing::i18n::labels;
/// use fairing::widgets::{Dropdown, Opener};
///
/// fn body(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>, picked: &mut usize) {
///     let no_match = cx.strings.get(labels::NO_MATCH);
///     Dropdown::new("pump", &["Inlet", "Outlet"], picked)
///         .opener(Opener::Search)
///         .no_match(no_match)
///         .show(ui, &mut cx.widgets());
/// }
/// ```
pub mod labels {
    /// A [`Dropdown`](crate::widgets::Dropdown) search with nothing found.
    pub const NO_MATCH: &str = "No match";
}

/// One language's table: the English text → the text in this language.
///
/// For your own screens, look a string up with `cx.strings.get("Pump pressure")` and give its
/// translation here; for a built-in one, an entry here wins over the built-in table's.
///
/// ```
/// use fairing::i18n::Translations;
///
/// let german = Translations::new("de", "Deutsch")
///     .entry("Settings", "Einstellungen")
///     .entry("Brightness", "Helligkeit");
/// assert_eq!(german.get("Settings"), Some("Einstellungen"));
/// ```
#[derive(Debug, Clone, Default)]
pub struct Translations {
    locale: String,
    name: String,
    entries: HashMap<String, String>,
}

impl Translations {
    /// A table for `locale` (`"ko"`, `"de"`), which the language list shows as `name` — in its
    /// own language, as language lists do ("한국어", "Deutsch").
    #[must_use]
    pub fn new(locale: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            locale: locale.into(),
            name: name.into(),
            entries: HashMap::new(),
        }
    }

    /// One entry: what `key`, the English text, reads as in this language. A key given twice
    /// keeps the last.
    #[must_use]
    pub fn entry(mut self, key: impl Into<String>, text: impl Into<String>) -> Self {
        self.entries.insert(key.into(), text.into());
        self
    }

    /// Many entries at once.
    #[must_use]
    pub fn entries<K: Into<String>, V: Into<String>>(
        mut self,
        entries: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        self.entries
            .extend(entries.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// The locale tag.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// What the language list shows.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The entry for `key`, where there is one.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// How many entries it has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it has none — English, whose keys are its text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The tables there are, and the one in use.
#[derive(Debug, Clone)]
pub struct Strings {
    /// The active table's tag, or the locale asked for where there is no table for it.
    locale: String,
    tables: Vec<Translations>,
    active: Option<usize>,
}

impl Default for Strings {
    fn default() -> Self {
        Self::new("en")
    }
}

impl Strings {
    /// The built-in tables — English, which needs none, and Korean — set to `locale` (`"en"`,
    /// `"ko"`; `"ko-KR"` finds `"ko"`). A locale with no table reads as English.
    #[must_use]
    pub fn new(locale: impl Into<String>) -> Self {
        let mut strings = Self {
            locale: String::new(),
            tables: vec![
                Translations::new("en", "English"),
                Translations::new("ko", "한국어").entries(ko::ENTRIES.iter().copied()),
            ],
            active: None,
        };
        strings.select(&locale.into());
        strings
    }

    /// Add a table, or extend one there is: entries for a locale that has a table join it and win
    /// over its own, and `name` replaces the name it had.
    #[must_use]
    pub fn with(mut self, translations: Translations) -> Self {
        self.add(translations);
        self
    }

    /// [`Strings::with`] in place.
    pub fn add(&mut self, translations: Translations) {
        match self
            .tables
            .iter_mut()
            .find(|t| t.locale == translations.locale)
        {
            Some(table) => {
                table.name = translations.name;
                table.entries.extend(translations.entries);
            }
            None => self.tables.push(translations),
        }
        // The locale asked for may be the one that just got a table.
        let locale = self.locale.clone();
        self.select(&locale);
    }

    /// The active locale — the table's tag, or the locale asked for where there is no table.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Switch to `locale`. `false`, and nothing changes, where there is no table for it.
    pub fn set_locale(&mut self, locale: &str) -> bool {
        if self.find(locale).is_none() {
            return false;
        }
        self.select(locale);
        true
    }

    /// Whether there is a table for `locale`.
    #[must_use]
    pub fn has_locale(&self, locale: &str) -> bool {
        self.find(locale).is_some()
    }

    /// The text for `key` in the active locale — the key itself where the table has no entry.
    #[must_use]
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.active
            .and_then(|i| self.tables.get(i))
            .and_then(|table| table.get(key))
            .unwrap_or(key)
    }

    /// The languages there are, as `(locale, name)`: English and Korean, then yours in the order
    /// they came.
    pub fn locales(&self) -> impl Iterator<Item = (&str, &str)> {
        self.tables
            .iter()
            .map(|t| (t.locale.as_str(), t.name.as_str()))
    }

    /// The letters the active table writes beyond ASCII, each once and in order — what a font
    /// has to have for this language. Empty for English.
    pub(crate) fn letters(&self) -> Vec<char> {
        let Some(table) = self.active.and_then(|i| self.tables.get(i)) else {
            return Vec::new();
        };
        let letters: std::collections::BTreeSet<char> = table
            .entries
            .values()
            .flat_map(|text| text.chars())
            .filter(|c| !c.is_ascii() && c.is_alphabetic())
            .collect();
        letters.into_iter().collect()
    }

    /// The table for `locale`: the exact tag, or its language alone (`"ko-KR"` → `"ko"`).
    fn find(&self, locale: &str) -> Option<usize> {
        self.tables
            .iter()
            .position(|t| t.locale == locale)
            .or_else(|| {
                let language = locale.split(['-', '_']).next().unwrap_or(locale);
                self.tables.iter().position(|t| t.locale == language)
            })
    }

    fn select(&mut self, locale: &str) {
        self.active = self.find(locale);
        self.locale = self
            .active
            .and_then(|i| self.tables.get(i))
            .map_or_else(|| locale.to_owned(), |t| t.locale.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::{ko, Strings, Translations};
    use std::collections::BTreeSet;
    use std::path::Path;

    /// The string literal starting at `text[at]` (a `"`), unescaped, and where it ends.
    fn literal(text: &str, at: usize) -> Option<(String, usize)> {
        let mut out = String::new();
        let mut chars = text.get(at + 1..)?.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return Some((out, at + 1 + i + 1)),
                '\\' => out.push(chars.next()?.1),
                c => out.push(c),
            }
        }
        None
    }

    /// The keys one file marks: `tr!(…, "key")`, `tr_key!("key")`, and the `&str` constants of
    /// its `labels` module. Comments are skipped.
    fn keys_in(text: &str, keys: &mut BTreeSet<String>) {
        let code: String = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for mark in ["tr_key!(", "tr!("] {
            let mut from = 0;
            while let Some(found) = code.get(from..).and_then(|rest| rest.find(mark)) {
                let at = from + found;
                let start = at + mark.len();
                // A whole word only: `include_str!(` ends in `tr!(` too.
                let joined = code[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                if joined {
                    from = start;
                    continue;
                }
                // `tr!(strings, "…")` — the literal after the comma; `tr_key!("…")` — right there.
                let quote = code.get(start..).and_then(|rest| rest.find('"'));
                let close = code.get(start..).and_then(|rest| rest.find(')'));
                match (quote, close) {
                    (Some(q), Some(c)) if q < c => {
                        if let Some((key, end)) = literal(&code, start + q) {
                            keys.insert(key);
                            from = end;
                            continue;
                        }
                    }
                    _ => {}
                }
                from = start;
            }
        }
        let mut depth = 0usize;
        let mut in_labels = false;
        for line in code.lines() {
            if line.contains("mod labels {") {
                in_labels = true;
                depth = 0;
            }
            if in_labels {
                if line.contains("const ") && line.contains(": &str = \"") {
                    if let Some(q) = line.find('"') {
                        if let Some((key, _)) = literal(line, q) {
                            keys.insert(key);
                        }
                    }
                }
                depth += line.matches('{').count();
                depth = depth.saturating_sub(line.matches('}').count());
                if depth == 0 {
                    in_labels = false;
                }
            }
        }
    }

    /// Every key the crate's source marks.
    fn marked_keys() -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        let mut dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    if let Ok(mut text) = std::fs::read_to_string(&path) {
                        // This module's tests name the marks without being any.
                        if path.ends_with("i18n/mod.rs") {
                            let tests = text.find("#[cfg(test)]").unwrap_or(text.len());
                            text.truncate(tests);
                        }
                        keys_in(&text, &mut keys);
                    }
                }
            }
        }
        keys
    }

    /// A string added, changed or removed in the code has to be in the Korean table — and a
    /// table entry nothing draws any more goes too.
    #[test]
    fn the_korean_table_has_every_key_and_nothing_else() {
        let marked = marked_keys();
        assert!(marked.len() > 20, "the scan found the marks: {marked:?}");
        let table: BTreeSet<&str> = ko::ENTRIES.iter().map(|(key, _)| *key).collect();
        let missing: Vec<&String> = marked
            .iter()
            .filter(|k| !table.contains(k.as_str()))
            .collect();
        let unused: Vec<&&str> = table.iter().filter(|k| !marked.contains(**k)).collect();
        assert!(missing.is_empty(), "no Korean for: {missing:#?}");
        assert!(
            unused.is_empty(),
            "in the Korean table, drawn nowhere: {unused:#?}"
        );
    }

    /// One entry a key, in the keys' order — so a merge never hides a second entry.
    #[test]
    fn the_korean_table_is_sorted_with_each_key_once() {
        for pair in ko::ENTRIES.windows(2) {
            if let [(a, _), (b, _)] = pair {
                assert!(a < b, "{a:?} has to come before {b:?}");
            }
        }
    }

    /// The placeholders of a key are the Korean's too: a `{n}` lost in translation would show
    /// as nothing, one added would show as itself. A date may drop the padding (`{mm}` → `{m}`).
    #[test]
    fn a_translation_keeps_its_keys_placeholders() {
        fn holes(text: &str) -> BTreeSet<String> {
            text.split('{')
                .skip(1)
                .filter_map(|part| part.split_once('}').map(|(name, _)| name))
                .map(|name| match name {
                    "mm" => "m".to_owned(),
                    "dd" => "d".to_owned(),
                    other => other.to_owned(),
                })
                .collect()
        }
        for (key, korean) in ko::ENTRIES {
            assert_eq!(holes(key), holes(korean), "{key:?} → {korean:?}");
        }
    }

    #[test]
    fn a_key_with_no_entry_reads_as_written() {
        let strings = Strings::new("ko");
        assert_eq!(strings.get("Pump pressure"), "Pump pressure");
        assert_eq!(Strings::new("en").get("Settings"), "Settings");
        let unknown = Strings::new("xx");
        assert_eq!(unknown.locale(), "xx");
        assert_eq!(unknown.get("Settings"), "Settings", "no table, so English");
    }

    #[test]
    fn korean_is_built_in_and_a_region_finds_its_language() {
        let strings = Strings::new("ko-KR");
        assert_eq!(strings.locale(), "ko");
        assert_ne!(strings.get("Settings"), "Settings");
        assert!(strings
            .locales()
            .any(|(tag, name)| tag == "ko" && name == "한국어"));
    }

    #[test]
    fn a_table_of_yours_adds_a_language_or_wins_over_a_built_in_entry() {
        let mut strings = Strings::new("de")
            .with(Translations::new("de", "Deutsch").entry("Settings", "Einstellungen"))
            .with(Translations::new("ko", "한국어").entry("Settings", "환경설정"));
        assert_eq!(
            strings.get("Settings"),
            "Einstellungen",
            "the locale asked for"
        );
        assert!(strings.set_locale("ko"));
        assert_eq!(strings.get("Settings"), "환경설정", "yours wins");
        assert!(!strings.set_locale("fr"));
        assert_eq!(strings.locale(), "ko", "nothing changed");
    }

    #[test]
    fn the_letters_are_the_language_s_own() {
        assert_eq!(Strings::new("en").letters(), Vec::<char>::new());
        let korean = Strings::new("ko").letters();
        assert!(korean.contains(&'가') && korean.contains(&'설'));
        assert!(
            korean.iter().all(|c| ('\u{ac00}'..='\u{d7a3}').contains(c)),
            "Hangul syllables only: {korean:?}"
        );
        assert!(
            korean.windows(2).all(|w| matches!(w, [a, b] if a < b)),
            "each once, in order"
        );
    }
}
