//! The widget painters — see [`WidgetPainters`].

use super::{
    BadgeTone, BadgeValue, ButtonKind, HandleStyle, LampState, Limit, MediaShape, Opener,
    RingStyle, Trigger,
};
use crate::icons::{IconRef, IconSet};
use crate::theme::{ColorRole, Theme};
use egui::{Color32, FontId, Pos2, Rect, Vec2};
use std::ops::RangeInclusive;

/// A long press's progress, on a widget given one ([`BigButton::long_press`],
/// [`IconButton::long_press`]).
///
/// [`BigButton::long_press`]: super::BigButton::long_press
/// [`IconButton::long_press`]: super::IconButton::long_press
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoldLook {
    /// Where the built-in drawing puts it: a ring in a button's top-right corner, an arc round an
    /// icon button's disc. On the frame the hold completes it pops — grows past its size and back
    /// — and this rect grows with it.
    pub rect: Rect,
    /// How far the hold has got: 0 at rest, filling while the finger stays down, back to 0 on a
    /// release before the end.
    pub progress: f32,
    /// Whether the hold has completed and the finger is still down.
    pub done: bool,
}

/// What a button painter draws one [`BigButton`](super::BigButton) with.
pub struct ButtonLook<'a> {
    /// The rect the button takes in the layout — what is pressed. It does not move.
    pub rect: Rect,
    /// The rect the built-in button paints: [`rect`](Self::rect), grown a little while pressed.
    pub drawn: Rect,
    /// The label, whole — the built-in button cuts it to its width with `…`.
    pub label: &'a str,
    /// The label's font: the button text style, or the size given with `text_size`.
    pub font: FontId,
    /// The icon before the label, where there is one.
    pub icon: Option<&'a IconRef>,
    /// What the button is for.
    pub kind: ButtonKind,
    /// Whether a finger is on it now.
    pub pressed: bool,
    /// The press, 0 to 1: out over `motion.press`, back over `motion.press_release`.
    pub press: f32,
    /// Whether it can be pressed.
    pub enabled: bool,
    /// Whether it has the keyboard focus. The built-in button rings itself.
    pub focused: bool,
    /// The long press, on a button given one.
    pub hold: Option<HoldLook>,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What an icon button painter draws one [`IconButton`](super::IconButton) with.
pub struct IconButtonLook<'a> {
    /// The square slot the button takes — what is pressed. At least a touch target.
    pub rect: Rect,
    /// The square the built-in disc fills: `control.icon_button` across, grown while pressed.
    pub disc: Rect,
    /// The glyph.
    pub icon: &'a IconRef,
    /// What it does, in words — the button has none on the glass, so this is for a screen reader
    /// or a painter that wants a label.
    pub name: &'a str,
    /// What the button is for.
    pub kind: ButtonKind,
    /// Whether a finger is on it now.
    pub pressed: bool,
    /// The press, 0 to 1.
    pub press: f32,
    /// Whether it can be pressed.
    pub enabled: bool,
    /// Whether it has the keyboard focus. The built-in button rings itself, except while a hold
    /// fills.
    pub focused: bool,
    /// The long press, on a button given one.
    pub hold: Option<HoldLook>,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a switch painter draws one [`Switch`](super::Switch) with.
#[allow(clippy::struct_excessive_bools)] // Independent facts about one widget, not a state machine.
pub struct SwitchLook<'a> {
    /// The slot the switch takes — what is pressed and dragged. A touch target on both axes.
    pub rect: Rect,
    /// The track, centred in the slot: `control.mark_size` high.
    pub track: Rect,
    /// The value.
    pub on: bool,
    /// Where the knob is, 0 (off) to 1 (on): on the switch tween after a tap, under the finger
    /// while it is dragged.
    pub travel: f32,
    /// Whether a finger is on it now.
    pub pressed: bool,
    /// The press, 0 to 1.
    pub press: f32,
    /// Whether it can be changed.
    pub enabled: bool,
    /// Whether it has the keyboard focus.
    pub focused: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a checkbox painter draws one [`Checkbox`](super::Checkbox) with.
#[allow(clippy::struct_excessive_bools)] // Independent facts about one widget, not a state machine.
pub struct CheckboxLook<'a> {
    /// The slot the box takes — what is pressed. A touch target on both axes.
    pub rect: Rect,
    /// The box the built-in checkbox paints, centred in the slot, grown while pressed.
    pub drawn: Rect,
    /// The value.
    pub on: bool,
    /// Whether it is drawn mixed — a dash rather than a tick — for a parent of a mixed set.
    pub indeterminate: bool,
    /// How far the box has crossed to checked, 0 to 1, on the crossfade tween.
    pub mark: f32,
    /// Whether a finger is on it now.
    pub pressed: bool,
    /// The press, 0 to 1.
    pub press: f32,
    /// Whether it can be changed.
    pub enabled: bool,
    /// Whether it has the keyboard focus.
    pub focused: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a radio painter draws one radio mark with — a [`Radio`](super::Radio) or one of a
/// [`RadioGroup`](super::RadioGroup)'s. A group draws its rows' press tint, labels and
/// focus itself.
#[allow(clippy::struct_excessive_bools)] // Independent facts about one widget, not a state machine.
pub struct RadioLook<'a> {
    /// The square the mark is centred in: the radio's slot, or the column a group's row keeps
    /// for it.
    pub rect: Rect,
    /// The square the built-in ring fills, grown while pressed.
    pub drawn: Rect,
    /// The value.
    pub selected: bool,
    /// How far it has crossed to selected, 0 to 1, on the crossfade tween.
    pub mark: f32,
    /// Whether a finger is on it now. A group's marks are never pressed: the row is.
    pub pressed: bool,
    /// The press, 0 to 1.
    pub press: f32,
    /// Whether it can be changed.
    pub enabled: bool,
    /// Whether it has the keyboard focus. A group's marks never have it: the row does.
    pub focused: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a segmented painter draws one [`SegmentedControl`](super::SegmentedControl) with, in its
