//! **Step 2 of the density chain** — the environment.
//!
//! The chain has five steps and the code implemented three: the integrator's pin, the backend's
//! `DisplayInfo`, and the fallback. The environment step was specified in full — four variables in
//! priority order, a policy lock, and a warning whenever it wins — and never built, so
//! [`ScaleSource::Env`](crate::unit::ScaleSource::Env) sat in the enum as the marker for something
//! that could not happen.
//!
//! # Why it exists at all
//!
//! The reason is a person rather than a feature: a maintenance engineer standing at
//! the device, with no permission to edit its files, who needs the UI bigger to read it. That is
//! also why the environment beats the config file but never the integrator's code pin — the
//! engineer is working around the file, and the integrator is stating a fact about their hardware.
//!
//! # The variables
//!
//! | | sets | note |
//! |---|---|---|
//! | `FAIRING_PPP` | [`ScalePolicy::ppp_pin`] | the bluntest — how big a `du` is, said directly |
//! | `FAIRING_PPI` | the density | pixels per **inch**, the number a spec sheet prints |
//! | `FAIRING_PHYSICAL_MM` | the panel's size, `"152.4x91.4"` | density comes from it and `size_px` |
//! | `FAIRING_UI_SCALE` | [`ScalePolicy::ui_scale`] | knows no density, just makes it bigger |
//!
//! Those four are **one chain**: the first that parses wins and the rest are ignored, in this order
//! (`FAIRING_PPP > FAIRING_PPI > FAIRING_PHYSICAL_MM > FAIRING_UI_SCALE`). Mixing
//! them would mean guessing whether `FAIRING_UI_SCALE` beside `FAIRING_PPP` means "and also scale"
//! or "instead", and a knob read at a device in a hurry should not need that guess.
//!
//! `FAIRING_FINGER_MM` is **not** in the chain and applies on its own. It says who is touching the
//! panel — a glove, a bare finger — which is a different question from how dense the panel is, and
//! both answers can be true at once.
//!
//! # Reading, once
//!
//! `std::env::var` is called once, from [`ShellBuilder::build`](super::ShellBuilder::build). There
//! is no IO inside a frame, and an environment that changed mid-run would move every dimension in
//! the shell. [`EnvScale::from_lookup`] is the whole of the parsing and takes the lookup as a
//! closure, so the tests never touch the process environment — which is shared by every test thread
//! and, from Rust 2024, `unsafe` to write.

use crate::unit::ScalePolicy;

/// The scale variables, already parsed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct EnvScale {
    /// The chain's winner, if any parsed.
    pub(crate) win: Option<EnvWin>,
    /// `FAIRING_FINGER_MM`, which is outside the chain.
    pub(crate) finger_mm: Option<f32>,
}

/// Which of the four won, and with what.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum EnvWin {
    /// `FAIRING_PPP` — a pinned `pixels_per_point`.
    Ppp(f32),
    /// `FAIRING_PPI` — converted to px per mm.
    PxPerMm(f32),
    /// `FAIRING_PHYSICAL_MM` — the panel's width and height in mm.
    PhysicalMm(f32, f32),
    /// `FAIRING_UI_SCALE` — a multiplier, no density.
    UiScale(f32),
}

impl EnvWin {
    /// The variable's name, for the warning.
    pub(crate) const fn var(self) -> &'static str {
        match self {
            Self::Ppp(_) => "FAIRING_PPP",
            Self::PxPerMm(_) => "FAIRING_PPI",
            Self::PhysicalMm(..) => "FAIRING_PHYSICAL_MM",
            Self::UiScale(_) => "FAIRING_UI_SCALE",
        }
    }
}

/// Millimetres in an inch — `FAIRING_PPI` is pixels per inch and the crate works in mm.
const MM_PER_INCH: f32 = 25.4;

