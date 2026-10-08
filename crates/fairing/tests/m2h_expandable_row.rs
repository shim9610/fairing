//! **A row that opens in place keeps its body reachable only while open, gives a header switch
//! its own patch, reveals what it opened, and takes the "Advanced" row away once tapped** —
//! `layout::ExpandableRow` · `advanced_rows` · `accordion`.

use fairing::testing::{single_level_access, Harness};
use fairing::{layout, screen, Cx, Shell};
use layout::ExpandableRow;

/// The screen's state and what it measured this frame.
struct Bench {
    /// The Display row and the Night light row: open or not.
    open: [bool; 2],
    /// The Night light header's switch, and the switch inside the Display body.
    switches: [bool; 2],
    /// The accordion's open slot.
    which: Option<usize>,
    /// The "Advanced" rows.
    revealed: bool,
    /// How many rows of filler sit above the expandable rows, to push them down the page.
    filler: usize,
    header: egui::Rect,
    night_header: egui::Rect,
    body: Option<egui::Rect>,
    /// The row after the expandable ones, whose y says whether it was pushed down.
    after: egui::Rect,
    /// The "Advanced" row's rect while it shows.
    advanced: egui::Rect,
    /// The accordion rows' headers.
    acc: [egui::Rect; 2],
}

impl Default for Bench {
    fn default() -> Self {
        Self {
            open: [false; 2],
            switches: [false; 2],
            which: None,
            revealed: false,
            filler: 0,
            header: egui::Rect::NOTHING,
            night_header: egui::Rect::NOTHING,
            body: None,
            after: egui::Rect::NOTHING,
            advanced: egui::Rect::NOTHING,
            acc: [egui::Rect::NOTHING; 2],
        }
    }
}

impl Bench {
    /// A copy of the state, for the test to read.
    fn copy(&self) -> Self {
        Self {
            open: self.open,
            switches: self.switches,
            which: self.which,
            revealed: self.revealed,
            filler: self.filler,
            header: self.header,
            night_header: self.night_header,
            body: self.body,
            after: self.after,
            advanced: self.advanced,
            acc: self.acc,
        }
    }
}

/// The screen: a card with filler rows, the two expandable rows and a row after them; a card with
/// an accordion of two; a card with a Wi-Fi row and the Advanced rows.
fn draw(ui: &mut egui::Ui, cx: &mut Cx<'_>, b: &mut Bench) {
    layout::page(ui, cx, "bench", |ui, cx| {
        layout::group(ui, cx, |ui, cx| {
            for i in 0..b.filler {
                layout::info_row(ui, cx, &format!("Filler {i}"), "—");
            }
            let [display, night] = &mut b.open;
            let [night_on, inner_on] = &mut b.switches;
            let out = ExpandableRow::new("display", "Display")
                .subtitle("Resolution, scale")
                .summary("1920 × 1080")
                .show(ui, cx, display, |ui, cx| {
                    layout::info_row(ui, cx, "Scale", "150 %");
                    layout::switch_row(ui, cx, "Inner", None, inner_on, true);
                });
            b.header = out.header.rect;
            b.body = out.body;
            let out = ExpandableRow::new("night", "Night light")
                .switch(night_on)
                .show(ui, cx, night, |ui, cx| {
                    layout::info_row(ui, cx, "Strength", "60 %");
                });
            b.night_header = out.header.rect;
            b.after = layout::info_row(ui, cx, "After", "—").rect;
        });
        layout::group(ui, cx, |ui, cx| {
            for (i, name) in ["Network", "Sound"].iter().enumerate() {
                let mut rect = egui::Rect::NOTHING;
                layout::accordion(&mut b.which, i, |open| {
                    rect = ExpandableRow::new(name, name)
                        .show(ui, cx, open, |ui, cx| {
                            layout::info_row(ui, cx, "Inside", name);
                        })
                        .header
                        .rect;
                });
                if let Some(slot) = b.acc.get_mut(i) {
                    *slot = rect;
                }
            }
        });
        layout::group(ui, cx, |ui, cx| {
            layout::info_row(ui, cx, "Wi-Fi", "fairing-lab");
            let top = ui.cursor().min;
            let showing = !b.revealed;
            layout::advanced_rows(
                ui,
                cx,
                "adv",
                "Advanced",
                &["Proxy", "MAC address"],
                &mut b.revealed,
                |ui, cx| {
                    layout::info_row(ui, cx, "Proxy", "None");
                    layout::info_row(ui, cx, "MAC address", "Randomised");
                },
            );
            b.advanced = if showing {
                egui::Rect::from_min_max(top, egui::pos2(ui.max_rect().max.x, ui.cursor().min.y))
            } else {
                egui::Rect::NOTHING
            };
        });
    });
}

