//! The overview — the recent screens as cards (A10).
//!
//! # A card is a title and an icon
//!
//! v1 draws no thumbnails (a shell that does not know its render backend has no snapshot
//! to give), so a card is the task's icon, its title, when it was last used and the level its
//! screen needs. Coming in, the screen on show shrinks into its card — the real content, scaled
//! whole by its layer's transform (A10) — and the card takes over from it at the end.
//!
//! # One drag, two meanings
//!
//! A drag across the cards scrolls them; one upward on a card throws it away (A10). Which it is
//! is decided by the drag's first few points and held to the end: a carousel that also closed
//! cards on a slanting flick would lose them by accident.
//!
//! # Drawn your way
//!
//! A card painter draws each card and a ground painter what the cards stand on
//! ([`RecentCardCx`], [`RecentsGroundCx`]). Everything else here is still the shell's: the deck,
//! the drag and its two meanings, the taps, the throw, the screen shrinking into its card, the
//! split buttons and "Close all".
//!
//! # Above the panes, below the chrome
//!
//! The cards are an `Area` at `Order::Middle`, over every screen layer and under the shade and the
//! prompt, so no screen raising its own layer can come up through them. The backdrop under the
//! shrinking screen is the desktop's layer, painted plain while the overview is up over a task.

use crate::i18n::Strings;
use crate::icons::{IconColor, IconRef, IconSet, IconStyle};
use crate::motion::{Animated, Mode as Motion, Spring, Tween};
use crate::screen::CxParts;
use crate::theme::{paint_elevation, ColorRole, Elevation, MotionTokens, OverviewMetrics, Theme};
use crate::workspace::painters::{paint_ground, RecentCardCx, RecentsOver, RecentsPainters};
use crate::workspace::InstanceId;
use egui::{CornerRadius, Pos2, Rect, Sense, Vec2};
use fairing_widgets::WidgetCx;
use std::time::Instant;

/// The cards' `Area`.
const AREA_ID: &str = "fairing.overview";
/// A10: a card's size over the content's — where that is no narrower than
/// `components.overview.card_min_width`.
pub(crate) const CARD_SCALE: f32 = 0.6;
/// A10: a card thrown this far up (du) goes — or faster up than `[motion] fling_px_s`.
const THROW_DISTANCE: f32 = 120.0;
/// Where in coming in the shrinking screen starts handing over to its card.
const HANDOVER: f32 = 0.7;
/// A thrown card's alpha at the top of its throw (A10: `1 → 0.3`).
const THROWN_ALPHA: f32 = 0.3;

/// What the overview says — looked up through [`Strings`] like the shade's own labels, so the
/// active language's table reaches them. `{n}` is replaced with the number.
pub(crate) mod labels {
    /// The button under the cards.
    pub(crate) const CLOSE_ALL: &str = "Close all";
    /// Over the cards, in the split control's picker.
    pub(crate) const PICK_HINT: &str = "Choose a screen for the other side";
    /// Where there are no cards.
    pub(crate) const EMPTY: &str = "No recent screens";
    /// A card's split button, for a screen reader.
    pub(crate) const SPLIT: &str = "Split";
    /// Used less than a minute ago.
    pub(crate) const JUST_NOW: &str = "Just now";
    /// Used `{n}` minutes ago.
    pub(crate) const MINUTES_AGO: &str = "{n} min ago";
    /// Used `{n}` hours ago.
    pub(crate) const HOURS_AGO: &str = "{n} h ago";
    /// Used `{n}` days ago.
    pub(crate) const DAYS_AGO: &str = "{n} d ago";
}

/// A task, as its card shows it.
#[derive(Debug, Clone)]
pub(crate) struct Card {
    /// The task's root instance — what the card is known by.
    pub(crate) key: InstanceId,
    pub(crate) title: String,
    pub(crate) icon: Option<IconRef>,
    pub(crate) last_active: Instant,
    /// The level its screen needs, where it needs more than everyone has.
    pub(crate) badge: Option<String>,
    /// Whether its split button shows — it can go beside the focused pane.
    pub(crate) beside: bool,
}

/// What the cards are for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The recent screens: a tap brings a task forward. `beside` gives the cards split buttons
    /// — a split may come up (`workspace.split`).
    Recents { beside: bool },
    /// The split control's picker: a tap puts the card's task beside the focused pane.
    Picker,
}

/// What the overview was asked this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Picked {
    /// A card tapped: its task forward.
    Resume(InstanceId),
    /// A card's split button, or a card tapped in picker mode: its task beside the focused pane.
    Beside(InstanceId),
    /// A card thrown away: its task ends.
    Close(InstanceId),
    /// "Close all".
    CloseAll,
    /// Back to what was on show — a tap past the cards.
    Dismiss,
}

