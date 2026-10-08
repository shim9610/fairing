//! The on-screen keyboard (A5). Feature `osk`.
//!
//! - **When it shows**: on every frame where `ctx.egui_wants_keyboard_input()` is true and the
//!   focused screen's `ChromePolicy::osk == Auto`. `Manual` shows only on the toggle (nav
//!   `Custom("osk")`), `Off` never. Hiding needs false to hold for `osk_hide_debounce` (100 ms),
//!   so tabbing between fields does not flicker.
//! - **A close request** ([`Osk::hide`]: back · the Hide key) puts the target straight back to
//!   hidden and sets `dismissed`. `Auto` will not reopen even with the focus unchanged.
//!   Two signals release it — a rising edge of `wants` (false → true), and **the frame
//!   after a tap on the screen while closed ends, if something still has focus**. The second
//!   is checked once the finger has lifted, not on the frame after the press: a tap on empty
//!   space takes the focus away only when it completes, so read any earlier it would reopen
//!   the keyboard for the debounce. The second is needed
//!   because in egui 0.36.1 pressing another `TextEdit` hands the focus over within the same
//!   pass, so `wants` never drops to false for even one frame (`TextEdit` calls `request_focus`
//!   on the **press** — `text_edit/builder.rs:798`). Watching only the rising edge would leave a
//!   state where "tapping another field does not reopen it".
//! - **The driving value**: `y ∈ [0, osk_h]`, 180 ms to show · 160 ms to hide, `CubicOut`. A hide
//!   trigger mid-show reverses from wherever y is.
//! - **Rendering**: an `Area(Order::Middle)` pinned to `Layout.osk` (**above** the nav bar, or the
//!   bottom of the screen when the nav bar is off), with `constrain(false)`. The key
//!   panel is always drawn at its full height (`height()`) and clipped to what is visible
//!   (`set_clip_rect`), so it **slides up from below** (A5, "top = `screen_h` − y").
//!   `Layout.osk` / `PaneInfo.inset_bottom = y` are handed over **every frame** (the content is
//!   not pushed).
//! - **Size**: height = the screen height × `height_ratio` (0.38), but no row of keys taller than
//!   `metrics.osk_max_key` (one and a half fingers, gap included; infinite when an integrator
//!   lifts the cap) and no key shorter than `min_key_px` (48, the gaps on top); the floor wins
//!   where the two cross. Either way it is never taller than the room above the nav bar.
//! - **Key taps**: the hit area is the drawn key Rect plus half the gap, so there is no dead band
//!   between keys. A pressed key is tinted immediately (A7) and shrinks by `press_scale`. Label
//!   galleys are not laid out again while the face and the text size are unchanged — zero heap
//!   allocation on the render path.
//! - **Focus**: egui takes the focus off a widget **when a click completes outside it** —
//!   `InputOptions::surrender_focus_on` defaults to [`egui::SurrenderFocusOn::Clicks`] (`Presses`
//!   is the non-default alternative and the shell does not touch the value), and the decision is
//!   `pointer_clicked_elsewhere` at `egui-0.36.1/src/context.rs:1550-1558`. So the `TextEdit`
//!   loses focus on the frame the key is **released**. The previous pass's `memory.focused()` is
//!   remembered and restored with `request_focus` after the injection. The injection itself is
//!   deferred to the next pass by [`inject()`].
//! - **⇧**: the built-in qwerty types one character in uppercase and returns to lowercase. Two
//!   presses within 400 ms lock it (a locked ⇧ takes the accent colour). An integrator `Custom`
//!   layout keeps its face as it is.
//! - Password masking is `TextEdit::password`'s job. PINs are [`PinPad`](crate::widgets::PinPad).
// An example using the manual toggle (`nav_item("osk")`) is `examples/`'s share.
// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the rest of clippy's pedantic set stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod compose;
mod hangul;
mod hooks;
mod inject;
mod layouts;

pub use compose::{Compose, Composer};
pub use hangul::HangulComposer;
pub use hooks::{OskKeyCx, OskKeyLayout, OskKeyLayoutCx, OskKeyPainter};
pub use inject::{inject, inject_compose};
pub use layouts::{KeyAction, KeyDef, KeyFace, KeyLayout, KeyRow, OskLayout};

use crate::config::OskConfig;
use crate::icons::{IconCache, IconDef, IconSet, IconStyle};
use crate::motion::Animated;
use crate::screen::OskMode;
use crate::theme::{ColorRole, MotionTokens, Theme};
use egui::Rect;
use std::sync::Arc;
use std::time::Instant;

/// The ⇧ double-tap (caps lock) window, in seconds.
const SHIFT_LOCK_SECS: f64 = 0.4;

/// The ⇧ key label (the key by which the lock indicator finds its key).
const SHIFT_LABEL: &str = "⇧";

/// One key's position, `(row, column)`.
type KeyIndex = (usize, usize);

/// One frame's key-panel result.
#[derive(Debug, Default, Clone, Copy)]
struct DrawOut {
    /// The key tapped on this frame.
    hit: Option<KeyIndex>,
    /// The key currently held down.
    down: Option<KeyIndex>,
}

/// The colours and scale one key is drawn with.
struct KeyLook<'a> {
    theme: &'a Theme,
    press_scale: f32,
    scaled: bool,
    locked_shift: bool,
}

impl KeyLook<'_> {
    /// The key as drawn: shrunk about its centre by the press scale while it is held.
    fn drawn(&self, rect: Rect) -> Rect {
        if self.scaled {
            Rect::from_center_size(rect.center(), rect.size() * self.press_scale)
        } else {
            rect
        }
    }
}

/// What the OSK hands back to the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OskAction {
    /// A key was pressed and injected.
    Injected(KeyAction),
    /// The hide key.
    HideRequested,
}

