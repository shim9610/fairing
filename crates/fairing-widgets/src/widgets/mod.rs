//! The basic touch widget set (A7).
//!
//! Every one of them draws a pressed state from the first frame, and every dimension any of them
//! uses comes from [`ControlMetrics`](crate::theme::ControlMetrics) — the tokens they share, so
//! that a switch, a slider and a button read as one family instead of three separate drawings.
//! Animation values live in the shell's store through `WidgetCx::animate`, so a stateless closure
//! screen gets them too.
//!
//! | Widget | For |
//! |---|---|
//! | [`BigButton`] | A large touch button (icon + label), with an optional long-press ring |
//! | [`Switch`] | A toggle whose knob can be dragged |
//! | [`Checkbox`] | A box that carries a drawn tick — on or off, independent of its neighbours |
//! | [`Radio`] · [`RadioGroup`] | One of a set — a ring that fills only at its centre |
//! | [`SegmentedControl`] | Two to four exclusive options sharing one track |
//! | [`ListRow`] | A settings row (icon · title · subtitle · trailing) |
//! | [`TouchSlider`] | A 1:1 finger slider with a selectable handle shape |
//! | [`TextField`] | An input field — egui's default height comes from the text line and does not reach a finger |
//! | [`ProgressBar`] | A track that **reports** a value, determinate or not — read-only, so not a slider |
//! | [`StatusLamp`] | A lit disc and its word — the instrument panel's "Laser ON · Lock · Interlock OK" |
//! | [`CountBadge`] | A count or a dot, inline or laid over a corner |
//! | [`IconButton`] | A disc with a glyph and no words — the `+` on a product card |
//! | [`Chip`] · [`ChipRow`] | An icon over a word, in a lane that scrolls — a **filter**, where a segmented control is a setting |
//! | [`Stepper`] | `−  n  +` for a whole number — a scalar editor, like the switch and the slider |
//! | [`NumberField`] | The same track with a typed figure and a unit — the instrument panel's setpoint |
//! | [`Dropdown`] | A closed list — five closed forms (`Trigger`), four ways of opening (`Opener`), a hint at the right of a row — over the content and clipped to the caller's own pane |
//! | [`MediaCard`] | A picture with a text block — the one structure six reference screens share |
//! | [`ProgressRing`] | The bar bent round: a value read from across the bench, with its number in the hole |
//! | [`Meter`] | A measured value on a scale — normal band, limits, setpoint — that hands back a verdict |
//! | [`FeatureCard`] | A tile with an icon, a title and a line, that opens something |
//! | [`WheelPicker`] | A drum of **ordered** values — an hour, a quantity — where a dropdown is a list of names |
//! | [`unroll()`] | The body under an expandable row: eased open and shut, measured as it goes — `layout::ExpandableRow` is built on it |
//! | [`PinPad`] | The digits of a PIN with their dot row — the shell's unlock prompt uses it, and so can a login screen of your own |
//! | [`PatternPad`] | A path drawn through a square of dots — the PIN's other way in, from the same prompt |
//!
//! Every one of them can be drawn your way: a [`WidgetPainters`] holds a painter per kind, told the
//! widget's look, and the widget keeps its press, its value and its motion.
//!
//! The tick and the dot are **drawn, not glyphs**: a typeface an integrator supplies may have no
//! `✓`, and tofu in a checkbox is worse than no checkbox.

mod badge;
mod button;
mod checkbox;
mod chip;
mod dropdown;
mod feature_card;
mod icon_button;
mod lamp;
mod list_row;
mod media_card;
mod meter;
mod number_field;
mod painters;
mod pattern_pad;
mod pin_pad;
mod progress;
mod progress_ring;
mod radio;
mod segmented;
mod slider;
mod stepper;
mod switch;
mod text_field;
mod unroll;
mod wheel;

pub use badge::{badge_text, BadgeAnchor, BadgeTone, BadgeValue, CountBadge};
pub use button::{BigButton, ButtonKind, LongPressResponse};
pub use checkbox::Checkbox;
pub use chip::{paint_lane_fade, Chip, ChipItem, ChipPick, ChipRow};
pub use dropdown::{Dropdown, FieldLook, Opener, Trigger};
pub use feature_card::{FeatureCard, FeatureCardPick};
pub use icon_button::IconButton;
pub use lamp::{LampMark, LampState, StatusLamp};
pub use list_row::{ListRow, RowPress};
pub use media_card::{
    MediaCard, MediaPick, MediaShape, ASPECT_COVER, ASPECT_PHOTO, ASPECT_SQUARE, ASPECT_WIDE,
};
pub use meter::{Limit, Meter, MeterReading};
pub use number_field::NumberField;
pub use painters::{
    BadgeLook, BadgePainter, ButtonLook, ButtonPainter, CheckboxLook, CheckboxPainter, ChipLook,
    ChipPainter, DropdownLook, DropdownPainter, DropdownPart, FeatureCardLook, FeatureCardPainter,
    HoldLook, IconButtonLook, IconButtonPainter, LampLook, LampPainter, MediaCardLook,
    MediaCardPainter, MeterLook, MeterPainter, NumberFieldLook, NumberFieldPainter, PatternPadLook,
    PatternPadPainter, PinKey, PinPadLook, PinPadPainter, PinPart, ProgressBarLook,
    ProgressBarPainter, ProgressFill, ProgressRingLook, ProgressRingPainter, RadioLook,
    RadioPainter, RowLook, RowPainter, SegmentedLook, SegmentedPainter, SliderLook, SliderPainter,
    StepEnd, StepperLook, StepperPainter, SwitchLook, SwitchPainter, TextFieldLook,
    TextFieldPainter, WheelLook, WheelPainter, WheelRow, WidgetPainters,
};
pub use pattern_pad::{PatternPad, PatternPadResponse, MAX_GRID, MIN_GRID};
pub use pin_pad::{PinPad, PinPadResponse, MAX_DIGITS, PHONE_ORDER};
pub use progress::{paint_bar, ProgressBar};
pub use progress_ring::{ProgressRing, RingStyle};
pub use radio::{Radio, RadioGroup};
pub use segmented::{SegmentedControl, SegmentedPick};
pub use slider::{
    base_thickness, paint_track, swollen, ColorSpec, HandleStyle, SliderColors, TouchSlider,
    TrackPaint,
};
pub use stepper::Stepper;
pub use switch::Switch;
pub use text_field::TextField;
pub use unroll::{unroll, Unroll, Unrolled};
pub use wheel::WheelPicker;

use crate::cx::WidgetCx as Cx;

/// The A7 press scale: `press_scale` while held, 1 on release. Stored per `id` in the shell's
/// store. The return value is the scale to draw at right now.
pub fn press_scale(cx: &mut Cx<'_>, id: egui::Id, pressed: bool) -> f32 {
    let tokens = cx.theme.motion;
    let target = if pressed { tokens.press_scale } else { 1.0 };
    let tween = if pressed {
        tokens.press
    } else {
        tokens.press_release
    };
    cx.animate(id.with("press"), target, tween)
}
