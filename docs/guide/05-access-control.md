# 05. Access control

## 0. The principle, and what exists today

fairing keeps no role names like "guest" and "admin", and no hash function, inside
the crate. How many levels there are, what they are called, which feature sits at
which level, and how credentials are stored and checked — all of that is yours.
What the crate does is **enforce** the gate decision, and — if you let it — draw
the prompt that asks for a credential. This page is about what the crate does for
you.

| The crate does | You decide |
|---|---|
| Gate enforcement: `Shell::launch` as the single point, setting writes, the desktop, tiles, the shade | The level table and which gate sits at which level (`[access]`, `[access.gates]`) |
| The unlock prompt and the lock screen — a PIN pad, a password card, a badge-reader wait (§5) | What a credential is and how it is checked: an [`Authenticator`](#5-the-shells-prompt-and-the-authenticator), or the reference `PinTable` |
| The session: temporary unlocks, the session timeout, the idle lock (§7) | Whether any of them is on, and for how long |
| `AccessEvent` for every request, grant, refusal, lockout and session change (§8) | Where the audit log goes |

Three modes decide how much of that the shell does:

| `[access] mode` | A failed gate | Who authenticates |
|---|---|---|
| `prompt` (the default) | `AccessEvent::UnlockRequested`, **and the shell's prompt opens** | Your `Authenticator` — or `PinTable`, built from `[access.pin_table]` |
| `routing` | `AccessEvent::UnlockRequested`, and nothing is ever drawn | You, entirely (§6) |
| `off` | Nothing fails — every gate passes | Nobody |

`prompt` needs something to ask. With no `[access.pin_table]` PINs and no
`ShellBuilder::authenticator`, there is nothing to draw and it behaves as
`routing` does — the event, and no prompt with nothing behind it. A single-level
table never prompts at all.

## 1. The level table

```toml
[access]
mode = "prompt"                                 # off | prompt | routing
levels = ["viewer", "operator", "maintainer"]   # low to high; names are yours
initial = "viewer"                              # starting level; omitted means the lowest
default_gate = "top"                            # unassigned gates: top | bottom | <level name>
```

`levels` becomes `Level(u16)` indices. `Subject.level` is just that index — the
crate never assumes `Level(0)` means "guest". Three configuration errors are
caught at start-up.

| Error | When |
|---|---|
| `levels` is empty | `[access] levels = []` |
| Two or more levels with no `default_gate` | `levels.len() >= 2` and `default_gate` omitted. With a single level everything passes anyway, so the check is skipped |
| `default_gate`, `initial` or an `[access.gates]` value is not in `levels` | Typo protection. `"top"` and `"bottom"` are reserved only inside `default_gate`, so a level may safely share the name |

All three make `Shell::new` / `Shell::builder(..).build(ctx)` return
`Err(Error::Config(..))`. The device refuses to start rather than come up with the
wrong level table.

```rust
use fairing::ShellConfig;

fn bad_config_is_rejected() {
    let cfg = ShellConfig::from_toml(
        r#"
[access]
levels = ["viewer", "maintainer"]
"#, // no default_gate, and there are two levels - an error
    )
    .expect("the TOML itself is valid");
    let ctx = egui::Context::default();
    let services = fairing::Services::builder().build();
    assert!(fairing::Shell::new(cfg, services, &ctx).is_err());
}
```

With one level, `Access::allows` returns early and **every gate passes**,
regardless of `mode`. A device that needs no authentication says so in one line:

```toml
[access]
levels = ["default"]   # no authentication; mode is irrelevant
```

## 2. Gates

### Naming

- Built-in features use the fixed names in §2.1.
- Every screen, action, status item, nav item and tile you declare **uses its own
  id as its gate name** by default. `.gate("shared-name")` lets several items
  share one gate. The declaration APIs themselves are in
  [02 Screens](02-screens.md) and [03 Chrome](03-chrome.md); this page is only
  about the gate side.
- No gate (`gate = None`) does not mean "always allowed" — it means **"use the id
  as the gate"**. For genuinely always-allowed, assign it to the lowest level in
  `[access.gates]`, or arrange for `default_gate = "bottom"` to catch it.

### 2.1 Built-in gates

Three things to know per gate: whether the crate actually **enforces** it, whether
an unassigned one gets a start-up `log::info!` (**warned**), and whether the
matching screen or feature exists at all.

| Gate | Enforced | Warned | Notes |
|---|:---:|:---:|---|
| `overlay.open` | ✔ | ✔ | Opening the shade. `LaunchAction::OpenOverlay`, the edge swipe and the status-bar tap all pass through this one gate |
| `chrome.emergency` | ✔ | ✔ | The two-second top-corner long press that survives an edge guard |
| `status.clock`, `status.wifi`, `status.bluetooth`, `status.battery`, `status.notifications`, `status.user`, `status.lock`, … | ✔ (render filter) | ✔ | Built-in status-bar item ids. Under `default_gate = "top"`, forgetting to assign these empties the whole bar — which is why they usually go to `bottom`. `status.ethernet`, `status.volume` and `status.brightness` also need a backend that reports the capability (see §2.2) |
| `tile.wifi`, `tile.bluetooth`, `tile.brightness`, `tile.lock`, … (all of `overlay::BUILTIN_IDS`) | ✔ (render filter) | ✘ | Quick-settings tile ids. **Unlike status items these are not in the warning list** — assign one wrong and it just disappears |
| `settings.wifi`, `settings.bluetooth`, `settings.display`, `settings.sound`, `settings.locale`, `settings.power`, `settings.network`, `settings.network.edit`, `settings.datetime`, `settings.datetime.set`, `settings.credentials` | ✔ | ✔ | These screens **do exist** — they ship with the crate behind the `settings` feature (`settings.credentials` only where the authenticator manages its entries, §5.4), but you **opt in** by calling `fairing::settings::add_all(&mut shell, &SettingsConfig::default())`. Narrow the set with `SettingsConfig::only([..])` or `.without(id)`, or replace one wholesale by declaring your own screen with the same id. A screen whose backend reports no capability is skipped |
| `nav.recents`, `workspace.split` | ✔ | ✔ | The recent screens, and **entering** a split ([03 §2.3](03-chrome.md#23-recent-screens-and-the-split)). Short of one, a press asks for an unlock; short of `workspace.split`, the cards carry no split buttons and `cx.open_in_other_pane` opens in the same pane. Going back from the cards, or to one pane, is never gated |
| `desktop.edit` | ✘ (the feature is M6) | ✔ | Desktop editing. Reserved — there is nothing to tap yet |
| `session.lock`, `session.logout` | ✘ | ✔ | Reserved names. Locking and logging out are always allowed; the controls that do them carry gates of their own (`tile.lock`, `status.lock`). An unlock from the lock screen reports `session.lock` (§8) |

`settings.home` and `settings.about` have no gate on purpose — a list screen shows
only the entries you may reach anyway, and the about screen carries legal notices,
so it always opens.

### 2.2 `Capabilities` and gates are different questions

A gate asks "may this level use the feature". `Capabilities`
([06 §2](06-services.md#2-capabilities--declaring-what-works)) asks "does this
device have it". They act independently: assign `status.wifi` to the lowest level
and the item still hides if `WifiBackend::capabilities()` does not report
`Capabilities::WIFI`.

### 2.3 Where enforcement happens

| Target | The code that enforces it |
|---|---|
| Opening a screen (icon tap, `ShellHandle::launch`, `cx.open`) | `Shell::launch` → `open_screen`, one place. Failing means it does not open: you get `UnlockRequested`, and the prompt in `prompt` mode |
| Writing a setting (`LaunchAction::Set` / `Toggle`) | `Shell::setting_allowed` — the gate name is the `SettingKey` string itself |
| Opening the shade | The `overlay.open` gate, through `Shell::launch(OpenOverlay)` |
| Quick-settings tiles | `refresh_tile_states` filters the display every frame with `access.allows(tile.gate_name())`; a tap becomes `OverlaySlotAction::TileLocked` → `UnlockRequested` |
| Desktop icons | `DesktopAction::TapLocked` → `UnlockRequested`; the desktop does the render filter |
| The emergency gesture | Opens the shade or emits `UnlockRequested`, depending on `chrome.emergency` |
| After the subject changes | `Shell::propagate_access_change` closes every open instance that can no longer pass its gate and queues `Lifecycle::AccessChanged` on the rest ([02 Screens](02-screens.md)) |

One rule holds it together: **hiding is presentation, blocking is enforcement.**
The render-stage filter — the padlock, the greyed-out icon — is UX. Whether
something actually runs is re-checked at the enforcement point, so a deep-link
string or a direct `cx.open` cannot slip past it.

## 3. `Visibility` — how a failed gate looks

```rust,ignore
pub enum Visibility {
    Hidden,   // not rendered at all
    Locked,   // the default: greyed out with a padlock; a tap asks for an unlock (§5)
}
```

Screens and actions take `.visibility(Visibility::Hidden)`; the default is
`Locked`. Use `Hidden` for things a guest should not even know about — service
tools — and leave `Locked` when "you can see it exists, you just cannot use it" is
the message.

## 4. Writing your own `AccessPolicy`

The default `LevelPolicy` is one ordered comparison: pass if the level assigned to
the gate is at or below the subject's level. For time-of-day policies, ACLs keyed
on `Subject.attrs`, or capability sets, implement `AccessPolicy` yourself. The
trait lives in `fairing::access` and is not re-exported at the root.

```rust
use fairing::access::{AccessPolicy, DefaultGate, Gate, Level, LevelPolicy, LevelTable, Subject};
use fairing::i18n::LabelKey;
use std::collections::BTreeMap;

/// Keep the ordered comparison, but also close the power screen out of hours.
struct BusinessHoursPolicy {
    inner: LevelPolicy,
}

impl AccessPolicy for BusinessHoursPolicy {
    fn allows(&self, subject: &Subject, gate: &Gate) -> bool {
        if gate.as_str() == "settings.power" && !is_business_hours() {
            return false;
        }
        self.inner.allows(subject, gate)
    }

    fn hint(&self, gate: &Gate) -> Option<LabelKey> {
        self.inner.hint(gate) // let the default policy word the padlock hint
    }
}

fn is_business_hours() -> bool {
    // Judge it with whatever clock you like - asking SystemClock directly is fine.
    true
}

// Building a LevelPolicy by hand needs a LevelTable, the gate map and a DefaultGate.
// Usually it is easier to wrap the one Shell::new already built.
fn custom_policy() -> BusinessHoursPolicy {
    let table = LevelTable::from_names(&["viewer".to_owned(), "maintainer".to_owned()]);
    let mut gates = BTreeMap::new();
    gates.insert("settings.power".to_owned(), Level(1));
    BusinessHoursPolicy {
        inner: LevelPolicy::new(table, gates, DefaultGate::Bottom),
    }
}
```

Install it with `access_mut()`:

```rust
# use fairing::access::{AccessPolicy, Subject};
# struct BusinessHoursPolicy;
# impl AccessPolicy for BusinessHoursPolicy {
#     fn allows(&self, _: &Subject, _: &fairing::Gate) -> bool { true }
# }
# fn custom_policy() -> BusinessHoursPolicy { BusinessHoursPolicy }
# fn install(shell: &mut fairing::Shell) {
shell.access_mut().set_policy(Box::new(custom_policy()));
# }
```

To use a level table other than the one `fairing.toml` built, just hold your own
inside the policy — the shell delegates the whole decision, so whether
`[access.gates]` matters at all is up to you.

## 5. The shell's prompt and the `Authenticator`

In `mode = "prompt"` a failed gate opens a modal over the screen. It draws only
what the authenticator offers, hands what was entered to it, and acts on the
answer. It follows the palette and the tokens, and two painters redraw what the way in stands
on — the lock screen's ground and clock, the prompt's backdrop and card
([04 §10.6](04-customization.md#106-the-lock-screen-and-the-unlock-prompt)). The way in is still
the shell's, drawn over them, and so is everything this section describes. For a flow of your
own, use `routing` (§6) and build the screen from `PinPad` and `PatternPad`.

```text
tap a locked item
   └─▶ policy.allows() = false
   └─▶ AccessEvent::UnlockRequested { gate, then }      (always — for the audit log)
   └─▶ the prompt: Authenticator::methods() → a keypad, a pattern, a password card, a badge wait
           Authenticator::submit(credential, now)
             Granted(subject) → the session takes it (§7) → `then` runs through Shell::launch again
             Denied { message } → the message in the title's place, the card shakes, try again
             Locked { until, message } → the keypad greys out and counts down
             Pending → "Checking…" until Authenticator::poll(now) answers; the keys still work,
                       and a new attempt calls Authenticator::cancel on it first
```

What was typed goes to `submit` **by value** and the shell keeps no copy — not in
a log, an event or its own state. `Credential`'s `Debug` prints the shape and never
the contents. A refusal is shown in the authenticator's own words: whether to say
that a user does not exist is your call, so the shell never words one.

The prompt sits above the shade and the bars, takes every press that misses it,
and pauses the focused screen (`Lifecycle::Paused`, then `Resumed`). Back cancels
it. Behind it, no new edge swipe starts.

A few rules hold whatever the authenticator does:

- **A grant has to be worth something.** A `Granted` level the table does not
  have, or — outside the lock screen — one no higher than the session already
  holds, is answered as a refusal ("That is not enough for this"). A valid but
  lower credential never demotes the session or closes its screens.
- **A lockout stays.** Closing the prompt and opening it again, or a second
  request arriving over an open one, keeps the countdown and the dead keys — for a
  password and a badge as much as a keypad. Nothing is submitted while locked.
- **A wait is one flag.** `Pending` puts it up and the title says "Checking…"; it
  comes down with the answer, or with `Authenticator::cancel` — when the prompt
  goes without an answer, when the lock screen comes up over it, or when a new
  attempt goes in. The keys are never held for it, so an answer that never comes
  holds nothing: the next attempt is the way past it, on the lock screen too.
  The shell sets no time limit of its own (see §5.2). Whatever `poll` reports is
  applied as it comes — the shell follows the authenticator's results — so the
  answer to a check `cancel` called off is the authenticator's to drop.
- **A request behind the lock screen waits for it.** Once the lock screen goes
  (an unlock, Continue, or your own `set_subject`), the request is asked again: it
  runs where the session now passes its gate, and the prompt opens where it does
  not. A new lock starts with nothing waiting.
- **`begin(gate, now)` comes first.** The shell calls it each time the prompt
  opens — the lock screen's gate is `session.lock` — so an authenticator can start
  listening, and drop whatever a reader picked up while no prompt was up.
- **An authenticator that offers nothing** (`methods()` empty) behaves as
  `routing` (§6): no prompt and no lock screen, and the lock and logout controls
  are only reported.
- **What the pads cannot draw is logged.** A `Pin` `len` or `max_len` above 16, a
  `Pattern` `grid` outside 3 to 5 or a `min_points` over the dots there are: the
  pad draws the nearest it can, and the first prompt with those methods says so
  with a `log::warn!`.
- The prompt's words — the titles, the tabs, Cancel, "Checking…", the countdown,
  the lock screen's date — pass through `Strings` like the rest of the shell's
  ([04 §9](04-customization.md#9-text-and-translations)). The refusals are the
  authenticator's own words, looked up as keys too: `PinTable`'s "Wrong PIN" reads
  "PIN이 틀렸어요" on a Korean panel, and yours read as written until you give them
  entries with `ShellBuilder::translations`.

### 5.1 `PinTable` — the reference

```toml
[access]
mode = "prompt"
levels = ["viewer", "operator", "maintainer"]
default_gate = "top"

[access.pin_table]   # level name = PIN, in clear text
operator = "1234"
maintainer = "987654"
attempt_limit = 5    # optional: this many wrong in a row lock the prompt...
lock_secs = 60       # ...for this long (60 when left out)
shuffle = true       # optional: a fresh digit layout each time the prompt opens
max_len = 8          # optional: the most digits a PIN may have (16 when left out)

[access.pattern_table]          # level name = pattern, in clear text
maintainer = "1-2-3-5-7-8-9"    # the dots in the order drawn, row by row from 1
grid = 3                        # optional: dots on a side, 3 to 5
min_points = 4                  # optional: the fewest dots a pattern has
show_path = true                # optional: draw the path as the finger draws it
```

- A right PIN or pattern grants `Subject { id: None, level }` for its level. An
  operator PIN on a maintainer gate is still a grant — the session goes up to
  operator, the launch is checked again, and the prompt asks again.
- A level may have a PIN, a pattern or both; the prompt has a tab for each kind
  the tables hold, PIN first.
- PINs are 1 to `max_len` digits. Two levels sharing a PIN, or a key that is not a
  level, is an `Error::Config` at start-up.
- When every PIN has the same length the keypad submits on the last digit;
  otherwise its ✓ key submits, and it takes no more than `max_len` digits.
- A pattern is its dots in the order drawn, numbered row by row from 1, between
  `-`, `,` or spaces; on the 3 × 3 grid the digits alone do (`"1235789"`). It has to
  be one a finger can draw: a stroke across a dot takes that dot, so `"1-3-6-9"`
  cannot be drawn — the finger records `"1-2-3-6-9"`, and the start-up error says
  so. A dot twice, a dot off the grid, fewer than `min_points` dots, or two levels
  sharing a pattern is an `Error::Config` too.
- Wrong PINs and wrong patterns count against **one** attempt limit — two ways in
  are not two allowances. Either table may set `attempt_limit` and `lock_secs`;
  where both do, the stricter limit and the longer lockout hold. The count lives
  in memory, and a restart clears it.
- The file holds the secrets in clear text, and the shell says so once at start-up
  with a `log::warn!`. Keeping that file unreadable is yours — or replace the table
  with an authenticator of your own.

The fixed keys — `attempt_limit`, `lock_secs`, `shuffle` and `max_len` in the PIN
table, `grid`, `min_points`, `show_path`, `attempt_limit` and `lock_secs` in the
pattern table — are the tables' own, so no level can be named after one of them.

### 5.2 Your own `Authenticator`

Hashing, accounts, a lockout that survives a restart, a TPM, a server: an
`Authenticator` of yours, handed to the builder, takes `PinTable`'s place.

```rust
use fairing::access::{AuthMethod, AuthOutcome, Authenticator, Credential, Subject};
use fairing::Level;
use std::time::{Duration, Instant};

/// Accounts checked however the device likes, with a lockout of its own.
struct Accounts {
    fails: u32,
}

impl Authenticator for Accounts {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::Password { needs_user: true }]
    }

    fn submit(&mut self, credential: Credential, now: Instant) -> AuthOutcome {
        let Credential::Password { user: Some(user), secret } = credential else {
            return AuthOutcome::Denied { message: "Enter a user and a password".into() };
        };
        // argon2, a TPM, a server — the crate takes no part in it.
        if let Some(level) = my_verify(&user, &secret) {
            self.fails = 0;
            return AuthOutcome::Granted(Subject { id: Some(user), level, ..Subject::default() });
        }
        self.fails += 1;
        if self.fails >= 5 {
            self.fails = 0;
            // `now` is the shell's clock — the one a lockout has to run on.
            return AuthOutcome::Locked {
                until: now + Duration::from_secs(60),
                message: "Too many attempts".into(),
            };
        }
        AuthOutcome::Denied { message: "Wrong user or password".into() }
    }
}
# fn my_verify(_user: &str, _secret: &str) -> Option<Level> { None }
# fn build(ctx: &egui::Context) -> fairing::Result<fairing::Shell> {

let shell = fairing::Shell::builder(fairing::ShellConfig::default())
    .authenticator(Accounts { fails: 0 })
    .build(ctx)?;
# Ok(shell)
# }
```

`submit` runs on the UI thread inside a frame, so it must return at once. A check
that takes time answers `Pending`, and the shell calls `poll(now)` every frame
while the prompt is up. It sets no limit on how long that takes: a server that
ought to answer within ten seconds is yours to time, by answering `Denied` from
`poll` when the time runs out. Everything here — `submit`, `poll`, `cancel` — is
called on the UI thread, so there is nothing to race: once `cancel` returns, the
check it stopped is over. `Shell::set_authenticator` swaps one in while the shell
runs; a prompt up at the time — the lock screen included — opens again for the new
one, with its methods and without the old one's lockout or a wait it owed, and the
old one is told to `cancel` first.

### 5.3 What each method draws

| `AuthMethod` | The prompt |
|---|---|
| `Pin { len, max_len, shuffle }` | The [`PinPad`](#55-a-keypad-or-a-pattern-in-a-login-screen-of-your-own) with its dot row; the bottom row is ⌫ · 0 · ✓, and ⌫ and ✓ dim while there is nothing to take off or submit. `len > 0` submits on the last digit (✓ submits early); with `0` the dots count what went in, ✓ submits, and the pad takes at most `max_len` digits (`0` is 16). A hardware keypad's digits, Backspace and Enter work too |
| `Pattern { grid, min_points, show_path }` | The [`PatternPad`](#55-a-keypad-or-a-pattern-in-a-login-screen-of-your-own): `grid` × `grid` dots. A path under `min_points` is answered on the spot ("Connect at least 4 dots") and never reaches `submit`; a refused one stays on the dots in red until the next stroke. `show_path: false` keeps the path off the glass. It arrives as `Credential::Pattern(dots)`, the dots counted row by row **from 0** |
| `Password { needs_user }` | A user field (where asked for), a secret field and Unlock; Enter in the secret submits. The on-screen keyboard comes up **above** the prompt |
| `External { label }` | `label` and a ring going round (redrawn four times a second, not every frame), until `poll` answers. A reader that types like a keyboard — most badge readers do — needs nothing more: what it types is submitted on Enter or Tab as `Credential::External(bytes)`, and a pause of more than 0.8 s between characters starts a new read, so half a swipe does not spoil the next one |

Several methods show as tabs, labelled `PIN`, `Pattern`, `Password` and the
external method's `label`; the card keeps one size whichever tab is on show. The
small line at the top names the gate: it is `AccessPolicy::hint` — the level's
label, with the default policy. Under it the title says what to do ("Enter PIN",
"Draw pattern"), and what just happened takes its place — the refusal in red, the
lockout's countdown, "Checking…" — so nothing below it moves. The way out (Cancel,
or the lock screen's Continue) is a row under the keys, as wide as they are.

A reader on a thread of its own answers through `poll`, and wakes the UI with the
`Waker` it is handed — the same one every backend gets:

```rust
use fairing::access::{AuthMethod, AuthOutcome, Authenticator, Credential, Gate};
use fairing::services::Waker;
use std::sync::mpsc::Receiver;
use std::time::Instant;

struct Badges {
    /// Badge numbers from the reader thread.
    read: Receiver<String>,
}

impl Authenticator for Badges {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::External { label: "Badge".into() }]
    }

    fn begin(&mut self, _gate: &Gate, _now: Instant) {
        // A badge swiped while nothing asked belongs to nobody: without this, the next
        // prompt — hours later, someone else's — would open on it.
        while self.read.try_recv().is_ok() {}
    }

    fn submit(&mut self, credential: Credential, _now: Instant) -> AuthOutcome {
        match credential {
            Credential::External(bytes) => check(&String::from_utf8_lossy(&bytes)),
            _ => AuthOutcome::Pending,
        }
    }

    fn poll(&mut self, _now: Instant) -> Option<AuthOutcome> {
        self.read.try_recv().ok().map(|badge| check(&badge))
    }

    fn attach(&mut self, waker: Waker) {
        // Give this to the reader thread: `waker.wake()` after each badge it sends.
        let _ = waker;
    }
}
# fn check(_badge: &str) -> AuthOutcome { AuthOutcome::Pending }
```

### 5.4 `settings.credentials` — when the authenticator manages its entries

An authenticator that returns `Some(..)` from `admin()` gets the built-in
`settings.credentials` screen (behind the gate of the same name) from
`fairing::settings::add_all`. It lists `CredentialAdmin::list()`, and changes an
entry's level, sets a new secret (typed twice), adds an entry and removes one
(held, not tapped). Every change is a call on your `CredentialAdmin`, and its
`Err(line)` is what the screen shows — the screen keeps no store of its own.

```rust,ignore
pub trait CredentialAdmin {
    fn set_actor(&mut self, subject: &Subject) {}   // who the next calls are by (default: ignored)
    fn list(&self) -> Vec<CredentialEntry>;          // { id, label, level, disabled }
    fn set_secret(&mut self, id: &str, credential: Credential) -> AdminResult;
    fn set_level(&mut self, id: &str, level: Level) -> AdminResult;
    fn add(&mut self, id: &str, level: Level, credential: Credential) -> AdminResult;
    fn remove(&mut self, id: &str) -> AdminResult;
}
```

**Nobody changes more than they have.** The level picker offers the levels up to
the session's own; an entry above it is listed but cannot be opened; and the shell
refuses, before your code sees it, a change that gives a higher level or touches a
higher entry — so a maintainer cannot add an administrator, or set the
administrator's PIN and use it. (With one level, or `mode = "off"`, levels mean
nothing and this does not apply.) Before each Apply, `set_actor` gets the
session's `Subject`: rules of your own about who may change what, and an audit
trail, start there.

**A form stays up until it is answered.** Apply hands its calls over — a new level
and a new secret are two — and the form waits for them: when everything went
through it closes, and the list says what was done; when something did not, the
form stays with everything typed, a line for each call that was refused and why,
and a line for each that went through. The form is the screen's own and goes
with it: leaving the screen, being covered or a new subject drops it and what was
typed in it, and Back closes the form before it leaves the screen.

The form sets each kind of secret your `methods()` offer, with a tab to pick when
there are several: a PIN (typed twice, no longer than the method's `max_len`, and
exactly `len` digits where the prompt takes a fixed length) arrives as
`Credential::Pin`, a pattern (drawn twice) as
`Credential::Pattern(dots)`, a password as
`Credential::Password { user: Some(id), .. }`. A badge (`External`) is enrolled at
its reader, not here; with nothing else on offer the form asks for a password.
`PinTable` has no `admin()`: its file is the source of truth, so the screen does
not appear for it.

### 5.5 A keypad or a pattern in a login screen of your own

In `routing` mode the login screen is yours, and the keypad does not have to be.
`fairing::widgets::PinPad` is the one the prompt uses. It types into your buffer
and nowhere else:

```rust
use fairing::widgets::PinPad;
# fn body(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>, pin: &mut String) {

let pad = PinPad::new(pin).len(4).keyboard(true).show(ui, &mut cx.widgets());
if pad.submitted {
    let entered = std::mem::take(pin);
    // Check it however you like, then cx.shell.set_subject(..).
#   let _ = entered;
}
# }
```

`PinPad::shuffled(seed)` makes a fresh digit order to pass to `.order(..)` when
your screen opens, `.max_len(n)` caps a PIN of unknown length, and
`PinPadResponse::digit_rect(d)` says where a key was drawn — for a test that
presses it the way a finger would.

`fairing::widgets::PatternPad` draws into a `Vec<u8>` of dots the same way:

```rust
use fairing::widgets::PatternPad;
# fn body(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>, path: &mut Vec<u8>) {

let pad = PatternPad::new(path).grid(3).min_points(4).show(ui, &mut cx.widgets());
if pad.too_short {
    // Say "Connect at least 4 dots" — nothing was submitted.
}
if pad.submitted {
    let drawn = std::mem::take(path); // the dots from 0, row by row
#   let _ = drawn;
}
# }
```

A stroke across a dot takes it on the way, and the stroke is followed through every
pointer position a frame carried, so a quick one on a slow frame takes what it
crossed. `PatternPad::as_drawn(dots, grid)` gives the path a finger drawing through
`dots` would record — what a stored pattern can be checked against — and
`.mark(Some(ColorRole::Danger))` draws a refused path in red.

## 6. Doing authentication yourself with `routing`

In `mode = "routing"` the crate does gate decisions and session storage, nothing
else. A blocked launch arrives as one event instead of a prompt.

```text
tap a locked item
   └─▶ policy.allows() = false
   └─▶ ShellEvent::Access(AccessEvent::UnlockRequested { gate, then })
           you: authenticate however you like - a login screen, a card reader, a server
           you: handle.set_subject(Subject { id, level, attrs }) to swap the session
           you: if `then` is Some, handle.launch(then) - it re-enters Shell::launch and is re-checked
```

The whole thing, handled wherever you call `poll_events()` each frame (see
"When you are not using the runner" in
[01 §4](01-getting-started.md#4-a-minimal-app) if you host egui yourself):

```rust
use fairing::access::{AccessEvent, Level, Subject};
use fairing::ShellEvent;
use std::collections::BTreeMap;
# fn on_events(shell: &mut fairing::Shell) {

// Every frame, after shell.frame(ui).
for event in shell.poll_events() {
    if let ShellEvent::Access(AccessEvent::UnlockRequested { gate, then }) = event {
        // Yours: open a PIN dialog as a screen, wake a card-reader thread, call a
        // remote server. This example is synchronous.
        if let Some((id, level)) = my_own_login_ui(&gate) {
            shell.handle().set_subject(Subject {
                id: Some(id),
                level,
                attrs: BTreeMap::new(),
            });
            if let Some(action) = then {
                shell.handle().launch(action); // goes through the gate check again
            }
        }
    }
}
# }

fn my_own_login_ui(_gate: &fairing::Gate) -> Option<(String, Level)> {
    // Your call: a screen, polling a card reader, an HTTP request.
    None
}
```

`set_subject` makes the shell emit `AccessEvent::SessionChanged { from, to, reason: Integrator }` (§8),
close every open screen the new subject cannot reach, and notify the rest with
`Lifecycle::AccessChanged` — what a screen should do about that is in
[02 Screens](02-screens.md). `AccessPolicy`, `LevelTable` and `Visibility` all
work the same under `routing`. The only difference is who holds the credentials.

`off` skips the flow entirely: like a single-level table, every gate passes.

## 7. The session — temporary unlocks, the timeout, the idle lock

```toml
[access]
unlock_mode = "temporary"   # switch | temporary
temporary_secs = 300        # a temporary unlock lasts this long, counted from the unlock
session_timeout_secs = 0    # no input this long → back to the starting subject (0 = off)
idle_lock_secs = 0          # no input this long → the panel locks (0 = off)

[access.lock_screen]
allow_continue = false      # the lock screen's Continue: leave without a PIN, as the starting subject
```

| Setting | What it does |
|---|---|
| `unlock_mode = "switch"` | A grant **becomes** the session, until a logout, the timeout or `set_subject` lowers it |
| `unlock_mode = "temporary"` | A grant holds for `temporary_secs` **from the unlock**, then the subject from before comes back by itself (`ChangeReason::ElevationExpired`). Unlocking again on top keeps the first one's way back. For "until the panel is left alone", use `switch` with `session_timeout_secs` instead |
| `session_timeout_secs` | No input for this long takes the session back to `[access] initial` (`ChangeReason::Timeout`). Any touch, key or pointer movement starts it again |
| `idle_lock_secs` | No input for this long locks the panel: the session goes back to its start (`ChangeReason::Lock`), `ShellEvent::LockRequested` goes out, and in `prompt` mode with an authenticator the **lock screen** comes up — the clock beside (or above) the way in. In `routing` mode the event is yours to answer with a lock screen of your own |

On every way down — a temporary unlock running out, the timeout, a lock, a logout,
`set_subject` to a lower level — screens that no longer pass their gate close,
the rest get `Lifecycle::AccessChanged`, and the shade and an unlock prompt close
too, and so do the recent screens where the session no longer passes the gate
they came up behind. The timers wake the shell on time on a panel nobody touches;
nothing has to repaint to notice them.

**The timers run on the wall clock.** A reactive panel draws nothing while
nothing moves, and the time it sleeps counts: a five-minute temporary unlock ends
five minutes after the unlock however few frames were drawn in between, and so
do the timeout, the idle lock and a lockout's countdown. A timer longer than a
year is a config error at start.

**Every way up starts the idle stretch again** — an unlock, and an integrator's
`set_subject`, as a touch would: a badge reader that grants after the panel sat
idle is not timed out or idle-locked in the same frame. An integrator's
`set_subject` above `[access] initial` also takes the lock screen away, with
`LockScreenToggled(false)` — it vouched for someone, so the lock has done its
work.

**An answer that never comes does not hold the prompt.** The keys work while
"Checking…" is up, so the next attempt calls the stalled check off and goes in —
on the lock screen too, which has no Cancel (§5, "A wait is one flag").

**The lock and logout controls do something in `prompt` mode.** `tile.lock`, the
shade footer's lock button and `LaunchAction::Lock` bring up the lock screen;
`LaunchAction::Logout` and `ShellHandle::logout()` take the session back to its
start. Both are still reported as `ShellEvent::LockRequested` /
`LogoutRequested`. In `routing` mode (or with no authenticator) they are only
reported, as [03 §2.4](03-chrome.md#24-controls-the-shell-draws-but-does-not-act-on)
describes: what locking means there is yours.

`status.lock` in a status-bar slot shows an open padlock while the session is
above its start; a tap is a logout. Give it a gate everyone passes
(`"status.lock" = "bottom"`).

`[shell] idle_lock_secs` was the key's old home. It is still read where
`[access] idle_lock_secs` is 0, with a warning at start-up — move it.

## 8. Audit logging with `AccessEvent`

```rust,ignore
pub enum AccessEvent {
    UnlockRequested { gate: Gate, then: Option<LaunchAction> },  // a gate failed (every mode)
    Unlocked { gate: Gate, subject_id: Option<String>, level: Level, mode: UnlockMode },
    Denied { gate: Gate },                                       // the authenticator said no
    Locked { gate: Gate, until: Instant },                       // ...and no more tries until `until`
    SessionChanged { from: Level, to: Level, reason: ChangeReason },
    LockScreenToggled(bool),                                     // the shell's lock screen came up / went
}
pub enum ChangeReason { Unlock, Logout, Timeout, ElevationExpired, Lock, Integrator }
```

They arrive as `ShellEvent::Access(..)`. The shell only emits; writing to a file
or shipping to a server is yours. No credential value is ever in an event — only
`Subject.id` and `Level`. An unlock from the lock screen reports the gate
`session.lock`.

```rust
# use fairing::ShellEvent;
# struct AuditLog;
# impl AuditLog { fn write(&self, _line: String) {} }
# fn on_events(shell: &mut fairing::Shell, audit_log: &AuditLog) {
for event in shell.poll_events() {
    if let ShellEvent::Access(access_event) = &event {
        audit_log.write(format!("{:?} @ {:?}", access_event, std::time::SystemTime::now()));
    }
}
# }
```

A run of wrong PINs reads `UnlockRequested`, `Denied`, `Denied`, `Locked`, then —
once it runs out — `Unlocked` and `SessionChanged { reason: Unlock }`.

---

## Hidden entry points — the service menu

Some screens cannot have a desktop icon: the service menu, factory mode,
diagnostics. Android's "tap the build number seven times" is the conventional
answer; industrial HMIs more often use corners in sequence; devices with physical
keys use a key combination.

> **A knock is concealment; the gate is what blocks.** It makes the door hard to
> find, not hard to open. **Always put a gate on anything irreversible.**

**The crate does not choose the trigger.** It gives you one `KnockTrigger` trait
and ships two common implementations. All three plug into the same call:

```rust
# use fairing::access::{HiddenEntry, TapKnock};
# use fairing::LaunchAction;
# fn add(shell: &mut fairing::Shell) {
# let (id, any_trigger, action) = ("service", TapKnock::new(7), LaunchAction::open("service_menu"));
shell.add_hidden_entry(HiddenEntry::new(id, any_trigger, action));
# }
```

### Taps — the screen pokes from its own secret spot

```rust
# fn add(shell: &mut fairing::Shell) {
use fairing::access::{HiddenEntry, TapKnock};
use fairing::LaunchAction;

shell.add_hidden_entry(
    HiddenEntry::new("service", TapKnock::new(7), LaunchAction::open("service_menu"))
        .gate("service")     // even after knocking, authentication applies
        .hint_from(3),       // start hinting with three left
);
# }
```

**You decide where to knock.** The shell does not know what was tapped — that is
the point: every device puts it somewhere else, and that is the concealment.

```rust
# use fairing::{screen, Cx};
# let _ =
screen("about", |ui: &mut egui::Ui, cx: &mut Cx| {
    let model = ui.add(egui::Label::new("Model ACME-7").sense(egui::Sense::click()));
    if model.clicked() {
        cx.knock("service");
    }
    // The crate draws no hint. Read the count and render it your way.
    if let Some(left) = cx.knock_remaining("service") {
        if (1..=3).contains(&left) {
            ui.small(format!("{left} more to open"));
        }
    }
})
# ;
```

### Coordinates — no widget needed

A `Zone` is in **screen fractions** (0..=1), so it lands in the same place at any
resolution. `ZoneKnock::corners` builds a corner sequence for you.

```rust
# use fairing::LaunchAction;
# fn add(shell: &mut fairing::Shell) {
use fairing::access::{Corner, HiddenEntry, Zone, ZoneKnock};

// Four corners, clockwise.
shell.add_hidden_entry(
    HiddenEntry::new(
        "factory",
        ZoneKnock::corners([
            Corner::TopLeft, Corner::TopRight, Corner::BottomRight, Corner::BottomLeft,
        ]),
        LaunchAction::open("factory"),
    )
    .gate("factory"),
);

// They need not be corners - here, a mid-screen band, left then right.
shell.add_hidden_entry(HiddenEntry::new(
    "diag",
    ZoneKnock::new([
        Zone::new((0.0, 0.4), (0.2, 0.6)),
        Zone::new((0.8, 0.4), (1.0, 0.6)),
    ]),
    LaunchAction::open("diag"),
));
# }
```

The shell reads this straight off the raw pointer, so **it works on a kiosk screen
with all the chrome hidden**. To keep it distinct from an edge swipe, `ZoneKnock`
counts **taps only** — start in a zone and drag, and it is not a knock.

### Anything else — implement the trait

For a trigger the crate does not ship (a key combination, long presses in a
particular order, a barcode value, a rotary knob), implement `KnockTrigger`. The
shell calls `feed` **every frame**, so time limits are yours to handle.

```rust
use fairing::access::{HiddenEntry, KnockInput, KnockStep, KnockTrigger};

/// The front-panel keys F1, F2, F1.
#[derive(Debug, Default)]
struct KeyCombo { hit: usize }

const WANT: [egui::Key; 3] = [egui::Key::F1, egui::Key::F2, egui::Key::F1];

impl KnockTrigger for KeyCombo {
    fn feed(&mut self, input: &KnockInput<'_>) -> KnockStep {
        let mut step = KnockStep::Idle;
        for key in input.keys {
            if WANT.get(self.hit) == Some(key) {
                self.hit += 1;
                step = KnockStep::Advanced;
                if self.hit == WANT.len() {
                    self.hit = 0;              // rewind yourself once it opens
                    return KnockStep::Opened;
                }
            } else if self.hit != 0 {
                self.hit = 0;
                step = KnockStep::Reset;
            }
        }
        step
    }

    fn remaining(&self) -> Option<u8> { u8::try_from(WANT.len() - self.hit).ok() }
    fn reset(&mut self) { self.hit = 0; }
}

# fn add(shell: &mut fairing::Shell) {
# use fairing::LaunchAction;
shell.add_hidden_entry(
    HiddenEntry::new("keypad", KeyCombo::default(), LaunchAction::open("service_menu"))
        .gate("service"),
);
# }
```

What a trigger sees each frame:

| `KnockInput` | What |
|---|---|
| `now` | **The shell's clock.** Use this instead of `Instant::now()` — tests drive it |
| `screen` | This frame's screen `Rect` |
| `pokes` | How many times `cx.knock(id)` poked **this entry** this frame |
| `taps` | Taps that finished this frame — `press`, `release`, `travel` |
| `keys` | Keys pressed this frame |
| `slop` | The drag tolerance. `tap.is_still(input.slop)` filters out swipes |

Three rules:

1. `feed` is **a decision, nothing else** — do not draw, do not block, and use
   `input.now`.
2. After returning `Opened`, **rewind yourself.** The shell does not call `reset`
   for you.
3. If `remaining()` is `None`, so is `cx.knock_remaining` — a trigger that cannot
   be counted gets no hint.

### It always leaves a record

When a knock completes, `ShellEvent::HiddenEntry { id }` fires **whether or not**
the gate passes. If the gate blocked it, `AccessEvent::UnlockRequested` follows.
Entering a service menu is auditable, so it never happens quietly.

```rust
# use fairing::ShellEvent;
# fn on_events(shell: &mut fairing::Shell) {
for event in shell.poll_events() {
    if let ShellEvent::HiddenEntry { id } = event {
        log::warn!("hidden entry {id} was opened");   // into the device log
    }
}
# }
```

### One entry point already exists

`chrome.emergency` — **a two-second long press in a top corner**. It survives on a
screen where `edge_guard` has blocked every gesture, and it opens the shade. To
keep one input from meaning two things, a corner **long press** is the emergency
gesture and a corner **tap** is left for hidden entries. If you really want a
long-press entry point, implement `KnockTrigger` yourself.
