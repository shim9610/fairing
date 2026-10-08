//! Hangul composition (dubeolsik). The automaton that stacks jamo into syllables.
//!
//! # The syllable formula
//!
//! Hangul syllables are in Unicode already composed:
//!
//! ```text
//! U+AC00 + (cho × 21 + jung) × 28 + jong
//! ```
//!
//! 19 initials (cho) × 21 medials (jung) × 28 finals (jong, including none) = 11,172
//! characters. So composition is **arithmetic** and the only tables are the jamo lists — no
//! external dependency needed.
//!
//! # The state
//!
//! [`HangulComposer`] holds three slots: cho, jung and jong. On each key:
//!
//! - **A consonant** becomes the cho when the cho is empty, or the jong once the jung is
//!   filled. With a jong already there it tries a compound final (`ㄱ`+`ㅅ`=`ㄳ`), and failing
//!   that it commits the current syllable and opens a new one.
//! - **A vowel** becomes the jung when the jung is empty, and otherwise tries a compound vowel
//!   (`ㅗ`+`ㅏ`=`ㅘ`). A vowel arriving with a jong present **carries the final over**:
//!   `간` + `ㅏ` → `가` + `나`. With a compound final, the first stays and the second carries:
//!   `갃` + `ㅏ` → `각` + `사`.
//! - **Backspace** undoes the last jamo stacked. Compound finals and vowels unwind one step at
//!   a time.
//!
//! The composing string is put into the buffer as real characters rather than an
//! [`egui::Event::Ime`] `Preedit`, so it is visible while composing and simply becomes final
//! when committed (see [`super::compose`]).

// The initial (cho), medial (jung) and final (jong) are this domain's own names and so resemble each
// other. Calling them anything else would part company with the standard terms and read worse.
#![expect(clippy::similar_names, reason = "cho/jung/jong are the standard terms")]

use super::compose::{Compose, Composer};

/// The start of the Hangul syllable block (`가`).
const SYLLABLE_BASE: u32 = 0xAC00;
/// The medial count.
const JUNG_COUNT: u32 = 21;
/// The final count (including none).
const JONG_COUNT: u32 = 28;
/// The start of the compatibility jamo vowel block (`ㅏ`). Its order matches [`JUNG`] exactly, so an index can just be added.
const COMPAT_VOWEL_BASE: u32 = 0x314F;

/// The 19 initials (their order is the index).
const CHO: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ',
    'ㅌ', 'ㅍ', 'ㅎ',
];

/// The 21 medials. In the same order as the compatibility jamo from `U+314F` (`ㅏ`).
const JUNG: [char; 21] = [
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ',
    'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
];

/// The 27 finals (**excluding "none"**, so index 0 is `ㄱ` — add 1 when putting it into the syllable formula).
const JONG: [char; 27] = [
    'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ', 'ㅁ',
    'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

/// Compound vowels as `(first, second, combined)` — [`JUNG`] indices.
const JUNG_COMPOUND: [(u8, u8, u8); 7] = [
    (8, 0, 9),    // ㅗ + ㅏ = ㅘ
    (8, 1, 10),   // ㅗ + ㅐ = ㅙ
    (8, 20, 11),  // ㅗ + ㅣ = ㅚ
    (13, 4, 14),  // ㅜ + ㅓ = ㅝ
    (13, 5, 15),  // ㅜ + ㅔ = ㅞ
    (13, 20, 16), // ㅜ + ㅣ = ㅟ
    (18, 20, 19), // ㅡ + ㅣ = ㅢ
];

/// Compound finals as `(first, second, combined)` — [`JONG`] indices.
const JONG_COMPOUND: [(u8, u8, u8); 11] = [
    (0, 18, 2),   // ㄱ + ㅅ = ㄳ
    (3, 21, 4),   // ㄴ + ㅈ = ㄵ
    (3, 26, 5),   // ㄴ + ㅎ = ㄶ
    (7, 0, 8),    // ㄹ + ㄱ = ㄺ
    (7, 15, 9),   // ㄹ + ㅁ = ㄻ
    (7, 16, 10),  // ㄹ + ㅂ = ㄼ
    (7, 18, 11),  // ㄹ + ㅅ = ㄽ
    (7, 24, 12),  // ㄹ + ㅌ = ㄾ
    (7, 25, 13),  // ㄹ + ㅍ = ㄿ
    (7, 26, 14),  // ㄹ + ㅎ = ㅀ
    (16, 18, 17), // ㅂ + ㅅ = ㅄ
];

/// The dubeolsik Hangul composer.
///
/// ```
/// use fairing::osk::{Composer, HangulComposer};
///
/// let mut ime = HangulComposer::new();
/// for jamo in ["ㄱ", "ㅏ", "ㄴ"] {
///     let _ = ime.feed(jamo);
/// }
/// assert_eq!(ime.preedit(), "간");
/// assert_eq!(ime.flush(), "간");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HangulComposer {
    /// The initial (a [`CHO`] index).
    cho: Option<u8>,
    /// The medial (a [`JUNG`] index).
    jung: Option<u8>,
    /// The final (a [`JONG`] index — "none" is `None`).
    jong: Option<u8>,
}