/// The on-screen keyboard.
// The show target, the manual toggle, the caps lock and a press while closed are independent switches —
// bundling them into one state machine would only multiply the combinations (the same judgement as
// `ChromePolicy`).
#[allow(clippy::struct_excessive_bools)]
pub struct Osk {
    layout: OskLayout,
    keys: KeyLayout,
    face: usize,
    height_ratio: f32,
    min_key_px: f32,
    /// The tallest a row of keys gets (du) — `metrics.osk_max_key`, handed over by the shell every
    /// frame because it is resolved in millimetres.
    max_key: f32,
    /// The gap between keys and round the panel (du) — `metrics.osk_key_gap`, handed over with
    /// `max_key`.
    key_gap: f32,
    /// The height above the nav bar (du), handed over with `max_key`. The panel never exceeds it.
    room: f32,
    y: Animated<f32>,
    height: f32,
    shown: bool,
    manual: bool,
    /// Once closed by [`Osk::hide`], `Some(the previous frame's wants)` — it blocks an `Auto`
    /// reopen until `wants` has a rising edge (false → true). `None` = no suppression.
    dismissed: Option<bool>,
    lost_focus_at: Option<Instant>,
    /// The key Rects drawn last frame (row, column, Rect). `clear` + `push` every frame — never reallocated.
    key_rects: Vec<(usize, usize, Rect)>,
    /// The label galley cache (in the current face's key order). No relayout while `labels_key` is unchanged.
    labels: Vec<Arc<egui::Galley>>,
    /// The galley cache's validity key = (face, text-size bits).
    labels_key: (usize, u32),
    /// The pressed key (A7 tint · scale).
    pressed_key: Option<KeyIndex>,
    /// The press scale's progress `0 → 1` (advanced in `update`).
    press: Animated<f32>,
    /// Caps lock (⇧ double tap).
    shift_lock: bool,
    /// The last ⇧ time (`ctx.input(|i| i.time)`, seconds).
    last_shift_at: Option<f64>,
    /// The previous pass's focused widget (what to restore after injecting).
    last_focus: Option<egui::Id>,
    /// A press on the screen began while closed and the finger is still down.
    press_held_while_dismissed: bool,
    /// A tap on the screen while closed ended on the last frame (if something still has focus, it reopens).
    press_while_dismissed: bool,
    /// The special-key icon cache — glyphs egui's default font does not have (⇧ ⌫ ↵ ▾ ✓) are drawn
    /// with built-in icons ([`special_icon`]). The label strings stay `key_rect`'s keys.
    icons: IconCache,
    /// The context, held from drawing time. `update(wants: bool, ..)` has no context, so this is
    /// the only way to read the "the screen was pressed" signal (the close-request item in the
    /// module docs). A `Context` is one `Arc` deep, so cloning is cheap, and it does not point
    /// back at the shell, so there is no cycle.
    ctx: Option<egui::Context>,
    /// The composing input method. Present only on a layout that stacks jamo into
    /// syllables, as Hangul does. Being a `Box<dyn>`, an integrator's — or a later language's
    /// (Chinese, Japanese) — goes in the same slot.
    composer: Option<Box<dyn Composer>>,
    /// How many of the composing characters are **already in the buffer**. The next step erases
    /// that many with ⌫ and types the new composition result (`inject::inject_compose`).
    provisional: usize,
    /// Where the caret of the field being composed into should stand once the last composition
    /// result has landed: `(the field, the caret's character index)`. A caret anywhere else at
    /// the next jamo means the buffer is not as the composition left it — the field refused a
    /// character, or text came from somewhere other than the keyboard.
    expect_caret: Option<(egui::Id, usize)>,
    /// The layout the `한/영` key crosses to. `None` means a single language, so there is no `한/영` key.
    lang_alt: Option<OskLayout>,
    /// Whether this keyboard runs as a `한/영` pair. It outlives a detour through a layout with
    /// no pair (the numpad, a custom pad), so the key comes back with the layout that had it.
    paired: bool,
    /// Drawn in `Order::Foreground` above the unlock prompt rather than in `Order::Middle` (the
    /// password card needs the keyboard, and the modal takes every press below it).
    raised: bool,
    /// The integrator's placement of the keys (rung 4).
    key_layout: Option<OskKeyLayout>,
    /// The integrator's key, in place of the built-in one (rung 5).
    key_painter: Option<OskKeyPainter>,
    /// This frame's key places in reading order — the built-in ones, then the layout's. A buffer
    /// whose values are overwritten each frame.
    places: Vec<Rect>,
}

impl std::fmt::Debug for Osk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Osk")
            .field("layout", &self.keys.name)
            .field("face", &self.face)
            .field("shown", &self.shown)
            .field("composer", &self.composer.as_ref().map(|c| c.name()))
            .finish_non_exhaustive()
    }
}

impl Osk {
    /// From `[osk]`.
    #[must_use]
    pub fn from_config(cfg: &OskConfig) -> Self {
        let layout = OskLayout::parse(&cfg.layout, cfg.numpad_decimal, cfg.numpad_sign)
            .unwrap_or_else(|| {
                log::warn!(
                    "[osk] layout = \"{}\" is not a known layout - using qwerty",
                    cfg.layout
                );
                OskLayout::Qwerty
            });
        // Starting on dubeolsik makes it a keyboard that crosses between two languages — the `한/영` key goes on and the other side comes into being.
        let lang_alt = if matches!(layout, OskLayout::Hangul) {
            layout.lang_pair()
        } else {
            None
        };
        let keys = layout.build_with(lang_alt.is_some());
        let composer = layout.composer();
        Self {
            layout,
            keys,
            face: 0,
            height_ratio: cfg.height_ratio,
            min_key_px: cfg.min_key_px,
            max_key: crate::theme::Metrics::default().osk_max_key,
            key_gap: crate::theme::Metrics::default().osk_key_gap,
            room: f32::INFINITY,
            y: Animated::new(0.0),
            height: 0.0,
            shown: false,
            manual: false,
            dismissed: None,
            lost_focus_at: None,
            key_rects: Vec::new(),
            labels: Vec::new(),
            labels_key: (usize::MAX, 0),
            pressed_key: None,
            press: Animated::new(0.0),
            shift_lock: false,
            last_shift_at: None,
            last_focus: None,
            press_held_while_dismissed: false,
            press_while_dismissed: false,
            icons: IconCache::new(),
            ctx: None,
            composer,
            provisional: 0,
            expect_caret: None,
            paired: lang_alt.is_some(),
            lang_alt,
            raised: false,
            key_layout: None,
            key_painter: None,
            places: Vec::new(),
        }
    }