fn bench(filler: usize) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("x", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let Some(mut b) = cx.app_mut::<Bench>().map(std::mem::take) else {
                return;
            };
            draw(ui, cx, &mut b);
            if let Some(slot) = cx.app_mut::<Bench>() {
                *slot = b;
            }
        }));
        shell.launch(fairing::LaunchAction::open("x"));
        Ok(shell)
    })?
    .with_app(Bench {
        filler,
        ..Bench::default()
    })
    // Tall enough that the three cards of finger-high rows all show at once.
    .with_size(1024.0, 1400.0);
    h.frames(20);
    Ok(h)
}

fn snap(h: &mut Harness) -> Bench {
    h.app_mut::<Bench>().map(|b| b.copy()).unwrap_or_default()
}

/// Where the text `word` is drawn this frame, if it is visible.
fn text_at(h: &mut Harness, word: &str) -> Option<egui::Pos2> {
    h.frame_shapes().into_iter().find_map(|c| match c.shape {
        egui::Shape::Text(t)
            if t.galley.job.text == word
                && c.clip_rect
                    .intersects(t.galley.rect.translate(t.pos.to_vec2())) =>
        {
            Some(t.pos)
        }
        _ => None,
    })
}

/// **A tap opens the body under the header and pushes the rows below down; a second tap
/// closes it and they come back up.** Closed, the body's rows are not on the screen.
#[test]
fn a_tap_opens_the_body_and_pushes_the_rows_below() -> fairing::Result<()> {
    let mut h = bench(0)?;
    let before = snap(&mut h);
    assert!(before.header.height() > 0.0, "no header");
    assert!(before.body.is_none(), "closed, there is no body");
    assert!(
        text_at(&mut h, "Scale").is_none(),
        "closed, the body's row is drawn"
    );
    assert!(
        text_at(&mut h, "1920 × 1080").is_some(),
        "closed, the summary shows"
    );
    h.tap(before.header.center());
    h.run_for(1.0);
    let open = snap(&mut h);
    assert!(open.open[0], "the tap did not open the row");
    let body = open.body.unwrap_or(egui::Rect::NOTHING);
    assert!(body.height() > 40.0, "the body did not unroll: {body:?}");
    assert!(
        open.after.min.y > before.after.min.y + 40.0,
        "the row after was not pushed down: {} -> {}",
        before.after.min.y,
        open.after.min.y
    );
    assert!(
        text_at(&mut h, "Scale").is_some(),
        "open, the body's row is not drawn"
    );
    assert!(
        text_at(&mut h, "1920 × 1080").is_none(),
        "open, the summary still shows"
    );
    h.tap(open.header.center());
    h.run_for(1.0);
    let closed = snap(&mut h);
    assert!(
        !closed.open[0] && closed.body.is_none(),
        "the second tap did not close it"
    );
    assert!(
        (closed.after.min.y - before.after.min.y).abs() < 1.0,
        "the rows below did not come back up"
    );
    Ok(())
}

/// **A body's widget takes a tap only while open.** The same spot, tapped closed, does nothing.
#[test]
fn a_closed_body_takes_no_taps() -> fairing::Result<()> {
    let mut h = bench(0)?;
    let s = snap(&mut h);
    h.tap(s.header.center());
    h.run_for(1.0);
    let inner = text_at(&mut h, "Inner").unwrap_or_default();
    let spot = egui::pos2(inner.x + 40.0, inner.y + 8.0);
    h.tap(spot);
    h.frames(5);
    assert!(
        snap(&mut h).switches[1],
        "open, the tap on the inner switch row did not toggle it"
    );
    let header = snap(&mut h).header;
    h.tap(header.center());
    h.run_for(1.0);
    assert!(!snap(&mut h).open[0]);
    // Closed, the same spot is another row now (or nothing) — the inner switch must not move.
    h.tap(spot);
    h.frames(5);
    assert!(
        snap(&mut h).switches[1],
        "closed, a tap where the inner row was reached it"
    );
    Ok(())
}

