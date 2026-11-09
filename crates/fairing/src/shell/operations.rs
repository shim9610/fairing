//! Task-specific trusted host operations. No method lends a mutable component.

use super::Shell;
use crate::chrome::{NavStyle, Slot, StatusItemSpec};

impl Shell {
    /// Send one synchronous trusted-host backend command without lending the store.
    /// This preserves the authority of the former `services_mut` host API; user
    /// settings must go through `set_setting`. A faulted shell issues no command.
    ///
    /// # Errors
    /// Returns the backend's error, or `Other` when quarantine/fault containment
    /// prevents execution. Observe asynchronously accepted work through the
    /// backend's polling/snapshot contract.
    ///
    /// ```compile_fail
    /// fn command(shell: &mut fairing::ProtectedShell) {
    ///     let _ = shell.request_service(fairing::services::ServiceRequest::WifiScan);
    /// }
    /// ```
    pub fn request_service(
        &mut self,
        request: crate::services::ServiceRequest<'_>,
    ) -> crate::services::ServiceResult {
        use crate::services::{ErrorKind, ServiceError, ServiceRequest as R};
        let mut result = Err(ServiceError::new(ErrorKind::Other, "shell unavailable"));
        self.contain_fault(|shell| {
            result = match request {
                R::SetTime(seconds) => shell.services.clock.set_time(seconds),
                R::Power(request) => shell.services.power.request(request),
                R::WifiEnabled(on) => shell.services.wifi.set_enabled(on),
                R::WifiScan => shell.services.wifi.scan(),
                R::WifiConnect { ssid, psk, hidden } => {
                    shell.services.wifi.connect(ssid, psk, hidden)
                }
                R::WifiDisconnect => shell.services.wifi.disconnect(),
                R::WifiForget(ssid) => shell.services.wifi.forget(ssid),
                R::BluetoothEnabled(on) => shell.services.bluetooth.set_enabled(on),
                R::BluetoothDiscovering(on) => shell.services.bluetooth.set_discovering(on),
                R::BluetoothPair(addr) => shell.services.bluetooth.pair(addr),
                R::BluetoothRespondPairing { accept, passkey } => {
                    shell.services.bluetooth.respond_pairing(accept, passkey)
                }
                R::BluetoothConnect { addr, connected } => {
                    shell.services.bluetooth.connect(addr, connected)
                }
                R::BluetoothRemove(addr) => shell.services.bluetooth.remove(addr),
                R::DisplayBrightness(value) => shell.services.display.set_brightness(value),
                R::DisplayPower(on) => shell.services.display.set_power(on),
                R::AudioVolume(value) => shell.services.audio.set_volume(value),
                R::AudioMuted(on) => shell.services.audio.set_muted(on),
                R::AudioCue(cue) => {
                    shell.services.audio.play_cue(cue);
                    Ok(())
                }
                R::NetworkConfigure { iface, config } => {
                    shell.services.network.configure(iface, config)
                }
                R::NetworkHostname(name) => shell.services.network.set_hostname(name),
            };
            shell.handle.waker().wake();
        });
        result
    }

    /// Replace the desktop background and schedule a redraw.
    pub fn set_wallpaper(&mut self, wallpaper: crate::desktop::Wallpaper) {
        self.desktop.set_wallpaper(wallpaper);
        self.handle.waker().wake();
    }

    /// Change desktop label contrast and schedule a redraw.
    pub fn set_desktop_legibility(&mut self, mode: crate::desktop::LabelLegibility) {
        self.desktop.set_legibility(mode);
        self.handle.waker().wake();
    }

    /// Replace the trusted legacy cell painter. Protected shells do not expose this operation.
    pub fn set_desktop_slot_painter(&mut self, painter: crate::desktop::SlotPainter) {
        self.desktop.set_slot_painter(painter);
        self.handle.waker().wake();
    }

    /// Select a desktop page immediately. Out-of-range indices use the desktop's clamping rule.
    pub fn set_desktop_page(&mut self, index: usize) {
        self.desktop.set_page(index);
        self.handle.waker().wake();
    }

    /// Animate to a desktop page using explicit motion tokens.
    pub fn swipe_desktop_to(&mut self, index: usize, tokens: &crate::theme::MotionTokens) {
        self.desktop.swipe_to(index, tokens);
        self.handle.waker().wake();
    }

    /// Copy a status item's configuration for editing as an owned value.
    #[must_use]
    pub fn status_item_spec(&self, id: &str) -> Option<StatusItemSpec> {
        self.status_bar.spec(id).cloned()
    }

