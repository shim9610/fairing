//! **One frame with every control on it, for looking at a palette.**
//!
//! `cargo run -p fairing --release --example palette_sheet --features "runner-x11 mock" --
//! --tour out/ --size=1440x5800 --panel-mm=305x381 --preset=base --theme=dark`
//!
//! **`--panel-mm` matters here more than anywhere else.** Without it the shell runs the
//! density-unaware fallback, which lands `du` roughly twice the size it takes on a real panel — and
//! since `du` does not shrink, the controls then overrun a half-width column and the
//! sheet stops being one frame.
//!
//! The other examples are each about something — a shell, an integrator's chrome, the motion
//! tokens. None of them puts the whole control vocabulary in one shot, so comparing two palettes
//! meant comparing two different screens and trusting memory for the rest. This does nothing but
//! draw the vocabulary once, densely, at a fixed size: switch on and off, checkbox, radio,
//! segmented, slider at three values, the three button kinds, rows with and without an icon, a
//! disabled row, and the four severity colours side by side.
//!
//! It is a **review surface, not a demo** — nothing here is interactive beyond what egui gives a
//! widget for free, and the values are fixed so two renders differ only by their colours.

use fairing::layout::ExpandableRow;
use fairing::runner::{self, Options};
use fairing::widgets::{
    BadgeTone, BigButton, ButtonKind, Checkbox, Chip, CountBadge, Dropdown, FieldLook, HandleStyle,
    IconButton, LampState, Limit, ListRow, Meter, NumberField, Opener, ProgressBar, Radio,
    SegmentedControl, StatusLamp, Stepper, TouchSlider, Trigger, WheelPicker,
};
use fairing::{icon, layout, screen, ColorRole, Cx, ShellConfig};
use std::cell::RefCell;
use std::rc::Rc;

#[path = "common/mod.rs"]
mod common;

use common::Act;

/// The panel the sheet is drawn at. Wide enough for two columns of controls and tall enough that
/// **one screenshot holds all of it** — which is the whole point of the sheet, and stops being true
/// silently as controls are added. If a render comes back with a section cut off at the bottom, this
/// is the number to raise; the columns are balanced by hand in [`sheet`].
const SIZE: (f32, f32) = (1440.0, 7100.0);

/// The one frame the tour takes.
///
/// **The sheet is opened on a later frame, not in `build`.** `set_fonts` only takes effect at the
/// next `begin_pass`, and the tour harness builds the shell *inside* frame 1 — so a screen drawn on
/// that frame still has the old font definitions, and the first `Theme::strong` draw panics epaint
/// with "`FontFamily::Name(\"fairing-strong\")` is not bound to any fonts". `runner::run_shell`
/// builds in eframe's creation closure, before any frame, and never sees this.
const PLAN: &[Act] = &[
    Act::Settle,
    Act::Wait(4), // let `set_fonts` land before anything asks for the strong family
    Act::Open("sheet"),
    Act::Settle,
    Act::Wait(20), // the font atlas and the first repaint
    Act::Shot("sheet.png"),
];

/// The instrument panel's own lock modes, so the list has real options in it.
const LOCK_MODES: [&str; 4] = [
    "Pound-Drever-Hall",
    "Top of fringe",
    "Side of fringe",
    "Off",
];
/// Its input channels.
const INPUTS: [&str; 3] = ["PD In 1", "PD In 2", "Fast In"];
/// The lock modes' shortcuts, at the right of their rows.
const LOCK_KEYS: [&str; 4] = ["F1", "F2", "F3", "F4"];
const UNITS: [&str; 3] = ["mV", "V", "kV"];
const SHOW: [&str; 3] = ["All", "Errors", "Warnings"];
const MODES: [&str; 3] = ["Standby", "Run", "Service"];
const HOURS: [&str; 24] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "13", "14", "15",
    "16", "17", "18", "19", "20", "21", "22", "23",
];
const QUANTITY: [&str; 20] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17",
    "18", "19", "20",
];

/// Which slot of [`Fixed::flags`] each specimen holds.
const SWITCH_ON: usize = 0;
/// The off switch.
const SWITCH_OFF: usize = 1;
/// The ticked checkbox.
const BOX_ON: usize = 2;
/// The empty checkbox.
const BOX_OFF: usize = 3;

