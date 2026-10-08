//! The unlock prompt and the lock screen (A9) — the shell's own modal, drawn at
//! frame stage 12 over everything but the heads-up and the toasts.
//!
//! # What it draws
//!
//! Only what the authenticator offers
//! ([`Authenticator::methods`](super::Authenticator::methods)): the [`PinPad`] for a PIN, the
//! [`PatternPad`] for a pattern, a user and a secret field for a password — the on-screen
//! keyboard rises **above** the prompt for them — and a waiting card for an external method. A
//! refusal is shown in the authenticator's own words; the shell never words one.
//!
//! # One column, no stray parts
//!
//! The top line is what to do — "Enter PIN" — and what just happened takes its place: the
//! refusal in red, the lockout's countdown, "Checking…". A first version kept a separate message
//! line under the keys, empty nearly all the time, and the way out sat below that gap as a lone
//! button that seemed to belong to nothing. Now the way out (Cancel, or the lock screen's
//! Continue) is one more row under the body, as wide as the keys and a key's gap below them.
//!
//! # What it keeps
//!
//! What is being typed, while it is typed, and nothing after: a submit moves the buffer into the
//! [`Credential`] by value, and closing clears whatever was left. A pattern is the one exception,
//! and only for as long as it is on the glass anyway: the path stays on the dots until the answer
//! comes, red if it is a refusal, and goes with the next stroke or the next tab.
//!
//! # Waiting for an answer
//!
//! A check that takes time ([`AuthOutcome::Pending`](super::AuthOutcome::Pending)) puts up one
//! flag, and the title says "Checking…" while it is up. The keys are not held for it: the shell
//! sets no time limit of its own, and a keypad held over an answer that never comes would be a
//! lock screen nobody gets past. A new attempt takes the flag down with
//! [`Authenticator::cancel`](super::Authenticator::cancel) and goes in fresh. The flag only goes
//! down with an answer or with `cancel` — everything here runs on the UI thread, so there is
//! nothing to race and nothing to time out.
//!
//! # Drawn your way
//!
//! A lock screen painter draws the lock screen's ground, clock and date; an unlock prompt painter
//! draws the prompt's backdrop and its card. The way in is still drawn here, over what they drew,
//! and everything above is still the shell's: what is typed, the answer, the lockout, the shake,
//! the motion. See [`LockScreenCx`] and [`UnlockPromptCx`].
//!
//! # Motion (A9)
//!
//! - In: the backdrop over 160 ms, the card fading in while it scales `0.98 → 1` over 200 ms
//!   (`CubicOut`); out, 140 ms the other way. The lock screen fades in over 200 ms and leaves
//!   fading out while it grows to 1.04.
//! - A wrong PIN shakes the card — `8 du × sin`, three cycles dying away over 320 ms — with the
//!   keys deaf meanwhile, and the dots clear when it stops. A wrong pattern shakes it the same
//!   way with the path turned red, and the next stroke may start at once: it clears the red path
//!   itself.
//! - A lockout greys the keypad and counts down. It does not move.
//! - `motion.reduce` makes all of it instant.
//!
//! The times live here rather than in `[motion]`, like the press pop's: nothing
//! else in the shell runs on them.

// UI geometry: small counts and pixel values crossing to f32. The loss is meaningless in this
// range (the workspace's pedantic lints stay on for everything else).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::{
    AuthMethod, Credential, Gate, LockScreenCx, LockScreenPainter, PromptPiece, UnlockPromptCx,
    UnlockPromptPainter,
};
use crate::i18n::{LabelKey, Strings};
use crate::screen::LaunchAction;
use crate::theme::{control_height, paint_elevation, ColorRole, Elevation, Theme};
use crate::widgets::{
    BigButton, ButtonKind, PatternPad, PinPad, ProgressRing, SegmentedControl, TextField,
};
use egui::emath::TSTransform;
use egui::{Align, Color32, CornerRadius, Layout, Pos2, Rect, Sense, UiBuilder, Vec2};
use fairing_widgets::WidgetCx;
use std::hash::BuildHasher;
use std::time::{Duration, Instant};

/// The backdrop `Area`. The card is a second `Area` above it, so it can be scaled and shaken by a
/// layer transform without laying anything out again.
const BACKDROP_ID: &str = "fairing.prompt";
/// The card `Area` (the unlock prompt only — the lock screen is one layer).
const CARD_ID: &str = "fairing.prompt.card";

/// The gate an unlock from the lock screen reports.
pub(crate) const LOCK_GATE: Gate = Gate::borrowed("session.lock");

const BACKDROP_IN: Duration = Duration::from_millis(160);
const CARD_IN: Duration = Duration::from_millis(200);
const CARD_OUT: Duration = Duration::from_millis(140);
const LOCK_IN: Duration = Duration::from_millis(200);
const LOCK_OUT: Duration = Duration::from_millis(200);
const SHAKE: Duration = Duration::from_millis(320);
/// How far the shake swings, in du.
const SHAKE_DU: f32 = 8.0;
const SHAKE_CYCLES: f32 = 3.0;
/// The card's scale as it comes in.
const CARD_SCALE_FROM: f32 = 0.98;
/// The lock screen's scale as it leaves.
const LOCK_SCALE_TO: f32 = 1.04;
/// The card's content width, in touch targets.
const CARD_TARGETS: f32 = 7.0;
/// A text line's height over its type size.
const LINE: f32 = 1.35;
/// The lock screen's clock over the heading size.
const CLOCK_SCALE: f32 = 3.0;
/// From this aspect up the lock screen puts the clock beside the keypad rather than above it.
const SIDE_BY_SIDE: f32 = 1.3;

/// What the modal is up for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Purpose {
    /// A gate failed: unlock, then run `then` (through the gate again).
    Unlock {
        /// The gate.
        gate: Gate,
        /// What to run once unlocked.
        then: Option<LaunchAction>,
    },
    /// The panel is locked.
    Lock,
}

/// What the prompt says itself — looked up through [`Strings`] like the shade's labels, so the
/// active language's table reaches them. What the authenticator says (a refusal, a
/// method's label, a hint) is looked up too. `{n}` is replaced with the number.
pub(crate) mod labels {
    /// The keypad's title.
    pub(crate) const ENTER_PIN: &str = "Enter PIN";
    /// The dots' title.
    pub(crate) const DRAW_PATTERN: &str = "Draw pattern";
    /// The password card's title.
    pub(crate) const SIGN_IN: &str = "Sign in";
    /// The tabs.
    pub(crate) const TAB_PIN: &str = "PIN";
    pub(crate) const TAB_PATTERN: &str = "Pattern";
    pub(crate) const TAB_PASSWORD: &str = "Password";
    /// The unlock prompt's way out.
    pub(crate) const CANCEL: &str = "Cancel";
    /// The lock screen's way out, where allowed.
    pub(crate) const CONTINUE: &str = "Continue";
    /// Waiting for the authenticator's answer.
    pub(crate) const CHECKING: &str = "Checking…";
    /// A pattern too short to submit.
    pub(crate) const MIN_DOTS: &str = "Connect at least {n} dots";
    /// The lockout's countdown after the authenticator's words.
    pub(crate) const SECONDS_LEFT: &str = "{n} s";
    /// The password card's fields and button.
    pub(crate) const USER: &str = "User";
    pub(crate) const PASSWORD: &str = "Password";
    pub(crate) const UNLOCK: &str = "Unlock";
    /// A grant at or below the level the session already has — it opens nothing.
    pub(crate) const NOT_ENOUGH: &str = "That is not enough for this";
}

