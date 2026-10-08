//! `MediaCard` — a picture with a text block under it, and the boundary argument that let it in.
//!
//! # Why this is not "a product card"
//!
//! `layout`'s refusal list says a product card "looks different in every trade and the crate cannot
//! guess". Measured against the seven screens this element set was designed from, that is not what
//! the screens do. **Six of them draw one structure across four trades** — a European restaurant, a
//! Chinese restaurant, a fast-food chain and an e-book reader — and the structure is: a fixed-aspect
//! picture, a left-aligned title, a muted line or two under it, and a figure.
//!
//! What *is* trade-specific is real and stays out: the price format, the spice pips, the allergen
//! marks, the star rating. Those are the caller's, and this takes them as strings and slots it has
//! no opinion about. The test for what the crate builds was never "does a shop use it" but **"is it
//! the same object in every trade"**, and the measurement says this one is.
//!
//! # Why the corners are asymmetric
//!
//! The picture is full-bleed: it runs to the card's own edge on three sides. So its outer corners
//! must be the **card's** radius and its inner ones must be square — a rounded corner where the
//! picture meets the text block would cut a notch out of the middle of the card.
//!
//! That is the concentric rule the crate adopted for cards (`inner = outer − gap`) at its
//! degenerate case: the gap is zero, so the radii are equal.
//!
//! # Why the image box's aspect is a number and the default is 4:3
//!
//! Measured: 1.314 (Bella Tavola), 1.310 (the Sichuan kiosk), 1.032 (the `McDonald's` kiosk) — two
//! of three at 4:3 and one square, with the e-reader's covers portrait at about 2:3. A trade does
//! not pick that; a *photographer* does, and the caller knows which they have. So it is one `f32`
//! with named constants beside it rather than an enum the crate would have to keep guessing at.
//!
//! # Why selection is a ring and not a fill
//!
//! **Not one of the seven references fills a card to say it is chosen.** The crate's own kiosk did,
//! and it is the single thing that made it read as a vending machine rather than a menu: an accent
//! that floods a photograph destroys the photograph, which was the reason to have a card at all.
//!
//! Selection here is a `Primary` ring inside the card's edge, and the count lives on the corner
//! action's badge, which is what every reference does. The rule is the one task #5 settled for the
//! quick tiles, applied where it matters most.
//!
//! # Why the corner action is a slot and not a button the card owns
//!
//! A card reports two different presses — "open this" and "add one of these" — and a widget that
//! collapsed them would make the commonest kiosk interaction impossible. [`MediaPick`] carries both
//! and the caller decides; the action itself is an [`IconButton`], so a `+` on a
//! card and a `+` anywhere else are the same object.

use crate::cx::WidgetCx as Cx;
use crate::fit::cover_uv;
use crate::icons::IconRef;
use crate::theme::{card_radius, ColorRole};
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Rect, Response, Sense, Vec2};

use super::{ButtonKind, IconButton, MediaCardLook};

/// A 4:3 picture — what two of the three reference kiosks shoot.
pub const ASPECT_PHOTO: f32 = 4.0 / 3.0;
/// A square picture.
pub const ASPECT_SQUARE: f32 = 1.0;
/// A 16:9 picture, for a banner.
pub const ASPECT_WIDE: f32 = 16.0 / 9.0;
/// A 2:3 picture, which is a book cover.
pub const ASPECT_COVER: f32 = 2.0 / 3.0;

/// Where the picture sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaShape {
    /// Above the text — a grid tile. What three of the references draw.
    #[default]
    Tile,
    /// Before the text — a list row. What the fourth draws.
    Row,
}

/// What one [`MediaCard`] reported this frame.
#[derive(Debug)]
pub struct MediaPick {
    /// The card itself.
    pub response: Response,
    /// The corner action was pressed on this frame.
    pub action: bool,
}

/// A picture with a text block.
#[derive(Debug)]
pub struct MediaCard<'a> {
    title: &'a str,
    subtitle: Option<&'a str>,
    value: Option<&'a str>,
    image: Option<(egui::TextureId, Vec2)>,
    aspect: f32,
    shape: MediaShape,
    action: Option<(IconRef, &'a str)>,
    /// A scrim and a word over the picture — "Sold out".
    veil: Option<&'a str>,
    selected: bool,
    enabled: bool,
}

