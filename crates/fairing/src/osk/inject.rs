//! Input injection: a key tap → `Event::Text` / `Event::Key` plus
//! `request_repaint()`. It is egui's official soft-keyboard path and takes no extra dependency.
//! Focus movement goes through a `Tab` key event (`Shift+Tab` for the previous) — a steadier API
//! than `memory_mut(focus_next)`.
//!
//! **Why not push straight into `ctx.input_mut`**: egui replaces `InputState::events` wholesale with `RawInput`'s
//! on each pass (`InputState::begin_pass`). So an event pushed mid-pass is seen only by widgets
//! **not yet drawn in that pass** and does not survive to the next one. The OSK is frame stage
//! 10 and draws after the screen's `TextEdit` (stage 9), so pushed as-is nobody sees it. And
//! `Tab`'s focus movement is decided by `Focus::begin_pass` from **`RawInput`'s** events only, so
//! a `Tab` pushed mid-pass has no effect at all.
//!
//! So events are queued in egui's store (`ctx.data`) and appended to **the next pass's
//! `RawInput::events`** in [`egui::Plugin::input_hook`], which egui calls just before
//! `begin_pass`. The queue `Vec` is drained rather than dropped, so from the second tap on there
//! is no heap allocation.

use super::layouts::KeyAction;
use egui::{Event, Id, Key, Modifiers, RawInput};

/// Where the injection queue lives (`ctx.data`).
fn queue_id() -> Id {
    Id::new("fairing.osk.inject.queue")
}

/// Whether the plugin has been registered (`ctx.data`).
fn installed_id() -> Id {
    Id::new("fairing.osk.inject.installed")
}

/// The egui plugin that appends the queue to the next pass's `RawInput`. All the state is in
/// `ctx.data`, so the plugin itself is an empty type (no lock types).
#[derive(Debug, Default, Clone, Copy)]
struct InjectPlugin;

impl egui::Plugin for InjectPlugin {
    fn debug_name(&self) -> &'static str {
        "fairing.osk.inject"
    }

    fn input_hook(&mut self, ctx: &egui::Context, input: &mut RawInput) {
        ctx.data_mut(|data| {
            let queue: &mut Vec<Event> = data.get_temp_mut_or_default(queue_id());
            if !queue.is_empty() {
                // `append` moves only the elements and leaves the capacity in the queue.
                input.events.append(queue);
            }
        });
    }
}

/// Register the plugin exactly once. `Context::add_plugin` does not take the same type twice,
/// but it makes an `Arc` on every call, so a flag in `ctx.data` is checked first.
fn install(ctx: &egui::Context) {
    if ctx.data(|data| data.get_temp::<bool>(installed_id())) == Some(true) {
        return;
    }
    ctx.add_plugin(InjectPlugin);
    ctx.data_mut(|data| data.insert_temp(installed_id(), true));
}

/// Inject one key action into the next pass. `Face` and `Hide` have nothing to inject, so
/// `false`.
///
/// The events are seen on **the next pass** (see the module docs) — which is why
/// `request_repaint()` goes with them. Even at rest, the next frame is guaranteed to run.
#[must_use]
pub fn inject(ctx: &egui::Context, action: &KeyAction) -> bool {
    let events: [Option<Event>; 2] = match action {
        KeyAction::Text(s) => [Some(Event::Text(s.to_string())), None],
        KeyAction::Space => [Some(Event::Text(" ".to_owned())), None],
        KeyAction::Backspace => key_pair(Key::Backspace, Modifiers::NONE),
        KeyAction::Enter => key_pair(Key::Enter, Modifiers::NONE),
        KeyAction::NextFocus => key_pair(Key::Tab, Modifiers::NONE),
        KeyAction::PrevFocus => key_pair(Key::Tab, Modifiers::SHIFT),
        KeyAction::Left => key_pair(Key::ArrowLeft, Modifiers::NONE),
        KeyAction::Right => key_pair(Key::ArrowRight, Modifiers::NONE),
        KeyAction::Face(_) | KeyAction::Lang | KeyAction::Hide => return false,
    };
    install(ctx);
    ctx.data_mut(|data| {
        let queue: &mut Vec<Event> = data.get_temp_mut_or_default(queue_id());
        for event in events.into_iter().flatten() {
            queue.push(event);
        }
    });
    ctx.request_repaint();
    true
}

