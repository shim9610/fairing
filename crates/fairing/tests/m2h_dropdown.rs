//! **A dropdown keeps one contract whatever it looks like and however it opens** — `Dropdown`.
//!
//! A tap opens it, a row picks, a tap outside closes it without landing on what is under it, and
//! an open list can be scrolled by a finger. Every closed form and every opener is driven from
//! the headless harness here, and the hint at the right of a row is read off the frame.

use fairing::testing::{single_level_access, Harness};
use fairing::widgets::{Dropdown, FieldLook, Opener, TextField, Trigger};
use fairing::{layout, screen, Cx, Shell};

const OPTIONS: [&str; 10] = [
    "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet",
];
/// A shortcut on every row.
const HINTS: [&str; 10] = [
    "Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4", "Ctrl+5", "Ctrl+6", "Ctrl+7", "Ctrl+8", "Ctrl+9",
    "Ctrl+0",
];

/// The closed form and the opener one bench is built with.
#[derive(Debug, Clone, Copy)]
struct Spec {
    trigger: Trigger,
    opener: Opener,
    hints: bool,
}

impl Spec {
    const PLAIN: Self = Self {
        trigger: Trigger::Button,
        opener: Opener::Anchored,
        hints: false,
    };

    const fn opener(opener: Opener) -> Self {
        Self {
            opener,
            ..Self::PLAIN
        }
    }
}

/// The screen's state: the choice, where the trigger was, how often the button under the list
/// was pressed, and a text field under them all that can raise the keyboard first.
struct Bench {
    selected: usize,
    trigger: egui::Rect,
    button_hits: u32,
    button: egui::Rect,
    field: egui::Rect,
    typed: String,
}

fn bench_with(spec: Spec) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("d", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let Some((mut selected, mut typed)) = cx
                .app_mut::<Bench>()
                .map(|b| (b.selected, std::mem::take(&mut b.typed)))
            else {
                return;
            };
            // The dropdown takes the left part; a button sits beside it, outside the list. All of
            // it on a page, as a screen has it: the page's clip is the bound the list is placed
            // in, and a page keeps clear of the keyboard.
            let mut trigger = egui::Rect::NOTHING;
            let mut button = egui::Rect::NOTHING;
            let mut hit = false;
            let mut field = egui::Rect::NOTHING;
            layout::page(ui, cx, "bench", |ui, cx| {
                ui.horizontal(|ui| {
                    ui.scope(|ui| {
                        ui.set_max_width(300.0);
                        let mut d = Dropdown::new("d", &OPTIONS, &mut selected)
                            .label("Letter")
                            .trigger(spec.trigger)
                            .opener(spec.opener);
                        if spec.hints {
                            d = d.hints(&HINTS);
                        }
                        trigger = d.show(ui, &mut cx.widgets()).rect;
                    });
                    ui.add_space(40.0);
                    let r = ui.button("beside");
                    hit = r.clicked();
                    button = r.rect;
                });
                ui.add_space(40.0);
                field = TextField::new(&mut typed)
                    .id_salt("typed")
                    .show(ui, &mut cx.widgets())
                    .rect;
            });
            if let Some(b) = cx.app_mut::<Bench>() {
                b.selected = selected;
                b.trigger = trigger;
                b.button = button;
                b.field = field;
                b.typed = typed;
                if hit {
                    b.button_hits += 1;
                }
            }
        }));
        shell.launch(fairing::LaunchAction::open("d"));
        Ok(shell)
    })?
    .with_app(Bench {
        selected: 0,
        trigger: egui::Rect::NOTHING,
        button_hits: 0,
        button: egui::Rect::NOTHING,
        field: egui::Rect::NOTHING,
        typed: String::new(),
    })
    // Tall enough that a list capped at a fraction of the pane still shows a handful of the
    // harness's gloved, finger-high rows.
    .with_size(1024.0, 1000.0);
    h.frames(20);
    Ok(h)
}

fn bench() -> fairing::Result<Harness> {
    bench_with(Spec::PLAIN)
}

fn state(h: &mut Harness) -> (usize, egui::Rect, egui::Rect, u32) {
    h.app_mut::<Bench>()
        .map_or((0, egui::Rect::NOTHING, egui::Rect::NOTHING, 0), |b| {
            (b.selected, b.trigger, b.button, b.button_hits)
        })
}

