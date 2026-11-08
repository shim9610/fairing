# 10. Migration from an earlier checkout

The first crates.io release has not been published. This page compares the current
pre-release implementation with `main` at `f4ca28ce21a3227d35805a3d9e325ec64f42b25f`,
before the authentication, recovery and resource-boundary changes in PR #8.
It describes changes for existing checkout users, not a published-version upgrade.

## 1. Source changes

| API | Earlier checkout | Current implementation |
|---|---|---|
| `Authenticator::begin` | `begin(&mut self, &Gate, Instant)` | `begin(&mut self, AuthRequestId, &Gate, Instant)` |
| `Authenticator::submit` | `submit(&mut self, Credential, Instant) -> AuthOutcome` | Adds `AuthRequestId` before `Credential`; synchronous return stays `AuthOutcome` |
| `Authenticator::poll` | `Option<AuthOutcome>` | `Option<AuthResponse>` |
| `IconCache::get` | `Rc<Flattened>` | `Option<&Flattened>`; `try_get` returns a detailed error |
| `Command::Notify`, `Toast`, `SetMotion` | Boxed payloads | Inline values; remove `Box::new` at explicit command construction sites |
| `Shell::toast`, `ShellHandle::toast` | `impl Into<Toast>` | `impl Into<ToastInput<'a>>` |
| `CenterChange` | `Added`, `Updated` | Adds `Rejected`; update exhaustive matches |
| `InstanceScreen::Owned` | Stores `Box<dyn Screen>` | Stores opaque `ScreenHandle`; construct with `InstanceScreen::owned(screen)` |

### Authentication

Store the supplied request token with asynchronous work. Return its response from
`poll`, even if cancellation happened before the worker finished:

```rust
use fairing::access::{AuthOutcome, AuthRequestId, AuthResponse};

fn finish_original_request(request: AuthRequestId, outcome: AuthOutcome) -> AuthResponse {
    request.respond(outcome)
}
```

