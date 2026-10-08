//! **The component tokens** — the twenty visual dimensions of the widgets and notifications.
//!
//! # Why they are kept apart from the global [`Metrics`](super::Metrics)
//!
//! The global tokens are **what the shell divides the screen with**: bar thickness, touch
//! targets, row heights. These twenty are what one widget draws itself with, inside that. In one
//! bucket, `Metrics` would pass sixty fields and "does touching this move the layout" would not
//! be answerable from the name.
//!
//! # Why they were promoted from constants
//!
//! Until they were, these twenty were `const`s inside their files. That leaves **rung 2 (the
//! tokens) of the override ladder shut for the whole widget area** — an
//! integrator changing one button's padding had to climb to rung 5 (the painter) and take over
//! the drawing. A cliff.
//!
//! The values are held as [`Span`]s by [`ComponentSpec`] and resolved with a [`Scale`] each
//! frame. So a device that knows its physical size can write them in mm or finger multiples, and
//! one that does not gets exactly today's values.
//!
//! # What does not come here
//!
//! **Purely visual ratios** like `THUMB_SCALE` and `PRESS_SWELL`, and **times and velocities**
//! like `POP_MS` and `FLING_UP`, do not. The former derive from other tokens and the latter
//! belong to `[motion]`.

use crate::unit::{Dim, Free, Hand, Rigid, Scale, Span};
use crate::Result;

/// The toast dimensions (6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToastMetrics {
    /// How far it rises from below on the way in.
    pub enter_offset: f32,
    /// How far each stacked toast steps back.
    pub lift_step: f32,
    /// The vertical gap between toasts.
    pub stack_gap: f32,
    /// The side padding.
    pub pad_x: f32,
    /// Between the icon and the text.
    pub icon_gap: f32,
    /// The accent stripe's thickness on the left.
    pub accent_w: f32,
}

/// The heads-up dimensions (3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadsUpMetrics {
    /// The padding on every side.
    pub pad: f32,
    /// Between the icon and the text.
    pub icon_gap: f32,
    /// The progress bar's thickness.
    pub progress_h: f32,
}

/// The button dimensions (4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonMetrics {
    /// The side padding.
    pub pad: f32,
    /// Between the icon and the label.
    pub icon_gap: f32,
    /// The long-press progress ring's diameter.
    pub ring: f32,
    /// That ring's stroke width.
    pub ring_stroke: f32,
}

/// The slider dimensions (2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderMetrics {
    /// Between the label and the track.
    pub label_gap: f32,
    /// The track thickness = `metrics.slider_thumb × this ratio`. A **ratio**
    /// rather than a length, so it is not a [`Span`] and does not go through scale resolution —
    /// a bigger thumb has to bring the track with it.
    pub track_ratio: f32,
}

/// The switch dimensions (2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwitchMetrics {
    /// The padding between the knob and the track.
    pub knob_inset: f32,
    /// The **drawn** height. The hit rect keeps the touch target independently of it.
    pub height: f32,
}

/// The list row dimensions (3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListRowMetrics {
    /// The gap **between** a row's parts - after the leading icon, and before each trailing item.
    ///
    /// Not the side padding any more. That is `metrics.content_inset`, the one token for where
    /// content starts inside a container; this held a second copy of the same 16 du, so raising the
    /// layout token moved the card, the note and the title and left every row's label exactly where
    /// it was.
    pub pad: f32,
    /// How far the title and subtitle separate from the centre on two lines.
    pub two_line_offset: f32,
    /// The width the chevron takes.
    pub chevron_w: f32,
}

/// The overview's dimensions (2) — its cards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverviewMetrics {
    /// The gap between two cards.
    pub card_gap: f32,
    /// **The narrowest a card is drawn.** A card is `0.6 ×` the content (A10); on a panel
    /// small enough for that to fall under this, the cards — and the screen shrinking into one —
    /// stay this wide instead, so what a finger taps and throws stays a touch target's family.
    pub card_min_width: f32,
}