    /// Replace the layout (when a screen wants the numpad, say).
    ///
    /// A composition in progress is committed. Switching to dubeolsik brings the `한/영` key with
    /// it and makes the English qwerty the other side — every other layout is single-language.
    pub fn set_layout(&mut self, layout: OskLayout) {
        self.flush_composer();
        // **The pair is not lost.** On dubeolsik a pair goes with it (a German kiosk has no reason for a
        // `한/영` key, so starting on qwerty does not get one), and where it was **already running as a
        // pair** it is kept even on moving to English — otherwise one `set_layout(Qwerty)` takes the key
        // to come back with away and it is stuck in English. [`Self::toggle_lang`], which the `한/영` key
        // calls, always kept it; only this one, called by the integrator, did not. The pairing
        // is remembered apart from the key: a detour through the numpad, which has no `한/영`
        // key, does not end it, so restoring the layout found before brings the key back.
        self.paired |= matches!(layout, OskLayout::Hangul);
        self.lang_alt = if self.paired {
            layout.lang_pair()
        } else {
            None
        };
        self.composer = layout.composer();
        self.keys = layout.build_with(self.lang_alt.is_some());
        self.layout = layout;
        self.face = 0;
        self.shift_lock = false;
        self.last_shift_at = None;
        self.invalidate_labels();
    }

    /// **Whether the `한/영` key is present.** Only on a layout that has a pair — opened as
    /// dubeolsik, or moved to English from there. A device that starts on qwerty does not get it.
    #[must_use]
    pub const fn has_lang_key(&self) -> bool {
        self.lang_alt.is_some()
    }

    /// `한/영` — cross to the other language's layout. A composition in progress is committed.
    /// Nothing happens on a single-language keyboard.
    #[doc(hidden)]
    pub fn toggle_lang(&mut self) {
        let Some(next) = self.lang_alt.take() else {
            return;
        };
        self.flush_composer();
        self.lang_alt = Some(std::mem::replace(&mut self.layout, next));
        self.composer = self.layout.composer();
        self.keys = self.layout.build_with(true);
        self.face = 0;
        self.shift_lock = false;
        self.last_shift_at = None;
        self.invalidate_labels();
    }

    /// Whether a composition is in progress (Hangul jamo being stacked).
    #[doc(hidden)]
    #[must_use]
    pub fn is_composing(&self) -> bool {
        self.composer.as_ref().is_some_and(|c| c.is_composing())
    }

    /// The composing string. Empty with no composer, or with no composition in progress.
    #[must_use]
    pub fn preedit(&self) -> String {
        self.composer
            .as_ref()
            .map(|c| c.preedit())
            .unwrap_or_default()
    }

    /// End the composition. **The characters are already in the buffer, so there is nothing to
    /// insert** — only the composer and [`Osk::provisional`] are cleared (see "Why not `Preedit`"
    /// in `inject_compose`).
    fn flush_composer(&mut self) {
        self.provisional = 0;
        self.expect_caret = None;
        let Some(composer) = self.composer.as_mut() else {
            return;
        };
        if !composer.is_composing() {
            return;
        }
        // The string handed back is the syllable that was composing, and it is already on the screen.
        let _ = composer.flush();
    }

    /// Move the composition result into the buffer: erase the composing characters put in last time and type the new ones.
    fn put_compose(&mut self, ctx: &egui::Context, out: &crate::osk::compose::Compose) {
        let mut text = out.commit.clone();
        text.push_str(&out.preedit);
        let field = self.last_focus;
        let before = field.and_then(|id| caret(ctx, id));
        let _ = inject_compose(ctx, self.provisional, &text);
        self.expect_caret = field.zip(before).map(|(id, at)| {
            (
                id,
                at.saturating_sub(self.provisional) + text.chars().count(),
            )
        });
        self.provisional = out.preedit.chars().count();
    }

    /// **The composition only goes on over a buffer it left as it was.** The erase-and-retype
    /// assumes the composing characters are the last ones before the caret. When the caret is
    /// not where the last result should have left it, they are not: a full field (`char_limit`)
    /// refused a jamo, or a hardware keyboard or a barcode scanner typed after it, or moved the
    /// caret. Erasing then would take a committed character or the scanned text. So the
    /// composition ends there — what is in the buffer stays — and the next jamo starts afresh.
    fn check_buffer(&mut self, ctx: &egui::Context) {
        if self.provisional == 0 {
            return;
        }
        let Some((id, want)) = self.expect_caret else {
            return;
        };
        if caret(ctx, id).is_some_and(|at| at != want) {
            self.flush_composer();
        }
    }

    /// Feed a character key to the composer. `true` if it consumed it — the caller then injects nothing more.
    fn feed_composer(&mut self, ctx: &egui::Context, text: &str) -> bool {
        self.check_buffer(ctx);
        let Some(composer) = self.composer.as_mut() else {
            return false;
        };
        let out = composer.feed(text);
        self.put_compose(ctx, &out);
        // For a character the composer does not take, only the commit is moved and the caller does the original injection.
        out.consumed
    }

    /// Feed a delete to the composer. `true` if a composition was in progress (one jamo was taken back).
    fn backspace_composer(&mut self, ctx: &egui::Context) -> bool {
        self.check_buffer(ctx);
        let Some(composer) = self.composer.as_mut() else {
            return false;
        };
        let out = composer.backspace();
        if !out.consumed {
            return false;
        }
        self.put_compose(ctx, &out);
        true
    }

    /// The current layout.
    #[must_use]
    pub fn layout(&self) -> &OskLayout {
        &self.layout
    }

