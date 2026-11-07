//! Transitions for `Overlay`.

use super::*;

impl Overlay {
    /// The driving value y (px).
    #[must_use]
    pub fn y(&self) -> f32 {
        self.shade.y()
    }
    /// The height H.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.shade.height()
    }
    /// `y / H`.
    #[must_use]
    pub fn progress(&self) -> f32 {
        self.shade.progress()
    }
    /// The scrim alpha, `0.5 × y/H` — or, on a two-step shade, `0.5 ×` the travel **past the
    /// stop**, so the tiles come down over an undimmed screen. See `Shade::scrim_progress`.
    #[doc(hidden)]
    #[must_use]
    pub fn scrim_alpha(&self) -> f32 {
        scrim::alpha(self.shade.scrim_progress())
    }
    /// The status bar icons' opacity, `1 − clamp(y/64)` (A1).
    #[doc(hidden)]
    #[must_use]
    pub fn status_bar_opacity(&self) -> f32 {
        (1.0 - (self.shade.y() / 64.0).clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }
    /// Whether it is fully closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.shade.is_closed()
    }
    /// Whether it is fully open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.shade.is_open()
    }
    /// Whether it is moving (the shade spring, or a notification row springing back).
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.shade.is_animating()
            || self.swipe_x.is_animating()
            || self.collapse.is_animating()
            || self.card_lagging
            || self.fade.is_some()
    }
    /// Open imperatively (`LaunchAction::OpenOverlay`, an emergency gesture). A split shade opens
    /// the panel on the side of the latest press — the status bar tap that asked for it.
    pub(crate) fn open(&mut self, tokens: &MotionTokens) {
        if self.shade.is_closed() {
            self.latch_panel();
        }
        self.shade.open(tokens);
    }
    /// Close (back, the scrim, or a session downgrade).
    pub(crate) fn close(&mut self, tokens: &MotionTokens) {
        self.shade.close(tokens);
    }
    /// **Which split panel this pull or tap opens** — by where the press that started it landed,
    /// left or right of `split_ratio`. Decided once, while the shade is shut, and
    /// kept until it shuts again; with no press to go by, the last panel shown.
    pub(super) fn latch_panel(&mut self) {
        self.fade = None;
        if let Some(side) = self.side_of_press() {
            self.panel = side;
        }
    }
    /// Under `split`, the panel whose side of the divide the latest press landed on.
    pub(super) fn side_of_press(&self) -> Option<OverlayPanel> {
        if self.layout != OverlayLayout::Split {
            return None;
        }
        let p = self.last_press?;
        let divide = self.screen.min.x + self.screen.width() * self.split_ratio;
        Some(if p.x < divide {
            OverlayPanel::Notifications
        } else {
            OverlayPanel::ControlCenter
        })
    }
    /// Under `split`, with a panel showing: the other panel, if the latest press landed on its
    /// side — a pull or a bar tap there asks for it.
    pub(super) fn other_side(&self) -> Option<OverlayPanel> {
        self.side_of_press().filter(|side| *side != self.panel)
    }
    /// **A status bar tap while the shade is open**: under `split`, a tap on the other
    /// panel's side of the bar crosses to it, and `true` says it did; anywhere else it is the
    /// shell's to close, as it always was.
    pub(crate) fn bar_tapped(&mut self, tokens: &MotionTokens) -> bool {
        if self.shade.is_closed() {
            return false;
        }
        let Some(other) = self.other_side() else {
            return false;
        };
        self.cross_to(other, tokens);
        self.shade.open(tokens);
        true
    }
    /// **Cross to the other split panel** — a pull or a status bar tap on its side while this one
    /// shows (it replaces the old header segment, so each panel holds only its own).
    ///
    /// What is left of this panel dissolves where it stands, from the plate it was last drawn on,
    /// under the new one. The new one comes in from shut, as a first open does — under the finger
    /// for a pull, on the open spring for a tap — so crossing and opening look the same. What a
    /// press on the old panel had started is dropped with it.
    pub(super) fn cross_to(&mut self, to: OverlayPanel, tokens: &MotionTokens) {
        let leaving = self.panel;
        self.panel = to;
        self.card_side = match to {
            OverlayPanel::Notifications => Some(CardSide::Left),
            OverlayPanel::ControlCenter => Some(CardSide::Right),
            OverlayPanel::Shade => self.card_side,
        };
        self.swiping = None;
        self.swipe_x.snap(0.0);
        self.scroll_to = None;
        self.grab = Grab::None;
        self.card_p = 0.0;
        self.card_lagging = false;
        self.shade.reset_closed();
        self.fade = match (self.last_plate, tokens.reduce) {
            (Some(plate), false) => {
                self.fade_t.snap(0.0);
                self.fade_t.to(1.0, tokens.split_crossfade);
                Some((leaving, plate))
            }
            _ => None,
        };
    }
    /// Last frame's output.
    #[must_use]
    pub fn frame(&self) -> OverlayFrame {
        self.frame
    }
    /// The tile ids, in order.
    #[must_use]
    pub fn tile_ids(&self) -> &[String] {
        &self.tile_ids
    }
    /// Remove one quick-settings tile. `false` if it was not there.
    ///
    /// The built-in tiles come from the `[overlay] tiles` list rather than the declaration registry
    /// — which is why, while [`Shell::remove`](crate::Shell::remove) looked only at the registry and
    /// the status bar, there was no way at all to remove `tile.wifi`. `Shell::remove`
    /// now looks this far.
    pub(crate) fn remove_tile(&mut self, id: &str) -> bool {
        let before = self.tile_ids.len();
        self.tile_ids.retain(|t| t != id);
        let removed = self.tile_ids.len() != before;
        if removed {
            self.mark_tiles_dirty();
        }
        removed
    }
    /// **The state a tile is drawn in** as of the last frame — on, off, a slider value, or
    /// unavailable.
    ///
    /// Read by the integrator to check that a [`TileDecl::lit_by`] key is reaching the tile, and by
    /// the tests instead of matching colours.
    #[must_use]
    pub fn tile_state(&self, id: &str) -> Option<TileState> {
        self.tiles
            .iter()
            .find(|(t, _, _)| t.id == id)
            .map(|(_, state, _)| *state)
    }
    /// **Where the tile block ended last frame**, as a height from the shade's own top — the stop
    /// a two-step shade rests on.
    ///
    /// `None` until the panel has drawn once, which makes the shade one-step on its first pull and
    /// two-step from then on. That is the safe way round: a stop guessed at the wrong height is a
    /// shade that sticks somewhere meaningless, and there is nothing to guess from before the
    /// tiles have been laid out.
    pub(super) fn tiles_bottom(&self) -> Option<f32> {
        let top = self.frame.panel?.min.y;
        // An expanded row (a slider, a panel body) is part of the tile block: the stop moves
        // down to take it in, and back up when it folds. Left out, the row opened under the
        // fold and was cut off there.
        let expanded = Some(self.slider_rect)
            .filter(Rect::is_positive)
            .map_or(f32::NEG_INFINITY, |r| r.max.y);
        let bottom = self
            .tile_rects
            .iter()
            .filter(|r| r.is_positive())
            .map(|r| r.max.y)
            .fold(expanded, f32::max);
        // The gap below the tiles is the one that sits *between* them, so the stop does not cut the
        // block off flush against the fold. A card keeps the air it has above them.
        let air = match self.reveal {
            OverlayReveal::Curtain => self.tile_gap * 2.0,
            OverlayReveal::Card => self.card_air,
        };
        (bottom.is_finite() && bottom > top).then_some(bottom - top + air)
    }
    /// The tile Rects drawn last frame (so tests need not duplicate the layout arithmetic).
    #[must_use]
    pub fn tile_rect(&self, id: &str) -> Option<Rect> {
        let i = self.tiles.iter().position(|(t, _, _)| t.id == id)?;
        self.tile_rects.get(i).copied().filter(Rect::is_positive)
    }
    /// The **top notification's close hit Rect** as drawn last frame.
    ///
    /// Wider than the × drawn — at a multiple of `shade.close` it comes to 9.4 mm under a gloved
    /// finger, short of `finger_hard_mm` (10.1 mm), so the drawing is left as it is and only the
    /// place pressed widens to `touch_target` (hit size and drawn size kept apart). Exposed here so
    /// that tests need not duplicate the layout arithmetic.
    #[doc(hidden)]
    #[must_use]
    pub fn close_hit_rect(&self) -> Option<Rect> {
        Some(self.close_hit).filter(Rect::is_positive)
    }
    /// The Rect of the expanded row that is open (a Slider track, or a [`TileKind::Panel`] body).
    /// `None` where there is none.
    ///
    /// **A drag started here is not taken by the shade** — which is what makes it possible to put a
    /// slider in a row. Exposed so that tests need not duplicate the layout arithmetic.
    #[doc(hidden)]
    #[must_use]
    pub fn expanded_rect(&self) -> Option<Rect> {
        Some(self.slider_rect).filter(Rect::is_positive)
    }
    /// The notification being removed right now and **the fraction of its place left** (1 = still
    /// at full height, 0 = fully collapsed).
    ///
    /// It has a value only while the place is collapsing, after the card has flown out sideways. On
    /// the frame it reaches 0 the notification really leaves the list — until then the rows below
    /// rise to fill the gap.
    #[doc(hidden)]
    #[must_use]
    pub fn dismissing(&self) -> Option<(NotificationId, f32)> {
        self.collapsing.map(|id| (id, self.collapse.value()))
    }
    /// Last frame's notification list Rect (while the panel was visible). Where the hand-off test presses.
    #[doc(hidden)]
    #[must_use]
    pub fn list_rect(&self) -> Option<Rect> {
        Some(self.list_rect).filter(Rect::is_positive)
    }
    /// Last frame's notification list scroll offset (px).
    #[doc(hidden)]
    #[must_use]
    pub fn list_offset(&self) -> f32 {
        self.list_offset
    }
    /// The expanded Slider tile's id.
    #[must_use]
    pub fn expanded_tile(&self) -> Option<&str> {
        self.expanded
            .and_then(|i| self.tiles.get(i))
            .map(|(t, _, _)| t.id.as_str())
    }
    /// This frame's tile list (id, state, allowed).
    #[must_use]
    pub fn tiles(&self) -> &[(QuickTile, TileState, bool)] {
        &self.tiles
    }
    /// A declaration changed — rebuild the tile list next frame.
    pub(crate) fn mark_tiles_dirty(&mut self) {
        self.tiles_dirty = true;
    }
    /// The time left on the peek (for scheduling the idle repaint).
    pub(crate) fn peek_remaining(&mut self, now: Instant) -> Option<Duration> {
        self.shade.peek_remaining(now)
    }
    /// Frame stage 5: consume the engine's top edge swipe and advance. `true` if it consumed it (a
    /// lower priority then does not start). It consumes a top pull while open as well (a rubber band
    /// downwards).
    pub(crate) fn update(&mut self, input: &OverlayInput, tokens: &MotionTokens) -> bool {
        self.status_hidden = input.status_hidden;
        self.dt = input.dt;
        self.screen = input.screen;
        // A split shade's panel is picked as the pull starts, before its height is worked out.
        if let Some(Gesture::EdgeSwipe {
            edge: Edge::Top,
            phase: Phase::Started,
            ..
        }) = input.gesture
        {
            if input.allowed && self.shade.is_closed() {
                self.latch_panel();
            } else if input.allowed {
                // A pull starting on the other panel's side while one is showing brings that one
                // in under the finger.
                if let Some(other) = self.other_side() {
                    self.cross_to(other, tokens);
                }
            }
        }
        self.status_height = input.status_height;
        // H = the smaller of the layout's content height (plus the status row where the status bar is hidden) and 0.85 × the screen.
        let extra = if input.status_hidden {
            input.status_height
        } else {
            0.0
        };
        let most =
            (input.content.height() + extra).min(input.screen.height() * self.max_height_ratio);
        // **A split panel fills the height**. Each half of a wide screen is a column of
        // its own, and a panel stopping where its content did read as a box floating in it; One
        // UI's run the whole way down. It runs to the bottom of the content area — never over the
        // navigation bar — and a card keeps its inset above that. The unified shade keeps its
        // limit, which leaves the scrim a band to be tapped; a split one has the other half.
        self.room = input.content.height() + extra;
        let h = if self.layout == OverlayLayout::Split {
            self.room
        } else {
            most
        };
        self.shade.set_height(h);
        // **The stop is where the tiles ended, measured off the frame that drew them.**
        //
        // It cannot be worked out here: how tall the tile block is depends on how many tiles the
        // gates left, how many columns they fell into and whether one is expanded, and all three
        // are decided while the panel draws. So the rects the panel recorded last frame are what
        // this reads — the same one-frame-late trick `CUSTOM_WIDTH_GUESS` uses in the status bar,
        // and stable for the same reason: it only changes when the layout does. Before the first
        // draw there are no rects and the shade is one-step, which is the safe way round.
        let resting = self.shade.at_detent();
        let moved = self.tiles_bottom().map(|stop| {
            let moved = self.tiles_stop.is_some_and(|old| (old - stop).abs() > 0.5);
            self.tiles_stop = Some(stop);
            moved
        });
        self.shade
            .set_detent(if self.two_step { self.tiles_stop } else { None });
        // A shade resting on the stop follows it when the stop moves — a row expanding under
        // it, or folding — rather than staying where the stop was. A stop that moved within a
        // hair of the bottom is no stop at all (`set_detent` drops it), and then the shade opens
        // the whole way: the row needs the room more than the screen behind needs the light.
        if self.two_step && resting && moved == Some(true) {
            let target = self.shade.detent().unwrap_or_else(|| self.shade.height());
            self.shade.settle_to(target, tokens);
        }
        let mut consumed = false;
        // A long press rides this frame only — the panel matches it against the tile places as it draws.
        if let Some(Gesture::LongPress { pos }) = input.gesture {
            self.long_press = Some(pos);
        }
        if let Some(Gesture::EdgeSwipe {
            edge: Edge::Top,
            progress,
            velocity,
            phase,
        }) = input.gesture
        {
            if input.allowed {
                consumed = true;
                match phase {
                    Phase::Started => {
                        // The engine takes it. Where the panel side already caught the same press (a
                        // re-entry mid-settle) the starting value is kept and it carries on — the origin
                        // being the same, y does not jump.
                        self.edge_active = true;
                        self.grab = Grab::None;
                        if !matches!(self.shade.state(), OverlayState::Dragging { .. }) {
                            self.shade.begin_drag();
                        }
                        self.shade.drag(progress, velocity);
                    }
                    Phase::Moved => {
                        self.edge_active = true;
                        self.shade.drag(progress, velocity);
                    }
                    // Taken by a higher priority (the prompt, the lock screen): the pull
                    // commits to nothing and the shade goes back up.
                    Phase::Cancelled => {
                        self.edge_active = false;
                        self.shade.close(tokens);
                    }
                    Phase::Ended => {
                        self.edge_active = false;
                        let opened = self.shade.release(velocity, tokens);
                        // The peek: not opened, and moved ≥ half the status bar's height, leaves the status row up for a moment.
                        if !opened && input.status_hidden && progress >= input.status_height * 0.5 {
                            self.shade.peek(input.now + self.peek_ms);
                        }
                    }
                }
            }
        }
        if self.dismissed {
            self.dismissed = false;
            self.shade.close(tokens);
        }
        self.shade.tick(input.dt);
        self.swipe_x.tick(input.dt);
        self.fade_t.tick(input.dt);
        if self.fade.is_some() && !self.fade_t.is_animating() {
            self.fade = None;
        }
        self.collapse.tick(input.dt);
        consumed
    }
    /// Rebuild the tile list from the declarations and the config (on a registry-dirty frame).
    pub(crate) fn rebuild_tiles(&mut self, custom: &[TileDecl]) {
        self.tiles.clear();
        self.expanded = None;
        for id in &self.tile_ids {
            if let Some(t) = custom
                .iter()
                .find(|d| d.id() == id)
                .map(|d| d.tile().clone())
                .or_else(|| tiles::builtin(id))
            {
                self.tiles.push((t, TileState::Off, true));
            } else {
                log::warn!(
                    "[overlay] tiles: `{id}` is neither built-in nor declared - ignoring it"
                );
            }
        }
        for d in custom {
            if !self.tiles.iter().any(|(t, _, _)| t.id == d.id()) {
                self.tiles.push((d.tile().clone(), TileState::Off, true));
            }
        }
        self.tiles_dirty = false;
    }
    /// Read the tile states from the backends and the config (every frame, allocation-free — the rules are in the [`tiles`] docs).
    pub(super) fn refresh_tile_states(
        &mut self,
        services: &Services,
        settings: &SettingsView,
        access: &Access,
        dark: bool,
    ) {
        for (t, state, allowed) in &mut self.tiles {
            *allowed = access.allows(t.gate_name());
            *state = match &t.kind {
                TileKind::Toggle(key) => match key.0.as_ref() {
                    keys::WIFI_ENABLED => on_off(services.wifi.enabled()),
                    keys::BLUETOOTH_ENABLED => on_off(services.bluetooth.enabled()),
                    // **The theme's truth is what is drawn right now.** Going by the settings memory
                    // alone, there is no value just after boot and it reads `Off` while the shell is
                    // already dark — drawing something that is on as off. The same reason Wi-Fi and
                    // Bluetooth ask the services.
                    keys::THEME_DARK => on_off(dark),
                    keys::DISPLAY_ROTATION_LOCK
                        if !tiles::rotation_lock_available(services.display.capabilities()) =>
                    {
                        TileState::Unavailable
                    }
                    _ => on_off(matches!(settings.get(key), Some(SettingValue::Bool(true)))),
                },
                // Neither can be summarised in one tile: a `Panel` row is drawn by the integrator, so the
                // crate knows no value for it, and a `Gauges` expansion row holds several values at once.
                // Both are drawn as on while they are open (the same rule as the other expansion tiles).
                TileKind::Panel { .. } | TileKind::Gauges { .. } => {
                    lit(t, settings, TileState::Off)
                }
                TileKind::Slider(key) => match key.0.as_ref() {
                    keys::DISPLAY_BRIGHTNESS => services
                        .display
                        .brightness()
                        .map_or(TileState::Unavailable, |b| {
                            TileState::Value(f32::from(b) / 100.0)
                        }),
                    // The audio backend is the original where it reports a volume; with the `Null`
                    // default the setting is all there is.
                    keys::AUDIO_VOLUME
                        if services
                            .audio
                            .capabilities()
                            .contains(crate::services::Capabilities::VOLUME) =>
                    {
                        services
                            .audio
                            .volume()
                            .map_or(TileState::Unavailable, |audio| {
                                TileState::Value(f32::from(audio.level) / 100.0)
                            })
                    }
                    _ => match settings.get(key) {
                        Some(SettingValue::Int(v)) => {
                            TileState::Value((*v as f32 / 100.0).clamp(0.0, 1.0))
                        }
                        Some(SettingValue::Float(v)) => {
                            TileState::Value((*v as f32 / 100.0).clamp(0.0, 1.0))
                        }
                        _ => TileState::Value(0.5),
                    },
                },
                // A tile that opens a screen is not a switch, so it has no on/off of its own — but the
                // **device** may (a conveyor running, a door open). `lit_by` is where it says so; with
                // no `lit_by` these keep the state they always had.
                TileKind::Action(_) => lit(t, settings, TileState::Off),
                TileKind::Status => lit(t, settings, TileState::Unavailable),
            };
        }
    }
    /// **The width a tile label is laid out against**, so a name too long for its tile is cut with
    /// an ellipsis instead of overflowing and being clipped at both ends — which is how "Bluetooth"
    /// drew as "luetoot", a letter gone from each end.
    ///
    /// It comes from [`panel::tile_width`], the same function the row itself uses, because the
    /// labels are laid out at the front of the frame and the row is drawn later: two copies of that
    /// arithmetic is precisely how the two came to disagree.
    pub(super) fn tile_label_width(&self, panel_w: f32, parts: &CxParts<'_>) -> f32 {
        panel::tile_width(panel_w, self.tile_columns, &parts.theme.metrics)
    }
    /// Bring the panel's text galley cache up to date (only the entries whose content changed are laid out again; unused ones are dropped).
    pub(super) fn refresh_texts(
        &mut self,
        ctx: &egui::Context,
        parts: &CxParts<'_>,
        center: &NotificationCenter,
        panel_w: f32,
    ) {
        let frame = parts.frame;
        let style = ctx.global_style();
        let small = egui::TextStyle::Small.resolve(&style);
        let body = egui::TextStyle::Body.resolve(&style);
        let button = egui::TextStyle::Button.resolve(&style);
        let strings = parts.strings;
        let name = parts.access.subject_name(strings);
        let settings = parts.settings;
        // The buffer holding the gauge value strings. One cell is reused so it is not rebuilt each frame
        // (`touch` does not even touch the buffer where the content is unchanged).
        let mut value_buf = std::mem::take(&mut self.value_buf);
        let tile_label_w = Some(self.tile_label_width(panel_w, parts));
        let Self { texts, tiles, .. } = self;
        ctx.fonts_mut(|fonts| {
            for (i, (t, _, _)) in tiles.iter().enumerate() {
                touch(
                    texts,
                    TextKey::TileLabel(i),
                    strings.get(&t.label),
                    &small,
                    fonts,
                    frame,
                    tile_label_w,
                );
                if let TileKind::Gauges { rows, .. } = &t.kind {
                    gauge_texts(
                        GaugeTexts {
                            texts,
                            tile: i,
                            rows,
                            settings,
                            strings,
                            buf: &mut value_buf,
                        },
                        (&body, &small),
                        fonts,
                        frame,
                    );
                }
            }
            touch(
                texts,
                TextKey::NoNotifications,
                strings.get(panel::labels::NO_NOTIFICATIONS),
                &body,
                fonts,
                frame,
                None,
            );
            touch(
                texts,
                TextKey::ClearAll,
                strings.get(panel::labels::CLEAR_ALL),
                &body,
                fonts,
                frame,
                None,
            );
            touch(
                texts,
                TextKey::Hidden,
                strings.get(panel::labels::HIDDEN_NOTIFICATION),
                &button,
                fonts,
                frame,
                None,
            );
            touch(texts, TextKey::FooterName, name, &body, fonts, frame, None);
            for item in center.iter() {
                touch(
                    texts,
                    TextKey::Title(item.id),
                    &item.title,
                    &button,
                    fonts,
                    frame,
                    None,
                );
                touch(
                    texts,
                    TextKey::Body(item.id),
                    &item.body,
                    &small,
                    fonts,
                    frame,
                    None,
                );
            }
        });
        texts.retain(|t| t.seen() == frame);
        value_buf.clear();
        self.value_buf = value_buf;
    }
    /// Decide and drive who owns a press over the panel (see the module docs). It does nothing on a frame the engine holds.
    pub(super) fn update_grab(
        &mut self,
        p: &Pointer,
        prev_panel: Option<Rect>,
        tokens: &MotionTokens,
    ) {
        if self.edge_active {
            self.grab = Grab::None;
            return;
        }
        if p.pressed {
            self.grab = Grab::None;
            let Some(pos) = p.origin.or(p.pos) else {
                return;
            };
            let on_panel = prev_panel.is_some_and(|r| r.contains(pos));
            if !on_panel || self.slider_rect.contains(pos) {
                return;
            }
            match self.shade.state() {
                OverlayState::Settling { .. } => {
                    // A re-entry: freeze it where it is (v = 0) and follow the finger.
                    self.shade.begin_drag();
                    self.grab = Grab::Shade { origin_y: pos.y };
                }
                OverlayState::Open => {
                    let in_list = self.list_rect.contains(pos);
                    self.grab = Grab::Pending {
                        origin: pos,
                        shade: !in_list || self.list_offset <= 0.5,
                        row: None,
                    };
                }
                OverlayState::Closed | OverlayState::Dragging { .. } => {}
            }
            return;
        }
        if !p.down {
            self.finish_grab(p, tokens);
            return;
        }
        let Some(pos) = p.pos else {
            return;
        };
        match self.grab {
            Grab::Pending { origin, shade, row } => {
                let d = pos - origin;
                if d.length() <= tokens.slop_px {
                    return;
                }
                // While an earlier removal is still flying out or collapsing, no new row is caught —
                // there is only one `swipe_x`, and catching one would cut that animation off.
                if let (Some(id), true) = (row, d.x.abs() > d.y.abs() && self.flying.is_none()) {
                    let w = self.list_rect.width().max(1.0);
                    // **The right only opens where there is something to open.** Pushed right on a
                    // notification with no routing it looks as though something will happen and nothing
                    // does — not following at all is the more honest of the two.
                    self.swipe_x
                        .set_range(-w, if self.swipe_open { w } else { 0.0 });
                    self.swipe_x.snap(0.0);
                    self.swipe_x.begin();
                    self.swipe_x.drag(d.x, p.velocity.x);
                    self.swiping = Some(id);
                    self.grab = Grab::Row {
                        id,
                        origin_x: origin.x,
                    };
                } else if shade {
                    self.shade.begin_drag();
                    self.shade.drag(d.y, p.velocity.y);
                    self.grab = Grab::Shade { origin_y: origin.y };
                } else {
                    // The list owns it: what could not be followed during the slop is caught up in one go
                    // (this frame's delta is added by the `ScrollArea`). The offset grows opposite to the finger.
                    self.scroll_to = Some(self.list_offset + (origin.y - (pos.y - p.delta.y)));
                    self.grab = Grab::List;
                }
            }
            Grab::Shade { origin_y } => self.shade.drag(pos.y - origin_y, p.velocity.y),
            Grab::Row { origin_x, .. } => self.swipe_x.drag(pos.x - origin_x, p.velocity.x),
            Grab::None | Grab::List => {}
        }
    }
    /// The release (or the pointer disappearing).
    pub(super) fn finish_grab(&mut self, p: &Pointer, tokens: &MotionTokens) {
        match self.grab {
            Grab::Shade { .. } => {
                self.shade.release(p.velocity.y, tokens);
            }
            Grab::Row { id, .. } => {
                // Confirmed: at least a third of the width, or a fling in the same direction.
                // **The direction decides the meaning** — left throws it away, right opens the screen
                // the notification points at.
                let w = self.list_rect.width().max(1.0);
                let x = self.swipe_x.value();
                let rule = ReleaseRule {
                    snap_ratio: 1.0 / 3.0,
                    fling: tokens.fling_px_s,
                };
                let confirmed =
                    x.abs() > 0.0 && rule.confirm(x.abs() / w, p.velocity.x * x.signum());
                match (confirmed, x < 0.0) {
                    // Left = remove. It is removed **after being flown out** — that is what makes what went visible.
                    (true, true) => {
                        self.flying = Some(PanelAction::Dismiss(id));
                        if tokens.reduce {
                            self.pending = self.flying.take();
                            self.swiping = None;
                            self.swipe_x.snap(0.0);
                        } else {
                            self.swipe_x.release(-w, tokens.shade.spring);
                        }
                    }
                    // Right = open. The shade is about to go up, so the row is put back in place.
                    (true, false) if self.swipe_open => {
                        self.pending = Some(PanelAction::NotificationTapped(id));
                        self.swiping = None;
                        self.swipe_x.snap(0.0);
                    }
                    _ if tokens.reduce => self.swipe_x.snap(0.0),
                    _ => self.swipe_x.release(0.0, tokens.shade.spring),
                }
            }
            Grab::None | Grab::Pending { .. } | Grab::List => {}
        }
        self.grab = Grab::None;
    }
    /// Clear the swipe state on a row whose removal has taken effect (it is no longer in the list) or
    /// whose return spring has finished.
    ///
    /// Removal is **two steps**: it flies out sideways ([`Overlay::flying`]) and then collapses the
    /// place it left down to 0 ([`Overlay::collapse`]). Only on the frame the second step finishes is
    /// `Dismiss` raised and the notification really out of the list.
    pub(super) fn settle_swipe(&mut self, center: &NotificationCenter, tokens: &MotionTokens) {
        // The row flying out has stopped — now it collapses vertically.
        if self.flying.is_some() && !self.swipe_x.is_animating() {
            if self.collapsing.is_none() && !tokens.reduce {
                self.collapsing = self.swiping;
                self.collapse.snap(1.0);
                self.collapse.to(0.0, tokens.toast_out);
                return;
            }
            if self.collapse.is_animating() {
                return;
            }
            self.pending = self.flying.take();
            self.swiping = None;
            self.collapsing = None;
            self.collapse.snap(1.0);
            self.swipe_x.snap(0.0);
            return;
        }
        if self.flying.is_some() {
            return;
        }
        let Some(id) = self.swiping else {
            return;
        };
        let settled = !matches!(self.grab, Grab::Row { .. })
            && !self.swipe_x.is_animating()
            && self.swipe_x.value().abs() < 0.5;
        if center.get(id).is_none() || settled {
            self.swiping = None;
            self.swipe_x.snap(0.0);
        }
    }
    /// A frame closed with no panel: clear the panel interaction state (a shade grab in progress is
    /// kept — its release is handled by [`Overlay::update_grab`]).
    pub(super) fn reset_panel_state(&mut self) {
        self.tile_rects.clear();
        self.card_p = 0.0;
        self.card_lagging = false;
        self.fade = None;
        self.last_plate = None;
        self.backdrop = None;
        self.shot_wait = 0;
        self.pending_shot = None;
        self.expanded = None;
        self.swiping = None;
        self.swipe_x.snap(0.0);
        self.scroll_to = None;
        self.pending = None;
        self.flying = None;
        self.collapsing = None;
        self.collapse.snap(1.0);
        self.long_press = None;
        self.swallow_tap = false;
        self.list_rect = Rect::NOTHING;
        self.close_hit = Rect::NOTHING;
        self.slider_rect = Rect::NOTHING;
    }
}
