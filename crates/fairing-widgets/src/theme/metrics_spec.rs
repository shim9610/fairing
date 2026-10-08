//! The dimension spec in physical units.
//!
//! [`Metrics`](super::Metrics) keeps its `f32` fields — none of the 71 places inside the crate
//! that read `metrics.*` change, and no branch appears in the paint loop. What changes is **how
//! those values are obtained**: today they were logical-pixel constants, and now they are
//! [`Span`]s resolved with the frame's [`Scale`].
//!
//! # Why only these fields
//!
//! Dimensions split three ways.
//!
//! - **Those that must scale with the finger** — touch targets, bar thickness, icon cells, the
//!   dock. They use `Dim::finger`. With gloves on, they have to grow.
//! - **Those that must keep a physical size** — status bar text and icons, label text. They use
//!   `Dim::mm`. Unrelated to the finger, but they are only legible if they are the same physical
//!   size at 7 inches and at 24.
//! - **Those relative to the field of view** — corner radii, padding, stroke widths. They stay
//!   `du`. The density correction (`ppp`) is enough, and pinning them physically makes them look
//!   fussy on a large screen.
//!
//! The third group keeps the values from [`Metrics::default`](super::Metrics::default).
//!
//! # Why there are floors
//!
//! Every `Span` has a `du` floor. It stops the physical terms going absurdly small on a panel
//! whose density is unknown (`ScaleConfidence::Assumed`), and at the same time holds them
//! **close to today's render**.

use super::Metrics;
use crate::unit::{Dim, Eye, Free, Hand, Rigid, Scale, Span};
use crate::Result;

