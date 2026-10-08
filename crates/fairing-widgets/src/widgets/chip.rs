//! `Chip` and `ChipRow` — an icon over a word, in a lane that scrolls.
//!
//! # Why this is not a `SegmentedControl`
//!
//! They look alike and they are different objects, and the docs have to say which to reach for or
//! callers will guess.
//!
//! A [`SegmentedControl`](super::SegmentedControl) is **one track** holding two to four cells of
//! equal width, capped at `control.segment_max` because past four every cell is narrower than a
//! touch target on the smallest panel. It is a setting with a handful of values — Auto / On / Off.
//!
//! A chip row is **five to eight separate objects** of unequal width that scroll sideways, each
//! with an icon over its label. It is a filter over a list: Starters · Main Dishes · Salads ·
//! Desserts · Drinks. Cramming that into a segmented strip gives eight 6 mm cells; laying a
//! three-value setting out as chips loses the fact that the three belong to one control.
//!
//! The rule: **a setting is segmented, a filter is chips.**
//!
//! # Why a chip is not a pill
//!
//! `metrics.control_radius`, which is 0.207 of the chip's height — the reference row measures
//! 0.112–0.135 and Material 3's chip is 0.25, so this sits between them. A pill (half the height)
//! is reserved for the things a mover rides along, which a chip is not; `card_radius` is for
//! containers, and a chip is content.
//!
//! The side padding is the **same** `control_radius`, so the label clears the corner by exactly one
//! radius. That is the concentric rule the crate adopted for cards, applied inside a control.
//!
//! # Why the minimum width is an aspect and not the content
//!
//! Measured: six of the reference row's eight chips sit at 1.230 of their own height while their
//! labels vary by 2.7×, from "All" to "Salads". A row floored on content alone staggers — one
//! stub, one slab — and a row of fixed cells is a segmented control again.
//! `control.chip_min_aspect` is 1.25, which is Windows 11's own 40 epx target on a 32 px control
//! and 1.6 % off the measurement.
//!
//! # Why an unselected chip is a tinted fill rather than `SurfaceVariant`
//!
//! `SurfaceVariant` would match the references pixel for pixel in base light, where it is white on
//! an `#ECECF1` page. It also measures **1.00** against a card, where `SurfaceVariant` *is* the
//! card — so a chip row inside a card would exist only as its hairline, which is exactly the
//! failure `BigButton::Normal` was rebuilt to fix. An ink tint at `control.fill_alpha` composites
//! over a page, a card and a photograph alike.
//!
//! The cost is named: in a light palette the references' chips are a step *lighter* than the page
//! and these are a step darker.
//!
//! # Why selection is three channels and not one
//!
//! The face goes to `Primary`, the label flips `OnSurface` → `OnPrimary`, and the identifying
//! hairline **leaves** — because on a filled chip the fill is its own boundary. Colour is never
//! alone (Rule 7.1), and a greyscale render still tells a picked chip from its neighbours.
//!
//! Selected against unselected measures 2.97 in base dark, which is quoted rather than hidden: it
//! is not the gating number, because the two chips never touch — a `control.gap` of page runs
//! between them — so each one's contrast is against the ground, where they measure 3.81 and 3.76.

use super::ChipLook;
use crate::cx::WidgetCx as Cx;
use crate::icons::IconRef;
use crate::theme::ColorRole;
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

/// One chip.
///
/// It edits a `&mut bool`, the arrangement [`Checkbox`](super::Checkbox) has: a chip that owned its
/// bit would force the screen to keep a second copy of state it already has.
#[derive(Debug)]
pub struct Chip<'a> {
    label: &'a str,
    selected: &'a mut bool,
    icon: Option<IconRef>,
    height: Option<f32>,
    enabled: bool,
}

/// One entry of a [`ChipRow`].
///
/// Public fields, so `ChipItem { label: "Starters", icon: None }` compiles outside the crate.
#[derive(Debug, Clone)]
pub struct ChipItem<'a> {
    /// The label.
    pub label: &'a str,
    /// The icon above it, or none for a label-only chip.
    pub icon: Option<IconRef>,
}

/// What one [`ChipRow`] reported this frame.
///
/// The [`SegmentedPick`](super::SegmentedPick) shape, so a screen can swap one control for the
/// other without touching its handler.
#[derive(Debug)]
pub struct ChipPick {
    /// The union of the chips' responses.
    pub response: Response,
    /// The index just picked, and only then. `None` for a tap on the chip already selected.
    pub picked: Option<usize>,
}

