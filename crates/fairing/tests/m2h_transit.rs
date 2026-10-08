//! **A page comes and goes the way it says** — `layout::transit`.
//!
//! A screen with several pages behind one rail swapped its body in one frame: the shell's two
//! transitions are about screens and never see a page change inside one. Now the page's identity
//! says how it enters and leaves (`Transit`), and `transit` plays the exit, then the entry, one
//! page on the screen at a time.

use fairing::layout::{self, Motion, Side, Transit};
use fairing::testing::{single_level_access, Harness};
use fairing::widgets::BigButton;
use fairing::{screen, Cx, Shell};

/// The page up, where the page area is, the button on the first page, and its presses.
struct Tabs<K> {
    tab: K,
    area: egui::Rect,
    button: egui::Rect,
    presses: u32,
}

/// The first page's ink and the second's: hues that survive a fade, since a faded fill is
/// premultiplied and every channel scales alike.
const FIRST: egui::Color32 = egui::Color32::from_rgb(200, 40, 40);
const SECOND: egui::Color32 = egui::Color32::from_rgb(40, 160, 40);

/// A screen of two pages under `transit`, keyed by `K`, showing `first` — with the motion
/// reduced, or not.
fn harness<K: Transit + Copy>(first: K, reduce: bool) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut config = single_level_access();
        config.motion.reduce = reduce;
        let mut shell = Shell::builder(config).build(ctx)?;
        shell.add(screen("t", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            cx.with_app::<Tabs<K>, _>(|t, cx| {
                t.area = ui.available_rect_before_wrap();
                let tab = t.tab;
                layout::transit(ui, cx, tab, |ui, cx, tab| {
                    let rect = ui.available_rect_before_wrap();
                    let ink = if *tab == first { FIRST } else { SECOND };
                    ui.painter().rect_filled(rect, 0.0, ink);
                    if *tab == first {
                        let shown = BigButton::new("Go").show(ui, &mut cx.widgets());
                        t.button = shown.response.rect;
                        if shown.response.clicked() {
                            t.presses += 1;
                        }
                    }
                });
            });
        }));
        shell.launch(fairing::LaunchAction::open("t"));
        Ok(shell)
    })?
    .with_app(Tabs {
        tab: first,
        area: egui::Rect::NOTHING,
        button: egui::Rect::NOTHING,
        presses: 0,
    });
    h.set_size(800.0, 480.0);
    h.run_for(1.0);
    Ok(h)
}

/// Whether a fill is the first page's red or the second's green, at any opacity that still reads.
fn is_first(c: egui::Color32) -> bool {
    c.r() > c.g() + 40 && c.r() > c.b() + 40
}
fn is_second(c: egui::Color32) -> bool {
    c.g() > c.r() + 40 && c.g() > c.b() + 40
}

/// A page's paint: its rect, its clip and its alpha.
type Paint = (egui::Rect, egui::Rect, u8);

/// The page in `which`'s ink among one frame's shapes. A page is the one big rect in its ink;
/// a button or a bar is never this large.
fn find(shapes: &[egui::epaint::ClippedShape], which: fn(egui::Color32) -> bool) -> Option<Paint> {
    shapes.iter().find_map(|c| match &c.shape {
        egui::Shape::Rect(r)
            if which(r.fill) && r.rect.width() > 300.0 && r.rect.height() > 150.0 =>
        {
            Some((r.rect, c.clip_rect, r.fill.a()))
        }
        _ => None,
    })
}

/// One frame: the first page's paint and the second's, from the same frame.
fn frame(h: &mut Harness) -> (Option<Paint>, Option<Paint>) {
    let shapes = h.frame_shapes();
    (find(&shapes, is_first), find(&shapes, is_second))
}

/// One frame, and one page's paint on it.
fn page(h: &mut Harness, which: fn(egui::Color32) -> bool) -> Option<Paint> {
    find(&h.frame_shapes(), which)
}

fn switch<K: Transit + Copy>(h: &mut Harness, to: K) {
    if let Some(t) = h.app_mut::<Tabs<K>>() {
        t.tab = to;
    }
}

fn area<K: Transit + Copy>(h: &mut Harness) -> egui::Rect {
    h.app_mut::<Tabs<K>>()
        .map_or(egui::Rect::NOTHING, |t| t.area)
}

