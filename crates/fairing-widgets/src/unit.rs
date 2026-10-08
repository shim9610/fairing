//! Length units and screen scale.
//!
//! This crate has **no fixed list of target devices**. Arbitrary specialised
//! touchscreen machines are the target, so there is no such thing as "a value tuned to the
//! representative device". Lengths are therefore not written as one logical pixel but as **a sum
//! of terms that mean different things** — the same 8 as "8 relative to the field of view", "8
//! physical mm" and "one finger" all have to move differently when the panel changes.
//!
//! # The anchor
//!
//! `1 du = 1 egui point = 1/160 in`. That value is fixed — changing it would make every `du` value
//! in the design and `theme`'s promoted-constant tests meaningless in one go.
//!
//! # Resolution
//!
//! [`Dim::resolve`] needs nothing but a [`Scale`] and does not care where it is called from. To
//! change what `frac_*` is relative to, make a `Copy` of the scale with a different reference
//! through [`Scale::in_container`] — the change of reference being **an explicit call** is what
//! keeps "resolution takes nothing but a `Scale`" true.

use std::marker::PhantomData;
use std::ops::{Add, AddAssign, Mul};

/// du → `epaint::CornerRadius` (a `u8`). Rounded and clamped to the range.
///
/// It sits on the `1 du = 1 egui point` anchor, so a du value goes straight in. Two lines, but
/// `theme`, `layout` and `widgets` all use it, so it lives here — copied into each, one copy
/// gets fixed and the corners differ from screen to screen. **Public** since the split:
/// `layout` is on the shell side of the crate boundary and needs the same rounding, and anyone
/// drawing their own control in du needs it for the same reason.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "it is after clamp(0,255), so it is inside u8's range"
)]
#[must_use]
pub fn round_u8(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// du → `epaint::Margin` (an `i8`). Rounded and clamped to the range.
#[expect(
    clippy::cast_possible_truncation,
    reason = "it is after clamp(-127,127), so it is inside i8's range"
)]
#[must_use]
pub fn round_i8(value: f32) -> i8 {
    value.round().clamp(-127.0, 127.0) as i8
}

/// The physical length of 1 du (mm). `1/160 in = 0.158750 mm`.
pub const MM_PER_DU: f32 = 0.158_750;

/// How many du are in 1 mm (the anchor density). `1 / MM_PER_DU ≈ 6.299213`.
pub const DU_PER_MM: f32 = 6.299_213;

/// The contact diameter of a bare finger (mm). The middle of the 8–10 mm found in contact-area studies.
pub const FINGER_BARE_MM: f32 = 9.0;

/// The finger diameter allowing for gloves or a stylus (mm).
pub const FINGER_GLOVED_MM: f32 = 13.0;

/// **How far the operator stands from the glass** (mm), when nothing says otherwise.
///
/// The hand and the eye are two different facts about a panel and they need two different knobs.
/// Until this one existed there was only [`ScalePolicy::finger_mm`], so the type scale had to hang
/// off the *finger* — and a screen read standing at a metre had no way to say so except to rewrite
/// `MetricsSpec::type_scale` in millimetres, which is rung 2 of the ladder and silently unhooks the
/// text from everything sized beside it. Measured on the shipped kiosk example, that is exactly
/// what happened: the text grew 2.30x and every control stayed at one finger.
///
/// 500 mm is a seated or leaning operator — the distance the shipped fractions already encoded.
pub const VIEWING_DEFAULT_MM: f32 = 500.0;

/// **The body em as a fraction of the viewing distance** — the one number that turns "how far away
/// is the reader" into "how big is the text".
///
/// It is not a taste. At [`VIEWING_DEFAULT_MM`] it gives a 4.342 mm em, which is exactly what
/// `Dim::finger(0.334)` gave at a gloved 13 mm finger, so no shipped panel moves. And it is the
/// value it is because of what that em subtends: a 4.342 mm em at 500 mm is 29.85 arcmin, and a
/// cap is 0.70 of an em, so the **capital subtends 20.9 arcmin** — ISO 9241-303's 20 arcmin floor
/// for comfortable reading, one tenth of a percent above it. Doubling the distance doubles the
/// text and the angle is unchanged, which is the whole point.
pub const BODY_PER_VIEWING: f32 = 0.008_684;

/// Fold a non-finite value onto a fallback. `Dim::px` goes to infinity when `pixels_per_point`
/// is 0, so this is not decoration.
#[inline]
fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

