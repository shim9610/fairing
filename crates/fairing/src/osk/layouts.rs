//! The OSK layouts: `NumPad` (0–9, ⌫, ✓, an optional decimal point and minus sign),
//! `Qwerty(en)` (three faces — lowercase, uppercase, symbols — plus space, ⌫, ↵, ⏮/⏭), and
//! `Custom(KeyLayout)`.
//!
//! **A face** is another panel of the same keyboard. `KeyAction::Face(i)` switches to face `i`.
//! The built-in qwerty has three — [`LOWER_FACE`], [`UPPER_FACE`] and [`SYMBOL_FACE`] — and the
//! uppercase face types **one character** and returns to lowercase; pressing ⇧ twice quickly
//! locks it ([`Osk`](super::Osk) decides).
//!
//! **A span** is a multiple of "one default key". Every row in a face divides up
//! [`KeyFace::max_span`] as its reference width, so keys stay the same size even when rows have
//! different key counts. Every row of the built-in qwerty is fitted to 10. A span is held to
//! `0.5..=6` ([`KeyDef::special`]).
// The caps lock (a ⇧ double tap) needs the time, so it is held by [`Osk`](super::Osk) rather than by the table.
// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the rest of clippy's pedantic set stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::borrow::Cow;

/// The built-in qwerty's lowercase face.
pub(super) const LOWER_FACE: usize = 0;
/// The built-in qwerty's uppercase face.
pub(super) const UPPER_FACE: usize = 1;
/// The built-in qwerty's symbol face.
pub(super) const SYMBOL_FACE: usize = 2;

/// What one key does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    /// Type a string (`Event::Text`).
    Text(Cow<'static, str>),
    /// Delete (`Key::Backspace`).
    Backspace,
    /// Confirm or newline (`Key::Enter`).
    Enter,
    /// Space.
    Space,
    /// Next focus (`Key::Tab`).
    NextFocus,
    /// Previous focus (`Shift+Tab`).
    PrevFocus,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Face switch (lowercase / uppercase / symbols): the target face index.
    Face(usize),
    /// Input-language switch (`한/영`). Moves between dubeolsik and the English qwerty.
    Lang,
    /// Hide the keyboard.
    Hide,
}

impl KeyAction {
    /// Whether it types a character (for deciding the uppercase face's one-character rule).
    #[must_use]
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text(_) | Self::Space)
    }
}

/// A key definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDef {
    /// The action.
    pub action: KeyAction,
    /// The label.
    pub label: Cow<'static, str>,
    /// The width multiplier in tenths (10 = one default key).
    pub span_x10: u8,
}

impl KeyDef {
    /// A character key.
    #[must_use]
    pub fn text(s: &'static str) -> Self {
        Self {
            action: KeyAction::Text(Cow::Borrowed(s)),
            label: Cow::Borrowed(s),
            span_x10: 10,
        }
    }

    /// A special key. `span` is its width in default keys, held to `0.5..=6` and rounded to a
    /// tenth: narrower than half a key is not a target a finger can hit, and a single key wider
    /// than six squeezes every other key on its face.
    #[must_use]
    pub fn special(action: KeyAction, label: &'static str, span: f32) -> Self {
        Self {
            action,
            label: Cow::Borrowed(label),
            span_x10: (span * 10.0).round().clamp(5.0, 60.0) as u8,
        }
    }

    /// The width multiplier.
    #[must_use]
    pub fn span(&self) -> f32 {
        f32::from(self.span_x10) / 10.0
    }
}

/// A key row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyRow {
    /// The keys (from the left).
    pub keys: Vec<KeyDef>,
}

/// One face.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyFace {
    /// The rows (from the top).
    pub rows: Vec<KeyRow>,
}

impl KeyFace {
    /// The longest row's total width (in span units).
    #[must_use]
    pub fn max_span(&self) -> f32 {
        self.rows
            .iter()
            .map(|r| r.keys.iter().map(KeyDef::span).sum::<f32>())
            .fold(0.0, f32::max)
    }
}

/// An integrator-defined layout (with any number of faces).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyLayout {
    /// The name (for logs and tests).
    pub name: Cow<'static, str>,
    /// The faces. `KeyAction::Face(i)` points into this.
    pub faces: Vec<KeyFace>,
}

/// The built-in layout selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OskLayout {
    /// A number pad.
    NumPad {
        /// The decimal point key.
        decimal: bool,
        /// A `-` key, which types a minus sign.
        sign: bool,
    },
    /// English qwerty.
    Qwerty,
    /// Dubeolsik Hangul. The `한/영` key switches to and from the English qwerty.
    Hangul,
    /// Integrator-defined.
    Custom(KeyLayout),
}