    /// The current face index.
    #[must_use]
    pub fn face(&self) -> usize {
        self.face
    }

    /// Whether caps is locked (⇧ double tap). Only ever on for the built-in qwerty.
    #[must_use]
    pub fn shift_locked(&self) -> bool {
        self.shift_lock
    }

    /// What bounds the panel, in du — the tallest a row of keys may get (`metrics.osk_max_key`),
    /// the gap between keys (`metrics.osk_key_gap`), and the room above the nav bar. The shell
    /// resolves them every frame and hands them over before [`Osk::update`].
    pub(crate) fn set_bounds(&mut self, max_key: f32, key_gap: f32, room: f32) {
        self.max_key = max_key;
        self.key_gap = key_gap.max(0.0);
        self.room = room.max(0.0);
    }

    /// The OSK height at this screen height: the `height_ratio` share, cut to `max_key` per row
    /// (gap included) and then raised so no key is shorter than `min_key_px` (the gaps come on
    /// top) — so where the cap and the floor cross, the floor wins and the keys stay big enough to
    /// hit. Last, it is cut to the screen and to the room above the nav bar: a keyboard whose top
    /// row is off the glass cannot be typed on at all.
    #[must_use]
    pub(crate) fn height_for(&self, screen_height: f32) -> f32 {
        let rows = self
            .keys
            .faces
            .get(self.face)
            .map_or(4, |f| f.rows.len().max(1)) as f32;
        let floor = self.min_key_px.mul_add(rows, self.key_gap * (rows + 1.0));
        (screen_height * self.height_ratio)
            .min(self.max_key * rows)
            .max(floor)
            .min(screen_height)
            .min(self.room)
    }

    /// Frame stage 5: the show/hide decision plus its progress. `wants` =
    /// `ctx.egui_wants_keyboard_input()`. `true` means it is moving.
    pub fn update(
        &mut self,
        wants: bool,
        mode: OskMode,
        screen_height: f32,
        now: Instant,
        dt: f32,
        tokens: &MotionTokens,
    ) -> bool {
        let height = self.height_for(screen_height);
        if (height - self.height).abs() > f32::EPSILON {
            self.height = height;
            self.invalidate_labels();
        }
        // The signals for reopening after a close request (see the module docs): a rising edge of `wants`,
        // or something having focus on the frame after a press on the screen while closed (so that tapping
        // the same field again opens it too).
        if let Some(prev) = self.dismissed {
            let regained = wants && (!prev || self.press_while_dismissed);
            self.dismissed = if regained { None } else { Some(wants) };
        }
        // A tap is over when the finger lifts: only then has egui settled where the focus went
        // (a tap on empty space surrenders it on the release), so it is read the frame after.
        let (pressed, released) = self.pointer_edges();
        let held = self.dismissed.is_some() && (self.press_held_while_dismissed || pressed);
        self.press_while_dismissed = held && released;
        self.press_held_while_dismissed = held && !released;
        if mode != OskMode::Auto || self.dismissed.is_some() {
            // Only the automatic mode hides on lost focus, so only it keeps a debounce running.
            self.lost_focus_at = None;
        }
        let want_shown = match mode {
            OskMode::Off => false,
            OskMode::Manual => self.manual,
            OskMode::Auto if self.dismissed.is_some() => false,
            OskMode::Auto => {
                if wants {
                    self.lost_focus_at = None;
                    true
                } else if self.shown {
                    let since = *self.lost_focus_at.get_or_insert(now);
                    now.saturating_duration_since(since) < tokens.osk_hide_debounce
                } else {
                    false
                }
            }
        };
        if want_shown != self.shown {
            self.shown = want_shown;
            if want_shown {
                self.y.to(self.height, tokens.osk_show);
            } else {
                self.y.to(0.0, tokens.osk_hide);
                self.lost_focus_at = None;
                // The composition ends with the panel going down — leaving the composing marking behind
                // as it disappears has the next composition erase the wrong range.
                self.flush_composer();
            }
        } else if want_shown && (self.y.target() - self.height).abs() > 0.5 {
            // The target follows a change in the screen's size.
            self.y.to(self.height, tokens.osk_show);
        }
        let moving = self.y.tick(dt);
        // The A7 press scale (the tint is immediate; only the scale tweens).
        let pressing = self.press.tick(dt);
        moving || pressing
    }

    /// When the lost-focus debounce runs out and an automatic keyboard hides — the moment the
    /// shell wakes for. `None` while nothing is counting down (focus held, or a manual keyboard,
    /// which never hides by itself).
    #[must_use]
    pub(crate) fn hide_deadline(&self, tokens: &MotionTokens) -> Option<Instant> {
        self.lost_focus_at
            .filter(|_| self.shown)
            .and_then(|since| since.checked_add(tokens.osk_hide_debounce))
    }

    /// Draw above the unlock prompt (`true`) or in the keyboard's own place under the shade
    /// (`false`). The shell raises it for the one draw that happens while the prompt is up.
    pub(crate) fn raise(&mut self, above: bool) {
        self.raised = above;
    }

    /// The manual toggle (nav bar `Custom("osk")`, `OskMode::Manual`). Turning it on voids any earlier close request.
    pub fn toggle(&mut self) {
        self.manual = !self.manual;
        if self.manual {
            self.dismissed = None;
        }
    }

    /// A close request (back · the Hide key). The target becomes hidden at once and the next
    /// [`Osk::update`] brings `y` down to 0 (the `osk_hide` tween). `Auto` will not reopen even
    /// with the focus unchanged — only a rising edge of `wants` releases it.
    pub fn hide(&mut self) {
        self.flush_composer();
        self.manual = false;
        self.lost_focus_at = None;
        self.press_held_while_dismissed = false;
        self.press_while_dismissed = false;
        // It conservatively assumes something has focus right now — if not, it becomes `Some(false)` next frame.
        self.dismissed = Some(true);
    }

    /// Whether a close request is holding it down (an `Auto` reopen is being suppressed).
    #[doc(hidden)]
    #[must_use]
    pub fn is_dismissed(&self) -> bool {
        self.dismissed.is_some()
    }

