//! An instance — one running screen.
//!
//! A resident screen stays with its declaration and the instance holds only the declaration id —
//! the screen is taken from the registry for the length of a call and put straight back, so there
//! is one owner, no shared cell and no way to reach it twice. A spawned one owns a
//! `Box<dyn Screen>`. Each instance being drawn borrows a different `LayerId`, so a transition
//! moves the layer whole. The layer ids are a **finite set** (the Pane slot = the stack
//! depth) — egui 0.36's `Areas` never prunes a layer once registered (`memory/mod.rs`
//! `Areas::set_state` only pushes into `order`, and `end_pass` only sorts), so a new id per
//! instance would grow `areas` / `order` for ever on a device left running for days.

use crate::icons::IconRef;
use crate::screen::{BackAction, ChromePolicy, Cx, Lifecycle, Screen, ScreenDecl, SplitSupport};
use crate::theme::ColorRole;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// An instance id. Monotonically increasing within the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstanceId(pub u64);

impl InstanceId {
    /// A context with no instance (actions and status items).
    pub const NONE: Self = Self(0);
}

/// The screen an instance holds.
pub enum InstanceScreen {
    /// Resident — **the declaration owns it.** Taken on loan through
    /// `Registry::take_resident` with the instance's
    /// `decl_id` and given straight back, so there is exactly one owner and no shared cell to
    /// borrow at run time.
    Resident,
    /// Spawned — owned.
    Owned(Box<dyn Screen>),
}

/// One running screen.
pub struct Instance {
    id: InstanceId,
    decl_id: String,
    title: String,
    icon: Option<IconRef>,
    screen: InstanceScreen,
    chrome: ChromePolicy,
    background: Option<ColorRole>,
    /// The declaration's `.split(..)`.
    split: SplitSupport,
    /// Events not yet out through `on_lifecycle`.
    pending: VecDeque<Lifecycle>,
    /// Events already out through `on_lifecycle` and due to go out through the closure's `cx.event`.
    for_closure: VecDeque<Lifecycle>,
    last: Option<Lifecycle>,
    last_ui: Option<Instant>,
    /// The shell time `Stopped` was delivered at (what `evict_after` is judged from). Cleared by `Resumed` / `Paused`.
    stopped_at: Option<Instant>,
    /// The declaration's `.evict_after(..)`.
    evict_after: Option<Duration>,
    /// The borrowed layer slot (= the stack depth inside the Pane). `Workspace::draw_panes` hands it over every frame.
    layer_slot: u32,
    /// The pane size it was last told — `Resized`'s size, or the first one it was put in.
    told: Option<egui::Vec2>,
}

impl std::fmt::Debug for Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Instance")
            .field("id", &self.id)
            .field("decl_id", &self.decl_id)
            .field("last", &self.last)
            .finish_non_exhaustive()
    }
}

impl Instance {
    /// Build from a declaration. `Created` goes into both queues (`on_lifecycle` and `cx.event`) together.
    #[must_use]
    pub fn new(id: InstanceId, decl: &ScreenDecl, screen: InstanceScreen) -> Self {
        let mut pending = VecDeque::new();
        pending.push_back(Lifecycle::Created);
        let mut for_closure = VecDeque::new();
        for_closure.push_back(Lifecycle::Created);
        Self {
            id,
            decl_id: decl.id.clone(),
            title: decl.title.clone(),
            icon: decl.icon.clone(),
            screen,
            chrome: decl.chrome,
            background: decl.background,
            split: decl.split,
            pending,
            for_closure,
            last: None,
            last_ui: None,
            stopped_at: None,
            evict_after: decl.evict_after,
            layer_slot: 0,
            told: None,
        }
    }

    /// id.
    #[must_use]
    pub fn id(&self) -> InstanceId {
        self.id
    }

    /// The declaration id.
    #[must_use]
    pub fn decl_id(&self) -> &str {
        &self.decl_id
    }

    /// The title (a task's name is the root's).
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The icon.
    #[must_use]
    pub fn icon(&self) -> Option<&IconRef> {
        self.icon.as_ref()
    }