/// Inject a composition result — **erase and retype** ([`super::compose::Compose`]).
///
/// It sends `erase` backspaces to take out the composing characters put in last time, and puts
/// `text` in as an [`Event::Text`]. The composing syllable goes into the buffer as **real
/// characters** too.
///
/// # Why not `ImeEvent::Preedit`
///
/// egui's `TextEdit` knows the `Preedit` contract (it removes the previous composing string and
/// inserts the new one) and even draws the composing underline, so that is what this used at
/// first. On a real device the symptom was **the composing characters not appearing at all and
/// only the committed ones showing up**, and the cause was two-layered.
///
/// 1. `Event::Ime` only reaches the widget **holding focus** on that frame
///    (`Event::Ime(ime_event) if owns_ime_events` in `builder.rs`). Pressing an OSK key
///    completes the click outside the field, so focus wavers for a frame and that frame's
///    `Preedit` is lost. `Commit` comes again the next frame and survives.
/// 2. Worse is **an empty `Preedit("")`**. As egui's own comment says, integration layers send
///    it with no composition in progress — winit does on every `set_ime_allowed` and
///    `set_ime_cursor_area`, and those happen whenever the caret moves, that is **on every
///    character typed**. An empty `Preedit` arriving mid-composition wipes the composing string
///    entirely.
///
/// Both are other people's circumstances that we cannot fix, and both apply only to `Preedit`.
/// So the composing characters go in as committed ones — the same path a physical keyboard
/// takes, so it behaves identically whatever the IME environment is. What is lost is the
/// composing underline; what is gained is **the characters being visible**.
///
/// The composition rules themselves are unchanged — what to stack and when to commit is
/// [`super::compose::Composer`]'s, and this only moves the result onto the screen.
#[must_use]
pub fn inject_compose(ctx: &egui::Context, erase: usize, text: &str) -> bool {
    if erase == 0 && text.is_empty() {
        return false;
    }
    install(ctx);
    ctx.data_mut(|data| {
        let queue: &mut Vec<Event> = data.get_temp_mut_or_default(queue_id());
        for _ in 0..erase {
            for event in key_pair(Key::Backspace, Modifiers::NONE)
                .into_iter()
                .flatten()
            {
                queue.push(event);
            }
        }
        if !text.is_empty() {
            queue.push(Event::Text(text.to_owned()));
        }
    });
    ctx.request_repaint();
    true
}

/// A press and release pair (widgets respond to `pressed: true`).
fn key_pair(key: Key, modifiers: Modifiers) -> [Option<Event>; 2] {
    let make = |pressed: bool| Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    };
    [Some(make(true)), Some(make(false))]
}

#[cfg(test)]
mod tests {
    use super::{inject, inject_compose, KeyAction};
    use egui::{Event, RawInput};
    use std::borrow::Cow;