impl OskLayout {
    /// From a config string.
    #[must_use]
    pub fn parse(name: &str, decimal: bool, sign: bool) -> Option<Self> {
        match name {
            "numpad" => Some(Self::NumPad { decimal, sign }),
            "qwerty" => Some(Self::Qwerty),
            "hangul" | "ko" => Some(Self::Hangul),
            _ => None,
        }
    }

    /// The actual key table (with no `한/영` key).
    #[must_use]
    pub fn build(&self) -> KeyLayout {
        self.build_with(false)
    }

    /// The actual key table. With `lang` true, a `한/영` key is added to the bottom row — true
    /// only on a keyboard that switches between two languages ([`super::Osk::from_config`]
    /// enables it for `layout = "hangul"`).
    #[must_use]
    pub(crate) fn build_with(&self, lang: bool) -> KeyLayout {
        match self {
            Self::NumPad { decimal, sign } => numpad(*decimal, *sign),
            Self::Qwerty => qwerty(lang),
            Self::Hangul => hangul(lang),
            Self::Custom(layout) => layout.clone(),
        }
    }

    /// Whether it is the built-in qwerty.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn is_qwerty(&self) -> bool {
        matches!(self, Self::Qwerty)
    }

    /// Whether the layout has a ⇧ face (which brings the one-character rule and the caps lock).
    /// Qwerty and dubeolsik share the same three-face structure, so they share the rule.
    #[must_use]
    pub(crate) fn has_shift(&self) -> bool {
        matches!(self, Self::Qwerty | Self::Hangul)
    }

    /// The composing input method this layout uses. `None` for a layout that needs no composition.
    #[must_use]
    pub(crate) fn composer(&self) -> Option<Box<dyn super::compose::Composer>> {
        match self {
            Self::Hangul => Some(Box::new(super::hangul::HangulComposer::new())),
            _ => None,
        }
    }

    /// The layout the `한/영` key switches to. `None` for a single-language layout.
    #[must_use]
    pub(crate) fn lang_pair(&self) -> Option<Self> {
        match self {
            Self::Hangul => Some(Self::Qwerty),
            Self::Qwerty => Some(Self::Hangul),
            _ => None,
        }
    }
}

/// The `한/영` key's label. The OSK substitutes the `language` icon when drawing it so it shows
/// on a device with no Hangul font (`osk::special_icon`).
pub(super) const LANG_LABEL: &str = "한/영";

fn row(keys: Vec<KeyDef>) -> KeyRow {
    KeyRow { keys }
}

/// The numpad: 3×4 plus a right column (⌫ / ▾ / ⏭ / ✓).
#[must_use]
pub(super) fn numpad(decimal: bool, sign: bool) -> KeyLayout {
    // The last row widens the 0 by the space left to fill four cells (the same width per row — max_span = 4).
    let extras = usize::from(sign) + usize::from(decimal);
    let mut last = Vec::new();
    if sign {
        last.push(KeyDef::text("-"));
    }
    last.push(match extras {
        0 => KeyDef::special(KeyAction::Text(Cow::Borrowed("0")), "0", 3.0),
        1 => KeyDef::special(KeyAction::Text(Cow::Borrowed("0")), "0", 2.0),
        _ => KeyDef::text("0"),
    });
    if decimal {
        last.push(KeyDef::text("."));
    }
    last.push(KeyDef::special(KeyAction::Enter, "✓", 1.0));
    KeyLayout {
        name: Cow::Borrowed("numpad"),
        faces: vec![KeyFace {
            rows: vec![
                row(vec![
                    KeyDef::text("7"),
                    KeyDef::text("8"),
                    KeyDef::text("9"),
                    KeyDef::special(KeyAction::Backspace, "⌫", 1.0),
                ]),
                row(vec![
                    KeyDef::text("4"),
                    KeyDef::text("5"),
                    KeyDef::text("6"),
                    KeyDef::special(KeyAction::Hide, "▾", 1.0),
                ]),
                row(vec![
                    KeyDef::text("1"),
                    KeyDef::text("2"),
                    KeyDef::text("3"),
                    KeyDef::special(KeyAction::NextFocus, "⏭", 1.0),
                ]),
                row(last),
            ],
        }],
    }
}