/// **What a length grows with**, carried in the type so a token cannot quietly change families.
///
/// Two of [`Dim`]'s seven terms are proportional to something about the person at the panel:
/// `finger` to the hand ([`ScalePolicy::finger_mm`]) and `text` to the eye
/// ([`ScalePolicy::viewing_distance_mm`]). The other five are absolute. A design holds together
/// because tokens that must stay in proportion are all driven by the *same* one of those two, and
/// it comes apart when they are not — not loudly, which is the problem: each token still resolves
/// to a perfectly good number, and the drawing is only right at the one panel size it was tuned
/// at.
///
/// So the anchor is a type parameter, `MetricsSpec`'s fields name the family each token belongs
/// to, and handing `row_height` a length built from `Dim::du` does not compile. Where a departure
/// is deliberate — a bench console whose status bar really is hand-sized rather than physical —
/// [`Span::reanchored`] spells it, and the spelling is the point.
///
/// # What stops compiling
///
/// Adding the two proportional terms together, which is a length with no single anchor:
///
/// ```compile_fail
/// use fairing_widgets::unit::Dim;
/// let d = Dim::text(3.0) + Dim::finger(1.0);
/// ```
///
/// Handing an eye-anchored token a length that is not anchored at all. This is the one that let
/// the console example match a drawing at 1536x1024 and come out of proportion everywhere else:
///
/// ```compile_fail
/// use fairing_widgets::theme::MetricsSpec;
/// use fairing_widgets::unit::{Dim, Span};
/// let spec = MetricsSpec {
///     row_height: Span::fixed(Dim::du(46.0)),
///     ..MetricsSpec::default()
/// };
/// ```
///
/// And handing it one anchored to the *other* thing — a type scale that follows the fingertip
/// while every other text-derived token follows the eye:
///
/// ```compile_fail
/// use fairing_widgets::theme::MetricsSpec;
/// use fairing_widgets::unit::{Dim, Span};
/// let spec = MetricsSpec {
///     type_scale: [Span::fixed(Dim::finger(0.334)); 5],
///     ..MetricsSpec::default()
/// };
/// ```
///
/// # What still does
///
/// The anchored form, a rigid trim on top of it, and either departure said out loud:
///
/// ```
/// use fairing_widgets::theme::MetricsSpec;
/// use fairing_widgets::unit::{Dim, Span};
/// let spec = MetricsSpec {
///     // A body-relative row with a flat trim, and a floor under it.
///     row_height: Span::fixed(Dim::text(2.6) + Dim::du(6.0)).min(Dim::du(32.0)),
///     // "56 du, and I do mean 56 du."
///     status_bar_height: Span::fixed(Dim::du(56.0)).pinned(),
///     // "Hand-sized, in a field the crate sizes in millimetres."
///     nav_bar_height: Span::fixed(Dim::finger(1.2)).reanchored(),
///     ..MetricsSpec::default()
/// };
/// assert!(spec.validate().is_ok());
/// ```
pub trait Anchor: Copy + Default + 'static {
    /// The family's name, for messages.
    const NAME: &'static str;
}

/// Anchored to the **eye**: built from [`Dim::text`], so it follows the viewing distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Eye;

/// Anchored to the **hand**: built from [`Dim::finger`], so it follows the fingertip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Hand;

/// Anchored to **neither** — `du`, `mm`, `px` and the fractions. A rigid length is admissible
/// anywhere, which is why it is the default parameter: every floor and cap in the crate is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rigid;

/// **No claim.** What [`Span::erased`] produces, for code that handles every token the same way
/// — validation walking a spec, say — and must not care which family each came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Free;

impl Anchor for Eye {
    const NAME: &'static str = "eye";
}
impl Anchor for Hand {
    const NAME: &'static str = "hand";
}
impl Anchor for Rigid {
    const NAME: &'static str = "rigid";
}
impl Anchor for Free {
    const NAME: &'static str = "free";
}

/// A length. **The sum** of seven terms that mean different things.
///
/// It is a sum rather than an enum because a combination like `mm(8) + du(4)` has to be
/// expressible without boxing, and because resolution is then a single dot product.
///
/// ```
/// use fairing_widgets::unit::{Dim, Scale};
///
/// let d = Dim::du(4.0) + Dim::mm(2.0);
/// let px = d.resolve(&Scale::identity());
/// assert!((px - (4.0 + 2.0 * fairing_widgets::unit::DU_PER_MM)).abs() < 1e-3);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Dim<A = Rigid> {
    /// Density-independent units (= egui points). Proportional to `ui_scale` — text, padding, visual radii.
    pub du: f32,
    /// Physical millimetres. Independent of `ui_scale` — hairline floors, icons, tick marks.
    pub mm: f32,
    /// Multiples of a finger. Proportional to [`ScalePolicy::finger_mm`] and independent of `ui_scale` — touch targets.
    pub finger: f32,
    /// Physical pixels. 1 px lines and texture alignment.
    pub px: f32,
    /// A fraction of the reference `Rect`'s width.
    pub frac_w: f32,
    /// A fraction of the reference `Rect`'s height.
    pub frac_h: f32,
    /// A fraction of the reference `Rect`'s shorter side.
    pub frac_min: f32,
    /// **Multiples of the body text.** Proportional to [`Scale::text_du`], so it follows the
    /// viewing distance rather than the hand — marks and gaps that are *looked at* beside a label.
    pub text: f32,
    /// Which of the two proportional terms this length is allowed to use. See [`Anchor`].
    anchor: PhantomData<A>,
}

impl<A: Anchor> Dim<A> {
    /// A length of zero.
    pub const ZERO: Self = Self {
        du: 0.0,
        mm: 0.0,
        finger: 0.0,
        px: 0.0,
        frac_w: 0.0,
        frac_h: 0.0,
        frac_min: 0.0,
        text: 0.0,
        anchor: PhantomData,
    };

    /// Resolve to du (the one-line formula).
    ///
    /// The `px` term **does not divide when it is 0.** `0.0 / 0.0` is `NaN`, not `inf`, and NaN
    /// poisons the whole sum the moment it enters it, making even the floor and cap folding
    /// (`finite_or`) meaningless — a length that does not use `px` must not become NaN because
    /// `pixels_per_point` is 0.
    #[must_use]
    pub fn resolve(self, s: &Scale) -> f32 {
        let root_min = s.root_du.x.min(s.root_du.y);
        let px_term = if self.px == 0.0 {
            0.0
        } else {
            self.px / s.pixels_per_point
        };
        self.du
            + self.mm * s.du_per_mm
            + self.finger * s.finger_mm * s.du_per_mm
            + px_term
            + self.frac_min * root_min
            + self.frac_w * s.root_du.x
            + self.frac_h * s.root_du.y
            + self.text * s.text_du
    }

