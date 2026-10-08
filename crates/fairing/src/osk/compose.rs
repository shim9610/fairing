//! The composing input layer. For writing systems where one character takes
//! several key presses.
//!
//! Latin is one key to one character and does not pass through this layer. Hangul stacks jamo
//! into syllables, and Chinese and Japanese write a pronunciation and then pick from candidates
//! — all three share the need to show "a character that is not final yet" (the composing string,
//! the preedit) on screen.
//!
//! A composer decides **only what to commit and what to leave composing**. Getting that onto the
//! screen is [`super::inject::inject_compose`]'s job, and it puts even the composing part in as
//! **real characters** — `ImeEvent::Preedit` is not used because it can vanish entirely on a
//! real device. So each time the composing string changes, the previous one is
//! taken back out with ⌫ and the new one typed.
//!
//! # Why a trait
//!
//! There is one implementation today, Hangul ([`super::hangul::HangulComposer`]). Chinese and
//! Japanese are not included because they need someone who can verify the input method itself —
//! what is provided instead is the place to attach one. A method that needs a candidate list
//! (pinyin, kana-kanji conversion) extends this trait with candidate methods.

/// What a composer returns for one input.
///
/// The caller handles it in this order:
/// 1. Take the previous step's `preedit` back out of the buffer.
/// 2. Type `commit + preedit` as characters — an empty `preedit` means the composition finished.
/// 3. If `consumed` is false, carry on with the key's original behaviour (input the composer did
///    not handle).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Compose {
    /// The string to commit. Empty means there is nothing to commit.
    pub commit: String,
    /// The composing string. Empty means there is no composition.
    pub preedit: String,
    /// Whether the composer consumed this input. `false` means the caller carries on with the original behaviour.
    pub consumed: bool,
}

impl Compose {
    /// It did nothing (the caller performs the original behaviour).
    #[must_use]
    pub fn passthrough() -> Self {
        Self::default()
    }

    /// It consumed it: something to commit, and something composing.
    #[must_use]
    pub fn taken(commit: String, preedit: String) -> Self {
        Self {
            commit,
            preedit,
            consumed: true,
        }
    }

    /// Commit only and hand the input back to the caller (a non-composing key arriving mid-composition).
    #[must_use]
    pub fn flushed(commit: String) -> Self {
        Self {
            commit,
            preedit: String::new(),
            consumed: false,
        }
    }

    /// Whether there is anything at all to apply to the screen.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.commit.is_empty() && self.preedit.is_empty() && !self.consumed
    }
}

/// A composing input method.
///
/// The implementation holds its own state and the caller calls [`Composer::feed`] on each key.
/// **It uses no lock types** — it is only ever touched as `&mut self` on the
/// single UI thread.
pub trait Composer {
    /// The name used in logs and config (`"hangul"`).
    fn name(&self) -> &'static str;

    /// Feed one character. For something it does not compose, it finishes the composition with
    /// [`Compose::flushed`] and returns `consumed = false`.
    fn feed(&mut self, text: &str) -> Compose;

    /// Backspace. Mid-composition it undoes one jamo and returns `consumed = true`; otherwise
    /// [`Compose::passthrough`] — the caller sends a real backspace.
    fn backspace(&mut self) -> Compose;

    /// Finish the composition and return the string to commit. An empty string when not composing.
    fn flush(&mut self) -> String;

    /// Throw the state away **without** committing. Used when focus moves to another widget — the
    /// composing string is already in the previous widget's buffer, so committing here would put
    /// the same characters into the new widget a second time.
    fn reset(&mut self);

    /// Whether it is composing.
    fn is_composing(&self) -> bool;

    /// The current composing string.
    fn preedit(&self) -> String;
}