impl<'a> Chip<'a> {
    /// The label, and the bit it edits.
    #[must_use]
    pub fn new(label: &'a str, selected: &'a mut bool) -> Self {
        Self {
            label,
            selected,
            icon: None,
            height: None,
            enabled: true,
        }
    }

    /// The icon, drawn at `control.icon` **above** the label.
    #[must_use]
    pub fn icon(mut self, icon: IconRef) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Force the height, so a hand-laid row of mixed chips does not stagger.
    ///
    /// [`ChipRow`] sets it for you. Like the other height overrides in this crate it can only push
    /// **up** — a chip shorter than a touch target is not a chip.
    #[must_use]
    pub fn height(mut self, du: f32) -> Self {
        self.height = Some(du.max(0.0));
        self
    }

    /// Enabled. Disabled it senses `hover()`, so the tap falls through.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The size this chip will take, without drawing it.
    #[must_use]
    pub fn measure(&self, ui: &egui::Ui, cx: &Cx<'_>) -> Vec2 {
        let t = Tokens::read(ui, cx);
        let h = self.height.map_or_else(
            || stack_height(&t, self.icon.is_some()),
            |du| du.max(stack_height(&t, self.icon.is_some())),
        );
        let content = self.content_width(ui, cx, &t);
        Vec2::new(chip_width(&t, h, content, f32::INFINITY), h + 2.0 * t.lane)
    }

    /// The widest of the icon and the label.
    fn content_width(&self, ui: &egui::Ui, cx: &Cx<'_>, t: &Tokens) -> f32 {
        let label = ui
            .painter()
            .layout_no_wrap(self.label.to_owned(), t.font.clone(), Color32::PLACEHOLDER)
            .rect
            .width();
        if self.icon.is_some() {
            label.max(cx.theme.control.icon)
        } else {
            label
        }
    }

    /// Draw it. On the frame the value changed, `response.changed()`.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let t = Tokens::read(ui, cx);
        let has_icon = self.icon.is_some();
        let h = self.height.map_or_else(
            || stack_height(&t, has_icon),
            |du| du.max(stack_height(&t, has_icon)),
        );
        let content = self.content_width(ui, cx, &t);
        // **The lane, not what is left of it.** Inside a horizontally scrolling row
        // `available_width()` is the distance from the cursor to the lane's right edge, so a chip
        // straddling that edge was silently narrowed and the one past it collapsed to `target` —
        // and a scrolling row is exactly where those chips are meant to be reached. The cap's job
        // is "no wider than the place it can be seen in", and in a lane that place is the lane:
        // `clip_rect()` is the viewport, the same handle `Dropdown` bounds its list with. It is
        // also what makes the drawn width agree with [`Chip::measure`], which caps at infinity.
        let viewport = ui
            .available_width()
            .max(ui.clip_rect().width())
            .max(t.target);
        let w = chip_width(&t, h, content, viewport);

        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        // The slot carries the ring's lane on both sides, so the sensed rect is larger than the
        // drawing and a pressed chip still has clearance inside its own row (R5).
        let (slot, mut response) = ui.allocate_exact_size(Vec2::new(w, h + 2.0 * t.lane), sense);
        if self.enabled && response.clicked() {
            *self.selected = !*self.selected;
            response.mark_changed();
        }
        let pressed = self.enabled && response.is_pointer_button_down_on();
        let t_sel = cx.animate(
            response.id.with("sel"),
            if *self.selected { 1.0 } else { 0.0 },
            cx.theme.motion.crossfade,
        );
        let t_press = cx.animate(
            response.id.with("press"),
            if pressed { 1.0 } else { 0.0 },
            if pressed {
                cx.theme.motion.press
            } else {
                cx.theme.motion.press_release
            },
        );
        let drawn = Rect::from_center_size(slot.center(), Vec2::new(w, h))
            .expand(cx.theme.control.press_grow * t_press);
        let focused = response.has_focus();
        if let Some(paint) = cx.painters.as_deref_mut().and_then(|p| p.chip.as_mut()) {
            paint(
                ui.painter(),
                &mut ChipLook {
                    rect: slot,
                    drawn,
                    label: self.label,
                    icon: self.icon.as_ref(),
                    selected: *self.selected,
                    mark: t_sel,
                    pressed,
                    press: t_press,
                    enabled: self.enabled,
                    focused,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            return response;
        }
        let ink = Ink::of(cx, t_sel, pressed, self.enabled);
        paint_face(ui.painter(), drawn, &t, t_sel, ink);
        paint_stack(ui, cx, drawn, self.label, self.icon.as_ref(), &t, ink);
        if focused {
            paint_focus_ring(ui.painter(), drawn, &t, ink.ring);
        }
        response
    }
}

/// A horizontally scrolling row of chips with **one** of them selected.
///
/// It owns the index the way [`RadioGroup`](super::RadioGroup) does, so the "exactly one"
/// invariant lives in exactly one place.
#[derive(Debug)]
pub struct ChipRow<'a> {
    id_salt: &'a str,
    items: &'a [ChipItem<'a>],
    selected: &'a mut usize,
    enabled: bool,
}

