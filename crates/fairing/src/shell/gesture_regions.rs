//! The gesture regions' share of the shell: the handles and the integrator's own
//! regions placed each frame, a guard over each, the touch in one told to it to its end, what it
//! asked for run, and the drawing.

use super::{AppRef, Shell};
use crate::gesture::{
    Edge, EdgeMask, Gesture, GestureHandle, GestureRegion, HandleRegion, Phase, Recognizer,
    RegionCx, RegionLook, RegionPlaceCx, RegionTouch, RegionZones, MAX_GESTURE_REGIONS,
    MAX_HANDLES_A_SIDE,
};
use egui::{Id, LayerId, Order, Rect, Sense, Vec2};
use std::time::Instant;

/// The layer the regions are drawn on: above the screens and the shade, under the prompt.
const REGION_LAYER: &str = "fairing.gesture_regions";

/// The guard over each region: an `Area` of its own that takes every press inside it.
const GUARD_AREA: &str = "fairing.gesture_regions.guard";

/// A region in the shell's list: its id, and the handle or the integrator's region it is.
pub(super) struct RegionSlot {
    /// Its id, unique across the handles and the regions.
    id: String,
    /// What it is.
    kind: SlotKind,
}

/// What a region in the list is.
enum SlotKind {
    /// One of the gesture handles.
    Handle(HandleRegion),
    /// One of the integrator's regions.
    Custom(Box<dyn GestureRegion>),
}

impl RegionSlot {
    /// The region, to place, tell or draw.
    fn region(&mut self) -> &mut dyn GestureRegion {
        match &mut self.kind {
            SlotKind::Handle(handle) => handle,
            SlotKind::Custom(region) => region.as_mut(),
        }
    }

    /// The handle it is, where it is one.
    fn handle(&self) -> Option<&GestureHandle> {
        match &self.kind {
            SlotKind::Handle(handle) => Some(&handle.handle),
            SlotKind::Custom(_) => None,
        }
    }
}

/// The touch a region is following.
pub(super) struct RegionTrack {
    /// Its region, by id: the list may change under a touch, and then the touch is nobody's.
    id: String,
    /// The touch as it last stood: what it is told `Cancelled` with when it is taken.
    last: RegionTouch,
}

impl Shell {
    /// **Add a gesture handle**: a thin strip at the edge and the gestures it answers,
    /// the way One Hand Operation+ handles work. One with the same id is replaced.
    ///
    /// Every press that begins in the strip is the handle's: nothing under it sees one,
    /// and the shell's own edge gesture does not start there either. The back gesture starts just
    /// inward of a side handle, and the rest of the edge keeps it. The bottom edge is the nav
    /// bar's or the handles': a bottom handle goes on only with the nav bar off, and
    /// stands aside while the bar is turned on again. See [`GestureHandle`].
    ///
    /// # Errors
    ///
    /// [`Error::Config`](crate::Error::Config) for a handle on the top edge, with no stretch of
    /// its edge, with a way foreign to its edge, or with a thickness, reach or diagonal angle out
    /// of range; for one on the bottom edge while the nav bar is on; for one whose stretch
    /// overlaps another's on the same edge; for a fourth on one edge; and for an id one of your
    /// gesture regions has.
    pub fn add_gesture_handle(&mut self, handle: GestureHandle) -> crate::Result<()> {
        let refuse = |why: String| {
            Err(crate::Error::Config(format!(
                "gesture handle `{}`: {why}",
                handle.id
            )))
        };
        if let Some(fault) = handle.fault() {
            return refuse(fault);
        }
        // The bottom edge is the nav bar's or the handles', not both.
        if handle.edge == Edge::Bottom && self.nav_bar.enabled {
            return refuse(
                "the bottom edge is the nav bar's: turn the nav bar off \
                 (`[nav_bar] enabled = false`) to put a handle there"
                    .to_owned(),
            );
        }
        if self.is_custom_region(&handle.id) {
            return refuse("the id is a gesture region's".to_owned());
        }
        let others = self
            .gesture_regions
            .iter()
            .filter_map(RegionSlot::handle)
            .filter(|h| h.id != handle.id);
        if let Some(other) = others.clone().find(|h| h.overlaps(&handle)) {
            return refuse(format!(
                "overlaps `{}` on the {:?} edge",
                other.id, handle.edge
            ));
        }
        if others.filter(|h| h.edge == handle.edge).count() >= MAX_HANDLES_A_SIDE {
            return refuse(format!(
                "{MAX_HANDLES_A_SIDE} handles on the {:?} edge already",
                handle.edge
            ));
        }
        let slot = RegionSlot {
            id: handle.id.clone(),
            kind: SlotKind::Handle(HandleRegion::new(handle)),
        };
        self.put_region(slot);
        Ok(())
    }