/// strip form. Where the labels do not fit a strip it stacks them as rows, and those are
/// [`ListRow`](super::ListRow)s.
pub struct SegmentedLook<'a> {
    /// The slot the strip takes — what is pressed. A touch target high.
    pub rect: Rect,
    /// The strip the built-in control paints, inside the slot by the focus ring's room.
    pub strip: Rect,
    /// The band the segments share — the strip less its inset. [`cell`](Self::cell) cuts it.
    pub band: Rect,
    /// The labels, one per segment.
    pub labels: &'a [&'a str],
    /// The selected segment.
    pub selected: usize,
    /// Where the selected face is, in segments: on the switch tween between two of them after a
    /// tap, so a fraction mid-way.
    pub travel: f32,
    /// The segment under a finger, if any.
    pub held: Option<usize>,
    /// The press, 0 to 1.
    pub press: f32,
    /// Whether it can be changed.
    pub enabled: bool,
    /// Whether it has the keyboard focus.
    pub focused: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

impl SegmentedLook<'_> {
    /// Segment `at`'s rect in the band — fractional `at` lands between two, which is where the
    /// built-in face is drawn mid-travel ([`travel`](Self::travel)).
    #[must_use]
    pub fn cell(&self, at: f32) -> Rect {
        // Segment counts are tiny, so the conversion is exact.
        #[allow(clippy::cast_precision_loss)]
        let width = self.band.width() / self.labels.len().max(1) as f32;
        Rect::from_min_size(
            egui::pos2(self.band.min.x + width * at, self.band.min.y),
            egui::vec2(width, self.band.height()),
        )
    }
}

/// What a chip painter draws one [`Chip`](super::Chip) with — on its own or in a
/// [`ChipRow`](super::ChipRow). A row fades its lane's ends itself.
#[allow(clippy::struct_excessive_bools)] // Independent facts about one widget, not a state machine.
pub struct ChipLook<'a> {
    /// The slot the chip takes — what is pressed. It keeps the focus ring's room above and below.
    pub rect: Rect,
    /// The chip the built-in drawing paints, grown while pressed.
    pub drawn: Rect,
    /// The label, whole.
    pub label: &'a str,
    /// The icon over the label, where there is one.
    pub icon: Option<&'a IconRef>,
    /// The value.
    pub selected: bool,
    /// How far it has crossed to selected, 0 to 1, on the crossfade tween.
    pub mark: f32,
    /// Whether a finger is on it now.
    pub pressed: bool,
    /// The press, 0 to 1.
    pub press: f32,
    /// Whether it can be changed.
    pub enabled: bool,
    /// Whether it has the keyboard focus.
    pub focused: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a slider painter draws one [`TouchSlider`](super::TouchSlider) with. The shade's
/// brightness row draws its track itself, with [`paint_track`](super::paint_track).
pub struct SliderLook<'a> {
    /// The slot the slider takes — what is pressed and dragged. A touch target high.
    pub rect: Rect,
    /// The track the built-in slider paints, the slot's width: thicker while pressed.
    pub track: Rect,
    /// The value axis: where 0 % and 100 % are. It does not move with the press.
    pub axis: Rect,
    /// The value.
    pub value: f32,
    /// The value's range.
    pub range: RangeInclusive<f32>,
    /// The value as a share of the range, 0 to 1.
    pub fraction: f32,
    /// Where the built-in mover is: on the axis at [`fraction`](Self::fraction).
    pub handle: Pos2,
    /// The value written the way the built-in label writes it over the mover while it is held.
    pub value_text: &'a str,
    /// The mover's shape the caller asked for.
    pub style: HandleStyle,
    /// The colours the caller gave — the filled side, the rest of the track, the mover's edge —
    /// resolved against the theme.
    pub colors: (Color32, Color32, Color32),
    /// Whether a finger is on it now.
    pub pressed: bool,
    /// The press, 0 to 1. The built-in track swells with it and the value label fades in.
    pub press: f32,
    /// Whether it can be changed.
    pub enabled: bool,
    /// Whether it has the keyboard focus.
    pub focused: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// One end of a [`Stepper`](super::Stepper) or a [`NumberField`](super::NumberField): `−` or `+`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepEnd {
    /// The end's square — what is pressed.
    pub rect: Rect,
    /// Whether it can be pressed: the widget is enabled and the value is not at that end.
    pub live: bool,
    /// Whether a finger is on it now.
    pub pressed: bool,
    /// Its press, 0 to 1.
    pub press: f32,
}

/// What a stepper painter draws one [`Stepper`](super::Stepper) with.
pub struct StepperLook<'a> {
    /// The track: both ends and the figure between them.
    pub rect: Rect,
    /// The `−` end.
    pub minus: StepEnd,
    /// The `+` end.
    pub plus: StepEnd,
    /// The value, after this frame's press.
    pub value: i32,
    /// The value's range.
    pub range: RangeInclusive<i32>,
    /// Whether it can be changed.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours (`builtin::MINUS`, `builtin::PLUS`).
    pub icons: &'a mut IconSet,
}