Do not attach the newest token to an old result. Fairing rejects canceled,
superseded and already-consumed requests; cancellation should still release worker
resources. Tokens correlate results from a trusted authenticator, not network
identity. `PinTable::check(credential, now)` supports synchronous use outside a
shell. See [05 Authentication](05-access-control.md#53-what-each-method-draws).

### Issuer-controlled sessions

The earlier type-wide `Permission::GATE` API is replaced by value-based mapping.
Pass values to `PermissionBinding::new`, `Protected::new`, all typed grant helpers
and `SessionSnapshot::allows_permission`. `PermissionBound` is now an external
metadata contract with a mandatory `required_permission` method. Fairing retains
ownership of authorization; implementations return values rather than decisions.

`ProtectedShell::add_screen` guards an external `Screen + PermissionBound`.
`add_elements` owns an `ElementTree` and filters/measures/lays out/paints its children.
Use these APIs instead of manually invoking a guard for every nested child. A
registered screen's requirement stays fixed; replace it to change that declaration.
See [the element integration example](../session-tokens.md#typed-permission-requirements)
for custom measurement/paint implementations and compilation contracts.

`ShellBuilder::build_protected` adds an opt-in typed registration facade. Enable
`access.session_tokens`, wrap each configured screen/action/status/nav/tile in
`Protected::new(value, declaration)`, and implement `Permission::gate(self)` for
your own enum or small `Copy` value struct. Different variants may name distinct
grants. Use `SessionGrant::base_for(value)` on the issuer side. The facade does not expose
mutable legacy registration. Protected screen/action icons are hidden on refusal;
screen lifecycle callbacks are also denied after revocation. Factory construction
is deferred to the first authorized hook. See [typed requirements](../session-tokens.md#typed-permission-requirements)
for nested scopes, cleanup and the callback-effect boundary.

Set `[access] session_tokens = true` and return `AuthOutcome::Session(token)` to
adopt split capabilities. The authenticator retains the non-cloneable controller.
Existing authenticators keep their legacy behavior when this flag is false.
`AccessConfig` now uses extensible construction: start with `Default` and set
`session_tokens`, directly or through `.configure(...)`. The optional `Authenticator::session_ended` callback revokes
the matching controller after local detachment. Direct subject injection does not
authorize token-mode work; host-owned login flows use the request-bound
`SessionRequested` / `accept_auth_response` path. See [session tokens](../session-tokens.md)
for role expansion, catalog scope, grant expiry and proof boundaries.

`build_protected` now rejects untyped painters/layouts, widget-painter bundles,
image loaders and procedural wallpapers before running setup hooks. Data-only
wallpapers, authenticators and backends remain supported. Put application UI in
`Protected<P, D>` declarations, or retain legacy `build` when trusted host hooks
are intentional.

Icon and widget-painter stores in a protected callback are local to that call.
Do not retain their custom icon IDs for a later frame. For persistent custom
rendering, own the stores in your screen and use them only inside its admitted
callback. App/backend references retain their original meaning.

### Commands and toast inputs

Pass values directly when constructing command variants:

```rust
use fairing::{notify::Toast, shell::Command};

let command = Command::Toast(Toast::new("Saved"));
# let _ = command;
```

Borrowed `&str`/`&String`, owned `String` and `Toast` still work with the toast
helpers. For a custom type previously implementing `Into<Toast>`, first convert it
to an explicitly typed `Toast`. Borrowed text takes the fallible construction
path; an already-created `String` cannot undo caller-side allocation.

### Icon cache

Handle omission and finish using the borrowed path before mutating the cache again:

```rust
use fairing::icons::{IconCache, IconDef};

fn inspect(cache: &mut IconCache, definition: &IconDef) -> Option<usize> {
    let flat = cache.get(definition, 24.0)?;
    Some(flat.polylines.len())
}
```

The default cache limit is 128 entries. `with_capacity_limit(0)` disables cached
icon display; allocation failure can reclaim older entries or omit an icon.
`try_get` distinguishes disabled storage from failed allocation.

## 2. Defaults and refusal handling

| Resource | Current default | Host action |
|---|---|---|
| Shell command queue | 1,024 shared slots, 256 commands per UI/logic update | Configure `command_queue_capacity` and `max_commands_per_update` for the workload |
| Message-based mock channels | 1,024 slots, 256 messages per poll | Use `with_inbox` / clock `*_with_inbox` constructors for connection-specific limits |
| Waiting toasts | 64 pending items, `KeepLatest` in the shell | Use `toast_queue_capacity` and `toast_overflow_policy`; standalone `ToastQueue` defaults to `RejectNew` |
| Notification retention | 100 items including persistent entries; zero selects 100 | Handle protected overflow as refusal rather than relying on unlimited retention |

`ShellHandle::send` and `MockControl::send` now return `false` for a full queue as
well as disconnection. Rejected values are discarded by these compatibility calls.
Void `ShellHandle` helpers discard rejection without calling an application logger.
Use `try_send` to recover the value and distinguish `Full` from `Disconnected`:

```rust
use fairing::{inbox::TrySendError, shell::Command, ShellHandle};

fn submit(handle: &ShellHandle, command: Command) -> Result<(), TrySendError<Command>> {
    handle.try_send(command)
}
```

`try_send_with` reserves capacity before constructing the payload;
`try_send_fallible_with` also releases the reservation when construction fails.
An accepted command is not evicted, but admission does not establish authorization,
execution or display completion. Define refusal/retry and completion handling in
the application. Standard transport internals may synchronize; these APIs do not
establish lock-free or hard real-time execution.

Display APIs such as `try_toast`, `try_notify` and `try_dismiss_notification` expose
storage refusal. Dismissal allocation failure preserves the retained notification
and unread state for a later retry. See [memory limits](../memory-limits.md).

## 3. Host integration and new optional APIs

- **egui/eframe:** update from 0.36.2 to 0.37.0. Use `fairing::egui` to avoid type
  mismatches. The minimum Rust version stays 1.95. The bundled runners process
  shell state during `App::logic`; a custom host should connect `Shell::logic*`
  alongside `Shell::frame*` as described in [01](01-getting-started.md#44-when-you-are-not-using-the-runner).
- **Runtime faults:** use `panic = "unwind"` for callback containment. A faulted
  shell denies gates, closes admission, stops ordinary dispatch and discards failed
  viewport output. Loan helpers restore ownership and propagate to that boundary;
  the interrupted callback does not continue with `None`.
- **Shell replacement:** `restart_with`, `RecoveryUi` and runner recovery callbacks
  expose host-controlled replacement. Obtain new producer handles afterward. There
  is no automatic replay, business rollback or egui/GPU reset.
- **Application transport:** optional `bridge::CommandSink` and `UpdateSource`
  impose no `Clone`, `Send`, `Sync` or `'static` bounds. Implement the chosen
  transport and thread/retry policy in the application; see [06](06-services.md#application-owned-uibusiness-boundary).
- **Fixed storage/display primitives:** `memory::FixedPool` and `DisplayWindow`
  provide bounded owned storage. Fallible display admission and CPU warning/preview
  helpers cover selected paths. Complete renderer OOM survival is unsupported.

## 4. Behavior fixes

- Built-in Wi-Fi, Bluetooth and brightness controls check setting authorization
  before mutating backends.
- Overdue session/elevation/idle deadlines precede queued commands and fresh input;
  waking a stalled UI cannot renew an already-expired session.
- Delayed Boolean acknowledgements preserve newer intent and Set/Toggle ordering.
- Configuration diagnostics redact PINs and configured/reconstructed patterns.
- Dropdown, pattern, hold, numeric draft, wheel, keyboard-request and password-reveal
  ownership is retired when obsolete. Numeric drafts check the exact caller value.
- Gesture regions use current geometry; region-owned presses cannot also activate
  ordinary widgets or hidden corner knocks. Emergency handling remains independent.

The [API contract map](../api-contracts.md) links implementation paths and test
evidence. Component proofs and finite UI histories do not establish complete
concrete UI-state coverage or universal interaction progress.

## Protected mediation ownership

`InstanceScreen::Owned` now stores an opaque `ScreenHandle`. Replace
`InstanceScreen::Owned(Box::new(screen))` with `InstanceScreen::owned(screen)`.
The original screen cannot be borrowed or extracted from that handle. Factory
creation occurs at the first dispatched hook, including legacy shells; move
launch-time setup that must run earlier into explicit application setup.

`runner::run_shell*`, `run_app*` and `Harness::from_builder` accept both shell modes.
For protected built-ins use `settings::add_all_protected(shell, config, permission_for)`
or its custom-list variant. The mapper returns your application permission value
for each registered screen ID. It selects requirements during construction and
does not grant write permissions. Protected recovery accepts only protected replacements.

Protected operation adapters and their automatic coverage derive from the same
[mediation contract](../protected-contracts.md). New operation declarations do not
require a new proof per screen or a separate verifier registration.


## Public API evolution after 217373f

The following are intentional pre-release source changes; there is no published
crate version to upgrade from yet.

| Before | Current migration |
| --- | --- |
| External literals for shell/widget configuration, including `..Default::default()` | `Type::default().configure(\|c\| { c.field = value; })` or direct field assignment on a default value; all configuration structs are non-exhaustive |
| `SessionGrant { gate, base, temporary_until }` | `SessionGrant::new(gate, base, temporary_until)`; read through `gate()`, `is_base()`, `temporary_until()` |
| Exhaustive matches on `SessionError`, `SessionReadError`, `Decl`, `DeclKind` | Handle known cases and retain a fallback; these enums are non-exhaustive |
| `screen::Registry`, `Shell::registry()` or `ProtectedShell::shell()` | `observe()` queries: `screen_ids`, `has_screen`, `screen_is_resident`, `nav_item_rect`, `settings`, `session_token`, `allows`, layout/time/fault/prompt state |
| `runner::run_app::<State>(...)` | Prefer inference or use `runner::run_app::<State, _>(...)`; the host type is now generic |

Ordinary `Harness` type annotations still mean `Harness<Shell>`; annotate a
protected harness as `Harness<ProtectedShell>` or let `from_builder` infer it.
Recovery callbacks receive the concrete shell mode and may use its
`restart_with`, `set_recovery_ui` and `take_restart_request` methods.

```rust
use fairing::{Shell, ShellConfig, ProtectedShell, testing::Harness};
let mut harness: Harness<ProtectedShell> = Harness::from_builder(|ctx| {
    let config = ShellConfig::default().configure(|c| c.access.session_tokens = true);
    Shell::builder(config).build_protected(ctx)
})?;
harness.frame();
assert!(harness.shell.observe().session_token().is_none());
# Ok::<(), fairing::Error>(())
```