    /// Whether any term is negative. With a negative term in a length, the sign of the resolved value is unpredictable.
    #[must_use]
    pub fn has_negative_term(self) -> bool {
        self.du < 0.0
            || self.mm < 0.0
            || self.finger < 0.0
            || self.px < 0.0
            || self.frac_w < 0.0
            || self.frac_h < 0.0
            || self.frac_min < 0.0
            || self.text < 0.0
    }
}

impl Dim<Rigid> {
    /// Density-independent units.
    #[must_use]
    pub const fn du(v: f32) -> Self {
        Self {
            du: v,
            ..Self::ZERO
        }
    }
    /// Physical millimetres.
    #[must_use]
    pub const fn mm(v: f32) -> Self {
        Self {
            mm: v,
            ..Self::ZERO
        }
    }
    /// Physical pixels.
    #[must_use]
    pub const fn px(v: f32) -> Self {
        Self {
            px: v,
            ..Self::ZERO
        }
    }
    /// A fraction of the reference `Rect`'s width.
    #[must_use]
    pub const fn frac_w(v: f32) -> Self {
        Self {
            frac_w: v,
            ..Self::ZERO
        }
    }
    /// A fraction of the reference `Rect`'s height.
    #[must_use]
    pub const fn frac_h(v: f32) -> Self {
        Self {
            frac_h: v,
            ..Self::ZERO
        }
    }
    /// A fraction of the reference `Rect`'s shorter side.
    #[must_use]
    pub const fn frac_min(v: f32) -> Self {
        Self {
            frac_min: v,
            ..Self::ZERO
        }
    }
}

impl Dim<Eye> {
    /// **Multiples of the body text**, which is to say of the viewing distance.
    ///
    /// The unit to write a length in when its job is to be *seen* next to a label — a badge, a
    /// lamp, the gap between two rows. Written this way the relationship is in the token table
    /// where a reader can see it: `text(1.0)` is one body em, and nobody has to divide
    /// `finger(0.333)` by `finger(0.334)` to discover that a badge is one line of text tall.
    ///
    /// Use [`Self::finger`] instead — or as the floor under this — for anything a hand must land
    /// on. The two are different facts and they move independently
    /// ([`ScalePolicy::viewing_distance_mm`] against [`ScalePolicy::finger_mm`]).
    #[must_use]
    pub const fn text(v: f32) -> Self {
        Self {
            text: v,
            ..Self::ZERO
        }
    }
}

impl Dim<Hand> {
    /// Multiples of a finger. `finger(1.0)` is the minimum touch target.
    #[must_use]
    pub const fn finger(v: f32) -> Self {
        Self {
            finger: v,
            ..Self::ZERO
        }
    }
}

impl<A: Anchor> Dim<A> {
    /// **Drop the anchor**, keeping the terms. See [`Span::erased`].
    #[must_use]
    pub const fn erased(self) -> Dim<Free> {
        Dim {
            du: self.du,
            mm: self.mm,
            finger: self.finger,
            px: self.px,
            frac_w: self.frac_w,
            frac_h: self.frac_h,
            frac_min: self.frac_min,
            text: self.text,
            anchor: PhantomData,
        }
    }

    /// **Re-anchor**, keeping the terms. See [`Span::reanchored`], which is how a caller reaches
    /// this; on its own it says nothing about whether the result is a good idea.
    #[must_use]
    pub const fn reanchored<B: Anchor>(self) -> Dim<B> {
        Dim {
            du: self.du,
            mm: self.mm,
            finger: self.finger,
            px: self.px,
            frac_w: self.frac_w,
            frac_h: self.frac_h,
            frac_min: self.frac_min,
            text: self.text,
            anchor: PhantomData,
        }
    }
}

/// **Addition, but only within a family.** `text(3.0) + du(8.0)` is a body-relative height with a
/// flat trim on it and stays eye-anchored; `finger(1.0) + du(10.0)` is the same shape for the hand.
/// `text(3.0) + finger(1.0)` is the one that does not compile, because the result would move with
/// the viewing distance *and* the fingertip and there is no honest name for that — it is two
/// designs added together, and it is how a spec ends up right at exactly one panel size.
///
/// A rigid term takes the other side's family, which is why the trims above work and why a length
/// built only from `du`/`mm`/`px` can still be added to anything.
macro_rules! sum {
    ($lhs:ty, $rhs:ty => $out:ty) => {
        impl Add<Dim<$rhs>> for Dim<$lhs> {
            type Output = Dim<$out>;
            fn add(self, o: Dim<$rhs>) -> Dim<$out> {
                Dim {
                    du: self.du + o.du,
                    mm: self.mm + o.mm,
                    finger: self.finger + o.finger,
                    px: self.px + o.px,
                    frac_w: self.frac_w + o.frac_w,
                    frac_h: self.frac_h + o.frac_h,
                    frac_min: self.frac_min + o.frac_min,
                    text: self.text + o.text,
                    anchor: PhantomData,
                }
            }
        }
    };
}