/// What a number field painter draws one [`NumberField`](super::NumberField) with.
///
/// While the field can be edited its figure is an egui `TextEdit`, drawn by the field over what
/// the painter drew — the painter draws the track, the ends and the unit. Disabled, nothing can
/// be typed, and the painter draws the figure too ([`figure_text`](Self::figure_text)).
pub struct NumberFieldLook<'a> {
    /// The whole field: the track and the unit after it.
    pub rect: Rect,
    /// The track: both ends and the figure between them.
    pub track: Rect,
    /// The `−` end.
    pub minus: StepEnd,
    /// The `+` end.
    pub plus: StepEnd,
    /// The cell between the ends, where the figure goes.
    pub figure: Rect,
    /// The figure as the field writes it, `decimals` after the point.
    pub figure_text: &'a str,
    /// The value, after this frame's press.
    pub value: f64,
    /// The unit after the track, where there is one — and where it goes.
    pub unit: Option<(&'a str, Rect)>,
    /// Whether it can be changed.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a text field painter draws one [`TextField`](super::TextField) with.
///
/// The text, the hint, the caret and the selection are egui's `TextEdit`, drawn over what the
/// painter drew: the painter draws the field — its ground, its edge, its focus.
#[allow(clippy::struct_excessive_bools)] // Independent facts about one widget, not a state machine.
pub struct TextFieldLook<'a> {
    /// The field.
    pub rect: Rect,
    /// Whether it has the keyboard focus — taken from the frame before, as the field is drawn
    /// before the `TextEdit` decides.
    pub focused: bool,
    /// Whether nothing has been typed — the hint shows.
    pub empty: bool,
    /// Whether it hides what is typed.
    pub password: bool,
    /// Whether it takes several lines.
    pub multiline: bool,
    /// Whether it can be typed in.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// One row of a [`WheelPicker`](super::WheelPicker) within reach of its window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelRow {
    /// The option it shows.
    pub index: usize,
    /// Where it is this frame — it moves with the drum.
    pub rect: Rect,
    /// How far it is from the window, in rows: 0 in it.
    pub distance: f32,
}

/// What a wheel painter draws one [`WheelPicker`](super::WheelPicker) with.
pub struct WheelLook<'a> {
    /// The drum.
    pub rect: Rect,
    /// The window the selected row stands in, across the middle.
    pub window: Rect,
    /// The options.
    pub options: &'a [&'a str],
    /// The rows within reach of the window, where they are this frame — the drum turns under the
    /// finger and settles on a row.
    pub rows: &'a [WheelRow],
    /// The selected option.
    pub selected: usize,
    /// Whether the ends join.
    pub wrap: bool,
    /// Whether it can be turned.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// A key on a [`PinPad`](super::PinPad).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PinKey {
    /// A digit, 0 to 9.
    Digit(u8),
    /// Take the last digit off.
    Erase,
    /// Submit what is there.
    Ok,
}

/// Which piece of a [`PinPad`](super::PinPad) a painter is drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PinPart {
    /// The dot row over the keys.
    Dots {
        /// How many digits are in.
        entered: usize,
        /// How long the PIN is, where the pad knows — a ring per digit to come.
        length: Option<u8>,
    },
    /// One key.
    Key {
        /// What it does.
        key: PinKey,
        /// Whether it can be pressed — erase and OK cannot on an empty buffer.
        live: bool,
        /// Whether a finger is on it now.
        pressed: bool,
        /// Its press, 0 to 1.
        press: f32,
    },
}

/// What a PIN pad painter draws one piece of a [`PinPad`](super::PinPad) with — called
/// once for the dot row and once per key.
pub struct PinPadLook<'a> {
    /// Which piece.
    pub part: PinPart,
    /// The piece's rect: the dot row's band, or the key — what is pressed.
    pub rect: Rect,
    /// Where the built-in drawing puts it: a key grown while pressed; the dot row's band.
    pub drawn: Rect,
    /// Whether the pad can be used.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a pattern pad painter draws one [`PatternPad`](super::PatternPad) with.
///
/// Where the caller keeps the path off the glass (`show_path(false)`), it is kept from the painter
/// too: [`path`](Self::path) is empty, [`finger`](Self::finger) is `None` and no dot is taken.
pub struct PatternPadLook<'a> {
    /// The pad — what is drawn on.
    pub rect: Rect,
    /// The dots' centres, row by row from the top left.
    pub dots: &'a [Pos2],
    /// The dots' size: what the built-in pad draws them at.
    pub dot: f32,
    /// How far from a dot's centre a stroke takes it.
    pub reach: f32,
    /// How far each dot has turned to taken, 0 to 1, in the order of [`dots`](Self::dots).
    pub taken: &'a [f32],
    /// The path so far, the dots in the order they were taken.
    pub path: &'a [u8],
    /// Where the finger is, while it draws.
    pub finger: Option<Pos2>,
    /// The colour the caller marked the path with — `Danger` for a refused one — if any.
    pub mark: Option<ColorRole>,
    /// Whether it can be drawn on.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// Which piece of a [`Dropdown`](super::Dropdown) a painter is drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum DropdownPart {
    /// The control: closed on the page, and again as the open panel's first row (`head`) — the
    /// panel is the trigger unrolled. A tap on either opens or closes it.
    Trigger {
        /// Whether the options are up.
        open: bool,
        /// Whether a finger is on it now.
        pressed: bool,
        /// Whether this is the open panel's head rather than the control on the page. In a
        /// search, egui's `TextEdit` takes the query over the head.
        head: bool,
    },
    /// The open panel's ground: the trigger's container unrolled over the options, or a sheet
    /// risen from the bottom of the pane. The dim over the rest of the pane behind a sheet stays
    /// the widget's, in the theme's `Scrim`, as the shade's scrim stays the shell's.
    Panel {
        /// What shows of it this frame — it unrolls from the trigger, or slides up as a sheet.
        shown: Rect,
        /// How far it has opened, 0 to 1.
        unroll: f32,
        /// A sheet's close button, where its tap is kept — the sheet's title is the label.
        close: Option<Rect>,
    },
    /// One option: a row, or a cell of a grid.
    Option {
        /// Its index in the options.
        index: usize,
        /// Whether it is the one chosen.
        selected: bool,
        /// Whether a finger is on it now.
        pressed: bool,
        /// Whether it is a grid's cell rather than a row.
        cell: bool,
    },
}