/// A drag on the cards.
#[derive(Debug, Clone, Copy)]
enum Drag {
    /// Not yet far enough to say which way.
    Undecided { start: Pos2, scroll0: f32 },
    /// Across: the carousel follows.
    Scroll { start_x: f32, scroll0: f32 },
    /// Up on a card: it follows, to be thrown away or put back.
    Throw { start_y: f32 },
}

/// What ending a drag needs from the frame.
#[derive(Debug, Clone, Copy)]
struct DragEnd {
    /// The finger's last velocity (du/s).
    velocity: Vec2,
    /// The last card's place in the carousel.
    last: f32,
    /// From one card's centre to the next.
    pitch: f32,
    /// How far a thrown card travels to be out of sight.
    out: f32,
    spring: Spring,
    reduce: bool,
    /// The release speed that throws a card (`[motion] fling_px_s`, du/s).
    fling: f32,
    /// A thrown card's way out.
    thrown: Tween,
}

/// A card being thrown: its key, how far up it is (du) and whether it is on its way out.
#[derive(Debug, Clone, Copy)]
struct Throw {
    key: InstanceId,
    dy: Animated<f32>,
    going: bool,
}

/// What a tick finished.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Ticked {
    /// The way back is done: the overview is gone.
    pub(crate) returned: bool,
    /// This card is out of sight: its task ends now.
    pub(crate) thrown: Option<InstanceId>,
}

/// Where a card was drawn last frame, and its split button — for a test or a tour pressing them
/// the way a finger would.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawnCard {
    /// The task's root instance.
    pub key: InstanceId,
    /// The card.
    pub rect: Rect,
    /// Its split button, where it has one.
    pub beside: Option<Rect>,
}

/// The overview while it is up.
#[derive(Debug, Clone)]
pub(crate) struct Overview {
    /// 0 → 1 coming in, 1 → 0 on the way back.
    presence: Animated<f32>,
    returning: bool,
    /// The carousel's place, in cards: 0 is the first card centred.
    scroll: Animated<f32>,
    drag: Option<Drag>,
    throw: Option<Throw>,
    /// A card thrown away whose task is still to end — another throw took over before it was
    /// out of sight. The next tick hands it over.
    gone: Option<InstanceId>,
    /// What the cards are for.
    mode: Mode,
    /// The task on show when it opened — the card its screen shrinks into.
    current: Option<InstanceId>,
    /// Opened over the desktop: it paints its own backdrop over it.
    over_desktop: bool,
    /// The cards as last drawn.
    drawn: Vec<DrawnCard>,
    /// "Close all" as last drawn.
    close_all: Option<Rect>,
    /// Where the screen on show was when a lift was held into the overview: it comes
    /// from there into its card, rather than shrinking from where it stands at rest.
    carry: Option<Rect>,
}

/// The screen on show as it shrinks into its card: its scale about `pivot`, then moved by
/// `shift`, and its opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScreenLook {
    pub(crate) scale: f32,
    pub(crate) opacity: f32,
    pub(crate) pivot: Pos2,
    pub(crate) shift: Vec2,
}

impl Overview {
    /// Up: over the task `current` (or the desktop, `over_desktop`), over
    /// `[motion.overview] in_ms`. Under `reduce`, at once.
    pub(crate) fn open(
        current: Option<InstanceId>,
        mode: Mode,
        over_desktop: bool,
        tokens: &MotionTokens,
    ) -> Self {
        let mut presence = Animated::new(0.0);
        if tokens.reduce {
            presence.snap(1.0);
        } else {
            presence.to(1.0, tokens.overview_in);
        }
        Self {
            presence,
            returning: false,
            scroll: Animated::new(0.0),
            drag: None,
            throw: None,
            gone: None,
            mode,
            current,
            over_desktop,
            drawn: Vec::new(),
            close_all: None,
            carry: None,
        }
    }

    /// Come up from a lifted screen at `from`: the screen goes from there straight into
    /// its card as the cards come in.
    pub(crate) fn carry_from(&mut self, from: Rect) {
        self.carry = Some(from);
    }

    /// The cards as last drawn.
    pub(crate) fn drawn(&self) -> &[DrawnCard] {
        &self.drawn
    }

    /// "Close all" as last drawn.
    pub(crate) fn close_all_rect(&self) -> Option<Rect> {
        self.close_all
    }

    /// Back to what was on show: the cards go and the screen grows out of its card again. `true`
    /// where it is gone at once (`reduce`).
    pub(crate) fn go_back(&mut self, tokens: &MotionTokens) -> bool {
        let reduce = tokens.reduce;
        self.returning = true;
        // Going back, the screen grows out of its card to where it stands at rest.
        self.carry = None;
        // Nothing stays under a finger that is no longer heard: a card held mid-throw goes back
        // into the row (one already on its way out still ends its task), the carousel stops.
        self.drag = None;
        if self.throw.is_some_and(|t| !t.going) {
            self.throw = None;
        }
        if matches!(self.scroll.mode(), Motion::Dragging) {
            self.scroll.snap(self.scroll.value());
        }
        if reduce {
            self.presence.snap(0.0);
            return true;
        }
        self.presence.to(0.0, tokens.overview_in);
        false
    }

