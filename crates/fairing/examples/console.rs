//! **The crate's own console**: the L-shaped chrome over fairing's brand, at a desk-shaped panel.
//!
//! ```text
//! cargo run -p fairing --features runner-x11,mock --example console -- --size=1280x800
//! ```
//!
//! `demo` shows the same material as a phone does — a desktop of icons, a screen at a time, the
//! nav bar underneath. This shows it the way a bench instrument does: a rail down the left that is
//! part of the chrome, the page in its elbow, and no screen stack at all. It is one screen with
//! five pages, which is what a machine bolted to a desk actually is.
//!
//! Everything here is a `layout::` or `widgets::` call.
//!
//! What it is for:
//!
//! * `layout::Rail` — the arm takes the status bar's own fill, so the two read as one L and there
//!   is no rule between them. Folding is on here, so a swipe across the arm drops it to
//!   icons — or away entirely with `--fold=away` — and a flick anywhere on the page does
//!   the same (`FoldGesture::Anywhere`, the default). Folding, the bar beside the
//!   live entry melts into a disc behind its icon.
//! * `layout::SelectMark` — the rail's selection, in the console's own idiom.
//! * `layout::transit` — the pages come and go: the one leaving plays its exit, then the one
//!   arriving plays its entry, and each page says which way (`Transit for Page`):
//!   the rail pages follow the mark, the overview is home, and an alert flies in from the right
//!   and lands with a little give.
//! * `[overlay] layout = "split"` — the left of the top edge opens the notifications and the
//!   right the controls, each a panel filling the height and holding only its own; a pull or a
//!   bar tap on the other side crosses between them.
//! * `[overlay] reveal = "card"` — each panel is a floating card that materialises on its own
//!   side, rather than a curtain drawn down over the run. It lies on a frosted copy of
//!   the page, and `--glass=0.65` sets how opaque it is over it (`[overlay] card_glass`, 0.7 by
//!   default). It stands off the page with the light along its top edge, and
//!   `--relief=0` lays it flat (`[overlay] card_relief`).

mod common;

use common::{Act, Expect, Side, Spot};
use fairing::notify::Level;
use fairing::runner::{self, Options};
use fairing::services::mock::{MockBluetooth, MockClock, MockDisplay, MockPower, MockWifi};
use fairing::services::{Services, WallTime};
use fairing::{icon, layout, screen, ColorRole, Cx, IconRef, ShellConfig};
use std::cell::RefCell;
use std::rc::Rc;

/// Which page the rail has open, and the values the controls page edits.
struct Console {
    page: Page,
    fan: bool,
    verbose: bool,
    brightness: f32,
    /// How far the rail folds: `--fold=away` hides it whole, the default keeps the icons.
    fold: layout::Fold,
    /// The settings screen open in the page — its tile's label and its id. `None` is the board.
    settings: Option<(&'static str, &'static str)>,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            page: Page::Overview,
            fan: true,
            verbose: false,
            brightness: 62.0,
            fold: layout::Fold::Icons,
            settings: None,
        }
    }
}

/// The rail, in the order a bench uses them.
const RAIL: &[(&str, IconRef)] = &[
    ("Overview", icon::GAUGE),
    ("Controls", icon::WRENCH),
    ("Alerts", icon::BELL),
    ("Storage", icon::HARD_DRIVE),
    ("Settings", icon::SETTINGS),
];

/// The console's pages, in the rail's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Controls,
    Alerts,
    Storage,
    Settings,
}

impl Page {
    /// In the rail's order — one entry a page, the same order as `RAIL`.
    const ALL: [Page; 5] = [
        Page::Overview,
        Page::Controls,
        Page::Alerts,
        Page::Storage,
        Page::Settings,
    ];

    /// Where it sits on the rail.
    fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

/// **How the console's pages come and go**. Three characters, not five styles, each
/// with a reason:
///
/// - a rail page **follows the mark**: further down the rail it comes up from below as the page
///   before goes up, and back the other way — the rule an index already has;
/// - the overview is **home**: leaving it, the next page comes up over it; coming home, the page
///   in hand drops back down and the overview grows in from a step behind, the way a task closes;
/// - an alert **arrives**: it flies in from beyond the right edge and lands with a little give,
///   because an alert is not browsed to, and it leaves the way it came.
impl layout::Transit for Page {
    fn enter(&self, from: &Self) -> layout::Motion {
        match self {
            Page::Alerts => layout::Motion::fly(layout::Side::Right).over(
                fairing::motion::Tween::back_out(std::time::Duration::from_millis(360)),
            ),
            Page::Overview => layout::Motion::zoom(0.94),
            _ => self.index().enter(&from.index()),
        }
    }