sum!(Rigid, Rigid => Rigid);
sum!(Eye, Eye => Eye);
sum!(Eye, Rigid => Eye);
sum!(Rigid, Eye => Eye);
sum!(Hand, Hand => Hand);
sum!(Hand, Rigid => Hand);
sum!(Rigid, Hand => Hand);
sum!(Free, Free => Free);

impl<A: Anchor> AddAssign for Dim<A>
where
    Dim<A>: Add<Dim<A>, Output = Dim<A>>,
{
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl<A: Anchor> Mul<f32> for Dim<A> {
    type Output = Self;
    fn mul(self, k: f32) -> Self {
        Self {
            du: self.du * k,
            mm: self.mm * k,
            finger: self.finger * k,
            px: self.px * k,
            frac_w: self.frac_w * k,
            frac_h: self.frac_h * k,
            frac_min: self.frac_min * k,
            text: self.text * k,
            anchor: PhantomData,
        }
    }
}

/// A [`Dim`] plus a floor and a cap.
///
/// **The floor beats the cap, and non-finite values fold onto the fallback.** `f32::clamp` is
/// not used because it panics on `min > max` and on NaN, and **the defaults actually reach that
/// condition** — on a 3.2-inch 240×320 panel, `touch_target`'s cap of `frac_min(0.14)` is
/// 33.6 du while its floor `du(40)` is 40 du. A device panicking during boot is not justifiable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span<A = Rigid> {
    /// The base length. Folded between the floor and the cap. **Its family is the span's.**
    pub base: Dim<A>,
    /// The floor. Where it contradicts the cap, this wins.
    ///
    /// Rigid, and not by omission: a floor is the point below which a length stops being usable at
    /// all — a 48 du touch target, a 1 px hairline — and that is an absolute claim about the panel,
    /// not a proportion of anything. Every floor and cap in the crate is one.
    pub min: Dim<Rigid>,
    /// The cap. Non-finite folds to infinity. Rigid for the same reason as [`Self::min`].
    pub max: Dim<Rigid>,
}

impl<A: Anchor> Span<A> {
    /// A length with no floor or cap.
    #[must_use]
    pub const fn fixed(base: Dim<A>) -> Self {
        Self {
            base,
            min: Dim::ZERO,
            max: Dim::ZERO,
        }
    }

    /// Attach a floor.
    #[must_use]
    pub const fn min(mut self, min: Dim<Rigid>) -> Self {
        self.min = min;
        self
    }

    /// Attach a cap.
    #[must_use]
    pub const fn max(mut self, max: Dim<Rigid>) -> Self {
        self.max = max;
        self
    }

    /// Resolve to du. It does not panic.
    #[must_use]
    pub fn resolve(self, s: &Scale) -> f32 {
        let lo = finite_or(self.min.resolve(s), 0.0);
        let hi_raw = self.max.resolve(s);
        // Not given a `max` (ZERO) means no cap — folding it to 0 would make every length disappear.
        let hi = if self.max == Dim::<Rigid>::ZERO {
            f32::INFINITY
        } else {
            finite_or(hi_raw, f32::INFINITY)
        };
        let base = finite_or(self.base.resolve(s), lo);
        base.max(lo).min(hi.max(lo))
    }

    /// Whether the floor exceeds the cap at this scale (for diagnostics).
    #[must_use]
    pub fn is_inverted(self, s: &Scale) -> bool {
        if self.max == Dim::<Rigid>::ZERO {
            return false;
        }
        let lo = finite_or(self.min.resolve(s), 0.0);
        let hi = finite_or(self.max.resolve(s), f32::INFINITY);
        lo > hi
    }

    /// Reject negative terms and check that the `Scale::identity()` resolution is positive.
    ///
    /// # Errors
    /// [`crate::Error::Config`] if any term is negative or the identity resolution is at or
    /// below 0. `what` goes into the message so you know which token it was.
    pub fn validate(&self, what: &str) -> crate::Result<()> {
        for (name, d) in [
            ("base", self.base.erased()),
            ("min", self.min.erased()),
            ("max", self.max.erased()),
        ] {
            if d.has_negative_term() {
                return Err(crate::Error::Config(format!(
                    "{what}.{name} has a negative term - a length cannot be negative"
                )));
            }
        }
        let v = self.resolve(&Scale::identity());
        if v.is_nan() || v <= 0.0 {
            return Err(crate::Error::Config(format!(
                "{what} resolves to {v} du at the base scale - it must be greater than 0"
            )));
        }
        Ok(())
    }

    /// **Put this length in another family on purpose.**
    ///
    /// The anchors exist so a spec cannot drift out of proportion by accident, not to
    /// claim the crate's choice of family is right for every panel. A console driven by a mouse
    /// under a bench lamp may genuinely want its status bar sized by the hand rather than in
    /// millimetres; a kiosk behind glass may want the opposite. Both are legitimate, and both are
    /// decisions somebody should be able to find later — which is the whole reason this is a word
    /// at the call site and not an implicit conversion.
    ///
    /// ```
    /// use fairing_widgets::unit::{Dim, Eye, Span};
    ///
    /// // `finger` is hand-anchored, so this needs saying out loud to sit in an eye-anchored field.
    /// let bar: Span<Eye> = Span::fixed(Dim::finger(0.95)).reanchored();
    /// # let _ = bar;
    /// ```
    #[must_use]
    pub const fn reanchored<B: Anchor>(self) -> Span<B> {
        Span {
            base: self.base.reanchored(),
            min: self.min,
            max: self.max,
        }
    }