/// What a dropdown painter draws one piece of a [`Dropdown`](super::Dropdown) with —
/// the control, and while it is open the panel and each option on it.
pub struct DropdownLook<'a> {
    /// Which piece.
    pub part: DropdownPart,
    /// The piece's rect — what is pressed, for the control and an option.
    pub rect: Rect,
    /// The control's closed form.
    pub trigger: Trigger,
    /// How the options open.
    pub opener: Opener,
    /// What the value is of, where the caller said.
    pub label: Option<&'a str>,
    /// The text: an option's own, or for the control and the panel the chosen option's.
    pub text: &'a str,
    /// An option's hint — the muted text at the right of its row — where it has one.
    pub hint: Option<&'a str>,
    /// Whether it can be used.
    pub enabled: bool,
    /// For the panel: the colour its ground is. It comes in as the built-in ground; set it to
    /// yours, and a list that scrolls fades into it at its ends.
    pub ground: Color32,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a row painter draws one [`ListRow`](super::ListRow) with.
///
/// A trailing switch or radio is not the row painter's to draw: the row draws it over what the
/// painter drew, as its own kind — through the switch or radio painter where one is given — in
/// the slot the look names.
#[allow(clippy::struct_excessive_bools)] // Independent facts about one widget, not a state machine.
pub struct RowLook<'a> {
    /// The row — what is pressed.
    pub rect: Rect,
    /// The title.
    pub title: &'a str,
    /// The line under the title, where there is one.
    pub subtitle: Option<&'a str>,
    /// The leading icon, where there is one.
    pub icon: Option<&'a IconRef>,
    /// The leading icon's colour, where the caller gave one (`icon_color`).
    pub icon_color: Option<ColorRole>,
    /// Whether the leading icon sits in a tinted badge (`icon_badge`).
    pub badge: bool,
    /// The title's colour, where the caller gave one (`title_role`).
    pub title_role: Option<ColorRole>,
    /// Whether the title is bold (`strong`).
    pub strong: bool,
    /// The value at the right, where there is one (`trailing`).
    pub value: Option<&'a str>,
    /// The icon at the right, where there is one (`trailing_icon`).
    pub trailing_icon: Option<&'a IconRef>,
    /// Where the row's switch goes, where it has one — drawn over the row, so keep clear of it.
    pub switch: Option<Rect>,
    /// Where the row's radio goes, where it has one — drawn over the row, so keep clear of it.
    pub radio: Option<Rect>,
    /// Whether the row shows `›`: it goes to another screen.
    pub chevron: bool,
    /// Whether it opens in place, and if so whether it is open now (`disclosure`).
    pub disclosure: Option<bool>,
    /// Whether a line runs under it.
    pub separator: bool,
    /// Whether a finger is on it now. The built-in tint comes on at once: a row does not ease.
    pub pressed: bool,
    /// Whether it can be pressed.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What the filled part of a [`ProgressBar`](super::ProgressBar) or a
/// [`ProgressRing`](super::ProgressRing) covers this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ProgressFill {
    /// Determinate: from the start to this share, 0 to 1. Eased towards each reported value, and
    /// never going back unless the caller allows it.
    To(f32),
    /// Indeterminate: a segment between these two shares, moving along.
    Segment(f32, f32),
    /// Indeterminate under reduced motion: the whole track, breathing at this alpha.
    Pulse(f32),
}

/// What a progress bar painter draws one [`ProgressBar`](super::ProgressBar) with.
///
/// The shell's notification banner draws its bar with [`paint_bar`](super::paint_bar), which has
/// no painters: the banner follows its own (`heads_up_painter`).
pub struct ProgressBarLook<'a> {
    /// The row the bar takes.
    pub rect: Rect,
    /// The track, the read-out's room taken off.
    pub track: Rect,
    /// What the fill covers.
    pub fill: ProgressFill,
    /// The read-out, already written by the caller, and where it goes.
    pub readout: Option<(&'a str, Rect)>,
    /// The fill's colour role (`tone`).
    pub tone: ColorRole,
    /// Whether the caller asked for a gap at the fill's head and a dot at the track's end.
    pub stop_indicator: bool,
    /// The cells the caller asked for (`steps`); 0 or 1 is one track.
    pub steps: u8,
    /// Where the worker has gone quiet past `stale_after`: the alpha the built-in fill breathes
    /// at this frame. The built-in read-out dims with it.
    pub stale: Option<f32>,
    /// Whether it is enabled.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a progress ring painter draws one [`ProgressRing`](super::ProgressRing) with.
pub struct ProgressRingLook<'a> {
    /// The square the ring takes.
    pub rect: Rect,
    /// The ring's centre.
    pub center: Pos2,
    /// The band's centre-line radius.
    pub radius: f32,
    /// The band's width.
    pub thickness: f32,
    /// Where the arc starts, in radians, clockwise from three o'clock — twelve o'clock for a
    /// ring, the gap's edge for a gauge.
    pub start: f32,
    /// The arc's length, in radians: a whole turn less the gap.
    pub sweep: f32,
    /// What the fill covers, as shares of the arc. See [`point`](Self::point).
    pub fill: ProgressFill,
    /// The number in the hole, already written by the caller.
    pub value_text: Option<&'a str>,
    /// The word under the number.
    pub label: Option<&'a str>,
    /// How the built-in band is painted.
    pub style: RingStyle,
    /// The fill's colour role (`tone`).
    pub tone: ColorRole,
    /// Where the worker has gone quiet past `stale_after`: the alpha the built-in band breathes at
    /// this frame. The built-in number and word dim with it.
    pub stale: Option<f32>,
    /// Whether it is enabled.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

impl ProgressRingLook<'_> {
    /// The point on the band's centre line at share `t` of the arc.
    #[must_use]
    pub fn point(&self, t: f32) -> Pos2 {
        self.center + Vec2::angled(self.sweep.mul_add(t, self.start)) * self.radius
    }
}