/// A keyboard-wedge reader types a whole badge in a burst: a pause longer than this starts a new
/// read, so half a read that lost its end does not spoil the next badge.
const READ_GAP: Duration = Duration::from_millis(800);

/// How often the badge wait's ring is redrawn — it can stand all night on a lock screen.
const WAIT_REPAINT: Duration = Duration::from_millis(250);

/// What the modal asks the shell to do.
#[derive(Debug)]
pub(crate) enum PromptAction {
    /// Hand this to the authenticator.
    Submit(Credential),
    /// Close without an answer (the unlock prompt's Cancel, or back).
    Cancel,
    /// Leave the lock screen as the starting subject (`[access.lock_screen] allow_continue`).
    Continue,
}

/// What the modal draws with.
pub(crate) struct PromptCx<'a> {
    /// The widget context — the theme, the icons, the animation store.
    pub(crate) widgets: WidgetCx<'a>,
    /// The shell's time.
    pub(crate) now: Instant,
    /// The time and the date, for the lock screen.
    pub(crate) clock: (String, String),
    /// Whether the lock screen offers Continue.
    pub(crate) allow_continue: bool,
    /// The words, through the string table.
    pub(crate) strings: &'a Strings,
}

/// How a message reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    /// The authenticator said no.
    Refused,
    /// The authenticator locked the keypad.
    Locked,
    /// The pattern pad's own: the path was too short to submit, and nothing was spent on it.
    Short,
}

/// The modal while it is up.
struct Open {
    purpose: Purpose,
    methods: Vec<AuthMethod>,
    /// The method on show.
    method: usize,
    pin: String,
    /// The pattern on the dots, from 0 — see "What it keeps".
    pattern: Vec<u8>,
    user: String,
    secret: String,
    /// What a keyboard-wedge reader typed while an external method is on show.
    wedge: String,
    /// When the reader last typed — a pause starts a new read.
    wedge_at: Option<Instant>,
    /// The dots a shake shows — the digits themselves went with the submit.
    shaken_digits: usize,
    order: [u8; 10],
    hint: Option<LabelKey>,
    message: Option<(LabelKey, Tone)>,
    locked_until: Option<Instant>,
    /// An answer is awaited (`AuthOutcome::Pending`) — the one flag a wait is. It goes down with
    /// the answer, or with `Authenticator::cancel`; nothing else holds a wait.
    awaiting: bool,
    shake_from: Option<Instant>,
    opened_at: Instant,
    /// Until the first frame has done its once-only work: drop a screen's text focus, focus the
    /// first field.
    fresh: bool,
    /// Where each digit's key was drawn last frame, on the screen (a tour or a test finds them
    /// here).
    digit_rects: [Option<Rect>; 10],
    /// Where each pattern dot was drawn last frame, on the screen.
    dot_centers: Vec<Pos2>,
    /// Where the method tabs were drawn last frame, on the screen.
    tabs: Option<(Rect, usize)>,
    /// Everything on the panel but the method's body, measured as drawn last frame — what the
    /// body has to leave room for (see [`panel`]).
    chrome: Option<f32>,
}

impl Open {
    /// Forget everything typed.
    fn clear(&mut self) {
        self.pin.clear();
        self.pattern.clear();
        self.user.clear();
        self.secret.clear();
        self.wedge.clear();
        self.wedge_at = None;
        self.shaken_digits = 0;
    }

    fn locked(&self, now: Instant) -> bool {
        self.locked_until.is_some_and(|until| now < until)
    }

    fn shaking(&self) -> bool {
        self.shake_from.is_some()
    }
}

/// The modal leaving.
struct Closing {
    open: Open,
    at: Instant,
}

/// The integrator's painters (rung 5): the lock screen's ground and clock, and the unlock
/// prompt's backdrop and card. The way in is drawn over either.
#[derive(Default)]
pub(crate) struct Painters {
    lock: Option<LockScreenPainter>,
    unlock: Option<UnlockPromptPainter>,
}

/// The unlock prompt and the lock screen.
#[derive(Default)]
pub(crate) struct Prompt {
    open: Option<Open>,
    closing: Option<Closing>,
    painters: Painters,
    /// How many times it has opened — the shuffle seed's input.
    opened: u64,
    /// A lockout the authenticator set, and what it said — kept when the prompt closes, so that
    /// reopening it shows the countdown rather than a keypad that only refuses.
    lockout: Option<(Instant, LabelKey)>,
    /// The methods last checked for what the pads cannot draw, so a misfit is logged once and
    /// not at every lock.
    checked: Vec<AuthMethod>,
}

impl std::fmt::Debug for Prompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prompt")
            .field("purpose", &self.open.as_ref().map(|o| &o.purpose))
            .field("closing", &self.closing.is_some())
            .field("lock_screen_painter", &self.painters.lock.is_some())
            .field("unlock_prompt_painter", &self.painters.unlock.is_some())
            .finish_non_exhaustive()
    }
}

/// `0..=1` through a span that started at `from`; instant under `reduce`.
fn progress(now: Instant, from: Instant, span: Duration, reduce: bool) -> f32 {
    if reduce || span.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(from).as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0)
}

fn cubic_out(t: f32) -> f32 {
    egui::emath::easing::cubic_out(t.clamp(0.0, 1.0))
}

/// `color` at `alpha` of its own alpha (`Color32` is premultiplied, so unpack first).
fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (f32::from(a) * alpha.clamp(0.0, 1.0)) as u8)
}

/// A digit order for a method, fresh per opening when it asks for one.
fn order_for(methods: &[AuthMethod], opened: u64) -> [u8; 10] {
    let shuffle = methods
        .iter()
        .any(|m| matches!(m, AuthMethod::Pin { shuffle: true, .. }));
    if shuffle {
        // `RandomState` is keyed from the OS per process; what this guards against is a smudge
        // pattern, not a prediction (see `PinPad::shuffled`).
        PinPad::shuffled(std::collections::hash_map::RandomState::new().hash_one(opened))
    } else {
        crate::widgets::PHONE_ORDER
    }
}