impl HangulComposer {
    /// An empty composer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The current state as a string. With only an initial or only a medial, it is one compatibility jamo.
    fn render(&self) -> String {
        match (self.cho, self.jung, self.jong) {
            (Some(cho), Some(jung), jong) => {
                let jong = jong.map_or(0, |j| u32::from(j) + 1);
                let code = SYLLABLE_BASE
                    + (u32::from(cho) * JUNG_COUNT + u32::from(jung)) * JONG_COUNT
                    + jong;
                char::from_u32(code).map(String::from).unwrap_or_default()
            }
            // A consonant alone has been pressed. It never sits in the final's slot (a final does not fill without a medial).
            (Some(cho), None, _) => CHO
                .get(cho as usize)
                .copied()
                .map(String::from)
                .unwrap_or_default(),
            // A vowel alone has been pressed.
            (None, Some(jung), _) => JUNG
                .get(jung as usize)
                .copied()
                .map(String::from)
                .unwrap_or_default(),
            (None, None, _) => String::new(),
        }
    }

    /// Whether it is empty.
    fn is_empty(&self) -> bool {
        self.cho.is_none() && self.jung.is_none() && self.jong.is_none()
    }

    /// Take the state out as a committed string and clear it.
    fn take(&mut self) -> String {
        let text = self.render();
        self.cho = None;
        self.jung = None;
        self.jong = None;
        text
    }

    /// Feed one consonant.
    fn push_consonant(&mut self, ch: char) -> Compose {
        let cho_index = cho_index(ch);
        let jong_index = jong_index(ch);

        // The initial's slot is empty and there is no medial → this consonant is the initial.
        if self.cho.is_none() && self.jung.is_none() {
            if let Some(cho) = cho_index {
                self.cho = Some(cho);
                return Compose::taken(String::new(), self.render());
            }
            // A jamo that cannot be an initial (a component of a compound final, and so on) is committed as one character as it stands.
            return Compose::taken(String::new(), String::new());
        }

        // A consonant arriving on a vowel-only state (`ㅏ`) commits the previous character and opens a new one.
        if self.cho.is_none() {
            let commit = self.take();
            return match cho_index {
                Some(cho) => {
                    self.cho = Some(cho);
                    Compose::taken(commit, self.render())
                }
                None => Compose::flushed(commit),
            };
        }

        // There is no medial yet → two consonants came in a row. The previous character is committed and a new one opened.
        let Some(_) = self.jung else {
            let commit = self.take();
            return match cho_index {
                Some(cho) => {
                    self.cho = Some(cho);
                    Compose::taken(commit, self.render())
                }
                None => Compose::flushed(commit),
            };
        };

        match (self.jong, jong_index) {
            // The final is empty and this consonant can be a final.
            (None, Some(jong)) => {
                self.jong = Some(jong);
                Compose::taken(String::new(), self.render())
            }
            // There is a final → a compound final is attempted.
            (Some(prev), Some(jong)) => match compound_jong(prev, jong) {
                Some(merged) => {
                    self.jong = Some(merged);
                    Compose::taken(String::new(), self.render())
                }
                None => self.restart_with_consonant(cho_index),
            },
            // A consonant that cannot be a final (ㄸ · ㅃ · ㅉ) → a new syllable is opened.
            (_, None) => self.restart_with_consonant(cho_index),
        }
    }