    fn exit(&self, to: &Self) -> layout::Motion {
        match (self, to) {
            (Page::Alerts, _) => layout::Motion::fly(layout::Side::Right),
            (_, Page::Overview) => layout::Motion::slide(layout::Side::Bottom).scaled(0.96),
            _ => self.index().exit(&to.index()),
        }
    }
}

/// **The console's palette** — a light instrument panel: a near-neutral chrome, a lighter page in
/// its elbow, and one restrained accent.
///
/// Not a preset: a product's colours are the product's, and `ShellBuilder::palettes` is the door
/// for them. The crate's own contrast gate binds only the presets it ships, so nothing here
/// is checked by it.
///
/// **The ramp runs the way the roles do.** `Surface` is what `chrome::status_bar` and
/// `layout::Rail` both fill with, so it is the chrome's colour; `SurfaceVariant` is the page
/// panel's, one step lighter. Read the other way round the L comes out lighter than the page it
/// frames and the shape reads inside out.
fn console_palette() -> fairing::theme::Palette {
    use egui::Color32;
    fairing::theme::Palette {
        background: Color32::from_rgb(0xed, 0xed, 0xef),
        surface: Color32::from_rgb(0xf1, 0xf2, 0xf4),
        surface_variant: Color32::from_rgb(0xf8, 0xf9, 0xfb),
        on_surface: Color32::from_rgb(0x0b, 0x10, 0x30),
        muted: Color32::from_rgb(0x6b, 0x70, 0x86),
        primary: Color32::from_rgb(0x58, 0x59, 0xba),
        on_primary: Color32::WHITE,
        danger: Color32::from_rgb(0xd0, 0x45, 0x50),
        warning: Color32::from_rgb(0x9a, 0x6b, 0x10),
        success: Color32::from_rgb(0x2f, 0x8a, 0x4f),
        focus: Color32::from_rgb(0x7b, 0x7c, 0xcc),
        scrim: Color32::from_black_alpha(90),
        outline: Color32::from_rgb(0xe6, 0xe7, 0xee),
        control_edge: Color32::from_rgb(0xb6, 0xb8, 0xc6),
        pressed: Color32::from_black_alpha(20),
        shadow: Color32::from_black_alpha(12),
        shade_surface: None,
    }
}

fn main() -> fairing::Result<()> {
    common::init_logger();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = Options {
        fullscreen: false,
        title: "fairing console".to_owned(),
        size: Some(common::arg_size(&args).unwrap_or((1280.0, 800.0))),
    };
    match common::arg_tour(&args) {
        Some(dir) => common::run_tour(options, dir, TOUR, |ctx| Ok((build(ctx)?, ()))),
        None => runner::run_shell(options, build),
    }
}

const TOUR: &[Act] = &[
    Act::Settle,
    Act::Wait(12),
    Act::Settle,
    Act::Shot("01-overview.png"),
    // Something for the notifications card to hold. It arrives as a heads-up, and the tour waits
    // the banner out (4.4 s, on the clock) so the pull below lands on the shade, not on the banner.
    Act::Notify(
        Level::Warning,
        "Chamber door open",
        "Close it to resume the run",
    ),
    Act::WaitMs(4600),
    Act::Settle,
    Act::Expect(Expect::NoText("Chamber door open")),
    // The README's animation is this stretch (`--record`): each half of the split shade pulled
    // open as a card on frosted glass, and put away again.
    Act::Record("console-shade"),
    Act::Wait(20),
    // The shade is split and each half is a card, so there is no tiles stop to
    // land on: a card let go short of the snap ratio (a third of its height) sinks back shut.
    // Each pull runs well past it, to three quarters of the content, and is held still before
    // the release, because a release still carrying speed is a fling. The notifications first,
    // from the left.
    Act::Press(Spot::Edge(Side::Top, 0.23)),
    Act::Wait(2),
    Act::DragTo {
        to: Spot::Page(0.23, 0.77),
        frames: 14,
    },
    Act::Hold(8),
    Act::Release,
    Act::Settle,
    Act::Wait(20),
    Act::Settle,
    Act::Expect(Expect::ShadeOpen),
    Act::Shot("02-notifications-card.png"),
    // A tap on the page beside a card puts it away; then the controls, from the right.
    Act::Tap(Spot::Page(0.9, 0.95)),
    Act::Settle,
    Act::Wait(20),
    Act::Settle,
    Act::Expect(Expect::ShadeClosed),
    Act::Press(Spot::Edge(Side::Top, 0.75)),
    Act::Wait(2),
    Act::DragTo {
        to: Spot::Page(0.75, 0.77),
        frames: 14,
    },
    Act::Hold(8),
    Act::Release,
    Act::Settle,
    Act::Wait(20),
    Act::Settle,
    Act::Expect(Expect::ShadeOpen),
    Act::Shot("03-controls-card.png"),
    Act::Tap(Spot::Page(0.16, 0.95)),
    Act::Settle,
    Act::Wait(20),
    Act::Settle,
    Act::Expect(Expect::ShadeClosed),
    Act::RecordEnd,
    // The rail's entries are pressed **by their words**, not by where a row once was: the row
    // height follows the finger and the type scale, and a coordinate written for one of them
    // lands on the entry below under another. Each page is then checked by a word only it shows.
    Act::Tap(Spot::Text("Controls")),
    Act::Settle,
    // The page's own transit is the example's, not the shell's, so `Settle` cannot wait for it.
    Act::Until(Expect::Text("Cooling fan"), 120),
    Act::Settle,
    Act::Shot("04-controls.png"),
    // Swipe the arm shut. The fold starts a few frames into the drag, so the frame at the end of
    // the move is the middle of it: the words gone, the icons on their way, the bar half melted.
    Act::Press(Spot::Page(0.09, 0.68)),
    Act::DragTo {
        to: Spot::Across(0.015),
        frames: 10,
    },
    Act::Shot("05a-rail-folding.png"),
    Act::Release,
    Act::Settle,
    Act::Wait(20),
    Act::Settle,
    Act::Expect(Expect::NoText("Overview")),
    Act::Shot("05-rail-folded.png"),
    // Back open with a flick across the page - nowhere near the arm - then along the rail: the
    // three boards that were rows of prose until now.
    Act::Press(Spot::Page(0.5, 0.92)),
    Act::DragTo {
        to: Spot::Across(0.7),
        frames: 3,
    },
    Act::Release,
    Act::Settle,
    Act::Wait(20),
    Act::Settle,
    Act::Expect(Expect::Text("Overview")),
    Act::Tap(Spot::Text("Alerts")),
    Act::Settle,
    // The page's own transit is the example's, not the shell's, so `Settle` cannot wait for it.
    Act::Until(Expect::Text("Lamp hours"), 120),
    Act::Settle,
    Act::Shot("06-alerts.png"),
    Act::Tap(Spot::Text("Storage")),
    Act::Settle,
    // The page's own transit is the example's, not the shell's, so `Settle` cannot wait for it.
    Act::Until(Expect::Text("Internal"), 120),
    Act::Settle,
    Act::Shot("07-storage.png"),
    Act::Tap(Spot::Text("Settings")),
    Act::Settle,
    // The page's own transit is the example's, not the shell's, so `Settle` cannot wait for it.
    Act::Until(Expect::Text("Language"), 120),
    Act::Settle,
    Act::Shot("08-settings.png"),
    // A tile opens the crate's own settings screen in the page, under a row back to the tiles.
    Act::Tap(Spot::Text("Display")),
    Act::Settle,
    Act::Until(Expect::Text("Brightness"), 120),
    Act::Settle,
    Act::Shot("09-settings-display.png"),
    Act::Tap(Spot::Text("All settings")),
    Act::Settle,
    Act::Until(Expect::Text("Language"), 120),
    Act::Expect(Expect::NoText("Brightness")),
];

/// The brand block, as a **status item** rather than a whole-bar painter.
///
/// `status_bar_painter` hands the integrator the entire bar — including the built-in items, which
/// it then does not draw. Using it to put two words on the left is how the clock, the Wi-Fi and
/// the battery all quietly disappeared: the shell was told the painter owned the bar, and it did
/// as it was told. A `status_item` in the `Left` slot adds the brand and leaves everything else to
/// `[status_bar] right`, which is what that API is for.
fn brand_item() -> fairing::chrome::StatusItemDecl {
    fairing::chrome::status_item(
        "console.brand",
        fairing::chrome::Slot::Left,
        |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let theme = cx.theme;
            let m = &theme.metrics;
            // The name in the display face and a size up from body, its line underneath: the block
            // is read as one thing, so it is laid out as one and allocated as one.
            let name = theme.display(m.type_scale.heading);
            let sub = egui::FontId::proportional(m.type_scale.small);
            let (name_h, sub_h) = ui
                .ctx()
                .fonts_mut(|f| (f.row_height(&name), f.row_height(&sub)));
            let w = ui
                .ctx()
                .fonts_mut(|f| {
                    f.layout_no_wrap(
                        "Bench console".to_owned(),
                        sub.clone(),
                        theme.color(ColorRole::Muted),
                    )
                })
                .rect
                .width();
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(w, name_h + sub_h), egui::Sense::hover());
            ui.painter().text(
                rect.left_top(),
                egui::Align2::LEFT_TOP,
                "Fairing",
                name,
                theme.color(ColorRole::OnSurface),
            );
            ui.painter().text(
                egui::pos2(rect.left(), rect.top() + name_h),
                egui::Align2::LEFT_TOP,
                "Bench console",
                sub,
                theme.color(ColorRole::Muted),
            );
        },
    )
}