    /// The task on show when it opened.
    pub(crate) fn current(&self) -> Option<InstanceId> {
        self.current
    }

    /// Forget the task on show — it ended under the overview.
    pub(crate) fn forget_current(&mut self, key: InstanceId) {
        if self.current == Some(key) {
            self.current = None;
        }
    }

    pub(crate) fn is_picker(&self) -> bool {
        self.mode == Mode::Picker
    }

    /// Whether the cards carry split buttons.
    pub(crate) fn has_beside(&self) -> bool {
        self.mode == Mode::Recents { beside: true }
    }

    /// Give the cards their split buttons or take them away — the session changed under them.
    pub(crate) fn set_beside(&mut self, beside: bool) {
        if let Mode::Recents { .. } = self.mode {
            self.mode = Mode::Recents { beside };
        }
    }

    pub(crate) fn is_returning(&self) -> bool {
        self.returning
    }

    /// Whether it covers the content whole — up and still.
    pub(crate) fn is_full(&self) -> bool {
        !self.returning && !self.presence.is_animating()
    }

    /// Whether anything in it moves.
    pub(crate) fn is_moving(&self) -> bool {
        self.presence.is_animating()
            || self.scroll.is_animating()
            || self.throw.is_some_and(|t| t.dy.is_animating())
    }

    /// Whether it is coming in or going back — input waits for it (A10, as A2).
    pub(crate) fn is_tweening(&self) -> bool {
        self.presence.is_animating()
    }

    /// Where its cards go over `content` — room is kept under them for "Close all", which the
    /// picker does not have.
    pub(crate) fn deck(&self, content: Rect, theme: &Theme) -> Deck {
        let footer = (!self.is_picker()).then(|| close_all_height(theme));
        Deck::new(content, theme.components.overview, footer)
    }

    /// The screen on show, as it shrinks into its card (`deck`).
    pub(crate) fn screen_look(&self, deck: &Deck) -> ScreenLook {
        let p = self.presence.value();
        let opacity = 1.0 - handover(p);
        match self.carry {
            // From where a lift left it, straight into the first card: both rects have the
            // content's shape, so the one in between does too and the scale stays uniform.
            Some(from) => {
                let content = deck.content;
                let rect = from.lerp_towards(&deck.card(0, 0.0), p);
                ScreenLook {
                    scale: rect.width() / content.width().max(1.0),
                    opacity,
                    pivot: content.center(),
                    shift: rect.center() - content.center(),
                }
            }
            None => ScreenLook {
                scale: 1.0 + (deck.scale - 1.0) * p,
                opacity,
                pivot: deck.pivot(),
                shift: Vec2::ZERO,
            },
        }
    }

    /// A card on its way out whose task has not ended yet — taken, for whoever takes the overview
    /// down at once to end it: a throw is a decision, not an animation to lose.
    pub(crate) fn take_thrown(&mut self) -> Option<InstanceId> {
        let flying = self.throw.filter(|t| t.going).map(|t| t.key);
        if flying.is_some() {
            self.throw = None;
        }
        self.gone.take().or(flying)
    }

    /// Advance.
    pub(crate) fn tick(&mut self, dt: f32) -> Ticked {
        let mut ticked = Ticked {
            thrown: self.gone.take(),
            ..Ticked::default()
        };
        let _ = self.scroll.tick(dt);
        let was = self.presence.is_animating();
        if was && !self.presence.tick(dt) && self.returning {
            ticked.returned = true;
        }
        if let Some(throw) = self.throw.as_mut() {
            let moving = throw.dy.is_animating();
            if moving && !throw.dy.tick(dt) {
                if throw.going {
                    // One a tick: where an earlier throw is being handed over now, this one waits.
                    match ticked.thrown {
                        None => ticked.thrown = Some(throw.key),
                        Some(_) => self.gone = Some(throw.key),
                    }
                }
                self.throw = None;
            }
        }
        ticked
    }

    /// A card closed: the ones after it move up one, and the carousel stays on the card it was on.
    pub(crate) fn closed(&mut self, index: usize, left: usize) {
        let place = self.scroll.target();
        // Card counts are tiny, so the conversions are exact.
        #[allow(clippy::cast_precision_loss)]
        let (index, last) = (index as f32, left.saturating_sub(1) as f32);
        let place = if index < place { place - 1.0 } else { place };
        self.scroll.snap(place.clamp(0.0, last.max(0.0)));
    }