/// **A switch in the header keeps its own patch**: a tap on it toggles and does not open; a tap
/// on the rest of the row opens and does not toggle.
#[test]
fn a_header_switch_toggles_without_opening_and_the_row_opens_without_toggling(
) -> fairing::Result<()> {
    let mut h = bench(0)?;
    let s = snap(&mut h);
    // The switch sits at the right, before the chevron: an inset, the chevron, a gap.
    let on_switch = egui::pos2(s.night_header.max.x - 110.0, s.night_header.center().y);
    h.tap(on_switch);
    h.run_for(0.5);
    let after = snap(&mut h);
    assert!(
        after.switches[0],
        "a tap on the header's switch did not toggle it"
    );
    assert!(
        !after.open[1],
        "a tap on the header's switch opened the row"
    );
    let on_title = egui::pos2(s.night_header.min.x + 60.0, s.night_header.center().y);
    h.tap(on_title);
    h.run_for(1.0);
    let opened = snap(&mut h);
    assert!(opened.open[1], "a tap on the title did not open the row");
    assert!(opened.switches[0], "a tap on the title toggled the switch");
    Ok(())
}

/// **The body is revealed.** A row near the bottom of the page opens, and the page scrolls just
/// enough for the body's end to show — the header stays on the screen.
#[test]
fn opening_a_row_near_the_bottom_scrolls_its_body_into_view() -> fairing::Result<()> {
    // Enough filler that the header shows near the bottom of the pane and its two-row body
    // would run past it. The pane ends above the nav bar, which takes the last of the screen.
    let mut found = None;
    for filler in 3..24 {
        let mut h = bench(filler)?;
        let s = snap(&mut h);
        let screen_bottom = h.screen_rect().max.y;
        let pane_bottom = h.shell.layout().content.max.y;
        let fits = s.header.max.y < pane_bottom;
        let runs_past = s.header.max.y + 2.0 * s.header.height() > pane_bottom;
        if fits && runs_past {
            found = Some((h, s, screen_bottom));
            break;
        }
    }
    let Some((mut h, s, screen_bottom)) = found else {
        return Err(fairing::Error::Config(
            "no filler count puts the header near the bottom".to_owned(),
        ));
    };
    h.tap(s.header.center());
    h.run_for(1.5);
    let open = snap(&mut h);
    let body = open.body.unwrap_or(egui::Rect::NOTHING);
    // The nav bar takes the bottom of the screen; the pane ends above it.
    let pane_bottom = h.shell.layout().content.max.y;
    assert!(
        body.max.y <= pane_bottom + 0.5,
        "the body's end ({}) is not in the pane (bottom {pane_bottom}, screen {screen_bottom})",
        body.max.y
    );
    assert!(open.header.min.y > 0.0, "the header left the screen");
    assert!(
        open.header.min.y < s.header.min.y,
        "the page did not scroll: header at {} then {}",
        s.header.min.y,
        open.header.min.y
    );
    Ok(())
}

/// **The "Advanced" row goes away once tapped and its rows take its place.**
#[test]
fn the_advanced_row_reveals_its_rows_and_goes_away() -> fairing::Result<()> {
    let mut h = bench(0)?;
    let s = snap(&mut h);
    assert!(
        text_at(&mut h, "Advanced").is_some(),
        "the Advanced row is not drawn"
    );
    assert!(
        text_at(&mut h, "Proxy, MAC address").is_some(),
        "its subtitle does not name the hidden rows"
    );
    assert!(
        text_at(&mut h, "Randomised").is_none(),
        "a hidden row is drawn before the tap"
    );
    h.tap(s.advanced.center());
    h.run_for(1.0);
    assert!(snap(&mut h).revealed);
    assert!(
        text_at(&mut h, "Advanced").is_none(),
        "the Advanced row stayed after the tap"
    );
    assert!(
        text_at(&mut h, "Randomised").is_some(),
        "the hidden rows did not appear"
    );
    Ok(())
}

/// **One at a time**: opening the second closes the first, and closing leaves none open.
#[test]
fn an_accordion_opens_one_and_closes_the_other() -> fairing::Result<()> {
    let mut h = bench(0)?;
    let s = snap(&mut h);
    h.tap(s.acc[0].center());
    h.run_for(1.0);
    assert_eq!(snap(&mut h).which, Some(0));
    let s = snap(&mut h);
    h.tap(s.acc[1].center());
    h.run_for(1.0);
    assert_eq!(
        snap(&mut h).which,
        Some(1),
        "opening the second did not close the first"
    );
    assert!(text_at(&mut h, "Network").is_some());
    let s = snap(&mut h);
    h.tap(s.acc[1].center());
    h.run_for(1.0);
    assert_eq!(
        snap(&mut h).which,
        None,
        "closing the open one left one open"
    );
    Ok(())
}