/// Every text drawn this frame whose words are an option, with where it starts — **the open
/// list's rows**, not the trigger's own value: that one lies whole inside the trigger, and no
/// opener puts a row there (the search view overlaps the trigger, but its rows start under it).
fn option_texts(h: &mut Harness, trigger: egui::Rect) -> Vec<(String, egui::Pos2)> {
    let mut out = Vec::new();
    for c in h.frame_shapes() {
        if let egui::Shape::Text(t) = c.shape {
            let text = t.galley.job.text.clone();
            let rect = t.galley.rect.translate(t.pos.to_vec2());
            let own = trigger.contains_rect(rect);
            // A scrolling list lays every row out and clips the ones past its edge: only what
            // is on the screen counts.
            let visible = c.clip_rect.intersects(rect);
            if OPTIONS.contains(&text.as_str()) && !own && visible {
                out.push((text, t.pos));
            }
        }
    }
    out
}

/// Where the text `word` is drawn this frame, and how wide it is.
/// Past the open/close/pick motion: the `switch` tween plus the frames the result takes to land,
/// read from the token rather than counted — ten frames once stood in for 140 ms.
fn settled(h: &mut Harness) {
    let switch = h.shell.theme().motion.switch.duration.as_secs_f64();
    h.run_for(switch + 3.0 * fairing::testing::FRAME_DT);
}

fn text_at(h: &mut Harness, word: &str) -> Option<(egui::Pos2, f32)> {
    h.frame_shapes().into_iter().find_map(|c| match c.shape {
        egui::Shape::Text(t) if t.galley.job.text == word => Some((t.pos, t.galley.size().x)),
        _ => None,
    })
}

#[test]
fn the_open_list_scrolls_under_a_finger() -> fairing::Result<()> {
    let mut h = bench()?;
    let (_, trigger, _, _) = state(&mut h);
    assert!(trigger.width() > 0.0, "no trigger");
    h.tap(trigger.center());
    settled(&mut h);
    let before = option_texts(&mut h, trigger);
    assert!(
        before.len() >= 3,
        "the list did not open, or shows fewer than three rows: {before:?}"
    );
    let first_before = before.first().map_or(0.0, |(_, p)| p.y);
    // A finger inside the list, dragged up by two rows.
    let inside = egui::pos2(trigger.center().x, trigger.max.y + 60.0);
    h.drag(inside, egui::pos2(inside.x, inside.y - 120.0), 12);
    settled(&mut h);
    let after = option_texts(&mut h, trigger);
    let first_after = after
        .iter()
        .find(|(t, _)| t == "Alpha")
        .map_or(f32::NAN, |(_, p)| p.y);
    assert!(
        first_after.is_nan() || first_after < first_before - 30.0,
        "dragging up inside the list did not scroll it: Alpha at {first_before} then {first_after}"
    );
    Ok(())
}

#[test]
fn a_tap_outside_closes_the_list_and_lands_on_nothing() -> fairing::Result<()> {
    let mut h = bench()?;
    let (_, trigger, button, _) = state(&mut h);
    h.tap(trigger.center());
    settled(&mut h);
    assert!(
        !option_texts(&mut h, trigger).is_empty(),
        "the list did not open"
    );
    // Tap to the right of the list, on the page.
    let outside = egui::pos2(trigger.max.x + 20.0, trigger.max.y + 200.0);
    h.tap(outside);
    h.frames(5);
    assert!(
        option_texts(&mut h, trigger).is_empty(),
        "a tap outside did not close the list"
    );
    // The button works when nothing is open.
    h.tap(button.center());
    h.frames(5);
    let hits_before = state(&mut h).3;
    assert_eq!(
        hits_before, 1,
        "the button beside the dropdown does not press"
    );
    // Open again and tap the button while the list is up: the list closes and the button is
    // not pressed — the tap was spent on closing.
    h.tap(trigger.center());
    settled(&mut h);
    assert!(
        !option_texts(&mut h, trigger).is_empty(),
        "the list did not reopen"
    );
    h.tap(button.center());
    h.frames(5);
    assert!(
        option_texts(&mut h, trigger).is_empty(),
        "a tap on the button did not close the list"
    );
    let hits_after = state(&mut h).3;
    assert_eq!(
        hits_after, hits_before,
        "the tap that closed the list also pressed the button beside it"
    );
    Ok(())
}