    /// **Drop the family**, for code that treats every token alike.
    ///
    /// Validation walks a whole spec and asks the same three questions of each length; it has no
    /// business knowing what any of them is anchored to. Erasing is safe in that direction and
    /// only that one — a [`Span<Free>`] cannot be put back into a typed field without
    /// [`Self::reanchored`] saying which family it is rejoining.
    #[must_use]
    pub const fn erased(self) -> Span<Free> {
        Span {
            base: self.base.erased(),
            min: self.min,
            max: self.max,
        }
    }
}

impl Span<Rigid> {
    /// **Hold this token constant** in a field whose family scales.
    ///
    /// Mechanically the same as [`Self::reanchored`] — a rigid length claims neither anchor, so it
    /// is admissible anywhere — but it is a different statement and deserves a different word. A
    /// rigid length in an anchored field is not a neutral choice: it is a token that will *not*
    /// move when the rest of the design does, so at any panel but the one it was measured on it
    /// sits out of proportion with its neighbours. That is drift, and it is the failure carrying
    /// the anchor in the type exists to stop, so it has to be typed out rather than fallen into.
    ///
    /// It is still often right — a hairline, a knob's inset, an optical nudge somebody measured
    /// once. `pinned()` says that was meant.
    ///
    /// ```
    /// use fairing_widgets::unit::{Dim, Eye, Span};
    ///
    /// // "46 du, and I do mean 46 du, whatever the panel" - not "46 du because I forgot".
    /// let row: Span<Eye> = Span::fixed(Dim::du(46.0)).pinned();
    /// # let _ = row;
    /// ```
    #[must_use]
    pub const fn pinned<B: Anchor>(self) -> Span<B> {
        self.reanchored()
    }
}

impl<A: Anchor> From<Dim<A>> for Span<A> {
    fn from(base: Dim<A>) -> Self {
        Self::fixed(base)
    }
}

/// Where the density came from (the five steps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
// **No `#[non_exhaustive]`.** It carried one until the element layer became its own crate,
// at which point `fairing` itself became a downstream crate and the attribute started forcing `_ =>`
// arms inside this project — exactly the cost that is not worth paying. Adding a variant is meant
// to break the matches; that is the warning.
pub enum ScaleSource {
    /// The integrator pinned it in code.
    Pin,
    /// An environment variable.
    Env,
    /// A configuration file's panel size — the designed `[display] physical_mm`. That TOML section
    /// was not built, so the shell never reports this source today.
    Config,
    /// The backend's reported `DisplayInfo::physical_mm`.
    Backend,
    /// Nothing is known — [`ScalePolicy::assume_px_per_mm`] was used.
    #[default]
    Fallback,
}

/// How much that source is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
// **No `#[non_exhaustive]`.** It carried one until the element layer became its own crate,
// at which point `fairing` itself became a downstream crate and the attribute started forcing `_ =>`
// arms inside this project — exactly the cost that is not worth paying. Adding a variant is meant
// to break the matches; that is the warning.
pub enum ScaleConfidence {
    /// It was measured.
    Measured,
    /// A person declared it.
    Declared,
    /// It was assumed. At this value, the diagnostics reveal that the density is unknown.
    #[default]
    Assumed,
}

/// **The policy** for how density is used. Separate from "what is known" (`DisplayInfo`).
///
/// **It carries no `#[non_exhaustive]`.** This is a struct the integrator **builds**. With the
/// attribute, struct expressions outside the defining crate are blocked entirely — not even
/// `..Default::default()` works (a Rust rule) — and changing one field would mean memorising
/// every setter. Instead it provides [`Default`] so FRU works, and FRU keeps compiling as fields
/// are added. The value the shell returns ([`Scale`]) carries none either.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalePolicy {
    /// The finger diameter (mm). This one value moves every `Dim::finger` term and the audit
    /// thresholds together.
    ///
    /// The default is **gloved** ([`FINGER_GLOVED_MM`]). With no list of target devices and every
    /// kind of input in scope, a shell that declares nothing has to work
    /// under the hardest condition — assume a bare finger and meet a glove and it **cannot be
    /// pressed**, while the other way round is merely loose, and visibly so.
    pub finger_mm: f32,
    /// The "below this it is unusable" floor (mm). `None` means `finger_mm × 7/9`.
    pub finger_hard_mm: Option<f32>,
    /// The field-of-view scale. Proportional to the `du` term only.
    pub ui_scale: f32,
    /// The density to assume when the physical size stays unknown (physical px per mm).
    ///
    /// The default is the anchor itself ([`DU_PER_MM`]). That gains two things at once: `ppp`
    /// comes out at exactly 1.0 so the render is bit-identical to today's, and on an industrial
    /// panel at 5–6 px/mm the `mm` term comes out **larger** than life, which is the safe
    /// direction to be wrong in for touch.
    pub assume_px_per_mm: f32,
    /// **How far the operator's eye is from the glass** (mm). This one value moves every
    /// [`Dim::text`] term — the type scale and everything sized beside it.
    ///
    /// It is the eye's knob, and [`Self::finger_mm`] is the hand's. A standing kiosk raises this
    /// one and leaves the finger alone; a gloved bench instrument raises the finger and leaves this
    /// alone; a screen read across a room raises both. The default is
    /// [`VIEWING_DEFAULT_MM`], which reproduces the values the crate shipped when the type scale
    /// still hung off the finger.
    ///
    /// **Raise this rather than overriding `MetricsSpec::type_scale`.** Rewriting the type scale
    /// changes the unit the text is written in, and every length written in the old unit stops
    /// following it — which is how the kiosk example ended up with 2.30x text beside 1.00x
    /// controls.
    pub viewing_distance_mm: f32,
    /// The clamp range for `pixels_per_point`.
    ///
    /// The floor of 1.0 means "1 du never becomes smaller than one physical px", and it is why
    /// everything written in `du` draws pixel-identically to today on a panel at ≤ 6.299 px/mm.
    pub ppp_range: (f32, f32),
    /// **A pinned `pixels_per_point`**, bypassing the density derivation entirely.
    ///
    /// What `FAIRING_PPP` sets, and the code form of the designed `[scale] pixels_per_point` (that
    /// TOML section was not built). It is a last resort: pinning ppp says how big a `du`
    /// is without saying anything about the panel, so `mm` and `finger` terms go on deriving from
    /// `px_per_mm` and only the commit to egui is overridden. `ppp_range` still clamps it — a pin
    /// outside the range is a typo, not an instruction.
    pub ppp_pin: Option<f32>,
    /// **Whether the environment may override any of this** (step 2).
    ///
    /// `true` by default, and the reason is a person: a maintenance engineer standing at the device
    /// with no permission to edit its files, who needs the UI bigger to read it. Set it `false` on a
    /// device where that must not be possible; the shell then ignores every `FAIRING_*` scale
    /// variable.
    pub allow_env: bool,
}

