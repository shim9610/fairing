//! **Whose drag it is** — [`claim`], [`claim_if_held`] and [`claimed`] for a control; [`keep`] and
//! [`kept`] for a region.
//!
//! A control that follows the finger — a slider, a switch, a wheel, a text field selecting its
//! text — says so every frame the finger is down on it. A gesture the shell reads straight off
//! the pointer, which takes part in no hit test, asks first and leaves such a drag alone: the
//! rail's fold on a swipe across the page (`fairing::layout::Rail`). Anything else
//! under a finger — a button, a tile, a page scrolling under it — has no meaning of its own for
//! a sideways swipe to take, and the gesture goes ahead.
//!
//! The first build asked egui instead: which widget held the drag, and how tall it was. A dragged
//! widget no taller than two rows was taken for a control and left its drag; a taller one for a
//! scroll area, and swiped over. The guess was wrong both ways. A button senses drags too — its
//! long-press ring has to know the finger moved — and is one row tall, so a swipe that began on
//! a button never folded the rail; a wheel is three rows tall and would have been folded over
//! mid-turn. Only the control knows whether the finger on it means something, so the control
//! says.
//!
//! The claim is a pass number in egui's temporary data. It counts for this pass and the next:
//! a gesture read before the page draws sees the page's claim a frame late, and a claim older
//! than that belongs to a finger that has lifted. Whoever reads it keeps its own memory of the
//! drag from there to the release, because a claim is never made on the release frame.
//!
//! A thing that reads the pointer itself and has no response to hand over — a canvas, a chart
//! being scrubbed, a pad an integrator drew on their own screen — keeps every drag that begins in
//! its rect with [`keep`], said every frame it is drawn; [`kept`] answers for a press point the
//! way [`claimed`] answers for a control. Neither takes anything away from the control
//! or the region: its own presses, drags, pinches and scrolls arrive as they always did. A claim
//! is a note to the gestures around it, not a shield over it.
//!
//! And [`press_point`] says where a press went down, for anything that reads the press itself: a
//! fast finger is already somewhere else by the frame that sees it.

/// Where the claim is kept.
const KEY: &str = "fairing.drag.claim";

/// **This drag is mine** — said every frame the finger is down on a control that follows it.
pub fn claim(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(KEY), pass));
}

/// [`claim`], where `response` has the finger on it: pressed on it and still down, or dragging
/// from it. The usual call — one line, once the response is known.
pub fn claim_if_held(response: &egui::Response) {
    if response.is_pointer_button_down_on() || response.dragged() {
        claim(&response.ctx);
    }
}

/// Whether a control claimed the drag this pass or the last.
#[must_use]
pub fn claimed(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new(KEY)))
        .is_some_and(|at| pass <= at.saturating_add(1))
}

/// Where the kept regions are: the pass each was said in, and its rect.
const KEPT: &str = "fairing.drag.kept";

/// **Every drag that begins in `rect` is yours** — said every frame the region is drawn, by a
/// thing that reads the pointer itself and has no response to hand to [`claim_if_held`].
///
/// It takes nothing from the region. Every press, drag, pinch and scroll reaches it exactly as
/// before; this is a note to whatever reads the raw pointer around it — the rail's page swipe —
/// to leave a drag that began here alone, and it is not a shield.
pub fn keep(ctx: &egui::Context, rect: egui::Rect) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let kept: &mut Vec<(u64, egui::Rect)> = d.get_temp_mut_or_default(egui::Id::new(KEPT));
        kept.retain(|(at, _)| pass <= at.saturating_add(1));
        kept.push((pass, rect));
    });
}

/// Whether a press at `origin` began in a region kept this pass or the last.
#[must_use]
pub fn kept(ctx: &egui::Context, origin: egui::Pos2) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<Vec<(u64, egui::Rect)>>(egui::Id::new(KEPT)))
        .is_some_and(|kept| {
            kept.iter()
                .any(|(at, rect)| pass <= at.saturating_add(1) && rect.contains(origin))
        })
}

