//! Update for `Shell`.

use super::{
    density, AppRef, Badge, CustomIconId, IconDef, IconPainter, Instant, LaunchAction, Shell,
    ShellEvent, TOUCH_GRACE,
};

impl Shell {
    /// Handle a knock's result: on an opening, check the gate; otherwise only the hint.
    pub(super) fn after_knock(
        &mut self,
        idx: usize,
        step: crate::access::KnockStep,
        app: &mut AppRef<'_>,
    ) {
        use crate::access::KnockStep;
        let Some(entry) = self.hidden.get(idx) else {
            return;
        };
        match step {
            KnockStep::Opened => {
                let (id, gate, action) =
                    (entry.id.clone(), entry.gate.clone(), entry.action.clone());
                // **It does not open quietly.** Entering a service menu is auditable.
                self.events.push(ShellEvent::HiddenEntry { id });
                match gate {
                    Some(gate) if !self.access.allows(&gate) => {
                        self.request_unlock(gate, Some(action));
                    }
                    _ => self.launch_in(action, app),
                }
            }
            KnockStep::Idle => return,
            KnockStep::Advanced | KnockStep::Reset => {}
        }
        self.refresh_hidden_remaining();
    }
    /// Rebuild the snapshot `Cx::knock_remaining` reads.
    pub(super) fn refresh_hidden_remaining(&mut self) {
        self.hidden_remaining.clear();
        for entry in &self.hidden {
            // A trigger with nothing to count has no hint either, and an entry is quiet until the
            // count falls to its `hint_from` — `Cx::knock_remaining` is `None` until then.
            let Some(from) = entry.hint_from else {
                continue;
            };
            if let Some(left) = entry.trigger.remaining().filter(|left| *left <= from) {
                self.hidden_remaining.insert(entry.id.clone(), left);
            }
        }
    }
    /// **Actually carry out** the power operation announced by [`ShellEvent::PowerRequest`].
    ///
    /// The point is that the crate does not call this by itself — a device often has to close a valve
    /// and flush its logs before shutting down, and the crate cannot do that tidying for it. Receiving
    /// the event and **not calling this is the refusal**.
    ///
    /// # Errors
    ///
    /// A [`ServiceError`](crate::services::ServiceError) where the backend does not support it or it
    /// fails.
    pub fn commit_power(
        &mut self,
        request: crate::services::PowerRequest,
    ) -> crate::services::ServiceResult {
        self.services.power.request(request)
    }
    /// Decide whether to show the mouse cursor (`[shell] cursor`).
    ///
    /// The default automatic policy hides the cursor after touch input and shows
    /// it for mouse-like input. Explicit configuration can override that policy.
    ///
    /// A touch emits pointer events **as well**, so for a few tens of milliseconds after the hand
    /// leaves it looks like a mouse. Ignoring [`TOUCH_GRACE`]'s worth removes that illusion.
    pub(super) fn update_cursor(&mut self, ctx: &egui::Context, now: Instant) {
        use crate::config::CursorPolicy;
        let hide = match self.config.shell.cursor {
            CursorPolicy::Visible => false,
            CursorPolicy::Hidden => true,
            CursorPolicy::Auto => {
                let (touching, moved) = ctx.input(|i| {
                    (
                        i.any_touches(),
                        i.pointer.is_moving() || i.smooth_scroll_delta != egui::Vec2::ZERO,
                    )
                });
                if touching {
                    self.last_touch = Some(now);
                    self.mouse_active = false;
                } else if moved
                    && self
                        .last_touch
                        .is_none_or(|t| now.saturating_duration_since(t) > TOUCH_GRACE)
                {
                    self.mouse_active = true;
                }
                !self.mouse_active
            }
        };
        if hide {
            // It **overrides** the cursor a widget set — doing it here rather than at the end of the
            // frame works because egui does not take the frame's last value as the output cursor and we
            // set it again every frame. A widget changing it afterwards shows for that frame only.
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
    }
    /// In a rail layout, open the first entry when at home.
    ///
    /// In rail mode, returning home opens the first eligible rail entry instead
    /// of leaving the workspace empty.
    ///
    /// It leaves a transition in progress alone — reopening mid-close makes the A2 transition jump.
    pub(super) fn open_rail_home(&mut self, app: &mut AppRef<'_>) {
        if self.layout.rail.is_none() || !self.workspace.is_home() || self.workspace.is_animating()
        {
            return;
        }
        let Some(id) = self
            .desktop
            .first_rail_entry(&self.access)
            .map(str::to_owned)
        else {
            return;
        };
        self.launch_in(LaunchAction::open(id), app);
    }
    /// `[desktop] rail` → the layout input.
    ///
    /// The rail **stands aside on a fullscreen screen** — with `ChromePolicy::fullscreen` there is no
    /// rail, just as there are no bars. Even a kiosk has moments that use a whole screen, such as a
    /// payment-complete screen.
    pub(super) fn rail_input(&self) -> Option<crate::shell::layout::RailInput> {
        use crate::shell::layout::RailInput;
        /// The rail width's default fraction.
        const DEFAULT_FRACTION: f32 = 0.26;
        // A screen with both bars hidden means to use the whole screen — the rail stands aside too.
        // Even a kiosk has moments that use a whole screen, such as a payment-complete screen.
        let policy = self.workspace.chrome_policy();
        if policy.status_bar == crate::screen::BarMode::Hide
            && policy.nav_bar == crate::screen::BarMode::Hide
        {
            return None;
        }
        let rail = self.config.desktop.rail.trim();
        let left = if rail.eq_ignore_ascii_case("left") {
            true
        } else if rail.eq_ignore_ascii_case("right") {
            false
        } else if rail.is_empty() || rail.eq_ignore_ascii_case("none") {
            return None;
        } else {
            log::warn!("[desktop] rail = \"{rail}\" is not a known value (none | left | right)");
            return None;
        };
        let fraction = if self.config.desktop.rail_width > 0.0 {
            self.config.desktop.rail_width.clamp(0.1, 0.5)
        } else {
            DEFAULT_FRACTION
        };
        Some(RailInput {
            left,
            fraction,
            min_width: self.theme.metrics.icon_cell,
        })
    }
    /// The settings read view.
    #[must_use]
    pub fn settings(&self) -> &crate::settings::SettingsView {
        &self.settings
    }
    /// Swap the wallpaper from a file. This is the path by which a shopkeeper chooses a photo
    /// from the settings screen.
    ///
    /// The decode is done by **the integrator loader** registered through
    /// [`ShellBuilder::image_loader`](crate::shell::ShellBuilder::image_loader) — the crate does not
    /// decode images. The texture that comes back is held by
    /// [`Wallpaper::Owned`](crate::desktop::Wallpaper::Owned), so the caller need not keep it.
    ///
    /// The application loader runs synchronously on the calling thread. Decode
    /// cost depends on the input and implementation; avoid expensive decoding
    /// during painting or prepare/cache the texture in the application.
    ///
    /// # Errors
    ///
    /// [`crate::Error::Config`] where there is no loader; where the loader fails, its error as it
    /// stands. **The current wallpaper is unchanged on a failure** — it is never left half swapped.
    pub fn load_wallpaper(
        &mut self,
        ctx: &egui::Context,
        path: impl AsRef<std::path::Path>,
        fit: crate::desktop::Fit,
    ) -> crate::Result<()> {
        let Some(loader) = self.image_loader.as_deref() else {
            return Err(crate::Error::Config(
                "no `ShellBuilder::image_loader` - the crate does not decode images \
                 (guide 09 §1)"
                    .to_owned(),
            ));
        };
        let texture = loader(ctx, path.as_ref())?;
        self.desktop
            .set_wallpaper(crate::desktop::Wallpaper::owned(texture, fit));
        Ok(())
    }
    /// Swap the dark/light palette pair. The palette in view is left alone — it takes effect
    /// from the next [`Shell::set_theme_dark`].
    ///
    /// Where the old behaviour is wanted (a toggle resetting to the neutral palette), it is one line:
    /// `set_palettes(Palette::dark(), Palette::light())`.
    pub fn set_palettes(&mut self, dark: crate::theme::Palette, light: crate::theme::Palette) {
        self.palettes = (dark, light);
    }
    /// The current pair, `(dark, light)`.
    #[must_use]
    pub fn palettes(&self) -> (&crate::theme::Palette, &crate::theme::Palette) {
        (&self.palettes.0, &self.palettes.1)
    }
    /// Change the current palette straight away, by cross-fade (the pair is left alone).
    pub fn apply_palette(&mut self, palette: crate::theme::Palette) {
        self.theme_fade.start(palette, self.theme.motion.theme_fade);
    }
    /// Whether a tracked animation or transition was running last frame.
    /// The repaint scheduler also considers queued work, requests and deadlines.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.animations.is_animating()
            || self.workspace.is_animating()
            || self.desktop.is_animating()
            || self.overlay.is_animating()
            || self.osk.is_animating()
            || self.toasts.is_animating()
            || self.heads_up.is_animating()
            || self.theme_fade.is_animating()
            || self.status_bar.is_animating(self.mono)
            || self.guard.is_animating()
            || self
                .prompt
                .is_animating(self.mono, self.theme.motion.reduce)
    }
    /// The frame number.
    #[must_use]
    pub fn frame_no(&self) -> u64 {
        self.frame_no
    }
    /// Last frame's dt (in seconds).
    #[must_use]
    pub fn last_dt(&self) -> f32 {
        self.last_dt
    }
    /// Register a vector icon (integrator extension).
    pub fn register_icon(&mut self, def: IconDef) -> CustomIconId {
        self.icons.register(def)
    }
    /// Register an arbitrary drawing-callback icon (`register_icon_painter`). Used as `IconRef::Custom(id)`.
    pub fn register_icon_painter(&mut self, painter: IconPainter) -> CustomIconId {
        self.icons.register_painter(painter)
    }
    /// An icon badge.
    pub fn set_badge(&mut self, id: &str, badge: Option<&Badge>) -> bool {
        self.desktop.set_badge(id, badge)
    }
    /// Resolve this pass's scale from the policy, panel metadata, permitted
    /// environment overrides and fallback inputs.
    ///
    /// Commit the egui zoom factor only when `pixels_per_point` changes, avoiding
    /// repeated font-atlas invalidation.
    pub(super) fn resolve_scale(&mut self, ctx: &egui::Context, root: egui::Rect) {
        use crate::unit::{Scale, ScaleConfidence, ScaleSource};

        let info = self.services.display.info();
        // A pin is a declaration that "this window is that panel", so the pixel count is taken from
        // **the root Rect × the committed ppp** rather than from the backend. The backend's `size_px`
        // may differ from the window's (a mock backend, several outputs), and that mismatch becomes a
        // density error as it stands.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the screen size is positive and inside u32's range"
        )]
        let root_px = (
            (root.width() * self.committed_ppp).max(1.0) as u32,
            (root.height() * self.committed_ppp).max(1.0) as u32,
        );
        // **The density chain**. The integrator's pin, then the environment, then the
        // backend, then the fallback. `FAIRING_PPI` is already a density and so needs no rect;
        // `FAIRING_PHYSICAL_MM` sits in the pin's slot and is told apart by `env_physical_mm`,
        // because a size in millimetres only becomes a density once the panel's pixels are known.
        let (px_per_mm, source, confidence) =
            match (self.physical_mm_pin, self.env_px_per_mm, info.physical_mm) {
                // Where the integrator pinned it, that wins — an integrator knows their own device's inches.
                (Some((w_mm, h_mm)), _, _) => (
                    density(root_px, (w_mm, h_mm)),
                    if self.env_physical_mm {
                        ScaleSource::Env
                    } else {
                        ScaleSource::Pin
                    },
                    ScaleConfidence::Declared,
                ),
                // A person at the device, working around a file they cannot edit. Declared,
                // not measured: a number someone typed is a claim, however carefully they measured it.
                (None, Some(px_per_mm), _) => {
                    (px_per_mm, ScaleSource::Env, ScaleConfidence::Declared)
                }
                (None, None, Some(mm)) => (
                    density(info.size_px, mm),
                    ScaleSource::Backend,
                    ScaleConfidence::Measured,
                ),
                // Nothing is known. `Scale::resolve` folds to the policy's assumption and changes the source to Fallback.
                (None, None, None) => (f32::NAN, ScaleSource::Fallback, ScaleConfidence::Assumed),
            };
        // `root` is in du at the committed ppp; `Scale::resolve` takes physical pixels and divides
        // by the ppp it settles on, so the `frac_*` reference is right on a frame the ppp changes too.
        let scale = Scale::resolve(
            px_per_mm,
            &self.scale_policy,
            source,
            confidence,
            root.size() * self.committed_ppp,
        );

        if (scale.pixels_per_point - self.committed_ppp).abs() > 1e-4 {
            ctx.set_zoom_factor(scale.pixels_per_point);
            self.committed_ppp = scale.pixels_per_point;
            log::debug!(
                "[scale] ppp {:.3} - {:.2} px/mm - finger {:.1} mm - source {:?}/{:?}",
                scale.pixels_per_point,
                scale.px_per_mm,
                scale.finger_mm,
                scale.source,
                scale.confidence
            );
        }
        if scale.confidence == ScaleConfidence::Assumed && self.frame_no == 0 {
            log::warn!(
                "[scale] the panel size in millimetres is unknown - assuming {:.3} px/mm. Pin it with `ShellBuilder::physical_mm(w, h)` and touch targets become real millimetres",
                self.scale_policy.assume_px_per_mm
            );
        }
        self.scale = scale;
        // Metrics fields stay f32; resolve their values again at this scale.
        if let Some(spec) = self.metrics_spec.as_ref() {
            let resolved = spec.resolve(&scale);
            // The text sizes go out as egui `TextStyle`s, so a change here has to reach the style too.
            if resolved.type_scale != self.theme.metrics.type_scale {
                self.theme_dirty = true;
            }
            self.theme.metrics = resolved;
            self.status_bar.height = self.theme.metrics.status_bar_height;
            self.nav_bar.height = self.theme.metrics.nav_bar_height;
        }
        // **Everything sized in `Dim::text` is resolved against the type scale that actually
        // landed, not the one the policy implies.** `Scale::text_du` comes from
        // `ScalePolicy::viewing_distance_mm`, which is the supported way to move the text; but an
        // integrator may also state `MetricsSpec::type_scale` outright, and when they do, the badge,
        // the lamp, the gaps and the control heights have to go with it. Leaving them behind is
        // precisely the defect this term was added to close.
        let scale = scale.with_text_du(self.theme.metrics.type_scale.body);
        if let Some(spec) = self.component_spec.as_ref() {
            self.theme.components = spec.resolve(&scale);
        }
        // **The control lengths are resolved here too, and were not.** `theme.control` was filled
        // once from `ControlMetrics::default()` — which resolves against `Scale::identity()`, a
        // gloved 13 mm finger at one pixel per du — and never again. Every drawn control therefore
        // came out at the gloved size on every panel: `mark_size` is `finger(0.6)`, so a switch
        // measured 7.8 mm tall whether the device declared a 9 mm bare finger or nothing at all,
        // while the row around it followed the real scale. The two disagreed by 1.45x, measured.
        if let Some(spec) = self.control_spec.as_ref() {
            self.theme.control = spec.resolve(&scale);
        }
        // Same reason again: both elevation lengths are millimetres with a pixel floor, so a spec
        // frozen at `Scale::identity()` would cast one panel's penumbra on every panel.
        if let Some(spec) = self.elevation_spec.as_ref() {
            self.theme.elevation = spec.resolve(&scale);
        }
    }
    /// This frame's scale. For an integrator who wants to measure in physical units.
    #[must_use]
    pub fn scale(&self) -> &crate::unit::Scale {
        &self.scale
    }
    /// Draw a shell pass into the root `Ui` supplied by the runner or custom host.
    ///
    /// The stage order is the table in the module docs.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        self.frame_with(ui, &mut ());
    }
    /// **The same, with the app's own state lent to the screens for this frame**.
    ///
    /// The app owns its state *and* owns the shell; the shell is a field of the app, not a
    /// container for it. Once a frame the app lends what its screens need, and
    /// [`Cx::app`](crate::Cx::app) / [`app_mut`](crate::Cx::app_mut) /
    /// [`with_app`](crate::Cx::with_app) hand it on:
    ///
    /// ```no_run
    /// # struct Console;
    /// struct App {
    ///     shell: fairing::Shell,
    ///     console: Console,
    /// }
    ///
    /// impl App {
    ///     fn frame(&mut self, ui: &mut egui::Ui) {
    ///         self.shell.frame_with(ui, &mut self.console);
    ///     }
    /// }
    /// ```
    ///
    /// One value goes over — an app with several things to share hands over one `struct` holding
    /// them. The shell borrows that value for the call without requiring shared
    /// ownership, an interior-mutability wrapper or a `'static` bound.
    ///
    /// [`Shell::frame`] is this with nothing lent; screens then see `cx.app::<T>() == None`.
    pub fn frame_with(&mut self, ui: &mut egui::Ui, app: &mut dyn std::any::Any) {
        if self.faulted {
            self.show_recovery_ui(ui);
            return;
        }
        self.contain_fault(|shell| {
            let mut app = Some(app);
            shell.sync_session(&mut app);
            shell.frame_inner(ui, &mut app);
        });
        if self.faulted {
            crate::runtime::blank_faulted_frame(ui.ctx());
        }
    }
}