impl EnvScale {
    /// Read from the process environment. Empty when the policy forbids it.
    pub(crate) fn from_env(policy: &ScalePolicy) -> Self {
        if !policy.allow_env {
            return Self::default();
        }
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// **The whole of the parsing**, against any lookup — see the module docs on why.
    ///
    /// A variable that is set but does not parse, or parses to something that is not finite and
    /// positive, is passed over as though it were not set at all. A typo should leave the device
    /// working, and the next step of the chain is the one that was going to be used anyway.
    pub(crate) fn from_lookup(look: impl Fn(&str) -> Option<String>) -> Self {
        let num = |name: &str| look(name).and_then(|v| positive(v.trim()));
        let win = num("FAIRING_PPP")
            .map(EnvWin::Ppp)
            .or_else(|| num("FAIRING_PPI").map(|ppi| EnvWin::PxPerMm(ppi / MM_PER_INCH)))
            .or_else(|| {
                look("FAIRING_PHYSICAL_MM")
                    .and_then(|v| parse_mm(v.trim()))
                    .map(|(w, h)| EnvWin::PhysicalMm(w, h))
            })
            .or_else(|| num("FAIRING_UI_SCALE").map(EnvWin::UiScale));
        Self {
            win,
            finger_mm: num("FAIRING_FINGER_MM"),
        }
    }

    /// Fold what is outside the chain into the policy — the finger, and a `FAIRING_UI_SCALE` win.
    pub(crate) fn apply(self, mut policy: ScalePolicy) -> ScalePolicy {
        if let Some(mm) = self.finger_mm {
            policy.finger_mm = mm;
        }
        match self.win {
            Some(EnvWin::Ppp(ppp)) => policy.ppp_pin = Some(ppp),
            Some(EnvWin::UiScale(k)) => policy.ui_scale = k,
            Some(EnvWin::PxPerMm(_) | EnvWin::PhysicalMm(..)) | None => {}
        }
        policy
    }

    /// The density this contributes, if it is one of the two that carry a density.
    ///
    /// `PhysicalMm` needs the panel's pixel size to become a density, so it stays in millimetres
    /// until the root rect is known — the same shape the integrator's pin already has.
    pub(crate) const fn physical_mm(self) -> Option<(f32, f32)> {
        match self.win {
            Some(EnvWin::PhysicalMm(w, h)) => Some((w, h)),
            _ => None,
        }
    }

    /// `FAIRING_PPI`'s density, in px per mm.
    pub(crate) const fn px_per_mm(self) -> Option<f32> {
        match self.win {
            Some(EnvWin::PxPerMm(v)) => Some(v),
            _ => None,
        }
    }
}

/// A finite, positive `f32`, or nothing.
fn positive(text: &str) -> Option<f32> {
    text.parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// `"152.4x91.4"` — the documented spelling for `FAIRING_PHYSICAL_MM`. `X` is taken too, and
/// spaces around it.
fn parse_mm(text: &str) -> Option<(f32, f32)> {
    let (w, h) = text.split_once(['x', 'X'])?;
    Some((positive(w.trim())?, positive(h.trim())?))
}

#[cfg(test)]
mod tests {
    use super::{EnvScale, EnvWin};
    use crate::unit::ScalePolicy;

    /// A lookup over a fixed table, standing in for the process environment.
    fn table<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn the_chain_takes_the_first_one_that_parses() {
        let all = EnvScale::from_lookup(table(&[
            ("FAIRING_PPP", "2.5"),
            ("FAIRING_PPI", "254"),
            ("FAIRING_PHYSICAL_MM", "152.4x91.4"),
            ("FAIRING_UI_SCALE", "1.4"),
        ]));
        assert_eq!(all.win, Some(EnvWin::Ppp(2.5)), "FAIRING_PPP is first");

        let ppi =
            EnvScale::from_lookup(table(&[("FAIRING_PPI", "254"), ("FAIRING_UI_SCALE", "9")]));
        assert_eq!(
            ppi.win,
            Some(EnvWin::PxPerMm(10.0)),
            "254 ppi is 10 px/mm, and it beats the scale below it"
        );
    }

    /// A variable that does not parse is passed over — the device goes on working.
    #[test]
    fn a_typo_falls_through_to_the_next_step() {
        let e = EnvScale::from_lookup(table(&[
            ("FAIRING_PPP", "twelve"),
            ("FAIRING_PPI", "-3"),
            ("FAIRING_PHYSICAL_MM", "152.4"),
            ("FAIRING_UI_SCALE", "1.25"),
        ]));
        assert_eq!(
            e.win,
            Some(EnvWin::UiScale(1.25)),
            "a word, a negative and a size with no height are all skipped"
        );
        assert_eq!(EnvScale::from_lookup(table(&[])).win, None);
    }

    #[test]
    fn the_panel_size_is_read_in_either_spelling() {
        for text in ["152.4x91.4", "152.4X91.4", " 152.4 x 91.4 "] {
            assert_eq!(
                EnvScale::from_lookup(table(&[("FAIRING_PHYSICAL_MM", text)])).win,
                Some(EnvWin::PhysicalMm(152.4, 91.4)),
                "{text}"
            );
        }
    }

    /// The finger is not in the chain: it answers a different question and applies beside a winner.
    #[test]
    fn the_finger_applies_beside_the_chain() {
        let e = EnvScale::from_lookup(table(&[("FAIRING_PPP", "3"), ("FAIRING_FINGER_MM", "9.0")]));
        assert_eq!(e.win, Some(EnvWin::Ppp(3.0)));
        let p = e.apply(ScalePolicy::default());
        assert!(
            (p.finger_mm - 9.0).abs() < f32::EPSILON && p.ppp_pin == Some(3.0),
            "both, not one or the other: {p:?}"
        );
    }

    /// `allow_env = false` is a lock, not a preference.
    #[test]
    fn the_policy_can_shut_the_whole_step_off() {
        let policy = ScalePolicy::default().with_allow_env(false);
        assert_eq!(
            EnvScale::from_env(&policy),
            EnvScale::default(),
            "with the lock on, nothing is even read"
        );
    }
}