/// **Every closed form opens on a tap and picks on a row.** The forms differ in what they draw,
/// not in what they do.
#[test]
fn every_trigger_opens_on_a_tap_and_a_row_picks() -> fairing::Result<()> {
    let triggers = [
        Trigger::Button,
        Trigger::Field(FieldLook::Outlined),
        Trigger::Field(FieldLook::Filled),
        Trigger::Field(FieldLook::Underlined),
        Trigger::Inline,
        Trigger::Tile,
        Trigger::Chip,
    ];
    for trigger in triggers {
        let mut h = bench_with(Spec {
            trigger,
            ..Spec::PLAIN
        })?;
        let (_, rect, _, _) = state(&mut h);
        assert!(
            rect.width() > 0.0 && rect.height() > 0.0,
            "{trigger:?}: no trigger"
        );
        h.tap(rect.center());
        settled(&mut h);
        let rows = option_texts(&mut h, rect);
        assert!(
            rows.len() >= 3,
            "{trigger:?}: the list did not open: {rows:?}"
        );
        h.tap_text("Charlie")?;
        settled(&mut h);
        let (selected, _, _, _) = state(&mut h);
        assert_eq!(selected, 2, "{trigger:?}: tapping Charlie did not pick it");
        assert!(
            option_texts(&mut h, rect).is_empty(),
            "{trigger:?}: the list stayed open after a pick"
        );
    }
    Ok(())
}

/// **A hint sits at the right of its row**, on the row's own line and inside the list.
#[test]
fn a_hint_sits_at_the_right_of_its_row() -> fairing::Result<()> {
    let mut h = bench_with(Spec {
        hints: true,
        ..Spec::PLAIN
    })?;
    let (_, trigger, _, _) = state(&mut h);
    h.tap(trigger.center());
    settled(&mut h);
    let rows = option_texts(&mut h, trigger);
    let bravo = rows
        .iter()
        .find(|(t, _)| t == "Bravo")
        .map(|(_, p)| *p)
        .unwrap_or_default();
    let (hint, width) = text_at(&mut h, "Ctrl+2").unwrap_or_default();
    assert!(
        (hint.y - bravo.y).abs() < 2.0,
        "the hint is not on Bravo's line: hint {hint:?}, Bravo {bravo:?}"
    );
    assert!(
        hint.x > bravo.x + 40.0,
        "the hint is not to the right of its option: hint {hint:?}, Bravo {bravo:?}"
    );
    // The widest row is narrower than the trigger, so the list is the trigger's width.
    assert!(
        hint.x + width <= trigger.max.x + 1.0,
        "the hint runs past the list's right edge: {} > {}",
        hint.x + width,
        trigger.max.x
    );
    Ok(())
}

/// **A grid cell picks its option.** The cells are laid out in a block, not a column: Golf, the
/// seventh of ten, is not on the seventh row.
#[test]
fn a_grid_cell_picks_its_option() -> fairing::Result<()> {
    let mut h = bench_with(Spec::opener(Opener::Grid))?;
    let (_, trigger, _, _) = state(&mut h);
    h.tap(trigger.center());
    settled(&mut h);
    let cells = option_texts(&mut h, trigger);
    assert!(cells.len() >= 9, "the grid did not open whole: {cells:?}");
    let golf = cells
        .iter()
        .find(|(t, _)| t == "Golf")
        .map(|(_, p)| *p)
        .unwrap_or_default();
    let alpha = cells
        .iter()
        .find(|(t, _)| t == "Alpha")
        .map(|(_, p)| *p)
        .unwrap_or_default();
    // Six rows of the list's own height — the metrics', not a guess at 40 px a row.
    let row = h.shell.theme().metrics.row_height;
    assert!(
        golf.y - alpha.y < 6.0 * row,
        "Golf is six rows under Alpha, so this is a column and not a grid"
    );
    let (_, width) = text_at(&mut h, "Golf").unwrap_or_default();
    h.tap(golf + egui::vec2(width * 0.5, 8.0));
    settled(&mut h);
    assert_eq!(state(&mut h).0, 6, "tapping the Golf cell did not pick it");
    Ok(())
}

/// **A sheet rises from the bottom of the pane**, and a tap on the scrim over the button beside
/// the trigger closes it without pressing that button.
#[test]
fn a_sheet_rises_from_the_bottom_and_its_scrim_takes_the_closing_tap() -> fairing::Result<()> {
    let mut h = bench_with(Spec::opener(Opener::Sheet))?;
    let (_, trigger, button, _) = state(&mut h);
    let screen = h.screen_rect();
    h.tap(trigger.center());
    h.frames(20);
    let rows = option_texts(&mut h, trigger);
    assert!(rows.len() >= 3, "the sheet did not open: {rows:?}");
    let top = rows.iter().map(|(_, p)| p.y).fold(f32::INFINITY, f32::min);
    let bottom = rows.iter().map(|(_, p)| p.y).fold(0.0, f32::max);
    // Not anchored to the trigger: the sheet's rows start well under it, at the pane's bottom.
    assert!(
        top > trigger.max.y + 50.0,
        "the sheet's first row ({top}) hangs off the trigger ({}), so it is anchored and not a sheet",
        trigger.max.y
    );
    assert!(
        bottom > screen.max.y * 0.7,
        "the sheet's last visible row ({bottom}) is nowhere near the bottom of the screen ({})",
        screen.max.y
    );
    h.tap(button.center());
    settled(&mut h);
    assert!(
        option_texts(&mut h, trigger).is_empty(),
        "a tap on the scrim did not close the sheet"
    );
    assert_eq!(
        state(&mut h).3,
        0,
        "the tap that closed the sheet pressed the button under the scrim"
    );
    Ok(())
}