/// The three-face English qwerty (lowercase, uppercase, symbols). Every row is 10 spans.
///
/// With `lang` true, a `한/영` key on the bottom row switches to and from dubeolsik.
#[must_use]
pub(super) fn qwerty(lang: bool) -> KeyLayout {
    const LOWER: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
    const UPPER: [&str; 3] = ["QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"];
    const SYM_TOP: [&str; 3] = ["1234567890", "-/:;()$&@\"", ".,?!'#%^*"];
    KeyLayout {
        name: Cow::Borrowed("qwerty"),
        faces: vec![
            letters(&LOWER, UPPER_FACE, lang),
            letters(&UPPER, LOWER_FACE, lang),
            symbols(&SYM_TOP, lang),
        ],
    }
}

/// The three-face dubeolsik Hangul (base, ⇧, symbols). Exactly the standard dubeolsik
/// (KS X 5002) positions.
///
/// Its rows are 10 · 9 · 7, the same as qwerty, so it uses the same grid. ⇧ changes only the
/// tense consonants (`ㅃㅉㄸㄲㅆ`) and `ㅒ` · `ㅖ`; the other two rows are unchanged.
///
/// The jamo typed on this layout are composed into syllables by
/// [`super::hangul::HangulComposer`] — they do not go in as they are.
#[must_use]
pub(super) fn hangul(lang: bool) -> KeyLayout {
    const LOWER: [&str; 3] = [
        "ㅂㅈㄷㄱㅅㅛㅕㅑㅐㅔ",
        "ㅁㄴㅇㄹㅎㅗㅓㅏㅣ",
        "ㅋㅌㅊㅍㅠㅜㅡ",
    ];
    const UPPER: [&str; 3] = [
        "ㅃㅉㄸㄲㅆㅛㅕㅑㅒㅖ",
        "ㅁㄴㅇㄹㅎㅗㅓㅏㅣ",
        "ㅋㅌㅊㅍㅠㅜㅡ",
    ];
    const SYM_TOP: [&str; 3] = ["1234567890", "-/:;()$&@\"", ".,?!'#%^*"];
    KeyLayout {
        name: Cow::Borrowed("hangul"),
        faces: vec![
            letters(&LOWER, UPPER_FACE, lang),
            letters(&UPPER, LOWER_FACE, lang),
            symbols(&SYM_TOP, lang),
        ],
    }
}