impl Prompt {
    /// Draw the lock screen's ground, clock and date with `painter`.
    pub(crate) fn set_lock_screen_painter(&mut self, painter: LockScreenPainter) {
        self.painters.lock = Some(painter);
    }

    /// Draw the unlock prompt's backdrop and card with `painter`.
    pub(crate) fn set_unlock_prompt_painter(&mut self, painter: UnlockPromptPainter) {
        self.painters.unlock = Some(painter);
    }

    /// Whether it is up (and taking every input).
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Whether anything is on screen — up, or still leaving.
    pub(crate) fn is_drawn(&self) -> bool {
        self.open.is_some() || self.closing.is_some()
    }

    /// Whether the lock screen is on screen (up or leaving), and so needs the clock.
    pub(crate) fn wants_clock(&self) -> bool {
        let purpose = self
            .open
            .as_ref()
            .map(|o| &o.purpose)
            .or_else(|| self.closing.as_ref().map(|c| &c.open.purpose));
        matches!(purpose, Some(Purpose::Lock))
    }

    /// Where the key for `digit` was drawn last frame, while the keypad is up.
    pub(crate) fn digit_rect(&self, digit: u8) -> Option<Rect> {
        self.open
            .as_ref()?
            .digit_rects
            .get(usize::from(digit))
            .copied()
            .flatten()
    }

    /// Where pattern dot `dot` (from 0) was drawn last frame, while the pattern is up.
    pub(crate) fn dot_center(&self, dot: u8) -> Option<Pos2> {
        self.open
            .as_ref()?
            .dot_centers
            .get(usize::from(dot))
            .copied()
    }

    /// Where the tab for method `index` was drawn last frame, while there are tabs. The strip's
    /// cells are equal (see `SegmentedControl`), so this is its share of the strip.
    pub(crate) fn tab_rect(&self, index: usize) -> Option<Rect> {
        let (strip, count) = self.open.as_ref()?.tabs?;
        if index >= count {
            return None;
        }
        let cell = strip.width() / count as f32;
        Some(Rect::from_min_size(
            egui::pos2(strip.min.x + cell * index as f32, strip.min.y),
            Vec2::new(cell, strip.height()),
        ))
    }

    /// Whether what is up is the lock screen.
    pub(crate) fn is_lock_screen(&self) -> bool {
        matches!(self.open.as_ref().map(|o| &o.purpose), Some(Purpose::Lock))
    }

    /// The gate the answer is for — `session.lock` on the lock screen.
    pub(crate) fn gate(&self) -> Option<Gate> {
        self.open.as_ref().map(|o| match &o.purpose {
            Purpose::Unlock { gate, .. } => gate.clone(),
            Purpose::Lock => LOCK_GATE,
        })
    }

    /// Open it. A second unlock request over an open prompt takes over what the prompt unlocks
    /// for and keeps what was typed; the lock screen is never replaced by an unlock prompt (the
    /// shell does not ask).
    pub(crate) fn open(
        &mut self,
        purpose: Purpose,
        methods: Vec<AuthMethod>,
        hint: Option<LabelKey>,
        now: Instant,
    ) {
        if let Some(open) = self.open.as_mut() {
            if matches!(open.purpose, Purpose::Unlock { .. })
                && matches!(purpose, Purpose::Unlock { .. })
            {
                open.purpose = purpose;
                open.hint = hint;
                // A lockout's countdown stays: the keys are still dead under it.
                if !matches!(open.message, Some((_, Tone::Locked))) {
                    open.message = None;
                }
                return;
            }
        }
        if self.checked != methods {
            for line in methods.iter().flat_map(AuthMethod::unhonoured) {
                log::warn!("unlock prompt: {line}");
            }
            self.checked.clone_from(&methods);
        }
        self.opened = self.opened.wrapping_add(1);
        let order = order_for(&methods, self.opened);
        self.closing = None;
        // Still locked out from before: the new prompt opens on the countdown.
        let lockout = self.lockout.clone().filter(|(until, _)| now < *until);
        self.open = Some(Open {
            purpose,
            methods,
            method: 0,
            pin: String::new(),
            pattern: Vec::new(),
            user: String::new(),
            secret: String::new(),
            wedge: String::new(),
            wedge_at: None,
            shaken_digits: 0,
            order,
            hint,
            message: lockout.clone().map(|(_, text)| (text, Tone::Locked)),
            locked_until: lockout.map(|(until, _)| until),
            awaiting: false,
            shake_from: None,
            opened_at: now,
            fresh: true,
            digit_rects: [None; 10],
            dot_centers: Vec::new(),
            tabs: None,
            chrome: None,
        });
    }

    /// Close it, keeping nothing typed. Returns what it was up for.
    pub(crate) fn close(&mut self, now: Instant) -> Option<Purpose> {
        let mut open = self.open.take()?;
        open.clear();
        let purpose = open.purpose.clone();
        self.closing = Some(Closing { open, at: now });
        Some(purpose)
    }

    /// The authenticator refused: say so, shake, and clear the dots once the shake is over (a
    /// pattern stays, red, until the next stroke).
    pub(crate) fn denied(&mut self, message: LabelKey, now: Instant) {
        if let Some(open) = self.open.as_mut() {
            open.message = Some((message, Tone::Refused));
            open.awaiting = false;
            open.shake_from = Some(now);
        }
    }

    /// A lockout set by an authenticator that is gone — the next one has locked nothing.
    pub(crate) fn forget_lockout(&mut self) {
        self.lockout = None;
    }

    /// The authenticator locked the keypad until `until`.
    pub(crate) fn locked(&mut self, until: Instant, message: LabelKey) {
        self.lockout = Some((until, message.clone()));
        if let Some(open) = self.open.as_mut() {
            open.message = Some((message, Tone::Locked));
            open.locked_until = Some(until);
            open.awaiting = false;
            open.shake_from = None;
            open.clear();
        }
    }

    /// The authenticator is still deciding: the flag goes up.
    pub(crate) fn wait(&mut self) {
        if let Some(open) = self.open.as_mut() {
            open.awaiting = true;
            open.message = None;
        }
    }

    /// Take the flag down without an answer, saying whether it was up — the caller then owes the
    /// authenticator its `cancel`.
    pub(crate) fn take_wait(&mut self) -> bool {
        self.open
            .as_mut()
            .is_some_and(|open| std::mem::take(&mut open.awaiting))
    }

    /// Whether anything is moving: coming in, going out, shaking.
    pub(crate) fn is_animating(&self, now: Instant, reduce: bool) -> bool {
        if reduce {
            return self.closing.is_some()
                || self.open.as_ref().is_some_and(|o| o.shaking() || o.fresh);
        }
        let open = self.open.as_ref().is_some_and(|o| {
            o.shaking() || o.fresh || now.saturating_duration_since(o.opened_at) < CARD_IN
        });
        open || self.closing.is_some()
    }