impl<'a> MediaCard<'a> {
    /// A card titled `title`.
    #[must_use]
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            subtitle: None,
            value: None,
            image: None,
            aspect: ASPECT_PHOTO,
            shape: MediaShape::default(),
            action: None,
            veil: None,
            selected: false,
            enabled: true,
        }
    }

    /// The muted line under the title. Wrapped to two lines and elided past them.
    #[must_use]
    pub fn subtitle(mut self, subtitle: &'a str) -> Self {
        self.subtitle = Some(subtitle);
        self
    }

    /// The figure — a price, a weight, a cycle time. **Already formatted**: the crate does not know
    /// the caller's currency, separator or digit shaping, and what goes inside stays the caller's.
    #[must_use]
    pub fn value(mut self, value: &'a str) -> Self {
        self.value = Some(value);
        self
    }

    /// The picture, and its own pixel size — which is what [`cover_uv`] needs to crop it without
    /// squashing it. `TextureHandle::size_vec2()` is the value.
    #[must_use]
    pub fn image(mut self, texture: egui::TextureId, source: Vec2) -> Self {
        self.image = Some((texture, source));
        self
    }

    /// The picture's aspect, width over height. [`ASPECT_PHOTO`] by default.
    #[must_use]
    pub fn aspect(mut self, aspect: f32) -> Self {
        if aspect.is_finite() && aspect > 0.0 {
            self.aspect = aspect;
        }
        self
    }

    /// Picture above the text, or before it.
    #[must_use]
    pub fn shape(mut self, shape: MediaShape) -> Self {
        self.shape = shape;
        self
    }

    /// A corner action, with the name of what it does — see [`IconButton`].
    #[must_use]
    pub fn action(mut self, icon: IconRef, name: &'a str) -> Self {
        self.action = Some((icon, name));
        self
    }

    /// A scrim and a word over the picture, for an item that cannot be had.
    ///
    /// It is not [`Self::enabled`]: an unavailable *item* still opens, so you can read why. A
    /// disabled *card* is one the operator has no rights to.
    #[must_use]
    pub fn veil(mut self, text: &'a str) -> Self {
        self.veil = Some(text);
        self
    }

    /// Chosen — drawn as a ring, never as a fill. See the module doc.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Enabled.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// **How tall this card has to be at `width` to keep its aspect** — what a grid should give it.
    ///
    /// A `Grid` picks a cell height from a row count and only then discovers how wide the cell came
    /// out, so a caller has no way to reason about the two together. This closes that: ask, then
    /// size the cell. Measured on the kiosk's menu grid before it did, a tile asking for a square
    /// picture was handed 282.7 du where it needed 636.4, and the shortfall was paid for out of the
    /// photograph.
    ///
    /// ```no_run
    /// # fn f(ui: &mut egui::Ui, cx: &mut fairing_widgets::WidgetCx<'_>) {
    /// use fairing_widgets::widgets::{MediaCard, ASPECT_SQUARE};
    /// let card = MediaCard::new("Cappuccino").value("₩5,000").aspect(ASPECT_SQUARE);
    /// let rows = card.height_for(240.0, ui, cx) / cx.theme.metrics.row_height;
    /// # let _ = rows;
    /// # }
    /// ```
    #[must_use]
    pub fn height_for(&self, width: f32, ui: &egui::Ui, cx: &Cx<'_>) -> f32 {
        match self.shape {
            MediaShape::Tile => width / self.aspect + self.text_height(ui, cx),
            MediaShape::Row => self.text_height(ui, cx).max(cx.theme.metrics.touch_target),
        }
    }

    /// Draw it, filling the width available.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> MediaPick {
        let m = &cx.theme.metrics;
        let pad = m.card_pad;
        let gap = cx.theme.control.gap;
        let width = ui.available_width().max(m.touch_target);
        let text_h = self.text_height(ui, cx);
        // The picture takes its aspect **unless the place is shorter**, in which case it takes what
        // is left. A card in a grid cell has its height decided by the grid, and a picture that
        // insisted on its aspect there would run over the cell below it — the grid does not clip.
        // **The aspect the caller asked for is kept, whatever room it is given.** It used to be the
        // *height* that was capped and the width left at the card's own, so a place too short for
        // the ratio silently widened the box instead of shrinking it — and `cover_uv` then cropped
        // the subject to fill it. Measured on the kiosk's menu grid: a tile that asked for 1.000
        // was drawn at **3.435**, throwing away 71 % of every photograph, in a card whose caller
        // had written "square, because a 4:3 box would crop the top off every cup". The caller's
        // reason for the parameter was exactly what the widget was defeating.
        //
        // Capping the width instead keeps the ratio and leaves the card's colour at the sides — so
        // a cell too short to hold the picture **looks** too short, and the caller can see it and
        // ask for more ([`Self::height_for`] says how much).
        let room = (ui.available_height() - text_h).max(m.touch_target);
        let picture = Vec2::new(width, width / self.aspect);
        let picture = if picture.y > room {
            Vec2::new((room * self.aspect).min(width), room)
        } else {
            picture
        };

        let size = match self.shape {
            MediaShape::Tile => Vec2::new(width, picture.y + text_h),
            // A row's picture is as tall as the text block beside it, never taller.
            MediaShape::Row => Vec2::new(width, text_h.max(m.touch_target)),
        };
        let (rect, response) = ui.allocate_exact_size(
            size,
            if self.enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let radius = card_radius(m, &cx.theme.control);
        let ink = Ink::of(cx, self.enabled);
        let (image_rect, text_rect) = self.split(rect, picture, gap);
        let action_at = self.action.as_ref().map(|_| {
            let button = IconButton::measure(cx);
            Rect::from_min_size(
                egui::pos2(rect.max.x - pad - button, rect.max.y - pad - button),
                Vec2::splat(button),
            )
        });
        let custom = cx
            .painters
            .as_deref_mut()
            .and_then(|p| p.media_card.as_mut());
        let painted = custom.is_some();
        if let Some(custom) = custom {
            custom(
                ui.painter(),
                &mut MediaCardLook {
                    rect,
                    picture: image_rect,
                    text: text_rect.shrink(pad),
                    image: self.image,
                    shape: self.shape,
                    title: self.title,
                    subtitle: self.subtitle,
                    value: self.value,
                    veil: self.veil,
                    action: action_at,
                    selected: self.selected,
                    pressed: self.enabled && response.is_pointer_button_down_on(),
                    enabled: self.enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
        } else {
            self.paint_card(ui, cx, rect, (image_rect, text_rect.shrink(pad)), ink);
        }

        let mut action = false;
        if let (Some((icon, name)), Some(at)) = (self.action.clone(), action_at) {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(at));
            action = IconButton::new(icon, name)
                .kind(ButtonKind::Primary)
                .enabled(self.enabled)
                .show(&mut child, cx)
                .clicked();
        }
        if self.selected && !painted {
            // A ring **inside** the edge, so a chosen card is exactly as wide as an unchosen one
            // and a grid does not shuffle. Never a fill — see the module doc.
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(round_u8(radius)),
                egui::Stroke::new(cx.theme.control.stroke_mark, ink.ring),
                egui::StrokeKind::Inside,
            );
        }
        MediaPick { response, action }
    }

    /// The built-in card under its action: the shadow and the ground, the picture in `picture`
    /// and the words in `text`.
    fn paint_card(
        &self,
        ui: &egui::Ui,
        cx: &Cx<'_>,
        rect: Rect,
        (picture, text): (Rect, Rect),
        ink: Ink,
    ) {
        let radius = card_radius(&cx.theme.metrics, &cx.theme.control);
        crate::theme::paint_elevation(
            ui.painter(),
            cx.theme,
            rect,
            CornerRadius::same(round_u8(radius)),
            cx.theme.card_elevation(),
        );
        ui.painter()
            .rect_filled(rect, CornerRadius::same(round_u8(radius)), ink.card);
        let bleeds = picture.width() >= rect.width();
        self.paint_image(ui, cx, picture, radius, ink, bleeds);
        self.paint_text(ui, cx, text, ink);
    }

    /// The text block's height: a title, up to two muted lines, and a figure.
    fn text_height(&self, ui: &egui::Ui, cx: &Cx<'_>) -> f32 {
        let m = &cx.theme.metrics;
        let line = ui.text_style_height(&egui::TextStyle::Body);
        let small = ui.text_style_height(&egui::TextStyle::Small);
        let sub = if self.subtitle.is_some() {
            small * 2.0 + cx.theme.control.line_gap
        } else {
            0.0
        };
        let value = if self.value.is_some() {
            line + cx.theme.control.line_gap
        } else {
            0.0
        };
        line + sub + value + m.card_pad * 2.0
    }

    /// The picture's rect and the text's, for this shape.
    ///
    /// A Tile's picture is centred: normally it is exactly the card's width and the centring is a
    /// no-op, and where the place was too short to hold the aspect it is narrower and sits in the
    /// middle rather than against one edge.
    fn split(&self, rect: Rect, picture: Vec2, gap: f32) -> (Rect, Rect) {
        match self.shape {
            MediaShape::Tile => (
                Rect::from_min_size(
                    egui::pos2(rect.center().x - picture.x * 0.5, rect.min.y),
                    picture,
                ),
                Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y + picture.y), rect.max),
            ),
            MediaShape::Row => {
                let side = rect.height() * self.aspect;
                (
                    Rect::from_min_size(rect.min, Vec2::new(side, rect.height())),
                    Rect::from_min_max(egui::pos2(rect.min.x + side + gap, rect.min.y), rect.max),
                )
            }
        }
    }

    /// The picture, with only its outer corners rounded where it runs to the card's own edge.
    ///
    /// `bleeds` is false when the place was too short for the aspect and the box came out narrower
    /// than the card: then it is not against the side edges at all, so all four of its corners are
    /// its own and rounding two of them would cut a notch out of the middle of the card.
    fn paint_image(
        &self,
        ui: &egui::Ui,
        cx: &Cx<'_>,
        at: Rect,
        radius: f32,
        ink: Ink,
        bleeds: bool,
    ) {
        if at.width() <= 0.0 || at.height() <= 0.0 {
            return;
        }
        let r = round_u8(radius);
        let corner = match self.shape {
            MediaShape::Tile if !bleeds => CornerRadius::same(r),
            MediaShape::Tile => CornerRadius {
                nw: r,
                ne: r,
                sw: 0,
                se: 0,
            },
            MediaShape::Row => CornerRadius {
                nw: r,
                ne: 0,
                sw: r,
                se: 0,
            },
        };
        match self.image {
            // A card with no picture yet still holds its place, so a grid does not reflow as
            // photographs arrive over the network.
            None => {
                ui.painter().rect_filled(at, corner, ink.well);
            }
            Some((texture, source)) => {
                // A textured `RectShape` and not `Painter::image`, which draws square corners: the
                // picture is full-bleed, so its outer corners have to be the card's.
                let mut shape = egui::epaint::RectShape::filled(at, corner, ink.tint);
                shape.brush = Some(std::sync::Arc::new(egui::epaint::Brush {
                    fill_texture_id: texture,
                    uv: cover_uv(source, at),
                }));
                ui.painter().add(shape);
            }
        }
        if let Some(word) = self.veil {
            ui.painter().rect_filled(at, corner, ink.scrim);
            ui.painter().text(
                at.center(),
                egui::Align2::CENTER_CENTER,
                word,
                egui::FontId::proportional(cx.theme.metrics.type_scale.body),
                ink.veil,
            );
        }
    }

    /// The title, the muted lines and the figure, all left-aligned.
    fn paint_text(&self, ui: &egui::Ui, cx: &Cx<'_>, at: Rect, ink: Ink) {
        let m = &cx.theme.metrics;
        let painter = ui.painter();
        let mut y = at.min.y;
        let body = egui::FontId::proportional(m.type_scale.body);
        let small = egui::FontId::proportional(m.type_scale.small);
        let title = painter.layout(self.title.to_owned(), body.clone(), ink.title, at.width());
        painter.galley(egui::pos2(at.min.x, y), title.clone(), ink.title);
        y += title.rect.height();
        if let Some(text) = self.subtitle {
            y += cx.theme.control.line_gap;
            let galley = painter.layout(text.to_owned(), small, ink.muted, at.width());
            painter.galley(egui::pos2(at.min.x, y), galley.clone(), ink.muted);
            y += galley.rect.height();
        }
        if let Some(value) = self.value {
            y += cx.theme.control.line_gap;
            painter.text(
                egui::pos2(at.min.x, y),
                egui::Align2::LEFT_TOP,
                value,
                cx.theme.strong(m.type_scale.body),
                ink.title,
            );
        }
    }
}

