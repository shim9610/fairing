//! `TextField` wraps egui text editing with Fairing sizing and styling.
//!
//! Its height has a floor from [`crate::theme::Metrics::touch_target`]. Colours,
//! corners, hint text and focus styling use
//! [`Theme::egui_style`](crate::theme::Theme::egui_style). The application supplies
//! the text buffer and validates its contents.
//!
//! ```no_run
//! # use fairing_widgets::{widgets::TextField, WidgetCx};
//! # fn ui(ui: &mut egui::Ui, cx: &mut WidgetCx<'_>, name: &mut String) {
//! TextField::new(name).hint("Machine name").show(ui, cx);
//! # }
//! ```

use super::TextFieldLook;
use crate::cx::WidgetCx as Cx;
use crate::icons::{builtin, IconColor, IconStyle};
use crate::theme::ColorRole;
use crate::unit::round_i8;

use egui::{Rect, Response, Vec2};

/// **Which keys a field is typed on** — what it asks the on-screen keyboard for while it has the
/// focus.
///
/// The keyboard's layout is the device's (`[osk] layout`), and that one layout is wrong for some
/// fields whatever it is: a PIN on a qwerty, a password on a number pad. A field that knows what
/// it takes says so here, and the shell's keyboard shows those keys while the field has the focus
/// and goes back to the device's layout when a field that asks for nothing takes it.
///
/// The crate's own fields ask for themselves ([`TextField::keys`], [`NumberField`](super::NumberField)
/// from its range and decimals). A field of an integrator's own — a raw `egui::TextEdit` — asks
/// with [`Keys::ask`] each frame it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Keys {
    /// Whatever the device's layout is.
    #[default]
    Any,
    /// The digits 0-9 and nothing else — a PIN.
    Digits,
    /// A number pad, with a decimal point key and a minus key where asked for.
    Number {
        /// A decimal point key.
        decimal: bool,
        /// A `-` key.
        sign: bool,
    },
    /// The English letters, digits and symbols — a password, a user name, a host name. On a
    /// Hangul device the `한/영` key stays, so a person can still cross over.
    Latin,
}

impl Keys {
    fn slot(id: egui::Id) -> egui::Id {
        id.with("fairing.keys")
    }

    /// Ask for these keys for the field `id` (the field's `Response::id`). Called each frame the
    /// field is drawn; the latest ask stands.
    pub fn ask(self, ctx: &egui::Context, id: egui::Id) {
        crate::interaction::store(ctx, Self::slot(id), self);
    }

    /// What the field `id` asked for — [`Keys::Any`] if it never asked or its request expired.
    /// The owner renews its request each drawn pass; reading this value does not renew it.
    #[must_use]
    pub fn of(ctx: &egui::Context, id: egui::Id) -> Self {
        crate::interaction::load(
            ctx,
            Self::slot(id),
            true,
            crate::interaction::Lifetime::Mounted,
        )
        .unwrap_or_default()
    }
}

/// The field's minimum height = `touch_target × this`. At 1.0 it is exactly one finger.
const HEIGHT: f32 = 1.0;
/// The default line count for a multi-line field.
const LINES: usize = 3;
/// The f32 form of [`LINES`], for the vertical maths. Two call sites need it, so the two
/// constants are kept in step here, once.
const LINES_F: f32 = 3.0;

/// An input field that follows the theme.
///
/// # The reveal button
///
/// A single-line [`password`](TextField::password) field carries an eye at its right end: a tap
/// shows what was typed, another hides it. On a touch panel every key is a guess at a small target,
/// and a person who cannot see a masked password cannot tell a typo from a wrong password.
///
/// The eye is **part of the field, not a widget beside it.** egui hands a press to the topmost
/// widget and takes the focus off a text edit that is not under the press, so a separate button
/// would drop the focus — and the on-screen keyboard with it — on every tap. Inside the field it is
/// a stretch of the edit's own margin: the tap lands on the edit, which keeps (or takes) the focus,
/// and the field reads where the tap fell.
///
/// What it shows lasts while the field is on screen: a field left undrawn for a frame (the prompt
/// closed, the screen changed) comes back masked, so the next person does not find the last
/// person's password in the clear. [`TextField::reveal`] turns the eye off.
#[allow(clippy::struct_excessive_bools)] // Independent builder options, not a state machine.
pub struct TextField<'a> {
    text: &'a mut String,
    hint: Option<String>,
    multiline: bool,
    width: Option<f32>,
    id_salt: Option<&'static str>,
    password: bool,
    reveal: bool,
    enabled: bool,
    keys: Keys,
}

/// Whether a password field shows its text, and the pass it was last drawn on — a gap means the
/// field went away, and it comes back masked.
#[derive(Debug, Clone, Copy)]
struct Reveal;

impl Reveal {
    fn slot(id: egui::Id) -> egui::Id {
        id.with("fairing.reveal")
    }
    fn shown(ctx: &egui::Context, id: egui::Id, enabled: bool) -> bool {
        crate::interaction::load::<bool>(
            ctx,
            Self::slot(id),
            enabled,
            crate::interaction::Lifetime::Mounted,
        )
        .unwrap_or(false)
    }
    fn store(ctx: &egui::Context, id: egui::Id, shown: bool) {
        crate::interaction::store(ctx, Self::slot(id), shown);
    }
}

/// The rect of a single-line field's reveal button: a square at the right end, the field's height
/// on a side.
fn eye_rect(field: Rect) -> Rect {
    let side = field.height().min(field.width());
    Rect::from_min_size(
        egui::pos2(field.right() - side, field.top()),
        Vec2::splat(side),
    )
}

