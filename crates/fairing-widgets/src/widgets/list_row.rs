//! `ListRow` — a settings row (icon · title · subtitle · trailing text or chevron). 56 high,
//! with the A7 press tint.
//!
//! The title, subtitle and trailing text are elided with `…` within the width left to them. A
//! thin separator runs under the row, so stacking them still shows the boundaries. A disabled
//! row is muted and takes no taps (it keeps its space — the same rule as a nav bar item with no
//! function behind it).
// The trailing **icon** slot was opened along with M2b's built-in settings screens — drawing a
// selection mark (`check`) as a glyph gives tofu (□) where an integrator's typeface has not got it.
//
// **An arbitrary widget slot is not opened**. Wanting a quantity stepper or a star rating on a
// row makes it screen content and the integrator's — they can draw their own row inside a
// `ui.horizontal` instead of a `ListRow`, and the shape is not settled enough for this crate to draw it
// for them. Left as "we will open it later", one slot becomes the doorway to a combinatorial explosion.

use super::RowLook;
use crate::cx::WidgetCx as Cx;
use crate::icons::IconRef;
use crate::theme::{ColorRole, Theme};
use egui::{Response, Sense, Vec2};

// The three dimensions (`pad` · `two_line_offset` · `chevron_w`) are `theme.components.list_row`.
// `SEPARATOR_PX` is a hairline and stays a global.
/// The separator's thickness (px).
const SEPARATOR_PX: f32 = 1.0;
/// What [`ListRow::paint_trailing`] needs. A struct rather than seven parameters, which is over
/// clippy's arity limit and past what a reader can hold anyway.
struct Trailing<'a> {
    /// The row's whole rect.
    rect: egui::Rect,
    /// The body font, for the chevron and the trailing value.
    body: &'a egui::FontId,
    /// The muted colour those two take.
    muted: egui::Color32,
    /// Where the title starts, so a trailing value knows how much room it may take.
    text_left: f32,
    /// The row's own id. A composed control remembers its animation under it, so the knob's travel
    /// belongs to the row rather than restarting each frame.
    row_id: egui::Id,
}

/// How a row is set apart from its neighbours. The two travel together on a navigation list —
/// `fairing_widgets::layout::list_item` badges every entry and bolds the selected one — and neither is set
/// on a content row, which is why they are one field rather than two loose flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Emphasis {
    /// The leading icon sits in a tinted round badge.
    badge: bool,
    /// The title is drawn in the bold face.
    strong: bool,
}

/// The badge's diameter as a multiple of the icon it holds (see [`ListRow::icon_badge`]).
const BADGE_RATIO: f32 = 1.75;

/// **The least a row can be and still hold its own text**, from the live `Ui`'s resolved styles.
///
/// `row_height`, `touch_target` and `type_scale` all derive from `finger_mm` in the default spec,
/// so at every panel size they move together and this floor never binds. It binds when an
/// integrator raises the text on its own - rung 2 of the override ladder, which is what a
/// low-vision setting is - and without it a two-line row kept the finger's height while its title
/// and subtitle grew straight through the bottom of the card.
fn text_floor(ui: &egui::Ui, cx: &Cx<'_>, two_line: bool) -> f32 {
    let body = ui.text_style_height(&egui::TextStyle::Body);
    let block = if two_line {
        body + cx.theme.control.line_gap + ui.text_style_height(&egui::TextStyle::Small)
    } else {
        body
    };
    block + cx.theme.control.gap * 2.0
}

/// Where [`ListRow::paint_text`] puts the title and the subtitle. A struct rather than eight
/// parameters, which is past clippy's arity limit and past what a reader can hold.
struct TextPlace {
    /// The text column's left edge.
    x: f32,
    /// How wide the text may run before it is truncated.
    max_w: f32,
    /// The row's vertical centre.
    centre: f32,
    /// The title's font (bold on an emphasised row).
    title_font: egui::FontId,
    /// The title's ink.
    title_color: egui::Color32,
    /// The subtitle's ink.
    muted: egui::Color32,
    /// The token floor for how far the two lines part.
    two_line_offset: f32,
}