/// The values every control is frozen at, so two palettes differ only in colour.
///
/// The four flags are one specimen each — an on control and an off one of each kind, side by side —
/// so they are a `[bool; 4]` rather than four fields; a control takes `&mut bool`, and a fixed array
/// hands out four independent ones without four names that only differ by their state.
struct Fixed {
    /// `[switch on, switch off, checkbox on, checkbox off]`.
    flags: [bool; 4],
    /// The fallback [`Fixed::flag`] hands back; never drawn.
    spare: bool,
    slider_lo: f32,
    slider_mid: f32,
    slider_hi: f32,
    segment: usize,
    qty_one: i32,
    qty_many: i32,
    qty_floor: i32,
    current: f64,
    temperature: f64,
    integrator: f64,
    lock_mode: usize,
    input: usize,
    input_filled: usize,
    input_under: usize,
    unit: usize,
    show: usize,
    mode: usize,
    hour: usize,
    quantity: usize,
    /// The Display and Night light expandable rows: open or not.
    expanders: [bool; 2],
    night_on: bool,
    advanced: bool,
}

impl Fixed {
    /// One flag by index, named at the call site. An out-of-range index lands on the first slot
    /// rather than panicking — this is a review sheet, and a wrong specimen beats a dead window.
    fn flag(&mut self, i: usize) -> &mut bool {
        let i = if i < self.flags.len() { i } else { 0 };
        self.flags.get_mut(i).unwrap_or(&mut self.spare)
    }
}

impl Default for Fixed {
    fn default() -> Self {
        Self {
            flags: [true, false, true, false],
            spare: false,
            slider_lo: 15.0,
            slider_mid: 50.0,
            slider_hi: 85.0,
            segment: 1,
            qty_one: 2,
            qty_many: 128,
            qty_floor: 0,
            current: 120.0,
            temperature: 25.0,
            integrator: 100.0,
            lock_mode: 0,
            input: 1,
            input_filled: 2,
            input_under: 0,
            unit: 1,
            show: 0,
            mode: 1,
            hour: 9,
            quantity: 2,
            expanders: [true, false],
            night_on: true,
            advanced: false,
        }
    }
}

fn main() -> fairing::Result<()> {
    common::init_logger();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let size = common::arg_size(&args);
    let tour = common::arg_tour(&args);
    let options = Options {
        fullscreen: false,
        title: "fairing palette sheet".to_owned(),
        size: Some(size.unwrap_or(SIZE)),
    };
    match tour {
        Some(dir) => common::run_tour(options, dir, PLAN, |ctx| Ok((build(ctx)?, ()))),
        None => runner::run_shell(options, build),
    }
}

/// The config: `--config=<file.toml>` for a whole candidate palette, then `--preset=<name>` and
/// `--theme=dark|light` on top of it.
///
/// The config route is what makes this useful for a palette that does not ship: `[theme.palette]`
/// takes role → `"#RRGGBB"` for all fourteen roles, so a candidate can be tried, looked at and
/// thrown away without touching [`Preset`](fairing::theme::Preset) or the palette gate.
fn config() -> fairing::Result<ShellConfig> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.iter().find_map(|a| a.strip_prefix("--config="));
    let mut config = match path {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|err| {
                fairing::Error::Config(format!("cannot read the palette config {path}: {err}"))
            })?;
            ShellConfig::from_toml(&text)?
        }
        None => ShellConfig::default(),
    };
    for arg in &args {
        if let Some(name) = arg.strip_prefix("--preset=") {
            name.clone_into(&mut config.theme.preset);
        }
        if let Some(name) = arg.strip_prefix("--theme=") {
            name.clone_into(&mut config.shell.theme);
        }
    }
    Ok(config)
}