/// The dimension spec written in physical units. [`MetricsSpec::resolve`] produces a
/// [`Metrics`].
///
/// ```
/// use fairing_widgets::theme::MetricsSpec;
/// use fairing_widgets::unit::{Scale, ScalePolicy, ScaleSource, ScaleConfidence};
///
/// // A 7-inch 800×480 (154 × 86 mm) — 5.2 px/mm
/// let scale = Scale::resolve(
///     5.2, &ScalePolicy::default(),
///     ScaleSource::Backend, ScaleConfidence::Measured,
///     egui::vec2(800.0, 480.0),
/// );
/// let m = MetricsSpec::default().resolve(&scale);
/// // The default is gloved (13 mm), so the touch target is a physical 13 mm.
/// assert!((scale.du_to_mm(m.touch_target) - 13.0).abs() < 0.5);
/// ```
// No `Copy` is attached — it is 14 `Span`s, so 1,180 bytes. Passed by value, clippy's
// `large_types_passed_by_value` makes a fair point. The answer is `&self`.
///
/// **It carries no `#[non_exhaustive]`** — it is a struct the integrator builds, and with the
/// attribute `..MetricsSpec::default()` would not compile outside the crate (see the same note
/// on [`crate::unit::ScalePolicy`]).
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsSpec {
    /// The minimum touch target. This is the axis that moves with `ScalePolicy::finger_mm`.
    pub touch_target: Span<Hand>,
    /// The status bar's thickness.
    pub status_bar_height: Span<Rigid>,
    /// The nav bar's thickness.
    pub nav_bar_height: Span<Hand>,
    /// The desktop icon cell.
    pub icon_cell: Span<Hand>,
    /// The drawn size of a desktop icon.
    pub icon_size: Span<Hand>,
    /// The status bar icon size.
    pub status_icon_size: Span<Rigid>,
    /// The status bar value text size.
    pub status_text_size: Span<Rigid>,
    /// The status bar item caption text size.
    pub status_label_size: Span<Rigid>,
    /// The nav bar icon size.
    pub nav_icon_size: Span<Rigid>,
    /// The dock's thickness.
    pub dock_height: Span<Hand>,
    /// The list row height.
    pub row_height: Span<Eye>,
    /// The gap between a card and the pane's edge.
    pub screen_inset: Span<Hand>,
    /// A container's corner radius.
    pub corner_radius: Span<Eye>,
    /// A control's corner radius.
    pub control_radius: Span<Eye>,
    /// Where a row's or a card's content starts, measured in from its edge.
    ///
    /// It is here rather than left at its default because it is the token every row-like thing now
    /// shares, and a panel that scales its rows has to scale the inset with them or the text drifts
    /// towards the edge. At adoption step 1 it carries only its `du` base, so nothing moves yet.
    pub content_inset: Span<Eye>,
    /// **A card's inner padding** — the gap between a card's edge and the row inside it.
    ///
    /// It was `row_height × 0.10`, a bare multiplier inside `layout::group_with`, which put the one
    /// number the concentric-corner rule needs out of reach of the theme. The value is unchanged:
    /// `finger(0.10) + du(0.8)` is exactly that product written out.
    pub card_pad: Span<Eye>,
    /// The widget height.
    pub widget_height: Span<Eye>,
    /// The desktop label text size.
    pub desktop_label_size: Span<Rigid>,
    /// The quick-settings tile size.
    pub tile_size: Span<Hand>,
    /// The slider thumb's diameter.
    ///
    /// It is dragged with a finger, so it belongs with the touch dimensions and not with the
    /// visual ones. Fixed at 28 du it was 4.4 mm — right for the Material-sized hand these numbers
    /// were drawn for, and visibly under-scale beside a 13 mm gloved target and the text that now
    /// tracks it.
    pub slider_thumb: Span<Hand>,
    /// The notification row height.
    pub notification_row_height: Span<Hand>,
    /// The edge gesture zone's thickness.
    pub edge_px: Span<Hand>,
    /// The tallest a row of the on-screen keyboard gets, gap included. It caps `[osk]
    /// height_ratio` on a tall panel: one and a half fingers, where a fraction of the screen had
    /// made each row as big as a palm. `[osk] min_key_px` still wins if the two cross.
    ///
    /// **`None` lifts the cap**: the keyboard is the `height_ratio` share alone again, above its
    /// floor. It resolves to `f32::INFINITY` in [`Metrics::osk_max_key`].
    pub osk_max_key: Option<Span<Hand>>,
    /// The five text sizes (`small` · `body` · `button` · `heading` · `monospace`).
    ///
    /// # Why these track the viewing distance
    ///
    /// Written as multiples of [`Dim::text`](crate::unit::Dim::text), so one body em is `text(1.0)`
    /// by definition and the other four say their ratio to it out loud. The em itself comes from
    /// [`ScalePolicy::viewing_distance_mm`](crate::unit::ScalePolicy::viewing_distance_mm): text is
    /// read, and how big a thing has to be to be read is a fact about **distance**, not about hands.
    ///
    /// They used to be `Dim::finger` fractions, on the reasoning below — which was right about the
    /// problem and wrong about the cure, because there was no knob for the eye and the finger was
    /// the only physical term going. The finger was a **proxy** for the viewing distance, and like
    /// every proxy it broke the first time the two came apart: a panel read standing at a metre had
    /// to state its type scale directly, and every length still written in finger terms stopped
    /// following it.
    ///
    /// # The argument that put them on the finger, kept because half of it still holds
    ///
    /// A `du` is already a physical unit — the anchor fixes it at about 0.159 mm — so a fixed `du`
    /// text size is a fixed *millimetre* text size. It just does not move with
    /// [`ScalePolicy::finger_mm`](crate::unit::ScalePolicy::finger_mm), and while these were fixed
    /// that produced a shell **sized for two different rooms at once**: the touch targets took the
    /// default gloved finger (13 mm, an operator at arm's length) while the text stayed at 16 du =
    /// 2.54 mm, which is a phone held at 30 cm. Rows came out 14 mm tall with text filling 18 % of
    /// them, against 29 % on Material and 38 % on iOS — a screen of large empty rows with small
    /// text adrift inside them.
    ///
    /// The safety argument that makes the finger gloved by default has a reading half: assume a
    /// close viewer and meet an operator at arm's length and the text **cannot be read**. So the
    /// text moves with the same assumption the targets do.
    ///
    /// The fractions are today's values read against a Material-sized finger (48 du ≈ 7.6 mm, the
    /// hand these numbers were drawn for): 13/47.9, 16/47.9, 17/47.9, 22/47.9. At that finger the
    /// scale is unchanged; at the gloved default it grows with everything else. Each keeps a `du`
    /// floor equal to the old fixed value.
    pub type_scale: [Span<Eye>; 5],
}