    /// Draw it and take its input. `cards` come in the order shown, the task on show first.
    #[allow(clippy::too_many_arguments)] // The frame's parts the cards are drawn with.
    #[allow(clippy::too_many_lines)] // The carousel's one drag, its taps and its two kinds of button.
    pub(crate) fn ui(
        &mut self,
        ctx: &egui::Context,
        parts: &mut CxParts<'_>,
        content: Rect,
        cards: &[Card],
        tokens: &MotionTokens,
        painters: &mut RecentsPainters,
    ) -> Option<Picked> {
        let (spring, reduce) = (tokens.spring, tokens.reduce);
        let theme = parts.theme;
        let p = self.presence.value();
        let live = self.is_full();
        // The other cards are in by `cards_in_ms`, a share of the way in.
        let cards_in = tokens.overview_cards_in.as_secs_f32()
            / tokens.overview_in.duration.as_secs_f32().max(f32::EPSILON);
        let others = (p / cards_in.clamp(f32::EPSILON, 1.0)).clamp(0.0, 1.0);
        let deck = self.deck(content, theme);
        let pitch = deck.pitch();
        // Card counts are tiny, so the conversions are exact.
        #[allow(clippy::cast_precision_loss)]
        let last = cards.len().saturating_sub(1) as f32;
        let id = egui::Id::new(AREA_ID);
        let now = Instant::now();
        let strings = parts.strings;
        let mut picked = None;
        self.drawn.clear();
        self.close_all = None;
        // Cards that went from under it (a task ended elsewhere — a lost gate, an eviction) leave
        // the carousel past its end: it comes back to the last card rather than show nothing.
        if self.drag.is_none() && self.scroll.target() > last {
            if reduce {
                self.scroll.snap(last);
            } else {
                self.scroll
                    .release_scaled(last, spring, 1.0 / pitch.max(1.0));
            }
        }
        let area = egui::Area::new(id)
            .order(egui::Order::Middle)
            .fixed_pos(content.min)
            .default_size(content.size())
            .constrain(false)
            .fade_in(false)
            .interactable(true);
        area.show(ctx, |ui| {
            ui.set_clip_rect(content);
            ui.set_min_size(content.size());
            let painter = ui.painter().clone();
            if self.over_desktop {
                let ground = painters.ground.as_mut();
                paint_ground(&painter, theme, ground, content, RecentsOver::Desktop, p);
            }
            // The surface takes what the buttons do not: drags, and taps on and past the cards.
            let sense = if live {
                Sense::click_and_drag()
            } else {
                Sense::hover()
            };
            let surface = ui.interact(content, id.with("surface"), sense);
            crate::drag::claim_if_held(&surface);
            let card_at = |at: Pos2, scroll: f32| {
                (0..cards.len())
                    .find(|&i| deck.card(i, scroll).contains(at))
                    .and_then(|i| cards.get(i))
                    .map(|card| card.key)
            };
            if live {
                let velocity = ctx.input(|i| i.pointer.velocity());
                if surface.drag_started() {
                    let start = ctx
                        .input(|i| i.pointer.press_origin())
                        .or_else(|| surface.interact_pointer_pos())
                        .unwrap_or(content.center());
                    self.drag = Some(Drag::Undecided {
                        start,
                        scroll0: self.scroll.value(),
                    });
                }
                if let (true, Some(at)) = (surface.dragged(), surface.interact_pointer_pos()) {
                    match self.drag {
                        Some(Drag::Undecided { start, scroll0 }) => {
                            let moved = at - start;
                            if moved.length() > tokens.slop_px {
                                let key = card_at(start, scroll0);
                                if moved.y < 0.0 && moved.y.abs() > moved.x.abs() && key.is_some() {
                                    // A card still flying from the last throw ends its task
                                    // now, rather than snap back into the row.
                                    if let Some(earlier) = self.throw.filter(|t| t.going) {
                                        self.gone = Some(earlier.key);
                                    }
                                    self.drag = Some(Drag::Throw { start_y: start.y });
                                    self.throw = key.map(|key| Throw {
                                        key,
                                        dy: Animated::new(0.0),
                                        going: false,
                                    });
                                } else {
                                    self.drag = Some(Drag::Scroll {
                                        start_x: start.x,
                                        scroll0,
                                    });
                                }
                            }
                        }
                        Some(Drag::Scroll { start_x, scroll0 }) => {
                            let place = scroll0 - (at.x - start_x) / pitch.max(1.0);
                            self.scroll
                                .drag(place.clamp(-0.3, last + 0.3), -velocity.x / pitch.max(1.0));
                        }
                        Some(Drag::Throw { start_y }) => {
                            if let Some(throw) = self.throw.as_mut() {
                                throw.dy.drag((at.y - start_y).min(0.0), velocity.y);
                            }
                        }
                        None => {}
                    }
                }
                // A drag ends on the lift — or, where the surface never heard the lift (it stopped
                // taking input under the finger), on the first frame with no finger down.
                let lifted = surface.drag_stopped()
                    || (self.drag.is_some() && !ctx.input(|i| i.pointer.any_down()));
                if lifted {
                    let end = DragEnd {
                        velocity,
                        last,
                        pitch,
                        out: content.height(),
                        spring,
                        reduce,
                        fling: tokens.fling_px_s,
                        thrown: tokens.overview_throw,
                    };
                    picked = picked.or(self.end_drag(end));
                }
                if surface.clicked() {
                    picked = Some(
                        match surface
                            .interact_pointer_pos()
                            .and_then(|at| card_at(at, self.scroll.value()))
                        {
                            Some(key) if self.is_picker() => Picked::Beside(key),
                            Some(key) => Picked::Resume(key),
                            None => Picked::Dismiss,
                        },
                    );
                }
            }
            // The cards.
            let scroll = self.scroll.value();
            for (index, card) in cards.iter().enumerate() {
                let mut rect = deck.card(index, scroll);
                if !rect.intersects(content) {
                    continue;
                }
                let mut alpha = if Some(card.key) == self.current {
                    handover(p)
                } else {
                    others
                };
                if let Some(throw) = self.throw.filter(|t| t.key == card.key) {
                    let dy = throw.dy.value();
                    rect = rect.translate(Vec2::new(0.0, dy));
                    let up = (-dy / rect.height().max(1.0)).clamp(0.0, 1.0);
                    alpha *= 1.0 + (THROWN_ALPHA - 1.0) * up;
                }
                if alpha <= 0.0 {
                    continue;
                }
                // A card's split button sits at the end of its header, whoever draws the card.
                let beside = (card.beside && live && self.throw.is_none_or(|t| t.key != card.key))
                    .then(|| split_button(header_rect(rect, theme), theme));
                // The title is a screen's title — a key, looked up as it is drawn.
                let title = strings.get(&card.title);
                let when = ago(card.last_active, now, strings);
                let level = card.badge.as_deref().map(|level| strings.get(level));
                if let Some(paint) = painters.card.as_mut() {
                    paint(
                        &painter,
                        &mut RecentCardCx {
                            rect,
                            corner: card_radius(theme),
                            title,
                            when: &when,
                            level,
                            icon: card.icon.as_ref(),
                            current: Some(card.key) == self.current,
                            split_button: beside,
                            alpha,
                            theme,
                            icons: &mut *parts.icons,
                        },
                    );
                } else {
                    let when_line = level.map_or_else(|| when.clone(), |l| format!("{when} · {l}"));
                    let text = (title, when_line.as_str());
                    paint_card(&painter, theme, parts.icons, rect, card, alpha, text);
                }
                let drawn = DrawnCard {
                    key: card.key,
                    rect,
                    beside,
                };
                if let Some(at) = beside {
                    let mut slot = ui.new_child(egui::UiBuilder::new().max_rect(at));
                    let mut wcx = widget_cx(parts);
                    let split = crate::widgets::IconButton::new(
                        crate::icons::builtin::SPLIT,
                        strings.get(labels::SPLIT),
                    )
                    .show(&mut slot, &mut wcx);
                    if split.clicked() {
                        picked = Some(Picked::Beside(card.key));
                    }
                }
                self.drawn.push(drawn);
            }
            // Under the cards: "Close all", or what there is to say.
            let band = deck.card(0, 0.0);
            let above = Rect::from_min_max(content.min, egui::pos2(content.max.x, band.min.y));
            let text_color = theme.color(ColorRole::Muted).gamma_multiply(others);
            let font = egui::FontId::proportional(theme.metrics.type_scale.body);
            if self.is_picker() {
                painter.text(
                    above.center(),
                    egui::Align2::CENTER_CENTER,
                    strings.get(labels::PICK_HINT),
                    font.clone(),
                    text_color,
                );
            }
            if cards.is_empty() {
                painter.text(
                    content.center(),
                    egui::Align2::CENTER_CENTER,
                    strings.get(labels::EMPTY),
                    font,
                    text_color,
                );
            } else if live && !self.is_picker() {
                // A button of the ordinary height, as wide as its label, a gap under the cards.
                let row = Rect::from_min_size(
                    egui::pos2(content.min.x, band.max.y + deck.gap),
                    Vec2::new(content.width(), close_all_height(theme)),
                );
                let mut slot = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(row)
                        .layout(egui::Layout::top_down(egui::Align::Center)),
                );
                let mut wcx = widget_cx(parts);
                let all = crate::widgets::BigButton::new(strings.get(labels::CLOSE_ALL))
                    .kind(crate::widgets::ButtonKind::Normal)
                    .show(&mut slot, &mut wcx);
                self.close_all = Some(all.response.rect);
                if all.clicked() {
                    picked = Some(Picked::CloseAll);
                }
            }
        });
        picked
    }

    /// The finger lifted from a drag: the carousel settles on the nearest card (a fling carries
    /// it on), and a card held up is thrown away past [`THROW_DISTANCE`] or faster than the fling
    /// speed, and put back otherwise.
    fn end_drag(&mut self, end: DragEnd) -> Option<Picked> {
        let DragEnd {
            velocity,
            last,
            pitch,
            out,
            spring,
            reduce,
            fling,
            thrown,
        } = end;
        match self.drag.take() {
            Some(Drag::Scroll { .. }) => {
                let v = -velocity.x / pitch.max(1.0);
                let target = (self.scroll.value() + v * 0.15).round().clamp(0.0, last);
                if reduce {
                    self.scroll.snap(target);
                } else {
                    self.scroll
                        .release_scaled(target, spring, 1.0 / pitch.max(1.0));
                }
                None
            }
            Some(Drag::Throw { .. }) => {
                let throw = self.throw.as_mut()?;
                let dy = throw.dy.value();
                if dy < -THROW_DISTANCE || velocity.y < -fling {
                    throw.going = true;
                    if reduce {
                        let key = throw.key;
                        self.throw = None;
                        return Some(Picked::Close(key));
                    }
                    throw.dy.to(-out, thrown);
                } else if reduce {
                    throw.dy.snap(0.0);
                } else {
                    throw.dy.release(0.0, spring);
                }
                None
            }
            Some(Drag::Undecided { .. }) | None => None,
        }
    }

    /// Whether the scroll is under a finger.
    #[cfg(test)]
    fn scrolling(&self) -> bool {
        matches!(self.scroll.mode(), crate::motion::Mode::Dragging)
    }
}