fn build(ctx: &egui::Context) -> fairing::Result<fairing::Shell> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut builder = fairing::Shell::builder(config()?).fonts(common::korean_fonts());
    if let Some((w, h)) = common::arg_panel_mm(&args) {
        builder = builder.physical_mm(w, h);
    }
    if let Some(finger) = common::arg_finger_mm(&args) {
        builder =
            builder.scale_policy(fairing::unit::ScalePolicy::default().with_finger_mm(finger));
    }
    // `--text-scale=k` multiplies **only** the type scale, leaving every other token where it is.
    // That is rung 2 of the override ladder on its own - what a low-vision setting does - and it is
    // the one axis the defaults do not couple. It is here so the sheet can show what happens when
    // text and rows move apart.
    //
    // **These fractions were `Dim::finger` until the anchor became part of the type.** The
    // defaults moved the type scale onto the eye when `viewing_distance_mm` arrived; this example
    // was not moved with them and went on multiplying a *finger* fraction, so `--text-scale` was
    // quietly resizing the text against the hand while every other text-derived token followed the
    // eye. Nothing failed and nothing said so - the sheet just drew a scale it was not asked for.
    // The fractions below are the defaults' own, which is what this was always meant to mirror.
    if let Some(k) = args
        .iter()
        .find_map(|a| a.strip_prefix("--text-scale="))
        .and_then(|v| v.parse::<f32>().ok())
    {
        use fairing::theme::MetricsSpec;
        use fairing::unit::{Dim, Span};
        let t = |text: f32, du: f32| Span::fixed(Dim::text(text * k)).min(Dim::du(du * k));
        builder = builder.metrics_spec(MetricsSpec {
            type_scale: [
                t(0.811, 13.0),
                t(1.0, 16.0),
                t(1.063, 17.0),
                t(1.374, 22.0),
                t(1.0, 16.0),
            ],
            ..MetricsSpec::default()
        });
    }
    let mut shell = builder.build(ctx)?;
    let state = Rc::new(RefCell::new(Fixed::default()));
    shell.add(
        screen("sheet", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            sheet(ui, cx, &mut state.borrow_mut());
        })
        .title("Controls")
        .icon(icon::GAUGE)
        .desktop(),
    );
    Ok(shell)
}

/// The sheet: two columns, every control once.
fn sheet(ui: &mut egui::Ui, cx: &mut Cx<'_>, f: &mut Fixed) {
    layout::page(ui, cx, "sheet", |ui, cx| {
        // `columns` and not `horizontal` + `allocate_ui`: the latter leaves each child in the
        // parent's left-to-right layout, so `section`/`group` lay themselves out sideways and run
        // off the edge. `columns` hands each side its own top-down `Ui`.
        ui.columns(2, |cols| {
            let mut side = cols.iter_mut();
            if let Some(left) = side.next() {
                movers(left, cx, f);
                additions(left, cx, f);
                lists(left, cx, f);
                drums(left, cx, f);
                expanders(left, cx, f);
                reports(left, cx);
            }
            if let Some(right) = side.next() {
                marks(right, cx, f);
                editors(right, cx, f);
                layout::section(right, cx, "Tabs");
                let tabs = [
                    layout::Tab {
                        label: "Home",
                        icon: icon::HOME,
                    },
                    layout::Tab {
                        label: "Orders",
                        icon: icon::LIST,
                    },
                    layout::Tab {
                        label: "Help",
                        icon: icon::INFO,
                    },
                ];
                let _ = layout::tab_bar(right, cx, "sheet.tabs", &tabs, &mut f.segment);
            }
        });
    });
}

/// The left column: the things that slide and flip.
fn movers(ui: &mut egui::Ui, cx: &mut Cx<'_>, f: &mut Fixed) {
    layout::section(ui, cx, "Switch");
    layout::group(ui, cx, |ui, cx| {
        let _ = layout::switch_row(ui, cx, "On", None, f.flag(SWITCH_ON), true);
        let _ = layout::switch_row(ui, cx, "Off", None, f.flag(SWITCH_OFF), true);
        let _ = layout::switch_row(ui, cx, "Disabled", None, f.flag(SWITCH_ON), false);
    });

    layout::section(ui, cx, "Slider");
    layout::group(ui, cx, |ui, cx| {
        let _ = layout::slider_row(ui, cx, "Low", &mut f.slider_lo, 0.0..=100.0, " %", true);
        let _ = layout::slider_row(ui, cx, "Half", &mut f.slider_mid, 0.0..=100.0, " %", true);
        let _ = layout::slider_row(ui, cx, "High", &mut f.slider_hi, 0.0..=100.0, " %", false);
        ui.horizontal(|ui| {
            ui.add_space(cx.theme.metrics.content_inset);
            let _ = TouchSlider::new(&mut f.slider_hi, 0.0..=100.0)
                .handle_style(HandleStyle::Knob)
                .show(ui, &mut cx.widgets());
            ui.add_space(cx.theme.metrics.content_inset);
        });
        ui.add_space(cx.theme.metrics.content_inset * 0.5);
    });

    layout::section(ui, cx, "Buttons");
    // Free content in a card: `pad_content` keeps the buttons clear of the corner, and a
    // wrapped row's second line lines up with its first.
    layout::group_with(ui, cx, layout::Deco::new().pad_content(), |ui, cx| {
        ui.horizontal_wrapped(|ui| {
            let _ = BigButton::new("Primary")
                .icon(icon::CHECK)
                .kind(ButtonKind::Primary)
                .show(ui, &mut cx.widgets());
            let _ = BigButton::new("Normal").show(ui, &mut cx.widgets());
            let _ = BigButton::new("Danger")
                .icon(icon::POWER)
                .kind(ButtonKind::Danger)
                .show(ui, &mut cx.widgets());
        });
    });
}