    /// Whether — and how small — it may be drawn in a split pane (the declaration's `.split(..)`).
    #[must_use]
    pub fn split_support(&self) -> SplitSupport {
        self.split
    }

    /// The layer this instance has **borrowed right now**. It is drawn as an `Area` at
    /// `Order::Background` — above the panels (the status bar and the nav bar), below `Middle`
    /// (the OSK) and `Foreground` (the shade). The slot can change through
    /// `Instance::set_layer_slot`, so the value is only valid within a frame.
    #[doc(hidden)]
    #[must_use]
    pub fn layer_id(&self) -> egui::LayerId {
        Self::slot_layer_id(self.layer_slot)
    }

    /// The `Area` id = `("fairing.screen", slot)`. The **slot** (the stack depth), not the instance
    /// id — the two instances being drawn (the one coming in and the one going out) are at
    /// different depths, so they do not collide, and the id set stays finite.
    #[must_use]
    pub(crate) fn area_id(&self) -> egui::Id {
        Self::slot_area_id(self.layer_slot)
    }

    /// An `Area` id made from the slot number alone. The layers being a finite set means one can be
    /// named without an instance — used by the slot warm-up and the z-order tests.
    #[must_use]
    pub(crate) fn slot_area_id(slot: u32) -> egui::Id {
        egui::Id::new(("fairing.screen", slot))
    }

    /// An [`egui::LayerId`] made from the slot number alone. See `Instance::slot_area_id`.
    #[doc(hidden)]
    #[must_use]
    pub fn slot_layer_id(slot: u32) -> egui::LayerId {
        egui::LayerId::new(egui::Order::Background, Self::slot_area_id(slot))
    }

    /// Borrow a layer slot (the workspace hands the stack depth over just before drawing).
    pub(crate) fn set_layer_slot(&mut self, slot: u32) {
        self.layer_slot = slot;
    }

    /// The layer slot borrowed right now.
    #[doc(hidden)]
    #[must_use]
    pub fn layer_slot(&self) -> u32 {
        self.layer_slot
    }

    /// Whether it is a spawned (owned) instance. `evict_after` applies only to spawned ones.
    #[must_use]
    pub(crate) fn is_owned(&self) -> bool {
        matches!(self.screen, InstanceScreen::Owned(_))
    }

    /// The declaration's `.evict_after(..)`.
    #[must_use]
    pub fn evict_after(&self) -> Option<Duration> {
        self.evict_after
    }

