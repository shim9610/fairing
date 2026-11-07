//! Rows.

use super::{
    card_divider, card_radius, paint_dots, paint_scrim, round_u8, rounded, unroll, ColorRole, Cx,
    FoldFrame, IconRef, ListRow, MarkGeometry, Response, SelectMark, Tab, TouchSlider, Ui,
    Unrolled, BAR_GUTTER, BAR_SHARE, CARD_GAP, DISC_RATIO, DISC_TINT, FOLD_KEY, HERO_ASPECT,
    LABEL_FADE, SLIDER_COL_GAP, SLIDER_INLINE_TRACK, SLIDER_LABEL_FRACTION, SLIDER_LABEL_MAX,
    SLIDER_PAD, SLIDER_VALUE_WIDTH, TAB_MARK, TOTAL_ROW,
};

/// A row leading into a sub-screen (with a chevron on the right).
pub fn nav_row(ui: &mut Ui, cx: &mut Cx<'_>, title: &str, value: Option<&str>) -> Response {
    card_divider(ui, cx);
    let mut row = ListRow::new(title).chevron(true).separator(false);
    if let Some(value) = value {
        row = row.trailing(value);
    }
    row.show(ui, &mut cx.widgets())
}
/// **A full-bleed picture with a headline over it**, and page dots under it where there is more
/// than one.
///
/// Both reference kiosks open with one: a 16:9 band carrying a promotion, a line of text and a CTA,
/// with three or four dots at its foot. It is a [`layout`](crate::layout) function and not a widget because
/// it divides the screen — it takes the width it is given and the height its aspect asks for, and
/// hands the caller a `Ui` for whatever goes on top.
///
/// # Why the scrim is a gradient and not a role
///
/// Text over a photograph has **no guaranteed contrast** — the crate's own gate cannot help,
/// because the ground is the integrator's picture. The shell already answers this for the desktop's
/// wallpaper labels with [`LabelLegibility`](crate::desktop::LabelLegibility), and its middle
/// answer is the right one here: a scrim, laid as a gradient from transparent at the top to
/// [`Scrim`](fairing_widgets::theme::ColorRole::Scrim)'s own alpha at the foot, so the headline sits
/// on something dark and the top of the picture is untouched.
///
/// A flat plate over the whole band (`Veil`) is certain and kills the photograph, which is the
/// reason the band exists; a text shadow alone (`Shadow`) is not enough under a headline this size.
/// The gradient is the crate's own `LabelLegibility::Veil` made directional.
///
/// # Why the dots are the caller's count and the crate's drawing
///
/// Which page is showing, and how paging happens, is the caller's — a promotion carousel is timed
/// in one shop and swiped in another. What the crate settles is that the dots are there, that the
/// live one is [`Primary`](fairing_widgets::theme::ColorRole::Primary) and the rest are
/// `OnPrimary` at the `badge_alpha`, and that they sit inside the picture rather than under it, so
/// the band costs exactly its own height. Past twelve pages twelve dots are drawn, a window that
/// follows the live one.
pub fn hero<R>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    aspect: f32,
    pages: (usize, usize),
    body: impl FnOnce(&mut Ui, &mut Cx<'_>, egui::Rect) -> R,
) -> R {
    let m = &cx.theme.metrics;
    let width = ui.available_width();
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        HERO_ASPECT
    };
    let height = (width / aspect).max(m.touch_target);
    let (outer, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let inset = m.screen_inset;
    let rect = outer.shrink2(egui::vec2(inset, 0.0));
    let radius = egui::CornerRadius::same(round_u8(card_radius(m, &cx.theme.control)));

    // The band's own ground, for the frames before a picture has arrived.
    ui.painter()
        .rect_filled(rect, radius, cx.theme.color(ColorRole::SurfaceVariant));
    let out = body(ui, cx, rect);
    paint_scrim(ui, cx, rect);
    paint_dots(ui, cx, rect, pages);
    ui.add_space(m.row_height * CARD_GAP);
    out
}
/// **A strip of equal cells that switches what the whole screen shows.**
///
/// Draws at the cursor and takes `metrics.nav_bar_height`, so it sits against whichever edge the
/// caller puts it — under a [`page`](crate::layout::page) for a bottom bar, before one for a top bar. Returns the index
/// just picked, and only then. A label wider than its cell is cut with an ellipsis, and a glyph
/// wider than its cell shrinks to fit, so nothing runs over the next cell.
///
/// # Which of the three to reach for
///
/// The crate now has three ways to show a row of choices, and they are three different objects:
///
/// | | What it is | Shape |
/// |---|---|---|
/// | [`SegmentedControl`](crate::widgets::SegmentedControl) | A **setting** with a few values | One track, 2–4 equal cells, capped at `segment_max` |
/// | [`ChipRow`](crate::widgets::ChipRow) | A **filter** over a list | 5–8 separate objects, unequal, scrolls |
/// | `tab_bar` | **Navigation** — which screen you are on | N equal cells, full width, pinned to an edge, no track |
///
/// A tab bar is chrome and the other two are content. That is why this is not "a segmented control
/// with the cap lifted": a segmented control lives inside a card and edits a value, and lifting its
/// cap would give a *setting* eight 6 mm cells on the smallest panel, which is the reason the cap
/// is there.
///
/// # Why the active cell is not filled
///
/// The instrument-panel reference fills its active tab; the two consumer references colour the
/// glyph and the word and add a short bar. This crate takes the second, for the reason task #5
/// settled for the quick tiles: an accent that fills furniture has nothing left to say when
/// something is actually selected. The mark is **three channels** — the glyph and the label go to
/// `Primary`, and a bar sits against the bar's own inner edge — so it survives greyscale and it
/// does not flood.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// # let mut tab = 0;
/// use fairing::layout::{tab_bar, Tab};
/// use fairing::icon;
///
/// let tabs = [
///     Tab { label: "Home", icon: icon::HOME },
///     Tab { label: "Orders", icon: icon::LIST },
///     Tab { label: "Help", icon: icon::INFO },
/// ];
/// if let Some(picked) = tab_bar(ui, cx, "shop.tabs", &tabs, &mut tab) {
///     let _ = picked;
/// }
/// # }
/// ```
pub fn tab_bar(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    id: &str,
    tabs: &[Tab<'_>],
    selected: &mut usize,
) -> Option<usize> {
    if tabs.is_empty() {
        return None;
    }
    let m = &cx.theme.metrics;
    let (height, gap, icon) = (
        m.nav_bar_height,
        cx.theme.control.gap,
        cx.theme.control.icon,
    );
    let (bar, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(bar, 0, cx.theme.color(ColorRole::Surface));
    ui.painter().hline(
        bar.x_range(),
        bar.min.y,
        egui::Stroke::new(
            cx.theme.control.stroke_hairline,
            cx.theme.color(ColorRole::Outline),
        ),
    );

    let mut picked = None;
    // A bar with more cells than a `u16` can count is not a bar; the cap keeps the width
    // arithmetic in a range an `f32` represents exactly.
    let count = u16::try_from(tabs.len()).unwrap_or(u16::MAX);
    let cell_w = bar.width() / f32::from(count);
    for (slot, tab) in (0_u16..).zip(tabs.iter()) {
        let index = usize::from(slot);
        let cell = egui::Rect::from_min_size(
            egui::pos2(cell_w.mul_add(f32::from(slot), bar.min.x), bar.min.y),
            egui::vec2(cell_w, bar.height()),
        );
        let response = ui.interact(cell, ui.scope_id().with((id, index)), egui::Sense::click());
        if response.clicked() && index != *selected {
            *selected = index;
            picked = Some(index);
        }
        let on = index == *selected;
        let ink = cx.theme.color(if on {
            ColorRole::Primary
        } else {
            ColorRole::Muted
        });
        // The label and the glyph stay inside their own cell. A label wider than the cell is cut
        // with an ellipsis, like every other one-line text in the crate, rather than running over
        // its neighbours; the glyph shrinks only when the cell is narrower than it.
        let room = gap.mul_add(-2.0, cell_w).max(1.0);
        let icon = icon.min(room);
        let font = egui::FontId::proportional(m.type_scale.small);
        let mut job = egui::text::LayoutJob::simple_singleline(tab.label.to_owned(), font, ink);
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let galley = ui.painter().layout_job(job);
        let block = icon + gap + galley.rect.height();
        let top = cell.center().y - block * 0.5;
        let at = egui::Rect::from_center_size(
            egui::pos2(cell.center().x, top + icon * 0.5),
            egui::Vec2::splat(icon),
        );
        let style = crate::icons::IconStyle {
            color: crate::icons::IconColor::Fixed(ink),
            ..crate::icons::IconStyle::default()
        };
        {
            let widgets = cx.widgets();
            widgets
                .icons
                .paint(ui.painter(), at, &tab.icon, &style, widgets.theme);
        }
        ui.painter().galley(
            egui::pos2(
                cell.center().x - galley.rect.width() * 0.5,
                top + icon + gap,
            ),
            galley,
            ink,
        );
        if on {
            // The third channel: a short bar on the strip's own inner edge, so a greyscale render
            // still says which cell is live.
            let w = cell_w * TAB_MARK;
            let h = cx.theme.control.stroke_mark;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(cell.center().x - w * 0.5, bar.min.y),
                    egui::vec2(w, h),
                ),
                egui::CornerRadius::same(round_u8(h * 0.5)),
                cx.theme.color(ColorRole::Primary),
            );
        }
    }
    picked
}
/// **A summary row heavier than the ones above it** — the last line of a bill, a cycle total, a
/// run count.
///
/// Every reference screen this was measured from draws the same three-part figure: two or three
/// quiet [`info_row`]s, a rule, and one loud row. This is the loud one.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::layout::{group, info_row, total_row};
///
/// group(ui, cx, |ui, cx| {
///     info_row(ui, cx, "Subtotal", "€ 34,00");
///     info_row(ui, cx, "Service charge", "€ 1,70");
///     total_row(ui, cx, "Total", "€ 35,70");
/// });
/// # }
/// ```
///
/// # Why the step is the type scale's own and not a new token
///
/// Measured off the three references, the total's text against the rows above it is 1.31 (Bella
/// Tavola), 1.38 (the `McDonald's` kiosk) and 1.72 (the Sichuan kiosk, whose figures are CJK and whose total
/// is deliberately enormous). `type_scale.heading / type_scale.body` is **1.375** — inside the
/// band the two Latin screens set, and already what the crate means by "one size up". A token of
/// its own would be a fourth number nobody could relate to the other three.
///
/// # Why the value is not the accent
///
/// All three references make the total the loudest text on the screen, and one paints it in its
/// brand red. `Primary` measures 3.81 on `surface` in base dark, under the 4.5 a word needs, so
/// **the accent is never a text colour** — the loudness here is size and weight, which is what the
/// other two references use and what survives a palette swap.
///
/// # Why it is not `info_row` with a flag
///
/// A total is not a row that happens to be bold: it carries a rule above it, a taller band and a
/// different type step. Folding three changes into a flag on the general row would give the
/// general row a mode.
pub fn total_row(ui: &mut Ui, cx: &mut Cx<'_>, title: &str, value: &str) -> Response {
    let m = &cx.theme.metrics;
    let height = m.row_height * TOTAL_ROW;
    let ink = cx.theme.color(ColorRole::OnSurface);
    let font = cx.theme.strong(m.type_scale.heading);
    let inset = m.content_inset;
    let gap = cx.theme.control.gap;

    card_divider(ui, cx);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    // The figure is laid out first and the title is given what is left: a long title must never
    // push the number off the row, because the number is the thing being read.
    let figure = painter.layout_no_wrap(value.to_owned(), font.clone(), ink);
    let room = 2.0f32
        .mul_add(-inset, rect.width() - figure.rect.width() - gap)
        .max(0.0);
    let label = painter.layout(title.to_owned(), font, ink, room);
    painter.galley(
        egui::pos2(
            rect.min.x + inset,
            rect.center().y - label.rect.height() * 0.5,
        ),
        label,
        ink,
    );
    painter.galley(
        egui::pos2(
            rect.max.x - inset - figure.rect.width(),
            rect.center().y - figure.rect.height() * 0.5,
        ),
        figure,
        ink,
    );
    response
}
/// Scroll the page so an opening body is seen: just enough for its end to show, or — where the
/// body is taller than the pane — the header to the top, so the header never leaves the screen.
/// Asked for every frame of the opening and once more when it has settled, against the body's
/// **whole** height, so the scroll lands where the body will end and not where it is this frame.
pub(super) fn reveal_body(ui: &Ui, id: egui::Id, header: egui::Rect, out: &Unrolled) {
    let flag = id.with("reveal");
    if !ui.data(|d| d.get_temp::<bool>(flag).unwrap_or(false)) {
        return;
    }
    let pane = ui.clip_rect();
    let whole = egui::Rect::from_min_max(
        header.min,
        egui::pos2(header.max.x, out.rect.min.y + out.natural),
    );
    if whole.height() > pane.height() {
        ui.scroll_to_rect(header, Some(egui::Align::Min));
    } else {
        ui.scroll_to_rect(whole, None);
    }
    if out.settled {
        ui.data_mut(|d| d.remove_temp::<bool>(flag));
    }
}
/// **The "Advanced" row at the end of a group** (after Android's settings): one row
/// that stands for the rows a group keeps back, its subtitle naming them, and that goes away
/// when tapped as they unroll in its place. One way, for the visit: `revealed` is the caller's,
/// so a screen that wants them back hidden on its next visit resets it.
///
/// `title` is the row's own word — "Advanced", or the screen's language for it. `hidden` names
/// what is behind it, for the subtitle; empty, the row has none. Returns `true` on the frame the
/// row was tapped.
pub fn advanced_rows(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    id_salt: &str,
    title: &str,
    hidden: &[&str],
    revealed: &mut bool,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>),
) -> bool {
    let id = ui.scope_id().with(id_salt);
    let mut tapped = false;
    if !*revealed {
        card_divider(ui, cx);
        let names = hidden.join(", ");
        let mut row = ListRow::new(title).disclosure(false).separator(false);
        if !names.is_empty() {
            row = row.subtitle(&names);
        }
        if row.show(ui, &mut cx.widgets()).clicked() {
            *revealed = true;
            tapped = true;
        }
    }
    if *revealed {
        // No indent: these are the group's own rows, not a body under a header.
        if let Some(band) = unroll(ui, &mut cx.widgets(), id, true, 0.0) {
            let _ = band.show(ui, |ui| body(ui, cx));
        }
    }
    tapped
}
/// **One open at a time.** Run one [`ExpandableRow`](crate::layout::ExpandableRow) (or anything with a `&mut bool`) as slot
/// `index` of a group whose open slot the caller keeps in `open`: opening this one closes the
/// one that was open, and closing it leaves none open. It is a helper and not a mode of the row
/// because auto-collapsing "takes control away from the user" (Windows' own words) and belongs
/// to a short panel, decided by the screen.
///
/// ```no_run
/// # fn body(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>, which: &mut Option<usize>) {
/// use fairing::layout::{self, ExpandableRow};
/// layout::accordion(which, 0, |open| {
///     ExpandableRow::new("net", "Network").show(ui, cx, open, |ui, cx| {
///         layout::info_row(ui, cx, "Wi-Fi", "fairing-lab");
///     });
/// });
/// # }
/// ```
pub fn accordion(open: &mut Option<usize>, index: usize, row: impl FnOnce(&mut bool)) {
    let mut this = *open == Some(index);
    row(&mut this);
    if this {
        *open = Some(index);
    } else if *open == Some(index) {
        *open = None;
    }
}
/// A read-only row. Neither a tap nor a chevron.
pub fn info_row(ui: &mut Ui, cx: &mut Cx<'_>, title: &str, value: &str) -> Response {
    card_divider(ui, cx);
    ListRow::new(title)
        .trailing(value)
        .chevron(false)
        .separator(false)
        .enabled(false)
        .show(ui, &mut cx.widgets())
}
/// A toggle row. `true` **only when the value changed**.
///
/// Pressing anywhere on the row toggles it — with only the switch as the touch target, a gloved hand
/// cannot hit it.
pub fn switch_row(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    subtitle: Option<&str>,
    on: &mut bool,
    enabled: bool,
) -> bool {
    card_divider(ui, cx);
    // **A composed `ListRow`, not a second copy of a row.** This function used to draw its own
    // background, its own title and its own subtitle with `painter.text` at `screen_inset`, because
    // `ListRow` had no trailing-widget slot. The cost was measured: a switch row's label sat four du
    // left of a nav row's label in the same card — five pixels on a real render — and a fix to
    // `ListRow` reached none of the built-in settings screens. The slot (`ListRow::trailing_switch`)
    // is a closed set of the crate's own controls. A slot for an *arbitrary* widget stays refused:
    // that is where screen content, which belongs to the integrator, would start entering the crate.
    //
    // The row owns the tap: pressing anywhere on it toggles, because with only the switch live a
    // gloved hand cannot hit it.
    let mut row = ListRow::new(title)
        .trailing_switch(*on)
        .chevron(false)
        .separator(false)
        .enabled(enabled);
    if let Some(subtitle) = subtitle {
        row = row.subtitle(subtitle);
    }
    let response = row.show(ui, &mut cx.widgets());
    if response.clicked() {
        *on = !*on;
        return true;
    }
    false
}
/// A column that takes **exactly** the width and height asked for. `Ui::allocate_ui_with_layout`
/// advances the cursor only by the content's size, so content narrower than requested pulls the next
/// column forward — where a layout is built out of column widths, that is a collapse of the
/// alignment. This one consumes the place it asked for as it stands.
pub(super) fn column(
    ui: &mut Ui,
    width: f32,
    height: f32,
    align: egui::Align,
    add: impl FnOnce(&mut Ui),
) {
    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width.max(0.0), height));
    let layout = match align {
        egui::Align::Max => egui::Layout::right_to_left(egui::Align::Center),
        _ => egui::Layout::left_to_right(egui::Align::Center),
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    child.set_clip_rect(child.clip_rect().intersect(rect));
    add(&mut child);
    ui.advance_cursor_after_rect(rect);
}
/// A slider row. `true` if the value changed.
///
/// **The layout divides by width.** With more than 240 du left for the track, the title, the track
/// and the value are laid along one line; with less, the track drops to a line below the title.
/// Always stacking two lines where one would do makes the card more than twice as thick as a switch
/// row — for one and the same item. Forcing one line on a narrow screen, conversely, shortens the
/// track until a finger cannot adjust it finely.
///
/// Whichever layout it is, the track's **hit height is `touch_target`**. What was reduced for density
/// is the empty padding above and below the track, not the width a finger touches (the glove
/// policy).
pub fn slider_row(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    enabled: bool,
) -> bool {
    card_divider(ui, cx);
    let m = cx.theme.metrics.row_height;
    let touch = cx.theme.metrics.touch_target;
    // The row's content inset, the same one `ListRow` uses. It was `screen_inset` (12 du) against
    // `ListRow`'s 16, which is the four-du gap that showed as five pixels between a slider row's
    // label and a nav row's in the same card.
    let inset = cx.theme.metrics.content_inset;
    let text = if enabled {
        ColorRole::OnSurface
    } else {
        ColorRole::Muted
    };
    let label = |cx: &Cx<'_>| {
        egui::RichText::new(title)
            .size(cx.theme.metrics.type_scale.body)
            .color(cx.theme.color(text))
    };
    let reading = |cx: &Cx<'_>, v: f32| {
        egui::RichText::new(format!("{}{suffix}", rounded(v)))
            .size(cx.theme.metrics.type_scale.body)
            .color(cx.theme.color(ColorRole::Primary))
    };

    let inner = (ui.available_width() - inset * 2.0).max(m);
    let gap = m * SLIDER_COL_GAP;
    let value_w = m * SLIDER_VALUE_WIDTH;
    let label_w = (inner * SLIDER_LABEL_FRACTION).min(m * SLIDER_LABEL_MAX);
    let track_w = inner - label_w - value_w - gap * 2.0;
    let mut changed = false;

    // ── The one-line layout ──
    if track_w >= SLIDER_INLINE_TRACK {
        // **One rect, placed like a `ListRow`'s.** This used to be `add_space` · `horizontal` ·
        // `add_space`, which leaves the row's height to egui - and egui inserts `item_spacing.y`
        // (11.8 du here) between each of those three, on top of the card's own margins. A `ListRow`
        // in the same card never asks: it allocates one rect and puts everything on
        // `rect.center().y`. The two disagreed by 6.5 px, measured, with the slider riding high.
        let row_h = touch + m * SLIDER_PAD * 2.0;
        let (band, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_h),
            egui::Sense::hover(),
        );
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_center_size(
                    band.center(),
                    egui::vec2(band.width(), touch),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        {
            let ui = &mut ui;
            // The column gap is already set aside by the arithmetic below. With egui's default spacing on
            // top, the three columns exceed the card's width and the value is pushed off to the right.
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(inset);
            column(ui, label_w, touch, egui::Align::Min, |ui| {
                ui.add(egui::Label::new(label(cx)).truncate());
            });
            ui.add_space(gap);
            column(ui, track_w, touch, egui::Align::Min, |ui| {
                changed = TouchSlider::new(value, range)
                    .enabled(enabled)
                    .show(ui, &mut cx.widgets())
                    .changed();
            });
            ui.add_space(gap);
            column(ui, value_w, touch, egui::Align::Max, |ui| {
                ui.label(reading(cx, *value));
            });
            ui.add_space(inset);
        }
        return changed;
    }

    // ── The two-line layout ──
    //
    // The same discipline as the one-line branch: this stack is bracketed by one pad on each side
    // and egui's `item_spacing.y` is taken out of the vertical, so the card's own margins are the
    // only thing deciding where the block sits.
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(m * SLIDER_PAD);
    ui.horizontal(|ui| {
        ui.add_space(inset);
        ui.label(label(cx));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(inset);
            ui.label(reading(cx, *value));
        });
    });
    // The title and the track are **one lump**; with `item_spacing.y` zeroed above there is nothing
    // to take back out between them, which is what the negative nudge here used to be for.
    ui.horizontal(|ui| {
        ui.add_space(inset);
        let width = (ui.available_width() - inset).max(m);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_size(
                    ui.cursor().min,
                    egui::vec2(width, touch),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        changed = TouchSlider::new(value, range)
            .enabled(enabled)
            .show(&mut child, &mut cx.widgets())
            .changed();
        ui.advance_cursor_after_rect(child.min_rect());
    });
    ui.add_space(m * SLIDER_PAD);
    changed
}
/// An exclusive choice list. When the selected item's index changes, it hands that index back.
///
/// It does not use a dropdown — a popup means aiming twice on touch, and a device screen usually has
/// the vertical room to lay the list out as it is.
pub fn choice_rows(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    options: &[(&str, Option<&str>)],
    selected: usize,
) -> Option<usize> {
    let mut picked = None;
    for (index, (label, subtitle)) in options.iter().enumerate() {
        card_divider(ui, cx);
        let mut row = ListRow::new(*label).chevron(false).separator(false);
        if let Some(subtitle) = subtitle {
            row = row.subtitle(*subtitle);
        }
        // **A radio on every row, not a tick on one.** A tick marks the chosen row and leaves the
        // others blank, so nothing on an unchosen row says it can be chosen at all; a radio group
        // is what a pick-one list is. The crate ships the control - this is the screen that was
        // still drawing its own mark instead of using it.
        row = row.trailing_radio(index == selected);
        if row.show(ui, &mut cx.widgets()).clicked() && index != selected {
            picked = Some(index);
        }
    }
    picked
}
/// The status card at the top of a screen. It gives what is going on at a glance.
///
/// A `tone` of `None` is a neutral card; given one, that role's colour is laid down faintly
/// (connected = `Primary`, failed = `Danger`).
pub fn status_card(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    detail: Option<&str>,
    tone: Option<ColorRole>,
) {
    let m = &cx.theme.metrics;
    let height = m.row_height * if detail.is_some() { 1.45 } else { 1.1 };
    // The title in body, the detail in small — type sizes are the scale's, never a share of the
    // row (a gloved row made the words half as big again). The two lines sit either side of the
    // card's centre, a line gap apart.
    let (title_size, detail_size) = (m.type_scale.body, m.type_scale.small);
    let line_gap = cx.theme.control.line_gap;
    // As in `slider_row`: a card's content starts where a row's content starts.
    let inset = m.content_inset;
    let radius = card_radius(m, &cx.theme.control);
    let gap = m.row_height * CARD_GAP;
    let (outer, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height + gap),
        egui::Sense::hover(),
    );
    let rect = egui::Rect::from_min_max(
        egui::pos2(outer.left() + inset, outer.top()),
        egui::pos2(outer.right() - inset, outer.top() + height),
    );
    // A neutral card is `SurfaceVariant` — in the dark palette `Surface` is nearly `Background` and the
    // card's boundary disappears (measured on Abyss).
    let base = cx.theme.color(ColorRole::SurfaceVariant);
    let fill = tone.map_or(base, |role| base.lerp_to_gamma(cx.theme.color(role), 0.22));
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(round_u8(radius)), fill);
    let title_y = if detail.is_some() {
        rect.center().y - f32::midpoint(detail_size, line_gap)
    } else {
        rect.center().y
    };
    painter.text(
        egui::pos2(rect.left() + inset, title_y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(title_size),
        cx.theme.color(ColorRole::OnSurface),
    );
    if let Some(detail) = detail {
        painter.text(
            egui::pos2(
                rect.left() + inset,
                rect.center().y + f32::midpoint(title_size, line_gap),
            ),
            egui::Align2::LEFT_CENTER,
            detail,
            egui::FontId::proportional(detail_size),
            cx.theme.color(ColorRole::Muted),
        );
    }
}
/// A **pressable** row with an icon. The icon's colour is a role, so it follows the palette.
///
/// A chevron is always attached — this row means "press it and something happens", and every caller
/// in the built-in settings screens looks at `.clicked()`. A read-only row is [`info_row`], and a row
/// that needs an icon but cannot be pressed can use [`ListRow`] directly — a chevron failing to say
/// "pressing here does nothing" is worse.
pub fn icon_row(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    icon: IconRef,
    title: &str,
    subtitle: Option<&str>,
    trailing: Option<&str>,
) -> Response {
    card_divider(ui, cx);
    let mut row = ListRow::new(title)
        .icon(icon)
        .icon_color(ColorRole::Primary)
        .separator(false);
    if let Some(subtitle) = subtitle {
        row = row.subtitle(subtitle);
    }
    if let Some(trailing) = trailing {
        row = row.trailing(trailing);
    }
    row.show(ui, &mut cx.widgets())
}
/// [`icon_row`] that times a long press as well — for a list where holding a row means something
/// (forgetting a saved network, a context menu). See [`ListRow::show_with_long_press`] for why the
/// row times its own press rather than asking egui.
pub fn icon_row_with_long_press(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    icon: IconRef,
    title: &str,
    subtitle: Option<&str>,
    trailing: Option<&str>,
) -> crate::widgets::RowPress {
    card_divider(ui, cx);
    let mut row = ListRow::new(title)
        .icon(icon)
        .icon_color(ColorRole::Primary)
        .separator(false);
    if let Some(subtitle) = subtitle {
        row = row.subtitle(subtitle);
    }
    if let Some(trailing) = trailing {
        row = row.trailing(trailing);
    }
    row.show_with_long_press(ui, &mut cx.widgets())
}
/// Which of the two marks a [`SelectMark`] draws: `(pill, bar)`.
pub(super) const fn mark_wants(mark: SelectMark) -> (bool, bool) {
    match mark {
        SelectMark::PillAndBar => (true, true),
        SelectMark::Pill => (true, false),
        SelectMark::Bar => (false, true),
        SelectMark::Tint => (false, false),
    }
}
pub(super) fn mark_geometry(cx: &Cx<'_>, rect: egui::Rect) -> MarkGeometry {
    let inset = cx.theme.metrics.screen_inset * 0.5;
    let bar_w = cx.theme.control.accent_bar;
    // **The bar sits beside the pill, not inside it.**
    //
    // It was inside, and then it had to dodge the pill's corner arc, which is what an earlier
    // version spent its arithmetic on and why a short row left it a stub floating in the fill.
    // Outside there is no arc to dodge: the bar is a mark in the leading gutter and the pill
    // begins after it, which is both what the console reference draws and the simpler shape to be
    // right about.
    //
    // The gutter is reserved whether or not a bar is drawn, so an unselected row, a `Pill` and a
    // `PillAndBar` all start their fill at the same x. Sizing the pill off whether it happens to
    // have a bar would shift the whole column sideways as the selection moved.
    let gutter = bar_w * BAR_GUTTER;
    let pill = egui::Rect::from_min_max(
        egui::pos2(rect.left() + inset + gutter, rect.top()),
        egui::pos2(rect.right() - inset, rect.bottom()),
    );
    // A share of the pill's height, so the mark cannot fade out as the row and the corner radius
    // move apart — a height taken off the radius once shrank to zero that way, and not depending
    // on the radius keeps that fixed.
    let half = pill.height() * 0.5 * BAR_SHARE;
    let bar = egui::Rect::from_center_size(
        egui::pos2(rect.left() + inset + bar_w * 0.5, rect.center().y),
        egui::vec2(bar_w, half * 2.0),
    );
    MarkGeometry {
        pill,
        pill_radius: cx.theme.metrics.control_radius.min(pill.height() * 0.5),
        bar,
        bar_w,
        inset,
    }
}
/// The selection mark's shapes for a row occupying `rect`, back to front.
///
/// Shapes rather than paint calls, because the caller does not know `rect` until the row has been
/// drawn: `ListRow` floors its own height against the touch target and against its text, so the
/// box it allocates is regularly taller than `metrics.row_height`. Painting the mark first, from
/// that token, is what put the pill 6 du above the label it was meant to be behind. The
/// caller reserves two slots, draws the row, then fills the slots in from the rect the row
/// actually took.
///
/// [`SelectMark::Tint`] returns neither: the icon and the label carry that one.
pub(super) fn selection_shapes(
    cx: &Cx<'_>,
    rect: egui::Rect,
    mark: SelectMark,
) -> (Option<egui::Shape>, Option<egui::Shape>) {
    let (pill_wanted, bar_wanted) = mark_wants(mark);
    let g = mark_geometry(cx, rect);
    // **Translucent, not a solid plate**. An opaque `SurfaceVariant` pill reads as a card
    // that happens to be under the words; the accent at `fill_alpha` reads as the row being *lit*,
    // which is what a console rail does and what leaves the label legible on top.
    let fill = pill_wanted.then(|| {
        egui::Shape::rect_filled(
            g.pill,
            egui::CornerRadius::same(round_u8(g.pill_radius)),
            cx.theme
                .color(ColorRole::Primary)
                .gamma_multiply(cx.theme.control.fill_alpha),
        )
    });
    let bar = bar_wanted.then(|| {
        egui::Shape::rect_filled(
            g.bar,
            egui::CornerRadius::same(round_u8(g.bar_w * 0.5)),
            cx.theme.color(ColorRole::Primary),
        )
    });
    (fill, bar)
}
/// The marks partway through a fold — `k` runs from 0 (open) to 1 (folded) — for a row occupying
/// `rect` whose icon is centred at `icon_at`. Back to front, like [`selection_shapes`].
///
/// The pill fades out. The bar **melts into a disc** behind the icon: its rect, its corner and its
/// ink all tween from the bar's to the disc's, so a solid line beside the words becomes a soft
/// tinted circle behind the glyph in one motion rather than one mark being swapped for another.
/// A `Pill` with no bar melts the pill instead; `Tint` has nothing to melt.
///
/// The disc is a step darker than the open pill (`DISC_TINT` × `fill_alpha`): folded, it is the
/// only thing saying which entry is live, and the pill's wash was sized to sit under a label.
pub(super) fn folding_marks(
    cx: &Cx<'_>,
    rect: egui::Rect,
    mark: SelectMark,
    k: f32,
    icon_at: egui::Pos2,
    band: (f32, f32),
) -> (Option<egui::Shape>, Option<egui::Shape>) {
    let (pill_wanted, bar_wanted) = mark_wants(mark);
    let g = mark_geometry(cx, rect);
    let primary = cx.theme.color(ColorRole::Primary);
    let fill_alpha = cx.theme.control.fill_alpha;
    let side = cx.theme.metrics.icon_size * 0.5;
    // As big as the band allows behind the icon, and never past the row's own inset from it.
    let room = (icon_at.x - band.0 - g.inset).min(band.1 - g.inset - icon_at.x) * 2.0;
    let d = (side * DISC_RATIO)
        .min(room)
        .min(rect.height() - 4.0)
        .max(0.0);
    let disc = egui::Rect::from_center_size(icon_at, egui::Vec2::splat(d));
    let disc_alpha = (fill_alpha * DISC_TINT).min(1.0);
    let pill = (pill_wanted && k < 1.0).then(|| {
        egui::Shape::rect_filled(
            g.pill,
            egui::CornerRadius::same(round_u8(g.pill_radius)),
            primary.gamma_multiply(fill_alpha * (1.0 - k)),
        )
    });
    let (from, from_radius, from_alpha) = if bar_wanted {
        (g.bar, g.bar_w * 0.5, 1.0)
    } else if pill_wanted {
        (g.pill, g.pill_radius, fill_alpha)
    } else {
        return (pill, None);
    };
    let melt = egui::Shape::rect_filled(
        from.lerp_towards(&disc, k),
        egui::CornerRadius::same(round_u8(egui::lerp(from_radius..=d * 0.5, k))),
        primary.gamma_multiply(egui::lerp(from_alpha..=disc_alpha, k)),
    );
    (pill, Some(melt))
}
/// A rail entry partway through a fold, folded (`k` = 1), or on an arm too narrow for words.
///
/// It draws rather than delegating to `ListRow` because a row lays its icon out against a *label
/// column* — and here the column is going: the icon slides from where the row had it to the
/// middle of the arm, the words fade over the first stretch of the fold, and the mark melts into
/// a disc behind the live icon ([`folding_marks`]). At `k` = 0 the icon and the words are exactly
/// where `ListRow` drew them, so the first frame of a fold is the row's own last frame.
pub(super) fn folding_item(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    mark: SelectMark,
    icon: &IconRef,
    title: &str,
    selected: bool,
    fold: FoldFrame,
) -> Response {
    let k = fold.k;
    let m = &cx.theme.metrics;
    let height = m.row_height.max(m.touch_target);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let side = m.icon_size * 0.5;
    // Where `ListRow` centres the icon, and where the folded band does; in between, the tween.
    // With no band to go by - a rail that is simply narrow - the row is the band.
    let band = fold.band.unwrap_or((rect.left(), rect.right()));
    let row_x = rect.left() + m.content_inset + side * 0.5;
    let fold_x = f32::midpoint(band.0, band.1);
    let icon_at = egui::pos2(egui::lerp(row_x..=fold_x, k), rect.center().y);
    // Painted out to the band's edges, not the row's clip. The rows of a rail sit in a scroll
    // area whose clip starts where the arm does, a screen inset in from the glass, while the
    // disc is centred in the band that runs from the glass - on a panel with large glyphs it
    // reaches into that inset and was cut there (user report). The clip keeps its top and
    // bottom: rows scrolled out of the view stay out of it.
    let clip = ui.clip_rect();
    // `set_clip_rect`, not `with_clip_rect`: the latter only ever narrows.
    let mut painter = ui.painter().clone();
    painter.set_clip_rect(egui::Rect::from_min_max(
        egui::pos2(band.0.min(clip.left()), clip.top()),
        egui::pos2(band.1.max(clip.right()), clip.bottom()),
    ));
    if selected {
        let (pill, melt) = folding_marks(cx, rect, mark, k, icon_at, band);
        for shape in [pill, melt].into_iter().flatten() {
            painter.add(shape);
        }
    }
    let role = if selected {
        ColorRole::Primary
    } else {
        ColorRole::Muted
    };
    let style = crate::icons::IconStyle::sized(side).color(crate::icons::IconColor::Role(role));
    cx.icons.paint(
        &painter,
        egui::Rect::from_center_size(icon_at, egui::Vec2::splat(side)),
        icon,
        &style,
        cx.theme,
    );
    // The words, where the row had them, fading over the first stretch of the fold.
    let fade = 1.0 - (k / LABEL_FADE).min(1.0);
    if fade > 0.0 {
        let x = rect.left() + m.content_inset + side + cx.theme.components.list_row.pad;
        let ink = cx
            .theme
            .color(if selected {
                ColorRole::Primary
            } else {
                ColorRole::OnSurface
            })
            .gamma_multiply(fade);
        painter.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            title,
            egui::TextStyle::Body.resolve(ui.style()),
            ink,
        );
    }
    response
}
/// One entry of a left/right split's list, marked with [`SelectMark`]'s default.
///
/// [`list_item_with`] picks a different mark.
pub fn list_item(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    icon: IconRef,
    title: &str,
    selected: bool,
) -> Response {
    list_item_with(ui, cx, SelectMark::default(), icon, title, selected)
}
/// One entry of a left/right split's list, marked the way `mark` says.
///
/// ```no_run
/// # fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
/// use fairing::{icon, layout::{self, SelectMark}};
/// // A dense list: the bar alone, so a column of rows does not read as stripes.
/// let _ = layout::list_item_with(ui, cx, SelectMark::Bar, icon::HOME, "Home", true);
/// # }
/// ```
pub fn list_item_with(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    mark: SelectMark,
    icon: IconRef,
    title: &str,
    selected: bool,
) -> Response {
    // **A folding arm draws the fold, and a narrow arm drops the label by itself.** A folded rail
    // is still a rail: the icons and the selection mark have to stay readable, and only the words
    // have anywhere to go. Reading the fold (`Rail::show` leaves it under `FOLD_KEY` while its
    // rail closure runs) and the width here, rather than taking a `collapsed` flag, means a rail
    // built out of `list_item` calls needs no branch in the caller — and a rail that is simply
    // narrow gets the folded drawing, which is the same thing seen from the other side.
    let fold = ui
        .ctx()
        .data(|d| d.get_temp::<FoldFrame>(egui::Id::unique(FOLD_KEY)));
    if let Some(f) = fold.filter(|f| f.k > 0.0) {
        return folding_item(ui, cx, mark, &icon, title, selected, f);
    }
    if ui.available_width() < cx.theme.metrics.icon_cell {
        let narrow = FoldFrame { k: 1.0, band: None };
        return folding_item(ui, cx, mark, &icon, title, selected, narrow);
    }
    // Two slots held open under the row, filled in once the row has told us how tall it is.
    let slots = selected.then(|| {
        [
            ui.painter().add(egui::Shape::Noop),
            ui.painter().add(egui::Shape::Noop),
        ]
    });
    let response = ListRow::new(title)
        .icon(icon)
        // **The selected row carries the accent; the rest are quiet.** A rail is a list of places,
        // and the one you are in is the only thing on it with a state — so it takes the colour and
        // the others take `Muted`, rather than every glyph being accented and the selection having
        // to find some other way to stand out.
        .icon_color(if selected {
            ColorRole::Primary
        } else {
            ColorRole::Muted
        })
        .title_role(if selected {
            ColorRole::Primary
        } else {
            ColorRole::OnSurface
        })
        // **No badge.** An earlier version put one behind every sidebar glyph, following One UI's
        // settings sidebar. An instrument rail is not that: its icons are bare line marks, and a
        // tinted disc behind each one competes with the pill that is already marking the selection.
        .icon_badge(false)
        // Nor bold. The accent says which row it is, and weight on top of colour says it twice.
        .strong(false)
        .chevron(false)
        .separator(false)
        .show(ui, &mut cx.widgets());
    if let Some([pill_slot, bar_slot]) = slots {
        let (pill, bar) = selection_shapes(cx, response.rect, mark);
        if let Some(pill) = pill {
            ui.painter().set(pill_slot, pill);
        }
        if let Some(bar) = bar {
            ui.painter().set(bar_slot, bar);
        }
    }
    response
}