/// The elements added with the new set: chips, icon buttons, bars, lamps and badges.
fn additions(ui: &mut egui::Ui, cx: &mut Cx<'_>, f: &mut Fixed) {
    layout::section(ui, cx, "Chips and icon buttons");
    layout::chip_row(ui, cx, "sheet.chips", |ui, cx| {
        for (i, (label, icon)) in [
            ("All", icon::GRID),
            ("Coffee", icon::THERMOMETER),
            ("Drinks", icon::VOLUME),
            ("Desserts", icon::CALENDAR),
            ("Beans", icon::FOLDER),
        ]
        .into_iter()
        .enumerate()
        {
            let mut on = i == f.segment;
            let _ = Chip::new(label, &mut on)
                .icon(icon)
                .show(ui, &mut cx.widgets());
        }
    });
    layout::group_with(ui, cx, layout::Deco::new().pad_x(0.0), |ui, cx| {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(cx.theme.metrics.content_inset);
            let _ = IconButton::new(icon::PLUS, "Add one")
                .kind(ButtonKind::Primary)
                .show(ui, &mut cx.widgets());
            let _ = IconButton::new(icon::MINUS, "Take one").show(ui, &mut cx.widgets());
            let _ = IconButton::new(icon::TRASH, "Clear the order")
                .kind(ButtonKind::Danger)
                .show(ui, &mut cx.widgets());
            let _ = IconButton::new(icon::CLOSE, "Dismiss")
                .enabled(false)
                .show(ui, &mut cx.widgets());
            ui.add_space(cx.theme.metrics.content_inset);
        });
        ui.add_space(cx.theme.control.gap);
    });
}

/// The closed lists, which are the only element here that draws over its neighbours: every
/// closed form, and a disabled one. What they open into is the demo's to show — a sheet is a
/// still.
fn lists(ui: &mut egui::Ui, cx: &mut Cx<'_>, f: &mut Fixed) {
    layout::section(ui, cx, "Closed lists");
    layout::group_with(ui, cx, layout::Deco::new().pad_content(), |ui, cx| {
        let gap = cx.theme.control.gap;
        let _ = Dropdown::new("sheet.lock", &LOCK_MODES, &mut f.lock_mode)
            .label("Lock mode")
            .hints(&LOCK_KEYS)
            .show(ui, &mut cx.widgets());
        ui.add_space(gap);
        ui.columns(3, |cols| {
            if let [a, b, c] = cols {
                let _ = Dropdown::new("sheet.input", &INPUTS, &mut f.input)
                    .label("Input")
                    .trigger(Trigger::Field(FieldLook::Outlined))
                    .show(a, &mut cx.widgets());
                let _ = Dropdown::new("sheet.input.filled", &INPUTS, &mut f.input_filled)
                    .label("Input")
                    .trigger(Trigger::Field(FieldLook::Filled))
                    .show(b, &mut cx.widgets());
                let _ = Dropdown::new("sheet.input.under", &INPUTS, &mut f.input_under)
                    .label("Input")
                    .trigger(Trigger::Field(FieldLook::Underlined))
                    .opener(Opener::Sheet)
                    .show(c, &mut cx.widgets());
            }
        });
        ui.add_space(gap);
        ui.horizontal(|ui| {
            let _ = Dropdown::new("sheet.unit", &UNITS, &mut f.unit)
                .label("Units")
                .trigger(Trigger::Inline)
                .opener(Opener::Grid)
                .show(ui, &mut cx.widgets());
            ui.add_space(gap);
            let _ = Dropdown::new("sheet.show", &SHOW, &mut f.show)
                .label("Show")
                .trigger(Trigger::Chip)
                .show(ui, &mut cx.widgets());
        });
        ui.add_space(gap);
        let _ = Dropdown::new("sheet.off", &INPUTS, &mut f.input)
            .label("Input")
            .enabled(false)
            .show(ui, &mut cx.widgets());
        ui.add_space(gap);
        let _ = Dropdown::new("sheet.mode", &MODES, &mut f.mode)
            .label("Mode")
            .trigger(Trigger::Tile)
            .opener(Opener::Search)
            .show(ui, &mut cx.widgets());
    });
}