    /// The bottom height the content should avoid right now (= y while animating).
    #[must_use]
    pub fn inset_bottom(&self) -> f32 {
        self.y.value().max(0.0)
    }

    /// Whether the target is "shown".
    #[doc(hidden)]
    #[must_use]
    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// Whether any of it is visible.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.y.value() > 0.5
    }

    /// Whether it is moving (the show/hide tween · the key press scale).
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.y.is_animating() || self.press.is_animating()
    }

    /// The fully extended height (as of the last update).
    #[must_use]
    pub fn height(&self) -> f32 {
        self.height
    }

    /// Look a key Rect up by label (the Rect **drawn** last frame, on the current face), so that
    /// tests need not duplicate the layout arithmetic. Labels are unique within a face (the
    /// `layouts` unit tests keep them so).
    #[must_use]
    pub fn key_rect(&self, label: &str) -> Option<Rect> {
        let face = self.keys.faces.get(self.face)?;
        self.key_rects.iter().find_map(|(r, k, rect)| {
            face.rows
                .get(*r)
                .and_then(|row| row.keys.get(*k))
                .filter(|key| key.label == label)
                .map(|_| *rect)
        })
    }

    /// Place the keys with `layout` (rung 4).
    pub(crate) fn set_key_layout(&mut self, layout: OskKeyLayout) {
        self.key_layout = Some(layout);
    }

    /// Draw each key with `painter` instead of the built-in key (rung 5).
    pub(crate) fn set_key_painter(&mut self, painter: OskKeyPainter) {
        self.key_painter = Some(painter);
    }

    /// Stage 10: draw. `osk_rect` is `Layout.osk` — **above the nav bar**, or the bottom of the
    /// screen when the nav bar is off (so that the back button, which closes the OSK, is not
    /// covered). A key tap is injected here and the result handed back. `icons` is the shell's icon
    /// set, for a key painter.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        osk_rect: Rect,
        theme: &Theme,
        icons: &mut IconSet,
    ) -> Option<OskAction> {
        if self.ctx.is_none() {
            self.ctx = Some(ctx.clone());
        }
        if !self.is_visible() {
            self.key_rects.clear();
            self.release_press();
            return None;
        }
        // The previous pass's focus is remembered. On the frame the tap completes, the screen
        // (stage 9) is drawn first and has already lost the focus, so this is the value caught the frame before.
        if let Some(id) = ctx.memory(egui::Memory::focused) {
            // The focus has moved to another widget — only the composition state is dropped. The syllable
            // that was composing is already **in the previous widget's buffer as real characters**, so
            // putting it in again here would make the same characters once more in the new widget.
            if self.last_focus.is_some_and(|prev| prev != id) {
                self.provisional = 0;
                self.expect_caret = None;
                if let Some(composer) = self.composer.as_mut() {
                    composer.reset();
                }
            }
            self.last_focus = Some(id);
        }
        // **A press outside the panel ends the composition.** The caret may have moved even within the
        // same field, and then the next jamo's ⌫ erases the wrong character. A real IME follows the same
        // convention.
        // Where the press went down: a finger that came down on a key and slid off in the same
        // frame pressed the panel, not the page.
        let outside = ctx.input(|i| {
            i.pointer.primary_pressed()
                && crate::drag::press_point(i)
                    .or(i.pointer.interact_pos())
                    .is_none_or(|p| !osk_rect.contains(p))
        });
        if outside {
            self.flush_composer();
        }
        let out = self.draw(ctx, osk_rect, theme, icons);
        self.set_pressed(out.down, theme);
        let (row, col) = out.hit?;
        let action = self
            .keys
            .faces
            .get(self.face)?
            .rows
            .get(row)?
            .keys
            .get(col)?
            .action
            .clone();
        match &action {
            KeyAction::Face(target) => {
                // The composition is left alone — a tense consonant may be being typed with ⇧ (`ㄱ` → ⇧ → `ㄲ`).
                self.switch_face(*target, ctx.input(|input| input.time));
                // A face switch restores the focus too. Pressing ⇧ or `123` completes a click
                // outside the `TextEdit`, so egui takes its focus away (the focus item in the module docs).
                // Without restoring it here, `wants` stays false and the panel goes down after
                // `osk_hide_debounce` — the bug where switching to the number face and pausing to look at
                // it made the keyboard disappear.
                self.restore_focus(ctx);
                None
            }
            KeyAction::Lang => {
                self.toggle_lang();
                self.restore_focus(ctx);
                None
            }
            KeyAction::Hide => {
                self.flush_composer();
                Some(OskAction::HideRequested)
            }
            other => {
                // Where there is a composer, characters and deletes go through it first. Only what it did
                // not consume is injected as it would have been ([`compose`]).
                let handled = match other {
                    KeyAction::Text(text) => self.feed_composer(ctx, text),
                    KeyAction::Backspace => self.backspace_composer(ctx),
                    _ => {
                        self.flush_composer();
                        false
                    }
                };
                if !handled {
                    let _ = inject(ctx, other);
                }
                self.restore_focus(ctx);
                if other.is_text() {
                    self.after_text_key();
                }
                Some(OskAction::Injected(action))
            }
        }
    }

    /// Draw the key panel and hand back `(the key tapped this frame, the key held down)`.
    fn draw(
        &mut self,
        ctx: &egui::Context,
        osk_rect: Rect,
        theme: &Theme,
        set: &mut IconSet,
    ) -> DrawOut {
        let gap = theme.metrics.osk_key_gap;
        // The key panel is always drawn at its full height and clipped to what is visible → it slides up from below (A5).
        let panel = Rect::from_min_max(
            egui::pos2(osk_rect.left(), osk_rect.bottom() - self.height.max(1.0)),
            osk_rect.max,
        );
        let press_scale = 1.0 - (1.0 - theme.motion.press_scale) * self.press.value();
        let shift_lock = self.shift_lock;
        let held = self.pressed_key;
        let order = if self.raised {
            egui::Order::Foreground
        } else {
            egui::Order::Middle
        };
        let Self {
            keys,
            face: face_index,
            key_rects,
            labels,
            labels_key,
            icons,
            key_layout,
            key_painter,
            places,
            ..
        } = self;
        let Some(face) = keys.faces.get(*face_index) else {
            key_rects.clear();
            return DrawOut::default();
        };
        place_keys(face, *face_index, panel, theme, key_layout.as_mut(), places);
        key_rects.clear();
        let mut out = DrawOut::default();
        // **The keyboard is the top of `Order::Middle` while it shows, and it is solid.** The keys
        // are read off the raw pointer, not through egui widgets, so egui saw the keyboard's
        // area as empty and handed a press on a key to whatever lay under it — a widget's own
        // layer opened in `Middle` after the keyboard's first appearance (a `Dropdown` search's
        // shield, drawn there so that the shade stays over it) took the second letter typed as
        // the tap that closes the search. So the panel is claimed as one hover-sensing widget
        // (a click-sensing one would take the focus from the field being typed in), which is
        // what egui's hit test needs to stop at this layer, and the layer asks to be on top of
        // its order every frame — egui keeps every area ever created in its order and puts a
        // new one above the old, so once is not enough. Raised, the same holds one order up,
        // over the unlock prompt.
        let layer = egui::LayerId::new(order, egui::Id::new("fairing.osk"));
        ctx.move_to_top(layer);
        egui::Area::new(egui::Id::new("fairing.osk"))
            .order(order)
            .fixed_pos(panel.min)
            .default_size(panel.size())
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.set_clip_rect(osk_rect);
                let _ = ui.allocate_rect(osk_rect, egui::Sense::hover());
                let painter = ui.painter();
                painter.rect_filled(osk_rect, 0.0, theme.color(ColorRole::Surface));
                if key_painter.is_none() {
                    let font = egui::TextStyle::Button.resolve(ui.style());
                    relayout_labels(painter, face, &font, labels, labels_key, *face_index);
                }
                let mut index = 0usize;
                for (r, row) in face.rows.iter().enumerate() {
                    for (k, def) in row.keys.iter().enumerate() {
                        let place = places.get(index).copied().filter(Rect::is_positive);
                        let glyph_index = index;
                        index += 1;
                        // A place the layout emptied leaves the key out.
                        let Some(rect) = place else {
                            continue;
                        };
                        key_rects.push((r, k, rect));
                        let look = KeyLook {
                            theme,
                            press_scale,
                            scaled: held == Some((r, k)),
                            locked_shift: shift_lock && def.label == SHIFT_LABEL,
                        };
                        // The hit area runs to half the gap — there is no dead band between keys.
                        let hot = rect.expand(gap / 2.0).intersect(panel);
                        let (clicked, is_down) = key_response(ui, hot, (r, k));
                        let draw_rect = look.drawn(rect);
                        if let Some(paint) = key_painter.as_mut() {
                            let mut cx = OskKeyCx {
                                rect: draw_rect,
                                label: def.label.as_ref(),
                                action: &def.action,
                                pressed: is_down,
                                locked: look.locked_shift,
                                theme,
                                icons: set,
                            };
                            paint(ui.painter(), &mut cx);
                        } else {
                            let glyph = match special_icon(def.label.as_ref()) {
                                Some(icon) => Glyph::Icon(icon),
                                None => Glyph::Text(labels.get(glyph_index)),
                            };
                            paint_key(ui, draw_rect, is_down, glyph, &look, icons);
                        }
                        if is_down {
                            out.down = Some((r, k));
                        }
                        if clicked {
                            out.hit = Some((r, k));
                        }
                    }
                }
            });
        out
    }

    /// When the pressed key changes, swap the A7 scale target.
    fn set_pressed(&mut self, down: Option<KeyIndex>, theme: &Theme) {
        if down == self.pressed_key {
            return;
        }
        self.pressed_key = down;
        if down.is_some() {
            self.press.to(1.0, theme.motion.press);
        } else {
            self.press.to(0.0, theme.motion.press_release);
        }
    }

    /// While out of sight, the press counts as released.
    fn release_press(&mut self) {
        self.pressed_key = None;
        self.press.snap(0.0);
    }

    /// Whether the screen was pressed, and released, on this frame. Both `false` while the
    /// context has not been captured yet (= only the rising edge is watched).
    fn pointer_edges(&self) -> (bool, bool) {
        self.ctx.as_ref().map_or((false, false), |ctx| {
            ctx.input(|input| (input.pointer.any_pressed(), input.pointer.any_released()))
        })
    }

    /// Restore the focus after injecting. If something already holds it, leave it alone.
    fn restore_focus(&self, ctx: &egui::Context) {
        let Some(id) = self.last_focus else {
            return;
        };
        if ctx.memory(egui::Memory::focused).is_none() {
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
    }

    /// Switch faces. The built-in qwerty's ⇧ locks when pressed twice within `SHIFT_LOCK_SECS`.
    fn switch_face(&mut self, target: usize, now_secs: f64) {
        if target >= self.keys.faces.len() {
            return;
        }
        // ⇧ is the key that crosses between lowercase and uppercase. The symbol face's `abc` (→ lowercase) is not a ⇧.
        let shift_key = self.layout.has_shift()
            && (target == layouts::UPPER_FACE
                || (self.face == layouts::UPPER_FACE && target == layouts::LOWER_FACE));
        if !shift_key {
            self.shift_lock = false;
            self.last_shift_at = None;
            self.set_face(target);
            return;
        }
        if self.shift_lock {
            // Pressing ⇧ while locked unlocks it and goes to lowercase.
            self.shift_lock = false;
            self.last_shift_at = None;
            self.set_face(layouts::LOWER_FACE);
            return;
        }
        let double = self
            .last_shift_at
            .is_some_and(|prev| now_secs - prev <= SHIFT_LOCK_SECS);
        self.last_shift_at = Some(now_secs);
        if double {
            self.shift_lock = true;
            self.set_face(layouts::UPPER_FACE);
            return;
        }
        self.set_face(target);
    }

    /// Swap the face and invalidate the galley cache.
    fn set_face(&mut self, target: usize) {
        if target != self.face {
            self.face = target;
            self.invalidate_labels();
        }
    }

    /// After a character key: an unlocked uppercase face returns to lowercase (the built-in qwerty).
    /// A character also ends the ⇧ double tap: ⇧, a letter, ⇧ is two one-shot capitals (typing
    /// "HI"), not a lock.
    fn after_text_key(&mut self) {
        self.last_shift_at = None;
        if self.layout.has_shift() && self.face == layouts::UPPER_FACE && !self.shift_lock {
            self.face = layouts::LOWER_FACE;
            self.invalidate_labels();
        }
    }

    /// Invalidate the galley cache (the face or the size changed).
    fn invalidate_labels(&mut self) {
        self.labels_key = (usize::MAX, 0);
    }
}