/// What a meter painter draws one [`Meter`](super::Meter) with.
pub struct MeterLook<'a> {
    /// The row the meter takes.
    pub rect: Rect,
    /// The track, the read-out's room taken off. See [`x`](Self::x).
    pub track: Rect,
    /// The span the track stands for.
    pub scale: RangeInclusive<f32>,
    /// The reported value.
    pub value: f32,
    /// Where the pointer is: the value eased, so a noisy signal glides. `None` for a value that is
    /// not a number — no pointer is drawn.
    pub pointer: Option<f32>,
    /// The normal band, where the caller gave one.
    pub normal: Option<RangeInclusive<f32>>,
    /// The setpoint, where the caller gave one.
    pub setpoint: Option<f32>,
    /// The limits.
    pub limits: &'a [Limit<'a>],
    /// The read-out, already written by the caller, and where it goes.
    pub readout: Option<(&'a str, Rect)>,
    /// The verdict on the reported value — what the built-in pointer and read-out are coloured by.
    pub verdict: LampState,
    /// Whether the reading is older than `stale_after` — the built-in meter greys out.
    pub stale: bool,
    /// Whether it is enabled.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

impl MeterLook<'_> {
    /// Where `value` falls across the track; off the scale, at its end.
    #[must_use]
    pub fn x(&self, value: f32) -> f32 {
        self.track.min.x + super::meter::fraction(value, &self.scale) * self.track.width()
    }
}

/// What a lamp painter draws one [`StatusLamp`](super::StatusLamp) with.
pub struct LampLook<'a> {
    /// The lamp and its word.
    pub rect: Rect,
    /// The disc's square.
    pub lens: Rect,
    /// Where the word goes.
    pub word: Rect,
    /// The word.
    pub label: &'a str,
    /// What the lamp says.
    pub state: LampState,
    /// Whether it is enabled.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a badge painter draws one [`CountBadge`](super::CountBadge) with — one shown
/// in the layout. [`CountBadge::paint_over`](super::CountBadge::paint_over) has no `WidgetCx`
/// and draws the built-in badge: what lays one over a rect of its own draws it its own way, as a
/// desktop slot painter does.
pub struct BadgeLook<'a> {
    /// The pill: a circle for a dot or a single digit.
    pub rect: Rect,
    /// What it says.
    pub value: BadgeValue<'a>,
    /// What the built-in badge writes: the count capped at `control.badge_max` (`99+`), the text,
    /// or nothing for a dot.
    pub text: &'a str,
    /// Which fill and ink.
    pub tone: BadgeTone,
    /// Whether it is enabled.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a media card painter draws one [`MediaCard`](super::MediaCard) with.
///
/// The corner action is an [`IconButton`](super::IconButton), drawn over the card as its own
/// kind — through the icon button painter where one is given.
pub struct MediaCardLook<'a> {
    /// The card — what is pressed.
    pub rect: Rect,
    /// The picture's box, at the caller's aspect.
    pub picture: Rect,
    /// The text block, the card's padding taken off.
    pub text: Rect,
    /// The picture and its own pixel size, where it has arrived — crop it to the box with
    /// [`cover_uv`](crate::fit::cover_uv).
    pub image: Option<(egui::TextureId, Vec2)>,
    /// Whether the picture is above the text or before it.
    pub shape: MediaShape,
    /// The title.
    pub title: &'a str,
    /// The muted lines under it.
    pub subtitle: Option<&'a str>,
    /// The figure, already written by the caller.
    pub value: Option<&'a str>,
    /// The word over a veiled picture — an item that cannot be had.
    pub veil: Option<&'a str>,
    /// Where the corner action goes, where the card has one.
    pub action: Option<Rect>,
    /// Whether it is chosen.
    pub selected: bool,
    /// Whether a finger is on it now.
    pub pressed: bool,
    /// Whether it is enabled.
    pub enabled: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// What a feature card painter draws one [`FeatureCard`](super::FeatureCard) with.