/// The drums: an hour that rolls round, a quantity that stops, and a disabled one.
fn drums(ui: &mut egui::Ui, cx: &mut Cx<'_>, f: &mut Fixed) {
    layout::section(ui, cx, "Drums");
    layout::group_with(ui, cx, layout::Deco::new().pad_content(), |ui, cx| {
        ui.columns(3, |cols| {
            if let [a, b, c] = cols {
                let _ = WheelPicker::new("sheet.hour", &HOURS, &mut f.hour)
                    .wrap(true)
                    .show(a, &mut cx.widgets());
                let _ = WheelPicker::new("sheet.quantity", &QUANTITY, &mut f.quantity)
                    .show(b, &mut cx.widgets());
                let _ = WheelPicker::new("sheet.off", &QUANTITY, &mut f.quantity)
                    .enabled(false)
                    .show(c, &mut cx.widgets());
            }
        });
    });
}

/// The rows that open in place: one open with its body, one closed with a header switch, and
/// the Advanced row that stands for hidden ones.
fn expanders(ui: &mut egui::Ui, cx: &mut Cx<'_>, f: &mut Fixed) {
    layout::section(ui, cx, "Expandable rows");
    layout::group(ui, cx, |ui, cx| {
        let [display, night] = &mut f.expanders;
        let _ = ExpandableRow::new("sheet.display", "Display")
            .icon(icon::DISPLAY)
            .subtitle("Resolution, scale")
            .summary("1920 × 1080")
            .show(ui, cx, display, |ui, cx| {
                layout::info_row(ui, cx, "Scale", "150 %");
                layout::info_row(ui, cx, "Orientation", "Landscape");
            });
        let _ = ExpandableRow::new("sheet.night", "Night light")
            .icon(icon::MOON)
            .subtitle("Warmer colours after dark")
            .switch(&mut f.night_on)
            .show(ui, cx, night, |ui, cx| {
                layout::info_row(ui, cx, "Strength", "60 %");
            });
        let _ = layout::advanced_rows(
            ui,
            cx,
            "sheet.advanced",
            "Advanced",
            &["Proxy", "MAC address"],
            &mut f.advanced,
            |ui, cx| {
                layout::info_row(ui, cx, "Proxy", "None");
            },
        );
    });
}

/// The two scalar editors: a whole number and a setpoint with a unit.
fn editors(ui: &mut egui::Ui, cx: &mut Cx<'_>, f: &mut Fixed) {
    layout::section(ui, cx, "Steppers");
    layout::group_with(ui, cx, layout::Deco::new().pad_x(0.0), |ui, cx| {
        let inset = cx.theme.metrics.content_inset;
        ui.horizontal(|ui| {
            ui.add_space(inset);
            let _ = Stepper::new(&mut f.qty_one)
                .range(0..=9)
                .show(ui, &mut cx.widgets());
            ui.add_space(inset);
            let _ = Stepper::new(&mut f.qty_many)
                .range(0..=250)
                .show(ui, &mut cx.widgets());
            ui.add_space(inset);
        });
        ui.add_space(cx.theme.control.gap);
        ui.horizontal(|ui| {
            ui.add_space(inset);
            // At the bottom of its range, so the exhausted end shows what it does.
            let _ = Stepper::new(&mut f.qty_floor)
                .range(0..=9)
                .show(ui, &mut cx.widgets());
            ui.add_space(inset);
            let _ = Stepper::new(&mut f.qty_one)
                .range(0..=9)
                .enabled(false)
                .show(ui, &mut cx.widgets());
            ui.add_space(inset);
        });
        ui.add_space(cx.theme.control.gap);
    });

    layout::section(ui, cx, "Setpoints");
    layout::group_with(ui, cx, layout::Deco::new().pad_x(0.0), |ui, cx| {
        let inset = cx.theme.metrics.content_inset;
        for (label, value, range, unit, decimals) in [
            ("Current", &mut f.current, 0.0..=200.0, "mA", 1),
            (
                "Temperature",
                &mut f.temperature,
                -10.0..=60.0,
                "\u{b0}C",
                1,
            ),
            ("Integrator", &mut f.integrator, 0.0..=500.0, "ms", 0),
        ] {
            ui.horizontal(|ui| {
                ui.add_space(inset);
                ui.label(label);
                ui.add_space(inset);
                let _ = NumberField::new(value)
                    .range(range)
                    .unit(unit)
                    .decimals(decimals)
                    .show(ui, &mut cx.widgets());
            });
            ui.add_space(cx.theme.control.gap);
        }
    });

    report_bars(ui, cx);
    report_meters(ui, cx);
}