/// The colours one frame paints with, faded **once** at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    card: Color32,
    /// Where a picture has not arrived.
    well: Color32,
    /// The picture's own tint, which is white unless the card is disabled.
    tint: Color32,
    title: Color32,
    muted: Color32,
    ring: Color32,
    scrim: Color32,
    veil: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, enabled: bool) -> Self {
        let ink = Self {
            card: cx.theme.color(ColorRole::SurfaceVariant),
            well: cx
                .theme
                .color(ColorRole::OnSurface)
                .gamma_multiply(cx.theme.control.fill_alpha),
            tint: Color32::WHITE,
            title: cx.theme.color(ColorRole::OnSurface),
            muted: cx.theme.color(ColorRole::Muted),
            ring: cx.theme.color(ColorRole::Primary),
            scrim: cx.theme.color(ColorRole::Scrim),
            // On the scrim, not on the picture: the scrim is what the word is actually read
            // against, and it is dark in every palette.
            veil: Color32::WHITE,
        };
        if enabled {
            ink
        } else {
            let a = cx.theme.control.disabled_alpha;
            Self {
                card: ink.card,
                well: ink.well.gamma_multiply(a),
                tint: ink.tint.gamma_multiply(a),
                title: ink.title.gamma_multiply(a),
                muted: ink.muted.gamma_multiply(a),
                ring: ink.ring.gamma_multiply(a),
                scrim: ink.scrim,
                veil: ink.veil.gamma_multiply(a),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaCard, MediaShape, ASPECT_COVER, ASPECT_PHOTO, ASPECT_SQUARE};
    use egui::{pos2, vec2, Rect};

    /// **The picture is full-bleed and the text sits under it**, with no gap between them — a card
    /// whose photograph stopped short of its own edge would read as a picture in a frame.
    #[test]
    fn a_tiles_picture_runs_to_the_cards_edge_and_the_text_starts_where_it_ends() {
        let card = MediaCard::new("Margherita");
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 350.0));
        let (image, text) = card.split(rect, vec2(rect.width(), 150.0), 8.0);
        assert!(
            (image.width() - rect.width()).abs() < 1e-3,
            "the picture was inset"
        );
        assert!((image.min.y - rect.min.y).abs() < 1e-3);
        assert!(
            (text.min.y - image.max.y).abs() < 1e-3,
            "a gap opened between them"
        );
        assert!((text.max.y - rect.max.y).abs() < 1e-3);
    }

    /// A row's picture is as tall as the card and as wide as its aspect asks, and the text takes
    /// what is left with one gap between.
    #[test]
    fn a_rows_picture_leads_and_the_text_takes_what_is_left() {
        let card = MediaCard::new("Grilled Ribeye").shape(MediaShape::Row);
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 100.0));
        let (image, text) = card.split(rect, vec2(0.0, 0.0), 8.0);
        assert!((image.height() - rect.height()).abs() < 1e-3);
        assert!((image.width() - rect.height() * ASPECT_PHOTO).abs() < 1e-3);
        assert!(text.min.x > image.max.x, "the text overlapped the picture");
        assert!((text.max.x - rect.max.x).abs() < 1e-3);
    }

    /// **A place too short for the aspect narrows the box; it never widens it.** This is the
    /// defect the `room` cap used to produce: the height was capped and the width left at the
    /// card's own, so a tile asking for 1:1 in a short cell was drawn at 3.435:1 and `cover_uv`
    /// cropped 71 % off the subject to fill it.
    #[test]
    fn a_short_place_keeps_the_aspect_and_centres_the_picture() {
        let card = MediaCard::new("Cappuccino").aspect(ASPECT_SQUARE);
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(500.0, 290.0));
        // What `show` computes when `room` (145) is shorter than the aspect wants (500).
        let picture = vec2(145.0 * ASPECT_SQUARE, 145.0);
        let (image, _) = card.split(rect, picture, 8.0);
        assert!(
            (image.width() / image.height() - ASPECT_SQUARE).abs() < 1e-3,
            "the box did not keep the caller's aspect: {image:?}"
        );
        assert!(
            image.width() < rect.width(),
            "the box was not narrowed: {image:?}"
        );
        assert!(
            (image.center().x - rect.center().x).abs() < 1e-3,
            "the narrowed box was not centred: {image:?}"
        );
    }

    /// The aspect refuses what would divide by zero or invert the box, rather than trusting it.
    #[test]
    fn the_aspect_refuses_a_value_that_would_collapse_the_picture() {
        assert!((MediaCard::new("x").aspect(0.0).aspect - ASPECT_PHOTO).abs() < 1e-6);
        assert!((MediaCard::new("x").aspect(-2.0).aspect - ASPECT_PHOTO).abs() < 1e-6);
        assert!((MediaCard::new("x").aspect(f32::NAN).aspect - ASPECT_PHOTO).abs() < 1e-6);
        assert!((MediaCard::new("x").aspect(ASPECT_SQUARE).aspect - 1.0).abs() < 1e-6);
        assert!((MediaCard::new("x").aspect(ASPECT_COVER).aspect - 2.0 / 3.0).abs() < 1e-6);
    }
}