fn build(ctx: &egui::Context) -> fairing::Result<fairing::Shell> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut config = ShellConfig::default();
    // The shade splits the way a wide screen's does: the left of the top edge opens
    // the notifications and the right the controls, each a column filling the height of its
    // half — so reaching for Wi-Fi covers half the run, not the run. Each is a card that
    // materialises on its own side, not a curtain drawn down over the page.
    "split".clone_into(&mut config.overlay.layout);
    "card".clone_into(&mut config.overlay.reveal);
    // The brand takes the left slot, so the clock joins the glyphs on the right.
    config.status_bar.left = Vec::new();
    config.status_bar.right = vec![
        "status.clock".to_owned(),
        "status.notifications".to_owned(),
        "status.wifi".to_owned(),
        "status.battery".to_owned(),
    ];
    // **No nav bar.** Back, home and recents are a screen stack's controls, and this console does
    // not have one - the rail is where you are and the page changes under it. Leaving the bar on
    // would put three buttons at the bottom of every page that either do nothing or leave the
    // console entirely.
    config.nav_bar.enabled = false;
    for arg in &args {
        if let Some(name) = arg.strip_prefix("--preset=") {
            name.clone_into(&mut config.theme.preset);
        }
        if let Some(name) = arg.strip_prefix("--theme=") {
            name.clone_into(&mut config.shell.theme);
        }
        // `[overlay] card_glass` — 0 is clear glass, 1 a solid card.
        if let Some(raw) = arg.strip_prefix("--glass=") {
            match raw.parse() {
                Ok(glass) => config.overlay.card_glass = glass,
                Err(_) => log::warn!("--glass={raw} is not a number (0 clear … 1 solid)"),
            }
        }
        // `[overlay] card_relief` — 0 is a flat card, 1 the relief as designed.
        if let Some(raw) = arg.strip_prefix("--relief=") {
            match raw.parse() {
                Ok(relief) => config.overlay.card_relief = relief,
                Err(_) => log::warn!("--relief={raw} is not a number (0 flat … 1 as designed)"),
            }
        }
    }
    // **Twice the crate's default bar.** The default is a phone's - one row of glyphs, sized in
    // millimetres. This one carries two lines of brand, so it is sized by the hand that reaches up
    // to it, which crosses families on purpose and so is written out.
    let spec = fairing::theme::MetricsSpec {
        status_bar_height: fairing::unit::Span::fixed(fairing::unit::Dim::finger(0.95))
            .min(fairing::unit::Dim::du(56.0))
            .reanchored(),
        ..fairing::theme::MetricsSpec::default()
    };
    let mut builder = fairing::Shell::builder(config)
        .fonts(common::korean_fonts())
        .metrics_spec(spec)
        // **All three specs, because a `Theme` is injected below.** `ShellBuilder::theme` is how a
        // product's colours reach the first frame, and a `Theme` carries *resolved* metrics as well
        // as a palette — so handing one over turns off the spec resolution for anything a spec was
        // not also given for. Leave these out and the switch, the slider thumb and every component
        // metric freeze at the default scale: on this panel the switch came out 49 du tall beside
        // rows sized for reading, which looks like a broken widget and is the shell being told to
        // use the numbers it was handed.
        .component_spec(fairing::theme::ComponentSpec::default())
        .control_spec(fairing::theme::ControlSpec::default());
    // **A 15-inch desk panel unless told otherwise.** Without this the density chain falls all the
    // way through to `assume_px_per_mm` and warns, and every physical token comes
    // out sized for a panel a third the real size — which is not a rendering bug, it is the crate
    // correctly drawing for the panel it was told about. An example that ships a size is an example
    // that shows what the tokens do.
    let (mm_w, mm_h) = common::arg_panel_mm(&args).unwrap_or((345.0, 215.0));
    builder = builder.physical_mm(mm_w, mm_h);
    // A bench instrument is read over the work, a little further off than the hand-held 360 mm
    // the crate assumes. Move the anchor and every text-derived token follows. The hand is the
    // crate's default bare finger, which is what reaches over a bench.
    let mut policy = fairing::unit::ScalePolicy::default().with_viewing_distance_mm(420.0);
    if let Some(finger) = common::arg_finger_mm(&args) {
        policy = policy.with_finger_mm(finger);
    }
    builder = builder.scale_policy(policy);
    // **Injected as a `Theme`, not only as a palette pair.** `ShellBuilder::palettes` stores the
    // pair the *toggle* switches between; the first frame still draws the theme built from
    // `[theme] preset`. Handing the theme itself is what makes these the colours on screen from
    // frame one, and the pair keeps a toggle from overturning them.
    let palette = console_palette();
    let theme = fairing::Theme {
        palette,
        dark: false,
        ..fairing::Theme::default()
    };
    // **The Mock backends, so the bar has something to show.** Without them `status.wifi` and
    // `status.battery` are items with no source and draw nothing, which reads as a status bar
    // missing its glyphs rather than as a device that was never told about its radio.
    let mut power = MockPower::new(72, false);
    power.drain_per_min = 3;
    let services = Services::builder()
        .clock(MockClock::running(WallTime {
            utc_secs: 1_704_069_000,
            offset_min: 540,
        }))
        .power(power)
        .wifi(MockWifi::new())
        .bluetooth(MockBluetooth::new())
        .display(MockDisplay::new(70))
        .build();
    let mut shell = builder
        .theme(theme)
        .palettes(palette, palette)
        .services(services)
        .build(ctx)?;
    let state = Rc::new(RefCell::new(Console {
        fold: match args.iter().find_map(|a| a.strip_prefix("--fold=")) {
            Some("away") => layout::Fold::Away,
            _ => layout::Fold::Icons,
        },
        ..Console::default()
    }));
    shell.add(
        screen("console", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            console(ui, cx, &mut state.borrow_mut());
        })
        .title("Console")
        .icon(icon::GAUGE)
        .desktop(),
    );
    shell.add(brand_item());
    // The built-in settings screens, for the Settings page's tiles to open. No desktop icon:
    // this console has no desktop, and the rail is the only way in.
    fairing::settings::add_all(
        &mut shell,
        &fairing::settings::SettingsConfig::default().without_home_icon(),
    );
    shell.launch(fairing::LaunchAction::open("console"));
    Ok(shell)
}