/// A settings row.
#[derive(Debug, Clone)]
pub struct ListRow {
    title: String,
    /// Paints the title in this role instead of `OnSurface` — see `ListRow::title_role`.
    title_role: Option<ColorRole>,
    subtitle: Option<String>,
    icon: Option<IconRef>,
    trailing: Option<String>,
    trailing_icon: Option<IconRef>,
    trailing_switch: Option<bool>,
    trailing_radio: Option<bool>,
    icon_color: Option<ColorRole>,
    chevron: bool,
    /// A disclosure chevron at the far right — down closed, up open — in place of `›`. See
    /// [`ListRow::disclosure`].
    disclosure: Option<bool>,
    /// Where the trailing switch was drawn this frame, for [`RowPress::switch`].
    switch_slot: Option<egui::Rect>,
    emphasis: Emphasis,
    separator: bool,
    enabled: bool,
    height: Option<f32>,
}

impl ListRow {
    /// The title.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            icon: None,
            trailing: None,
            trailing_icon: None,
            trailing_switch: None,
            trailing_radio: None,
            icon_color: None,
            chevron: true,
            disclosure: None,
            switch_slot: None,
            title_role: None,
            emphasis: Emphasis::default(),
            separator: true,
            enabled: true,
            height: None,
        }
    }

    /// The row height (du). The default is `max(row_height, widget_height)`.
    ///
    /// **It can only go up.** A value below `touch_target` is raised to it — that is the floor
    /// the gloved-hand policy sets, not something to break for one pretty
    /// screen. To tighten the spacing, the thing to touch is the card (`Deco::padding`), not
    /// this.
    ///
    /// Going up is free — a row with a two-line subtitle, or a large produce list, is a place
    /// where one row has to be bigger.
    #[must_use]
    pub fn height(mut self, du: f32) -> Self {
        self.height = Some(du);
        self
    }

    /// The subtitle.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// The icon.
    #[must_use]
    pub fn icon(mut self, icon: IconRef) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The value text on the right.
    #[must_use]
    pub fn trailing(mut self, text: impl Into<String>) -> Self {
        self.trailing = Some(text.into());
        self
    }

    /// Put a [`Switch`](super::Switch) in the trailing position, showing `on`.
    ///
    /// # Why a typed slot and not a widget slot
    ///
    /// Opening an arbitrary trailing-widget slot was refused on the grounds that one slot
    /// becomes the doorway to a combinatorial explosion. That reasoning holds, and the cost of the
    /// refusal was paid anyway: `layout::switch_row` hand-drew the whole row instead, so its label
    /// sat four du off a `ListRow`'s in the same card, and a fix to either reached only one of
    /// them. A slot per control the crate itself ships is neither of those things — it is a closed
    /// set, and it is the only way a widget-level fix reaches the built-in screens.
    ///
    /// **The row owns the tap.** The switch is drawn, not shown: it allocates nothing, senses
    /// nothing, and does not grow under the press (Rule 1.3). Pressing anywhere on the row is the
    /// toggle — with only the switch live, a gloved hand cannot hit it — so the caller flips its own
    /// `bool` when the returned [`Response`] reports a click.
    #[must_use]
    pub fn trailing_switch(mut self, on: bool) -> Self {
        self.trailing_switch = Some(on);
        self
    }

    /// A trailing [`Radio`](crate::widgets::Radio) — one row of a pick-one list.
    ///
    /// The second member of the closed set [`Self::trailing_switch`] opened, and for the same
    /// reason: a list of mutually exclusive choices is exactly a radio group, and drawing a bare
    /// tick on the chosen row leaves every other row with **no affordance at all** — nothing says
    /// the row can be chosen until it already has been.
    ///
    /// **The row owns the tap**, as with the switch: the radio is drawn, not shown. It allocates
    /// nothing and senses nothing, so a gloved hand lands on the row rather than on a 3 mm circle.
    #[must_use]
    pub fn trailing_radio(mut self, selected: bool) -> Self {
        self.trailing_radio = Some(selected);
        self
    }

    /// A trailing icon (a selection mark, say). Drawn **inside** the chevron.
    ///
    /// An icon rather than a glyph (`"✓"`) is the point — with the integrator's own typeface
    /// installed, a missing glyph comes out as tofu (□) (guide 09 §3).
    #[must_use]
    pub fn trailing_icon(mut self, icon: IconRef) -> Self {
        self.trailing_icon = Some(icon);
        self
    }

    /// The leading icon's colour role. The default is `OnSurface`, the same as the title — where
    /// the icon distinguishes the entries, as in a settings list, give it `Primary` so the eye
    /// can scan by colour.
    #[must_use]
    pub fn icon_color(mut self, role: ColorRole) -> Self {
        self.icon_color = Some(role);
        self
    }

    /// **Paint the title in this role** instead of [`ColorRole::OnSurface`].
    ///
    /// For a selected rail entry, where a console carries the selection in the accent rather than
    /// in weight. A disabled row still goes [`ColorRole::Muted`] — "you cannot press
    /// this" outranks "this is the one you are on".
    #[must_use]
    pub fn title_role(mut self, role: ColorRole) -> Self {
        self.title_role = Some(role);
        self
    }

    /// The separator under the row. Turn it off for a list grouped in a card,
    /// where the card's boundary expresses the group — a line as well splits the inside of one
    /// card in two.
    #[must_use]
    pub fn separator(mut self, separator: bool) -> Self {
        self.separator = separator;
        self
    }

    /// Set the leading icon in a **tinted round badge**.
    ///
    /// Off by default, and deliberately narrow. One UI badges the icons in its settings *sidebar*
    /// and leaves the icons inside a content card as bare glyphs; iOS does the same thing with a
    /// rounded square. A badge on every icon on every row turns a list into a sticker sheet, so
    /// this is for a navigation list — `fairing_widgets::layout::list_item` switches it on.
    #[must_use]
    pub fn icon_badge(mut self, icon_badge: bool) -> Self {
        self.emphasis.badge = icon_badge;
        self
    }

    /// Draw the title in the **bold** face ([`crate::theme::Theme::strong`]).
    ///
    /// Off by default, and it should stay off for an ordinary row: iOS and One UI both set list-row
    /// labels in the regular weight and spend bold on the one row that is *selected*, plus the
    /// screen title. Bold on every row is the same flatness as bold on none.
    #[must_use]
    pub fn strong(mut self, strong: bool) -> Self {
        self.emphasis.strong = strong;
        self
    }

    /// Show the chevron.
    #[must_use]
    pub fn chevron(mut self, chevron: bool) -> Self {
        self.chevron = chevron;
        self
    }

    /// **A row that opens in place**, not one that goes somewhere: a chevron at the far right
    /// pointing down while closed and up while `open`, in place of `›`. The two are kept apart
    /// on purpose — `›` says *another screen*, `⌄` says *here, under this row* — which is the
    /// distinction iOS draws between a push row and a disclosure group. Drawn from the
    /// icon set, so it is the same glyph a `Dropdown` turns over.
    #[must_use]
    pub fn disclosure(mut self, open: bool) -> Self {
        self.disclosure = Some(open);
        self.chevron = false;
        self
    }

    /// Enabled.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw it. A tap is `response.clicked()`. Disabled, no tap arrives.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        self.show_with_long_press(ui, cx).response
    }

    /// The right-hand furniture — chevron, trailing icon, trailing text — taken from the right edge
    /// inwards. Returns the x the title may run to. Split out of `show_with_long_press` for length.
    fn paint_trailing(&mut self, ui: &egui::Ui, cx: &mut Cx<'_>, t: &Trailing<'_>) -> f32 {
        let &Trailing {
            rect,
            body,
            muted,
            text_left,
            row_id,
        } = t;
        let c = cx.theme.components.list_row;
        // The **edge** inset is `metrics.content_inset`, the one token that says where content
        // starts inside a container; `components.list_row.pad` keeps the job it is actually named
        // for, the gap between one trailing item and the next. They were the same 16 du number in
        // two places, so raising the layout token moved the card, the note and the title and left
        // every row's label where it was - measured, the label did not shift a pixel.
        let edge = cx.theme.metrics.content_inset;
        let (switch_slot, radio_slot) = self.control_slots(cx.theme, rect);
        let mut right = rect.max.x - edge;
        if self.chevron {
            ui.painter().text(
                egui::pos2(right, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                "›",
                body.clone(),
                muted,
            );
            right -= c.chevron_w;
        }
        if let Some(open) = self.disclosure {
            // The glyph is swapped, not rotated, as the dropdown's is: the crate ships both.
            let icon = if open {
                crate::icons::builtin::CHEVRON_UP
            } else {
                crate::icons::builtin::CHEVRON_DOWN
            };
            let mark = cx.theme.control.icon;
            let at = egui::Rect::from_center_size(
                egui::pos2(right - mark / 2.0, rect.center().y),
                Vec2::splat(mark),
            );
            let style = crate::icons::IconStyle::sized(mark)
                .enabled(self.enabled)
                .color(crate::icons::IconColor::Role(ColorRole::Muted));
            cx.icons.paint(ui.painter(), at, &icon, &style, cx.theme);
            right -= mark + c.pad;
        }
        if let (Some(on), Some(slot)) = (self.trailing_switch.take(), switch_slot) {
            self.switch_slot = Some(slot);
            // A local for the borrow: `Switch` is built around `&mut bool` because it normally owns
            // the toggle, and on this path the row does. `draw` never writes through it.
            let mut shown = on;
            crate::widgets::Switch::new(&mut shown)
                .enabled(self.enabled)
                .draw(ui.painter(), cx, row_id, slot);
            right = slot.min.x - c.pad;
        }
        if let (Some(selected), Some(slot)) = (self.trailing_radio.take(), radio_slot) {
            crate::widgets::Radio::new(selected)
                .enabled(self.enabled)
                .draw(ui.painter(), cx, row_id.with("radio"), slot);
            right = slot.min.x - c.pad;
        }
        if let Some(icon) = self.trailing_icon.take() {
            right -= paint_icon(ui, cx, &icon, right, rect.center().y, self.enabled, None) + c.pad;
        }
        let painter = ui.painter();
        if let Some(trailing) = self.trailing.take() {
            let max_w = ((right - text_left) * 0.5).max(0.0);
            let galley = elided(painter, trailing, body.clone(), max_w);
            let size = galley.size();
            painter.galley(
                egui::pos2(right - size.x, rect.center().y - size.y / 2.0),
                galley,
                muted,
            );
            right -= size.x + c.pad;
        }
        right
    }

    /// A row painter's drawing, then the row's switch or radio over it as its own kind.
    /// Returns whether a painter was given.
    fn paint_custom(
        &mut self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        rect: egui::Rect,
        response: &Response,
    ) -> bool {
        let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.row.as_mut()) else {
            return false;
        };
        let (switch, radio) = self.control_slots(cx.theme, rect);
        custom(
            ui.painter(),
            &mut RowLook {
                rect,
                title: &self.title,
                subtitle: self.subtitle.as_deref(),
                icon: self.icon.as_ref(),
                icon_color: self.icon_color,
                badge: self.emphasis.badge,
                title_role: self.title_role,
                strong: self.emphasis.strong,
                value: self.trailing.as_deref(),
                trailing_icon: self.trailing_icon.as_ref(),
                switch,
                radio,
                chevron: self.chevron,
                disclosure: self.disclosure,
                separator: self.separator,
                pressed: self.enabled && response.is_pointer_button_down_on(),
                enabled: self.enabled,
                theme: cx.theme,
                icons: &mut *cx.icons,
            },
        );
        if let (Some(on), Some(slot)) = (self.trailing_switch, switch) {
            self.switch_slot = Some(slot);
            // The row's own copy, as in `paint_trailing`: `draw` never writes through it.
            let mut shown = on;
            crate::widgets::Switch::new(&mut shown)
                .enabled(self.enabled)
                .draw(ui.painter(), cx, response.id, slot);
        }
        if let (Some(selected), Some(slot)) = (self.trailing_radio, radio) {
            crate::widgets::Radio::new(selected)
                .enabled(self.enabled)
                .draw(ui.painter(), cx, response.id.with("radio"), slot);
        }
        true
    }

    /// Where the row's switch and radio go in `rect`: in from the right edge, past the chevron or
    /// the disclosure mark — the walk [`ListRow::paint_trailing`] draws by.
    fn control_slots(
        &self,
        theme: &Theme,
        rect: egui::Rect,
    ) -> (Option<egui::Rect>, Option<egui::Rect>) {
        let c = theme.components.list_row;
        let mut right = rect.max.x - theme.metrics.content_inset;
        if self.chevron {
            right -= c.chevron_w;
        }
        if self.disclosure.is_some() {
            right -= theme.control.icon + c.pad;
        }
        let switch = self.trailing_switch.map(|_| {
            let size = crate::widgets::Switch::drawn_size(theme);
            let slot = egui::Rect::from_center_size(
                egui::pos2(right - size.x / 2.0, rect.center().y),
                size,
            );
            right = slot.min.x - c.pad;
            slot
        });
        let radio = self.trailing_radio.map(|_| {
            let side = theme.control.box_size();
            egui::Rect::from_center_size(
                egui::pos2(right - side / 2.0, rect.center().y),
                Vec2::splat(side),
            )
        });
        (switch, radio)
    }

    /// The built-in row: the press tint, the leading icon, the furniture at the right, the text
    /// and the separator.
    fn paint_built_in(
        &mut self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        rect: egui::Rect,
        response: &Response,
    ) {
        let title_color = if self.enabled {
            cx.theme
                .color(self.title_role.unwrap_or(ColorRole::OnSurface))
        } else {
            cx.theme.color(ColorRole::Muted)
        };
        let muted = cx.theme.color(ColorRole::Muted);
        if self.enabled && response.is_pointer_button_down_on() {
            // A7: the press tint is immediate on the first frame (a row does not scale — the list would wobble).
            ui.painter()
                .rect_filled(rect, 0.0, cx.theme.color(ColorRole::Pressed));
        }

        let c = cx.theme.components.list_row;
        let edge = cx.theme.metrics.content_inset;
        let mut x = rect.min.x + edge;
        if let Some(icon) = self.icon.take() {
            x += paint_leading(
                ui,
                cx,
                &icon,
                x,
                rect.center().y,
                self.enabled,
                self.icon_color,
                self.emphasis.badge,
            ) + c.pad;
        }

        // The places are taken from the right and what is left over goes to the title.
        let body = egui::TextStyle::Body.resolve(ui.style());
        // Regular unless the row asked for bold — see `ListRow::strong`. With no bold face
        // registered the two are the same font anyway.
        let title_font = if self.emphasis.strong {
            cx.theme.strong(body.size)
        } else {
            body.clone()
        };
        let right = self.paint_trailing(
            ui,
            cx,
            &Trailing {
                rect,
                body: &body,
                muted,
                text_left: x,
                row_id: response.id,
            },
        );
        let painter = ui.painter();
        let max_w = (right - x).max(0.0);
        self.paint_text(
            ui,
            cx,
            painter,
            &TextPlace {
                x,
                max_w,
                centre: rect.center().y,
                title_font,
                title_color,
                muted,
                two_line_offset: c.two_line_offset,
            },
        );
        // The separator (laid below the last row too — the list container clips it).
        if self.separator {
            painter.hline(
                rect.min.x + cx.theme.metrics.content_inset..=rect.max.x,
                rect.max.y - SEPARATOR_PX,
                egui::Stroke::new(SEPARATOR_PX, cx.theme.color(ColorRole::Outline)),
            );
        }
    }

    /// Draw it and time a long press as well ([`RowPress`]).
    ///
    /// The engine's `fairing_widgets::gesture::Gesture::LongPress` reaches the quick
    /// settings tiles and the shade, not a row drawn inside a screen, and egui's
    /// `Response::long_touched()` only stands up where **real touch events** arrive — on a panel
    /// that passes touch through as the mouse it never fires at all. So the row times
    /// its own press, off the same `[gesture] long_press` token the engine uses, and it works the
    /// same way the tiles do: it fires **once** at the threshold, and the release that ends it is
    /// not also a tap.
    pub fn show_with_long_press(mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> RowPress {
        let m = &cx.theme.metrics;
        // **The row is floored against its own text, not only against the finger.** `row_height`
        // and `touch_target` both derive from `finger_mm`, and so does `type_scale`, so at every
        // panel size the three move together and this floor never binds. It binds when an
        // integrator raises the text on its own - rung 2 of the override ladder, which is exactly
        // what a low-vision setting does - and without it a two-line row kept the finger's height
        // while its title and subtitle grew straight through the bottom of the card.
        let height = self
            .height
            .unwrap_or_else(|| m.row_height.max(m.widget_height))
            .max(m.touch_target)
            .max(text_floor(ui, cx, self.subtitle.is_some()));
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), sense);
        if !self.paint_custom(ui, cx, rect, &response) {
            self.paint_built_in(ui, cx, rect, &response);
        }
        // Read before timing: on the releasing frame the pointer is already up, so `long_press`
        // clears the state — the answer to "did this press become a long one" has to be taken first.
        let already_fired = fired_this_press(ui, &response);
        let long_pressed = self.enabled && long_press(ui, cx, &response);
        RowPress {
            // A press that became a long press is not also a tap — the same rule the tiles keep.
            tapped: response.clicked() && !already_fired,
            long_pressed,
            switch: self.switch_slot,
            response,
        }
    }
}