/// The info popover's dimensions (3) — the card a long press brings up over a desktop icon.
/// Its padding, corner and type are the global tokens a card uses
/// (`content_inset`, `corner_radius`, the type scale); these are what only a popover has.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopoverMetrics {
    /// **The widest the card is drawn** — a toast's widest, so the two floating cards keep one
    /// measure. A description wraps to it; on a panel narrower than this plus the screen inset
    /// either side, the card takes the width there is.
    pub max_width: f32,
    /// The caret's height — how far the card stands off the icon it points at. Its base is twice
    /// this.
    pub caret: f32,
    /// The gap between an icon and the text beside it, in the title row and the level row.
    pub icon_gap: f32,
}

/// The shade dimensions (9) — **all of them ratios**.
///
/// The shade does not carry lengths; it **derives them from two axes**, so that the rhythm
/// follows on its own when the density changes — no case of the icons growing on a 7-inch panel
/// while the padding stays at 12 du.
///
/// * **The visual axis** — multiples of `metrics.corner_radius`. Padding and corners have
///   nothing to do with fingers.
/// * **The finger axis** — multiples of `metrics.touch_target`. What gets pressed follows the
///   finger.
///
/// The four on the finger axis are **drawing sizes**. The hit rect is a full `touch_target`, so
/// shrinking these does not shrink what a finger can land on — what made `switch.height`
/// dangerous was that the hit shrank with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadeMetrics {
    /// The panel's edge padding (× `corner_radius`).
    pub pad: f32,
    /// Between blocks — the tile row, the list, the footer (× `corner_radius`).
    pub gap: f32,
    /// The side padding inside a card (× `corner_radius`).
    pub card_pad: f32,
    /// The vertical gap between cards (× `corner_radius`).
    pub card_gap: f32,
    /// The corner radius of cards and pills (× `corner_radius`).
    pub radius: f32,
    /// The diameter of a quick-settings tile's round toggle (× `touch_target`).
    pub tile_puck: f32,
    /// The diameter of a notification row's icon disc (× `touch_target`).
    pub note_icon: f32,
    /// The diameter of a notification's dismiss button (× `touch_target`).
    pub close: f32,
    /// The footer button's height (× `touch_target`).
    pub footer_button: f32,
    /// **The card's corner radius** (× `corner_radius`) — `[overlay] reveal = "card"`.
    ///
    /// Measured off One UI 8's quick panel: a radius of 9 % of the card's width, which at the
    /// default width is four `corner_radius`. A curtain rounds its two bottom corners with
    /// `metrics.shade_corner`; a card rounds all four with this, and larger, because it is a plate
    /// floating on the page rather than a sheet hanging from its top edge.
    pub card_corner: f32,
    /// **The card's inner padding** (× `corner_radius`), on top of `pad`.
    ///
    /// One UI pads its card at 6 % of the card's width where the curtain pads at 3 %; the content
    /// has to sit clear of a corner that large, and a card reads as a card by the air around what
    /// it holds.
    pub card_inset: f32,
    /// **How far above its rest a card arrives from** (× `corner_radius`).
    ///
    /// One UI's panel comes down 4–6 % of the screen while it fades in — enough to give the arrival
    /// a direction, not enough to be a journey; four `corner_radius` is 5.6 % of the console
    /// example's 800 px. The drop is what makes a fade read as "came from above" rather than
    /// "appeared". Under a visible status bar the card comes out from beneath it.
    pub card_drop: f32,
}

impl Default for ShadeMetrics {
    /// Exactly the multiples `PanelStyle` uses today.
    fn default() -> Self {
        Self {
            pad: 1.34,
            gap: 1.0,
            card_pad: 1.17,
            card_gap: 0.67,
            radius: 1.34,
            tile_puck: 0.8,
            note_icon: 0.62,
            close: 0.72,
            footer_button: 0.82,
            card_corner: 4.0,
            card_inset: 2.68,
            card_drop: 4.0,
        }
    }
}

/// The resolved component tokens (= `Theme::components`). Everything but the ones written as ratios is in du.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComponentMetrics {
    /// Toasts.
    pub toast: ToastMetrics,
    /// Heads-up.
    pub heads_up: HeadsUpMetrics,
    /// Buttons.
    pub button: ButtonMetrics,
    /// Sliders.
    pub slider: SliderMetrics,
    /// Switches.
    pub switch: SwitchMetrics,
    /// List rows.
    pub list_row: ListRowMetrics,
    /// The overview's cards.
    pub overview: OverviewMetrics,
    /// The desktop icon's info popover.
    pub popover: PopoverMetrics,
    /// The shade — all ratios.
    pub shade: ShadeMetrics,
}