/// The rail down the left and the page beside it — **one `Rail::show` call**.
fn console(ui: &mut egui::Ui, cx: &mut Cx<'_>, state: &mut Console) {
    let open = state.page;
    // Folding is on, and the gesture is read anywhere (the default): a bench has one hand free
    // and it lands where it lands, so a flick across the page folds the arm as readily as a drag
    // across it.
    let out = layout::Rail::new()
        .collapsible(true)
        .fold_to(state.fold)
        .show(
            ui,
            cx,
            |ui, cx| rail(ui, cx, open),
            // Each page says how it comes and goes (`Transit for Page`, above). The body draws
            // the page it is handed — the one on its way out, for as long as its exit plays —
            // never the one the state names.
            |ui, cx| layout::transit(ui, cx, open, |ui, cx, open| page(ui, cx, *open, state)),
        );
    if let Some(Some(i)) = out.picked {
        state.page = i;
    }
}

/// The rail: one `list_item` an entry, with power pinned to the foot.
fn rail(ui: &mut egui::Ui, cx: &mut Cx<'_>, open: Page) -> Option<Page> {
    let mut picked = None;
    layout::action_bar_with(
        ui,
        cx,
        1.4,
        // Full-bleed: the foot holds a row, and a row insets itself.
        layout::Deco::new().container(layout::Container::Divided),
        |ui, cx| {
            layout::page(ui, cx, "console.rail", |ui, cx| {
                for (page, (name, glyph)) in Page::ALL.iter().zip(RAIL) {
                    if layout::list_item(ui, cx, glyph.clone(), name, open == *page).clicked() {
                        picked = Some(*page);
                    }
                }
            });
        },
        |ui, cx| {
            let _ = layout::list_item(ui, cx, icon::POWER, "Power off", false);
        },
    );
    picked
}