/// **Where this frame's press went down** — the point a gesture or a "pressed outside" check
/// starts from, read off the press itself rather than off wherever the pointer is now.
///
/// egui's `interact_pos` is the position of the *last* pointer event of the pass. On a slow frame,
/// or a quick flick on a touch digitizer, the finger comes down and moves on before the frame that
/// sees it runs, and the pass holds both: `interact_pos` is already the moved point, and a press
/// read off it lands wherever the finger got to. A pull begun on the top edge then reads as a press
/// below the edge zone, and is no pull at all (one capture run in seven lost its first
/// pull to this). So this is the pass's last press event, else `press_origin` — where the press
/// still held began. The events come first because a press released within the same pass leaves
/// `press_origin` unset.
#[must_use]
pub fn press_point(input: &egui::InputState) -> Option<egui::Pos2> {
    input
        .events
        .iter()
        .rev()
        .find_map(|e| match e {
            egui::Event::PointerButton {
                pos, pressed: true, ..
            } => Some(*pos),
            _ => None,
        })
        .or(input.pointer.press_origin())
}

#[cfg(test)]
mod tests {
    use super::{claim, claimed, keep, kept, press_point};

    /// A kept region answers for the press points inside it, for this pass and the next.
    #[test]
    fn a_kept_region_holds_its_press_points_this_pass_and_the_next() {
        let ctx = egui::Context::default();
        let input = egui::RawInput::default();
        let pass = |check: &dyn Fn(&egui::Context)| {
            let mut out = ctx.run_ui(input.clone(), |ui| check(ui.ctx()));
            out.textures_delta.clear();
        };
        let region = egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(50.0, 50.0));
        let inside = egui::pos2(20.0, 20.0);
        let outside = egui::pos2(80.0, 80.0);
        pass(&|ctx| {
            keep(ctx, region);
            assert!(
                kept(ctx, inside),
                "a press in the region is kept in the pass it is said"
            );
            assert!(!kept(ctx, outside), "and one outside it is not");
        });
        pass(&|ctx| assert!(kept(ctx, inside), "and in the pass after"));
        pass(&|ctx| assert!(!kept(ctx, inside), "but not the one after that"));
    }

    /// A claim holds for the pass it was made in and the one after, and no longer.
    #[test]
    fn a_claim_lasts_this_pass_and_the_next() {
        let ctx = egui::Context::default();
        let input = egui::RawInput::default();
        // One pass, with the font texture it would hand a backend dropped on purpose.
        let pass = |check: &dyn Fn(&egui::Context)| {
            let mut out = ctx.run_ui(input.clone(), |ui| check(ui.ctx()));
            out.textures_delta.clear();
        };
        assert!(!claimed(&ctx), "nothing has been claimed yet");
        pass(&|ctx| {
            claim(ctx);
            assert!(claimed(ctx), "a claim counts in the pass it is made");
        });
        pass(&|ctx| assert!(claimed(ctx), "and in the pass after it"));
        pass(&|ctx| assert!(!claimed(ctx), "but not the one after that"));
    }

    /// The press point is where the finger came down — in the pass that carried it on as well,
    /// where egui's own `interact_pos` is already the moved point; while it is held after; and in
    /// a pass that pressed and released, where `press_origin` is already gone.
    #[test]
    fn the_press_point_is_where_the_finger_came_down() {
        let ctx = egui::Context::default();
        let (down, moved) = (egui::pos2(10.0, 4.0), egui::pos2(10.0, 60.0));
        let button = |pressed| egui::Event::PointerButton {
            pos: down,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let pass = |events: Vec<egui::Event>, check: &dyn Fn(&egui::InputState)| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| ui.input(|i| check(i)));
            out.textures_delta.clear();
        };
        pass(
            vec![
                egui::Event::PointerMoved(down),
                button(true),
                egui::Event::PointerMoved(moved),
            ],
            &|i| {
                assert_eq!(
                    i.pointer.interact_pos(),
                    Some(moved),
                    "egui's is where it got to"
                );
                assert_eq!(
                    press_point(i),
                    Some(down),
                    "the press point is where it came down"
                );
            },
        );
        pass(
            vec![egui::Event::PointerMoved(egui::pos2(10.0, 90.0))],
            &|i| {
                assert_eq!(
                    press_point(i),
                    Some(down),
                    "held, it is where the press began"
                );
            },
        );
        pass(
            vec![
                egui::Event::PointerButton {
                    pos: egui::pos2(10.0, 90.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                },
                egui::Event::PointerGone,
            ],
            &|i| assert_eq!(press_point(i), None, "let go, there is no press"),
        );
        pass(vec![button(true), button(false)], &|i| {
            assert_eq!(
                i.pointer.press_origin(),
                None,
                "pressed and released in one pass"
            );
            assert_eq!(
                press_point(i),
                Some(down),
                "the press event still says where"
            );
        });
    }
}