///
/// The action disc is an [`IconButton`](super::IconButton), drawn over the card as its own kind —
/// through the icon button painter where one is given.
pub struct FeatureCardLook<'a> {
    /// The card — what is pressed.
    pub rect: Rect,
    /// The column the words go in: the card's padding and the art's share taken off.
    pub text: Rect,
    /// The title.
    pub title: &'a str,
    /// The line under it.
    pub body: Option<&'a str>,
    /// The drawing beside the words, and its square.
    pub art: Option<(&'a IconRef, Rect)>,
    /// Where the action disc goes, where the card has one.
    pub action: Option<Rect>,
    /// Whether the card is lit — the one of a set to press first.
    pub lit: bool,
    /// Whether the caller asked for an edge rather than the fill alone.
    pub outlined: bool,
    /// Whether a finger is on it now.
    pub pressed: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// The callback that draws a [`BigButton`](super::BigButton).
pub type ButtonPainter = Box<dyn FnMut(&egui::Painter, &mut ButtonLook<'_>)>;
/// The callback that draws an [`IconButton`](super::IconButton).
pub type IconButtonPainter = Box<dyn FnMut(&egui::Painter, &mut IconButtonLook<'_>)>;
/// The callback that draws a [`Switch`](super::Switch).
pub type SwitchPainter = Box<dyn FnMut(&egui::Painter, &mut SwitchLook<'_>)>;
/// The callback that draws a [`Checkbox`](super::Checkbox).
pub type CheckboxPainter = Box<dyn FnMut(&egui::Painter, &mut CheckboxLook<'_>)>;
/// The callback that draws a radio mark.
pub type RadioPainter = Box<dyn FnMut(&egui::Painter, &mut RadioLook<'_>)>;
/// The callback that draws a [`SegmentedControl`](super::SegmentedControl)'s strip.
pub type SegmentedPainter = Box<dyn FnMut(&egui::Painter, &mut SegmentedLook<'_>)>;
/// The callback that draws a [`Chip`](super::Chip).
pub type ChipPainter = Box<dyn FnMut(&egui::Painter, &mut ChipLook<'_>)>;
/// The callback that draws a [`TouchSlider`](super::TouchSlider).
pub type SliderPainter = Box<dyn FnMut(&egui::Painter, &mut SliderLook<'_>)>;
/// The callback that draws a [`Stepper`](super::Stepper).
pub type StepperPainter = Box<dyn FnMut(&egui::Painter, &mut StepperLook<'_>)>;
/// The callback that draws a [`NumberField`](super::NumberField).
pub type NumberFieldPainter = Box<dyn FnMut(&egui::Painter, &mut NumberFieldLook<'_>)>;
/// The callback that draws a [`TextField`](super::TextField)'s field.
pub type TextFieldPainter = Box<dyn FnMut(&egui::Painter, &mut TextFieldLook<'_>)>;
/// The callback that draws a [`WheelPicker`](super::WheelPicker).
pub type WheelPainter = Box<dyn FnMut(&egui::Painter, &mut WheelLook<'_>)>;
/// The callback that draws a piece of a [`PinPad`](super::PinPad).
pub type PinPadPainter = Box<dyn FnMut(&egui::Painter, &mut PinPadLook<'_>)>;
/// The callback that draws a [`PatternPad`](super::PatternPad).
pub type PatternPadPainter = Box<dyn FnMut(&egui::Painter, &mut PatternPadLook<'_>)>;
/// The callback that draws a piece of a [`Dropdown`](super::Dropdown).
pub type DropdownPainter = Box<dyn FnMut(&egui::Painter, &mut DropdownLook<'_>)>;
/// The callback that draws a [`ListRow`](super::ListRow).
pub type RowPainter = Box<dyn FnMut(&egui::Painter, &mut RowLook<'_>)>;
/// The callback that draws a [`ProgressBar`](super::ProgressBar).
pub type ProgressBarPainter = Box<dyn FnMut(&egui::Painter, &mut ProgressBarLook<'_>)>;
/// The callback that draws a [`ProgressRing`](super::ProgressRing).
pub type ProgressRingPainter = Box<dyn FnMut(&egui::Painter, &mut ProgressRingLook<'_>)>;
/// The callback that draws a [`Meter`](super::Meter).
pub type MeterPainter = Box<dyn FnMut(&egui::Painter, &mut MeterLook<'_>)>;
/// The callback that draws a [`StatusLamp`](super::StatusLamp).
pub type LampPainter = Box<dyn FnMut(&egui::Painter, &mut LampLook<'_>)>;
/// The callback that draws a [`CountBadge`](super::CountBadge).
pub type BadgePainter = Box<dyn FnMut(&egui::Painter, &mut BadgeLook<'_>)>;
/// The callback that draws a [`MediaCard`](super::MediaCard).
pub type MediaCardPainter = Box<dyn FnMut(&egui::Painter, &mut MediaCardLook<'_>)>;
/// The callback that draws a [`FeatureCard`](super::FeatureCard).
pub type FeatureCardPainter = Box<dyn FnMut(&egui::Painter, &mut FeatureCardLook<'_>)>;

/// **The integrator's widget painters** — rung 5 of the override ladder for the element layer.
///
/// A `WidgetPainters` holds one painter per kind of widget, each optional. A widget whose kind
/// has one is drawn by it; the rest draw themselves. Either way the widget keeps everything that
/// is not paint: the rect it takes in the layout and the rect that is pressed, the press and the
/// drag, the value it changes and when, its animations, focus and the keyboard. The painter is
/// handed the widget's **look** for the frame — where it is, what it says, what state it is in and
/// how far each of its animations has got — and draws all of it, the focus ring included.
///
/// The shell lends its painters to every widget it draws and every widget a screen draws through
/// `cx.widgets()`, its own screens and prompts among them; give them with
/// `ShellBuilder::widget_painters`. Outside the shell, put them in [`WidgetCx::painters`].
///
/// A look's rects are in the coordinates of the `Ui` the widget is drawn in. Where that `Ui` is on
/// a layer something scales or moves — the unlock prompt's card, a screen sliding in — the same
/// transform carries what the painter drew, so it stays on the widget.
///
/// ```
/// use fairing_widgets::widgets::{ButtonKind, ButtonLook, SwitchLook, WidgetPainters};
/// use fairing_widgets::theme::ColorRole;
///
/// let painters = WidgetPainters::new()
///     // Square buttons: the kind is the fill, the press darkens it.
///     .button(|painter: &egui::Painter, button: &mut ButtonLook<'_>| {
///         let theme = button.theme;
///         let role = match button.kind {
///             ButtonKind::Primary => ColorRole::Primary,
///             ButtonKind::Danger => ColorRole::Danger,
///             _ => ColorRole::SurfaceVariant,
///         };
///         let fill = theme.color(role).gamma_multiply(1.0 - 0.2 * button.press);
///         painter.rect_filled(button.drawn, 0.0, fill);
///         painter.text(
///             button.drawn.center(),
///             egui::Align2::CENTER_CENTER,
///             button.label,
///             button.font.clone(),
///             theme.color(ColorRole::OnSurface),
///         );
///     })
///     // A switch that is a lamp: lit on, dark off.
///     .switch(|painter: &egui::Painter, switch: &mut SwitchLook<'_>| {
///         let theme = switch.theme;
///         let off = theme.color(ColorRole::Outline);
///         let lit = off.lerp_to_gamma(theme.color(ColorRole::Primary), switch.travel);
///         painter.circle_filled(switch.track.center(), switch.track.height() / 2.0, lit);
///     });
/// # let _ = painters;
/// ```
///
/// [`WidgetCx::painters`]: crate::WidgetCx::painters
///
/// Build it with [`WidgetPainters::new`] and a method per kind; each widget's look says what its
/// painter is told.
#[derive(Default)]
pub struct WidgetPainters {
    pub(crate) button: Option<ButtonPainter>,
    pub(crate) icon_button: Option<IconButtonPainter>,
    pub(crate) switch: Option<SwitchPainter>,
    pub(crate) checkbox: Option<CheckboxPainter>,
    pub(crate) radio: Option<RadioPainter>,
    pub(crate) segmented: Option<SegmentedPainter>,
    pub(crate) chip: Option<ChipPainter>,
    pub(crate) slider: Option<SliderPainter>,
    pub(crate) stepper: Option<StepperPainter>,
    pub(crate) number_field: Option<NumberFieldPainter>,
    pub(crate) text_field: Option<TextFieldPainter>,
    pub(crate) wheel: Option<WheelPainter>,
    pub(crate) pin_pad: Option<PinPadPainter>,
    pub(crate) pattern_pad: Option<PatternPadPainter>,
    pub(crate) dropdown: Option<DropdownPainter>,
    pub(crate) row: Option<RowPainter>,
    pub(crate) progress_bar: Option<ProgressBarPainter>,
    pub(crate) progress_ring: Option<ProgressRingPainter>,
    pub(crate) meter: Option<MeterPainter>,
    pub(crate) lamp: Option<LampPainter>,
    pub(crate) badge: Option<BadgePainter>,
    pub(crate) media_card: Option<MediaCardPainter>,
    pub(crate) feature_card: Option<FeatureCardPainter>,
}

impl WidgetPainters {
    /// None yet: every widget draws itself.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Draw every [`BigButton`](super::BigButton) with `painter`. See [`ButtonLook`].
    #[must_use]
    pub fn button(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut ButtonLook<'_>) + 'static,
    ) -> Self {
        self.button = Some(Box::new(painter));
        self
    }

    /// Draw every [`IconButton`](super::IconButton) with `painter`. See [`IconButtonLook`].
    #[must_use]
    pub fn icon_button(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut IconButtonLook<'_>) + 'static,
    ) -> Self {
        self.icon_button = Some(Box::new(painter));
        self
    }

    /// Draw every [`Switch`](super::Switch) with `painter`. See [`SwitchLook`].
    #[must_use]
    pub fn switch(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut SwitchLook<'_>) + 'static,
    ) -> Self {
        self.switch = Some(Box::new(painter));
        self
    }

    /// Draw every [`Checkbox`](super::Checkbox) with `painter`. See [`CheckboxLook`].
    #[must_use]
    pub fn checkbox(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut CheckboxLook<'_>) + 'static,
    ) -> Self {
        self.checkbox = Some(Box::new(painter));
        self
    }

    /// Draw every radio mark with `painter` — a [`Radio`](super::Radio)'s and a
    /// [`RadioGroup`](super::RadioGroup)'s. See [`RadioLook`].
    #[must_use]
    pub fn radio(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut RadioLook<'_>) + 'static,
    ) -> Self {
        self.radio = Some(Box::new(painter));
        self
    }

    /// Draw every [`SegmentedControl`](super::SegmentedControl) strip with `painter`. See
    /// [`SegmentedLook`].
    #[must_use]
    pub fn segmented(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut SegmentedLook<'_>) + 'static,
    ) -> Self {
        self.segmented = Some(Box::new(painter));
        self
    }

    /// Draw every [`Chip`](super::Chip) with `painter`, a [`ChipRow`](super::ChipRow)'s
    /// included. See [`ChipLook`].
    #[must_use]
    pub fn chip(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut ChipLook<'_>) + 'static,
    ) -> Self {
        self.chip = Some(Box::new(painter));
        self
    }

    /// Draw every [`TouchSlider`](super::TouchSlider) with `painter`. See [`SliderLook`].
    #[must_use]
    pub fn slider(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut SliderLook<'_>) + 'static,
    ) -> Self {
        self.slider = Some(Box::new(painter));
        self
    }

    /// Draw every [`Stepper`](super::Stepper) with `painter`. See [`StepperLook`].
    #[must_use]
    pub fn stepper(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut StepperLook<'_>) + 'static,
    ) -> Self {
        self.stepper = Some(Box::new(painter));
        self
    }

    /// Draw every [`NumberField`](super::NumberField) with `painter` — all but the figure being
    /// typed. See [`NumberFieldLook`].
    #[must_use]
    pub fn number_field(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut NumberFieldLook<'_>) + 'static,
    ) -> Self {
        self.number_field = Some(Box::new(painter));
        self
    }

    /// Draw every [`TextField`](super::TextField)'s field with `painter` — the text over it is
    /// egui's. See [`TextFieldLook`].
    #[must_use]
    pub fn text_field(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut TextFieldLook<'_>) + 'static,
    ) -> Self {
        self.text_field = Some(Box::new(painter));
        self
    }

    /// Draw every [`WheelPicker`](super::WheelPicker) with `painter`. See [`WheelLook`].
    #[must_use]
    pub fn wheel(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut WheelLook<'_>) + 'static,
    ) -> Self {
        self.wheel = Some(Box::new(painter));
        self
    }

    /// Draw every [`PinPad`](super::PinPad) with `painter`, a piece at a time — the unlock
    /// prompt's included. See [`PinPadLook`].
    #[must_use]
    pub fn pin_pad(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut PinPadLook<'_>) + 'static,
    ) -> Self {
        self.pin_pad = Some(Box::new(painter));
        self
    }

    /// Draw every [`PatternPad`](super::PatternPad) with `painter` — the unlock prompt's
    /// included. See [`PatternPadLook`].
    #[must_use]
    pub fn pattern_pad(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut PatternPadLook<'_>) + 'static,
    ) -> Self {
        self.pattern_pad = Some(Box::new(painter));
        self
    }

    /// Draw every [`Dropdown`](super::Dropdown) with `painter`, a piece at a time. See
    /// [`DropdownLook`].
    #[must_use]
    pub fn dropdown(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut DropdownLook<'_>) + 'static,
    ) -> Self {
        self.dropdown = Some(Box::new(painter));
        self
    }

    /// Draw every [`ListRow`](super::ListRow) with `painter` — the shell's settings rows
    /// included. A row's switch or radio is still drawn as its own kind. See [`RowLook`].
    #[must_use]
    pub fn row(mut self, painter: impl FnMut(&egui::Painter, &mut RowLook<'_>) + 'static) -> Self {
        self.row = Some(Box::new(painter));
        self
    }

    /// Draw every [`ProgressBar`](super::ProgressBar) with `painter`. See [`ProgressBarLook`].
    #[must_use]
    pub fn progress_bar(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut ProgressBarLook<'_>) + 'static,
    ) -> Self {
        self.progress_bar = Some(Box::new(painter));
        self
    }

    /// Draw every [`ProgressRing`](super::ProgressRing) with `painter`. See [`ProgressRingLook`].
    #[must_use]
    pub fn progress_ring(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut ProgressRingLook<'_>) + 'static,
    ) -> Self {
        self.progress_ring = Some(Box::new(painter));
        self
    }

    /// Draw every [`Meter`](super::Meter) with `painter`. See [`MeterLook`].
    #[must_use]
    pub fn meter(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut MeterLook<'_>) + 'static,
    ) -> Self {
        self.meter = Some(Box::new(painter));
        self
    }

    /// Draw every [`StatusLamp`](super::StatusLamp) with `painter`. See [`LampLook`].
    #[must_use]
    pub fn lamp(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut LampLook<'_>) + 'static,
    ) -> Self {
        self.lamp = Some(Box::new(painter));
        self
    }

    /// Draw every [`CountBadge`](super::CountBadge) shown in the layout with `painter`. See
    /// [`BadgeLook`].
    #[must_use]
    pub fn badge(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut BadgeLook<'_>) + 'static,
    ) -> Self {
        self.badge = Some(Box::new(painter));
        self
    }

    /// Draw every [`MediaCard`](super::MediaCard) with `painter`. See [`MediaCardLook`].
    #[must_use]
    pub fn media_card(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut MediaCardLook<'_>) + 'static,
    ) -> Self {
        self.media_card = Some(Box::new(painter));
        self
    }

    /// Draw every [`FeatureCard`](super::FeatureCard) with `painter`. See [`FeatureCardLook`].
    #[must_use]
    pub fn feature_card(
        mut self,
        painter: impl FnMut(&egui::Painter, &mut FeatureCardLook<'_>) + 'static,
    ) -> Self {
        self.feature_card = Some(Box::new(painter));
        self
    }

    /// The kinds that have a painter, for `Debug`.
    fn kinds(&self) -> Vec<&'static str> {
        [
            ("button", self.button.is_some()),
            ("icon_button", self.icon_button.is_some()),
            ("switch", self.switch.is_some()),
            ("checkbox", self.checkbox.is_some()),
            ("radio", self.radio.is_some()),
            ("segmented", self.segmented.is_some()),
            ("chip", self.chip.is_some()),
            ("slider", self.slider.is_some()),
            ("stepper", self.stepper.is_some()),
            ("number_field", self.number_field.is_some()),
            ("text_field", self.text_field.is_some()),
            ("wheel", self.wheel.is_some()),
            ("pin_pad", self.pin_pad.is_some()),
            ("pattern_pad", self.pattern_pad.is_some()),
            ("dropdown", self.dropdown.is_some()),
            ("row", self.row.is_some()),
            ("progress_bar", self.progress_bar.is_some()),
            ("progress_ring", self.progress_ring.is_some()),
            ("meter", self.meter.is_some()),
            ("lamp", self.lamp.is_some()),
            ("badge", self.badge.is_some()),
            ("media_card", self.media_card.is_some()),
            ("feature_card", self.feature_card.is_some()),
        ]
        .into_iter()
        .filter_map(|(kind, set)| set.then_some(kind))
        .collect()
    }
}

impl std::fmt::Debug for WidgetPainters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetPainters")
            .field("painted", &self.kinds())
            .finish()
    }
}