fn page(ui: &mut egui::Ui, cx: &mut Cx<'_>, open: Page, state: &mut Console) {
    match open {
        Page::Overview => overview(ui, cx),
        Page::Controls => controls(ui, cx, state),
        Page::Alerts => alerts(ui, cx),
        Page::Storage => storage(ui, cx),
        Page::Settings => settings(ui, cx, state),
    }
}

/// **The page is symbol-led.** A rail that is one word an entry does not want a wall of prose
/// beside it: the eye has already been told where it is, so the page's job is to show the state and
/// offer the few big things a hand reaches for. Cards of body text belong on a settings screen,
/// which this is not.
///
/// One tile: a large glyph over a short label, lit when it is the live one.
fn tile(ui: &mut egui::Ui, cx: &mut Cx<'_>, cell: &layout::Cell<'_, Tile>, lit: bool) {
    let m = &cx.theme.metrics;
    let glyph = (cell.visual.height() * 0.42).min(cell.visual.width() * 0.42);
    let ink = if lit {
        ColorRole::Primary
    } else {
        ColorRole::OnSurface
    };
    let at = egui::Rect::from_center_size(
        egui::pos2(
            cell.visual.center().x,
            cell.visual.center().y - glyph * 0.28,
        ),
        egui::Vec2::splat(glyph),
    );
    let style = crate_icon_style(glyph, ink);
    cx.icons
        .paint(ui.painter(), at, &cell.item.icon, &style, cx.theme);
    // The word in the same ink as the glyph. A quiet tile is a thing to press, not a thing
    // turned off: in `Muted` the eight unlit toggles of the controls page read as disabled.
    let label_y = at.bottom() + m.screen_inset;
    ui.painter().text(
        egui::pos2(cell.visual.center().x, label_y),
        egui::Align2::CENTER_TOP,
        cell.item.label,
        egui::FontId::proportional(m.type_scale.body),
        cx.theme.color(ink),
    );
    // The reading goes under the word, in the display face: on a console the number is the thing
    // being looked at and the word is only there to say what it is.
    if let Some(value) = cell.item.value {
        ui.painter().text(
            egui::pos2(
                cell.visual.center().x,
                label_y + m.type_scale.body + m.screen_inset * 0.5,
            ),
            egui::Align2::CENTER_TOP,
            value,
            cx.theme.display(m.type_scale.heading),
            cx.theme.color(ink),
        );
    }
}