impl<'a> ChipRow<'a> {
    /// The items, and the index of the chosen one. The salt names the scroll state.
    #[must_use]
    pub fn new(id_salt: &'a str, items: &'a [ChipItem<'a>], selected: &'a mut usize) -> Self {
        Self {
            id_salt,
            items,
            selected,
            enabled: true,
        }
    }

    /// Enabled. The whole row fades once and every chip senses `hover()`.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw it, and write the pick through.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> ChipPick {
        let t = Tokens::read(ui, cx);
        // **One height for the whole row.** A row holding any icon chip takes the taller stack for
        // all of them; a row that staggered would read as two rows.
        let any_icon = self.items.iter().any(|i| i.icon.is_some());
        let height = stack_height(&t, any_icon);
        let inset = cx.theme.metrics.screen_inset;
        let mut picked = None;
        let out = egui::ScrollArea::horizontal()
            // Every source: a pointer drags the lane as a finger does.
            .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
            .id_salt(self.id_salt)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = cx.theme.control.gap;
                    ui.add_space(inset);
                    let mut union: Option<Response> = None;
                    for (index, item) in self.items.iter().enumerate() {
                        let mut on = index == *self.selected;
                        let chip = Chip::new(item.label, &mut on)
                            .height(height)
                            .enabled(self.enabled);
                        let chip = match item.icon.clone() {
                            Some(icon) => chip.icon(icon),
                            None => chip,
                        };
                        let response = chip.show(ui, cx);
                        // A tap on the chip already chosen reports nothing: a single-select row has
                        // no "off", so the bool the chip flipped is discarded here rather than
                        // letting it clear the row's own invariant.
                        if response.changed() && index != *self.selected {
                            *self.selected = index;
                            picked = Some(index);
                        }
                        union = Some(match union {
                            Some(prev) => prev.union(response),
                            None => response,
                        });
                    }
                    ui.add_space(inset);
                    union
                })
                .inner
            });
        paint_lane_fade(
            ui,
            cx,
            out.inner_rect,
            out.content_size.x,
            out.state.offset.x,
        );
        let response = out.inner.unwrap_or_else(|| {
            ui.interact(out.inner_rect, ui.id().with(self.id_salt), Sense::hover())
        });
        ChipPick { response, picked }
    }
}

/// The lengths one frame draws with, read from the theme once.
#[derive(Debug, Clone)]
struct Tokens {
    gap: f32,
    icon: f32,
    /// The side padding and the corner radius, which are the same number on purpose.
    radius: f32,
    /// One line of the label.
    line: f32,
    font: egui::FontId,
    stroke_edge: f32,
    stroke_mark: f32,
    focus_gap: f32,
    /// The ring's reach outside the silhouette.
    lane: f32,
    min_aspect: f32,
    target: f32,
}

impl Tokens {
    fn read(ui: &egui::Ui, cx: &Cx<'_>) -> Self {
        let c = &cx.theme.control;
        // `TextStyle::Button`, the style a segmented control resolves: a chip is a choice and not
        // body copy. The reference's chip label measures the same cap height as its card titles,
        // so `type_scale.small` is ruled out by measurement rather than by taste.
        let font = egui::TextStyle::Button.resolve(ui.style());
        Self {
            gap: c.gap,
            icon: c.icon,
            radius: cx.theme.metrics.control_radius,
            line: ui.text_style_height(&egui::TextStyle::Button),
            font,
            stroke_edge: c.stroke_edge,
            stroke_mark: c.stroke_mark,
            focus_gap: c.focus_gap,
            lane: c.focus_gap + c.stroke_mark,
            min_aspect: c.chip_min_aspect,
            target: crate::theme::control_height(&cx.theme.metrics, &cx.theme.control),
        }
    }
}