impl Default for MetricsSpec {
    fn default() -> Self {
        // The floor is today's `Metrics::default()` value — it does not get smaller than today on a panel
        // of unknown density. A physical term is the value of "it takes this much to be pressed or read by a finger".
        Self {
            // **The least a finger may be asked to hit** — and nothing else.
            //
            // It was doing two jobs and only says one of them. Eleven controls took their *drawn
            // size* from it — Switch, Slider, Segmented, Checkbox, Radio, Chip, IconButton,
            // Stepper, NumberField, Dropdown, TextField — so a token named for the hand was
            // deciding how big things looked, and when the text moved for the eye the controls
            // stayed put. Measured on the kiosk example: text 2.30x, rows 2.49x, controls 1.00x.
            // The size now lives in `ControlSpec::height`, and this is the floor under it.
            touch_target: Span::fixed(Dim::finger(1.0)).min(Dim::du(48.0)),
            status_bar_height: Span::fixed(Dim::mm(7.0)).min(Dim::du(32.0)),
            nav_bar_height: Span::fixed(Dim::finger(1.0) + Dim::du(8.0)).min(Dim::du(56.0)),
            icon_cell: Span::fixed(Dim::finger(1.7)).min(Dim::du(96.0)),
            icon_size: Span::fixed(Dim::finger(0.85)).min(Dim::du(48.0)),
            status_icon_size: Span::fixed(Dim::mm(3.6)).min(Dim::du(18.0)),
            status_text_size: Span::fixed(Dim::mm(3.0)).min(Dim::du(15.0)),
            status_label_size: Span::fixed(Dim::mm(2.4)).min(Dim::du(12.0)),
            nav_icon_size: Span::fixed(Dim::mm(5.0)).min(Dim::du(24.0)),
            dock_height: Span::fixed(Dim::finger(1.7)).min(Dim::du(96.0)),
            // **A control, plus the 8 du that keeps two rows from touching.** It was
            // `finger(1.0) + du(8.0)`, which was the same number by a different road: a row's job
            // is to hold a line of text, so it has to move with the text and not with the hand.
            // Left on the finger it broke the crate's own `a_row_keeps_its_proportion_to_its_text`
            // invariant the moment the text stopped tracking the finger.
            row_height: Span::fixed(Dim::text(3.0) + Dim::du(8.0)).min(Dim::du(56.0)),
            // **Adoption step 4: the finger terms go on.** Until now each of these carried only its `du` base,
            // so the render matched what the crate shipped before the control vocabulary existed. The terms
            // below make them move with `ScalePolicy::finger_mm` like the rows and the text already do — a
            // panel whose rows scale to a gloved hand has to scale the space around them too, or the content
            // drifts towards the edges as everything else grows. Each keeps its old value as a `du` floor, so a
            // panel resolving at the floor renders exactly as it did.
            //
            // This had to come **after** the shell stopped hand-drawing its rows (step 3). With those rows
            // still reading `screen_inset` while `ListRow` read `content_inset`, turning the terms on would
            // have widened the reported four-du label gap to 6.8 rather than closing it.
            // **The inset does not scale one-for-one with the finger.** At `finger(0.333)` it
            // resolved to 18.88 du against a 64.7 du row - 0.29 - where One UI sits at 0.33 and
            // iOS at 0.36, and it read visibly tight: the label crowded the card's edge on exactly
            // the rows that are tallest. Scaling it straight off the finger is not the fix either,
            // because the row grows with the finger and the inset would grow with it: at the
            // gloved 13 mm default a proportional inset reaches 0.44 of the row, nearly half the
            // height in padding. The references hold it near-constant while the target grows, so
            // the base is mostly physical with a small finger term - 27.3 du at 9 mm and 32.0 at
            // 13 mm, which is 0.42 and 0.36 of their rows.
            // The finger term was 0.186, which is 0.557 body ems — an inset is the white space a
            // *reader* needs before the first letter, so it moves with the text. The millimetre
            // term stays: part of this gap is the glass edge, and that is a physical distance.
            content_inset: Span::fixed(Dim::mm(2.66) + Dim::text(0.557)).min(Dim::du(24.0)),
            screen_inset: Span::fixed(Dim::finger(0.25)).min(Dim::du(12.0)),
            // A corner is looked at, not pressed, and it has to keep step with `card_pad` — the
            // concentric rule is `inner = outer - gap` and the gap is now text. The fractions are
            // the old finger ones over the body's 0.334, so nothing moves.
            corner_radius: Span::fixed(Dim::text(0.7485)).min(Dim::du(12.0)),
            // **The concentric rule, as a token.** `inner = outer − gap`: a control inside a card
            // takes the card's radius less the card's padding, so the two curves share a centre and
            // the gap between them stays one thickness all the way round. Apple builds it into the
            // API (`ConcentricRectangle`, `.concentric(minimum:)`); Windows 11 ships the same idea
            // as two tiers, 8 px for containers and 4 px for in-page controls.
            //
            // card_radius = corner_radius × card_radius_ratio = finger(0.25) × 1.6 = finger(0.40),
            // card_pad    = finger(0.10) + du(0.8),
            // so concentric = finger(0.30) − du(0.8). A `Span` cannot carry the negative term, and
            // it does not need to: `finger(0.30)` lands a flat 0.8 du (0.13 mm) over concentric at
            // every finger size, and the references say a hair more than the maths reads better
            // than a hair less. `metrics_are_concentric` holds it to within 1 du.
            control_radius: Span::fixed(Dim::text(0.8982)).min(Dim::du(14.0)),
            card_pad: Span::fixed(Dim::text(0.3) + Dim::du(0.8)).min(Dim::du(5.6)),
            widget_height: Span::fixed(Dim::text(3.0) + Dim::du(8.0)).min(Dim::du(56.0)),
            desktop_label_size: Span::fixed(Dim::mm(2.7)).min(Dim::du(13.0)),
            tile_size: Span::fixed(Dim::finger(1.3)).min(Dim::du(72.0)),
            slider_thumb: Span::fixed(Dim::finger(0.585)).min(Dim::du(28.0)),
            notification_row_height: Span::fixed(Dim::finger(1.0) + Dim::du(10.0))
                .min(Dim::du(72.0)),
            edge_px: Span::fixed(Dim::finger(0.5)).min(Dim::du(24.0)),
            osk_max_key: Some(Span::fixed(Dim::finger(1.5)).min(Dim::du(72.0))),
            // **Text tracks the finger, like the rows do.** See the field's own docs for why the
            // fractions are what they are; the `du` floor is exactly the old fixed value, so a panel
            // of unknown density renders identically to before.
            // **Adoption step 5: the text hangs off the eye, not the hand.** Each of these was
            // `Dim::finger(k)` — 0.271 / 0.334 / 0.355 / 0.459 — because `ScalePolicy` had no knob
            // for the viewing distance and the finger was the only physical term going. Divided
            // through by the body's own 0.334 those five numbers are 0.811 / 1 / 1.063 / 1.374 /
            // 1, which is what they are written as here, so **the value is unchanged at the
            // default policy** and the scale's own shape is now legible in the table.
            type_scale: [
                Span::fixed(Dim::text(0.811)).min(Dim::du(13.0)),
                Span::fixed(Dim::text(1.0)).min(Dim::du(16.0)),
                Span::fixed(Dim::text(1.063)).min(Dim::du(17.0)),
                Span::fixed(Dim::text(1.374)).min(Dim::du(22.0)),
                Span::fixed(Dim::text(1.0)).min(Dim::du(16.0)),
            ],
        }
    }
}