impl Default for ComponentMetrics {
    /// Exactly today's values.
    fn default() -> Self {
        ComponentSpec::default().resolve(&Scale::identity())
    }
}

/// The component spec in physical units. Resolved each frame with a [`Scale`] into [`ComponentMetrics`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComponentSpec {
    /// Toasts — `enter_offset` · `lift_step` · `stack_gap` · `pad_x` · `icon_gap` · `accent_w`.
    pub toast: [Span<Rigid>; 6],
    /// Heads-up — `pad` · `icon_gap` · `progress_h`.
    pub heads_up: [Span<Rigid>; 3],
    /// Buttons — `pad` · `icon_gap` · `ring` · `ring_stroke`.
    pub button: [Span<Rigid>; 4],
    /// Sliders — `label_gap`.
    pub slider_label_gap: Span<Rigid>,
    /// The slider track ratio (a multiplier rather than a length, so not a `Span`).
    pub slider_track_ratio: f32,
    /// Switches — `knob_inset` · `height`.
    pub switch: [Span<Hand>; 2],
    /// List rows — `pad` (the gap between parts, not the side inset) · `two_line_offset` · `chevron_w`.
    pub list_row: [Span<Rigid>; 3],
    /// The overview — `card_gap` · `card_min_width`.
    pub overview: [Span<Hand>; 2],
    /// The info popover — `max_width` · `caret` · `icon_gap`.
    pub popover: [Span<Rigid>; 3],
    /// The shade — multiples rather than lengths, so not a `Span` and not scale-resolved.
    pub shade: ShadeMetrics,
}

/// A spec of one `du`.
const fn du(v: f32) -> Span<Rigid> {
    Span::fixed(Dim::du(v))
}

impl Default for ComponentSpec {
    /// The default values. **Pixel-identical to today's render** — they are all
    /// `du`, so scale-independent, and only `switch.height` has a finger term (see the comment
    /// below).
    fn default() -> Self {
        Self {
            toast: [du(24.0), du(8.0), du(16.0), du(16.0), du(12.0), du(3.0)],
            heads_up: [du(16.0), du(12.0), du(4.0)],
            button: [du(12.0), du(8.0), du(24.0), du(3.0)],
            slider_label_gap: du(8.0),
            slider_track_ratio: 0.64,
            switch: [
                du(3.0).pinned(),
                // Today it is `touch_target × 0.6`. `touch_target` being `max(finger(1.0), du(48))`,
                // **`finger(0.6)` with a `du(28.8)` floor** reproduces that value at any finger width —
                // 49.1 with a 13 mm glove, 34.0 bare-fingered at 9 mm, and 28.8 where the finger is small
                // enough for the du floor to bite. A bare `du(28.8)` is only the third case's
                // value, and using only that would suddenly shrink the switch
                // on a gloved device.
                Span::fixed(Dim::finger(0.6)).min(Dim::du(28.8)),
            ],
            list_row: [du(16.0), du(11.0), du(20.0)],
            overview: [
                du(24.0).pinned(),
                // Three touch targets: `touch_target` is `max(finger(1.0), du(48))`.
                Span::fixed(Dim::finger(3.0)).min(Dim::du(144.0)),
            ],
            popover: [du(420.0), du(8.0), du(12.0)],
            shade: ShadeMetrics::default(),
        }
    }
}