/// The stack's height: a gap, the icon, a gap, the line, a gap — floored at a touch target.
fn stack_height(t: &Tokens, has_icon: bool) -> f32 {
    let stack = if has_icon {
        3.0f32.mul_add(t.gap, t.icon + t.line)
    } else {
        2.0f32.mul_add(t.gap, t.line)
    };
    t.target.max(stack)
}

/// **The signal that a lane has more in it**, painted over each end that has something past it.
///
/// A row with the scrollbar hidden and nothing else to say gives a chip cut in half at the edge,
/// which reads as a clipping bug rather than as "there is more". The reference kiosks all ramp
/// their category strips out at the edge instead. The ramp is the page's own colour going to
/// transparent, so it works on any ground without a new token, and it is drawn **only** on the side
/// that has content past it — a lane that fits gets nothing, and a lane scrolled to its end stops
/// advertising the direction it cannot go.
pub fn paint_lane_fade(ui: &egui::Ui, cx: &Cx<'_>, lane: Rect, content: f32, offset: f32) {
    // Half a chip: enough to say "cut off on purpose", not enough to hide one.
    let ramp = cx.theme.metrics.touch_target * 0.5;
    let over = content - lane.width();
    if over <= 1.0 || ramp <= 0.0 {
        return;
    }
    let base = cx.theme.color(ColorRole::Surface);
    let clear = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 0);
    let edge = |x: f32, towards_right: bool| {
        let band = if towards_right {
            Rect::from_min_max(
                egui::pos2(x - ramp, lane.top()),
                egui::pos2(x, lane.bottom()),
            )
        } else {
            Rect::from_min_max(
                egui::pos2(x, lane.top()),
                egui::pos2(x + ramp, lane.bottom()),
            )
        };
        let (near, far) = if towards_right {
            (clear, base)
        } else {
            (base, clear)
        };
        let mut mesh = egui::Mesh::default();
        for (pos, color) in [
            (band.left_top(), near),
            (band.right_top(), far),
            (band.left_bottom(), near),
            (band.right_bottom(), far),
        ] {
            mesh.colored_vertex(pos, color);
        }
        for v in &mut mesh.vertices {
            v.uv = egui::pos2(0.0, 0.0);
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(2, 1, 3);
        ui.painter().add(egui::Shape::mesh(mesh));
    };
    if offset > 1.0 {
        edge(lane.left(), false);
    }
    if offset < over - 1.0 {
        edge(lane.right(), true);
    }
}

/// The chip's width: the aspect floor, or the content plus its padding, capped at the viewport.
fn chip_width(t: &Tokens, height: f32, content: f32, viewport: f32) -> f32 {
    let floor = t.min_aspect * height;
    let wanted = floor.max(2.0f32.mul_add(t.radius, content));
    wanted.min(viewport)
}

/// The four colours one frame paints with, faded **once** at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    face: Color32,
    /// Leaves as the chip fills: on a filled chip the fill is its own boundary.
    edge: Color32,
    label: Color32,
    ring: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, t_sel: f32, pressed: bool, enabled: bool) -> Self {
        let unselected = cx
            .theme
            .color(ColorRole::OnSurface)
            .gamma_multiply(cx.theme.control.fill_alpha);
        let mut face = unselected.lerp_to_gamma(cx.theme.color(ColorRole::Primary), t_sel);
        if pressed {
            // `face.blend(tint)`: `blend`'s receiver is the layer **behind**.
            face = face.blend(cx.theme.color(ColorRole::Pressed));
        }
        let ink = Self {
            face,
            edge: cx
                .theme
                .color(ColorRole::ControlEdge)
                .gamma_multiply(1.0 - t_sel),
            label: cx
                .theme
                .color(ColorRole::OnSurface)
                .lerp_to_gamma(cx.theme.color(ColorRole::OnPrimary), t_sel),
            ring: cx.theme.color(ColorRole::Focus),
        };
        if enabled {
            ink
        } else {
            let a = cx.theme.control.disabled_alpha;
            Self {
                face: ink.face.gamma_multiply(a),
                edge: ink.edge.gamma_multiply(a),
                label: ink.label.gamma_multiply(a),
                ring: ink.ring.gamma_multiply(a),
            }
        }
    }
}

/// The body, edged while it is unselected.
fn paint_face(painter: &egui::Painter, rect: Rect, t: &Tokens, t_sel: f32, ink: Ink) {
    let radius = CornerRadius::same(round_u8(t.radius));
    painter.rect_filled(rect, radius, ink.face);
    if t_sel < 1.0 {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(t.stroke_edge, ink.edge),
            // Inside, so a chip never measures wider than its own width and the row's rhythm holds.
            StrokeKind::Inside,
        );
    }
}