/// The widget context the overview's buttons draw with, from the frame's parts.
fn widget_cx<'p>(parts: &'p mut CxParts<'_>) -> WidgetCx<'p> {
    WidgetCx {
        theme: parts.theme,
        icons: &mut *parts.icons,
        anims: &mut *parts.animations,
        anim_scope: egui::Id::new(AREA_ID),
        frame: parts.frame,
        inset_bottom: 0.0,
        painters: Some(&mut *parts.widget_painters),
    }
}

/// How far the shrinking screen has handed over to its card, `0..=1`.
fn handover(p: f32) -> f32 {
    ((p - HANDOVER) / (1.0 - HANDOVER)).clamp(0.0, 1.0)
}

/// Where the cards go over `content`: one scale for all of them, the gap between two, and the
/// row's centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Deck {
    content: Rect,
    /// A card's size over the content's: [`CARD_SCALE`], or more where that would be narrower
    /// than `card_min_width` — never more than the content.
    scale: f32,
    gap: f32,
    /// The centred card's centre: the content's, or higher by half of what goes under the cards,
    /// so the cards and "Close all" sit in the middle together.
    center: Pos2,
}

impl Deck {
    /// `footer` is the height of what goes under the cards, a `card_gap` below them.
    pub(crate) fn new(content: Rect, metrics: OverviewMetrics, footer: Option<f32>) -> Self {
        let floor = metrics.card_min_width / content.width().max(1.0);
        let scale = floor.clamp(CARD_SCALE, 1.0);
        let gap = metrics.card_gap.max(0.0);
        let card_h = content.height() * scale;
        let below = footer.map_or(0.0, |h| gap + h.max(0.0));
        // As far as there is room for it: short of room, the cards go to the top.
        let spare = (content.height() - card_h - below).max(0.0);
        Self {
            content,
            scale,
            gap,
            center: egui::pos2(
                content.center().x,
                content.min.y + spare / 2.0 + card_h / 2.0,
            ),
        }
    }