    /// Take a gesture handle off. `false` for an id that was not there.
    pub fn remove_gesture_handle(&mut self, id: &str) -> bool {
        self.take_region(id, true)
    }

    /// The gesture handles' ids, in the order they were added.
    pub fn gesture_handles(&self) -> impl Iterator<Item = &str> {
        self.gesture_regions
            .iter()
            .filter(|slot| slot.handle().is_some())
            .map(|slot| slot.id.as_str())
    }

    /// **Add a gesture region of your own**: a stretch of the glass that takes every
    /// touch beginning in it, placed, told and drawn by `region` — a part of a screen used as a
    /// trackpad, say. One with the same id is replaced.
    ///
    /// Nothing under the region sees a press that begins in it, and no gesture of the shell's
    /// starts from it. The emergency gesture still works through it, and a tap in it still counts
    /// as a hidden entry's corner knock. Where regions overlap, the
    /// one added later is on top: a region added after a handle takes the presses in the strip
    /// it covers. See [`GestureRegion`].
    ///
    /// # Errors
    ///
    /// [`Error::Config`](crate::Error::Config) for an id a gesture handle has, and for a region
    /// past [`MAX_GESTURE_REGIONS`].
    pub fn add_gesture_region(
        &mut self,
        id: impl Into<String>,
        region: impl GestureRegion + 'static,
    ) -> crate::Result<()> {
        let id = id.into();
        let refuse = |why: String| {
            Err(crate::Error::Config(format!(
                "gesture region `{id}`: {why}"
            )))
        };
        if self
            .gesture_regions
            .iter()
            .any(|slot| slot.id == id && slot.handle().is_some())
        {
            return refuse("the id is a gesture handle's".to_owned());
        }
        let others = self
            .gesture_regions
            .iter()
            .filter(|slot| slot.handle().is_none() && slot.id != id)
            .count();
        if others >= MAX_GESTURE_REGIONS {
            return refuse(format!("{MAX_GESTURE_REGIONS} regions already"));
        }
        let slot = RegionSlot {
            id,
            kind: SlotKind::Custom(Box::new(region)),
        };
        self.put_region(slot);
        Ok(())
    }

    /// Take a gesture region of yours off. `false` for an id that was not there.
    pub fn remove_gesture_region(&mut self, id: &str) -> bool {
        self.take_region(id, false)
    }

    /// The ids of your gesture regions, in the order they were added.
    pub fn gesture_regions(&self) -> impl Iterator<Item = &str> {
        self.gesture_regions
            .iter()
            .filter(|slot| slot.handle().is_none())
            .map(|slot| slot.id.as_str())
    }

    /// Whether `id` is one of the integrator's regions.
    fn is_custom_region(&self, id: &str) -> bool {
        self.gesture_regions
            .iter()
            .any(|slot| slot.id == id && slot.handle().is_none())
    }

    /// Put `slot` in the list: in the place of one with its id, or on top. A touch the old one
    /// was following is over.
    fn put_region(&mut self, slot: RegionSlot) {
        self.drop_touch_of(&slot.id);
        if let Some(old) = self.gesture_regions.iter_mut().find(|s| s.id == slot.id) {
            *old = slot;
        } else {
            self.gesture_regions.push(slot);
        }
    }

    /// Take the handle (`handle`) or the region with `id` off. A touch it was following is over.
    fn take_region(&mut self, id: &str, handle: bool) -> bool {
        let before = self.gesture_regions.len();
        self.gesture_regions
            .retain(|slot| slot.id != id || slot.handle().is_some() != handle);
        let gone = self.gesture_regions.len() != before;
        if gone {
            self.drop_touch_of(id);
        }
        gone
    }

    /// The touch the region `id` was following is nobody's: the engine lets the press go.
    fn drop_touch_of(&mut self, id: &str) {
        if self.region_track.as_ref().is_some_and(|t| t.id == id) {
            self.region_track = None;
            self.let_go_of_region_press();
        }
    }

    /// The engine's region touch is over: the rest of the press is passed through, never to be
    /// reported again.
    fn let_go_of_region_press(&mut self) {
        if matches!(self.gestures.recognizer(), Recognizer::Region { .. }) {
            self.gestures.cancel();
        }
    }