/// The bars: every state and both of the new looks, so one shot compares them.
fn report_bars(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    layout::section(ui, cx, "Report");
    layout::group_with(ui, cx, layout::Deco::new().pad_x(0.0), |ui, cx| {
        let inset = cx.theme.metrics.content_inset;
        for (label, bar) in [
            ("Empty", ProgressBar::determinate(0.0).trailing("0 %")),
            ("Part", ProgressBar::determinate(0.4).trailing("40 %")),
            ("Full", ProgressBar::determinate(1.0).trailing("100 %")),
            ("Unknown", ProgressBar::indeterminate().trailing("…")),
            (
                "Off",
                ProgressBar::determinate(0.6)
                    .enabled(false)
                    .trailing("60 %"),
            ),
            (
                "Stop",
                ProgressBar::determinate(0.4)
                    .stop_indicator(true)
                    .trailing("40 %"),
            ),
            (
                "Steps",
                ProgressBar::determinate(3.0 / 7.0)
                    .steps(7)
                    .trailing("3 / 7"),
            ),
            (
                "Stale",
                ProgressBar::determinate(0.6)
                    .stale_after(std::time::Duration::from_millis(100))
                    .trailing("60 %"),
            ),
        ] {
            ui.horizontal(|ui| {
                ui.add_space(inset);
                ui.label(label);
                ui.add_space(inset);
                // The bar takes **all** the width left to it, so the right-hand inset has to be
                // taken off before it is shown, not added after — added after, the row came out
                // `inset` wider than its card and the read-out was clipped by the card's edge.
                ui.scope(|ui| {
                    ui.set_max_width(ui.available_width() - inset);
                    let _ = bar.show(ui, &mut cx.widgets());
                });
            });
            ui.add_space(cx.theme.control.gap);
        }
    });
}

/// The meters: the same scale in every verdict, so one shot shows what colour is kept for.
fn report_meters(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    layout::section(ui, cx, "Meters");
    layout::group_with(ui, cx, layout::Deco::new().pad_x(0.0), |ui, cx| {
        let inset = cx.theme.metrics.content_inset;
        let limits = [
            Limit::low(45.0, LampState::Fault).label("45"),
            Limit::high(67.0, LampState::Warn).label("67"),
            Limit::high(70.0, LampState::Fault).label("70 !"),
        ];
        let meter = |value: f32| {
            Meter::new(value, 40.0..=80.0)
                .normal(55.0..=65.0)
                .setpoint(60.0)
                .limits(&limits)
        };
        for (label, meter) in [
            ("In range", meter(60.2).readout("60.2 °C")),
            ("Deviation", meter(66.1).readout("66.1 °C")),
            ("Warning", meter(68.3).readout("68.3 °C")),
            ("Alarm", meter(72.8).readout("72.8 °C")),
            (
                "Stale",
                meter(60.2)
                    .readout("60.2 °C")
                    .stale_after(std::time::Duration::from_millis(100)),
            ),
        ] {
            ui.horizontal(|ui| {
                ui.add_space(inset);
                ui.label(label);
                ui.add_space(inset);
                ui.scope(|ui| {
                    ui.set_max_width(ui.available_width() - inset);
                    let _ = meter.show(ui, &mut cx.widgets());
                });
            });
            ui.add_space(cx.theme.control.gap);
        }
    });
}