    /// From one card's centre to the next.
    fn pitch(&self) -> f32 {
        (self.content.width() * self.scale + self.gap).max(1.0)
    }

    /// Card `index`'s rect with the carousel at `scroll`: the content's shape at the deck's scale.
    pub(crate) fn card(&self, index: usize, scroll: f32) -> Rect {
        // Card counts are tiny, so the conversion is exact.
        #[allow(clippy::cast_precision_loss)]
        let offset = (index as f32 - scroll) * self.pitch();
        Rect::from_center_size(
            self.center + Vec2::new(offset, 0.0),
            self.content.size() * self.scale,
        )
    }

    /// What the screen on show scales about to land exactly on the first card (A10): the one
    /// point that stays put while the content's centre travels to the card's.
    pub(crate) fn pivot(&self) -> Pos2 {
        let middle = self.content.center();
        let shrink = 1.0 - self.scale;
        if shrink < 1e-4 {
            return middle;
        }
        middle + (self.center - middle) / shrink
    }
}

/// "Close all"'s height: an ordinary button's.
fn close_all_height(theme: &Theme) -> f32 {
    theme.metrics.row_height.max(theme.metrics.touch_target)
}

/// A card's corners.
fn card_radius(theme: &Theme) -> CornerRadius {
    CornerRadius::same(crate::unit::round_u8(theme.metrics.corner_radius))
}