    /// Whether the regions are in play: over a screen or the desktop, with nothing of the shell's
    /// over them — the shade, the prompt, the recent screens.
    fn regions_free(&self) -> bool {
        self.overlay.is_closed() && !self.prompt.is_open() && !self.workspace.is_overview_open()
    }

    /// **The keyboard's band** for the regions to keep off: from the top of the keys,
    /// or of where they will stop while they come up, down to the nav bar. `None` while the
    /// keyboard is down.
    fn keys_band(&self, screen: Rect) -> Option<Rect> {
        let floor = self
            .layout
            .nav
            .map_or(screen.max.y, |r| r.min.y.min(screen.max.y));
        let mut top = f32::INFINITY;
        if let Some(keys) = self.layout.osk {
            top = top.min(keys.min.y);
        }
        if self.osk.is_shown() {
            top = top.min(floor - self.osk.height());
        }
        (top < floor).then(|| Rect::from_x_y_ranges(screen.x_range(), top..=floor))
    }

    /// **This frame's region zones**: each region placed where it says, the later on top. None
    /// while the regions are not in play, and none with gestures off: there no region is there at
    /// all, and its presses are the screen's.
    pub(super) fn place_regions(
        &mut self,
        screen: Rect,
        edge_zones: [Rect; 4],
        blocked: EdgeMask,
    ) -> RegionZones {
        let mut placed = RegionZones::NONE;
        if self.gesture_regions.is_empty()
            || !self.regions_free()
            || !self.gestures.tuning().enabled
        {
            return placed;
        }
        let keys = self.keys_band(screen);
        let focused = self
            .workspace
            .focused()
            .map(crate::workspace::Instance::decl_id);
        let cx = RegionPlaceCx {
            screen,
            content: self.layout.content,
            focused,
            pane: if focused.is_some() {
                self.workspace.focused_pane_rect()
            } else {
                self.layout.content
            },
            keys,
            blocked,
            scale: self.scale,
            edge_zones,
            status_bar: self.layout.status,
            nav_bar: self.layout.nav,
            nav_bar_on: self.nav_bar.enabled,
        };
        for (index, slot) in self.gesture_regions.iter_mut().enumerate() {
            let Ok(tag) = u8::try_from(index) else {
                break;
            };
            if let Some(rect) = slot.region().place(&cx).filter(Rect::is_positive) {
                placed.push(rect, tag);
            }
        }
        placed
    }

    /// **Tell the region its touch**, every frame from the press to the end: the engine reports
    /// it. A frame the engine says nothing of it, or one the regions are not in play on (the
    /// shade, the prompt or the cards came over them), the touch is taken and the region hears
    /// it `Cancelled`. What the region asked for runs after it has heard.
    pub(super) fn region_gestures(
        &mut self,
        ctx: &egui::Context,
        gesture: Option<Gesture>,
        now: Instant,
        app: &mut AppRef<'_>,
    ) {
        let reported = match gesture {
            Some(Gesture::Region {
                region,
                origin,
                pos,
                delta,
                velocity,
                held,
                moved,
                phase,
            }) => Some((
                region,
                RegionTouch {
                    phase,
                    origin,
                    pos,
                    delta,
                    velocity,
                    held,
                    moved,
                },
            )),
            _ => None,
        };
        let Some((tag, touch)) = reported.filter(|_| self.regions_free()) else {
            self.take_region_touch(ctx, now, app);
            return;
        };
        match (touch.phase, self.region_track.take()) {
            // A track lasts no longer than the engine's touch, so a press finds none left over.
            (Phase::Started, _) => {
                let Some(id) = self
                    .gesture_regions
                    .get(usize::from(tag))
                    .map(|s| s.id.clone())
                else {
                    return;
                };
                self.tell_region(ctx, &id, &touch, now, app);
                self.region_track = Some(RegionTrack { id, last: touch });
            }
            (phase, Some(mut track)) => {
                self.tell_region(ctx, &track.id, &touch, now, app);
                if phase == Phase::Moved {
                    track.last = touch;
                    self.region_track = Some(track);
                }
            }
            // A press let go within its own frame: the region hears it begin, and end.
            (Phase::Ended | Phase::Cancelled, None) => {
                let Some(id) = self
                    .gesture_regions
                    .get(usize::from(tag))
                    .map(|s| s.id.clone())
                else {
                    return;
                };
                let began = RegionTouch {
                    phase: Phase::Started,
                    ..touch
                };
                self.tell_region(ctx, &id, &began, now, app);
                self.tell_region(ctx, &id, &touch, now, app);
            }
            // A touch nobody follows any more.
            (Phase::Moved, None) => {}
        }
    }

