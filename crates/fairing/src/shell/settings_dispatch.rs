//! Settings dispatch for `Shell`.

use super::{
    bool_ack, keys, percent_from_f64, Gate, LaunchAction, PendingBool, SettingKey, SettingValue,
    Shell, ShellEvent, ThemeFade,
};

impl Shell {
    /// Writing a settings value (the enforcement point). The gate = the key name. On a failure,
    /// `UnlockRequested { then: Set }`. It updates [`Shell::settings`], calls the backend of a
    /// built-in key and emits [`ShellEvent::SettingChanged`]. The shell writes no files: keeping
    /// the value past a restart is the integrator's, and [`Shell::restore_settings`] hands it back.
    pub fn set_setting(&mut self, key: SettingKey, value: SettingValue) {
        self.contain_fault(|shell| shell.set_setting_inner(key, value));
    }
    pub(super) fn set_setting_inner(&mut self, key: SettingKey, value: SettingValue) {
        self.observe_subject_change(&mut None);
        if !self.setting_allowed(&key, || LaunchAction::Set(key.clone(), value.clone())) {
            return;
        }
        self.write_setting(key, value);
    }
    /// **Put saved settings back** — at start, from wherever the device keeps them.
    ///
    /// The shell writes no files. A change reaches you as [`ShellEvent::SettingChanged`], to store
    /// where your device keeps such things, and this hands what you stored back. Each value goes
    /// into [`Shell::settings`], and a built-in key reaches its backend, the theme or the language
    /// as a change would — the theme at once, without the crossfade.
    ///
    /// Unlike [`Shell::set_setting`] it **checks no gate** — the values passed one when they were
    /// set, and it is the integrator restoring them, not the person at the panel — and it **emits
    /// no `SettingChanged`**, since they are stored already. `radio.airplane` is applied after the
    /// rest and only ever turns the radios off: airplane mode off leaves each radio to its own
    /// key.
    ///
    /// ```no_run
    /// # fn load() -> Vec<(String, fairing::SettingValue)> { Vec::new() }
    /// # fn start(shell: &mut fairing::Shell) {
    /// shell.restore_settings(load()); // what your `SettingChanged` handler stored
    /// # }
    /// ```
    pub fn restore_settings<K: Into<SettingKey>>(
        &mut self,
        values: impl IntoIterator<Item = (K, SettingValue)>,
    ) {
        if self.faulted {
            return;
        }
        let mut airplane = false;
        for (key, value) in values {
            let key = key.into();
            match (key.0.as_ref(), &value) {
                (keys::RADIO_AIRPLANE, on) => airplane = matches!(on, SettingValue::Bool(true)),
                (keys::THEME_DARK, SettingValue::Bool(dark)) => self.snap_theme_dark(*dark),
                _ => self.apply_setting_to_backend(&key, &value),
            }
            self.settings.set(key, value);
        }
        if airplane {
            let key = SettingKey::from(keys::RADIO_AIRPLANE);
            self.apply_setting_to_backend(&key, &SettingValue::Bool(true));
            for radio in [keys::WIFI_ENABLED, keys::BLUETOOTH_ENABLED] {
                self.settings.set(radio, SettingValue::Bool(false));
            }
        }
    }
    /// Switch between dark and light **without** the crossfade — a restored theme is the one the
    /// panel starts in, not a change to watch.
    pub(super) fn snap_theme_dark(&mut self, dark: bool) {
        self.theme.dark = dark;
        let palette = if dark {
            self.palettes.0
        } else {
            self.palettes.1
        };
        self.theme.palette = palette;
        self.theme_fade = ThemeFade::idle(palette);
        self.theme_dirty = true;
    }
    /// `Toggle(key)` first inverts an accepted pending Bool target, then the current backend
    /// value for built-in keys, and otherwise the memory view. Going by the memory view alone
    /// would make a fresh shell turn already-enabled Wi-Fi on again.
    pub(super) fn toggle_setting(&mut self, key: SettingKey) {
        if !self.setting_allowed(&key, || LaunchAction::Toggle(key.clone())) {
            return;
        }
        let current = self
            .pending_bools
            .get(&key)
            .map(|pending| pending.desired)
            .or_else(|| self.builtin_bool(&key))
            .or_else(|| match self.settings.get(&key) {
                None => Some(false),
                Some(SettingValue::Bool(b)) => Some(*b),
                Some(other) => {
                    log::warn!("Toggle: setting `{}` is not a Bool ({other:?})", key.0);
                    None
                }
            });
        let Some(current) = current else {
            return;
        };
        self.write_setting(key, SettingValue::Bool(!current));
    }
    /// A built-in Bool key's **current value** — only for keys the backend is the single source of (for the `Toggle` decision).
    pub(super) fn builtin_bool(&self, key: &SettingKey) -> Option<bool> {
        match key.0.as_ref() {
            keys::WIFI_ENABLED => Some(self.services.wifi.enabled()),
            keys::BLUETOOTH_ENABLED => Some(self.services.bluetooth.enabled()),
            keys::AUDIO_MUTED => self.services.audio.volume().map(|audio| audio.muted),
            keys::THEME_DARK => Some(self.theme.dark),
            _ => None,
        }
    }
    /// At most one unconfirmed command per key: a snapshot equal to the original state
    /// cannot acknowledge a later command while an earlier, opposite command is in flight.
    pub(super) fn reconcile_pending_bools(&mut self) {
        let pending = std::mem::take(&mut self.pending_bools);
        for (key, request) in pending {
            self.reconcile_bool(key, request);
        }
    }
    pub(super) fn reconcile_bool(&mut self, key: SettingKey, mut request: PendingBool) {
        let reported = self.builtin_bool(&key);
        let failed = reported != Some(request.sent) && self.wifi_failed(&key);
        match bool_ack::reconcile(
            request.sent,
            request.desired,
            request.was_failed,
            reported,
            failed,
        ) {
            bool_ack::Decision::Send(desired) => self.send_bool_target(&key, desired),
            bool_ack::Decision::Keep(failed) => {
                request.was_failed = failed;
                self.pending_bools.insert(key, request);
            }
            bool_ack::Decision::Complete => {}
        }
    }
    pub(super) fn wifi_failed(&self, key: &SettingKey) -> bool {
        key.0.as_ref() == keys::WIFI_ENABLED
            && matches!(
                self.services.wifi.snapshot().state,
                crate::services::WifiState::Failed { .. }
            )
    }
    pub(super) fn accept_bool_target(&mut self, key: &SettingKey, desired: bool) {
        if self.builtin_bool(key).is_none() {
            // Audio may accept writes without reporting a state. There is no acknowledgement
            // to wait for; keep dispatching in order and let SettingsView carry the intent.
            self.pending_bools.remove(key);
            self.send_bool_target(key, desired);
            return;
        }
        if let Some(mut pending) = self.pending_bools.remove(key) {
            pending.desired = desired;
            self.reconcile_bool(key.clone(), pending);
        } else {
            self.send_bool_target(key, desired);
        }
    }
    pub(super) fn send_bool_target(&mut self, key: &SettingKey, desired: bool) {
        // An already satisfied target is a no-op. Sending it would have no observable
        // acknowledgement and could remain outstanding behind a later external change.
        let reported = self.builtin_bool(key);
        if reported == Some(desired) {
            return;
        }
        let was_failed = self.wifi_failed(key);
        let result = match key.0.as_ref() {
            keys::WIFI_ENABLED => self.services.wifi.set_enabled(desired),
            keys::BLUETOOTH_ENABLED => self.services.bluetooth.set_enabled(desired),
            keys::AUDIO_MUTED => self.services.audio.set_muted(desired),
            _ => return,
        };
        if result.is_ok() && reported.is_some() && self.builtin_bool(key) != Some(desired) {
            self.pending_bools.insert(
                key.clone(),
                PendingBool {
                    sent: desired,
                    desired,
                    was_failed,
                },
            );
        }
        Self::note_setting_error(key, result);
    }
    pub(super) fn setting_allowed(
        &mut self,
        key: &SettingKey,
        then: impl FnOnce() -> LaunchAction,
    ) -> bool {
        let gate = Gate::from(&*key.0);
        if self.access.allows(&gate) {
            return true;
        }
        self.request_unlock(gate, Some(then()));
        false
    }
    /// Refresh the memory view, plus the backend wiring for built-in keys, plus a
    /// `SettingChanged` — the integrator's cue to store the value.
    pub(super) fn write_setting(&mut self, key: SettingKey, value: SettingValue) {
        self.settings.set(key.clone(), value.clone());
        if key.0.as_ref() == keys::RADIO_AIRPLANE {
            let on = matches!(value, SettingValue::Bool(true));
            for radio in [keys::WIFI_ENABLED, keys::BLUETOOTH_ENABLED] {
                self.write_setting(radio.into(), SettingValue::Bool(!on));
            }
            self.events.push(ShellEvent::SettingChanged { key, value });
            return;
        }
        self.apply_setting_to_backend(&key, &value);
        // Enabling one radio leaves airplane mode without changing the other radio.
        if matches!(key.0.as_ref(), keys::WIFI_ENABLED | keys::BLUETOOTH_ENABLED)
            && matches!(value, SettingValue::Bool(true))
            && self.settings.get(&keys::RADIO_AIRPLANE.into()) == Some(&SettingValue::Bool(true))
        {
            self.settings
                .set(keys::RADIO_AIRPLANE, SettingValue::Bool(false));
            self.events.push(ShellEvent::SettingChanged {
                key: keys::RADIO_AIRPLANE.into(),
                value: SettingValue::Bool(false),
            });
        }
        self.events.push(ShellEvent::SettingChanged { key, value });
    }
    /// A built-in key → the backend ("call the related backend"). A failure is only logged — the value is already in memory.
    pub(super) fn apply_setting_to_backend(&mut self, key: &SettingKey, value: &SettingValue) {
        let as_bool = matches!(value, SettingValue::Bool(true));
        if matches!(
            key.0.as_ref(),
            keys::WIFI_ENABLED | keys::BLUETOOTH_ENABLED | keys::AUDIO_MUTED
        ) {
            self.accept_bool_target(key, as_bool);
            return;
        }
        let as_u8 = match value {
            SettingValue::Int(v) => u8::try_from((*v).clamp(0, 100)).unwrap_or(100),
            SettingValue::Float(v) => percent_from_f64(*v),
            SettingValue::Bool(b) => u8::from(*b) * 100,
            SettingValue::Text(_) => 0,
        };
        let result = match key.0.as_ref() {
            keys::DISPLAY_BRIGHTNESS => self.services.display.set_brightness(as_u8),
            keys::AUDIO_VOLUME => self.services.audio.set_volume(as_u8),
            keys::RADIO_AIRPLANE => {
                for radio in [keys::WIFI_ENABLED, keys::BLUETOOTH_ENABLED] {
                    self.apply_setting_to_backend(&radio.into(), &SettingValue::Bool(!as_bool));
                }
                return;
            }
            keys::THEME_DARK => {
                self.set_theme_dark(as_bool);
                Ok(())
            }
            keys::UI_LOCALE => {
                if let SettingValue::Text(locale) = value {
                    self.set_locale(locale);
                }
                Ok(())
            }
            _ => Ok(()),
        };
        Self::note_setting_error(key, result);
    }
    pub(super) fn note_setting_error(key: &SettingKey, result: crate::services::ServiceResult) {
        // `Unsupported` is an absence, not a failure: with the `Null` audio backend the integrator
        // applies `audio.volume` from `SettingChanged` instead, and every slider step would warn.
        match result {
            Err(e) if e.kind != crate::services::ErrorKind::Unsupported => {
                log::warn!("setting `{}` did not reach the backend: {e}", key.0);
            }
            _ => {}
        }
    }
    /// Switch the language (the `ui.locale` setting): the next frame draws in it.
    pub(super) fn set_locale(&mut self, locale: &str) {
        if !self.strings.set_locale(locale) {
            log::warn!(
                "ui.locale = \"{locale}\" has no table - staying with \"{}\" (add one with ShellBuilder::translations)",
                self.strings.locale()
            );
            return;
        }
        self.status_bar.forget_texts();
    }
}
