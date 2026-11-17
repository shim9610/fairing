//! `FeatureCard` displays a title, supporting text and optional artwork or action.
//!
//! Artwork uses `IconRef` and does not determine the card's height. The action is
//! painted as a disc; layout and colour come from theme and component tokens.
//! The application supplies the content and handles the resulting interaction.

use super::button::ButtonKind;
use super::icon_button::IconButton;
use super::FeatureCardLook;
use crate::cx::WidgetCx as Cx;
use crate::icons::IconRef;
use crate::theme::ColorRole;
use egui::{Rect, Sense, Vec2};

/// What a [`FeatureCard`] gives back.
pub struct FeatureCardPick {
    /// The card's own response — clicking anywhere on it counts.
    pub response: egui::Response,
    /// Whether the action disc itself was pressed.
    pub action: bool,
}

impl FeatureCardPick {
    /// **Whether the feature was opened**, by the card or by its disc.
    #[must_use]
    pub fn opened(&self) -> bool {
        self.action || self.response.clicked()
    }
}

/// A card led by its title, with a drawing beside it and one action — see the module docs.
pub struct FeatureCard<'a> {
    title: &'a str,
    body: Option<&'a str>,
    art: Option<IconRef>,
    action: Option<(IconRef, &'a str)>,
    rows: f32,
    outlined: bool,
    lit: bool,
}

/// How tall the card is, in rows, when the caller does not say.
const DEFAULT_ROWS: f32 = 3.2;
/// The art's share of the card's width.
///
/// Measured off the reference: its drawing occupies a little over two fifths of the card, which is
/// also what keeps the body text to the two short lines the card was written for.
const ART_SHARE: f32 = 0.42;