/// The icon over the label, centred as one block.
fn paint_stack(
    ui: &egui::Ui,
    cx: &mut Cx<'_>,
    rect: Rect,
    label: &str,
    icon: Option<&IconRef>,
    t: &Tokens,
    ink: Ink,
) {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), t.font.clone(), ink.label);
    let block = icon.map_or(galley.rect.height(), |_| {
        t.icon + t.gap + galley.rect.height()
    });
    let top = rect.center().y - block * 0.5;
    if let Some(icon) = icon {
        let at = Rect::from_center_size(
            egui::pos2(rect.center().x, top + t.icon * 0.5),
            Vec2::splat(t.icon),
        );
        let style = crate::icons::IconStyle {
            color: crate::icons::IconColor::Fixed(ink.label),
            ..crate::icons::IconStyle::default()
        };
        cx.icons.paint(ui.painter(), at, icon, &style, cx.theme);
    }
    let text_top = if icon.is_some() {
        top + t.icon + t.gap
    } else {
        top
    };
    let at = egui::pos2(rect.center().x - galley.rect.width() * 0.5, text_top);
    ui.painter().galley(at, galley, ink.label);
}

/// The focus ring, outside the silhouette. A rect, so it is stroked down the middle and the lane
/// carries a half-stroke term — unlike a circle, which epaint strokes outside.
fn paint_focus_ring(painter: &egui::Painter, rect: Rect, t: &Tokens, color: Color32) {
    let out = t.focus_gap + t.stroke_mark * 0.5;
    painter.rect_stroke(
        rect.expand(out),
        CornerRadius::same(round_u8(t.radius + out)),
        Stroke::new(t.stroke_mark, color),
        StrokeKind::Middle,
    );
}

#[cfg(test)]
mod tests {
    use crate::theme::{contrast, ControlSpec, Palette, Preset};

    /// **A chip's minimum width is an aspect**, so a one-word category is not a stub beside a long
    /// one. The reference row's six floored chips sit at 1.230 while their labels vary by 2.7×.
    #[test]
    fn a_short_label_and_a_long_one_floor_at_the_same_width() {
        let c = ControlSpec::default().resolve(&crate::unit::Scale::identity());
        let (height, radius) = (68.0_f32, 14.0_f32);
        let floor = c.chip_min_aspect * height;
        // "All" — 12 du of label — cannot get narrower than the floor.
        let short = floor.max(2.0f32.mul_add(radius, 12.0));
        assert!((short - floor).abs() < 1e-3, "a short chip left the floor");
        // "Main Dishes" — 86 du — does, and only then.
        let long = floor.max(2.0f32.mul_add(radius, 86.0));
        assert!(long > floor, "a long chip did not grow past the floor");
    }

    /// **Selection is three channels, and none of them alone is hue.** The face fills, the label
    /// flips its role, and the identifying hairline leaves.
    #[test]
    fn a_selected_chip_and_an_unselected_one_differ_by_more_than_colour() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                let (name, mode) = (preset.as_str(), if dark { "dark" } else { "light" });
                // The selected fill identifies itself against the ground.
                let filled = contrast(p.primary, p.surface);
                assert!(
                    filled >= 3.0,
                    "{name} {mode}: a picked chip measures {filled:.2}"
                );
                // The unselected one is identified by its hairline, not by its face.
                let edge = contrast(p.control_edge, p.surface);
                assert!(
                    edge >= 3.0,
                    "{name} {mode}: an unpicked chip measures {edge:.2}"
                );
                // And each label reads on its own face.
                let on_filled = contrast(p.on_primary, p.primary);
                assert!(on_filled >= 4.5, "{name} {mode}: {on_filled:.2}");
            }
        }
    }

    /// The face the references would use is the one that disappears inside a card, which is why
    /// this element tints the ink instead.
    #[test]
    fn an_opaque_surface_variant_face_would_vanish_on_a_card() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                let got = contrast(p.surface_variant, p.surface_variant);
                assert!(
                    (got - 1.0).abs() < 1e-3,
                    "{} {}: a `SurfaceVariant` chip on a `SurfaceVariant` card measures {got:.2}",
                    preset.as_str(),
                    if dark { "dark" } else { "light" }
                );
            }
        }
    }
}