/// **The largest a text token may resolve to**, in du at the base scale.
///
/// Derived rather than chosen. epaint refuses an atlas narrower than 1024 px
/// (`TextureAtlas::new`'s own assert), so 1024 is the narrowest atlas that can exist, and
/// `TextureAtlas::allocate` asserts every glyph fits the atlas's width. This crate clamps
/// `pixels_per_point` to `ScalePolicy::ppp_range`, 8.0 at the top by default, and a glyph is at
/// most about its font size wide. 1024 / 8 = 128 du is therefore the size that is still safe at the
/// worst permitted density — and 128 du is around 20 mm of type, far past any real interface.
///
/// Raising `ppp_range` past 8.0 lowers the real ceiling proportionally; the check here stays at the
/// default policy's, which is the one a spec is written against.
const TEXT_CEILING_DU: f32 = 128.0;

impl MetricsSpec {
    /// The spec that reproduces today's render exactly. Every term is `du`, so it is
    /// scale-independent.
    ///
    /// It is the switch for telling "is this the dimensions or not" apart during the migration —
    /// with it, the result has to be pixel-identical to before M2b.
    #[must_use]
    pub fn legacy_du() -> Self {
        Self::from_metrics(&Metrics::default())
    }

    /// The spec that reproduces **these** metrics exactly, every term in `du`.
    ///
    /// `fairing_widgets::Shell::metrics_spec_mut` seeds itself with this where the
    /// shell was built without a spec, so a run-time change has a source of truth to be written on
    /// rather than being overwritten by the next frame's resolve.
    #[must_use]
    pub fn from_metrics(d: &Metrics) -> Self {
        // **Every one of these is `pinned`, and that is the point of the function.** It exists to
        // reproduce a `Metrics` term for term, so each token has to be the number it already is
        // rather than a proportion that would re-derive it — which is exactly the deliberate,
        // spelled-out constant `Span::pinned` is for. Written without it this would not
        // compile, and that is the design working: an all-`du` spec is a legitimate thing to want
        // and an accidental thing to produce, and the two have to look different.
        let f = |v: f32| Span::fixed(Dim::du(v));
        Self {
            touch_target: f(d.touch_target).pinned(),
            status_bar_height: f(d.status_bar_height).pinned(),
            nav_bar_height: f(d.nav_bar_height).pinned(),
            icon_cell: f(d.icon_cell).pinned(),
            icon_size: f(d.icon_size).pinned(),
            status_icon_size: f(d.status_icon_size).pinned(),
            status_text_size: f(d.status_text_size).pinned(),
            status_label_size: f(d.status_label_size).pinned(),
            nav_icon_size: f(d.nav_icon_size).pinned(),
            dock_height: f(d.dock_height).pinned(),
            row_height: f(d.row_height).pinned(),
            content_inset: f(d.content_inset).pinned(),
            card_pad: f(d.card_pad).pinned(),
            screen_inset: f(d.screen_inset).pinned(),
            corner_radius: f(d.corner_radius).pinned(),
            control_radius: f(d.control_radius).pinned(),
            widget_height: f(d.widget_height).pinned(),
            desktop_label_size: f(d.desktop_label_size).pinned(),
            tile_size: f(d.tile_size).pinned(),
            slider_thumb: f(d.slider_thumb).pinned(),
            notification_row_height: f(d.notification_row_height).pinned(),
            edge_px: f(d.edge_px).pinned(),
            // No cap stays no cap: pinned, an infinite length would fold to 0 (`Span::resolve`).
            osk_max_key: d.osk_max_key.is_finite().then(|| f(d.osk_max_key).pinned()),
            type_scale: [
                f(d.type_scale.small).pinned(),
                f(d.type_scale.body).pinned(),
                f(d.type_scale.button).pinned(),
                f(d.type_scale.heading).pinned(),
                f(d.type_scale.monospace).pinned(),
            ],
        }
    }