/// What one row reported this frame ([`ListRow::show_with_long_press`]).
#[derive(Debug)]
pub struct RowPress {
    /// The row's response — its `rect`, hover and so on. Prefer [`RowPress::tapped`] to
    /// `response.clicked()`: the click is still true on the release that ends a long press.
    pub response: Response,
    /// A tap. False on the release that ends a long press, so one press never does both.
    pub tapped: bool,
    /// A long press, on the single frame it completes.
    pub long_pressed: bool,
    /// Where the trailing switch was drawn, when the row has one — so a caller whose row does
    /// something *else* on a tap (opens, say) can give the switch's own patch to the switch.
    pub switch: Option<egui::Rect>,
}

/// Where a row's press is remembered between frames: when it started, where, and whether the long
/// press has already fired for it.
type PressState = (f64, egui::Pos2, bool);

/// The key the press state hangs on.
fn press_key(response: &Response) -> egui::Id {
    response.id.with("fairing.list_row.press")
}

/// Whether the long press already fired during the press that is ending now.
fn fired_this_press(ui: &egui::Ui, response: &Response) -> bool {
    ui.data(|d| d.get_temp::<PressState>(press_key(response)))
        .is_some_and(|(_, _, fired)| fired)
}

/// Times the press and answers on the frame it crosses the threshold.
fn long_press(ui: &egui::Ui, cx: &Cx<'_>, response: &Response) -> bool {
    let key = press_key(response);
    if !response.is_pointer_button_down_on() {
        ui.data_mut(|d| d.remove::<PressState>(key));
        return false;
    }
    let now = ui.input(|i| i.time);
    let Some(at) = ui.ctx().pointer_interact_pos() else {
        return false;
    };
    let Some((start, origin, fired)) = ui.data(|d| d.get_temp::<PressState>(key)) else {
        // From where the finger came down: the slop below is measured from there.
        let origin = ui.input(crate::drag::press_point).unwrap_or(at);
        ui.data_mut(|d| d.insert_temp(key, (now, origin, false)));
        return false;
    };
    if fired {
        return false;
    }
    // Moving past the slop is a drag — the list scrolls, and this press is no longer a long one.
    if (at - origin).length() > cx.theme.motion.slop_px {
        ui.data_mut(|d| d.remove::<PressState>(key));
        return false;
    }
    if now - start < cx.theme.motion.long_press.as_secs_f64() {
        return false;
    }
    ui.data_mut(|d| d.insert_temp(key, (start, origin, true)));
    true
}

