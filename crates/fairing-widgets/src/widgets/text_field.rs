//! `TextField` — a single- or multi-line input field that follows the theme.
//!
//! It wraps `egui::TextEdit`, for one reason: **egui's field height comes from the text line
//! height and has nothing to do with a touch target.** Left at the default it is a 25 du field
//! against 16 du body text, and a gloved 13 mm finger is twice that. Putting
//! [`crate::theme::Metrics::touch_target`] under it as a floor is all this widget does; the
//! colours, corners, hint text and focus ring are whatever
//! [`Theme::egui_style`](crate::theme::Theme::egui_style) already painted.
//!
//! ```no_run
//! # use fairing_widgets::{widgets::TextField, WidgetCx};
//! # fn ui(ui: &mut egui::Ui, cx: &mut WidgetCx<'_>, name: &mut String) {
//! TextField::new(name).hint("Machine name").show(ui, cx);
//! # }
//! ```

use super::TextFieldLook;
use crate::cx::WidgetCx as Cx;
use crate::unit::round_i8;

use egui::Response;

/// The field's minimum height = `touch_target × this`. At 1.0 it is exactly one finger.
const HEIGHT: f32 = 1.0;
/// The default line count for a multi-line field.
const LINES: usize = 3;
/// The f32 form of [`LINES`], for the vertical maths. Two call sites need it, so the two
/// constants are kept in step here, once.
const LINES_F: f32 = 3.0;

/// An input field that follows the theme.
pub struct TextField<'a> {
    text: &'a mut String,
    hint: Option<String>,
    multiline: bool,
    width: Option<f32>,
    id_salt: Option<&'static str>,
    password: bool,
    enabled: bool,
}

impl std::fmt::Debug for TextField<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextField")
            .field("hint", &self.hint)
            .field("multiline", &self.multiline)
            .field("width", &self.width)
            .field("password", &self.password)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl<'a> TextField<'a> {
    /// Edits `text`.
    pub fn new(text: &'a mut String) -> Self {
        Self {
            text,
            hint: None,
            multiline: false,
            width: None,
            id_salt: None,
            password: false,
            enabled: true,
        }
    }

    /// Dimmed guidance shown when it is empty. Drawn in the `Muted` role.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Multi-line. The default height becomes three lines.
    #[must_use]
    pub const fn multiline(mut self, multiline: bool) -> Self {
        self.multiline = multiline;
        self
    }

    /// The width (du). Without it, all the remaining width — which is what you want inside a settings card.
    #[must_use]
    pub const fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Give each field a different value when a screen has more than one.
    #[must_use]
    pub const fn id_salt(mut self, salt: &'static str) -> Self {
        self.id_salt = Some(salt);
        self
    }

    /// Draw it masked, single-line or [`multiline`](Self::multiline).
    #[must_use]
    pub const fn password(mut self, password: bool) -> Self {
        self.password = password;
        self
    }

    /// Enabled.
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw it. On the frame the text changed, `response.changed()`.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let m = cx.theme.metrics;
        let line = ui.text_style_height(&egui::TextStyle::Body);
        let pad_x = m.screen_inset;
        // **A single-line field gives the padding back out of the height.** egui settles a field's height
        // as `padding + line height`, so to get the finger's height the vertical left over has to be given
        // back as padding above and below — otherwise `add_sized` grows only the field and the text clings
        // to the top.
        //
        // **Multi-line is the opposite.** The text flows from the top, so the padding is fixed and the
        // height settled by the line count. Using the single-line arithmetic as it stands floats the hint
        // in the middle of the field and it does not read as an empty note.
        let (height, pad_y) = if self.multiline {
            let pad = m.screen_inset;
            (line.mul_add(LINES_F, pad * 2.0), pad)
        } else {
            let h = m.touch_target * HEIGHT;
            (h, ((h - line) / 2.0).max(0.0))
        };
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let is_password = self.password;
        let (empty, multiline, enabled) = (self.text.is_empty(), self.multiline, self.enabled);
        let margin = egui::Margin::symmetric(round_i8(pad_x), round_i8(pad_y));

        let mut edit = egui::TextEdit::singleline(self.text)
            .hint_text(self.hint.clone().unwrap_or_default())
            .password(self.password)
            .margin(margin)
            .desired_width(width);
        if self.multiline {
            // The multi-line edit is a new builder: it keeps the mask only if it is given it
            // again.
            edit = egui::TextEdit::multiline(self.text)
                .hint_text(self.hint.unwrap_or_default())
                .password(self.password)
                .margin(margin)
                .desired_width(width)
                .desired_rows(LINES);
        }
        let custom = cx
            .painters
            .as_deref_mut()
            .and_then(|p| p.text_field.as_mut());
        let response = if let Some(custom) = custom {
            // A painter draws the field and egui's `TextEdit` the text over it, so the field is
            // drawn first: its rect is taken before the edit, and the edit is given its id so the
            // focus can be read before it runs.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            let id = self
                .id_salt
                .map_or_else(|| ui.next_auto_id(), |salt| ui.make_persistent_id(salt));
            custom(
                ui.painter(),
                &mut TextFieldLook {
                    rect,
                    focused: ui.memory(|m| m.has_focus(id)),
                    empty,
                    password: is_password,
                    multiline,
                    enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            // Given a frame, egui leaves out the edit's margin: the padding goes in the frame.
            let edit = edit.id(id).frame(egui::Frame::NONE.inner_margin(margin));
            ui.add_enabled_ui(enabled, |ui| ui.put(rect, edit)).inner
        } else {
            if let Some(salt) = self.id_salt {
                edit = edit.id_salt(salt);
            }
            ui.add_enabled_ui(enabled, |ui| ui.add_sized([width, height], edit))
                .inner
        };
        // Selecting text is a drag the field owns, and it says so: the rail's page swipe leaves
        // it alone.
        crate::drag::claim_if_held(&response);
        if is_password {
            Self::forget_undo_history(ui, response.id);
        }
        response
    }

    /// **A password field keeps no undo history.**
    ///
    /// egui feeds the raw text into the undoer every frame whatever `password` says
    /// (`text_edit/builder.rs` calls `feed_state` with `text.as_str().to_owned()` before and after
    /// handling input). The clipboard is guarded by `copy_if_not_password` and the accessibility
    /// text by `mask_if_password` — the undoer is not. So up to `Undoer::max_undos` (100) plaintext
    /// snapshots would sit in egui's memory under this widget's id and **outlive the field**, where
    /// no wrapper the caller puts round its own buffer can reach them.
    ///
    /// It is cleared every frame rather than when the field closes: there is then no path where a
    /// caller forgets, or where the field goes away with the last snapshots still in memory. The
    /// only thing lost is undo and redo inside a password field, which is the usual behaviour.
    fn forget_undo_history(ui: &egui::Ui, id: egui::Id) {
        if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), id) {
            state.clear_undoer();
            state.store(ui.ctx(), id);
        }
    }
}