    /// Commit the current syllable and open a new one with a consonant.
    fn restart_with_consonant(&mut self, cho_index: Option<u8>) -> Compose {
        let commit = self.take();
        match cho_index {
            Some(cho) => {
                self.cho = Some(cho);
                Compose::taken(commit, self.render())
            }
            None => Compose::flushed(commit),
        }
    }

    /// Feed one vowel.
    fn push_vowel(&mut self, ch: char) -> Compose {
        let Some(jung) = jung_index(ch) else {
            return Compose::flushed(self.take());
        };

        // With a final present it carries over: `간` + `ㅏ` → `가` + `나`.
        if let Some(prev_jong) = self.jong {
            let (keep, moved) = split_jong(prev_jong);
            self.jong = keep;
            let commit = self.take();
            self.cho = JONG.get(moved as usize).copied().and_then(cho_index);
            self.jung = Some(jung);
            return Compose::taken(commit, self.render());
        }

        match self.jung {
            // The medial is empty → this vowel is the medial (with no initial, a one-character vowel).
            None => {
                self.jung = Some(jung);
                Compose::taken(String::new(), self.render())
            }
            // There is a medial → a compound vowel is attempted, and failing that a new syllable is opened.
            Some(prev) => {
                if let Some(merged) = compound_jung(prev, jung) {
                    self.jung = Some(merged);
                    Compose::taken(String::new(), self.render())
                } else {
                    let commit = self.take();
                    self.jung = Some(jung);
                    Compose::taken(commit, self.render())
                }
            }
        }
    }
}

impl Composer for HangulComposer {
    fn name(&self) -> &'static str {
        "hangul"
    }

    fn feed(&mut self, text: &str) -> Compose {
        // What composes is a single jamo. Any other input only ends the composition and is handed to the caller.
        let mut chars = text.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return Compose::flushed(self.take());
        };
        if is_vowel(ch) {
            self.push_vowel(ch)
        } else if is_consonant(ch) {
            self.push_consonant(ch)
        } else {
            Compose::flushed(self.take())
        }
    }

    fn backspace(&mut self) -> Compose {
        if self.is_empty() {
            return Compose::passthrough();
        }
        if let Some(jong) = self.jong {
            // A compound final unwinds one step only: `갃` → `각`.
            self.jong = decompose_jong(jong);
        } else if let Some(jung) = self.jung {
            // A compound vowel unwinds one step only too: `과` → `고`.
            self.jung = decompose_jung(jung);
        } else {
            self.cho = None;
        }
        Compose::taken(String::new(), self.render())
    }

    fn flush(&mut self) -> String {
        self.take()
    }

    fn reset(&mut self) {
        self.cho = None;
        self.jung = None;
        self.jong = None;
    }

    fn is_composing(&self) -> bool {
        !self.is_empty()
    }

    fn preedit(&self) -> String {
        self.render()
    }
}

/// Whether it is a compatibility jamo vowel (`ㅏ`..`ㅣ`).
fn is_vowel(ch: char) -> bool {
    ('ㅏ'..='ㅣ').contains(&ch)
}

/// Whether it is a compatibility jamo consonant (`ㄱ`..`ㅎ`).
fn is_consonant(ch: char) -> bool {
    ('ㄱ'..='ㅎ').contains(&ch)
}