    /// The next moment the prompt changes with nobody touching it: the lockout countdown's next
    /// second, and the end of the lockout.
    pub(crate) fn next_deadline(&self, now: Instant) -> Option<Instant> {
        let until = self.open.as_ref()?.locked_until?;
        let left = until.checked_duration_since(now)?;
        let shown = left.as_secs_f32().ceil().max(1.0) as u64;
        until.checked_sub(Duration::from_secs(shown - 1))
    }

    /// Frame stage 12. `screen` is the whole screen; `bottom` is where the room for the card ends
    /// (the top of the on-screen keyboard, while it is up).
    pub(crate) fn ui(
        &mut self,
        ctx: &egui::Context,
        screen: Rect,
        bottom: f32,
        cx: &mut PromptCx<'_>,
    ) -> Option<PromptAction> {
        let now = cx.now;
        let reduce = cx.widgets.theme.motion.reduce;
        if let Some(open) = self.open.as_mut() {
            if open.fresh {
                // A screen's field that had the focus would keep the keyboard up and keep typing
                // into itself under the modal.
                if let Some(id) = ctx.memory(egui::Memory::focused) {
                    ctx.memory_mut(|m| m.surrender_focus(id));
                }
            }
            if let Some(from) = open.shake_from {
                if progress(now, from, SHAKE, reduce) >= 1.0 {
                    open.shake_from = None;
                    open.shaken_digits = 0;
                }
            }
            if open.locked_until.is_some_and(|until| now >= until) {
                open.locked_until = None;
                open.message = None;
            }
            let look = Look::coming(open, now, reduce);
            let action = draw(ctx, screen, bottom, cx, open, look, &mut self.painters);
            open.fresh = false;
            return action;
        }
        let closing = self.closing.as_mut()?;
        let span = match closing.open.purpose {
            Purpose::Lock => LOCK_OUT,
            Purpose::Unlock { .. } => CARD_OUT,
        };
        let t = progress(now, closing.at, span, reduce);
        if t >= 1.0 {
            self.closing = None;
            return None;
        }
        let look = Look::going(&closing.open, t);
        let _ = draw(
            ctx,
            screen,
            bottom,
            cx,
            &mut closing.open,
            look,
            &mut self.painters,
        );
        None
    }
}

/// How present the modal is this frame.
#[derive(Debug, Clone, Copy)]
struct Look {
    /// The backdrop's share of its full alpha.
    backdrop: f32,
    /// The card's (or the lock screen's) alpha.
    alpha: f32,
    /// The card's (or the lock screen's) scale about its centre.
    scale: f32,
    /// The shake, in points.
    shake: f32,
    /// Whether it takes input — not while it leaves.
    live: bool,
}

impl Look {
    fn coming(open: &Open, now: Instant, reduce: bool) -> Self {
        let shake = open.shake_from.map_or(0.0, |from| {
            let t = progress(now, from, SHAKE, reduce);
            let swing = (t * SHAKE_CYCLES * std::f32::consts::TAU).sin();
            SHAKE_DU * swing * (1.0 - t)
        });
        match open.purpose {
            Purpose::Unlock { .. } => {
                let card = cubic_out(progress(now, open.opened_at, CARD_IN, reduce));
                Self {
                    backdrop: progress(now, open.opened_at, BACKDROP_IN, reduce),
                    alpha: card,
                    scale: CARD_SCALE_FROM + (1.0 - CARD_SCALE_FROM) * card,
                    shake,
                    live: true,
                }
            }
            Purpose::Lock => {
                let t = cubic_out(progress(now, open.opened_at, LOCK_IN, reduce));
                Self {
                    backdrop: t,
                    alpha: t,
                    scale: 1.0,
                    shake,
                    live: true,
                }
            }
        }
    }

    fn going(open: &Open, t: f32) -> Self {
        let left = 1.0 - cubic_out(t);
        match open.purpose {
            Purpose::Unlock { .. } => Self {
                backdrop: left,
                alpha: left,
                scale: CARD_SCALE_FROM + (1.0 - CARD_SCALE_FROM) * left,
                shake: 0.0,
                live: false,
            },
            Purpose::Lock => Self {
                backdrop: left,
                alpha: left,
                scale: 1.0 + (LOCK_SCALE_TO - 1.0) * cubic_out(t),
                shake: 0.0,
                live: false,
            },
        }
    }
}

/// A scale about `centre`, then a sideways nudge.
fn transform(centre: egui::Pos2, scale: f32, shake: f32) -> TSTransform {
    TSTransform::from_translation(centre.to_vec2() + Vec2::new(shake, 0.0))
        * TSTransform::from_scaling(scale)
        * TSTransform::from_translation(-centre.to_vec2())
}

/// The modal, whichever it is.
fn draw(
    ctx: &egui::Context,
    screen: Rect,
    bottom: f32,
    cx: &mut PromptCx<'_>,
    open: &mut Open,
    look: Look,
    painters: &mut Painters,
) -> Option<PromptAction> {
    let theme = cx.widgets.theme;
    let lock_screen = matches!(open.purpose, Purpose::Lock);
    let target = control_height(&theme.metrics, &theme.control);
    let room = Rect::from_min_max(
        screen.min,
        egui::pos2(
            screen.max.x,
            bottom.clamp(screen.min.y + target * 4.0, screen.max.y),
        ),
    )
    .shrink(theme.metrics.screen_inset);
    // The lock screen puts the clock beside the way in on a wide panel, above it on a tall one.
    let (clock_room, panel_room) = if !lock_screen {
        (Rect::NOTHING, room)
    } else if room.width() > room.height() * SIDE_BY_SIDE {
        let split = room.min.x + room.width() * 0.45;
        (
            Rect::from_min_max(room.min, egui::pos2(split, room.max.y)),
            Rect::from_min_max(egui::pos2(split, room.min.y), room.max),
        )
    } else {
        let split = room.min.y + room.height() * 0.28;
        (
            Rect::from_min_max(room.min, egui::pos2(room.max.x, split)),
            Rect::from_min_max(egui::pos2(room.min.x, split), room.max),
        )
    };
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new(BACKDROP_ID));
    // **On top of its order every frame.** The shade pins its own layers with `move_to_top` each
    // pass, and egui keeps every area it has seen in its order — once is not enough (see the OSK).
    ctx.move_to_top(layer);
    let grow = if lock_screen { look.scale } else { 1.0 };
    ctx.set_transform_layer(layer, transform(screen.center(), grow, 0.0));
    egui::Area::new(egui::Id::new(BACKDROP_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .default_size(screen.size())
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            // Everything below is behind glass: the backdrop takes every press that misses the
            // panel, and does nothing with it — a PIN is deliberate, and so is leaving.
            let sense = if look.live {
                Sense::click()
            } else {
                Sense::hover()
            };
            let _ = ui.allocate_rect(screen, sense);
            ground(
                ui,
                cx,
                painters,
                (screen, clock_room, panel_room),
                look,
                lock_screen,
            );
        });
    // `panel` draws a card for the unlock prompt only: the lock screen's way in stands on its
    // ground.
    let card_painter = painters.unlock.as_mut();
    panel(ctx, cx, open, (screen, panel_room), look, card_painter)
}