impl<'a> FeatureCard<'a> {
    /// A card with this title and nothing else yet.
    #[must_use]
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            body: None,
            art: None,
            action: None,
            rows: DEFAULT_ROWS,
            outlined: false,
            lit: false,
        }
    }

    /// The line under the title saying what the feature does.
    #[must_use]
    pub fn body(mut self, body: &'a str) -> Self {
        self.body = Some(body);
        self
    }

    /// The drawing beside the words — see the module docs on why it is an [`IconRef`].
    #[must_use]
    pub fn art(mut self, art: IconRef) -> Self {
        self.art = Some(art);
        self
    }

    /// The action disc and its name. Without one the card is still tappable, just unmarked.
    #[must_use]
    pub fn action(mut self, icon: IconRef, name: &'a str) -> Self {
        self.action = Some((icon, name));
        self
    }

    /// Highlight this card using the accent at `fill_alpha`, with a primary action disc.
    #[must_use]
    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }

    /// **Draw a hairline edge instead of relying on the fill** — the outlined container's idea,
    /// for a card that sits on a light panel.
    ///
    /// A `FeatureCard` is filled with `SurfaceVariant`, which reads as a raised block against a
    /// `Surface` page. On a page that is *itself* `SurfaceVariant` - which is what
    /// `layout::rail_split` puts inside its elbow - the fill and the panel are the same colour and
    /// the card has no edge at all. The outline is what gives it one back without making the page
    /// darker to suit it.
    #[must_use]
    pub fn outlined(mut self, outlined: bool) -> Self {
        self.outlined = outlined;
        self
    }

    /// The card's height in rows. The default is 3.2 — two lines of body and the disc under them.
    #[must_use]
    pub fn rows(mut self, rows: f32) -> Self {
        self.rows = rows.max(1.0);
        self
    }

    /// Draw at the current cursor, taking the available width.
    /// Title, supporting text and action are measured before determining the height;
    /// [`rows`](Self::rows) provides a minimum height.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> FeatureCardPick {
        let m = &cx.theme.metrics;
        // **`content_inset`, on both axes.** That token is the crate's answer to this exact
        // question — "where a row's, a card's or a bar's content starts, measured in from its
        // edge" — and it exists because five different paddings once disagreed by four du.
        //
        // Not `card_pad`: that is the small extra a *stack of rows* needs so the first and last do
        // not sit in the corner's curve, and each of those rows already carries its own height.
        // This card draws its title flush at the top with no row box around it, so at `card_pad`
        // the words land on the frame.
        let pad = m.content_inset;
        let gap = cx.theme.control.line_gap;
        // **Ask the button, do not guess at it.** This was `m.touch_target`, and an `IconButton`
        // is `max(control_height, icon_button + 2 * (press_grow + focus_gap + stroke_mark))` - a
        // number anchored to the eye where `touch_target` is anchored to the hand, so the
        // two are free to disagree. When they did, the card reserved the smaller of them and the
        // button, which floors itself and does not shrink to fit, drew its disc hanging out of the
        // card's bottom edge. `IconButton::measure` is the number the button will use.
        let disc = IconButton::measure(cx);
        let width = ui.available_width();

        let title_font = cx.theme.display(m.type_scale.heading);
        let title_h = ui.ctx().fonts_mut(|f| f.row_height(&title_font));
        let body_font = egui::FontId::proportional(m.type_scale.small);
        // The art takes its share off the right before the words are measured, so a long line
        // wraps inside its own column instead of running under the drawing.
        let art_w = self.art.as_ref().map_or(0.0, |_| width * ART_SHARE);
        let text_w = (width - pad * 2.0 - art_w).max(1.0);
        let body = self.body.map(|body| {
            ui.ctx().fonts_mut(|f| {
                f.layout(
                    body.to_owned(),
                    body_font,
                    cx.theme.color(ColorRole::Muted),
                    text_w,
                )
            })
        });
        let body_h = body.as_ref().map_or(0.0, |g| gap + g.rect.height());
        let disc_h = self.action.as_ref().map_or(0.0, |_| gap * 2.0 + disc);
        let height = (pad * 2.0 + title_h + body_h + disc_h).max(m.row_height * self.rows);

        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
        // Square, and never taller than the card's inner height — a drawing that touches the
        // frame reads as a crop rather than as a mark.
        let art_at = self.art.as_ref().map(|art| {
            let side = art_w.min(height - pad * 2.0);
            let at = Rect::from_center_size(
                egui::pos2(rect.right() - pad - art_w * 0.5, rect.center().y),
                Vec2::splat(side),
            );
            (art, at)
        });
        // Pinned to the foot, so a row of cards has its discs on one line whatever their text.
        let action_at = self.action.as_ref().map(|_| {
            Rect::from_min_size(
                egui::pos2(rect.left() + pad, rect.bottom() - pad - disc),
                Vec2::splat(disc),
            )
        });
        let title_at = egui::pos2(rect.left() + pad, rect.top() + pad);
        if let Some(custom) = cx
            .painters
            .as_deref_mut()
            .and_then(|p| p.feature_card.as_mut())
        {
            custom(
                ui.painter(),
                &mut FeatureCardLook {
                    rect,
                    text: Rect::from_min_size(title_at, Vec2::new(text_w, title_h + body_h)),
                    title: self.title,
                    body: self.body,
                    art: art_at,
                    action: action_at,
                    lit: self.lit,
                    outlined: self.outlined,
                    pressed: response.is_pointer_button_down_on(),
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
        } else {
            let body_at = egui::pos2(title_at.x, title_at.y + title_h + gap);
            self.paint_card(
                ui,
                cx,
                rect,
                art_at,
                (title_at, title_font),
                body.map(|galley| (galley, body_at)),
            );
        }

        let action = if let (Some((icon, name)), Some(at)) = (self.action, action_at) {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(at));
            IconButton::new(icon, name)
                .kind(if self.lit {
                    ButtonKind::Primary
                } else {
                    ButtonKind::Normal
                })
                .show(&mut child, cx)
                .response
                .clicked()
        } else {
            false
        };

        FeatureCardPick { response, action }
    }

    /// The built-in card under its action disc: the ground, the edge where it was asked for, the
    /// art in its square, the title and the body where they go.
    fn paint_card(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        rect: Rect,
        art: Option<(&IconRef, Rect)>,
        (title_at, title_font): (egui::Pos2, egui::FontId),
        body: Option<(std::sync::Arc<egui::Galley>, egui::Pos2)>,
    ) {
        let theme = cx.theme;
        let radius = egui::CornerRadius::same(round_u8(crate::theme::card_radius(
            &theme.metrics,
            &theme.control,
        )));
        ui.painter().rect_filled(
            rect,
            radius,
            if self.lit {
                theme
                    .color(ColorRole::Primary)
                    .gamma_multiply(theme.control.fill_alpha)
            } else {
                theme.color(ColorRole::SurfaceVariant)
            },
        );
        if self.outlined {
            ui.painter().rect_stroke(
                rect,
                radius,
                egui::Stroke::new(
                    theme.control.stroke_hairline,
                    theme.color(ColorRole::Outline),
                ),
                egui::StrokeKind::Inside,
            );
        }
        if let Some((art, at)) = art {
            // Sized to the art's square: a card's art is several times a list icon, and a
            // `default()` style would draw it with a list icon's stroke.
            let style = crate::icons::IconStyle::sized(at.width().min(at.height()))
                .color(crate::icons::IconColor::Role(ColorRole::Primary));
            cx.icons.paint(ui.painter(), at, art, &style, theme);
        }
        ui.painter().text(
            title_at,
            egui::Align2::LEFT_TOP,
            self.title,
            title_font,
            theme.color(ColorRole::OnSurface),
        );
        if let Some((galley, at)) = body {
            ui.painter().galley(at, galley, egui::Color32::BLACK);
        }
    }
}

/// `f32` → `u8`, saturating. A corner radius is small and positive.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a corner radius is a small positive number"
)]
fn round_u8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}