/// A card's header, inside its padding: the icon, the title over the when line, and the split
/// button at the end.
fn header_rect(rect: Rect, theme: &Theme) -> Rect {
    let pad = theme.metrics.content_inset;
    let lines = (theme.metrics.type_scale.body + theme.metrics.type_scale.small) * 1.3;
    Rect::from_min_size(
        rect.min + Vec2::splat(pad),
        Vec2::new(
            (rect.width() - pad * 2.0).max(1.0),
            lines.max(theme.control.icon),
        ),
    )
}

/// Where a card's split button goes: at the end of its header.
fn split_button(header: Rect, theme: &Theme) -> Rect {
    let side = theme.control.icon_button;
    Rect::from_min_size(
        egui::pos2(header.max.x - side, header.center().y - side / 2.0),
        Vec2::splat(side),
    )
}

/// One card: its face, the header (icon, title, when, level), and the task's icon large in the
/// middle. `text` is the title and the when line, worded.
fn paint_card(
    painter: &egui::Painter,
    theme: &Theme,
    icons: &mut IconSet,
    rect: Rect,
    card: &Card,
    alpha: f32,
    text: (&str, &str),
) {
    let (title, when) = text;
    let mut p = painter.clone();
    p.set_opacity(alpha);
    let radius = card_radius(theme);
    paint_elevation(&p, theme, rect, radius, Elevation::Floating);
    p.rect_filled(rect, radius, theme.color(ColorRole::Surface));
    let icon = theme.control.icon;
    let title_font = theme.strong(theme.metrics.type_scale.body);
    let small = egui::FontId::proportional(theme.metrics.type_scale.small);
    let header = header_rect(rect, theme);
    if let Some(glyph) = &card.icon {
        let at = Rect::from_min_size(
            egui::pos2(header.min.x, header.center().y - icon / 2.0),
            Vec2::splat(icon),
        );
        let style =
            IconStyle::sized(icon).color(IconColor::Fixed(theme.color(ColorRole::OnSurface)));
        icons.paint(&p, at, glyph, &style, theme);
    }
    let text_x = header.min.x + icon + theme.control.gap;
    let room = (header.max.x - theme.control.icon_button - text_x).max(1.0);
    let line = |text: &str, font: egui::FontId, color| {
        let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        p.layout_job(job)
    };
    let on = theme.color(ColorRole::OnSurface);
    let muted = theme.color(ColorRole::Muted);
    let title = line(title, title_font, on);
    let title_h = title.size().y;
    p.galley(egui::pos2(text_x, header.min.y), title, on);
    p.galley(
        egui::pos2(text_x, header.min.y + title_h),
        line(when, small, muted),
        muted,
    );
    if let Some(glyph) = &card.icon {
        let big = (rect.height() * 0.28).min(rect.width() * 0.28);
        let body = Rect::from_min_max(egui::pos2(rect.min.x, header.max.y), rect.max);
        let at = Rect::from_center_size(body.center(), Vec2::splat(big));
        let style = IconStyle::sized(big).color(IconColor::Fixed(muted));
        icons.paint(&p, at, glyph, &style, theme);
    }
}