/// One left-aligned line (vertically centred), elided with `…` past the width left.
fn draw_left(
    painter: &egui::Painter,
    center_left: egui::Pos2,
    text: String,
    font: egui::FontId,
    max_width: f32,
    color: egui::Color32,
) {
    let galley = elided(painter, text, font, max_width);
    let size = galley.size();
    painter.galley(
        egui::pos2(center_left.x, center_left.y - size.y / 2.0),
        galley,
        color,
    );
}

/// A galley truncated with `…` when it exceeds `max_width`.
fn elided(
    painter: &egui::Painter,
    text: String,
    font: egui::FontId,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text, font, egui::Color32::PLACEHOLDER);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(1.0));
    painter.layout_job(job)
}

/// Draw one icon with `right` as its right edge, and return the width it took.
/// The leading icon and, when the row asks for one, the badge behind it. Returns the width taken.
#[expect(
    clippy::too_many_arguments,
    reason = "it is one call site, split out of `show_with_long_press` for its length"
)]
fn paint_leading(
    ui: &egui::Ui,
    cx: &mut Cx<'_>,
    icon: &IconRef,
    left: f32,
    center_y: f32,
    enabled: bool,
    tint: Option<ColorRole>,
    badge: bool,
) -> f32 {
    let size = cx.theme.metrics.icon_size * 0.5;
    if badge {
        ui.painter().circle_filled(
            egui::pos2(left + size / 2.0, center_y),
            size * BADGE_RATIO / 2.0,
            cx.theme
                .color(ColorRole::Primary)
                // `control.badge_alpha`, not a private copy of the same 0.16. The theme's token is
                // named for exactly this — "a tinted badge's alpha behind an icon" — and a second
                // copy is how the old `desktop_badge_pad_x` ended up reading as twice Material's,
                // and that token then sat unread until it was removed as an orphan.
                .gamma_multiply(cx.theme.control.badge_alpha),
        );
    }
    paint_icon(ui, cx, icon, left + size, center_y, enabled, tint)
}