/// The caret of the `TextEdit` `id` as a character index — the start of the selection when
/// there is one, which is where typed text lands. `None` for a field that keeps no egui
/// text-edit state.
fn caret(ctx: &egui::Context, id: egui::Id) -> Option<usize> {
    let range = egui::text_edit::TextEditState::load(ctx, id)?
        .cursor
        .char_range()?;
    Some(range.primary.index.0.min(range.secondary.index.0))
}

/// Rebuild the label galleys for this frame.
///
/// **They are not held across frames.** A `Galley` carries UVs into the font atlas, so when a new
/// glyph arrives and the atlas grows, those UVs shift wholesale and a cached galley draws the
/// wrong characters from then on — reproducible straight away on dubeolsik with a Hangul font
/// loaded. epaint's `GalleyCache` memoises the same input, so the layout cost is not paid twice.
/// The `Vec` is only cleared and refilled, so its capacity is not reallocated either.
fn relayout_labels(
    painter: &egui::Painter,
    face: &KeyFace,
    font: &egui::FontId,
    labels: &mut Vec<Arc<egui::Galley>>,
    labels_key: &mut (usize, u32),
    face_index: usize,
) {
    let cache_key = (face_index, font.size.to_bits());
    labels.clear();
    for def in face.rows.iter().flat_map(|row| row.keys.iter()) {
        labels.push(painter.layout_no_wrap(
            def.label.as_ref().to_owned(),
            font.clone(),
            egui::Color32::PLACEHOLDER,
        ));
    }
    *labels_key = cache_key;
}