    /// Replace an existing item's configuration. Its identity must remain unchanged.
    /// Returns false without changing anything for a missing item or mismatched identity.
    pub fn set_status_item_spec(&mut self, id: &str, spec: StatusItemSpec) -> bool {
        if spec.id != id {
            return false;
        }
        let Some(current) = self.status_bar.spec_mut(id) else {
            return false;
        };
        *current = spec;
        self.status_bar.invalidate_layout();
        self.handle.waker().wake();
        true
    }

    /// Replace one status slot's owned configuration, invalidating its layout caches.
    /// Register executable items through `add`; specs do not install callbacks.
    /// Empty or duplicate IDs, including collisions with other slots, are rejected.
    pub fn set_status_items(&mut self, slot: Slot, specs: Vec<StatusItemSpec>) -> bool {
        let mut ids = std::collections::BTreeSet::new();
        let others = match slot {
            Slot::Left => self.status_bar.center.iter().chain(&self.status_bar.right),
            Slot::Center => self.status_bar.left.iter().chain(&self.status_bar.right),
            Slot::Right => self.status_bar.left.iter().chain(&self.status_bar.center),
        };
        for spec in others.chain(&specs) {
            if spec.id.is_empty() || !ids.insert(spec.id.as_str()) {
                return false;
            }
        }
        match slot {
            Slot::Left => self.status_bar.left = specs,
            Slot::Center => self.status_bar.center = specs,
            Slot::Right => self.status_bar.right = specs,
        }
        self.status_bar.invalidate_layout();
        self.handle.waker().wake();
        true
    }

    /// Set status bar visibility and request layout recomputation.
    pub fn set_status_bar_enabled(&mut self, enabled: bool) {
        self.status_bar.enabled = enabled;
        self.handle.waker().wake();
    }

    /// Set status bar height in display units. Non-finite or negative values are rejected.
    pub fn set_status_bar_height(&mut self, height: f32) -> bool {
        if !height.is_finite() || height < 0.0 {
            return false;
        }
        let mut spec = self.metrics_spec();
        spec.status_bar_height = crate::unit::Span::fixed(crate::unit::Dim::du(height)).pinned();
        self.set_metrics_spec(spec);
        true
    }

    /// Set the status bar background role.
    pub fn set_status_bar_background(&mut self, role: crate::theme::ColorRole) {
        self.status_bar.background = role;
        self.handle.waker().wake();
    }

    /// Set the status bar's default icon presentation.
    pub fn set_status_bar_icon(&mut self, icon: crate::icons::IconStyle) {
        self.status_bar.icon = icon;
        self.handle.waker().wake();
    }

    /// Choose whether tapping empty status space opens the shade.
    pub fn set_status_tap_opens_shade(&mut self, enabled: bool) {
        self.status_bar.tap_opens_shade = enabled;
        self.handle.waker().wake();
    }

    /// Set navigation presentation; next-frame warning/layout checks use this value.
    pub fn set_nav_style(&mut self, style: NavStyle) {
        self.nav_bar.style = style;
        self.handle.waker().wake();
    }

    /// Set navigation visibility.
    pub fn set_nav_enabled(&mut self, enabled: bool) {
        self.nav_bar.enabled = enabled;
        self.handle.waker().wake();
    }

    /// Set navigation bar height in display units. Non-finite or negative values are rejected.
    pub fn set_nav_height(&mut self, height: f32) -> bool {
        if !height.is_finite() || height < 0.0 {
            return false;
        }
        let mut spec = self.metrics_spec();
        spec.nav_bar_height = crate::unit::Span::fixed(crate::unit::Dim::du(height)).pinned();
        self.set_metrics_spec(spec);
        true
    }

    /// Set navigation background role.
    pub fn set_nav_background(&mut self, role: crate::theme::ColorRole) {
        self.nav_bar.background = role;
        self.handle.waker().wake();
    }

    /// Copy the current unresolved metrics, seeding from resolved metrics if necessary.
    #[must_use]
    pub fn metrics_spec(&self) -> crate::theme::MetricsSpec {
        self.metrics_spec
            .clone()
            .unwrap_or_else(|| crate::theme::MetricsSpec::from_metrics(&self.theme.metrics))
    }

    /// Replace unresolved metrics and refresh derived theme/style state on the next update.
    pub fn set_metrics_spec(&mut self, spec: crate::theme::MetricsSpec) {
        self.metrics_spec = Some(spec);
        self.theme_dirty = true;
        self.handle.waker().wake();
    }

    /// Install a policy. In token mode a policy can only narrow issuer grants.
    pub fn set_policy(&mut self, policy: Box<dyn crate::access::AccessPolicy>) {
        self.access.set_policy(policy);
        self.handle.waker().wake();
    }

    /// Set a trusted legacy subject using the shell's clock. Token mode detaches
    /// authority rather than accepting injected identity. Change propagation runs at update.
    pub fn set_subject(&mut self, subject: crate::access::Subject) -> crate::access::Level {
        let previous = self.access.set_subject_at(subject, self.mono);
        self.handle.waker().wake();
        previous
    }