/// **A search narrows the list to what was typed, picks from the matches, and forgets the
/// query when it closes.**
#[test]
fn a_search_narrows_the_list_and_forgets_the_query_when_closed() -> fairing::Result<()> {
    let mut h = bench_with(Spec::opener(Opener::Search))?;
    let (_, trigger, _, _) = state(&mut h);
    let screen = h.screen_rect();
    h.tap(trigger.center());
    settled(&mut h);
    let all = option_texts(&mut h, trigger);
    assert!(
        all.len() >= 3,
        "the search did not open with the list: {all:?}"
    );
    // The matches hang off the trigger itself — under it, or over it where there was no room —
    // in the same panel: the search is the control that was tapped, not a view docked elsewhere
    // on the pane.
    let row = h
        .shell
        .theme()
        .metrics
        .row_height
        .max(h.shell.theme().metrics.touch_target);
    let under = all
        .iter()
        .any(|(_, p)| p.y >= trigger.max.y - 1.0 && p.y <= trigger.max.y + row * 1.5);
    let over = all
        .iter()
        .any(|(_, p)| p.y <= trigger.min.y && p.y >= trigger.min.y - row * 1.5);
    assert!(
        under || over,
        "the matches do not hang off the trigger {trigger:?}: rows at {all:?} (screen {screen:?})"
    );
    // The field is the head, on the trigger: a tap there gives it the keyboard.
    h.tap(trigger.center());
    settled(&mut h);
    h.type_text("jul");
    settled(&mut h);
    let some = option_texts(&mut h, trigger);
    assert_eq!(
        some.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
        vec!["Juliet"],
        "typing \"jul\" did not narrow the list to Juliet"
    );
    h.tap_text("Juliet")?;
    settled(&mut h);
    assert_eq!(state(&mut h).0, 9, "tapping Juliet did not pick it");
    assert!(
        option_texts(&mut h, trigger).is_empty(),
        "the search stayed open after a pick"
    );
    h.tap(trigger.center());
    settled(&mut h);
    let again = option_texts(&mut h, trigger);
    assert!(
        again.len() >= 3,
        "reopened, the search still remembers the old query: {again:?}"
    );
    Ok(())
}

/// **A search can be typed on the on-screen keyboard.** The keyboard comes up when the field
/// takes focus and lies over the pane's bottom, where a page's clip — the list's bound — ends:
/// a key tap has to reach the field, and it must not count as a tap outside the bound that
/// closes the search (which is what the first build did, and what the list drawing in
/// `Order::Foreground` over the keyboard's `Middle` did before that).
#[cfg(feature = "osk")]
#[test]
fn the_on_screen_keyboard_types_into_the_search() -> fairing::Result<()> {
    let mut h = bench_with(Spec::opener(Opener::Search))?;
    let (_, trigger, _, _) = state(&mut h);
    h.tap(trigger.center());
    h.run_for(1.0);
    // Opening is not typing: the list is up and the keyboard is not.
    assert!(
        !h.shell.osk().is_visible(),
        "opening a search raised the keyboard before the field was tapped"
    );
    assert!(
        !option_texts(&mut h, trigger).is_empty(),
        "the search did not open"
    );
    // The field is the head, on the trigger: a tap there gives it the keyboard.
    h.tap(trigger.center());
    h.run_for(1.0);
    assert!(
        h.shell.osk().is_visible(),
        "the keyboard did not come up for the tapped search field"
    );
    // Two keys, not one: the first tap on a key that came up under a fresh shield got through,
    // and it was the second that the shield took as the tap that closes the list.
    for label in ["j", "u"] {
        let key = h
            .shell
            .osk()
            .key_rect(label)
            .ok_or_else(|| fairing::Error::Config(format!("no {label} key on the keyboard")))?;
        h.tap(key.center());
        h.frames(4);
    }
    h.frames(6);
    let rows = option_texts(&mut h, trigger);
    assert_eq!(
        rows.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
        vec!["Juliet"],
        "the taps on the keyboard's j and u did not both reach the search field"
    );
    assert!(
        h.shell.osk().is_visible(),
        "typing on the keyboard closed the search and took the keyboard with it"
    );
    Ok(())
}