/// What the backdrop layer draws under the way in: the lock screen's ground, clock and date, or the
/// unlock prompt's scrim — a painter's where one is given, the built-in ones where not.
/// `rooms` is the screen, the clock's room and the way in's.
///
/// A painter is handed the layer at full opacity and told the fade, as every painter is.
fn ground(
    ui: &mut egui::Ui,
    cx: &mut PromptCx<'_>,
    painters: &mut Painters,
    rooms: (Rect, Rect, Rect),
    look: Look,
    lock_screen: bool,
) {
    let theme = cx.widgets.theme;
    let (screen, clock_room, panel_room) = rooms;
    match (lock_screen, painters) {
        (
            true,
            Painters {
                lock: Some(paint), ..
            },
        ) => {
            let (time, date) = &cx.clock;
            paint(
                ui.painter(),
                &mut LockScreenCx {
                    screen,
                    clock: clock_room,
                    room: panel_room,
                    time,
                    date,
                    alpha: look.alpha,
                    leaving: !look.live,
                    theme,
                    icons: &mut *cx.widgets.icons,
                },
            );
        }
        (true, _) => {
            let fill = with_alpha(theme.color(ColorRole::Background), look.backdrop);
            ui.painter().rect_filled(screen, 0.0, fill);
            ui.set_opacity(look.alpha);
            clock(ui, cx, clock_room);
        }
        (
            false,
            Painters {
                unlock: Some(paint),
                ..
            },
        ) => paint(
            ui.painter(),
            &mut UnlockPromptCx {
                piece: PromptPiece::Backdrop,
                rect: screen,
                corner: CornerRadius::ZERO,
                alpha: look.backdrop,
                leaving: !look.live,
                theme,
            },
        ),
        (false, _) => {
            let fill = with_alpha(theme.color(ColorRole::Scrim), look.backdrop);
            ui.painter().rect_filled(screen, 0.0, fill);
        }
    }
}

/// The lock screen's clock and date, centred in `room`.
fn clock(ui: &egui::Ui, cx: &PromptCx<'_>, room: Rect) {
    let theme = cx.widgets.theme;
    let gap = theme.control.gap;
    let (time, date) = &cx.clock;
    let clock_font = theme.strong(theme.metrics.type_scale.heading * CLOCK_SCALE);
    let date_font = egui::FontId::proportional(theme.metrics.type_scale.body);
    let time =
        ui.painter()
            .layout_no_wrap(time.clone(), clock_font, theme.color(ColorRole::OnSurface));
    let date = ui
        .painter()
        .layout_no_wrap(date.clone(), date_font, theme.color(ColorRole::Muted));
    let block = time.size().y + gap + date.size().y;
    let top = room.center().y - block / 2.0;
    let time_at = egui::pos2(room.center().x - time.size().x / 2.0, top);
    let date_at = egui::pos2(
        room.center().x - date.size().x / 2.0,
        top + time.size().y + gap,
    );
    ui.painter()
        .galley(time_at, time, theme.color(ColorRole::OnSurface));
    ui.painter()
        .galley(date_at, date, theme.color(ColorRole::Muted));
}

/// The way in — the unlock prompt's card, or the lock screen's column — in a layer of its own
/// above the backdrop, so it can be scaled and shaken without being laid out again.
///
/// **It is sized from what it drew last frame.** Everything on it but the method's body (the
/// hint, the top line, the tabs, the way out) is measured after it is drawn, and the body gets
/// the height that leaves — the keypad's keys shrink to fit it. Estimating those
/// heights from the type sizes came out short on a panel whose touch target had grown, and the
/// card ran off both ends of the screen.
///
/// Where even the smallest keys do not fit, the panel is **shrunk whole** rather than cut: every
/// key and the way out stay on the glass, smaller than a finger would like.
fn panel(
    ctx: &egui::Context,
    cx: &mut PromptCx<'_>,
    open: &mut Open,
    (screen, room): (Rect, Rect),
    look: Look,
    card_painter: Option<&mut UnlockPromptPainter>,
) -> Option<PromptAction> {
    let theme = cx.widgets.theme;
    let target = control_height(&theme.metrics, &theme.control);
    let gap = theme.control.gap;
    let framed = matches!(open.purpose, Purpose::Unlock { .. });
    let pad = if framed { theme.metrics.card_pad } else { 0.0 };
    let width = (target * CARD_TARGETS)
        .min(room.width() - pad * 2.0)
        .max(target * 3.0);
    let chrome = open
        .chrome
        .unwrap_or_else(|| chrome_estimate(open, theme, cx.allow_continue));
    let body_room = Vec2::new(width, (room.height() - pad * 2.0 - chrome).max(0.0));
    let body = body_size(open, &cx.widgets, body_room);
    let size = Vec2::new(width + pad * 2.0, pad * 2.0 + chrome + body.y);
    let rect = Rect::from_center_size(room.center(), size);
    let fit = (room.height() / size.y.max(1.0)).min(1.0);

    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new(CARD_ID));
    ctx.move_to_top(layer);
    // The card comes in at 0.98; the lock screen's column leaves at 1.04 with its clock.
    let to_screen = transform(rect.center(), look.scale * fit, look.shake);
    ctx.set_transform_layer(layer, to_screen);
    // An area clips to its constrain rect **in its own space**, and the layer transform scales
    // that clip along with everything else — shrunk to fit, the panel was cut off at a clip that
    // had shrunk with it. The clip is the screen taken back through the transform.
    let clip = to_screen.inverse() * screen;
    egui::Area::new(egui::Id::new(CARD_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        // `constrain_to` turns constraining on as well, so it comes first and `constrain(false)`
        // after it: the clip without the area being pushed around.
        .constrain_to(clip)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_opacity(look.alpha);
            if framed {
                card(ui.painter(), theme, card_painter, rect, look);
            }
            let mut inner = ui.new_child(
                UiBuilder::new()
                    .max_rect(rect.shrink(pad))
                    .layout(Layout::top_down(Align::Center)),
            );
            if !look.live {
                inner.disable();
            }
            inner.spacing_mut().item_spacing = Vec2::splat(gap);
            if framed {
                if let Some(hint) = open.hint.clone() {
                    let hint = cx.strings.get(&hint).to_owned();
                    hint_line(&mut inner, cx, &hint, width);
                }
            }
            let (line, role) = headline(open, cx.now, cx.strings);
            text_line(
                &mut inner,
                &line,
                theme.strong(theme.metrics.type_scale.heading),
                theme.color(role),
                width,
            );
            let mut action = method_column(&mut inner, cx, open, body, look.live);
            // The way out is one more row under the body, as wide as it is — part of the column,
            // not a button left floating under the card.
            let exit = if framed {
                Some((labels::CANCEL, PromptAction::Cancel))
            } else if cx.allow_continue {
                Some((labels::CONTINUE, PromptAction::Continue))
            } else {
                None
            };
            if let Some((label, then)) = exit {
                let row = BigButton::new(cx.strings.get(label))
                    .kind(ButtonKind::Normal)
                    .min_size(Vec2::new(body.x.clamp(target * 3.0, width), target))
                    .enabled(look.live)
                    .show(&mut inner, &mut cx.widgets);
                if row.clicked() {
                    action = Some(then);
                }
            }
            open.chrome = Some((inner.min_rect().height() - body.y).max(0.0));
            action
        })
        .inner
}

