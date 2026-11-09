# 05. Access control

For issuer-owned, continuously checked sessions with independent operation permissions,
see [session tokens](../session-tokens.md) and the
[security architecture](../security-architecture.md) for ownership, admission and
session-lifecycle diagrams. Enable `access.session_tokens` explicitly;
the level/PIN behavior below describes the default legacy mode.

For mandatory typed application declarations, use `build_protected` and
`Protected<P, D>`; see [typed requirements](../session-tokens.md#typed-permission-requirements).
The facade retains ordinary UI/app/backend references inside admitted callbacks
and checks the current token before invoking them. The
[common mediation contract](../protected-contracts.md) describes the shared kernel
proof and automatic protected-route check. The
[application metadata checker](../permission-metadata-check.md) checks supported
permission getters/mappers in an integrator's workspace.

## 0. Responsibilities

Applications define level names, feature-to-level assignments and credential
verification/storage. Fairing supplies level/gate coordination and the reference
clear-text PIN/pattern table; production credential handling can be supplied through
an application `Authenticator`.
Fairing enforces gate decisions and can draw the credential prompt.

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
| `off` | Legacy gate checks pass while the shell is operational; invalid with token mode | Nobody |

`prompt` needs something to ask. With no `[access.pin_table]` PINs and no
`ShellBuilder::authenticator`, there is nothing to draw and it behaves as
`routing` does — the event, and no prompt with nothing behind it. A single-level
table never prompts at all in legacy mode. Token mode requires explicit grants
even with one level and rejects `mode = "off"`.

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
| Two or more levels with no `default_gate` | `levels.len() >= 2` and `default_gate` omitted. An operational legacy single-level shell bypasses level checks, so this configuration check is skipped |
| `default_gate`, `initial` or an `[access.gates]` value is not in `levels` | Typo protection. `"top"` and `"bottom"` are reserved inside `default_gate`. Gate assignments resolve an actual level name first; without one, those two names are aliases for the highest/lowest level |

All three make `Shell::new` / `Shell::builder(..).build(ctx)` return
`Err(Error::Config(..))`, so shell construction stops with a configuration error.

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

In an operational legacy shell with one level, `Access::allows` returns early and
every gate passes, regardless of `mode`. Token mode requires explicit grants.
Fault quarantine denies gates even in this configuration. A device that needs no authentication says so in one line:

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
  [02 Screens](02-screens.md) and [03 Chrome](03-chrome.md).
- No gate (`gate = None`) does not mean "always allowed" — it means **"use the id
  as the gate"**. For a legacy gate reachable at the lowest level under the default `LevelPolicy`,
  assign that level in `[access.gates]` or set `default_gate = "bottom"`. A custom policy
  or fault quarantine can still deny it.

### 2.1 Built-in gates

Three things to know per gate: whether the crate actually **enforces** it, whether
an unassigned one gets a start-up `log::info!` (**warned**), and whether the
matching screen or feature exists at all.

| Gate | Enforced | Warned | Notes |
|---|:---:|:---:|---|
| `overlay.open` | ✔ | ✔ | Opening the shade. `LaunchAction::OpenOverlay`, the edge swipe and the status-bar tap all pass through this one gate |
| `chrome.emergency` | ✔ | ✔ | The top-corner hold (`emergency_ms`, two seconds by default) that remains recognized with an edge guard |
| `status.clock`, `status.wifi`, `status.bluetooth`, `status.battery`, `status.notifications`, `status.user`, `status.lock`, … | ✔ (render filter) | ✔ | Built-in status-bar item ids. Under `default_gate = "top"`, forgetting to assign these empties the whole bar — which is why they usually go to `bottom`. `status.ethernet`, `status.volume` and `status.brightness` also need a backend that reports the capability (see §2.2) |
| `tile.wifi`, `tile.bluetooth`, `tile.brightness`, `tile.lock`, … (all of `overlay::BUILTIN_IDS`) | ✔ (render filter) | ✘ | Quick-settings tile ids. **Unlike status items these are not in the warning list** — assign one wrong and it just disappears |
| `settings.wifi`, `settings.bluetooth`, `settings.display`, `settings.sound`, `settings.locale`, `settings.power`, `settings.network`, `settings.network.edit`, `settings.datetime`, `settings.datetime.set`, `settings.credentials` | ✔ | ✔ | These ids include built-in screens and the `settings.network.edit` / `settings.datetime.set` operation gates. The screens ship behind the `settings` feature (`settings.credentials` only where the authenticator manages its entries, §5.4), but you **opt in** by calling `fairing::settings::add_all(&mut shell, &SettingsConfig::default())`. Narrow the set with `SettingsConfig::only([..])` or `.without(id)`, or replace one wholesale by declaring your own screen with the same id. Wi-Fi, Bluetooth, network and power eligibility depends on the relevant capability. Display, Sound and other candidates without capability requirements remain eligible; `ignoring_capabilities()` bypasses capability filtering |
| `nav.recents`, `workspace.split` | ✔ | ✔ | The recent screens, and **entering** a split ([03 §2.3](03-chrome.md#23-recent-screens-and-the-split)). Short of one, a press asks for an unlock; short of `workspace.split`, the cards carry no split buttons and `cx.open_in_other_pane` opens in the same pane. Going back from the cards, or to one pane, is never gated |
| `desktop.edit` | ✘ | ✔ | Desktop editing is unimplemented; the name is reserved |
| `session.lock`, `session.logout` | ✘ | ✔ | Reserved names. These operations do not require an additional gate named `session.lock` or `session.logout`; the controls that do them carry gates of their own (`tile.lock`, `status.lock`). An unlock from the lock screen reports `session.lock` (§8) |

`settings.home` and `settings.about` use their screen ids as gates. Assign them
to the lowest level in legacy mode if they should be generally reachable; token
mode requires their explicit grants too. Their screen contents do not bypass
the opening gate.

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

Install it with `set_policy(..)`:

```rust
# use fairing::access::{AccessPolicy, Subject};
# struct BusinessHoursPolicy;
# impl AccessPolicy for BusinessHoursPolicy {
#     fn allows(&self, _: &Subject, _: &fairing::Gate) -> bool { true }
# }
# fn custom_policy() -> BusinessHoursPolicy { BusinessHoursPolicy }
# fn install(shell: &mut fairing::Shell) {
shell.set_policy(Box::new(custom_policy()));
# }
```

To use a level table other than the one `fairing.toml` built, just hold your own
inside the policy. When legacy level checks are active, the shell delegates the
decision, so the policy decides whether `[access.gates]` matters. Legacy `off` mode
and a single-level table bypass the policy. In token mode, an installed policy
can narrow issuer grants but cannot add authority.

Open screen gates are revalidated during operational state updates, before commands and again after backend
polling. Replacing a policy or changing a dynamic policy can close a screen without
re-announcing the subject or restarting its session timers. The shade and overview obey
their opening gates too and are removed at that revalidation when authority is
revoked, including during a closing animation. A policy that changes while the app is asleep still needs the
integrator to wake the UI when the change takes effect.

## 5. The shell's prompt and the `Authenticator`

In `mode = "prompt"`, when an authenticator offers a prompt method, a failed gate opens a modal over the screen. It draws only
what the authenticator offers, hands what was entered to it, and acts on the
answer. It follows the palette and the tokens, and two painters redraw what the way in stands
on — the lock screen's ground and clock, the prompt's backdrop and card
([04 §10.6](04-customization.md#106-the-lock-screen-and-the-unlock-prompt)).
The shell owns the prompt controls. For an application-owned flow, use `routing`
(§6) and build the screen from `PinPad` and `PatternPad`.

```text
tap a locked item
   └─▶ policy.allows() = false
   └─▶ AccessEvent::UnlockRequested { gate, then }      (failed-gate request event)
   └─▶ the prompt: Authenticator::methods() → a keypad, a pattern, a password card, a badge wait
           Authenticator::submit(credential, now)
             Granted(subject) → the session takes it (§7) → `then` runs through Shell::launch again
             Denied { message } → the message in the title's place, the card shakes, try again
             Locked { until, message } → the keypad greys out and counts down
             Pending → "Checking…" until Authenticator::poll(now) answers; the keys still work,
                       and a new attempt calls Authenticator::cancel on it first
```

`submit` receives an owned `Credential`. The shell does not include credential
contents in its authentication events, and `Credential`'s `Debug` redacts them.
The pattern prompt retains its points while awaiting an answer, and egui/input
buffers may hold text copies.
A refusal displays the authenticator's message, so the authenticator determines
whether that message reveals the existence of a user.

The prompt sits above the shade and bars. Its guard intercepts ordinary pointer
interaction outside the card and pauses the focused screen (`Lifecycle::Paused`,
then `Resumed`). Back cancels it. While it is open, new edge swipes are disabled.

With authenticator callbacks that return promptly, the prompt applies these rules:

- **Granted levels are validated.** A `Granted` level the table does not
  have, one no higher than the session already holds on the unlock prompt, or
  one below the session's start on the lock screen (which takes the start
  itself), is answered as a refusal ("That is not enough for this"). A valid but
  lower credential never demotes the session or closes its screens. The
  authenticator hears of the refusal through `Authenticator::refused(now)` and
  may count it as a failure — `PinTable` does, so a PIN someone knows does not
  start the attempt count again between guesses at one they do not.
- **A lockout stays.** Closing the prompt and opening it again, or a second
  request arriving over an open one, keeps the countdown and the dead keys — for a
  password and a badge as much as a keypad. Nothing is submitted while locked.
- **A wait is one flag.** `Pending` puts it up and the title says "Checking…"; it
  comes down with the answer, or with `Authenticator::cancel` — when the prompt
  goes without an answer, when the lock screen comes up over it, or when a new
  attempt goes in. While a check returns `Pending`, the keys remain usable: a new attempt cancels the
  pending request, on the lock screen too.
  The shell sets no time limit of its own (see §5.2). `poll` responses must carry
  the original request token; canceled, superseded and already-consumed responses
  are rejected. The authenticator should also stop canceled work where possible.
- **A request behind the lock screen waits for it.** Once the lock screen goes
  (an unlock, Continue, or your own `set_subject`), the request is asked again: it
  runs where the session now passes its gate, and the prompt opens where it does
  not. A new lock starts with nothing waiting.
- **`begin(request, gate, now)` comes first.** The shell calls it each time the prompt
  opens — the lock screen's gate is `session.lock` — so an authenticator can start
  listening, and drop whatever a reader picked up while no prompt was up.
  Retargeting an open prompt to a different gate calls `cancel` and then `begin`
  for the new gate before polling again. A request for the same gate keeps its
  pending check. Lockout countdowns survive either case.
- **A session boundary cancels the previous check.** Logout cancels a pending
  unlock even when the subject is already the initial one. Explicit relocking
  starts a fresh lock challenge and discards deferred actions. Logout while locked
  also discards them and restarts the challenge, preserving the lockout.
  A timeout or integrator subject replacement invalidates pending authentication
  even when the old and new subjects have the same numeric level. Direct
  `Shell::set_subject` changes are observed before the next frame or launch;
  both it and queued `ShellHandle::set_subject` use the shell's monotonic clock.
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
  cannot be drawn — the finger records `"1-2-3-6-9"`. The start-up error names the
  level and the validation rules, without printing the pattern. A dot twice, a dot off the grid, fewer than `min_points` dots, or two levels
  sharing a pattern is an `Error::Config` too.
- Wrong PINs and wrong patterns count against **one** attempt limit — two ways in
  are not two allowances. A right PIN the shell refuses (an operator PIN typed
  by an operator at a maintainer gate) counts too. Either table may set `attempt_limit` and `lock_secs`;
  where both do, the stricter limit and the longer lockout hold. The count lives
  in memory, and a restart clears it.
- The file holds the secrets in clear text, and the shell says so once at start-up
  with a `log::warn!`. Keeping that file unreadable is yours — or replace the table
  with an authenticator of your own.

Configuration `Debug` output lists the levels that have secrets, without their PINs or
patterns. TOML syntax/type errors retain a byte offset but omit the source and parser
message, since either can contain a secret.

The fixed keys — `attempt_limit`, `lock_secs`, `shuffle` and `max_len` in the PIN
table, `grid`, `min_points`, `show_path`, `attempt_limit` and `lock_secs` in the
pattern table — are the tables' own, so no level can be named after one of them.

### 5.2 Your own `Authenticator`

Hashing, accounts, a lockout that survives a restart, a TPM, a server: an
`Authenticator` of yours, handed to the builder, takes `PinTable`'s place.

```rust
use fairing::access::{AuthMethod, AuthOutcome, Authenticator, Credential, Subject};
use fairing::widgets::Keys;
use fairing::Level;
use std::time::{Duration, Instant};

/// Accounts checked however the device likes, with a lockout of its own.
struct Accounts {
    fails: u32,
}

impl Authenticator for Accounts {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::Password { needs_user: true, keys: Keys::Latin }]
    }

    fn submit(&mut self, _request: fairing::access::AuthRequestId, credential: Credential, now: Instant) -> AuthOutcome {
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
that takes time answers `Pending`, and the shell calls `poll(now)` during operational state updates
while the prompt is up. It sets no limit on how long that takes: a server that
ought to answer within ten seconds is yours to time, by answering `Denied` from
`poll` when the time runs out. The authenticator methods run on the UI thread,
but background work may finish after `cancel` returns. Keep its original request
token: Fairing rejects responses from canceled, replaced or completed requests.
Cancellation should still stop unnecessary worker activity and release resources.
`Shell::set_authenticator` swaps one in while the shell
runs; a prompt up at the time — the lock screen included — opens again for the new
one, with its methods and without the old one's lockout or a wait it owed, and the
old one is told to `cancel` first.

### 5.3 What each method draws

| `AuthMethod` | The prompt |
|---|---|
| `Pin { len, max_len, shuffle }` | The [`PinPad`](#55-a-keypad-or-a-pattern-in-a-login-screen-of-your-own) with its dot row; the bottom row is ⌫ · 0 · ✓, and ⌫ and ✓ dim while there is nothing to take off or submit. `len > 0` submits on the last digit (✓ submits early); with `0` the dots count what went in, ✓ submits, and the pad takes at most `max_len` digits (`0` is 16). A hardware keypad's digits, Backspace and Enter work too |
| `Pattern { grid, min_points, show_path }` | The [`PatternPad`](#55-a-keypad-or-a-pattern-in-a-login-screen-of-your-own): `grid` × `grid` dots. A path under `min_points` is answered on the spot ("Connect at least 4 dots") and never reaches `submit`; a refused one stays on the dots in red until the next stroke. `show_path: false` keeps the path off the glass. It arrives as `Credential::Pattern(dots)`, the dots counted row by row **from 0** |
| `Password { needs_user, keys }` | A user field (where asked for), a secret field and Unlock; Enter in the secret submits. The on-screen keyboard comes up **above** the prompt, on `keys` (`Keys::Latin` for a password, `Keys::Digits` for a numeric one) whatever the device's layout is, and `settings.credentials` sets the secret on the same keys. A refusal gives the secret field the focus back with the keyboard up, so the next try is typed straight in. The secret field has the eye that shows what was typed, as `TextField::password` does by default (it can be disabled with `.reveal(false)`) |
| `External { label }` | `label` and a ring going round (requests a repaint after 250 ms while waiting; other UI activity can repaint it sooner), until `poll` answers. A keyboard-emulating reader submits its input on Enter or Tab as `Credential::External(bytes)`, and a pause of more than 0.8 s between characters starts a new read, so half a swipe does not spoil the next one |

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
    read: Receiver<(fairing::access::AuthRequestId, String)>,
    start: Box<dyn FnMut(fairing::access::AuthRequestId)>,
}

impl Authenticator for Badges {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::External { label: "Badge".into() }]
    }

    fn begin(&mut self, request: fairing::access::AuthRequestId, _gate: &Gate, _now: Instant) {
        // The worker must retain this token with the operation, including after cancel.
        (self.start)(request);
    }

    fn submit(&mut self, _request: fairing::access::AuthRequestId, credential: Credential, _now: Instant) -> AuthOutcome {
        match credential {
            Credential::External(bytes) => check(&String::from_utf8_lossy(&bytes)),
            _ => AuthOutcome::Pending,
        }
    }

    fn poll(&mut self, _now: Instant) -> Option<fairing::access::AuthResponse> {
        self.read.try_recv().ok().map(|(request, badge)| request.respond(check(&badge)))
    }

    fn attach(&mut self, waker: Waker) {
        // Give this to the reader thread: `waker.wake()` after each badge it sends.
        let _ = waker;
    }
}
# fn check(_badge: &str) -> AuthOutcome { AuthOutcome::Pending }
```

Authentication implementors receive a Fairing-issued `AuthRequestId` in `begin` and
`submit`. `poll` returns `Option<AuthResponse>`: retain the request token with the
original work and return `request.respond(outcome)`. Do not stamp a late result with
the most recent token. A terminal response consumes its token; if the prompt remains
open after a refusal, Fairing calls `begin` with a fresh token for the next attempt.
`PinTable::check` remains available for synchronous use outside a shell.

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

**The credentials UI applies level checks.** The level picker offers the levels up to
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
`fairing::widgets::PinPad` is the one the prompt uses. It edits the `String` you
supply; your code decides when to submit that value:

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

The pad checks recorded pointer positions and the segments between them for
crossed dots. `PatternPad::as_drawn(dots, grid)` gives the path a finger drawing through
`dots` would record — what a stored pattern can be checked against — and
`.mark(Some(ColorRole::Danger))` draws a refused path in red.

## 6. Doing authentication yourself with `routing`

In legacy `mode = "routing"`, the crate handles gate decisions and session
storage while the host supplies the login UI. The `set_subject` examples below
apply to legacy sessions. Token-mode routing instead supplies an
`AccessEvent::SessionRequested` and installs a request-bound
`AuthOutcome::Session`; see [session tokens](../session-tokens.md).

```text
tap a locked item
   └─▶ policy.allows() = false
   └─▶ ShellEvent::Access(AccessEvent::UnlockRequested { gate, then })
           you: authenticate however you like - a login screen, a card reader, a server
           you: handle.set_subject(Subject { id, level, attrs }) to swap the session
           you: if `then` is Some, handle.launch(then) - it re-enters Shell::launch and is re-checked
```

The whole thing, handled when you drain `poll_events()` after UI or logic updates (see
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
still govern admission under `routing`; the host also supplies the authentication presentation
and response flow.

In legacy mode, `off` skips the authentication flow and operational gate checks pass.
Fault quarantine still denies access; token mode rejects `off`.

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
| `session_timeout_secs` | No input for this long takes the session back to `[access] initial` (`ChangeReason::Timeout`). A touch, key or pointer movement before expiry starts it again |
| `idle_lock_secs` | No input for this long locks the panel: the session goes back to its start (`ChangeReason::Lock`), `ShellEvent::LockRequested` goes out, and in `prompt` mode with an authenticator the **lock screen** comes up — the clock beside (or above) the way in. In `routing` mode the event is yours to answer with a lock screen of your own |

On every way down — a temporary unlock running out, the timeout, a lock, a logout,
`set_subject` to a lower level — screens that no longer pass their gate close,
the rest get `Lifecycle::AccessChanged`, and the shade and an unlock prompt close
too, and so do the recent screens where the session no longer passes the gate
they came up behind. Timers request a host update for their deadlines; enforcement
occurs when the host supplies a logic or UI update.

Shell/state runners use monotonic elapsed time through `Shell::logic*`, so
deadlines advance between UI passes. Frame-only custom hosts use the egui input
timeline. Expiry is checked at the next supplied update; presentation timing is
host-controlled. A timer longer than a year is a config error.

Overdue timers are enforced before the command queue and before input renews activity.
The first touch after a delayed frame cannot preserve an expired session, and queued
actions cannot execute with a temporary grant that has already run out. Authenticator
polling still happens after backend polling.

When a temporary grant and the session timeout are both overdue, the session timeout
wins: the shell returns directly to the initial subject, with no intermediate restore
subject available to queued commands or wake-up input.

**Every way up starts the idle stretch again** — an unlock, and an integrator's
`set_subject`, as a touch would: a badge reader that grants after the panel sat
idle is not timed out or idle-locked in the same frame. An integrator's
`set_subject` above `[access] initial` also takes the lock screen away, with
`LockScreenToggled(false)` — it vouched for someone, so the lock has done its
work.

**A pending result does not hold the prompt, provided authenticator callbacks return promptly.** The keys work while
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
#[non_exhaustive]
pub enum AccessEvent {
    SessionRequested { request: AuthRequestId, gate: Gate },      // token-mode host login
    UnlockRequested { gate: Gate, then: Option<LaunchAction> },  // a gate failed (every mode)
    Unlocked { gate: Gate, subject_id: Option<String>, level: Level, mode: UnlockMode },
    Denied { gate: Gate },                                       // the authenticator said no
    Locked { gate: Gate, until: Instant },                       // ...and no more tries until `until`
    SessionChanged { from: Level, to: Level, reason: ChangeReason },
    LockScreenToggled(bool),                                     // the shell's lock screen came up / went
}
#[non_exhaustive]
pub enum ChangeReason { Unlock, Logout, Timeout, ElevationExpired, Lock, Integrator, SessionToken }
```

They arrive as `ShellEvent::Access(..)`. The shell only emits; writing to a file
or shipping to a server is yours. No credential value is ever in an event — only
identity, level and gate metadata plus the opaque request correlation id.
`SessionRequested` carries the request needed by `accept_auth_response`; a
`SessionChanged` reason of `SessionToken` also covers issuer permission publication,
revocation or expiry, including changes at the same numeric level. An unlock from
the lock screen reports the gate `session.lock`.

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

For screens omitted from the desktop, such as a service menu or diagnostics,
register an entry trigger. Examples include repeated taps, corner sequences or
a key combination. The trigger is separate from authorization.

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

**You decide where to knock.** The screen calls `cx.knock` from its chosen UI
location. The trigger controls discovery of the entry point; its gate controls access.

```rust
# use fairing::{screen, Cx};
# let _ =
screen("about", |ui: &mut egui::Ui, cx: &mut Cx| {
    let model = ui.add(egui::Label::new("Model ACME-7").sense(egui::Sense::click()));
    if model.clicked() {
        cx.knock("service");
    }
    // The crate draws no hint. The count comes from `hint_from` down; render it your way.
    if let Some(left) = cx.knock_remaining("service") {
        ui.small(format!("{left} more to open"));
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

### Handling the entry event

When a knock completes, `ShellEvent::HiddenEntry { id }` fires **whether or not**
the gate passes. If the gate blocked it, `AccessEvent::UnlockRequested` follows.
This is an in-memory event, not a persistent audit log. The application must
drain the events and record them through its logging or audit backend if needed.

```rust
# use fairing::ShellEvent;
# fn on_events(shell: &mut fairing::Shell) {
for event in shell.poll_events() {
    if let ShellEvent::HiddenEntry { id } = event {
        log::warn!("hidden entry {id} trigger completed"); // admission may still be denied
    }
}
# }
```

### One entry point already exists

`chrome.emergency` — **a top-corner hold for `[gesture] emergency_ms` (two seconds by default)**. Recognition remains
when `edge_guard` suppresses built-in edge gestures. Opening the shade requires its feature and
the emergency gate to pass. To
keep one input from meaning two things, a corner **long press** is the emergency
gesture and a corner **tap** is left for hidden entries. If you really want a
long-press entry point, implement `KnockTrigger` yourself.