    /// The touch in a region is taken: the engine lets the press go, and the region following it
    /// hears it `Cancelled`.
    fn take_region_touch(&mut self, ctx: &egui::Context, now: Instant, app: &mut AppRef<'_>) {
        self.let_go_of_region_press();
        if let Some(track) = self.region_track.take() {
            self.tell_cancelled(ctx, &track, now, app);
        }
    }

    /// Tell the region `track` follows that its touch is `Cancelled`, where the finger last was.
    fn tell_cancelled(
        &mut self,
        ctx: &egui::Context,
        track: &RegionTrack,
        now: Instant,
        app: &mut AppRef<'_>,
    ) {
        let touch = RegionTouch {
            phase: Phase::Cancelled,
            delta: Vec2::ZERO,
            ..track.last
        };
        self.tell_region(ctx, &track.id, &touch, now, app);
    }

    /// Tell the region `id` the touch, then run what it asked for: the events first, then the
    /// actions, each behind its gate.
    fn tell_region(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        touch: &RegionTouch,
        now: Instant,
        app: &mut AppRef<'_>,
    ) {
        let slop_px = self.gestures.tuning().slop_px;
        let Some(slot) = self.gesture_regions.iter_mut().find(|s| s.id == id) else {
            return;
        };
        let mut cx = RegionCx {
            now,
            scale: self.scale,
            slop_px,
            ctx,
            app: app.as_deref_mut(),
            out: &mut self.region_out,
        };
        slot.region().touch(touch, &mut cx);
        let mut out = std::mem::take(&mut self.region_out);
        self.events.append(&mut out.events);
        for (action, gate) in out.launches.drain(..) {
            match gate {
                Some(gate) if !self.access.allows(&gate) => {
                    self.request_unlock(gate, Some(action));
                }
                _ => self.launch_in(action, app),
            }
        }
        self.region_out = out;
    }

    /// **Lay a guard over each region in play**: an invisible `Area` that takes every
    /// press beginning inside it, the way One Hand Operation+ handles do. Nothing under a region —
    /// a screen's list or button, a desktop page — sees such a press; the engine reads the raw
    /// pointer, so the region still does. Each guard goes to the top of `Foreground` every frame,
    /// in the regions' order, so a popup a screen opens over a region does not take its presses
    /// back.
    ///
    /// egui senses an `Area` at the size it ended the frame before with, and has no public way to
    /// set it, so a guard is a frame behind a region that moves or grows: for the two frames after
    /// the keyboard appears, a side strip's guard still reaches over the outer edge of the keys
    /// beside it — a documented limit.
    pub(super) fn guard_regions(&self, ctx: &egui::Context) {
        for (rect, tag) in self.region_zones.iter() {
            let Some(slot) = self.gesture_regions.get(usize::from(tag)) else {
                continue;
            };
            let id = Id::new(GUARD_AREA).with(&slot.id);
            let _ = egui::Area::new(id)
                .order(Order::Foreground)
                .fixed_pos(rect.min)
                .default_size(rect.size())
                .constrain(false)
                .fade_in(false)
                .interactable(true)
                .sense(Sense::click_and_drag())
                .show(ctx, |ui| ui.allocate_rect(rect, Sense::click_and_drag()));
            ctx.move_to_top(LayerId::new(Order::Foreground, id));
        }
    }

    /// Draw the regions in play — a layer of their own, over the screens and the shade, the later
    /// over the earlier.
    pub(super) fn paint_regions(&mut self, ctx: &egui::Context, now: Instant) {
        let Self {
            region_zones,
            gesture_regions,
            region_track,
            handle_painter,
            scale,
            theme,
            icons,
            ..
        } = self;
        let layer = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new(REGION_LAYER)));
        for (rect, tag) in region_zones.iter() {
            let Some(RegionSlot { id, kind }) = gesture_regions.get_mut(usize::from(tag)) else {
                continue;
            };
            let touch = region_track
                .as_ref()
                .filter(|t| t.id == *id)
                .map(|t| t.last);
            let mut look = RegionLook {
                id,
                rect,
                touch,
                now,
                scale: *scale,
                theme,
                icons: &mut *icons,
                handle_painter: handle_painter.as_mut(),
            };
            match kind {
                SlotKind::Handle(handle) => handle.paint(&layer, &mut look),
                SlotKind::Custom(region) => region.paint(&layer, &mut look),
            }
        }
    }
}