/// The unlock prompt's card under the way in: a painter's where one is given, the
/// built-in floating plate in the surface colour where not. `faded` is the card layer's painter, at
/// the card's fade.
fn card(
    faded: &egui::Painter,
    theme: &Theme,
    card_painter: Option<&mut UnlockPromptPainter>,
    rect: Rect,
    look: Look,
) {
    let radius = CornerRadius::same(crate::unit::round_u8(
        theme.metrics.corner_radius * theme.control.card_radius_ratio,
    ));
    let Some(paint) = card_painter else {
        paint_elevation(faded, theme, rect, radius, Elevation::Floating);
        faded.rect_filled(rect, radius, theme.color(ColorRole::Surface));
        return;
    };
    // Told the fade rather than faded by it: a painter multiplies its colours by `alpha`, as every
    // painter does.
    let mut full = faded.clone();
    full.set_opacity(1.0);
    paint(
        &full,
        &mut UnlockPromptCx {
            piece: PromptPiece::Card,
            rect,
            corner: radius,
            alpha: look.alpha,
            leaving: !look.live,
            theme,
        },
    );
}

/// A first guess at everything but the body, for the one frame before it has been measured.
fn chrome_estimate(open: &Open, theme: &Theme, allow_continue: bool) -> f32 {
    let target = control_height(&theme.metrics, &theme.control);
    let gap = theme.control.gap;
    let sizes = &theme.metrics.type_scale;
    let framed = matches!(open.purpose, Purpose::Unlock { .. });
    let mut chrome = sizes.heading * LINE + gap;
    if open.methods.len() > 1 {
        chrome += target + gap;
    }
    if framed && open.hint.is_some() {
        chrome += sizes.small * LINE + gap;
    }
    if framed || allow_continue {
        chrome += target + gap;
    }
    chrome
}

/// The top line: what to do — or, in its place, so that nothing under it moves, what just
/// happened.
fn headline(open: &Open, now: Instant, strings: &Strings) -> (String, ColorRole) {
    match &open.message {
        Some((text, Tone::Locked)) => {
            let left = open
                .locked_until
                .and_then(|until| until.checked_duration_since(now))
                .map_or(0, |d| d.as_secs_f32().ceil() as u64);
            let seconds = strings
                .get(labels::SECONDS_LEFT)
                .replace("{n}", &left.to_string());
            (
                format!("{} · {seconds}", strings.get(text)),
                ColorRole::Warning,
            )
        }
        Some((text, Tone::Refused)) => (strings.get(text).to_owned(), ColorRole::Danger),
        Some((text, Tone::Short)) => (text.clone(), ColorRole::Warning),
        None if open.awaiting => (strings.get(labels::CHECKING).to_owned(), ColorRole::Muted),
        None => (title_for(open, strings), ColorRole::OnSurface),
    }
}

/// What the top line says for the method on show.
fn title_for(open: &Open, strings: &Strings) -> String {
    let key = match open.methods.get(open.method) {
        Some(AuthMethod::Pin { .. }) => labels::ENTER_PIN,
        Some(AuthMethod::Pattern { .. }) => labels::DRAW_PATTERN,
        Some(AuthMethod::Password { .. }) => labels::SIGN_IN,
        Some(AuthMethod::External { label }) => label,
        None => "",
    };
    strings.get(key).to_owned()
}

/// What a method's tab says.
fn tab_label(method: &AuthMethod, strings: &Strings) -> String {
    let key = match method {
        AuthMethod::Pin { .. } => labels::TAB_PIN,
        AuthMethod::Pattern { .. } => labels::TAB_PATTERN,
        AuthMethod::Password { .. } => labels::TAB_PASSWORD,
        AuthMethod::External { label } => label,
    };
    strings.get(key).to_owned()
}

/// **The body's column** in `room`: as tall as the tallest method needs, and as wide as the
/// widest is drawn in that height — the same whichever tab is on show, so switching moves
/// nothing: not the card, not its scale to fit, not the way out under it.
fn body_size(open: &Open, cx: &WidgetCx<'_>, room: Vec2) -> Vec2 {
    let height = open
        .methods
        .iter()
        .map(|m| method_size(m, cx, room).y)
        .fold(0.0, f32::max);
    let column = Vec2::new(room.x, height);
    let width = open
        .methods
        .iter()
        .map(|m| method_size(m, cx, column).x)
        .fold(0.0, f32::max);
    Vec2::new(width.min(room.x), height)
}

/// One method's body in `room`.
fn method_size(method: &AuthMethod, cx: &WidgetCx<'_>, room: Vec2) -> Vec2 {
    let theme = cx.theme;
    let target = control_height(&theme.metrics, &theme.control);
    let gap = theme.control.gap;
    match method {
        AuthMethod::Pin { .. } => PinPad::measure(cx, room),
        AuthMethod::Pattern { grid, .. } => PatternPad::measure(cx, room, *grid),
        AuthMethod::Password { needs_user } => {
            let fields = if *needs_user { 2.0 } else { 1.0 };
            Vec2::new(room.x, (target + gap) * fields + target)
        }
        AuthMethod::External { .. } => Vec2::new(room.x, target * 2.0 + gap),
    }
}

/// `size`, centred across `ui` at its cursor. The caller moves the cursor past it.
fn centred(ui: &egui::Ui, size: Vec2) -> Rect {
    Rect::from_min_size(
        egui::pos2(ui.max_rect().center().x - size.x / 2.0, ui.cursor().min.y),
        size,
    )
}