impl ComponentSpec {
    /// Resolve at this scale.
    #[must_use]
    pub fn resolve(&self, s: &Scale) -> ComponentMetrics {
        ComponentMetrics {
            toast: ToastMetrics {
                enter_offset: self.toast[0].resolve(s),
                lift_step: self.toast[1].resolve(s),
                stack_gap: self.toast[2].resolve(s),
                pad_x: self.toast[3].resolve(s),
                icon_gap: self.toast[4].resolve(s),
                accent_w: self.toast[5].resolve(s),
            },
            heads_up: HeadsUpMetrics {
                pad: self.heads_up[0].resolve(s),
                icon_gap: self.heads_up[1].resolve(s),
                progress_h: self.heads_up[2].resolve(s),
            },
            button: ButtonMetrics {
                pad: self.button[0].resolve(s),
                icon_gap: self.button[1].resolve(s),
                ring: self.button[2].resolve(s),
                ring_stroke: self.button[3].resolve(s),
            },
            slider: SliderMetrics {
                label_gap: self.slider_label_gap.resolve(s),
                track_ratio: self.slider_track_ratio.max(0.0),
            },
            switch: SwitchMetrics {
                knob_inset: self.switch[0].resolve(s),
                height: self.switch[1].resolve(s),
            },
            list_row: ListRowMetrics {
                pad: self.list_row[0].resolve(s),
                two_line_offset: self.list_row[1].resolve(s),
                chevron_w: self.list_row[2].resolve(s),
            },
            overview: OverviewMetrics {
                card_gap: self.overview[0].resolve(s),
                card_min_width: self.overview[1].resolve(s),
            },
            popover: PopoverMetrics {
                max_width: self.popover[0].resolve(s),
                caret: self.popover[1].resolve(s),
                icon_gap: self.popover[2].resolve(s),
            },
            shade: self.shade,
        }
    }