/// The reporting half of the new set: totals, bars, lamps and badges.
fn reports(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    layout::section(ui, cx, "Totals");
    layout::group(ui, cx, |ui, cx| {
        let _ = layout::info_row(ui, cx, "Subtotal", "\u{20a9}34,000");
        let _ = layout::info_row(ui, cx, "Service charge", "\u{20a9}1,700");
        let _ = layout::total_row(ui, cx, "Total", "\u{20a9}35,700");
    });

    layout::section(ui, cx, "Lamps and badges");
    layout::group_with(ui, cx, layout::Deco::new().pad_x(0.0), |ui, cx| {
        let inset = cx.theme.metrics.content_inset;
        ui.horizontal_wrapped(|ui| {
            ui.add_space(inset);
            for (state, label) in [
                (LampState::Ok, "Lock"),
                (LampState::Active, "Laser ON"),
                (LampState::Warn, "Drift"),
                (LampState::Fault, "Interlock"),
                (LampState::Off, "Purge"),
                (LampState::Unknown, "Cell"),
            ] {
                let _ = StatusLamp::new(state, label).show(ui, &mut cx.widgets());
                ui.add_space(inset);
            }
        });
        ui.add_space(cx.theme.control.gap);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(inset);
            for badge in [
                CountBadge::count(3),
                CountBadge::count(12),
                CountBadge::count(240),
                CountBadge::dot(),
                CountBadge::count(7).tone(BadgeTone::Neutral),
                CountBadge::count(0)
                    .show_zero(true)
                    .tone(BadgeTone::Neutral),
            ] {
                let _ = badge.show(ui, &mut cx.widgets());
                ui.add_space(inset);
            }
        });
        ui.add_space(cx.theme.control.gap);
    });
}

/// The right column: the things that mark a choice, plus the severity colours.
fn marks(ui: &mut egui::Ui, cx: &mut Cx<'_>, f: &mut Fixed) {
    layout::section(ui, cx, "Choice");
    layout::group(ui, cx, |ui, cx| {
        // The card's own inset on **both** sides, and the segmented strip given the width that is
        // left rather than the full column - it used to run to the card's edge and look as if it
        // had overflowed.
        let inset = cx.theme.metrics.content_inset;
        ui.horizontal(|ui| {
            ui.add_space(inset);
            let _ = Checkbox::new(f.flag(BOX_ON)).show(ui, &mut cx.widgets());
            let _ = Checkbox::new(f.flag(BOX_OFF)).show(ui, &mut cx.widgets());
            ui.add_space(inset);
            let _ = Radio::new(true).show(ui, &mut cx.widgets());
            let _ = Radio::new(false).show(ui, &mut cx.widgets());
            ui.add_space(inset);
        });
        ui.add_space(inset * 0.5);
        ui.horizontal(|ui| {
            ui.add_space(inset);
            let width = (ui.available_width() - inset).max(inset);
            ui.allocate_ui_with_layout(
                egui::vec2(width, ui.available_height()),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let _ = SegmentedControl::new(&["Auto", "On", "Off"], f.segment)
                        .show(ui, &mut cx.widgets());
                },
            );
            ui.add_space(inset);
        });
        ui.add_space(inset);
    });

    layout::section(ui, cx, "Rows");
    layout::group(ui, cx, |ui, cx| {
        let _ = ListRow::new("With an icon")
            .subtitle("and a subtitle")
            .icon(icon::ETHERNET)
            .icon_color(ColorRole::Primary)
            .trailing("up")
            .chevron(true)
            .separator(false)
            .show(ui, &mut cx.widgets());
        let _ = ListRow::new("Selected")
            .strong(true)
            .icon(icon::CHECK)
            .icon_color(ColorRole::Primary)
            .separator(false)
            .show(ui, &mut cx.widgets());
        let _ = ListRow::new("Disabled")
            .subtitle("Unavailable")
            .icon(icon::HARD_DRIVE)
            .enabled(false)
            .separator(false)
            .show(ui, &mut cx.widgets());
    });

    layout::section(ui, cx, "Severity");
    layout::group(ui, cx, |ui, cx| {
        for (title, role, glyph) in [
            ("Info", ColorRole::OnSurface, icon::INFO),
            ("Success", ColorRole::Success, icon::CHECK),
            ("Warning", ColorRole::Warning, icon::WARNING),
            ("Error", ColorRole::Danger, icon::ERROR),
        ] {
            let _ = ListRow::new(title)
                .icon(glyph)
                .icon_color(role)
                .separator(false)
                .show(ui, &mut cx.widgets());
        }
    });
}