impl Default for ScalePolicy {
    fn default() -> Self {
        Self {
            finger_mm: FINGER_GLOVED_MM,
            finger_hard_mm: None,
            ui_scale: 1.0,
            assume_px_per_mm: DU_PER_MM,
            viewing_distance_mm: VIEWING_DEFAULT_MM,
            ppp_range: (1.0, 8.0),
            ppp_pin: None,
            allow_env: true,
        }
    }
}

impl ScalePolicy {
    /// A bare-finger-only device (`finger_mm = 9.0`).
    #[must_use]
    pub fn bare() -> Self {
        Self {
            finger_mm: FINGER_BARE_MM,
            ..Self::default()
        }
    }

    /// A device operated with gloves (`finger_mm = 13.0`). The same as the default.
    #[must_use]
    pub fn gloved() -> Self {
        Self::default()
    }

    /// Set how far the operator stands from the glass (mm) — the eye's knob.
    #[must_use]
    pub fn with_viewing_distance_mm(mut self, mm: f32) -> Self {
        self.viewing_distance_mm = mm;
        self
    }

    /// Set the finger diameter (mm).
    #[must_use]
    pub fn with_finger_mm(mut self, mm: f32) -> Self {
        self.finger_mm = mm;
        self
    }

    /// Set the "below this it is unusable" floor (mm). Without it, `finger_mm × 7/9`.
    #[must_use]
    pub fn with_finger_hard_mm(mut self, mm: f32) -> Self {
        self.finger_hard_mm = Some(mm);
        self
    }

    /// Set the field-of-view scale.
    #[must_use]
    pub fn with_ui_scale(mut self, k: f32) -> Self {
        self.ui_scale = k;
        self
    }

    /// Set the density to assume when the physical size stays unknown (physical px per mm).
    #[must_use]
    pub fn with_assume_px_per_mm(mut self, v: f32) -> Self {
        self.assume_px_per_mm = v;
        self
    }

    /// Set [`ppp_pin`](Self::ppp_pin).
    #[must_use]
    pub fn with_ppp_pin(mut self, ppp: f32) -> Self {
        self.ppp_pin = Some(ppp);
        self
    }

    /// Set [`allow_env`](Self::allow_env).
    #[must_use]
    pub fn with_allow_env(mut self, allow: bool) -> Self {
        self.allow_env = allow;
        self
    }

    /// Set the clamp range for `pixels_per_point`.
    #[must_use]
    pub fn with_ppp_range(mut self, lo: f32, hi: f32) -> Self {
        self.ppp_range = (lo, hi);
        self
    }

    /// The effective floor (mm).
    #[must_use]
    pub fn hard_mm(&self) -> f32 {
        self.finger_hard_mm.unwrap_or(self.finger_mm * 7.0 / 9.0)
    }
}

/// A resolved scale. Decided once per frame and fixed for that whole frame.
#[derive(Debug, Clone, Copy, PartialEq)]
// **No `#[non_exhaustive]`.** It carried one until the element layer became its own crate,
// at which point `fairing` itself became a downstream crate and the attribute started forcing `_ =>`
// arms inside this project — exactly the cost that is not worth paying. Adding a variant is meant
// to break the matches; that is the warning.
pub struct Scale {
    /// The value committed to egui. Physical px per du.
    pub pixels_per_point: f32,
    /// The resolution coefficient for the `mm` term.
    pub du_per_mm: f32,
    /// Physical pixels per mm.
    pub px_per_mm: f32,
    /// The resolution coefficient for the `finger` term.
    pub finger_mm: f32,
    /// The resolution coefficient for the [`Dim::text`] term — one body em, in du.
    ///
    /// Derived from [`ScalePolicy::viewing_distance_mm`], not from `MetricsSpec`, so it is known
    /// before any spec resolves. `Shell` writes the resolved `type_scale.body` back over it
    /// afterwards, so a spec that *does* state its text another way still carries everything sized
    /// in `text` with it.
    pub text_du: f32,
    /// The `frac_*` reference = the shell's root `Rect` size (du).
    pub root_du: egui::Vec2,
    /// The field-of-view scale.
    pub ui_scale: f32,
    /// Where the density came from.
    pub source: ScaleSource,
    /// How much that source is trusted.
    pub confidence: ScaleConfidence,
}