/// The tabs and the method's body, in a column `body` across ([`body_size`]) — what the card and
/// the lock screen share. A body smaller than the column (the dots beside a keypad) sits in its
/// middle.
fn method_column(
    ui: &mut egui::Ui,
    cx: &mut PromptCx<'_>,
    open: &mut Open,
    body: Vec2,
    live: bool,
) -> Option<PromptAction> {
    if open.methods.len() > 1 {
        let names: Vec<String> = open
            .methods
            .iter()
            .map(|m| tab_label(m, cx.strings))
            .collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        // The strip draws inside a margin it keeps for its focus ring; the slot is that much wider
        // so the strip's edges line up with the keys' under it.
        let control = &cx.widgets.theme.control;
        let across = body.x + 2.0 * (control.focus_gap + control.stroke_mark);
        let strip = centred(ui, Vec2::new(across, 0.0));
        let mut tabs = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    strip.min,
                    Vec2::new(across, f32::INFINITY),
                ))
                .layout(Layout::top_down(Align::Center)),
        );
        let pick = SegmentedControl::new(&refs, open.method)
            .enabled(live)
            .show(&mut tabs, &mut cx.widgets);
        let _ = ui.allocate_exact_size(Vec2::new(across, tabs.min_rect().height()), Sense::hover());
        open.tabs = Some((to_screen(ui) * pick.response.rect, names.len()));
        if let Some(picked) = pick.picked {
            open.method = picked;
            // A lockout's countdown stays whichever tab is on show.
            if !matches!(open.message, Some((_, Tone::Locked))) {
                open.message = None;
            }
            // A path left on the dots belongs to the tab it was drawn on — red or not, it would
            // come back without its message; half a badge read belongs to no tab.
            open.pattern.clear();
            open.wedge.clear();
            open.wedge_at = None;
            open.fresh = true;
        }
    }
    let now = cx.now;
    let mut action = None;
    let own = open
        .methods
        .get(open.method)
        .map_or(Vec2::ZERO, |m| method_size(m, &cx.widgets, body))
        .min(body);
    let mut slot = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_center_size(centred(ui, body).center(), own))
            .layout(Layout::top_down(Align::Center)),
    );
    slot.spacing_mut().item_spacing = ui.spacing().item_spacing;
    match open.methods.get(open.method).cloned() {
        Some(AuthMethod::Pin { len, max_len, .. }) => {
            action = pin_body(&mut slot, cx, open, (len, max_len), live, now);
        }
        Some(AuthMethod::Pattern {
            grid,
            min_points,
            show_path,
        }) => {
            action = pattern_body(
                &mut slot,
                cx,
                open,
                (grid, min_points, show_path),
                live,
                now,
            );
        }
        Some(AuthMethod::Password { needs_user }) => {
            action = password_body(&mut slot, cx, open, needs_user, live, now);
        }
        Some(AuthMethod::External { label }) => {
            action = external_body(&mut slot, cx, open, &label, live, now);
        }
        None => {}
    }
    let _ = ui.allocate_exact_size(body, Sense::hover());
    action
}

/// Where this `ui`'s layer puts a point on the screen — the card is scaled and shaken by a layer
/// transform, and a tour or a test presses the screen.
fn to_screen(ui: &egui::Ui) -> TSTransform {
    ui.ctx()
        .layer_transform_to_global(ui.layer_id())
        .unwrap_or(TSTransform::IDENTITY)
}

/// The keypad. `length` is the method's `(len, max_len)`.
fn pin_body(
    ui: &mut egui::Ui,
    cx: &mut PromptCx<'_>,
    open: &mut Open,
    length: (u8, u8),
    live: bool,
    now: Instant,
) -> Option<PromptAction> {
    let (len, max_len) = length;
    let locked = open.locked(now);
    if open.shaking() {
        // The digits went with the submit; the shake shows as many dots, and the keys are deaf
        // until it stops (A9).
        let mut ghost = "0".repeat(open.shaken_digits);
        let drawn = PinPad::new(&mut ghost)
            .len(len)
            .max_len(max_len)
            .order(open.order)
            .keyboard(false)
            .show(ui, &mut cx.widgets);
        // Deaf, and seen to be: a cover over the keys takes the presses, so none of them dips as
        // if it had been heard.
        let _ = ui.interact(
            drawn.response.rect,
            ui.id().with("shake-cover"),
            Sense::click_and_drag(),
        );
        return None;
    }
    // Live while an answer is awaited: a new PIN calls the old check off (see "Waiting for an
    // answer").
    let pad = PinPad::new(&mut open.pin)
        .len(len)
        .max_len(max_len)
        .order(open.order)
        .enabled(live && !locked)
        .keyboard(live)
        .show(ui, &mut cx.widgets);
    let screen = to_screen(ui);
    for (digit, slot) in (0u8..).zip(open.digit_rects.iter_mut()) {
        *slot = pad.digit_rect(digit).map(|rect| screen * rect);
    }
    if pad.changed && !open.pin.is_empty() {
        open.message = None;
    }
    if pad.submitted {
        open.shaken_digits = open.pin.len();
        return Some(PromptAction::Submit(Credential::Pin(std::mem::take(
            &mut open.pin,
        ))));
    }
    None
}

/// The dots. `method` is the method's `(grid, min_points, show_path)`.
///
/// Unlike the keypad it is not deaf through a refusal's shake: the refused path stays on the
/// dots in red until the next stroke, and the next stroke clears it itself — there is nothing
/// for a stroke started early to lose.
fn pattern_body(
    ui: &mut egui::Ui,
    cx: &mut PromptCx<'_>,
    open: &mut Open,
    method: (u8, u8, bool),
    live: bool,
    now: Instant,
) -> Option<PromptAction> {
    let (grid, min_points, show_path) = method;
    let locked = open.locked(now);
    let wrong = matches!(open.message, Some((_, Tone::Refused | Tone::Short)));
    let pad = PatternPad::new(&mut open.pattern)
        .grid(grid)
        .min_points(min_points)
        .show_path(show_path)
        .enabled(live && !locked)
        .mark(wrong.then_some(ColorRole::Danger))
        .show(ui, &mut cx.widgets);
    let screen = to_screen(ui);
    open.dot_centers = (0..grid.saturating_mul(grid))
        .map_while(|dot| pad.dot_center(dot))
        .map(|centre| screen * centre)
        .collect();
    if pad.changed && !open.pattern.is_empty() {
        // A new stroke: whatever was said about the last one is done with.
        open.message = None;
    }
    if pad.too_short {
        let text = cx
            .strings
            .get(labels::MIN_DOTS)
            .replace("{n}", &min_points.to_string());
        open.message = Some((text, Tone::Short));
    }
    if pad.submitted {
        // The authenticator gets its own copy; this one stays on the glass until the answer.
        return Some(PromptAction::Submit(Credential::Pattern(
            open.pattern.clone(),
        )));
    }
    None
}