    #[cfg(feature = "settings")]
    pub(crate) fn has_credential_admin(&mut self) -> bool {
        self.access.has_admin()
    }

    /// Replace all trusted host backends. Attach their waker before publication and
    /// discard acknowledgements belonging to the old backend instances.
    pub fn set_services(&mut self, mut services: crate::Services) {
        self.contain_fault(|shell| {
            services.attach(shell.handle.waker());
            shell.pending_bools.clear();
            shell.services = services;
            shell.handle.waker().wake();
        });
    }

    /// Replace the trusted power backend, attaching its waker before publication.
    pub fn set_power_backend(&mut self, backend: impl crate::services::PowerBackend + 'static) {
        self.contain_fault(|shell| {
            let mut backend = Box::new(backend);
            backend.attach(shell.handle.waker().clone());
            shell.services.power = backend;
            shell.handle.waker().wake();
        });
    }

    /// Replace the trusted wifi backend, attaching its waker before publication.
    pub fn set_wifi_backend(&mut self, backend: impl crate::services::WifiBackend + 'static) {
        self.contain_fault(|shell| {
            let mut backend = Box::new(backend);
            backend.attach(shell.handle.waker().clone());
            shell
                .pending_bools
                .remove(&crate::settings::SettingKey::from(
                    crate::settings::keys::WIFI_ENABLED,
                ));
            shell.services.wifi = backend;
            shell.handle.waker().wake();
        });
    }

    /// Replace the trusted bluetooth backend, attaching its waker before publication.
    pub fn set_bluetooth_backend(
        &mut self,
        backend: impl crate::services::BluetoothBackend + 'static,
    ) {
        self.contain_fault(|shell| {
            let mut backend = Box::new(backend);
            backend.attach(shell.handle.waker().clone());
            shell
                .pending_bools
                .remove(&crate::settings::SettingKey::from(
                    crate::settings::keys::BLUETOOTH_ENABLED,
                ));
            shell.services.bluetooth = backend;
            shell.handle.waker().wake();
        });
    }

    /// Replace the trusted display backend, attaching its waker before publication.
    pub fn set_display_backend(&mut self, backend: impl crate::services::DisplayBackend + 'static) {
        self.contain_fault(|shell| {
            let mut backend = Box::new(backend);
            backend.attach(shell.handle.waker().clone());
            shell.services.display = backend;
            shell.handle.waker().wake();
        });
    }

    /// Replace the trusted audio backend, attaching its waker before publication.
    pub fn set_audio_backend(&mut self, backend: impl crate::services::AudioBackend + 'static) {
        self.contain_fault(|shell| {
            let mut backend = Box::new(backend);
            backend.attach(shell.handle.waker().clone());
            shell
                .pending_bools
                .remove(&crate::settings::SettingKey::from(
                    crate::settings::keys::AUDIO_MUTED,
                ));
            shell.services.audio = backend;
            shell.handle.waker().wake();
        });
    }

    /// Replace the trusted network backend, attaching its waker before publication.
    pub fn set_network_backend(&mut self, backend: impl crate::services::NetworkBackend + 'static) {
        self.contain_fault(|shell| {
            let mut backend = Box::new(backend);
            backend.attach(shell.handle.waker().clone());
            shell.services.network = backend;
            shell.handle.waker().wake();
        });
    }

    /// Replace the trusted info backend, attaching its waker before publication.
    pub fn set_info_backend(&mut self, backend: impl crate::services::InfoBackend + 'static) {
        self.contain_fault(|shell| {
            let mut backend = Box::new(backend);
            backend.attach(shell.handle.waker().clone());
            shell.services.info = backend;
            shell.handle.waker().wake();
        });
    }

    /// Change the keyboard layout while preserving its composition-reset rules.
    #[cfg(feature = "osk")]
    pub fn set_osk_layout(&mut self, layout: crate::osk::OskLayout) {
        self.osk.0.set_layout(layout);
        self.handle.waker().wake();
    }

    /// Switch the keyboard's language pair.
    #[cfg(feature = "osk")]
    pub fn toggle_osk_language(&mut self) {
        self.osk.0.toggle_lang();
        self.handle.waker().wake();
    }

    /// Toggle the manual keyboard visibility override.
    #[cfg(feature = "osk")]
    pub fn toggle_osk(&mut self) {
        self.osk.0.toggle();
        self.handle.waker().wake();
    }

    /// Dismiss the keyboard according to its existing focus policy.
    #[cfg(feature = "osk")]
    pub fn hide_osk(&mut self) {
        self.osk.0.hide();
        self.handle.waker().wake();
    }
}