impl std::fmt::Debug for TextField<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextField")
            .field("hint", &self.hint)
            .field("multiline", &self.multiline)
            .field("width", &self.width)
            .field("password", &self.password)
            .field("reveal", &self.reveal)
            .field("enabled", &self.enabled)
            .field("keys", &self.keys)
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
            reveal: true,
            enabled: true,
            keys: Keys::Any,
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

    /// Give each field a stable source for its egui interaction-state identifier.
    /// Use a different value for each field on the same screen.
    #[must_use]
    pub const fn id_salt(mut self, id_source: &'static str) -> Self {
        self.id_salt = Some(id_source);
        self
    }

    /// Draw it masked, single-line or [`multiline`](Self::multiline). A single-line one carries
    /// the [reveal button](TextField#the-reveal-button) unless [`reveal`](Self::reveal) turns it off.
    #[must_use]
    pub const fn password(mut self, masked: bool) -> Self {
        self.password = masked;
        self
    }

    /// The eye that shows a [`password`](Self::password) field's text — on by default; `false`
    /// leaves the field masked with no way to look. Shown, the text can also be copied, as egui
    /// guards the clipboard by the mask.
    #[must_use]
    pub const fn reveal(mut self, reveal: bool) -> Self {
        self.reveal = reveal;
        self
    }

    /// Enabled.
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The keys it is typed on — see [`Keys`]. [`Keys::Any`] (the device's layout) by default.
    #[must_use]
    pub const fn keys(mut self, keys: Keys) -> Self {
        self.keys = keys;
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
        let (empty, multiline, enabled) = (
            self.text.is_empty(),
            self.multiline,
            self.enabled && ui.is_enabled(),
        );
        let eye = is_password && self.reveal && !multiline;
        // The reveal state is read before the edit is built (it decides the mask), so a field with
        // an eye names its id up front — the same id the painter path already names.
        let id = (eye
            || cx
                .painters
                .as_deref()
                .is_some_and(|p| p.text_field.is_some()))
        .then(|| {
            self.id_salt
                .map_or_else(|| ui.next_auto_id(), |salt| ui.make_persistent_id(salt))
        });
        let shown = eye && id.is_some_and(|id| Reveal::shown(ui.ctx(), id, enabled));
        let mut margin = egui::Margin::symmetric(round_i8(pad_x), round_i8(pad_y));
        if eye {
            // The eye is the edit's own right margin, so a tap on it is a tap on the field.
            margin.right = round_i8(height.min(width));
        }

        let mut edit = egui::TextEdit::singleline(self.text)
            .hint_text(self.hint.clone().unwrap_or_default())
            .password(is_password && !shown)
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
        let response = if let (Some(custom), Some(id)) = (custom, id) {
            // A painter draws the field and egui's `TextEdit` the text over it, so the field is
            // drawn first: its rect is taken before the edit, and the edit is given its id so
            // the focus can be read before it runs.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            custom(
                ui.painter(),
                &mut TextFieldLook {
                    rect,
                    focused: ui.memory(|m| m.has_focus(id)),
                    empty,
                    password: is_password,
                    reveal: eye.then_some(shown),
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
            if let Some(id) = id {
                edit = edit.id(id);
            } else if let Some(salt) = self.id_salt {
                edit = edit.id_salt(salt);
            }
            ui.add_enabled_ui(enabled, |ui| ui.add_sized([width, height], edit))
                .inner
        };
        // Selecting text is a drag the field owns, and it says so: the rail's page swipe leaves
        // it alone.
        crate::drag::claim_if_held(&response);
        if enabled {
            self.keys.ask(ui.ctx(), response.id);
        } else {
            let _ = crate::interaction::take::<Keys>(ui.ctx(), Keys::slot(response.id));
            crate::interaction::forget_focus(ui.ctx(), response.id);
        }
        if eye {
            Self::eye(ui, cx, &response, shown);
        }
        if is_password {
            Self::forget_undo_history(ui, response.id);
        }
        response
    }

    /// The reveal button: flip on a tap that fell on it, then draw the glyph for what a tap will do
    /// next — an eye while the text is masked, the struck eye while it shows.
    fn eye(ui: &egui::Ui, cx: &mut Cx<'_>, response: &Response, shown: bool) {
        let at = eye_rect(response.rect);
        let on_eye = |pos: Option<egui::Pos2>| pos.is_some_and(|p| at.contains(p));
        let shown = shown ^ (response.clicked() && on_eye(response.interact_pointer_pos()));
        Reveal::store(ui.ctx(), response.id, shown);
        let pressed =
            response.is_pointer_button_down_on() && on_eye(ui.input(|i| i.pointer.interact_pos()));
        let c = &cx.theme.control;
        if pressed {
            ui.painter().circle_filled(
                at.center(),
                c.icon_button * 0.5,
                cx.theme.color(ColorRole::Pressed),
            );
        }
        let ink = if response.enabled() {
            ColorRole::OnSurface
        } else {
            ColorRole::Muted
        };
        let icon = if shown {
            builtin::EYE_OFF
        } else {
            builtin::EYE
        };
        let style = IconStyle {
            color: IconColor::Fixed(cx.theme.color(ink)),
            ..IconStyle::default()
        };
        let glyph = Rect::from_center_size(at.center(), Vec2::splat(c.icon));
        cx.icons.paint(ui.painter(), glyph, &icon, &style, cx.theme);
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