fn paint_icon(
    ui: &egui::Ui,
    cx: &mut Cx<'_>,
    icon: &IconRef,
    right: f32,
    center_y: f32,
    enabled: bool,
    tint: Option<ColorRole>,
) -> f32 {
    let size = cx.theme.metrics.icon_size * 0.5;
    let icon_rect =
        egui::Rect::from_center_size(egui::pos2(right - size / 2.0, center_y), Vec2::splat(size));
    let mut style = crate::icons::IconStyle::sized(size).enabled(enabled);
    if let Some(role) = tint {
        style = style.color(crate::icons::IconColor::Role(role));
    }
    cx.icons
        .paint(ui.painter(), icon_rect, icon, &style, cx.theme);
    size
}

impl ListRow {
    /// The title, and the subtitle under it when there is one.
    fn paint_text(&mut self, ui: &egui::Ui, cx: &Cx<'_>, painter: &egui::Painter, p: &TextPlace) {
        let title = std::mem::take(&mut self.title);
        let subtitle = self.subtitle.take();
        match subtitle {
            Some(sub) => {
                let small = egui::TextStyle::Small.resolve(ui.style());
                // **How far the two lines part follows the lines, not a fixed length.** The token
                // is a `du` value, so at 1.6x text the title and the subtitle crowded into each
                // other while the row around them grew; a quarter of the two lines' own block is
                // what actually keeps them apart. The token stays the floor, so an integrator can
                // still open the pair up - but at the shipped text it is the measurement that wins
                // by a couple of du, and a subtitle sits that much lower than it used to.
                let offset = p
                    .two_line_offset
                    .max((text_floor(ui, cx, true) - cx.theme.control.gap * 2.0) * 0.25);
                draw_left(
                    painter,
                    egui::pos2(p.x, p.centre - offset),
                    title,
                    p.title_font.clone(),
                    p.max_w,
                    p.title_color,
                );
                draw_left(
                    painter,
                    egui::pos2(p.x, p.centre + offset),
                    sub,
                    small,
                    p.max_w,
                    p.muted,
                );
            }
            None => draw_left(
                painter,
                egui::pos2(p.x, p.centre),
                title,
                p.title_font.clone(),
                p.max_w,
                p.title_color,
            ),
        }
    }
}