/// When a card's task was last used, in words.
fn ago(since: Instant, now: Instant, strings: &Strings) -> String {
    let secs = now.saturating_duration_since(since).as_secs();
    let (label, n) = match secs {
        0..=59 => return strings.get(labels::JUST_NOW).to_owned(),
        60..=3599 => (labels::MINUTES_AGO, secs / 60),
        3600..=86_399 => (labels::HOURS_AGO, secs / 3600),
        _ => (labels::DAYS_AGO, secs / 86_400),
    };
    strings.get(label).replace("{n}", &n.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ago, handover, Deck, Mode, Overview, ScreenLook, CARD_SCALE};
    use crate::i18n::Strings;
    use crate::theme::OverviewMetrics;
    use crate::workspace::scale_about;
    use egui::{pos2, vec2, Rect};
    use std::time::{Duration, Instant};

    const METRICS: OverviewMetrics = OverviewMetrics {
        card_gap: 24.0,
        card_min_width: 144.0,
    };

    #[test]
    fn the_first_card_is_the_content_scaled_about_its_centre() {
        let content = Rect::from_min_size(pos2(0.0, 32.0), vec2(1000.0, 500.0));
        let deck = Deck::new(content, METRICS, None);
        let card = deck.card(0, 0.0);
        assert!((card.width() - 1000.0 * CARD_SCALE).abs() < 0.01);
        assert!((card.center() - content.center()).length() < 0.01);
        let next = deck.card(1, 0.0);
        assert!(
            (next.min.x - card.max.x - 24.0).abs() < 0.01,
            "the next card a gap beside it"
        );
        let scrolled = deck.card(1, 1.0);
        assert!((scrolled.center() - content.center()).length() < 0.01);
    }

    /// On a panel too narrow for `0.6 ×`, a card keeps its floor — at the content's shape,
    /// so the screen shrinking into it still lands on it.
    #[test]
    fn a_small_panel_keeps_the_card_floor() {
        let content = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 150.0));
        let deck = Deck::new(content, METRICS, None);
        let card = deck.card(0, 0.0);
        assert!((card.width() - 144.0).abs() < 0.01, "{card:?}");
        assert!((card.aspect_ratio() - content.aspect_ratio()).abs() < 0.001);
        let tiny = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 80.0));
        let whole = Deck::new(tiny, METRICS, Some(48.0));
        assert!(
            (whole.card(0, 0.0).width() - 100.0).abs() < 0.01,
            "never wider than the content"
        );
        assert!((whole.pivot() - tiny.center()).length() < 0.01);
    }

    /// With "Close all" under them, the cards rise so the two sit in the middle together — and the
    /// screen on show, scaled about the deck's pivot, still lands exactly on the first card.
    #[test]
    fn the_cards_make_room_for_close_all_and_the_screen_still_lands() {
        let content = Rect::from_min_size(pos2(0.0, 44.0), vec2(1024.0, 490.0));
        let deck = Deck::new(content, METRICS, Some(88.0));
        let card = deck.card(0, 0.0);
        let top = card.min.y - content.min.y;
        let bottom = content.max.y - (card.max.y + 24.0 + 88.0);
        assert!(
            (top - bottom).abs() < 0.01 && top > 0.0,
            "{top} above, {bottom} below"
        );
        let landed = scale_about(deck.pivot(), CARD_SCALE).mul_rect(content);
        assert!(
            (landed.min - card.min).length() < 0.01,
            "{landed:?} vs {card:?}"
        );
        assert!((landed.max - card.max).length() < 0.01);
    }

    #[test]
    fn the_screen_hands_over_to_its_card_at_the_end() {
        assert!(handover(0.5) <= 0.0);
        assert!((handover(1.0) - 1.0).abs() < f32::EPSILON);
        let content = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 500.0));
        let deck = Deck::new(content, METRICS, Some(88.0));
        let tokens = crate::theme::MotionTokens::default();
        let mut overview = Overview::open(None, Mode::Recents { beside: true }, false, &tokens);
        assert!(
            (overview.screen_look(&deck).scale - 1.0).abs() < 0.01,
            "starts whole"
        );
        for _ in 0..30 {
            let _ = overview.tick(0.016);
        }
        let look = overview.screen_look(&deck);
        assert!((look.scale - CARD_SCALE).abs() < 0.01 && look.opacity <= 0.0);
        assert!(overview.is_full() && !overview.scrolling());
        assert!(!overview.go_back(&tokens));
        let mut back = false;
        for _ in 0..30 {
            back |= overview.tick(0.016).returned;
        }
        assert!(back, "the way back reports its end");
    }

    /// Carried on from a lift: the screen starts where the lift left it — not whole —
    /// and lands on its card all the same, with no pivot trick in between.
    #[test]
    fn a_carried_screen_goes_from_the_lift_into_its_card() {
        let content = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 500.0));
        let deck = Deck::new(content, METRICS, Some(88.0));
        let tokens = crate::theme::MotionTokens::default();
        let mut overview = Overview::open(None, Mode::Recents { beside: true }, false, &tokens);
        let lifted = Rect::from_min_size(pos2(150.0, 40.0), content.size() * 0.8);
        overview.carry_from(lifted);
        let at = |look: ScreenLook| {
            let min = look.pivot + (content.min - look.pivot) * look.scale + look.shift;
            Rect::from_min_size(min, content.size() * look.scale)
        };
        let start = at(overview.screen_look(&deck));
        assert!(
            (start.min - lifted.min).length() < 0.01 && (start.max - lifted.max).length() < 0.01,
            "{start:?}"
        );
        for _ in 0..30 {
            let _ = overview.tick(0.016);
        }
        let landed = at(overview.screen_look(&deck));
        let card = deck.card(0, 0.0);
        assert!((landed.min - card.min).length() < 0.01 && (landed.max - card.max).length() < 0.01);
        // Going back, the screen grows from its card to where it stands at rest.
        let _ = overview.go_back(&tokens);
        assert!(overview.carry.is_none());
    }

    #[test]
    fn time_reads_in_words() {
        let strings = Strings::new("en");
        let then = Instant::now();
        assert_eq!(ago(then, then, &strings), "Just now");
        let at = |secs| ago(then, then + Duration::from_secs(secs), &strings);
        assert_eq!(at(150), "2 min ago");
        assert_eq!(at(7200), "2 h ago");
        assert_eq!(at(3 * 86_400 + 5), "3 d ago");
    }
}