    /// The [`Metrics`] resolved at this scale. Fields the spec does not cover keep their defaults.
    #[must_use]
    pub fn resolve(&self, s: &Scale) -> Metrics {
        Metrics {
            touch_target: self.touch_target.resolve(s),
            status_bar_height: self.status_bar_height.resolve(s),
            nav_bar_height: self.nav_bar_height.resolve(s),
            icon_cell: self.icon_cell.resolve(s),
            icon_size: self.icon_size.resolve(s),
            status_icon_size: self.status_icon_size.resolve(s),
            status_text_size: self.status_text_size.resolve(s),
            status_label_size: self.status_label_size.resolve(s),
            nav_icon_size: self.nav_icon_size.resolve(s),
            dock_height: self.dock_height.resolve(s),
            row_height: self.row_height.resolve(s),
            content_inset: self.content_inset.resolve(s),
            card_pad: self.card_pad.resolve(s),
            screen_inset: self.screen_inset.resolve(s),
            corner_radius: self.corner_radius.resolve(s),
            control_radius: self.control_radius.resolve(s),
            widget_height: self.widget_height.resolve(s),
            desktop_label_size: self.desktop_label_size.resolve(s),
            tile_size: self.tile_size.resolve(s),
            slider_thumb: self.slider_thumb.resolve(s),
            notification_row_height: self.notification_row_height.resolve(s),
            edge_px: self.edge_px.resolve(s),
            osk_max_key: self.osk_max_key.map_or(f32::INFINITY, |cap| cap.resolve(s)),
            type_scale: crate::theme::TypeScale {
                small: self.type_scale[0].resolve(s),
                body: self.type_scale[1].resolve(s),
                button: self.type_scale[2].resolve(s),
                heading: self.type_scale[3].resolve(s),
                monospace: self.type_scale[4].resolve(s),
            },
            ..Metrics::default()
        }
    }