impl Scale {
    /// The scale that is bit-identical to today's render. `ppp = 1`, `du = physical px`.
    #[must_use]
    pub fn identity() -> Self {
        Self {
            pixels_per_point: 1.0,
            du_per_mm: DU_PER_MM,
            px_per_mm: DU_PER_MM,
            finger_mm: FINGER_GLOVED_MM,
            text_du: BODY_PER_VIEWING * VIEWING_DEFAULT_MM * DU_PER_MM,
            root_du: egui::vec2(1024.0, 600.0),
            ui_scale: 1.0,
            source: ScaleSource::Fallback,
            confidence: ScaleConfidence::Assumed,
        }
    }

    /// Resolve from a density and a policy. If `px_per_mm` is not finite and positive, the policy's assumption is used.
    #[must_use]
    pub fn resolve(
        px_per_mm: f32,
        policy: &ScalePolicy,
        source: ScaleSource,
        confidence: ScaleConfidence,
        root_px: egui::Vec2,
    ) -> Self {
        let (px_per_mm, source, confidence) = if px_per_mm.is_finite() && px_per_mm > 0.0 {
            (px_per_mm, source, confidence)
        } else {
            (
                policy.assume_px_per_mm,
                ScaleSource::Fallback,
                ScaleConfidence::Assumed,
            )
        };
        // A pinned ppp skips the derivation; the clamp below still applies to it, because a pin
        // outside `ppp_range` is a typo rather than an instruction.
        let ppp_raw = policy
            .ppp_pin
            .filter(|p| p.is_finite() && *p > 0.0)
            .unwrap_or(px_per_mm * MM_PER_DU * policy.ui_scale);
        let (lo, hi) = policy.ppp_range;
        let ppp = finite_or(ppp_raw, 1.0).max(lo).min(hi.max(lo));
        // Where the clamp bit, the model is made consistent again.
        let ui_scale = ppp / (px_per_mm * MM_PER_DU);
        Self {
            pixels_per_point: ppp,
            du_per_mm: px_per_mm / ppp,
            px_per_mm,
            finger_mm: policy.finger_mm,
            text_du: BODY_PER_VIEWING * policy.viewing_distance_mm * (px_per_mm / ppp),
            root_du: root_px / ppp,
            ui_scale: finite_or(ui_scale, 1.0),
            source,
            confidence,
        }
    }

    /// A `Copy` with the [`Dim::text`] coefficient replaced by a resolved body em (du).
    ///
    /// `Shell` calls this between resolving `MetricsSpec` and resolving everything sized in `text`,
    /// so that an integrator who states the type scale directly still drags the badge, the lamp and
    /// the control heights along with it instead of leaving them behind.
    #[must_use]
    pub fn with_text_du(self, body_du: f32) -> Self {
        Self {
            text_du: if body_du.is_finite() && body_du > 0.0 {
                body_du
            } else {
                self.text_du
            },
            ..self
        }
    }

    /// A `Copy` with only the `frac_*` reference changed to a container.
    #[must_use]
    pub fn in_container(self, rect: egui::Rect) -> Self {
        Self {
            root_du: rect.size(),
            ..self
        }
    }

    /// Physical mm to du at this scale.
    #[must_use]
    pub fn mm_to_du(self, mm: f32) -> f32 {
        mm * self.du_per_mm
    }