/// The initial index. `None` for a jamo that cannot be an initial.
fn cho_index(ch: char) -> Option<u8> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "CHO has 19 entries, so the index fits in a u8"
    )]
    CHO.iter().position(|&c| c == ch).map(|i| i as u8)
}

/// The medial index. The compatibility jamo vowel block has the same order as [`JUNG`], so a subtraction is enough.
fn jung_index(ch: char) -> Option<u8> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the vowel block is 21 characters, so the index fits in a u8"
    )]
    if is_vowel(ch) {
        Some((ch as u32 - COMPAT_VOWEL_BASE) as u8)
    } else {
        None
    }
}

/// The final index. `None` for a consonant that cannot be a final (`ㄸ`, `ㅃ`, `ㅉ`).
fn jong_index(ch: char) -> Option<u8> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "JONG has 27 entries, so the index fits in a u8"
    )]
    JONG.iter().position(|&c| c == ch).map(|i| i as u8)
}

/// Combine a compound vowel.
fn compound_jung(first: u8, second: u8) -> Option<u8> {
    JUNG_COMPOUND
        .iter()
        .find(|(a, b, _)| *a == first && *b == second)
        .map(|(_, _, merged)| *merged)
}

/// Combine a compound final.
fn compound_jong(first: u8, second: u8) -> Option<u8> {
    JONG_COMPOUND
        .iter()
        .find(|(a, b, _)| *a == first && *b == second)
        .map(|(_, _, merged)| *merged)
}

/// Unwind a compound vowel by one step. `None` for a simple vowel (that is, the medial disappears).
fn decompose_jung(jung: u8) -> Option<u8> {
    JUNG_COMPOUND
        .iter()
        .find(|(_, _, merged)| *merged == jung)
        .map(|(first, _, _)| *first)
}

/// Unwind a compound final by one step. `None` for a simple final (that is, the final disappears).
fn decompose_jong(jong: u8) -> Option<u8> {
    JONG_COMPOUND
        .iter()
        .find(|(_, _, merged)| *merged == jong)
        .map(|(first, _, _)| *first)
}