    /// Whether every length resolves to a finite positive at the reference scale.
    ///
    /// # Errors
    /// [`crate::Error::Config`] on any violation — the message names the token.
    pub fn validate(&self) -> Result<()> {
        let s = Scale::identity();
        let named: [(&str, Span<Free>); 24] = [
            ("toast.enter_offset", self.toast[0].erased()),
            ("toast.lift_step", self.toast[1].erased()),
            ("toast.stack_gap", self.toast[2].erased()),
            ("toast.pad_x", self.toast[3].erased()),
            ("toast.icon_gap", self.toast[4].erased()),
            ("toast.accent_w", self.toast[5].erased()),
            ("heads_up.pad", self.heads_up[0].erased()),
            ("heads_up.icon_gap", self.heads_up[1].erased()),
            ("heads_up.progress_h", self.heads_up[2].erased()),
            ("button.pad", self.button[0].erased()),
            ("button.icon_gap", self.button[1].erased()),
            ("button.ring", self.button[2].erased()),
            ("button.ring_stroke", self.button[3].erased()),
            ("slider.label_gap", self.slider_label_gap.erased()),
            ("switch.knob_inset", self.switch[0].erased()),
            ("switch.height", self.switch[1].erased()),
            ("list_row.pad", self.list_row[0].erased()),
            ("list_row.two_line_offset", self.list_row[1].erased()),
            ("list_row.chevron_w", self.list_row[2].erased()),
            ("overview.card_gap", self.overview[0].erased()),
            ("overview.card_min_width", self.overview[1].erased()),
            ("popover.max_width", self.popover[0].erased()),
            ("popover.caret", self.popover[1].erased()),
            ("popover.icon_gap", self.popover[2].erased()),
        ];
        for (name, span) in named {
            let v = span.resolve(&s);
            if !v.is_finite() || v <= 0.0 {
                return Err(crate::Error::Config(format!(
                    "[components] {name} resolves to {v} at the base scale - it must be finite and positive"
                )));
            }
        }
        let ratios: [(&str, f32); 13] = [
            ("slider.track_ratio", self.slider_track_ratio),
            ("shade.pad", self.shade.pad),
            ("shade.gap", self.shade.gap),
            ("shade.card_pad", self.shade.card_pad),
            ("shade.card_gap", self.shade.card_gap),
            ("shade.radius", self.shade.radius),
            ("shade.tile_puck", self.shade.tile_puck),
            ("shade.note_icon", self.shade.note_icon),
            ("shade.close", self.shade.close),
            ("shade.footer_button", self.shade.footer_button),
            ("shade.card_corner", self.shade.card_corner),
            ("shade.card_inset", self.shade.card_inset),
            ("shade.card_drop", self.shade.card_drop),
        ];
        for (name, v) in ratios {
            if !v.is_finite() || v <= 0.0 {
                return Err(crate::Error::Config(format!(
                    "[components] {name} = {v} - it must be finite and positive"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ComponentMetrics, ComponentSpec};
    use crate::unit::Scale;

    /// **Promotion does not change the values.** They have to be exactly the constants the
    /// widgets drew with before — promoting to a token is giving the integrator a handle, not
    /// changing the render.
    #[test]
    fn the_defaults_keep_todays_numbers() {
        let c = ComponentMetrics::default();
        assert_eq!(
            (
                c.toast.enter_offset,
                c.toast.lift_step,
                c.toast.stack_gap,
                c.toast.pad_x,
                c.toast.icon_gap,
                c.toast.accent_w
            ),
            (24.0, 8.0, 16.0, 16.0, 12.0, 3.0)
        );
        assert_eq!(
            (c.heads_up.pad, c.heads_up.icon_gap, c.heads_up.progress_h),
            (16.0, 12.0, 4.0)
        );
        assert_eq!(
            (
                c.button.pad,
                c.button.icon_gap,
                c.button.ring,
                c.button.ring_stroke
            ),
            (12.0, 8.0, 24.0, 3.0)
        );
        assert_eq!((c.slider.label_gap, c.slider.track_ratio), (8.0, 0.64));
        assert!((c.switch.knob_inset - 3.0).abs() < f32::EPSILON);
        assert_eq!(
            (
                c.list_row.pad,
                c.list_row.two_line_offset,
                c.list_row.chevron_w
            ),
            (16.0, 11.0, 20.0)
        );
        assert_eq!(
            (c.popover.max_width, c.popover.caret, c.popover.icon_gap),
            (420.0, 8.0, 12.0)
        );
    }

    /// **The switch height follows the finger.** Today's formula (`touch_target × 0.6`) has to
    /// be reproduced at any finger width — the plain `du(28.8)` the document writes suddenly
    /// shrinks on a gloved device.
    #[test]
    fn the_switch_height_reproduces_todays_formula() {
        let spec = ComponentSpec::default();
        for finger_mm in [13.0_f32, 9.0, 5.0] {
            let scale = Scale::resolve(
                crate::unit::DU_PER_MM,
                &crate::unit::ScalePolicy::default().with_finger_mm(finger_mm),
                crate::unit::ScaleSource::Backend,
                crate::unit::ScaleConfidence::Measured,
                egui::vec2(1024.0, 600.0),
            );
            let metrics = crate::theme::MetricsSpec::default().resolve(&scale);
            // scale-exempt: this multiplication **is what is under test** — the pre-promotion formula is
            // written out as it was and the token checked against it (the ban on multiplying a touch
            // token by a bare number is a rule for the drawing code).
            let today = metrics.touch_target * 0.6;
            let token = spec.resolve(&scale).switch.height;
            assert!(
                (today - token).abs() < 0.05,
                "finger {finger_mm} mm: today {today} vs the token {token}"
            );
        }
    }

    /// **An overview card is never narrower than three touch targets**, at any finger width.
    #[test]
    fn the_overview_card_floor_is_three_touch_targets() {
        let spec = ComponentSpec::default();
        for finger_mm in [13.0_f32, 9.0, 5.0] {
            let scale = Scale::resolve(
                crate::unit::DU_PER_MM,
                &crate::unit::ScalePolicy::default().with_finger_mm(finger_mm),
                crate::unit::ScaleSource::Backend,
                crate::unit::ScaleConfidence::Measured,
                egui::vec2(1024.0, 600.0),
            );
            let metrics = crate::theme::MetricsSpec::default().resolve(&scale);
            // scale-exempt: the floor is checked against the token it is written in terms of.
            let three = metrics.touch_target * 3.0;
            let token = spec.resolve(&scale).overview.card_min_width;
            assert!(
                (three - token).abs() < 0.1,
                "finger {finger_mm} mm: three targets {three} vs the token {token}"
            );
        }
        assert!((ComponentMetrics::default().overview.card_gap - 24.0).abs() < f32::EPSILON);
    }

    /// A negative or non-finite token is caught as a config error.
    #[test]
    fn a_broken_token_is_a_config_error() {
        assert!(ComponentSpec::default().validate().is_ok());
        let mut bad = ComponentSpec::default();
        bad.button[0] = crate::unit::Span::fixed(crate::unit::Dim::du(-4.0));
        let Err(err) = bad.validate() else {
            unreachable!("a negative token was let through")
        };
        assert!(format!("{err}").contains("button.pad"), "{err}");

        let ratio = ComponentSpec {
            slider_track_ratio: 0.0,
            ..ComponentSpec::default()
        };
        assert!(ratio.validate().is_err());
    }
}