    /// du to physical mm at this scale. Used to judge the audit thresholds.
    #[must_use]
    pub fn du_to_mm(self, du: f32) -> f32 {
        if self.du_per_mm > 0.0 {
            du / self.du_per_mm
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_anchor_is_self_consistent() {
        assert!((MM_PER_DU * DU_PER_MM - 1.0).abs() < 1e-5);
    }

    #[test]
    fn identity_renders_like_today() {
        let s = Scale::identity();
        assert!((s.pixels_per_point - 1.0).abs() < 1e-6, "du = physical px");
        assert!((Dim::du(48.0).resolve(&s) - 48.0).abs() < 1e-6);
    }

    #[test]
    fn the_fallback_density_gives_exactly_one_ppp() {
        let p = ScalePolicy::default();
        let s = Scale::resolve(
            f32::NAN,
            &p,
            ScaleSource::Backend,
            ScaleConfidence::Measured,
            egui::vec2(800.0, 480.0),
        );
        assert!((s.pixels_per_point - 1.0).abs() < 1e-5);
        assert_eq!(
            s.source,
            ScaleSource::Fallback,
            "an assumption changes the source"
        );
        assert_eq!(s.confidence, ScaleConfidence::Assumed);
    }

    #[test]
    fn terms_mean_different_things() {
        let p = ScalePolicy::bare();
        // A high-density panel at 10 px/mm.
        let s = Scale::resolve(
            10.0,
            &p,
            ScaleSource::Backend,
            ScaleConfidence::Measured,
            egui::vec2(1920.0, 1080.0),
        );
        // An mm term keeps its physical length.
        let ten_mm_du = Dim::mm(10.0).resolve(&s);
        assert!((s.du_to_mm(ten_mm_du) - 10.0).abs() < 1e-3);
        // A finger term is proportional to the finger.
        let one_finger = Dim::finger(1.0).resolve(&s);
        assert!((s.du_to_mm(one_finger) - FINGER_BARE_MM).abs() < 1e-3);
    }

    #[test]
    fn a_glove_moves_every_touch_term_at_once() {
        let root = egui::vec2(800.0, 480.0);
        let bare = ScalePolicy::bare();
        let gloved = ScalePolicy::gloved();
        let sb = Scale::resolve(
            6.0,
            &bare,
            ScaleSource::Backend,
            ScaleConfidence::Measured,
            root,
        );
        let sg = Scale::resolve(
            6.0,
            &gloved,
            ScaleSource::Backend,
            ScaleConfidence::Measured,
            root,
        );
        let target = Dim::finger(1.0);
        assert!(
            target.resolve(&sg) > target.resolve(&sb) * 1.4,
            "one value grows all of it"
        );
        // What is written in du does not move — it is a value unrelated to the finger.
        assert!((Dim::du(12.0).resolve(&sb) - Dim::du(12.0).resolve(&sg)).abs() < 1e-6);
    }

    #[test]
    fn a_span_never_panics_when_the_bounds_contradict() {
        // A 3.2-inch 240×320 — the cap frac_min(0.14) = 33.6 is below the floor du(40).
        let s = Scale::resolve(
            8.0,
            &ScalePolicy::default(),
            ScaleSource::Backend,
            ScaleConfidence::Measured,
            egui::vec2(240.0, 320.0),
        );
        let span = Span::fixed(Dim::finger(1.0))
            .min(Dim::du(40.0))
            .max(Dim::frac_min(0.14));
        let v = span.resolve(&s);
        assert!(v.is_finite() && v > 0.0);
        assert!(v >= 40.0, "the floor beats the ceiling: {v}");
        assert!(span.is_inverted(&s));
    }

    #[test]
    fn a_span_without_a_max_is_unbounded() {
        let s = Scale::identity();
        assert!((Span::fixed(Dim::du(48.0)).resolve(&s) - 48.0).abs() < 1e-6);
    }

    #[test]
    fn a_non_finite_term_folds_instead_of_poisoning() {
        let mut s = Scale::identity();
        s.pixels_per_point = 0.0; // the px term goes to infinity
        let span = Span::fixed(Dim::px(1.0)).min(Dim::du(2.0));
        let v = span.resolve(&s);
        assert!(v.is_finite(), "infinity folds down to the floor: {v}");
        assert!((v - 2.0).abs() < 1e-6);
    }

    /// `0.0 / 0.0` is NaN and poisons even lengths that do not use px. This was a real bug.
    #[test]
    fn a_zero_px_term_never_divides() {
        let mut s = Scale::identity();
        s.pixels_per_point = 0.0;
        assert!(
            Dim::du(2.0).resolve(&s).is_finite(),
            "a px term of 0 is not divided by"
        );
        assert!((Dim::du(2.0).resolve(&s) - 2.0).abs() < 1e-6);
        assert!(Dim::mm(1.0).resolve(&s).is_finite());
        assert!(Dim::<Rigid>::ZERO.resolve(&s).is_finite());
    }

    #[test]
    fn validate_rejects_negatives_and_zero() {
        assert!(Span::fixed(Dim::du(-1.0)).validate("t").is_err());
        assert!(Span::fixed(Dim::<Rigid>::ZERO).validate("t").is_err());
        assert!(Span::fixed(Dim::du(1.0)).validate("t").is_ok());
    }

    #[test]
    fn ppp_never_goes_below_one() {
        // A very coarse panel at 3 px/mm.
        let s = Scale::resolve(
            3.0,
            &ScalePolicy::default(),
            ScaleSource::Backend,
            ScaleConfidence::Measured,
            egui::vec2(800.0, 480.0),
        );
        assert!(
            (s.pixels_per_point - 1.0).abs() < 1e-6,
            "du does not shrink"
        );
        // The model stays consistent after the clamp too.
        assert!((s.du_per_mm - 3.0).abs() < 1e-5);
    }

    #[test]
    fn a_dense_panel_scales_up() {
        let s = Scale::resolve(
            12.0,
            &ScalePolicy::default(),
            ScaleSource::Backend,
            ScaleConfidence::Measured,
            egui::vec2(1920.0, 1080.0),
        );
        assert!(s.pixels_per_point > 1.8, "ppp = {}", s.pixels_per_point);
        // A physical 9 mm is 9 mm on any panel.
        assert!((s.du_to_mm(Dim::mm(9.0).resolve(&s)) - 9.0).abs() < 1e-3);
    }

    #[test]
    fn container_relative_fractions_change_only_the_reference() {
        let s = Scale::identity();
        let c = s.in_container(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(200.0, 100.0),
        ));
        assert!((Dim::frac_w(0.5).resolve(&c) - 100.0).abs() < 1e-6);
        assert!((Dim::frac_min(1.0).resolve(&c) - 100.0).abs() < 1e-6);
        assert!((c.pixels_per_point - s.pixels_per_point).abs() < f32::EPSILON);
    }

    #[test]
    fn dims_add_and_scale() {
        let s = Scale::identity();
        let d = (Dim::du(2.0) + Dim::du(3.0)) * 2.0;
        assert!((d.resolve(&s) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn the_hard_floor_follows_the_finger() {
        let p = ScalePolicy::default();
        assert!((p.hard_mm() - FINGER_GLOVED_MM * 7.0 / 9.0).abs() < 1e-4);
        let p2 = ScalePolicy::default().with_finger_hard_mm(7.0);
        assert!((p2.hard_mm() - 7.0).abs() < 1e-6);
    }
}