/// A user field (where asked for), the secret, and Unlock. Enter in the secret submits too.
fn password_body(
    ui: &mut egui::Ui,
    cx: &mut PromptCx<'_>,
    open: &mut Open,
    needs_user: bool,
    live: bool,
    now: Instant,
) -> Option<PromptAction> {
    let strings = cx.strings;
    let mut first = None;
    if needs_user {
        let user = TextField::new(&mut open.user)
            .hint(strings.get(labels::USER))
            .show(ui, &mut cx.widgets);
        first = Some(user);
    }
    let secret = TextField::new(&mut open.secret)
        .password(true)
        .hint(strings.get(labels::PASSWORD))
        .show(ui, &mut cx.widgets);
    if open.fresh && live {
        first.as_ref().unwrap_or(&secret).request_focus();
    }
    // A lockout holds here as on the keypad: Unlock and Enter wait it out. An answer awaited does
    // not — a new secret calls the old check off.
    let ready = !open.locked(now) && !open.secret.is_empty();
    let entered = secret.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let unlock = BigButton::new(strings.get(labels::UNLOCK))
        .kind(ButtonKind::Primary)
        .enabled(live && ready)
        .show(ui, &mut cx.widgets);
    if (unlock.clicked() || entered) && ready {
        open.message = None;
        let user = needs_user.then(|| std::mem::take(&mut open.user));
        let secret = std::mem::take(&mut open.secret);
        return Some(PromptAction::Submit(Credential::Password { user, secret }));
    }
    None
}

/// The waiting card: what to do, and a ring going round. A reader that types like a keyboard is
/// heard here — what it types is submitted on Enter.
fn external_body(
    ui: &mut egui::Ui,
    cx: &mut PromptCx<'_>,
    open: &mut Open,
    label: &str,
    live: bool,
    now: Instant,
) -> Option<PromptAction> {
    let theme = cx.widgets.theme;
    let target = control_height(&theme.metrics, &theme.control);
    text_line(
        ui,
        cx.strings.get(label),
        egui::FontId::proportional(theme.metrics.type_scale.body),
        theme.color(ColorRole::OnSurface),
        ui.available_width(),
    );
    // A badge wait can last all night on a lock screen: a few steps a second, not sixty.
    let _ = ProgressRing::indeterminate()
        .diameter(target)
        .repaint_every(WAIT_REPAINT)
        .show(ui, &mut cx.widgets);
    if !live {
        return None;
    }
    if open
        .wedge_at
        .is_some_and(|at| now.saturating_duration_since(at) > READ_GAP)
    {
        open.wedge.clear();
        open.wedge_at = None;
    }
    let mut submit = false;
    ui.input(|i| {
        for event in &i.events {
            match event {
                egui::Event::Text(text) => {
                    open.wedge.push_str(text);
                    open.wedge_at = Some(now);
                }
                // Readers end a read with Enter, or with Tab.
                egui::Event::Key {
                    key: egui::Key::Enter | egui::Key::Tab,
                    pressed: true,
                    repeat: false,
                    ..
                } => submit = true,
                _ => {}
            }
        }
    });
    if !submit || open.wedge.is_empty() {
        return None;
    }
    let bytes = std::mem::take(&mut open.wedge).into_bytes();
    open.wedge_at = None;
    // Not while the authenticator has locked the way in. Over an answer still awaited it goes in,
    // and the check it replaces is called off.
    if open.locked(now) {
        return None;
    }
    Some(PromptAction::Submit(Credential::External(bytes)))
}

/// One line of text, centred and cut to `width`.
fn text_line(ui: &mut egui::Ui, text: &str, font: egui::FontId, color: Color32, width: f32) {
    let height = font.size * LINE;
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    let galley = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let size = galley.size();
    ui.painter()
        .galley(rect.center() - size / 2.0, galley, color);
}

/// The padlock and the gate's hint (`AccessPolicy::hint`) — which gate the prompt is for.
fn hint_line(ui: &mut egui::Ui, cx: &mut PromptCx<'_>, hint: &str, width: f32) {
    let theme = cx.widgets.theme;
    let size = theme.metrics.type_scale.small;
    let color = theme.color(ColorRole::Muted);
    let font = egui::FontId::proportional(size);
    let mut job = egui::text::LayoutJob::simple_singleline(hint.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((width - size * 1.4).max(1.0));
    let galley = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, size * LINE), Sense::hover());
    let lump = size + size * 0.4 + galley.size().x;
    let x = rect.center().x - lump / 2.0;
    let icon = Rect::from_center_size(
        egui::pos2(x + size / 2.0, rect.center().y),
        Vec2::splat(size),
    );
    let style = crate::icons::IconStyle::sized(size).color(crate::icons::IconColor::Fixed(color));
    cx.widgets
        .icons
        .paint(ui.painter(), icon, &crate::icon::LOCK, &style, theme);
    ui.painter().galley(
        egui::pos2(x + size * 1.4, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::{Prompt, Purpose};
    use crate::access::{AuthMethod, Gate};
    use std::time::{Duration, Instant};

    fn pin() -> Vec<AuthMethod> {
        vec![AuthMethod::Pin {
            len: 4,
            max_len: 0,
            shuffle: false,
        }]
    }

    #[test]
    fn a_second_request_takes_over_the_open_prompt() {
        let now = Instant::now();
        let mut prompt = Prompt::default();
        prompt.open(
            Purpose::Unlock {
                gate: Gate::from("a"),
                then: None,
            },
            pin(),
            None,
            now,
        );
        prompt.open(
            Purpose::Unlock {
                gate: Gate::from("b"),
                then: None,
            },
            pin(),
            None,
            now,
        );
        assert_eq!(prompt.gate(), Some(Gate::from("b")));
        // The lock screen is not an unlock prompt: it opens fresh and reports `session.lock`.
        prompt.close(now);
        prompt.open(Purpose::Lock, pin(), None, now);
        assert!(prompt.is_lock_screen());
        assert_eq!(prompt.gate(), Some(super::LOCK_GATE));
    }

    #[test]
    fn closing_keeps_nothing_typed() {
        let now = Instant::now();
        let mut prompt = Prompt::default();
        prompt.open(Purpose::Lock, pin(), None, now);
        if let Some(open) = prompt.open.as_mut() {
            open.pin.push_str("12");
        }
        assert_eq!(prompt.close(now), Some(Purpose::Lock));
        assert!(!prompt.is_open());
        assert!(prompt
            .closing
            .as_ref()
            .is_some_and(|c| c.open.pin.is_empty()));
    }

    #[test]
    fn the_countdown_wakes_on_each_second_and_at_the_end() {
        let now = Instant::now();
        let mut prompt = Prompt::default();
        prompt.open(Purpose::Lock, pin(), None, now);
        prompt.locked(now + Duration::from_millis(2500), "Too many".to_owned());
        // 2.5 s left shows "3 s"; it turns to "2 s" with 2 s left.
        assert_eq!(
            prompt.next_deadline(now),
            Some(now + Duration::from_millis(500))
        );
        assert_eq!(
            prompt.next_deadline(now + Duration::from_millis(2200)),
            Some(now + Duration::from_millis(2500))
        );
        assert_eq!(prompt.next_deadline(now + Duration::from_secs(3)), None);
    }
}