/// **The page leaves, then the next arrives, and never both at once.** An index moves along its
/// order: the page before goes up and out, the page after comes up from below, each faded at
/// its far end; at rest the new page is the page area exactly, drawn plainly.
#[test]
fn a_page_leaves_before_the_next_arrives() -> fairing::Result<()> {
    let mut h = harness(0_usize, false)?;
    let area = area::<usize>(&mut h);
    let Some((rest, clip, alpha)) = page(&mut h, is_first) else {
        return Err(fairing::Error::Config("the first page is up".to_owned()));
    };
    assert!(
        (rest.min - area.min).length() < 0.5 && (clip.min - area.min).length() < 0.5 && alpha == 255,
        "at rest the page is the page area, plainly: {rest:?} clipped to {clip:?} at {alpha} in {area:?}"
    );

    switch(&mut h, 1_usize);
    let mut both = 0;
    let mut first_rose = false;
    let mut second_came_from_below = None;
    let mut first_frame_had_first = None;
    for _ in 0..90 {
        let (first, second) = frame(&mut h);
        if first_frame_had_first.is_none() {
            first_frame_had_first = Some(first.is_some() && second.is_none());
        }
        if first.is_some() && second.is_some() {
            both += 1;
        }
        if let Some((r, _, _)) = first {
            first_rose |= r.top() < area.top() - 1.0;
        }
        if let (Some((r, _, a)), None) = (second, second_came_from_below) {
            second_came_from_below = Some((r.top() > area.top() + 1.0, a));
        }
        if second.is_some_and(|(r, _, a)| (r.min - area.min).length() < 0.5 && a == 255) {
            break;
        }
    }
    assert_eq!(
        first_frame_had_first,
        Some(true),
        "on the frame after the switch the first page is still up and the second is not"
    );
    assert_eq!(both, 0, "the two pages are never on the screen together");
    assert!(
        first_rose,
        "the first page has to go up and out on its way to a later page"
    );
    assert!(
        matches!(second_came_from_below, Some((true, a)) if a < 255),
        "the second page has to come up from below, faded: {second_came_from_below:?}"
    );
    h.run_for(0.5);
    let Some((rest, clip, alpha)) = page(&mut h, is_second) else {
        return Err(fairing::Error::Config("the second page is up".to_owned()));
    };
    assert!(
        (rest.min - area.min).length() < 0.5 && (clip.min - area.min).length() < 0.5 && alpha == 255,
        "settled, the second page is the page area, plainly: {rest:?} clipped to {clip:?} at {alpha}"
    );
    assert!(
        page(&mut h, is_first).is_none(),
        "and the first page is gone"
    );
    Ok(())
}

/// **A tap on a page on its way out does nothing.** At rest the button on the first page
/// presses; once the page is leaving, the same tap lands on a page that is already going.
#[test]
fn a_tap_on_a_page_on_its_way_out_does_nothing() -> fairing::Result<()> {
    let mut h = harness(0_usize, false)?;
    let button = h
        .app_mut::<Tabs<usize>>()
        .map_or(egui::Rect::NOTHING, |t| t.button);
    assert!(button.is_positive(), "the button was drawn: {button:?}");
    h.tap(button.center());
    let at_rest = h.app_mut::<Tabs<usize>>().map_or(0, |t| t.presses);
    assert_eq!(at_rest, 1, "at rest the button presses");

    switch(&mut h, 1_usize);
    h.frames(1);
    let moving = h
        .app_mut::<Tabs<usize>>()
        .map_or(egui::Rect::NOTHING, |t| t.button);
    h.tap(moving.center());
    let leaving = h.app_mut::<Tabs<usize>>().map_or(0, |t| t.presses);
    assert_eq!(
        leaving, 1,
        "a tap on the page on its way out has to do nothing"
    );
    Ok(())
}

/// A page identity of the integrator's own: it says how it comes and goes.
#[derive(Clone, Copy, PartialEq)]
enum Lane {
    Left,
    Right,
}

impl Transit for Lane {
    /// Swept in from the left, like a trace.
    fn enter(&self, _from: &Self) -> Motion {
        Motion::sweep(Side::Left)
    }
    /// Gone at once.
    fn exit(&self, _to: &Self) -> Motion {
        Motion::CUT
    }
}