/// **The keyboard's keys reach the search even when the keyboard has been up before**, so its
/// area is older than the search's own. egui puts a reappearing area on top of its order by
/// itself; this pins that down, because the whole search rests on it.
#[cfg(feature = "osk")]
#[test]
fn the_keyboard_stays_over_a_search_opened_after_it_first_showed() -> fairing::Result<()> {
    let mut h = bench_with(Spec::opener(Opener::Search))?;
    let (_, trigger, button, _) = state(&mut h);
    let field = h
        .app_mut::<Bench>()
        .map_or(egui::Rect::NOTHING, |b| b.field);
    // The keyboard's first appearance: for the text field, before any search exists.
    h.tap(field.center());
    h.run_for(1.0);
    assert!(
        h.shell.osk().is_visible(),
        "the text field did not raise the keyboard"
    );
    h.tap(button.center());
    h.run_for(2.0);
    assert!(
        !h.shell.osk().is_visible(),
        "the keyboard did not go away when the field lost focus"
    );
    // Now the search, whose shield and panel are the newer areas: opened, then its field tapped.
    h.tap(trigger.center());
    h.run_for(1.0);
    h.tap(trigger.center());
    h.run_for(1.0);
    assert!(
        h.shell.osk().is_visible(),
        "the keyboard did not come back for the search field"
    );
    for label in ["j", "u"] {
        let key = h
            .shell
            .osk()
            .key_rect(label)
            .ok_or_else(|| fairing::Error::Config(format!("no {label} key on the keyboard")))?;
        h.tap(key.center());
        h.frames(4);
    }
    h.frames(6);
    let rows = option_texts(&mut h, trigger);
    assert_eq!(
        rows.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
        vec!["Juliet"],
        "a key tap went to the search's shield instead of its field"
    );
    Ok(())
}

/// **The open panel is the trigger, unrolled**: an outlined field opens into its own
/// focus ring, grown to hold the rows — one stroked silhouette round the trigger and the list,
/// not a card of its own under the trigger.
#[test]
fn the_open_panel_wears_the_triggers_container_as_one_silhouette() -> fairing::Result<()> {
    let mut h = bench_with(Spec {
        trigger: Trigger::Field(FieldLook::Outlined),
        ..Spec::PLAIN
    })?;
    let (_, trigger, _, _) = state(&mut h);
    h.tap(trigger.center());
    h.run_for(1.0);
    let rows = option_texts(&mut h, trigger);
    let Some(first) = rows.first().map(|(_, p)| *p) else {
        return Err(fairing::Error::Config("the list did not open".to_owned()));
    };
    let focus = h.shell.theme().color(fairing::ColorRole::Focus);
    let rings: Vec<egui::Rect> = h
        .frame_shapes()
        .into_iter()
        .filter_map(|c| match c.shape {
            egui::Shape::Rect(r) if r.stroke.width > 0.0 && r.stroke.color == focus => Some(r.rect),
            _ => None,
        })
        .collect();
    let whole: Vec<&egui::Rect> = rings
        .iter()
        .filter(|r| r.contains_rect(trigger) && r.contains(first))
        .collect();
    let [ring] = whole.as_slice() else {
        return Err(fairing::Error::Config(format!(
            "exactly one focus ring holds both the trigger {trigger:?} and the first row at \
             {first:?}: {rings:?}"
        )));
    };
    // And the ring is the field's own: it starts where the trigger starts.
    assert!(
        (ring.min.x - trigger.min.x).abs() < 1.0 && (ring.min.y - trigger.min.y).abs() < 1.0,
        "the ring {ring:?} does not start at the trigger {trigger:?}"
    );
    Ok(())
}

/// A tap on the head — the trigger drawn again at the top of the panel — closes the panel, as a
/// tap on the trigger always did.
#[test]
fn a_tap_on_the_head_closes_the_panel() -> fairing::Result<()> {
    let mut h = bench()?;
    let (before, trigger, _, _) = state(&mut h);
    h.tap(trigger.center());
    h.run_for(1.0);
    assert!(
        !option_texts(&mut h, trigger).is_empty(),
        "the list did not open"
    );
    h.tap(trigger.center());
    h.run_for(1.0);
    assert!(
        option_texts(&mut h, trigger).is_empty(),
        "a tap on the head did not close the list"
    );
    assert_eq!(state(&mut h).0, before, "and it picked nothing");
    Ok(())
}