/// Split a final into `(what stays, what carries over)`. With a compound final the first stays and the second carries.
fn split_jong(jong: u8) -> (Option<u8>, u8) {
    JONG_COMPOUND
        .iter()
        .find(|(_, _, merged)| *merged == jong)
        .map_or((None, jong), |(first, second, _)| (Some(*first), *second))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed a list of jamo in order and return `(everything committed, what is composing)`.
    fn type_jamo(input: &[&str]) -> (String, String) {
        let mut ime = HangulComposer::new();
        let mut committed = String::new();
        for jamo in input {
            let out = ime.feed(jamo);
            committed.push_str(&out.commit);
            if !out.consumed {
                committed.push_str(jamo);
            }
        }
        (committed, ime.preedit())
    }

    /// The final string after feeding every jamo and committing.
    fn typed(input: &[&str]) -> String {
        let (mut committed, preedit) = type_jamo(input);
        committed.push_str(&preedit);
        committed
    }

    #[test]
    fn tables_have_the_documented_sizes() {
        assert_eq!(CHO.len(), 19);
        assert_eq!(JUNG.len(), 21);
        assert_eq!(JONG.len(), 27, "not counting 'no final'");
        assert_eq!(JUNG_COUNT as usize, JUNG.len());
        assert_eq!(JONG_COUNT as usize, JONG.len() + 1);
    }

    #[test]
    fn the_vowel_block_lines_up_with_the_jung_table() {
        for (index, &vowel) in JUNG.iter().enumerate() {
            assert_eq!(
                jung_index(vowel),
                u8::try_from(index).ok(),
                "the medial index of {vowel}"
            );
        }
    }

    #[test]
    fn compound_tables_point_at_real_jamo() {
        for (first, second, merged) in JUNG_COMPOUND {
            for index in [first, second, merged] {
                assert!((index as usize) < JUNG.len(), "medial index {index}");
            }
        }
        for (first, second, merged) in JONG_COMPOUND {
            for index in [first, second, merged] {
                assert!((index as usize) < JONG.len(), "final index {index}");
            }
            let tail = JONG.get(second as usize).copied();
            assert!(
                tail.and_then(cho_index).is_some(),
                "the second jamo of a compound final, {tail:?}, has to carry over as an initial"
            );
        }
    }

    #[test]
    fn a_simple_syllable_builds_up() {
        let (committed, preedit) = type_jamo(&["ㄱ"]);
        assert_eq!((committed.as_str(), preedit.as_str()), ("", "ㄱ"));
        let (committed, preedit) = type_jamo(&["ㄱ", "ㅏ"]);
        assert_eq!((committed.as_str(), preedit.as_str()), ("", "가"));
        let (committed, preedit) = type_jamo(&["ㄱ", "ㅏ", "ㄴ"]);
        assert_eq!((committed.as_str(), preedit.as_str()), ("", "간"));
    }

    #[test]
    fn a_lone_vowel_is_a_compatibility_jamo() {
        assert_eq!(typed(&["ㅏ"]), "ㅏ");
        assert_eq!(
            typed(&["ㅗ", "ㅏ"]),
            "ㅘ",
            "a compound vowel forms on its own too"
        );
        assert_eq!(
            typed(&["ㅏ", "ㅏ"]),
            "ㅏㅏ",
            "what cannot merge commits the first"
        );
    }

    #[test]
    fn a_final_consonant_migrates_to_the_next_syllable() {
        assert_eq!(typed(&["ㄱ", "ㅏ", "ㄴ", "ㅏ"]), "가나");
        assert_eq!(typed(&["ㅎ", "ㅏ", "ㄴ", "ㄱ", "ㅡ", "ㄹ"]), "한글");
    }

    #[test]
    fn a_compound_final_splits_when_a_vowel_follows() {
        // ㄱ + ㅏ + ㄱ + ㅅ = 갃, and an ㅏ after it gives 각 + 사.
        assert_eq!(typed(&["ㄱ", "ㅏ", "ㄱ", "ㅅ", "ㅏ"]), "각사");
        // 앉 + 아 = 안자.
        assert_eq!(typed(&["ㅇ", "ㅏ", "ㄴ", "ㅈ", "ㅏ"]), "안자");
    }

    #[test]
    fn compound_finals_and_vowels_form() {
        assert_eq!(typed(&["ㅇ", "ㅏ", "ㄴ", "ㅈ"]), "앉");
        assert_eq!(typed(&["ㄷ", "ㅏ", "ㄹ", "ㄱ"]), "닭");
        assert_eq!(
            typed(&["ㄱ", "ㅘ"]),
            "과",
            "a compound vowel typed as one jamo"
        );
        assert_eq!(
            typed(&["ㄱ", "ㅗ", "ㅏ"]),
            "과",
            "a compound vowel typed as two"
        );
        assert_eq!(typed(&["ㅁ", "ㅜ", "ㅓ"]), "뭐");
        assert_eq!(typed(&["ㅇ", "ㅡ", "ㅣ"]), "의");
    }

    #[test]
    fn a_consonant_that_cannot_be_a_final_starts_a_new_syllable() {
        // ㄸ cannot be a final.
        assert_eq!(typed(&["ㄱ", "ㅏ", "ㄸ"]), "가ㄸ");
        assert_eq!(typed(&["ㄱ", "ㅏ", "ㄸ", "ㅏ"]), "가따");
    }

    #[test]
    fn two_consonants_in_a_row_commit_the_first() {
        assert_eq!(typed(&["ㄱ", "ㄴ"]), "ㄱㄴ");
        assert_eq!(typed(&["ㄱ", "ㅏ", "ㄴ", "ㄴ"]), "간ㄴ");
    }

    #[test]
    fn backspace_unwinds_one_jamo_at_a_time() {
        let mut ime = HangulComposer::new();
        for jamo in ["ㄱ", "ㅏ", "ㄱ", "ㅅ"] {
            let _ = ime.feed(jamo);
        }
        assert_eq!(ime.preedit(), "갃");
        assert_eq!(
            ime.backspace().preedit,
            "각",
            "one step of the compound final"
        );
        assert_eq!(ime.backspace().preedit, "가", "the final goes");
        assert_eq!(ime.backspace().preedit, "ㄱ", "the medial goes");
        assert_eq!(ime.backspace().preedit, "", "the initial goes");
        assert!(!ime.is_composing());
        assert!(
            !ime.backspace().consumed,
            "with nothing composing, the caller sends a real backspace"
        );
    }

    #[test]
    fn backspace_unwinds_compound_vowels() {
        let mut ime = HangulComposer::new();
        for jamo in ["ㄱ", "ㅗ", "ㅏ"] {
            let _ = ime.feed(jamo);
        }
        assert_eq!(ime.preedit(), "과");
        assert_eq!(ime.backspace().preedit, "고");
        assert_eq!(ime.backspace().preedit, "ㄱ");
    }

    #[test]
    fn non_jamo_input_flushes_and_passes_through() {
        let mut ime = HangulComposer::new();
        for jamo in ["ㄱ", "ㅏ"] {
            let _ = ime.feed(jamo);
        }
        let out = ime.feed(" ");
        assert_eq!(out.commit, "가");
        assert!(!out.consumed, "the caller puts the space in as it is");
        assert!(!ime.is_composing());

        let out = ime.feed("1");
        assert_eq!(out.commit.len(), 0);
        assert!(!out.consumed);
    }

    #[test]
    fn flush_and_reset_differ_in_what_they_hand_back() {
        let mut ime = HangulComposer::new();
        let _ = ime.feed("ㄱ");
        let _ = ime.feed("ㅏ");
        assert_eq!(ime.flush(), "가");
        assert!(!ime.is_composing());

        let _ = ime.feed("ㄴ");
        let _ = ime.feed("ㅏ");
        assert_eq!(ime.preedit(), "나");
        ime.reset();
        assert!(
            !ime.is_composing(),
            "reset drops the syllable rather than committing it"
        );
        assert_eq!(ime.flush(), "");
    }

    #[test]
    fn every_syllable_in_the_block_round_trips() {
        // It walks every initial, medial and final and checks the formula against the Unicode syllable block.
        for (cho, _) in CHO.iter().enumerate() {
            for (jung, _) in JUNG.iter().enumerate() {
                for jong in 0..=JONG.len() {
                    let composer = HangulComposer {
                        cho: u8::try_from(cho).ok(),
                        jung: u8::try_from(jung).ok(),
                        jong: if jong == 0 {
                            None
                        } else {
                            u8::try_from(jong - 1).ok()
                        },
                    };
                    let text = composer.render();
                    assert_eq!(text.chars().count(), 1, "has to be one syllable: {text:?}");
                    let Some(ch) = text.chars().next() else {
                        continue;
                    };
                    let code = ch as u32 - SYLLABLE_BASE;
                    assert_eq!((code / JONG_COUNT / JUNG_COUNT) as usize, cho);
                    assert_eq!((code / JONG_COUNT % JUNG_COUNT) as usize, jung);
                    assert_eq!((code % JONG_COUNT) as usize, jong);
                }
            }
        }
    }

    #[test]
    fn a_realistic_sentence_types_out() {
        // "안녕하세요"
        assert_eq!(
            typed(&[
                "ㅇ", "ㅏ", "ㄴ", "ㄴ", "ㅕ", "ㅇ", "ㅎ", "ㅏ", "ㅅ", "ㅔ", "ㅇ", "ㅛ"
            ]),
            "안녕하세요"
        );
        // "값" — a word ending in a compound final.
        assert_eq!(typed(&["ㄱ", "ㅏ", "ㅂ", "ㅅ"]), "값");
        // "없다"
        assert_eq!(typed(&["ㅇ", "ㅓ", "ㅂ", "ㅅ", "ㄷ", "ㅏ"]), "없다");
    }
}