fn crate_icon_style(side: f32, role: ColorRole) -> fairing::icons::IconStyle {
    fairing::icons::IconStyle::sized(side).color(fairing::icons::IconColor::Role(role))
}

/// A tile: a glyph, a word, and — where the tile is reporting rather than offering — a value.
#[derive(Clone)]
struct Tile {
    label: &'static str,
    icon: IconRef,
    /// A short reading under the label. `None` on a tile that is a button rather than a gauge.
    value: Option<&'static str>,
}

impl Tile {
    const fn new(label: &'static str, icon: IconRef) -> Self {
        Self {
            label,
            icon,
            value: None,
        }
    }

    const fn reading(label: &'static str, icon: IconRef, value: &'static str) -> Self {
        Self {
            label,
            icon,
            value: Some(value),
        }
    }
}

/// The overview: the run in progress, then the six things a hand reaches for.
///
/// The run block is the console reference's: a thick ring with the number inside it, and the
/// readings in a column beside it — label small and muted, value in the display face. The ring
/// carries the sweep, so the eye finds the head by colour rather than by remembering where twelve
/// o'clock was.
fn overview(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    let tiles = [
        Tile::new("Start run", icon::ACTIVITY),
        Tile::new("New recipe", icon::PLUS),
        Tile::new("Results", icon::CHART),
        Tile::new("Diagnostics", icon::SHIELD),
        Tile::new("Export", icon::DOWNLOAD),
        Tile::new("Service", icon::WRENCH),
    ];
    layout::page(ui, cx, "overview", |ui, cx| {
        let inset = cx.theme.metrics.content_inset;
        let band = ui
            .available_rect_before_wrap()
            .shrink2(egui::vec2(inset, 0.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(band));
        let ui = &mut ui;
        run_block(ui, cx);
        ui.add_space(cx.theme.metrics.screen_inset);
        layout::Grid::new(3.4, 2.0)
            .max_columns(3)
            .show(ui, cx, &tiles, |ui, cx, cell| {
                tile(ui, cx, &cell, cell.item.label == "Start run");
            });
    });
}

/// The run in progress: the ring, and what it is doing beside it.
fn run_block(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    use fairing::widgets::{LampState, Limit, Meter, ProgressBar, ProgressRing};
    let m = &cx.theme.metrics;
    let diameter = m.row_height * 3.6;
    layout::group(ui, cx, |ui, cx| {
        ui.horizontal(|ui| {
            ui.add_space(cx.theme.metrics.content_inset);
            let _ = ProgressRing::determinate(0.63)
                .value_text("63%")
                .label("Running")
                .diameter(diameter)
                .show(ui, &mut cx.widgets());
            ui.add_space(cx.theme.metrics.content_inset);
            ui.vertical(|ui| {
                let m = &cx.theme.metrics;
                let small = m.type_scale.small;
                let line_gap = cx.theme.control.line_gap;
                let muted = cx.theme.color(ColorRole::Muted);
                // The steps are counted, not measured: a cell each, so "3 of 4" is read at a
                // glance across the bench and the fourth fills when it is reached.
                ui.set_max_width(diameter * 1.8);
                ui.label(egui::RichText::new("Step").size(small).color(muted));
                let _ = ProgressBar::determinate(3.0 / 4.0)
                    .steps(4)
                    .trailing("3 / 4")
                    .show(ui, &mut cx.widgets());
                ui.add_space(line_gap);
                reading(ui, cx, "Current step", "Annealing");
                // The block temperature is a measurement, not a count: a meter, with the
                // band it should sit in and the limit past which it is a fault.
                ui.label(egui::RichText::new("Block").size(small).color(muted));
                let limits = [Limit::high(70.0, LampState::Fault).label("70")];
                let _ = Meter::new(60.0, 20.0..=90.0)
                    .normal(55.0..=65.0)
                    .setpoint(60.0)
                    .limits(&limits)
                    .readout("60.0 °C")
                    .show(ui, &mut cx.widgets());
                ui.add_space(line_gap);
                reading(ui, cx, "Elapsed", "00:12 / 00:30");
            });
        });
    });
}

/// One caption-over-value reading in the run block.
fn reading(ui: &mut egui::Ui, cx: &mut Cx<'_>, label: &str, value: &str) {
    let m = &cx.theme.metrics;
    ui.label(
        egui::RichText::new(label)
            .size(m.type_scale.small)
            .color(cx.theme.color(ColorRole::Muted)),
    );
    ui.label(
        egui::RichText::new(value)
            .font(cx.theme.display(m.type_scale.body))
            .color(cx.theme.color(ColorRole::OnSurface)),
    );
    ui.add_space(cx.theme.control.line_gap);
}

/// The controls: big toggles, not rows of prose. One slider, because a lamp is a scalar and a
/// scalar has no honest tile.
fn controls(ui: &mut egui::Ui, cx: &mut Cx<'_>, state: &mut Console) {
    let toggles = [
        Tile::new("Cooling fan", icon::GAUGE),
        Tile::new("Verbose log", icon::FILE),
        Tile::new("Interlock", icon::SHIELD),
        Tile::new("Printer", icon::PRINTER),
        Tile::new("Network", icon::WIFI),
        Tile::new("Display", icon::DISPLAY),
        Tile::new("Bluetooth", icon::SIGNAL),
        Tile::new("Keyboard", icon::KEYBOARD),
    ];
    layout::page(ui, cx, "controls", |ui, cx| {
        let inset = cx.theme.metrics.content_inset;
        let band = ui
            .available_rect_before_wrap()
            .shrink2(egui::vec2(inset, 0.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(band));
        let ui = &mut ui;
        let (fan, verbose) = (state.fan, state.verbose);
        let mut hit = None;
        layout::Grid::new(2.6, 2.0)
            .max_columns(4)
            .fill_height(1.15)
            .show(ui, cx, &toggles, |ui, cx, cell| {
                let lit = match cell.item.label {
                    "Cooling fan" => fan,
                    "Verbose log" => verbose,
                    _ => false,
                };
                if cell.response.clicked() {
                    hit = Some(cell.item.label);
                }
                tile(ui, cx, &cell, lit);
            });
        match hit {
            Some("Cooling fan") => state.fan = !state.fan,
            Some("Verbose log") => state.verbose = !state.verbose,
            _ => {}
        }
        ui.add_space(cx.theme.metrics.screen_inset);
        layout::group(ui, cx, |ui, cx| {
            let _ = layout::slider_row(
                ui,
                cx,
                "Lamp",
                &mut state.brightness,
                0.0..=100.0,
                "%",
                true,
            );
        });
    });
}

/// The alerts: one tile a thing that could stop a run, lit when it wants attention.
///
/// A list of rows would be the settings-screen answer. On a console the question is "is anything
/// wrong", and nine tiles answer it from across the bench in a way nine subtitles do not.
fn alerts(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    let items = [
        Tile::reading("Lamp hours", icon::GAUGE, "412"),
        Tile::reading("Calibration", icon::SIGNAL, "6 d"),
        Tile::reading("Backup cell", icon::BATTERY, "OK"),
        Tile::reading("Interlock", icon::SHIELD, "Closed"),
        Tile::reading("Stage", icon::ACTIVITY, "Home"),
        Tile::reading("Filter", icon::WRENCH, "Clean"),
        Tile::reading("Printer", icon::PRINTER, "Idle"),
        Tile::reading("Network", icon::WIFI, "Down"),
        Tile::reading("Firmware", icon::DOWNLOAD, "2.4.1"),
    ];
    // Calibration is due and the network is down: those two are lit, the rest are quiet.
    let _ = board(ui, cx, "alerts", &items, |t| {
        matches!(t.label, "Calibration" | "Network")
    });
}

/// The storage: how much is left, then where it went.
fn storage(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    let items = [
        Tile::reading("Internal", icon::HARD_DRIVE, "66 GB"),
        Tile::reading("Exports", icon::DOWNLOAD, "4 GB"),
        Tile::reading("Logs", icon::FILE, "1 GB"),
        Tile::reading("Images", icon::DISPLAY, "48 GB"),
        Tile::reading("Recipes", icon::PLUS, "0.2 GB"),
        Tile::reading("Spool", icon::PRINTER, "Empty"),
        Tile::reading("Free", icon::CHECK, "62 GB"),
        Tile::reading("Backup", icon::SHIELD, "Nightly"),
        Tile::reading("Total", icon::GAUGE, "128 GB"),
    ];
    let _ = board(ui, cx, "storage", &items, |t| t.label == "Free");
}

/// **The settings tiles, each the crate's own screen behind it.** The nine are the built-in
/// settings screens (`settings::add_all`), one tile a screen; a tile opens its screen *in the
/// page*, under a row that leads back to the tiles, rather than on the stack — this console has
/// no nav bar and no back button, so a pushed screen would have had no way home. `Cx::draw_screen`
/// lends the screen for the frame; it is the same screen the demo opens, cards and all.
const SETTINGS: &[(Tile, &str)] = &[
    (Tile::new("Wi-Fi", icon::WIFI), "settings.wifi"),
    (Tile::new("Network", icon::ETHERNET), "settings.network"),
    (
        Tile::new("Bluetooth", icon::BLUETOOTH),
        "settings.bluetooth",
    ),
    (Tile::new("Display", icon::DISPLAY), "settings.display"),
    (Tile::new("Sound", icon::VOLUME), "settings.sound"),
    (Tile::new("Date & time", icon::CLOCK), "settings.datetime"),
    (Tile::new("Language", icon::LANGUAGE), "settings.locale"),
    (Tile::new("Power", icon::BATTERY), "settings.power"),
    (Tile::new("About", icon::INFO), "settings.about"),
];

/// The settings: the places to go, as places rather than as a list of words — and, once one is
/// chosen, the place itself.
fn settings(ui: &mut egui::Ui, cx: &mut Cx<'_>, state: &mut Console) {
    if let Some((label, id)) = state.settings {
        // The way back first, as a rail-style row, then the screen's name — a lent screen draws
        // no chrome of its own — and the screen itself in what is left.
        if layout::list_item(ui, cx, icon::BACK, "All settings", false).clicked() {
            state.settings = None;
        }
        layout::title(ui, cx, label);
        if !cx.draw_screen(ui, id) {
            layout::note(ui, cx, "That screen is not registered.");
        }
        return;
    }
    let items: Vec<Tile> = SETTINGS.iter().map(|(tile, _)| tile.clone()).collect();
    if let Some(hit) = board(ui, cx, "settings", &items, |_| false) {
        state.settings = SETTINGS
            .iter()
            .find(|(tile, _)| tile.label == hit)
            .map(|(tile, id)| (tile.label, *id));
    }
}

/// **The page shape these three share**: a full-bleed board of tiles and nothing else.
///
/// Written once because it is one decision — that a console page is a board — and three copies of
/// it would be three places for that decision to drift, which is the same thing the crate's own
/// layout functions exist to stop an integrator doing.
///
/// One shape, but **one page id each**: the id is the scroll position's identity, and the three
/// boards under one id shared one offset — scrolled halfway on Alerts, Storage opened halfway.
///
/// The label of the tile pressed this frame, where one was.
fn board(
    ui: &mut egui::Ui,
    cx: &mut Cx<'_>,
    id: &str,
    items: &[Tile],
    lit: impl Fn(&Tile) -> bool,
) -> Option<&'static str> {
    let mut hit = None;
    layout::page(ui, cx, ("board", id), |ui, cx| {
        let inset = cx.theme.metrics.content_inset;
        let band = ui
            .available_rect_before_wrap()
            .shrink2(egui::vec2(inset, 0.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(band));
        layout::Grid::new(5.4, 2.0)
            .max_columns(3)
            .fill_height(1.15)
            .show(&mut ui, cx, items, |ui, cx, cell| {
                if cell.response.clicked() {
                    hit = Some(cell.item.label);
                }
                let on = lit(cell.item);
                tile(ui, cx, &cell, on);
            });
    });
    hit
}