/// One letter face. `shift_face` is the face ⇧ switches to.
fn letters(rows: &[&'static str; 3], shift_face: usize, lang: bool) -> KeyFace {
    KeyFace {
        rows: vec![
            row(chars(rows[0])),
            row(chars(rows[1])),
            {
                let mut keys = vec![KeyDef::special(KeyAction::Face(shift_face), "⇧", 1.5)];
                keys.extend(chars(rows[2]));
                keys.push(KeyDef::special(KeyAction::Backspace, "⌫", 1.5));
                row(keys)
            },
            bottom_row(SYMBOL_FACE, "?123", lang),
        ],
    }
}

/// The symbol face. Unlike a letter face it has no ⇧ position, so the third row is symbols
/// throughout — and to keep the labels unique, the only face-switch key is the `abc` on the
/// bottom row (so `key_rect` points at one place).
fn symbols(rows: &[&'static str; 3], lang: bool) -> KeyFace {
    KeyFace {
        rows: vec![
            row(chars(rows[0])),
            row(chars(rows[1])),
            {
                // The symbol face's third row has nine cells with the ⇧ place empty — the ⌫ is shrunk to one cell to make it 10.
                let mut keys = chars(rows[2]);
                keys.push(KeyDef::special(KeyAction::Backspace, "⌫", 1.0));
                row(keys)
            },
            bottom_row(LOWER_FACE, "abc", lang),
        ],
    }
}

/// The bottom row: the face switch · (한/영) · ⏮ · space · ⏭ · ▾ · ↵ (10 spans in total).
///
/// With the `한/영` key in, space shrinks from 4 spans to 3 to keep the total at 10.
fn bottom_row(face: usize, label: &'static str, lang: bool) -> KeyRow {
    let mut keys = vec![KeyDef::special(KeyAction::Face(face), label, 1.5)];
    if lang {
        keys.push(KeyDef::special(KeyAction::Lang, LANG_LABEL, 1.0));
    }
    keys.push(KeyDef::special(KeyAction::PrevFocus, "⏮", 1.0));
    keys.push(KeyDef::special(
        KeyAction::Space,
        " ",
        if lang { 3.0 } else { 4.0 },
    ));
    keys.push(KeyDef::special(KeyAction::NextFocus, "⏭", 1.0));
    keys.push(KeyDef::special(KeyAction::Hide, "▾", 1.0));
    keys.push(KeyDef::special(KeyAction::Enter, "↵", 1.5));
    row(keys)
}

/// The pieces from `split("")` are parts of the original `&'static str` and are borrowed as-is — no heap allocation.
fn chars(s: &'static str) -> Vec<KeyDef> {
    s.split("")
        .filter(|c| !c.is_empty())
        .map(KeyDef::text)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        hangul, numpad, qwerty, KeyAction, KeyDef, KeyFace, KeyLayout, OskLayout, LOWER_FACE,
        SYMBOL_FACE, UPPER_FACE,
    };
    use std::collections::BTreeSet;

    fn all_keys(face: &KeyFace) -> Vec<&KeyDef> {
        face.rows.iter().flat_map(|r| r.keys.iter()).collect()
    }

    #[test]
    fn numpad_has_digits_backspace_enter() {
        let l = numpad(true, false);
        let keys: Vec<_> = l.faces.iter().flat_map(all_keys).collect();
        assert!(keys.iter().any(|k| k.action == KeyAction::Text("0".into())));
        assert!(keys.iter().any(|k| k.action == KeyAction::Backspace));
        assert!(keys.iter().any(|k| k.action == KeyAction::Enter));
        assert!(keys.iter().any(|k| k.action == KeyAction::Text(".".into())));
    }

    /// The numpad is always 4 spans wide, with or without the decimal point and sign (so the key size does not shift).
    #[test]
    fn numpad_rows_are_always_four_spans() {
        for (decimal, sign) in [(false, false), (true, false), (false, true), (true, true)] {
            let l = numpad(decimal, sign);
            for face in &l.faces {
                for r in &face.rows {
                    let span: f32 = r.keys.iter().map(KeyDef::span).sum();
                    assert!(
                        (span - 4.0).abs() < 1e-4,
                        "decimal={decimal} sign={sign} span={span}"
                    );
                }
            }
        }
    }

    #[test]
    fn qwerty_has_three_faces_and_space() {
        let l = qwerty(false);
        assert_eq!(l.faces.len(), 3);
        assert!(l.faces.iter().all(|f| f.rows.len() == 4));
        assert!(l
            .faces
            .first()
            .and_then(|f| f.rows.last())
            .is_some_and(|r| r.keys.iter().any(|k| k.action == KeyAction::Space)));
    }

    /// Labels have to be unique within a face for `Osk::key_rect(label)` to point at one place.
    #[test]
    fn qwerty_labels_are_unique_within_a_face() {
        for (i, face) in qwerty(false).faces.iter().enumerate() {
            let keys = all_keys(face);
            let unique: BTreeSet<&str> = keys.iter().map(|k| k.label.as_ref()).collect();
            assert_eq!(unique.len(), keys.len(), "a repeated label on face {i}");
        }
    }

    /// Every row is 10 spans — the key width is the same on each. The total is unchanged with the `한/영` key in.
    #[test]
    fn qwerty_rows_share_the_same_span() {
        for face in qwerty(false)
            .faces
            .iter()
            .chain(qwerty(true).faces.iter())
            .chain(hangul(false).faces.iter())
            .chain(hangul(true).faces.iter())
        {
            assert!((face.max_span() - 10.0).abs() < 1e-4);
            for r in &face.rows {
                let span: f32 = r.keys.iter().map(KeyDef::span).sum();
                assert!(span <= 10.0 + 1e-4, "span = {span}");
            }
        }
    }

    /// The face switches: lowercase ⇄ uppercase, either letter face to symbols, and symbols back to lowercase.
    #[test]
    fn face_switches_are_reachable() {
        let l = qwerty(false);
        let face_targets = |i: usize| -> BTreeSet<usize> {
            l.faces
                .get(i)
                .map(|f| {
                    all_keys(f)
                        .iter()
                        .filter_map(|k| match k.action {
                            KeyAction::Face(t) => Some(t),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        assert_eq!(
            face_targets(LOWER_FACE),
            [UPPER_FACE, SYMBOL_FACE].into_iter().collect()
        );
        assert_eq!(
            face_targets(UPPER_FACE),
            [LOWER_FACE, SYMBOL_FACE].into_iter().collect()
        );
        assert_eq!(
            face_targets(SYMBOL_FACE),
            [LOWER_FACE].into_iter().collect()
        );
    }

    /// Every face has a hide key (so the back button's "close the OSK" can be done from inside the
    /// keyboard).
    #[test]
    fn every_face_can_hide() {
        for l in [qwerty(false), numpad(true, true)] {
            for face in &l.faces {
                assert!(all_keys(face).iter().any(|k| k.action == KeyAction::Hide));
            }
        }
    }

    // ---------------------------------------------------------------- dubeolsik

    /// Dubeolsik has the same three faces and 10/9/7 grid as qwerty.
    #[test]
    fn hangul_mirrors_the_qwerty_grid() {
        let ko = hangul(true);
        let en = qwerty(true);
        assert_eq!(ko.faces.len(), en.faces.len());
        for (k, e) in ko.faces.iter().zip(en.faces.iter()) {
            assert_eq!(k.rows.len(), e.rows.len());
            for (kr, er) in k.rows.iter().zip(e.rows.iter()) {
                assert_eq!(
                    kr.keys.len(),
                    er.keys.len(),
                    "the rows have to be the same length for the grids to match"
                );
            }
        }
    }

    /// The standard dubeolsik positions (KS X 5002). ⇧ changes only the tense consonants and `ㅒ` · `ㅖ`.
    #[test]
    fn hangul_uses_the_standard_dubeolsik_positions() {
        let l = hangul(false);
        let labels = |face: usize, r: usize| -> String {
            l.faces
                .get(face)
                .and_then(|f| f.rows.get(r))
                .map(|row| {
                    row.keys
                        .iter()
                        .filter(|k| matches!(k.action, KeyAction::Text(_)))
                        .map(|k| k.label.as_ref())
                        .collect::<String>()
                })
                .unwrap_or_default()
        };
        assert_eq!(labels(LOWER_FACE, 0), "ㅂㅈㄷㄱㅅㅛㅕㅑㅐㅔ");
        assert_eq!(labels(LOWER_FACE, 1), "ㅁㄴㅇㄹㅎㅗㅓㅏㅣ");
        assert_eq!(labels(LOWER_FACE, 2), "ㅋㅌㅊㅍㅠㅜㅡ");
        assert_eq!(labels(UPPER_FACE, 0), "ㅃㅉㄸㄲㅆㅛㅕㅑㅒㅖ");
        assert_eq!(
            labels(UPPER_FACE, 1),
            labels(LOWER_FACE, 1),
            "⇧ does not change the middle row"
        );
        assert_eq!(labels(UPPER_FACE, 2), labels(LOWER_FACE, 2));
    }

    /// Labels have to be unique within a face for `Osk::key_rect(label)` to point at one place.
    #[test]
    fn hangul_labels_are_unique_within_a_face() {
        for (i, face) in hangul(true).faces.iter().enumerate() {
            let keys = all_keys(face);
            let unique: BTreeSet<&str> = keys.iter().map(|k| k.label.as_ref()).collect();
            assert_eq!(unique.len(), keys.len(), "a repeated label on face {i}");
        }
    }

    /// The `한/영` key appears only when `lang` is true, and then **on every face** — being stranded on the symbol face is not acceptable.
    #[test]
    fn the_lang_key_is_on_every_face_only_when_asked() {
        let has_lang = |l: &KeyLayout| {
            l.faces
                .iter()
                .all(|f| all_keys(f).iter().any(|k| k.action == KeyAction::Lang))
        };
        let no_lang = |l: &KeyLayout| {
            l.faces
                .iter()
                .all(|f| all_keys(f).iter().all(|k| k.action != KeyAction::Lang))
        };
        assert!(has_lang(&hangul(true)));
        assert!(has_lang(&qwerty(true)));
        assert!(no_lang(&hangul(false)));
        assert!(no_lang(&qwerty(false)));
    }

    /// The config string resolves to a layout, and brings its composer and its other language with it.
    #[test]
    fn hangul_parses_and_carries_a_composer() {
        assert_eq!(
            OskLayout::parse("hangul", false, false),
            Some(OskLayout::Hangul)
        );
        assert_eq!(
            OskLayout::parse("ko", false, false),
            Some(OskLayout::Hangul)
        );
        assert!(OskLayout::Hangul.composer().is_some());
        assert!(OskLayout::Qwerty.composer().is_none());
        assert!(OskLayout::NumPad {
            decimal: false,
            sign: false
        }
        .composer()
        .is_none());
        assert_eq!(OskLayout::Hangul.lang_pair(), Some(OskLayout::Qwerty));
        assert_eq!(OskLayout::Qwerty.lang_pair(), Some(OskLayout::Hangul));
        assert_eq!(
            OskLayout::NumPad {
                decimal: false,
                sign: false
            }
            .lang_pair(),
            None
        );
        assert!(OskLayout::Hangul.has_shift());
        assert!(!OskLayout::Hangul.is_qwerty());
    }
}
