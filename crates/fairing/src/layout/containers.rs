//! Containers.

use super::{
    round_i8, screen, Cell, ColorRole, Container, Cx, Deco, Elevation, Grid, Rail, ScreenDecl, Ui,
    BAND_MIN_ROWS, BAND_TOP_MAX, BAND_TOP_MIN, CARD_GAP, HERO_SCRIM, LIST_FRACTION, LIST_MAX,
    NOTE_GAP, NOTE_MEASURE, RAIL_FRACTION, RAIL_MAX, RAIL_MIN, SECTION_GAP, SECTION_TRACKING,
    SPLIT_MIN_ASPECT, SPLIT_MIN_WIDTH,
};

/// Whether to use a left/right split, and if so the left column's width.
///
/// **It looks at both the width and the aspect ratio.** Unfolding a foldable, or sitting on a wide
/// panel, makes one entry per list row a waste, but a tall screen split into two columns leaves both
/// sides cramped however large it is.
///
/// The judgement is made on **the Pane, not the screen** — when the workspace splits in half,
/// the settings screen inside it has to go back to one column.
#[must_use]
pub fn split_width(cx: &Cx<'_>) -> Option<f32> {
    let rect = cx.pane.rect;
    let (w, h) = (rect.width(), rect.height().max(1.0));
    (w >= SPLIT_MIN_WIDTH && w / h >= SPLIT_MIN_ASPECT).then(|| (w * LIST_FRACTION).min(LIST_MAX))
}
/// Lay out a navigation rail beside a page panel.
///
/// The rail uses the chrome surface role; `deco` styles the page panel. When
/// [`split_width`] determines that the pane is too narrow or upright, the rail
/// callback is not called and the page uses the available area.
///
/// The rail and page callbacks return separate results. The selected rail value is
/// returned after both callbacks, allowing the caller to update shared state once
/// their borrows have ended. [`Rail`] adds folding and other rail options.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// # let mut open = 0usize;
/// use fairing::{icon, layout};
/// let (picked, ()) = layout::rail_split(
///     ui,
///     cx,
///     |ui, cx| {
///         let names = ["Home", "Run", "Results"];
///         (0..names.len()).find(|i| {
///             layout::list_item(ui, cx, icon::HOME, names[*i], open == *i).clicked()
///         })
///     },
///     |ui, cx| layout::note(ui, cx, "the page"),
/// );
/// if let Some(i) = picked.flatten() {
///     open = i;
/// }
/// # }
/// ```
pub fn rail_split<A, B>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    rail: impl FnOnce(&mut Ui, &mut Cx<'_>) -> A,
    page: impl FnOnce(&mut Ui, &mut Cx<'_>) -> B,
) -> (Option<A>, B) {
    let out = Rail::new().show(ui, cx, rail, page);
    (out.picked, out.page)
}
/// Inject the page panel's colours and corner into [`rail_split`].
///
/// For anything more than that — a rail a swipe can fold, or two rails on one screen — use
/// [`Rail`], which this is a shorthand for.
pub fn rail_split_with<A, B>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    deco: Deco,
    rail: impl FnOnce(&mut Ui, &mut Cx<'_>) -> A,
    page: impl FnOnce(&mut Ui, &mut Cx<'_>) -> B,
) -> (Option<A>, B) {
    let out = Rail::new().deco(deco).show(ui, cx, rail, page);
    (out.picked, out.page)
}
/// How far a finger has to travel across the arm before it counts as a fold rather than a tap,
/// and the strip along the left edge that still counts as the arm once it has folded away (so a
/// [`Fold::Away`] rail can be dragged back out without anything to press): both are the edge
/// zone, `metrics.edge_px`, which follows the finger as the other edge gestures' zones do.
pub(super) fn rail_grip(cx: &Cx<'_>) -> f32 {
    cx.theme.metrics.edge_px
}
/// The chrome band the arm paints: `arm_rect`, pushed out to the pane's edge on any side where
/// only the screen inset keeps it from the glass. The right edge is the arm's.
pub(super) fn chrome_band(cx: &Cx<'_>, full: egui::Rect, arm_rect: egui::Rect) -> egui::Rect {
    let pane = cx.pane.outer;
    let inset = cx.theme.metrics.screen_inset + 0.5;
    let left = if full.left() - pane.left() <= inset {
        pane.left()
    } else {
        full.left()
    };
    let top = if full.top() - pane.top() <= inset {
        pane.top()
    } else {
        full.top()
    };
    let bottom = if pane.bottom() - full.bottom() <= inset {
        pane.bottom()
    } else {
        full.bottom()
    };
    egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(arm_rect.right(), bottom))
}
/// **The rail arm's width, or `None` where the panel cannot carry one** — [`rail_split`]'s
/// judgement, made on the same grounds as [`split_width`]'s and with the rail's own proportions.
///
/// A caller normally does not need this: `rail_split` folds by itself. It is public so a screen
/// can lay its header out knowing whether a rail is beside it.
#[must_use]
pub fn rail_width(cx: &Cx<'_>) -> Option<f32> {
    let rect = cx.pane.rect;
    let (w, h) = (rect.width(), rect.height().max(1.0));
    (w >= SPLIT_MIN_WIDTH && w / h >= SPLIT_MIN_ASPECT)
        .then(|| (w * RAIL_FRACTION).clamp(RAIL_MIN, RAIL_MAX))
}
/// Decide whether to divide top and bottom, and if so hand back **the top band's height** —
/// [`split_width`]'s vertical counterpart.
///
/// On an upright panel (an ordering kiosk, an access terminal, portrait signage) the top of the
/// screen is out of reach. So the top becomes a **band to look at** and the bottom a **band to
/// press** — which [`action_bar`] cannot do, since it only pins a bar to the bottom and cannot move
/// the body's **starting point** down.
///
/// **The judgement is made on the Pane and the height measured from `ui`.** "Is this device worth
/// dividing into bands" is a question about the device, so like [`split_width`] it looks at
/// `cx.pane`; the height handed back is measured from where the caller will actually cut (`ui`'s
/// remaining Rect). Wider than it is tall, or lower than **12 times the row height**, gives `None`
/// (lower than that and neither band can hold anything), and then the screen can simply use the
/// whole `ui` — the same convention as `split_width` returning `None` when it is narrow.
///
/// **The crate does not set the ratio.** How much the top band should be is decided by the mounting
/// height (the stand, the panel size, the statutory control range), and that is not something the
/// crate can know. Give `top` that share and it is clamped to **0.15 … 0.60** — 0.1 is padding
/// rather than a band, and 0.7 pushes the control band below where the hand reaches.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout;
/// // A 900 mm panel on an 800 mm stand — the top third is the eye's.
/// let Some(hero_h) = layout::split_height(ui, cx, 0.34) else {
///     return; // laid on its side, or a low panel: draw it as one block
/// };
/// let full = ui.available_rect_before_wrap();
/// let split = full.top() + hero_h;
/// # let _ = split;
/// # }
/// ```
#[must_use]
pub fn split_height(ui: &Ui, cx: &Cx<'_>, top: f32) -> Option<f32> {
    let pane = cx.pane.rect;
    let avail = ui.available_rect_before_wrap();
    let tall_enough = pane.height() >= cx.theme.metrics.row_height * BAND_MIN_ROWS;
    (pane.height() > pane.width() && tall_enough)
        .then(|| avail.height() * top.clamp(BAND_TOP_MIN, BAND_TOP_MAX))
}
/// Wrap a page in an egui scroll area with Fairing side padding.
///
/// Use a stable, distinct ID for each page whose scroll position should be retained
/// independently. Reusing an ID across tabs shares their retained scroll position.
pub fn page(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    id: impl std::hash::Hash + std::fmt::Debug,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
) {
    let bottom = cx.pane.inset_bottom;
    let gap = cx.theme.metrics.row_height * CARD_GAP;
    // The height is **stated explicitly.** Where the parent's size is undecided, `available_height()`
    // is infinite and the `ScrollArea` decides "nothing overflows", killing the scroll entirely.
    let height = cx.pane.rect.height().max(cx.theme.metrics.row_height);
    egui::ScrollArea::vertical()
        .id_salt(&id)
        .max_height(height)
        // The default is `DragScroll::OnTouch`, so a list does not push on a device operated with a
        // mouse or a trackball. Touch is common on devices, but it is not the only thing.
        .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(gap);
            body(ui, cx);
            // The bottom is padded so the OSK and the nav bar do not eat the last card.
            ui.add_space(bottom + gap * 2.0);
            keep_focus_clear(ui, cx, &id);
        });
}
/// **A page keeps what is being typed into clear of the keyboard**.
///
/// The keyboard lies over the bottom of the pane and pushes nothing. A page pads its foot
/// by the keyboard's height so its rows *can* be scrolled clear of it, but nothing scrolled them:
/// a field in the lower half of the screen was typed into blind (user report). So while the
/// keyboard is rising, or the focus has just moved to another widget, the page scrolls the
/// focused widget up until it sits half a row above the keys. Once it is clear the page is left
/// alone — a page the operator scrolled by hand while typing stays where they put it.
///
/// Two pages side by side each see the one focus; only the page whose view holds the widget
/// moves.
pub(super) fn keep_focus_clear(
    ui: &Ui,
    cx: &Cx<'_>,
    id: &(impl std::hash::Hash + std::fmt::Debug),
) {
    let inset = cx.pane.inset_bottom;
    let ctx = ui.ctx();
    let inset_key = egui::Id::unique(("fairing.page.keyboard", id));
    let focus_key = inset_key.with("focus");
    let expect_key = inset_key.with("expect");
    let was = ctx.data(|d| d.get_temp::<f32>(inset_key).unwrap_or(0.0));
    ctx.data_mut(|d| d.insert_temp(inset_key, inset));
    if inset <= 0.0 {
        ctx.data_mut(|d| {
            d.remove_temp::<Option<egui::Id>>(focus_key);
            d.remove_temp::<f32>(expect_key);
        });
        return;
    }
    let Some(focused) = ctx.memory(egui::Memory::focused) else {
        return;
    };
    let last = ctx
        .data(|d| d.get_temp::<Option<egui::Id>>(focus_key))
        .flatten();
    ctx.data_mut(|d| d.insert_temp(focus_key, Some(focused)));
    let rising = inset > was + 0.01;
    let moved = last != Some(focused);
    if !rising && !moved {
        ctx.data_mut(|d| d.remove_temp::<f32>(expect_key));
        return;
    }
    let Some(rect) = ctx.read_response(focused).map(|r| r.rect) else {
        return;
    };
    if !ui.clip_rect().intersects(rect) {
        return;
    }
    let clear = cx.pane.rect.max.y - inset - cx.theme.metrics.row_height * 0.5;
    // Where the widget's foot will be once every scroll already asked for has landed. A request
    // takes egui a frame or two to show in the rect, and measuring the gap again from the old
    // rect in the meantime asked for it twice over - the first build sent the field off the top.
    // A foot higher than expected is the operator's own scroll, and is taken as it stands.
    let observed = rect.max.y;
    let expected = if moved {
        observed
    } else {
        ctx.data(|d| d.get_temp::<f32>(expect_key))
            .unwrap_or(observed)
    };
    let base = expected.min(observed);
    let gap = base - clear;
    let next = if gap > 0.5 {
        // A negative delta moves the content up. At once, not eased: the keyboard's own rise is
        // the motion, and this is measured again on every frame of it.
        ui.scroll_with_delta_animation(
            egui::Vec2::new(0.0, -gap),
            egui::style::ScrollAnimation::none(),
        );
        base - gap
    } else {
        base
    };
    ctx.data_mut(|d| d.insert_temp(expect_key, next));
}
/// Declare a screen whose body is wrapped in [`page`] for scrolling and padding.
/// The resulting [`ScreenDecl`] accepts the usual title, icon, gate and chrome
/// configuration.
///
/// ```no_run
/// # fn main() {
/// # let shell: &mut fairing::Shell = todo!();
/// # fn body(_: &mut egui::Ui, _: &mut fairing::Cx<'_>) {}
/// use fairing::{layout, screen};
///
/// shell.add(screen("app.heater", |ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>| {
///     layout::page(ui, cx, "app.heater", |ui, cx| body(ui, cx));
/// }).title("Heater"));
/// # }
/// ```
///
/// ```no_run
/// # fn main() {
/// # let shell: &mut fairing::Shell = todo!();
/// # fn body(_: &mut egui::Ui, _: &mut fairing::Cx<'_>) {}
/// use fairing::layout;
///
/// shell.add(layout::page_screen("app.heater", body).title("Heater"));
/// # }
/// ```
pub fn page_screen(
    id: impl Into<String>,
    mut body: impl FnMut(&mut Ui, &mut Cx<'_>) + 'static,
) -> ScreenDecl {
    let id = id.into();
    let salt = id.clone();
    screen(id, move |ui: &mut Ui, cx: &mut Cx<'_>| {
        page(ui, cx, &salt, |ui, cx| body(ui, cx));
    })
}
/// A section heading paired with a card. Use [`group`] for a card alone or
/// [`section`] for a heading without a card.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout::{info_row, section_card};
///
/// section_card(ui, cx, "Hopper", |ui, cx| {
///     let _ = info_row(ui, cx, "Resin A", "72 %");
///     let _ = info_row(ui, cx, "Resin B", "40 %");
/// });
/// # }
/// ```
pub fn section_card(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
) {
    section_card_with(ui, cx, title, Deco::new(), body);
}
/// [`section_card`] with explicit container decoration; see [`Container`].
pub fn section_card_with(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    deco: Deco,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
) {
    section(ui, cx, title);
    group_with(ui, cx, deco, body);
}
/// **A horizontally scrolling lane of chips.**
///
/// The vessel only — what goes in it is the caller's, which is how a *multi*-select filter row is
/// written: put [`Chip`](crate::widgets::Chip)s in it yourself, each editing a bit of your own.
/// For the ordinary single-select case, [`ChipRow`](crate::widgets::ChipRow) owns the index and
/// keeps the "exactly one" invariant in one place.
///
/// The lane is inset by `metrics.screen_inset` at each end rather than by the screen's own margin,
/// which is what a full-bleed scroller looks like: the reference row this was measured from starts
/// ten pixels outside its own card column, so the chips run under the screen edge instead of
/// stopping short of it.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// # let mut hot = false;
/// use fairing::layout::chip_row;
/// use fairing::widgets::Chip;
///
/// chip_row(ui, cx, "menu.filters", |ui, cx| {
///     let _ = Chip::new("Hot", &mut hot).show(ui, &mut cx.widgets());
/// });
/// # }
/// ```
pub fn chip_row<R>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    id: &str,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>) -> R,
) -> R {
    let inset = cx.theme.metrics.screen_inset;
    let gap = cx.theme.control.gap;
    let out = egui::ScrollArea::horizontal()
        .id_salt(id)
        .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.add_space(inset);
                let out = body(ui, cx);
                ui.add_space(inset);
                out
            })
            .inner
        });
    crate::widgets::paint_lane_fade(
        ui,
        &cx.widgets(),
        out.inner_rect,
        out.content_size.x,
        out.state.clamped_offset().x,
    );
    ui.add_space(cx.theme.metrics.row_height * CARD_GAP);
    out.inner
}
/// A scrolling body with a bar pinned at the bottom.
///
/// The body scrolls within the remaining height. The return value is the bar
/// callback's result. `bar_rows` expresses bar height in multiples of
/// `metrics.row_height`, so it follows the configured size policy.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout;
/// use fairing::widgets::{BigButton, ButtonKind};
///
/// let pay = layout::action_bar(
///     ui,
///     cx,
///     1.8,
///     |ui, cx| {
///         // A body that may run long — it scrolls by itself.
///         for line in 0..40 {
///             layout::info_row(ui, cx, "Item", &line.to_string());
///         }
///     },
///     |ui, cx| {
///         BigButton::new("Pay")
///             .kind(ButtonKind::Primary)
///             .show(ui, &mut cx.widgets())
///             .clicked()
///     },
/// );
/// # let _ = pay;
/// # }
/// ```
pub fn action_bar<R>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    bar_rows: f32,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
    bar: impl FnOnce(&mut Ui, &mut Cx<'_>) -> R,
) -> R {
    action_bar_with(ui, cx, bar_rows, Deco::new(), body, bar)
}
/// Inject colours and corners into [`action_bar`]. For painting a payment bar in the brand colour.
pub fn action_bar_with<R>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    bar_rows: f32,
    deco: Deco,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
    bar: impl FnOnce(&mut Ui, &mut Cx<'_>) -> R,
) -> R {
    let m = &cx.theme.metrics;
    let height = m.row_height * bar_rows.max(1.0);
    let radius = deco.radius_of(cx);
    let fill = deco.fill_of(ui, cx);
    let inset = m.screen_inset;
    // When the OSK comes up the bar hides below it — screens where confirm has to be pressed with the
    // keyboard up are common.
    let bottom = cx.pane.inset_bottom;

    let area = ui.available_rect_before_wrap();
    // **The logical place.** The body flows only this far and the scroll does not go below it.
    let slot = egui::Rect::from_min_max(
        egui::pos2(area.left(), area.bottom() - height - bottom),
        egui::pos2(area.right(), area.bottom() - bottom),
    );
    // **The place drawn.** Given a `visual_inset` the bar lifts off the screen's edge and becomes a
    // floating bar — the place is still taken, so the body does not flow underneath it.
    let vi = deco.visual_inset_of();
    let bar_rect = if vi > 0.0 { slot.shrink(vi) } else { slot };
    let body_rect = egui::Rect::from_min_max(
        area.min,
        egui::pos2(area.right(), slot.top().max(area.top())),
    );

    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body_rect));
    body_ui.set_clip_rect(body_rect);
    egui::ScrollArea::vertical()
        .id_salt("fairing.layout.action_bar")
        .max_height(body_rect.height())
        .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
        .auto_shrink([false, false])
        .show(&mut body_ui, |ui| {
            body(ui, cx);
            ui.add_space(cx.theme.metrics.row_height * CARD_GAP);
        });

    let level = deco.elevation_of(Elevation::Floating);
    // A bar draws in order, so its height goes down first and the fill covers the shadow's
    // interior — no reserved index is needed here, unlike a card whose rect `Frame` decides.
    {
        let corner = egui::CornerRadius::same(radius);
        let ink = cx.theme.elevate(level);
        if ink.shadow != egui::epaint::Shadow::NONE {
            ui.painter().add(ink.shadow.as_shape(bar_rect, corner));
        }
        if ink.rim != egui::Stroke::NONE {
            ui.painter()
                .rect_stroke(bar_rect, corner, ink.rim, egui::StrokeKind::Inside);
        }
    }
    if fill.a() > 0 {
        // A bar against the edge is rounded on its top corners only — the bottom is the screen's end and
        // the curvature is cut. A floating bar (`visual_inset`) shows all four.
        let corner = if vi > 0.0 {
            egui::CornerRadius::same(radius)
        } else {
            egui::CornerRadius {
                nw: radius,
                ne: radius,
                sw: 0,
                se: 0,
            }
        };
        ui.painter().rect_filled(bar_rect, corner, fill);
    }
    deco.paint_stroke(ui, cx, bar_rect);
    // **`Divided` is full-bleed here too**. A bar of buttons is inset from the screen's
    // edge, because a `BigButton` flush against the glass reads as a cut-off one. A bar holding a
    // *row* is the opposite case: `list_item` and the rest do their own insetting, so a second one
    // here puts the foot's icon a `screen_inset` to the right of the icons in the list above it,
    // which is what a rail's pinned Power Off once looked like. The container kind
    // already carries this distinction everywhere else, so it carries it here.
    let bar_inset = match deco.container_kind() {
        Container::Divided => 0.0,
        Container::Filled | Container::Outlined | Container::Tinted => inset,
    };
    let mut bar_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_rect.shrink2(egui::vec2(bar_inset, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar_ui.set_clip_rect(bar_rect);
    let out = bar(&mut bar_ui, cx);
    ui.advance_cursor_after_rect(area);
    out
}
/// [`Grid`] with the default decoration, in one line (the same as `Grid::new(..).show(..)`).
pub fn grid<T>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    min_cell_rows: f32,
    cell_rows: f32,
    items: &[T],
    cell: impl FnMut(&mut Ui, &mut Cx<'_>, Cell<'_, T>),
) {
    Grid::new(min_cell_rows, cell_rows).show(ui, cx, items, cell);
}
/// How many columns fit the width (at least 1).
pub(super) fn columns_for(avail: f32, min_cell: f32, gap: f32) -> usize {
    let mut cols = 1usize;
    while gap.mul_add(f32_from(cols + 1) + 1.0, min_cell * f32_from(cols + 1)) <= avail {
        cols += 1;
        if cols >= 12 {
            break;
        }
    }
    cols
}
/// `usize` → `f32`. The column count is 12 or under, so it is exact.
#[expect(clippy::cast_precision_loss, reason = "at most 12 columns")]
pub(super) fn f32_from(value: usize) -> f32 {
    value as f32
}
/// The screen title. Once, large, above the cards — it says what the right column is in a left/right split.
///
/// [`header`] with no subtitle and no actions, so the one that has them and the one that does not
/// start their text at the same place.
pub fn title(ui: &mut Ui, cx: &mut Cx<'_>, text: &str) {
    header(ui, cx, text, None, |_, _| ());
}
/// Page header with title, optional subtitle and a trailing action slot.
///
/// The left edge follows `metrics.content_inset`. The subtitle uses
/// `type_scale.small` and `ColorRole::Muted`; trailing actions are laid out
/// right-to-left and centred on the title line. The title uses the configured
/// display face. This page heading is separate from `ScreenDecl::title`, which the
/// shell uses in navigation and recent-screen views.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout;
/// use fairing::widgets::{BigButton, ButtonKind};
///
/// layout::header(ui, cx, "Appearance", Some("Display and interface"), |ui, cx| {
///     let _ = BigButton::new("Restore defaults")
///         .kind(ButtonKind::Normal)
///         .show(ui, &mut cx.widgets());
/// });
/// # }
/// ```
pub fn header<R>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    subtitle: Option<&str>,
    actions: impl FnOnce(&mut Ui, &mut Cx<'_>) -> R,
) -> R {
    let (inset, gap, line_gap) = (
        cx.theme.metrics.content_inset,
        cx.theme.metrics.row_height * CARD_GAP,
        cx.theme.control.line_gap,
    );
    let title_font = cx.theme.display(cx.theme.metrics.type_scale.heading);
    let sub_font = egui::FontId::proportional(cx.theme.metrics.type_scale.small);
    let title_h = ui.ctx().fonts_mut(|f| f.row_height(&title_font));
    let sub_h = subtitle.map_or(0.0, |_| {
        line_gap + ui.ctx().fonts_mut(|f| f.row_height(&sub_font))
    });
    // **A band with actions in it is at least a control tall**, so a button in the header has
    // room and two headers with buttons sit at one height. A band with none is as tall as its
    // words: reserving a control's height under a bare title left a gap between the title and the
    // first card that read as a missing row, and at a one-finger row it was most of one.
    let block_h = title_h + sub_h;
    let control_h = crate::theme::control_height(&cx.theme.metrics, &cx.theme.control);
    let top = ui.cursor().min;
    let width = ui.available_width();
    let with_actions = block_h.max(control_h);
    // The actions first, right to left — then the words take what is left. Painted first and
    // unbounded, a title ran under the actions as soon as the pane was narrower than both: a split
    // pane put "Dashboard" through its own badge.
    // Centred on the title's line, not on the band: see the doc.
    let slot = egui::Rect::from_min_max(
        egui::pos2(top.x + inset, top.y),
        egui::pos2(
            top.x + width - inset,
            top.y + title_h.max(with_actions - sub_h),
        ),
    );
    let (out, used) = {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(slot)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let out = actions(&mut child, cx);
        (out, child.min_rect())
    };
    let has_actions = used.width() > 0.5;
    let band_h = if has_actions { with_actions } else { block_h };
    let (band, _) = ui.allocate_exact_size(egui::vec2(width, band_h), egui::Sense::hover());
    let text = egui::Rect::from_min_max(
        egui::pos2(band.min.x + inset, band.min.y),
        egui::pos2(band.max.x - inset, band.max.y),
    );
    let words_end = if has_actions {
        (used.min.x - inset).max(text.min.x)
    } else {
        text.max.x
    };
    let room = (words_end - text.min.x).max(1.0);
    // **Stacked from the top, not centred on it.** Centring put the subtitle's baseline exactly on
    // the band's bottom edge — the arithmetic came out flush to the last decimal — so a pixel of
    // rounding, or a face whose descender runs past `row_height`, let it under whatever was drawn
    // next. It showed up as a subtitle sliced in half by the first card below it.
    let mut y = band.min.y + (band_h - block_h).max(0.0) * 0.5;
    let painter = ui.painter();
    let line = |text: &str, font: egui::FontId, color: egui::Color32| {
        let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        painter.layout_job(job)
    };
    let title_color = cx.theme.color(ColorRole::OnSurface);
    painter.galley(
        egui::pos2(text.min.x, y),
        line(title, title_font, title_color),
        title_color,
    );
    y += title_h + line_gap;
    if let Some(sub) = subtitle {
        let muted = cx.theme.color(ColorRole::Muted);
        painter.galley(egui::pos2(text.min.x, y), line(sub, sub_font, muted), muted);
    }
    // **Put the cursor back below the whole band.** The actions live in a child whose rect stops at
    // the title's line, and a scope advances its parent by what the child used — so a header with
    // no actions left the cursor *above* the subtitle, and the next thing drawn landed on it. It
    // showed as a subtitle with its descenders sliced off by the card below.
    ui.advance_cursor_after_rect(band);
    ui.add_space(gap);
    out
}
/// A subheading above a card, using muted colour, small text and letter spacing.
pub fn section(ui: &mut Ui, cx: &Cx<'_>, text: &str) {
    let m = &cx.theme.metrics;
    // `content_inset` — see `title`. A subheading belongs to the card below it and has to line up
    // with that card's rows, not sit its own distance in.
    let size = m.type_scale.small;
    // **Its own text tall, not a control tall.** Laid out in `ui.horizontal`, the line took egui's
    // `interact_size` as its height — one touch target — and the heading floated in a band as
    // tall as a row, as far from its card as from the one above it. A `Frame` indents it without
    // that floor.
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: round_i8(m.content_inset),
            right: 0,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(size)
                    // Tracking scales with the text, so it stays proportional at any density.
                    .extra_letter_spacing(size * SECTION_TRACKING)
                    .color(cx.theme.color(ColorRole::Muted)),
            );
        });
    ui.add_space(m.row_height * SECTION_GAP);
}
/// The explanation below a card. One line on what the group does — pushed into a row's subtitle it would thicken the list.
pub fn note(ui: &mut Ui, cx: &Cx<'_>, text: &str) {
    let m = &cx.theme.metrics;
    let pad = round_i8(m.content_inset);
    ui.add_space(m.row_height * NOTE_GAP);
    // **The padding is given through a `Frame`.** Pushed with `add_space` inside a `horizontal_wrapped`,
    // **only the first line** is indented and the wrapped lines cling to the left edge — an explanation
    // running over two lines came out stepped. A `Frame` narrows the wrap width with it.
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: pad,
            right: pad,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_max_width(m.type_scale.small * NOTE_MEASURE);
            ui.label(
                egui::RichText::new(text)
                    .size(m.type_scale.small)
                    .color(cx.theme.color(ColorRole::Muted)),
            );
        });
    // The same gap a card leaves under itself, so the next section's heading does not sit on the
    // explanation's last line.
    ui.add_space(m.row_height * CARD_GAP);
}
/// Draw a hairline before each row except the first in a container.
/// The cursor position identifies the first row. The line starts at the row's text
/// inset and ends at the card edge.
pub(super) fn card_divider(ui: &mut Ui, cx: &Cx<'_>) {
    let y = ui.cursor().min.y;
    if y <= ui.max_rect().min.y + 0.5 {
        return;
    }
    let pad = cx.theme.components.list_row.pad;
    let rect = ui.max_rect();
    ui.painter().hline(
        rect.min.x + pad..=rect.max.x,
        y,
        egui::Stroke::new(
            cx.theme.control.stroke_hairline,
            cx.theme.color(ColorRole::Outline),
        ),
    );
}
/// Group related content into a rounded card, with spacing between groups.
/// Row helpers determine any internal dividers.
pub fn group(ui: &mut Ui, cx: &mut Cx<'_>, body: impl FnOnce(&mut Ui, &mut Cx<'_>)) {
    group_with(ui, cx, Deco::new(), body);
}
/// Inject colours, corners and padding into [`group`].
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout::{group_with, info_row, Deco};
/// use fairing::ColorRole;
///
/// // The warning group is set apart by its border.
/// group_with(
///     ui,
///     cx,
///     Deco::new().stroke(1.0, ColorRole::Warning).radius(8.0),
///     |ui, cx| {
///         info_row(ui, cx, "Pressure", "Critical");
///     },
/// );
/// # }
/// ```
pub fn group_with(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    deco: Deco,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
) {
    let m = &cx.theme.metrics;
    let inset = m.screen_inset;
    let gap = m.row_height * CARD_GAP;
    let pad = deco.padding.unwrap_or(m.card_pad);
    // Why the default background is **`SurfaceVariant`**: a screen Pane's default background is already
    // `Surface` (`workspace::draw_pane`), so with the cards at `Surface` too the colours match and the
    // group disappears whole — which is how it was built at first, and measuring pixels caught it.
    // On a panel that is `SurfaceVariant` itself the card takes `Surface` — see `Container`.
    let fill = deco.fill_of(ui, cx);
    let radius = deco.radius_of(cx);
    // `visual_inset` narrows **the card only** — the place stays and the drawing is inset, making a
    // "floating" card. To shrink the place itself, `screen_inset` is the one.
    let vi = deco.visual_inset_of();
    // **The shadow is reserved before the frame and set after it.** A card's rect is not known
    // until `Frame::show` returns, and a shape added afterwards would paint *over* the card rather
    // than under it. `Shape::Noop` holds the place in the paint order; this is the pattern egui's
    // own `Frame` uses for its background.
    // The margins the card is drawn inside — the shadow, the rim and the stroke follow the
    // drawing, not the place: drawn on the place, the rim stood `screen_inset` clear of the
    // fill on both sides and hugged it top and bottom.
    let outer = egui::Margin::symmetric(
        round_i8(if deco.kind == Container::Divided {
            vi
        } else {
            inset + vi
        }),
        round_i8(vi),
    );
    let under = ui.painter().add(egui::Shape::Noop);
    let level = deco.elevation_of(cx.theme.card_elevation());
    let response = egui::Frame::NONE
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(radius))
        // **`Divided` is full-bleed.** Its rule closes a group rather than boxing it, and a rule
        // that stops short of the screen's edge reads as the bottom of a card that forgot to draw
        // the rest of itself — see `Container`.
        .outer_margin(outer)
        // Breathing room above and below the card. The first and last rows against the corners make the rounding look cut.
        .inner_margin(egui::Margin::symmetric(
            round_i8(deco.pad_x.unwrap_or(if deco.pad_content {
                m.content_inset
            } else {
                0.0
            })),
            round_i8(pad),
        ))
        .show(ui, |ui| {
            // **A card takes the width it is given; it does not grow to fit its content.**
            //
            // egui's default is the other way round. A child wider than the space it was handed
            // expands the parent's `max_rect` through `advance_cursor_after_rect`, and because a
            // card usually sits in a column, that widened column is then inherited by **every
            // later card in it**. Measured on the palette sheet: one segmented control 65 px too
            // wide for its half-width column pushed the cards below it 65, 65, 130 and 245 px past
            // the panel's right edge, where the screen clipped their trailing text away. The
            // control that did not fit was the fourth-most visible symptom of its own defect.
            //
            // So the cursor is advanced by a rect pinned to the card's own bounds — the same
            // number on both sides, which is also what the old `set_width(available_width())` was
            // for. A child that will not fit now bleeds over its own card's edge, where it is
            // obvious and local, instead of moving its neighbours.
            //
            // It is **not clipped**: a [`Dropdown`](crate::widgets::Dropdown) opens its list as a
            // floating layer bounded by `ui.clip_rect()`, so narrowing the clip here would shut
            // the list inside the card that opened it.
            // The body runs in a **child** `Ui`, not this one: `new_child` reports nothing back on
            // its own (unlike `scope`), so an over-wide grandchild expands the child's `min_rect`
            // and stops there. The cursor is then advanced by hand, by a rect pinned to the card's
            // own bounds on both sides — which is also the job the old `set_width` was doing.
            let bounds = ui.max_rect();
            let mut inner =
                ui.new_child(egui::UiBuilder::new().max_rect(bounds).layout(*ui.layout()));
            body(&mut inner, cx);
            let used = inner.min_rect().max.y;
            ui.advance_cursor_after_rect(egui::Rect::from_min_max(
                bounds.min,
                egui::pos2(bounds.max.x, used),
            ));
        });
    let rect = response.response.rect - outer;
    let corner = egui::CornerRadius::same(radius);
    let ink = cx.theme.elevate(level);
    if ink.shadow != egui::epaint::Shadow::NONE {
        ui.painter().set(under, ink.shadow.as_shape(rect, corner));
    }
    if ink.rim != egui::Stroke::NONE {
        ui.painter()
            .rect_stroke(rect, corner, ink.rim, egui::StrokeKind::Inside);
    }
    deco.paint_stroke(ui, cx, rect);
    ui.add_space(gap);
}
/// The scrim: transparent at the top, `Scrim`'s own alpha at the foot.
///
/// A mesh rather than a stack of `rect_filled`s, because a ramp built from bands shows its bands on
/// an 8-bit panel — the same reason the elevation token's own doc gives for its pixel floors.
pub(super) fn paint_scrim(ui: &Ui, cx: &Cx<'_>, rect: egui::Rect) {
    let base = cx.theme.color(ColorRole::Scrim);
    let clear = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 0);
    let mut mesh = egui::Mesh::default();
    let top = rect.top() + rect.height() * (1.0 - HERO_SCRIM);
    let band = egui::Rect::from_min_max(egui::pos2(rect.min.x, top), rect.max);
    for (pos, color) in [
        (band.left_top(), clear),
        (band.right_top(), clear),
        (band.left_bottom(), base),
        (band.right_bottom(), base),
    ] {
        mesh.colored_vertex(pos, color);
    }
    for v in &mut mesh.vertices {
        v.uv = egui::pos2(0.0, 0.0);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(2, 1, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}
/// The page dots, inside the picture's foot.
pub(super) fn paint_dots(ui: &Ui, cx: &Cx<'_>, rect: egui::Rect, (current, count): (usize, usize)) {
    const MAX_DOTS: usize = 12;
    if count < 2 {
        return;
    }
    // At most `MAX_DOTS` dots: past that, a window of them that keeps the live one in it.
    let first = current
        .saturating_sub(MAX_DOTS / 2)
        .min(count.saturating_sub(MAX_DOTS));
    let count = u16::try_from(count.min(MAX_DOTS)).unwrap_or(u16::MAX);
    let d = cx.theme.control.badge_h * cx.theme.control.badge_dot_ratio * 0.5;
    let gap = cx.theme.control.gap;
    let total = (d + gap).mul_add(f32::from(count) - 1.0, d);
    let y = rect.max.y - cx.theme.metrics.card_pad - d * 0.5;
    let mut x = rect.center().x - total * 0.5 + d * 0.5;
    let live = cx.theme.color(ColorRole::Primary);
    let waiting = cx
        .theme
        .color(ColorRole::OnPrimary)
        .gamma_multiply(cx.theme.control.badge_alpha);
    for slot in 0..count {
        let on = first + usize::from(slot) == current;
        ui.painter()
            .circle_filled(egui::pos2(x, y), d * 0.5, if on { live } else { waiting });
        x += d + gap;
    }
}