    /// The shell time `Stopped` was delivered at.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn stopped_at(&self) -> Option<Instant> {
        self.stopped_at
    }

    /// Whether `evict_after` has passed. `now` is the shell's time ([`crate::Shell::now`]).
    ///
    /// There are three conditions:
    /// 1. It is **spawned** and the declaration has `.evict_after(d)` — a resident screen keeps its
    ///    state in the declaration, so taking it down reclaims nothing (`ScreenDecl::evict_after`
    ///    ignores it in the first place).
    /// 2. The last **visibility** notification was `Stopped` — the reference time is
    ///    [`Instance::stopped_at`], and a `Resumed` / `Paused` clears it. `AccessChanged` and
    ///    `Resized` have nothing to do with being visible, so they do not disturb it.
    /// 3. `now − stopped_at ≥ d`. With `Duration::ZERO` it is true on the frame right after the
    ///    `Stopped` notification.
    ///
    /// If the not-yet-delivered queue holds an event that would make it visible again
    /// (`Created` / `Resumed` / `Paused`), the decision is deferred — so as not to take down an
    /// instance that is about to come back on that very frame.
    #[must_use]
    pub(crate) fn evict_due(&self, now: Instant) -> bool {
        let Some(after) = self.evict_after else {
            return false;
        };
        if !self.is_owned() || self.last == Some(Lifecycle::Destroyed) {
            return false;
        }
        if self.pending.iter().any(|event| {
            matches!(
                event,
                Lifecycle::Created | Lifecycle::Resumed | Lifecycle::Paused
            )
        }) {
            return false;
        }
        self.stopped_at
            .is_some_and(|since| now.saturating_duration_since(since) >= after)
    }

    /// The chrome policy.
    #[must_use]
    pub fn chrome(&self) -> ChromePolicy {
        self.chrome
    }

    /// A runtime chrome change.
    pub fn set_chrome(&mut self, policy: ChromePolicy) {
        self.chrome = policy;
    }

    /// The background colour role.
    #[must_use]
    pub fn background(&self) -> Option<ColorRole> {
        self.background.or(self.chrome.background)
    }

    /// The last lifecycle notified.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn last_lifecycle(&self) -> Option<Lifecycle> {
        self.last
    }

    /// Whether it is out of sight (after `Stopped`).
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        matches!(self.last, Some(Lifecycle::Stopped | Lifecycle::Destroyed))
            || self
                .pending
                .back()
                .is_some_and(|e| matches!(e, Lifecycle::Stopped | Lifecycle::Destroyed))
    }

    /// The time of the last `ui` call.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn last_ui_at(&self) -> Option<Instant> {
        self.last_ui
    }

    /// Tell it its pane is `size` — a `Resized`, and the size remembered.
    pub(crate) fn tell_size(&mut self, size: egui::Vec2) {
        self.told = Some(size);
        self.queue(Lifecycle::Resized(size));
    }

    /// Its pane is `size` now: a `Resized` where that is not what it was last told. The first
    /// size it is put in is remembered without one — its first frame already sees it.
    pub(crate) fn keep_size(&mut self, size: egui::Vec2) {
        match self.told {
            None => self.told = Some(size),
            Some(told) if (told - size).length() < 0.5 => {}
            Some(_) => self.tell_size(size),
        }
    }

    /// Queue an event. There are three delivery rules:
    ///
    /// 1. **Only consecutive duplicates are merged.** An event the same as the one right before it
    ///    (or as the last notified, with the queue empty) is dropped. `Paused, Resumed, Paused`
    ///    keeps all three — the same event with something else in between is a separate
    ///    transition. A `Resized` at a different size is a different event.
    /// 2. **Nothing goes in after `Destroyed`.** It is the last notification, and a drop follows it.
    /// 3. The same event goes out to two destinations — `on_lifecycle` (the trait,
    ///    `Instance::flush_lifecycle`) and `cx.event` (the closure,
    ///    `Instance::next_closure_event`). The closure side goes out one per `ui` call, so it has
    ///    to be filled **here** for `Created` to ride the first `ui` (the flush is at the end of the
    ///    frame, which is too late).
    ///
    /// The closure queue builds up while nothing is drawn (which is how a `Stopped` instance coming
    /// back receives the backlog in order). Thanks to the merge rule it does not grow when the same
    /// event repeats.
    pub fn queue(&mut self, event: Lifecycle) {
        let previous = self.pending.back().copied().or(self.last);
        if previous == Some(Lifecycle::Destroyed) || previous == Some(event) {
            return;
        }
        self.pending.push_back(event);
        self.for_closure.push_back(event);
    }

    /// Call the screen this instance runs.
    ///
    /// A factory one is right here in the instance. A resident one lives in its declaration, and
    /// `cx` is the way to it — it takes the screen out for the length of the call and puts it back.
    /// Nothing happens where the declaration went away while the instance was
    /// still up.
    fn with_screen(&mut self, cx: &mut Cx<'_>, f: impl FnOnce(&mut dyn Screen, &mut Cx<'_>)) {
        match &mut self.screen {
            InstanceScreen::Owned(screen) => f(&mut **screen, cx),
            InstanceScreen::Resident => {
                cx.with_resident(&self.decl_id, f);
            }
        }
    }

    /// Send every queued event out through `on_lifecycle` (frame stage 14; an instance being drawn
    /// has [`Instance::ui`] call it first). The closure queue was already filled by
    /// [`Instance::queue`].
    pub(crate) fn flush_lifecycle(&mut self, cx: &mut Cx<'_>) {
        while let Some(event) = self.pending.pop_front() {
            self.last = Some(event);
            match event {
                Lifecycle::Stopped => self.stopped_at = Some(cx.now),
                Lifecycle::Resumed | Lifecycle::Paused => self.stopped_at = None,
                _ => {}
            }
            self.with_screen(cx, |screen, cx| screen.on_lifecycle(event, cx));
        }
    }

    /// The event to ride `cx.event` on the next `ui` call. One per call, in order — `Created`
    /// always comes on the first `ui` (`cx.event == Some(Created)`).
    pub(crate) fn next_closure_event(&mut self) -> Option<Lifecycle> {
        self.for_closure.pop_front()
    }

    /// The screen's `ui`.
    ///
    /// It sends any pending `on_lifecycle` out before drawing — `Created` comes "right after the
    /// instance is created, **before the first `ui`**", and frame stage 14's batch flush
    /// comes after the drawing, too late. Instances that are not drawn (a task gone home, the
    /// graveyard) are left to stage 14 as before.
    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self.flush_lifecycle(cx);
        self.last_ui = Some(cx.now);
        self.with_screen(cx, |screen, cx| screen.ui(ui, cx));
    }

    /// Back.
    pub fn on_back(&mut self, cx: &mut Cx<'_>) -> BackAction {
        let mut action = BackAction::Pop;
        self.with_screen(cx, |screen, cx| action = screen.on_back(cx));
        action
    }

    /// Deliver a child's result.
    pub fn on_result(&mut self, from: &str, value: crate::screen::ScreenValue, cx: &mut Cx<'_>) {
        self.with_screen(cx, |screen, cx| screen.on_result(from, value, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::{Instance, InstanceId, InstanceScreen, Lifecycle};
    use crate::screen::cx::fixture::{pane, Fixture};
    use crate::screen::{screen, screen_with, Cx, Screen, ScreenDecl};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    /// A screen that writes down every notification it receives, in order. `ui` writes itself down too, with its `cx.event`.
    struct Recorder(Rc<RefCell<Vec<String>>>);

    impl Screen for Recorder {
        fn ui(&mut self, _ui: &mut egui::Ui, cx: &mut Cx<'_>) {
            self.0.borrow_mut().push(format!("ui:{:?}", cx.event));
        }

        fn on_lifecycle(&mut self, event: Lifecycle, _cx: &mut Cx<'_>) {
            self.0.borrow_mut().push(format!("life:{event:?}"));
        }
    }

    fn generated_decl(id: &str) -> ScreenDecl {
        screen_with(id, || Recorder(Rc::new(RefCell::new(Vec::new()))))
    }

    fn resident_decl(id: &str) -> ScreenDecl {
        screen(id, |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("x");
        })
    }

    /// A spawned instance sharing the log.
    fn recorder_instance(decl: &ScreenDecl, id: u64) -> (Instance, Rc<RefCell<Vec<String>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let screen = InstanceScreen::Owned(Box::new(Recorder(Rc::clone(&log))));
        (Instance::new(InstanceId(id), decl, screen), log)
    }

    fn spawn_instance(decl: &mut ScreenDecl, id: u64) -> Instance {
        let screen = decl.spawn();
        Instance::new(InstanceId(id), decl, screen)
    }

    /// Drain the whole closure queue.
    fn drain_closure(instance: &mut Instance) -> Vec<Lifecycle> {
        let mut out = Vec::new();
        while let Some(event) = instance.next_closure_event() {
            out.push(event);
        }
        out
    }

    /// Only consecutive duplicates are merged. `Paused, Resumed, Paused` keeps all three.
    #[test]
    fn queue_merges_only_consecutive_duplicates() {
        let mut decl = generated_decl("a");
        let mut instance = spawn_instance(&mut decl, 1);
        instance.queue(Lifecycle::Created); // it merges with the one already put in at creation
        instance.queue(Lifecycle::Resumed);
        instance.queue(Lifecycle::Resumed);
        instance.queue(Lifecycle::Paused);
        instance.queue(Lifecycle::Resumed);
        instance.queue(Lifecycle::Paused);
        assert_eq!(
            drain_closure(&mut instance),
            vec![
                Lifecycle::Created,
                Lifecycle::Resumed,
                Lifecycle::Paused,
                Lifecycle::Resumed,
                Lifecycle::Paused,
            ]
        );
    }

    /// A `Resized` at a different size is a separate event.
    #[test]
    fn queue_treats_different_resizes_as_different_events() {
        let mut decl = generated_decl("a");
        let mut instance = spawn_instance(&mut decl, 1);
        let a = Lifecycle::Resized(egui::vec2(10.0, 10.0));
        let b = Lifecycle::Resized(egui::vec2(20.0, 10.0));
        instance.queue(a);
        instance.queue(a);
        instance.queue(b);
        assert_eq!(drain_closure(&mut instance), vec![Lifecycle::Created, a, b]);
    }

    /// `Destroyed` is the last notification — nothing after it goes into the queue.
    #[test]
    fn queue_ignores_everything_after_destroyed() -> crate::Result<()> {
        let mut decl = generated_decl("a");
        let mut instance = spawn_instance(&mut decl, 1);
        instance.queue(Lifecycle::Destroyed);
        instance.queue(Lifecycle::Resumed);
        instance.queue(Lifecycle::Destroyed);
        assert_eq!(
            drain_closure(&mut instance),
            vec![Lifecycle::Created, Lifecycle::Destroyed]
        );

        // The same once the notifications are finished (the queue empty and `last == Destroyed`).
        let mut fixture = Fixture::new()?;
        let mut parts = fixture.parts();
        let mut cx = parts.cx(pane(instance.id()), None);
        instance.flush_lifecycle(&mut cx);
        assert_eq!(instance.last_lifecycle(), Some(Lifecycle::Destroyed));
        instance.queue(Lifecycle::Resumed);
        assert_eq!(drain_closure(&mut instance), Vec::new());
        Ok(())
    }

    /// `Created` is delivered right after the instance is created, **before the first `ui`** —
    /// the trait receives it through `on_lifecycle` and the closure through `cx.event`.
    #[test]
    fn created_is_delivered_before_the_first_ui() -> crate::Result<()> {
        let decl = generated_decl("a");
        let (mut instance, log) = recorder_instance(&decl, 1);
        instance.queue(Lifecycle::Resumed);

        let mut fixture = Fixture::new()?;
        let ctx = egui::Context::default();
        {
            let mut parts = fixture.parts();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let event = instance.next_closure_event();
                let mut cx = parts.cx(pane(instance.id()), event);
                instance.ui(ui, &mut cx);
            });
            // Headless, there is nothing to consume the texture deltas — not clearing them panics on drop.
            output.textures_delta.clear();
        }
        assert_eq!(
            log.borrow().as_slice(),
            ["life:Created", "life:Resumed", "ui:Some(Created)"],
            "on_lifecycle first, then the first ui sees Created"
        );

        // The second `ui` receives the pending `Resumed` (one per call).
        log.borrow_mut().clear();
        {
            let mut parts = fixture.parts();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let event = instance.next_closure_event();
                let mut cx = parts.cx(pane(instance.id()), event);
                instance.ui(ui, &mut cx);
            });
            // Headless, there is nothing to consume the texture deltas — not clearing them panics on drop.
            output.textures_delta.clear();
        }
        assert_eq!(log.borrow().as_slice(), ["ui:Some(Resumed)"]);
        assert_eq!(instance.last_lifecycle(), Some(Lifecycle::Resumed));
        Ok(())
    }

    /// One turn round the lifecycle state diagram: `Created → Resumed → Paused → Stopped →
    /// Resumed → AccessChanged → Destroyed`. The trait (`on_lifecycle`) and the closure (`cx.event`) receive
    /// them in **the same order**, and `Destroyed` is last.
    #[test]
    fn lifecycle_sequence_follows_the_state_diagram() -> crate::Result<()> {
        let decl = generated_decl("a");
        let (mut instance, log) = recorder_instance(&decl, 1);
        let mut fixture = Fixture::new()?;

        // The notifications go out per frame — the queue is filled and flushed each frame.
        let frames = [
            vec![Lifecycle::Resumed],
            vec![Lifecycle::Paused],
            vec![Lifecycle::Stopped],
            vec![Lifecycle::Resumed],
            vec![Lifecycle::AccessChanged, Lifecycle::Destroyed],
        ];
        for events in frames {
            for event in events {
                instance.queue(event);
            }
            let mut parts = fixture.parts();
            let mut cx = parts.cx(pane(instance.id()), None);
            instance.flush_lifecycle(&mut cx);
        }

        assert_eq!(
            log.borrow().as_slice(),
            [
                "life:Created",
                "life:Resumed",
                "life:Paused",
                "life:Stopped",
                "life:Resumed",
                "life:AccessChanged",
                "life:Destroyed",
            ]
        );
        assert_eq!(instance.last_lifecycle(), Some(Lifecycle::Destroyed));
        assert_eq!(
            drain_closure(&mut instance),
            vec![
                Lifecycle::Created,
                Lifecycle::Resumed,
                Lifecycle::Paused,
                Lifecycle::Stopped,
                Lifecycle::Resumed,
                Lifecycle::AccessChanged,
                Lifecycle::Destroyed,
            ],
            "the closure gets them in the same order"
        );
        Ok(())
    }

    /// The `evict_due` boundary: spawned + the last visibility notification `Stopped` + elapsed ≥ `evict_after`.
    #[test]
    fn evict_due_boundary_and_reset() -> crate::Result<()> {
        let after = Duration::from_millis(100);
        let mut decl = generated_decl("a").evict_after(after);
        let mut instance = spawn_instance(&mut decl, 1);
        let mut fixture = Fixture::new()?;
        let t0 = fixture.now;

        // Only `Created` has been notified — it has never even been visible, so it is not a candidate.
        {
            let mut parts = fixture.parts();
            let mut cx = parts.cx(pane(instance.id()), None);
            instance.flush_lifecycle(&mut cx);
        }
        assert!(!instance.evict_due(t0 + after));

        // The time `Stopped` was notified becomes the reference.
        instance.queue(Lifecycle::Stopped);
        {
            let mut parts = fixture.parts();
            let mut cx = parts.cx(pane(instance.id()), None);
            instance.flush_lifecycle(&mut cx);
        }
        assert_eq!(instance.stopped_at(), Some(t0));
        assert!(
            !instance.evict_due(t0 + Duration::from_millis(99)),
            "just short of the boundary"
        );
        assert!(
            instance.evict_due(t0 + after),
            "the boundary is included (≥)"
        );

        // A notification unrelated to being out of sight (`AccessChanged`) disturbs neither the reference nor the decision.
        instance.queue(Lifecycle::AccessChanged);
        assert!(
            instance.evict_due(t0 + after),
            "AccessChanged does not put the decision off"
        );
        {
            let mut parts = fixture.parts();
            let mut cx = parts.cx(pane(instance.id()), None);
            instance.flush_lifecycle(&mut cx);
        }
        assert!(
            instance.evict_due(t0 + after),
            "AccessChanged does not clear the reference point either"
        );

        // `Resumed` clears the reference.
        instance.queue(Lifecycle::Resumed);
        assert!(
            !instance.evict_due(t0 + after),
            "it waits while an event that would revive it is in the queue"
        );
        {
            let mut parts = fixture.parts();
            let mut cx = parts.cx(pane(instance.id()), None);
            instance.flush_lifecycle(&mut cx);
        }
        assert_eq!(instance.stopped_at(), None);
        assert!(!instance.evict_due(t0 + Duration::from_secs(10)));
        Ok(())
    }

    /// `Duration::ZERO` is true at the first decision after the `Stopped` notification.
    #[test]
    fn evict_due_with_zero_duration_is_immediate() -> crate::Result<()> {
        let mut decl = generated_decl("a").evict_after(Duration::ZERO);
        let mut instance = spawn_instance(&mut decl, 1);
        let mut fixture = Fixture::new()?;
        let t0 = fixture.now;
        instance.queue(Lifecycle::Stopped);
        {
            let mut parts = fixture.parts();
            let mut cx = parts.cx(pane(instance.id()), None);
            instance.flush_lifecycle(&mut cx);
        }
        assert!(instance.evict_due(t0));
        Ok(())
    }

    /// A resident screen is never a candidate — the declaration ignores `evict_after`, and the decision looks at `is_owned` too.
    #[test]
    fn evict_due_never_fires_for_resident_instances() -> crate::Result<()> {
        let mut decl = resident_decl("a").evict_after(Duration::ZERO);
        assert_eq!(
            decl.evict_after, None,
            "a resident declaration ignores evict_after"
        );
        let mut instance = spawn_instance(&mut decl, 1);
        assert!(!instance.is_owned());
        let mut fixture = Fixture::new()?;
        let t0 = fixture.now;
        instance.queue(Lifecycle::Stopped);
        {
            let mut parts = fixture.parts();
            let mut cx = parts.cx(pane(instance.id()), None);
            instance.flush_lifecycle(&mut cx);
        }
        assert!(!instance.evict_due(t0 + Duration::from_mins(1)));
        Ok(())
    }

    /// Without `evict_after` it stays alive however long it has been stopped (the default).
    #[test]
    fn evict_due_is_false_without_evict_after() -> crate::Result<()> {
        let mut decl = generated_decl("a");
        let mut instance = spawn_instance(&mut decl, 1);
        let mut fixture = Fixture::new()?;
        let t0 = fixture.now;
        instance.queue(Lifecycle::Stopped);
        {
            let mut parts = fixture.parts();
            let mut cx = parts.cx(pane(instance.id()), None);
            instance.flush_lifecycle(&mut cx);
        }
        assert!(!instance.evict_due(t0 + Duration::from_hours(1)));
        Ok(())
    }

    /// A layer id follows the **slot**, not the instance.
    #[test]
    fn area_id_follows_the_layer_slot_not_the_instance() {
        let mut decl = generated_decl("a");
        let mut first = spawn_instance(&mut decl, 1);
        let mut second = spawn_instance(&mut decl, 2);
        first.set_layer_slot(0);
        second.set_layer_slot(0);
        assert_eq!(
            first.area_id(),
            second.area_id(),
            "the same slot = the same layer"
        );
        second.set_layer_slot(1);
        assert_ne!(first.area_id(), second.area_id());
        assert_eq!(second.layer_slot(), 1);
        assert_eq!(first.layer_id().order, egui::Order::Background);
        assert_eq!(first.layer_id().id, first.area_id());
    }

    /// `is_stopped` looks at the queue as well as what has been notified.
    #[test]
    fn is_stopped_looks_at_the_queue_too() {
        let mut decl = generated_decl("a");
        let mut instance = spawn_instance(&mut decl, 1);
        assert!(!instance.is_stopped());
        instance.queue(Lifecycle::Stopped);
        assert!(
            instance.is_stopped(),
            "it stops shortly even before the notification"
        );
        instance.queue(Lifecycle::Resumed);
        assert!(!instance.is_stopped());
    }

    /// It inherits the declaration's title, icon, chrome and background, and the background comes back through the chrome too.
    #[test]
    fn instance_inherits_the_declaration() {
        let mut decl = generated_decl("a")
            .title("A")
            .icon(crate::icons::IconRef::Builtin("gauge"))
            .background(crate::theme::ColorRole::Surface);
        let instance = spawn_instance(&mut decl, 9);
        assert_eq!(instance.id(), InstanceId(9));
        assert_eq!(instance.decl_id(), "a");
        assert_eq!(instance.title(), "A");
        assert!(instance.icon().is_some());
        assert_eq!(
            instance.background(),
            Some(crate::theme::ColorRole::Surface)
        );
        assert_eq!(instance.last_ui_at(), None);
        assert_eq!(instance.last_lifecycle(), None);
    }

    /// A runtime chrome change (`cx.set_chrome`).
    #[test]
    fn set_chrome_replaces_the_policy() {
        let mut decl = generated_decl("a");
        let mut instance = spawn_instance(&mut decl, 1);
        assert_eq!(instance.chrome(), crate::screen::ChromePolicy::default());
        instance.set_chrome(crate::screen::ChromePolicy::fullscreen());
        assert_eq!(instance.chrome(), crate::screen::ChromePolicy::fullscreen());
    }
}