/// What to draw on a key — the label galley, or a built-in icon.
#[derive(Clone, Copy)]
enum Glyph<'a> {
    /// A label (from the galley cache).
    Text(Option<&'a Arc<egui::Galley>>),
    /// A built-in icon (in place of a special-key glyph the font does not have).
    Icon(&'static IconDef),
}

/// The icon substitute for special-key glyphs egui's default fonts (Ubuntu-Light plus the emoji
/// icon font) do **not** have. The label strings themselves are left alone, so lookups and
/// injection — `key_rect("⇧")` and friends — behave the same.
fn special_icon(label: &str) -> Option<&'static IconDef> {
    let name = match label {
        "⇧" => "arrow-up",
        "⌫" => "arrow-left",
        "↵" | "✓" => "check",
        "▾" => "chevron-down",
        // It keeps the language-switch key visible on a device with no Hangul font.
        layouts::LANG_LABEL => "language",
        _ => return None,
    };
    crate::icons::find(name)
}

/// Where the face's keys go this frame, into `places` in reading order: their built-in places —
/// the rows evenly down the panel, each centred, each key as wide as its span — and then wherever
/// the integrator's layout moves them.
fn place_keys(
    face: &KeyFace,
    face_index: usize,
    panel: Rect,
    theme: &Theme,
    layout: Option<&mut OskKeyLayout>,
    places: &mut Vec<Rect>,
) {
    let gap = theme.metrics.osk_key_gap;
    let rows = face.rows.len().max(1) as f32;
    let row_h = (panel.height() - gap * (rows + 1.0)) / rows;
    let max_span = face.max_span().max(1.0);
    let unit = (panel.width() - gap * (max_span + 1.0)) / max_span;
    places.clear();
    for (r, row) in face.rows.iter().enumerate() {
        let row_span: f32 = row.keys.iter().map(KeyDef::span).sum();
        let row_w = row_span * unit + gap * (row.keys.len().saturating_sub(1)) as f32;
        let mut x = panel.center().x - row_w / 2.0;
        let y = panel.min.y + gap + (row_h + gap) * r as f32;
        for def in &row.keys {
            let w = def.span() * unit;
            places.push(Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, row_h)));
            x += w + gap;
        }
    }
    if let Some(layout) = layout {
        layout(&OskKeyLayoutCx::new(panel, face_index, face, theme), places);
    }
}

/// One key's hit area: `(was it tapped, is it held)`.
fn key_response(ui: &egui::Ui, hot: Rect, index: KeyIndex) -> (bool, bool) {
    let (r, k) = index;
    let resp = ui.interact(
        hot,
        egui::Id::new(("fairing.osk.key", r, k)),
        egui::Sense::click(),
    );
    (resp.clicked(), resp.is_pointer_button_down_on())
}