    /// An event pushed mid-pass rides the **next pass's** `RawInput` to the widget. The text
    /// goes in even in the frame order where the screen draws before the OSK (9 → 10).
    #[test]
    fn text_reaches_a_widget_drawn_earlier_in_the_next_pass() {
        let ctx = egui::Context::default();
        let mut text = String::new();
        // Pass 1: the field takes focus and the injection happens after it (= at the OSK's place).
        ctx.run_ui(RawInput::default(), |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id_salt("f"))
                .request_focus();
            assert!(inject(&ctx, &KeyAction::Text(Cow::Borrowed("q"))));
        })
        .drop_without_applying_deltas();
        assert!(text.is_empty(), "not in yet on this pass");
        // Pass 2: even with the field drawn first, the event is already in `RawInput`.
        ctx.run_ui(RawInput::default(), |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id_salt("f"));
        })
        .drop_without_applying_deltas();
        assert_eq!(text, "q");
    }

    /// A composition result is **erased with ⌫ and retyped as characters.** Not using the IME
    /// events is the point — the composing syllable has to enter the buffer as real characters
    /// to be visible.
    #[test]
    fn a_composition_is_erased_and_retyped_as_plain_text() {
        let ctx = egui::Context::default();
        ctx.run_ui(RawInput::default(), |_ui| {
            // "ㅎ" was put in just before as one character, and it now becomes "하".
            assert!(inject_compose(&ctx, 1, "하"));
        })
        .drop_without_applying_deltas();
        let mut seen = Vec::new();
        ctx.run_ui(RawInput::default(), |_ui| {
            seen = ctx.input(|i| i.events.clone());
        })
        .drop_without_applying_deltas();
        let backspaces = seen
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::Key {
                        key: egui::Key::Backspace,
                        pressed: true,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            backspaces, 1,
            "takes back the one character composed just before: {seen:?}"
        );
        assert!(
            seen.iter()
                .any(|e| matches!(e, Event::Text(t) if t == "하")),
            "the composing syllable goes in as a real character: {seen:?}"
        );
        assert!(
            !seen.iter().any(|e| matches!(e, Event::Ime(_))),
            "no IME events: they are at the mercy of the focus and an empty Preedit: {seen:?}"
        );
    }

    /// With nothing to erase and nothing to type, it emits nothing.
    #[test]
    fn an_empty_composition_injects_nothing() {
        let ctx = egui::Context::default();
        ctx.run_ui(RawInput::default(), |_ui| {
            assert!(!inject_compose(&ctx, 0, ""));
        })
        .drop_without_applying_deltas();
    }

    /// A special key pushes a press/release pair; face switches and hide inject nothing.
    #[test]
    fn special_keys_push_a_press_release_pair() {
        let ctx = egui::Context::default();
        ctx.run_ui(RawInput::default(), |_ui| {
            assert!(inject(&ctx, &KeyAction::Backspace));
            assert!(!inject(&ctx, &KeyAction::Face(1)));
            assert!(!inject(&ctx, &KeyAction::Hide));
        })
        .drop_without_applying_deltas();
        let mut keys = 0;
        ctx.run_ui(RawInput::default(), |_ui| {
            keys = ctx.input(|i| {
                i.events
                    .iter()
                    .filter(|e| {
                        matches!(
                            e,
                            Event::Key {
                                key: egui::Key::Backspace,
                                ..
                            }
                        )
                    })
                    .count()
            });
        })
        .drop_without_applying_deltas();
        assert_eq!(keys, 2, "press + release");
    }

    /// `Tab` has to ride `RawInput` for `Focus::begin_pass` to move the focus.
    #[test]
    fn next_focus_moves_the_focus() {
        let ctx = egui::Context::default();
        let (mut a, mut b) = (String::new(), String::new());
        let mut first = egui::Id::NULL;
        ctx.run_ui(RawInput::default(), |ui| {
            first = ui.add(egui::TextEdit::singleline(&mut a).id_salt("a")).id;
            ui.add(egui::TextEdit::singleline(&mut b).id_salt("b"));
            ctx.memory_mut(|m| m.request_focus(first));
            assert!(inject(&ctx, &KeyAction::NextFocus));
        })
        .drop_without_applying_deltas();
        ctx.run_ui(RawInput::default(), |ui| {
            ui.add(egui::TextEdit::singleline(&mut a).id_salt("a"));
            ui.add(egui::TextEdit::singleline(&mut b).id_salt("b"));
        })
        .drop_without_applying_deltas();
        let focused = ctx.memory(egui::Memory::focused);
        assert!(
            focused.is_some() && focused != Some(first),
            "moves on to the next widget: {focused:?}"
        );
    }
}