    /// Whether every `Span` has no negative term and resolves positive at the reference scale.
    ///
    /// # Errors
    /// [`crate::Error::Config`] on any violation — the message names the token.
    pub fn validate(&self) -> Result<()> {
        let all: [(&str, Span<Free>); 27] = [
            ("touch_target", self.touch_target.erased()),
            ("status_bar_height", self.status_bar_height.erased()),
            ("nav_bar_height", self.nav_bar_height.erased()),
            ("icon_cell", self.icon_cell.erased()),
            ("icon_size", self.icon_size.erased()),
            ("status_icon_size", self.status_icon_size.erased()),
            ("status_text_size", self.status_text_size.erased()),
            ("status_label_size", self.status_label_size.erased()),
            ("nav_icon_size", self.nav_icon_size.erased()),
            ("dock_height", self.dock_height.erased()),
            ("row_height", self.row_height.erased()),
            ("content_inset", self.content_inset.erased()),
            ("card_pad", self.card_pad.erased()),
            ("screen_inset", self.screen_inset.erased()),
            ("corner_radius", self.corner_radius.erased()),
            ("control_radius", self.control_radius.erased()),
            ("widget_height", self.widget_height.erased()),
            ("desktop_label_size", self.desktop_label_size.erased()),
            ("tile_size", self.tile_size.erased()),
            ("slider_thumb", self.slider_thumb.erased()),
            (
                "notification_row_height",
                self.notification_row_height.erased(),
            ),
            ("edge_px", self.edge_px.erased()),
            ("type_scale.small", self.type_scale[0].erased()),
            ("type_scale.body", self.type_scale[1].erased()),
            ("type_scale.button", self.type_scale[2].erased()),
            ("type_scale.heading", self.type_scale[3].erased()),
            ("type_scale.monospace", self.type_scale[4].erased()),
        ];
        if let Some(cap) = self.osk_max_key {
            cap.validate("osk_max_key")?;
        }
        for (name, span) in all {
            span.validate(name)?;
            // **A text token also has a ceiling**, because past it epaint stops being a library and
            // becomes an abort: `TextureAtlas::allocate` asserts a glyph is no wider than the
            // atlas, and a mis-typed `type_scale` is the ordinary way to get there.
            if name.starts_with("type_scale") || name.ends_with("text_size") {
                let size = span.resolve(&Scale::identity());
                if size > TEXT_CEILING_DU {
                    return Err(crate::Error::Config(format!(
                        "{name} resolves to {size} du at the base scale, above the                          {TEXT_CEILING_DU} du ceiling — a glyph that size cannot be allocated in                          the font atlas and epaint would abort rather than report it"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unit::{
        ScaleConfidence, ScalePolicy, ScaleSource, FINGER_BARE_MM, FINGER_GLOVED_MM,
    };

    fn scale_for(px_per_mm: f32, finger_mm: f32, size: egui::Vec2) -> Scale {
        Scale::resolve(
            px_per_mm,
            &ScalePolicy::default().with_finger_mm(finger_mm),
            ScaleSource::Backend,
            ScaleConfidence::Measured,
            size,
        )
    }

    /// **A mis-typed text token is an error from `build`, not an abort inside epaint.**
    ///
    /// Without the ceiling this spec validates cleanly and the process dies later, in
    /// `TextureAtlas::allocate`, with a message about texture atlases that names nothing the
    /// integrator wrote.
    #[test]
    fn a_text_token_past_the_atlas_ceiling_is_reported() {
        let spec = MetricsSpec {
            type_scale: [
                Span::fixed(Dim::du(12.0)).pinned(),
                Span::fixed(Dim::du(2000.0)).pinned(),
                Span::fixed(Dim::du(14.0)).pinned(),
                Span::fixed(Dim::du(20.0)).pinned(),
                Span::fixed(Dim::du(13.0)).pinned(),
            ],
            ..MetricsSpec::default()
        };
        let Err(err) = spec.validate() else {
            unreachable!("2000 du of type has to be refused before anything tries to draw it")
        };
        let text = err.to_string();
        assert!(
            text.contains("type_scale.body") && text.contains("128"),
            "the message has to name the token and the ceiling: {text}"
        );
        // And the ceiling is not so low that ordinary type trips it.
        assert!(
            MetricsSpec {
                type_scale: [
                    Span::fixed(Dim::du(12.0)).pinned(),
                    Span::fixed(Dim::du(64.0)).pinned(),
                    Span::fixed(Dim::du(14.0)).pinned(),
                    Span::fixed(Dim::du(96.0)).pinned(),
                    Span::fixed(Dim::du(13.0)).pinned(),
                ],
                ..MetricsSpec::default()
            }
            .validate()
            .is_ok(),
            "96 du of heading is large but legitimate on a big panel"
        );
    }

    #[test]
    fn the_default_spec_is_valid() -> Result<()> {
        MetricsSpec::default().validate()?;
        MetricsSpec::legacy_du().validate()
    }

    /// The switch that reproduces today's render gives today's values whatever the scale.
    #[test]
    fn legacy_du_reproduces_todays_metrics() {
        let d = Metrics::default();
        for ppmm in [3.0, 5.2, 6.299_213, 12.0] {
            let s = scale_for(ppmm, FINGER_GLOVED_MM, egui::vec2(800.0, 480.0));
            let m = MetricsSpec::legacy_du().resolve(&s);
            assert!((m.touch_target - d.touch_target).abs() < 1e-3, "{ppmm}");
            assert!((m.icon_cell - d.icon_cell).abs() < 1e-3, "{ppmm}");
            assert!(
                (m.status_bar_height - d.status_bar_height).abs() < 1e-3,
                "{ppmm}"
            );
        }
    }

    /// The touch target is **the same physical size** on a 7-inch 800×480 and a 15.6-inch
    /// 1920×1080. Today both are 48 logical px, so the physical sizes differ by more than 2×.
    #[test]
    fn the_touch_target_is_the_same_physical_size_on_both_panels() {
        // 7" 800×480 = 154 × 86 mm → 5.19 px/mm
        let small = scale_for(800.0 / 154.0, FINGER_GLOVED_MM, egui::vec2(800.0, 480.0));
        // 15.6" 1920×1080 = 345 × 194 mm → 5.57 px/mm
        let big = scale_for(1920.0 / 345.0, FINGER_GLOVED_MM, egui::vec2(1920.0, 1080.0));

        let ms = MetricsSpec::default();
        let a = ms.resolve(&small);
        let b = ms.resolve(&big);
        let a_mm = small.du_to_mm(a.touch_target);
        let b_mm = big.du_to_mm(b.touch_target);
        assert!((a_mm - FINGER_GLOVED_MM).abs() < 0.5, "7 inch {a_mm} mm");
        assert!((b_mm - FINGER_GLOVED_MM).abs() < 0.5, "15.6 inch {b_mm} mm");

        // Why today's values are a problem is pinned down with it.
        let today = Metrics::default().touch_target;
        assert!(
            small.du_to_mm(today) < 10.0,
            "today's 48 du is {} mm on a 7 inch",
            small.du_to_mm(today)
        );
    }

    /// One gloved value moves the whole touch family and leaves the physical text sizes alone.
    #[test]
    fn one_value_moves_the_touch_family_and_leaves_text_alone() {
        let size = egui::vec2(800.0, 480.0);
        let bare = scale_for(5.2, FINGER_BARE_MM, size);
        let gloved = scale_for(5.2, FINGER_GLOVED_MM, size);
        let ms = MetricsSpec::default();
        let b = ms.resolve(&bare);
        let g = ms.resolve(&gloved);

        for (name, x, y) in [
            ("touch_target", b.touch_target, g.touch_target),
            ("icon_cell", b.icon_cell, g.icon_cell),
            ("nav_bar_height", b.nav_bar_height, g.nav_bar_height),
            ("dock_height", b.dock_height, g.dock_height),
        ] {
            assert!(y > x, "{name}: a glove has to be bigger ({x} → {y})");
        }
        assert!(
            (b.desktop_label_size - g.desktop_label_size).abs() < 1e-3,
            "text has nothing to do with the finger"
        );
        assert!((b.status_bar_height - g.status_bar_height).abs() < 1e-3);
    }

    /// With an unknown density the floors hold it, so it never goes below today.
    #[test]
    fn an_unknown_panel_never_shrinks_below_today() {
        let s = Scale::resolve(
            f32::NAN,
            &ScalePolicy::default(),
            ScaleSource::Backend,
            ScaleConfidence::Measured,
            egui::vec2(800.0, 480.0),
        );
        let m = MetricsSpec::default().resolve(&s);
        let d = Metrics::default();
        assert!(m.touch_target >= d.touch_target);
        assert!(m.icon_cell >= d.icon_cell);
        assert!(m.status_bar_height >= d.status_bar_height);
    }

    /// Even a tiny panel resolves finite and positive without panicking.
    #[test]
    fn tiny_panels_resolve_without_panicking() {
        for (w, h) in [
            (240.0, 320.0),
            (320.0, 240.0),
            (480.0, 272.0),
            (3840.0, 160.0),
        ] {
            for finger in [7.0, 9.0, 13.0] {
                let s = scale_for(8.0, finger, egui::vec2(w, h));
                let m = MetricsSpec::default().resolve(&s);
                for v in [m.touch_target, m.icon_cell, m.status_bar_height, m.edge_px] {
                    assert!(v.is_finite() && v > 0.0, "{w}×{h} finger={finger}: {v}");
                }
            }
        }
    }
}

#[cfg(test)]
mod scale_tests {
    use super::MetricsSpec;
    use crate::theme::ControlSpec;
    use crate::unit::{Scale, ScaleConfidence, ScalePolicy, ScaleSource};

    /// Resolve both specs at one panel, and hand back the scale so a length can be read in mm.
    fn at(finger_mm: f32, px_per_mm: f32) -> (super::Metrics, crate::theme::ControlMetrics, Scale) {
        let s = Scale::resolve(
            px_per_mm,
            &ScalePolicy::default().with_finger_mm(finger_mm),
            ScaleSource::Config,
            ScaleConfidence::Declared,
            egui::vec2(1024.0, 600.0),
        );
        (
            MetricsSpec::default().resolve(&s),
            ControlSpec::default().resolve(&s),
            s,
        )
    }

    /// **The same physical size at any pixel density.** `du` is a physical unit, so a panel that
    /// packs twice the pixels into the same millimetres must resolve every token to the same `du`.
    /// A token that had drifted into pixels would show up here and nowhere else.
    /// **In millimetres**, not in `du`. Below about 6.3 px/mm the `pixels_per_point` floor of 1.0
    /// starts to stretch what one `du` is worth - a `Dim::px(1)` hairline may never resolve under
    /// one real pixel - so the `du` numbers legitimately differ there while the physical sizes do
    /// not. Comparing `du` across densities would call that a regression; comparing the millimetre
    /// each one resolves to is the invariant that actually matters.
    #[test]
    fn density_does_not_change_a_single_length() {
        for finger in [6.0_f32, 9.0, 13.0, 16.0] {
            let (a, ca, sa) = at(finger, 6.67);
            let (b, cb, sb) = at(finger, 20.0);
            for (name, x, y) in [
                ("row_height", a.row_height, b.row_height),
                ("touch_target", a.touch_target, b.touch_target),
                ("content_inset", a.content_inset, b.content_inset),
                ("body", a.type_scale.body, b.type_scale.body),
                ("mark_size", ca.mark_size, cb.mark_size),
                ("box_size", ca.box_size(), cb.box_size()),
                ("stroke_edge", ca.stroke_edge, cb.stroke_edge),
                ("stroke_mark", ca.stroke_mark, cb.stroke_mark),
                ("focus_gap", ca.focus_gap, cb.focus_gap),
            ] {
                let (mm_a, mm_b) = (x / sa.du_per_mm, y / sb.du_per_mm);
                assert!(
                    (mm_a - mm_b).abs() < 0.01,
                    "{name} moved with the density at finger {finger}: \
                     {mm_a:.3} mm at 6.67 px/mm, {mm_b:.3} mm at 20 px/mm"
                );
            }
        }
    }

    /// **The row and the text it holds grow together.** Both derive from `finger_mm`, so the ratio
    /// between them has to stay put across the whole range a device might declare - otherwise a
    /// gloved panel would get roomy rows and a bare-fingered one crowded ones, or the reverse.
    #[test]
    fn a_row_keeps_its_proportion_to_its_text() {
        let mut seen = Vec::new();
        for finger in [6.0_f32, 9.0, 11.0, 13.0, 16.0] {
            let (m, c, _) = at(finger, 6.67);
            seen.push((
                finger,
                m.row_height / m.type_scale.body,
                m.row_height / c.mark_size,
                m.content_inset / m.row_height,
            ));
        }
        for &(finger, row_text, row_mark, inset_row) in &seen {
            assert!(
                (3.1..=3.6).contains(&row_text),
                "finger {finger}: row/body is {row_text:.2}, outside 3.1..3.6"
            );
            assert!(
                (1.7..=2.0).contains(&row_mark),
                "finger {finger}: row/mark is {row_mark:.2}, outside 1.7..2.0"
            );
            assert!(
                (0.30..=0.45).contains(&inset_row),
                "finger {finger}: inset/row is {inset_row:.2}, outside 0.30..0.45"
            );
        }
    }

    /// **The corners nest concentrically.** `inner = outer − gap`: a control inside a card takes
    /// the card's radius less the card's padding, so the two curves share a centre point and the
    /// gap between them keeps one thickness the whole way round. Apple ships the rule as an API
    /// (`ConcentricRectangle`, `.concentric(minimum:)`); Windows 11 ships it as two tiers.
    ///
    /// The tolerance is one `du`: a `Span` cannot carry the negative term the exact expression
    /// needs, so the shipped `finger(0.30)` sits a flat 0.8 du over concentric at every finger
    /// size - the side the references say to err on.
    ///
    /// What this catches is what it was written for: the card radius and the control radius were
    /// two unrelated numbers in two files, the layout multiplying `corner_radius` by 2.8 of its own
    /// while the control token stayed at 0.125 of the finger. They came out 5.6x apart - a pill
    /// container holding square children.
    #[test]
    fn metrics_are_concentric() {
        for finger in [6.0_f32, 9.0, 13.0, 16.0] {
            for px_per_mm in [2.0_f32, 6.67, 20.0] {
                let (m, c, _) = at(finger, px_per_mm);
                let card = crate::theme::card_radius(&m, &c);
                let want = card - m.card_pad;
                let got = m.control_radius;
                assert!(
                    (got - want).abs() <= 1.0,
                    "finger {finger} at {px_per_mm} px/mm: card {card:.2} − pad {:.2} wants a \
                     control radius of {want:.2}, got {got:.2}",
                    m.card_pad
                );
            }
        }
    }

    /// A panel too coarse to render the finger's own size falls back to the `du` floors, and the
    /// floors are a **consistent set** - not one token at its minimum beside another still scaling.
    #[test]
    fn the_low_density_floors_are_one_consistent_set() {
        let (m, c, _) = at(13.0, 2.0);
        assert!(
            (m.row_height - 56.0).abs() < 0.01,
            "row_height {}",
            m.row_height
        );
        assert!(
            (m.touch_target - 48.0).abs() < 0.01,
            "touch {}",
            m.touch_target
        );
        assert!(
            (m.content_inset - 24.0).abs() < 0.01,
            "inset {}",
            m.content_inset
        );
        assert!((c.mark_size - 28.8).abs() < 0.01, "mark {}", c.mark_size);
        let row_text = m.row_height / m.type_scale.body;
        assert!(
            (3.1..=3.6).contains(&row_text),
            "at the floors the row/body ratio is {row_text:.2}"
        );
    }
}