/// One key, the built-in way: its face, tinted while it is held, and its label or icon.
/// `draw_rect` is already shrunk by the press scale where the key is held.
fn paint_key(
    ui: &egui::Ui,
    draw_rect: Rect,
    is_down: bool,
    glyph: Glyph<'_>,
    look: &KeyLook<'_>,
    icons: &mut IconCache,
) {
    let theme = look.theme;
    let mut fill = if look.locked_shift {
        theme.color(ColorRole::Primary)
    } else {
        theme.color(ColorRole::SurfaceVariant)
    };
    if is_down {
        // A7: the tint is immediate on the first frame; only the scale tweens.
        fill = theme.color(ColorRole::Pressed).blend(fill);
    }
    let painter = ui.painter();
    painter.rect_filled(draw_rect, theme.metrics.corner_radius, fill);
    let color = if look.locked_shift {
        theme.color(ColorRole::OnPrimary)
    } else {
        theme.color(ColorRole::OnSurface)
    };
    match glyph {
        Glyph::Text(Some(galley)) => {
            painter.galley(
                draw_rect.center() - galley.size() / 2.0,
                Arc::clone(galley),
                color,
            );
        }
        Glyph::Text(None) => {}
        Glyph::Icon(def) => {
            // The in-chrome icon size rule (half of `icon_size` = 24 px), or the key's height where the key is lower.
            let size = (theme.metrics.icon_size * 0.5).min(draw_rect.height() * 0.6);
            let icon_rect = Rect::from_center_size(draw_rect.center(), egui::vec2(size, size));
            let style = IconStyle::sized(size);
            crate::icons::paint(painter, icon_rect, def, color, style.stroke_px(), icons);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::layouts::{LOWER_FACE, SYMBOL_FACE, UPPER_FACE};
    use super::Osk;
    use crate::config::{MotionConfig, OskConfig};
    use crate::screen::OskMode;
    use crate::theme::MotionTokens;
    use std::time::{Duration, Instant};

    fn qwerty() -> Osk {
        Osk::from_config(&OskConfig::default())
    }

    /// `hide()` does not reopen even with the focus unchanged, and releases only on the
    /// frame `wants` turns false → true. `Manual` closes on `hide()` too.
    #[test]
    fn hide_sticks_until_focus_is_regained() {
        let tokens = MotionTokens::from_config(&MotionConfig {
            reduce: true,
            ..MotionConfig::default()
        });
        let mut osk = qwerty();
        let t0 = Instant::now();
        let step = Duration::from_millis(16);
        osk.update(true, OskMode::Auto, 600.0, t0, 0.016, &tokens);
        assert!(osk.is_shown() && osk.inset_bottom() > 0.0);
        osk.hide();
        osk.update(true, OskMode::Auto, 600.0, t0 + step, 0.016, &tokens);
        assert!(!osk.is_shown(), "does not reopen while the focus stays put");
        assert!(osk.inset_bottom() < 0.5, "zero at once with reduce");
        osk.update(true, OskMode::Auto, 600.0, t0 + step * 2, 0.016, &tokens);
        assert!(!osk.is_shown() && osk.is_dismissed());
        osk.update(false, OskMode::Auto, 600.0, t0 + step * 3, 0.016, &tokens);
        osk.update(true, OskMode::Auto, 600.0, t0 + step * 4, 0.016, &tokens);
        assert!(
            osk.is_shown() && !osk.is_dismissed(),
            "reopens on the rising edge"
        );

        osk.hide();
        osk.update(false, OskMode::Manual, 600.0, t0 + step * 5, 0.016, &tokens);
        assert!(!osk.is_shown());
        osk.toggle();
        osk.update(false, OskMode::Manual, 600.0, t0 + step * 6, 0.016, &tokens);
        assert!(osk.is_shown(), "a manual toggle cancels the hide");
    }

    /// One ⇧ = one uppercase character; two quick ones = locked.
    #[test]
    fn shift_is_one_shot_and_double_tap_locks() {
        let mut osk = qwerty();
        osk.switch_face(UPPER_FACE, 1.0);
        assert_eq!(osk.face(), UPPER_FACE);
        assert!(!osk.shift_locked());
        osk.after_text_key();
        assert_eq!(
            osk.face(),
            LOWER_FACE,
            "back to lowercase after one character"
        );

        // Two ⇧s from lowercase (the uppercase face's ⇧ is a `Face(LOWER_FACE)`) → locked.
        osk.switch_face(UPPER_FACE, 10.0);
        osk.switch_face(LOWER_FACE, 10.2);
        assert!(osk.shift_locked(), "a double tap locks it");
        assert_eq!(
            osk.face(),
            UPPER_FACE,
            "the lock stays on the uppercase face"
        );
        osk.after_text_key();
        assert_eq!(osk.face(), UPPER_FACE, "locked, it stays");

        // Pressing ⇧ while locked unlocks it and goes to lowercase.
        osk.switch_face(LOWER_FACE, 12.0);
        assert!(!osk.shift_locked());
        assert_eq!(osk.face(), LOWER_FACE);

        // The symbol face is not a ⇧ and so drops out of the lock decision.
        osk.switch_face(SYMBOL_FACE, 13.0);
        assert!(!osk.shift_locked());
        assert_eq!(osk.face(), SYMBOL_FACE);
    }

    /// Two slow presses are not a lock.
    #[test]
    fn slow_double_shift_does_not_lock() {
        let mut osk = qwerty();
        osk.switch_face(UPPER_FACE, 1.0);
        osk.switch_face(LOWER_FACE, 1.2);
        osk.switch_face(UPPER_FACE, 2.0);
        assert!(!osk.shift_locked());
    }

    /// It does not cross to a face that does not exist (a guard for integrator `Custom` layouts).
    #[test]
    fn switch_face_ignores_out_of_range() {
        let mut osk = qwerty();
        osk.switch_face(99, 0.0);
        assert_eq!(osk.face(), LOWER_FACE);
    }

    /// The height target follows a growing screen (A5, "the interruption rule").
    #[test]
    fn height_follows_the_screen() {
        let tokens = MotionTokens::from_config(&MotionConfig {
            reduce: true,
            ..MotionConfig::default()
        });
        let mut osk = qwerty();
        let t0 = Instant::now();
        osk.update(true, OskMode::Auto, 600.0, t0, 0.016, &tokens);
        let small = osk.inset_bottom();
        osk.update(
            true,
            OskMode::Auto,
            1000.0,
            t0 + Duration::from_millis(16),
            0.016,
            &tokens,
        );
        assert!(
            osk.inset_bottom() > small,
            "{small} → {}",
            osk.inset_bottom()
        );
        assert!((osk.inset_bottom() - osk.height()).abs() < 0.5);
    }
}