/// **A page says how it comes and goes, and that is what is drawn.** With an instant exit the
/// first page is gone on the very next frame, and the second is revealed from the left: its
/// rect is at rest but its clip is narrower than the page, growing from the left edge.
#[test]
fn a_page_says_how_it_comes_and_goes() -> fairing::Result<()> {
    let mut h = harness(Lane::Left, false)?;
    let area = area::<Lane>(&mut h);
    switch(&mut h, Lane::Right);
    let (first, second) = frame(&mut h);
    assert!(
        first.is_none(),
        "a cut is gone on the next frame: {first:?}"
    );
    let Some((rest, clip, alpha)) = second else {
        return Err(fairing::Error::Config(
            "the second page is up at once".to_owned(),
        ));
    };
    assert!(
        (rest.min - area.min).length() < 0.5 && alpha == 255,
        "a sweep does not move or fade: {rest:?} at {alpha} in {area:?}"
    );
    assert!(
        clip.width() < area.width() - 1.0 && (clip.left() - area.left()).abs() < 0.5,
        "and is cut by an edge sweeping in from the left: clipped to {clip:?} in {area:?}"
    );
    h.run_for(0.5);
    let Some((_, clip, _)) = page(&mut h, is_second) else {
        return Err(fairing::Error::Config(
            "the second page stays up".to_owned(),
        ));
    };
    assert!(
        (clip.right() - area.right()).abs() < 0.5,
        "settled, the whole page shows: {clip:?} in {area:?}"
    );
    Ok(())
}

/// **Under `[motion] reduce` the swap is a cut**: the frame after the switch is the next page,
/// at rest, and nothing of the page before is left.
#[test]
fn reduced_motion_cuts_the_swap() -> fairing::Result<()> {
    let mut h = harness(0_usize, true)?;
    let area = area::<usize>(&mut h);
    switch(&mut h, 1_usize);
    let (first, second) = frame(&mut h);
    assert!(
        first.is_none(),
        "with the motion reduced the first page is gone at once: {first:?}"
    );
    let Some((rest, clip, alpha)) = second else {
        return Err(fairing::Error::Config(
            "the second page is up at once".to_owned(),
        ));
    };
    assert!(
        (rest.min - area.min).length() < 0.5
            && (clip.min - area.min).length() < 0.5
            && alpha == 255,
        "and the second is the page area, plainly: {rest:?} clipped to {clip:?} at {alpha}"
    );
    Ok(())
}

/// A page that arrives rather than one browsed to: it flies in from beyond the right edge and
/// lands with a little give. The home page is cut, so nothing else is in the way.
#[derive(Clone, Copy, PartialEq)]
enum Post {
    Home,
    Alert,
}

impl Transit for Post {
    fn enter(&self, _from: &Self) -> Motion {
        match self {
            Post::Alert => Motion::fly(Side::Right).over(fairing::motion::Tween::back_out(
                std::time::Duration::from_millis(400),
            )),
            Post::Home => Motion::CUT,
        }
    }
    fn exit(&self, _to: &Self) -> Motion {
        Motion::CUT
    }
}

/// **A page can fly in from beyond the edge, and land past its rest before it settles**.
/// On the first frame of its entry the alert page is wholly off the page to the
/// right; with a back-out curve some later frame has it past its rest to the left; settled, it
/// is the page area exactly.
#[test]
fn a_page_can_fly_in_from_beyond_the_edge_and_overshoot() -> fairing::Result<()> {
    let mut h = harness(Post::Home, false)?;
    let area = area::<Post>(&mut h);
    switch(&mut h, Post::Alert);
    let (first, second) = frame(&mut h);
    assert!(first.is_none(), "a cut is gone at once: {first:?}");
    let Some((rect, _, _)) = second else {
        return Err(fairing::Error::Config(
            "the alert page is on its way in".to_owned(),
        ));
    };
    assert!(
        rect.left() >= area.right() - 1.0,
        "on its first frame the flying page is beyond the right edge: {rect:?} of {area:?}"
    );
    let mut past_rest = false;
    for _ in 0..90 {
        let (_, second) = frame(&mut h);
        if let Some((r, _, _)) = second {
            past_rest |= r.left() < area.left() - 1.0;
            if past_rest && (r.min - area.min).length() < 0.5 {
                break;
            }
        }
    }
    assert!(
        past_rest,
        "a back-out lands a little past the rest before it settles"
    );
    h.run_for(0.5);
    let Some((rect, clip, alpha)) = page(&mut h, is_second) else {
        return Err(fairing::Error::Config("the alert page stays up".to_owned()));
    };
    assert!(
        (rect.min - area.min).length() < 0.5
            && (clip.min - area.min).length() < 0.5
            && alpha == 255,
        "settled, it is the page area, plainly: {rect:?} clipped to {clip:?} at {alpha}"
    );
    Ok(())
}
