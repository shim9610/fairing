# Bounded admission and allocation failure

Display pressure must refuse or defer optional display work while preserving
accepted state and unrelated commands. Fairing supplies bounded queues, inline
object storage and fallible display admission. **A process-wide fixed heap is
not a supported production recovery policy.** The earlier `fairing/fixed-heap`
feature was removed: egui/epaint and remaining infallible Rust allocations can
abort, so that feature could not provide continued operation under exhaustion.
The diagnostic allocator is isolated from the product dependency graph below.

## Allocation-free display copies and raster fallback

`memory::DisplayWindow<T, N>` retains only N inline Copy samples. New samples
replace the oldest display copies. Bulk input visits at most N source elements,
`recent(limit)` exposes only the newest limited suffix in chronological order,
and no heap allocation, factory, destructor or overflow list is involved.
Original acquisition records stay with the caller. N = 0 omits all display samples.
Choose N for the caller's stack/static budget; payload references still refer to
caller-owned storage. Debug reports metadata rather than sample contents.

```rust
use fairing::memory::{DisplayWindow, paint_display_warning};
let samples = [1u32, 2, 3, 4, 5];
let mut displayed = DisplayWindow::<u32, 3>::new();
let status = displayed.extend(&samples);
assert!(displayed.recent(2).copied().eq([4, 5]));
let mut emergency_rgba = [0u8; 96 * 12 * 4];
if status.is_reduced() {
    paint_display_warning(&mut emergency_rgba, [96, 12], 96 * 12)?;
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`paint_image_preview` borrows validated RGBA8 source data and writes directly to
an already allocated output surface within its pixel budget. It does not clone
the source, decode, allocate a second image or upload a texture. Nearest-neighbour
sampling reduces the image to the output size. Information lost on either axis
is reported even if the other axis grows. Reduced previews include the fixed
DISPLAY LIMITED notice; a surface too small for its text gets a warning-colour
corner. Invalid/overflowing geometry is refused before modifying the output.

`paint_display_warning` clears a prepared RGBA8 surface and paints the same
notice using fixed bitmap glyphs. It has no String, font loader, glyph atlas,
egui context or allocation. Both functions reject bad dimensions and output
sizes above the caller's pixel budget. The host must present/reuse the prepared
surface through its backend; these CPU functions do not establish that a later
`ColorImage` construction, `load_texture`, stock egui frame or GPU upload is fallible.
They are portable CPU rendering primitives, not a replacement for the complete
interactive egui shell or a GPU zero-copy claim.

Real 16/64 MiB exhausted-arena trials execute these exact APIs in the same process.
They require clipping and visible warning pixels while even a one-byte reserve
fails, 10,000 continued preallocated work commands and no allocation in that
window/preview/warning/work interval. Separate checks require failed text/frost
work to be omitted, and old toast storage to be freed before replacement.

## Command admission and work count

```rust
use fairing::{Shell, ShellConfig};
fn build(ctx: &fairing::egui::Context, config: ShellConfig) -> fairing::Result<Shell> {
    Shell::builder(config)
        .command_queue_capacity(64)
        .max_commands_per_update(16)
        .toast_queue_capacity(8)
        .build(ctx)
}
```

The defaults are 1,024 command slots and 256 commands per UI/logic update.
Both options accept 1 through 65,536. Invalid values fail startup validation.
Command payloads are stored inline, including notification/toast/motion variants;
no new Box is allocated merely to enqueue one. Larger command values also mean
larger preallocated transport buffers: size the slot count for the device.

`ShellHandle::try_send` returns Full/Disconnected and the original command.
`try_send_with` reserves before constructing its payload; full/closed queues skip
the factory. `ShellHandle` and bounded `Inbox` senders expose `try_send_fallible_with`: a
factory returning Err releases the reservation without enqueuing anything.
A concurrent receiver close can occur after construction starts. Avoid unrelated
side effects in factories. Send errors redact payloads in Debug/Display.

`queue_stats()` samples capacity, outstanding reservations, high-water count,
full rejections and receiver-open state separately. Outstanding excludes an
already received command. Accepted order is retained and no accepted command is
evicted. Admission is not authorization, execution or backend completion.
Critical work needs an explicit refusal/retry and completion policy. Do not spin
retrying on the UI thread or move rejected work into an unbounded retry list.
The `send` compatibility method returns `false` for either full or disconnected
admission. Void `ShellHandle` helpers discard rejected commands without logging;
critical producers must use an error-returning method.

## Fallible display admission

```rust
use fairing::notify::{Notification, NotificationCenter, NotificationId, Toast};
let toast = Toast::try_new("Saved")?;
let mut center = NotificationCenter::new(100);
center.try_push(Notification::try_new(NotificationId(1), "Device ready")?)?;
# let _ = toast;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `Shell::try_toast(&str)` for direct display admission and
`ShellHandle::try_toast(&str)` for fallible command/text construction.
The void `toast` conveniences now accept `ToastInput`: &str and &String take the
same fallible path, while String and Toast transfer existing ownership. A custom
type previously implementing Into<Toast> must explicitly convert to Toast first.
Caller-side `Toast::new`, formatting and owned payload construction can still
allocate before admission; prefer borrowed text or the fallible constructors.
`Shell::try_notify` and `NotificationCenter::try_push` return rejected notification
ownership. Errors distinguish Full, TooLarge and AllocationFailed. A queued
notification/toast command may later be refused by its display retention budget;
command admission alone does not acknowledge display.

The waiting toast queue defaults to 64 items, excluding the configured visible
slots. `toast_queue_capacity(0)` disables new toasts; values through 65,536 are
accepted. `ToastQueue::with_pending_capacity` configures a standalone queue.
`try_push` preserves rejected ownership; `try_push_text` checks capacity before
copying borrowed text. Standalone ToastQueue keeps RejectNew compatibility;
ShellBuilder defaults to KeepLatest. `toast_overflow_policy(RejectNew)` restores
refusal. KeepLatest drops the oldest pending display text before constructing its
replacement. On a fallible allocation failure it clears optional icon caches and
reclaims older visible/pending toasts before retrying; every retry consumes an
old item, so it cannot spin indefinitely. If reclamation is insufficient, it
returns AllocationFailed and omits the new display item. `take_display_status`
reports discarded items/allocation failures without allocating a warning message.
The shell shows a compiled DISPLAY LIMITED stencil until the status is
acknowledged. `report_display_status` lets an application report clipped views
through the same notice. The ordinary egui adapter submits geometry and can still
allocate in egui; the separate raw CPU warning raster is the allocation-free path.
Handle-side text construction still reports failure to its caller; it cannot
mutate the UI thread's retained items. A startup-allocated shared atomic flag
coalesces rejected handle toasts into the shell notice until acknowledgment;
recording the flag needs no allocation, lock or extra command slot. It does not
invoke a wake callback on failure. Duplicate text/severity renews or deduplicates existing
items. Text plus owned glyph capacity is limited to 4,096 bytes per toast.
Void handle toast helpers omit rejected display work without invoking a logger;
use try_toast or try_send to observe and respond to their errors.

Notifications have a hard item cap including persistent entries. Zero selects
100 rather than unlimited retention; the standalone constructor caps the setting
at 65,536. Existing persistent items are preserved. If no dismissible item can be
replaced, a new item is refused. Existing ids can still update. All retained
string capacities, including source/title/body, glyph, gate and action/setting
strings, are charged against 16,384 bytes per notification. Spare capacity also
counts. This avoids small logical text hiding a large retained allocation.

Notification item/unread buffers reserve fallibly before replacing any retained
entry. Unread tracking uses reusable vector storage rather than allocating tree
nodes on every arrival. Failed admission leaves retained items and unread state
unchanged. The compatibility `push` reports CenterChange::Rejected; void Shell
helpers omit refused display work.

When visible toast storage cannot grow, accepted waiting items remain queued,
optional icon cache entries are cleared, and retry is scheduled after 100 ms.
There is no capacity wait or immediate retry loop. Banner-copy failure hides the
banner; failed owned toast/banner text copies and icon-coordinate copies omit
that display work. These paths do not establish that every egui operation is
fallible: egui can allocate before or after these calls.

Card frost processing validates source geometry, declines screenshot requests
above 16,777,216 physical pixels, and reduces its output to at most 16,384 texels.
All blur work/output buffers reserve fallibly; a failure drops the optional work
and leaves the card opaque. Adaptive shrink uses u64 accumulators and a reusable
column buffer. The incoming screenshot/egui texture allocation remains upstream.

Existing `new`/Into constructors and user callbacks may still allocate
infallibly. Use the explicit try APIs before copying untrusted borrowed input;
passing an already-created String cannot recover an earlier caller allocation.
The contracts above cover the named owned storage and CPU raster paths. Arbitrary
egui callbacks, fonts/tessellation, startup and GPU/native allocation remain outside
that scope; bounded-display test results do not establish whole-renderer survival
under complete heap exhaustion.

## Icon storage, deferred dismissal and producer rejection

Icon curve flattening reserves every point/subpath buffer fallibly. `try_flatten`
returns an allocation error; the compatibility `flatten` returns an empty path if
it cannot build the complete result. No partial cache entry is published.
`IconCache` owns paths directly, avoiding an infallible `Rc::new` on cache misses.
It defaults to 128 entries; `with_capacity_limit(n)` selects another entry limit,
including zero to omit cached icon display. This limits entries, not all renderer
bytes. A hit allocates nothing. New entries replace the oldest-created paths at
capacity; allocation failures reclaim older paths one at a time before retrying.
Each retry removes an entry, so failure cannot create an unbounded retry loop.
If reclamation is insufficient, drawing omits the icon and records display status.

`IconCache::get` now returns `Option<&Flattened>` instead of `Rc<Flattened>`.
Handle `None`, and end the borrowed view before mutating the cache again. `try_get`
returns `IconCacheError` for callers needing the reason. `clear` releases both
paths and metadata. The cache, shell, toast/banner and keyboard status paths expose
allocation failures without allocating diagnostic text. Unknown built-in icon names
return `false` from `IconSet::paint` without invoking a logger.

`Shell::try_dismiss_notification` reserves its completion event before deleting
anything. If allocation fails, it returns `AdmissionError::AllocationFailed` and
preserves the notification and unread state. The Boolean convenience returns false;
a later application attempt can complete once memory is available. A queued dismissal
is not automatically re-enqueued on refusal and produces no completion event.
Persistent/missing items return false without reserving event storage.

`ShellHandle::try_send_fallible_with` reserves a command slot before running a
fallible payload factory. Full/closed queues skip the factory; factory failure
returns the slot and does not wake or log. Void handle conveniences no longer log
rejections, because an application logger could allocate or panic in an already
exhausted process. Use try APIs and queue statistics for explicit handling. Successful
sends still call the configured host wake path; callbacks remain application code.

The actual exhausted-arena trials require cache hits, cold-icon omission, cache
reclamation, deferred dismissal followed by exactly one completion, returned factory
reservations, and no rejection logger calls. These cases remain outside the panic
quarantine state and keep the preallocated work queue running. The separate stock-egui
frame negative control prepares input before exhaustion, then demonstrates that
ordinary `Context::run_ui` still aborts under complete heap exhaustion. These storage
fixes do not establish panic/abort-free operation of the entire interactive renderer.

## Backend channels and object storage

```rust
use fairing::inbox::Inbox;
let (sender, receiver) = Inbox::<u32>::bounded(8)?;
sender.try_send_with(|| 42)?;
assert_eq!(receiver.try_recv()?, 42);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Clones share capacity. `drain()` stops after at most capacity items;
`drain_up_to(n)` after at most n, even while producers refill. Standard transport
may synchronize internally; no API waits for capacity. Construction can still
allocate infallibly in std, so this is a runtime admission bound rather than a
universal recoverable startup-allocation contract. `Inbox::pair()` remains an
unbounded compatibility API; select bounded channels at service boundaries.

Message-based mocks now default to 1,024 slots and 256 messages per poll. Their
`with_inbox` constructors (`fixed_with_inbox` / `running_with_inbox` for clocks)
select bounded or explicitly unbounded storage per connection. Even unbounded
storage uses a finite polling budget; full batches schedule continuation through
`next_wake`, without requiring another producer message. `set_inbox_batch_limit(0)`
pauses consumption without spinning; a positive budget resumes the same queue.
Timed simulation continues independently. `MockControl::try_send` preserves the
rejected payload and distinguishes `Full` from `Disconnected`; compatibility
`send` returns false for either and drops that payload. No accepted message is
evicted. Producers still wake the shell after accepted sends. See the
[service guide](guide/06-services.md#bounded-service-channels) for worker backpressure.

```rust
use fairing::memory::FixedPool;
let mut pool = FixedPool::<u32, 8>::new()?;
let key = pool.try_insert(42).map_err(|_| "pool full")?;
assert_eq!(pool.remove(key), Some(42));
assert!(pool.get(key).is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

FixedPool<T, N> has N inline slots, no container heap allocation and no fallback.
Keys reject foreign identities and removed/replaced generations; moves preserve
keys. Identities never wrap and a slot reaching generation u64::MAX retires after
removal. Payloads can own separate allocations. Large pools require appropriate
stack/static/application storage. Destructors remain application code.

## Diagnostic allocator and real failure injection

`fairing-fixed-heap` is an unpublished diagnostic crate, with no dependency edge
from either product library. Its global feature installs one static arena only
in its explicitly linked diagnostic executable. Do not use it as a device
recovery mechanism. It uses pinned Talc/spinning_top and one reviewed unsafe
static ownership transfer; both UI crates still forbid unsafe code.

```sh
FAIRING_HEAP_BYTES=16777216 cargo run -p fairing-fixed-heap \
  --example allocation_pressure --features global
python3 verification/memory.py
```

The normal product example uses the system allocator. The 16 MiB/64 MiB lab
runs first exercise 120 egui frames and 10,000 queue/pool reuse operations with
zero additional allocations in that reuse loop. They then exhaust the actual
arena until even a one-byte reserve fails: fallible text/factory admission must
return errors, reservations must be released, protected notifications must
remain, and a preallocated queue must continue processing. Toast display waits
without retrying before 100 ms and resumes after ballast is released.

Isolated infallible-allocation and stock-egui-frame children still abort. Those
negative diagnostics record why the global production option was withdrawn; an abort is not an
acceptable production recovery result. Native tests, Miri, Loom, generated
inputs and the API fuzzer cover the bounded contracts with their documented
limits. No whole-application or medical-device certification follows from them.

## Runtime panics and sensitive UI state

With `panic = "unwind"`, Shell frame/logic entry points, direct launch/setting/hidden
entry dispatch and the supplied runners contain runtime callback unwinds. A failed
shell enters `is_faulted()`: access gates deny even in single-level/off
mode, command admission closes, pending commands are retained without dispatch,
and partially generated shell events are not published. `quarantine()` lets a
host latch the same state after its own failure. Independent business workers can
continue; normal shell polling and dispatch stop. Replacement and optional guidance
are described below.

A faulting UI pass erases its viewport's pending graphics and platform output,
including clipboard commands. Logic/event handlers receive the same output cleanup.
The failed pass is blanked. Subsequent frames can show an opt-in `RecoveryUi`;
without that option they remain blank. Hosts can also use the prepared CPU warning surface.
Loan helpers restore borrowed state/resident ownership and then propagate the unwind
to the runtime boundary. They no longer swallow a panic and resume the caller's body.
Nested public calls propagate to the outer boundary so an interrupted command batch
cannot continue after a nested backend failure.

This is not transaction rollback. Callback mutations, backend writes or event-handler
side effects performed before a panic may already have happened. Reconcile the
business system and authorization in the application's recovery path. Closing the
queue prevents new factories from starting; an already running producer factory may
finish, but the faulted shell will not execute its command. Hosts with other UI in the
same viewport must account for whole-viewport output being discarded on failure.

The supported panic payload types are exactly `String`, `&'static str` and
`recovery::PanicCode(u16)`. Their destruction does not run application-defined code.
The library releases these payloads without formatting or logging string contents;
`PanicCode` contains only an application-defined numeric diagnostic identifier.
Expected failures must use `Result`, `AuthOutcome`, queue rejection or the relevant
fallible API, rather than initiating an unwind. The product crates forbid explicit
panic/abort entry points through their lint configuration; the scoped unwind propagation
helper restores borrowed ownership before forwarding an existing callback failure.

An arbitrary payload from application or dependency code is outside this contract.
Fairing never invokes its destructor, including under a second `catch_unwind`: arbitrary
destructors can abort, double-panic, block or allocate. The payload is retained with
`mem::forget`, without exposing a retrieval/execution handle. This avoids running its
cleanup code but retains any owned memory, descriptors, locks or sensitive data.
Repeated unsupported payloads can accumulate resources; there is no global byte bound
and shell replacement does not reclaim these objects. The contract cannot restrict
what an external library passes to `panic_any` at runtime.

`recovery::panic_cleanup_stats()` exposes saturating process-wide counts of caught
unwinds, supported payloads released and unsupported payloads retained, plus the last
observed numeric code. Individual fields are atomic, not a consistent per-shell snapshot;
string/unknown payloads do not clear the last numeric code. There is no payload formatting
or diagnostic allocation. A fault guidance view which unwinds is disabled until explicitly
installed again. Applications retain control of recovery scheduling and can inspect
retention diagnostics before attempting replacement. These diagnostics are not a memory
budget or a mechanism for recovering unknown payload resources.

Application panic hooks run before containment and must redact sensitive
data themselves. Panicking hooks, abort builds, double panics in unwinding destructors,
allocator aborts, and rendering/native work outside these boundaries remain outside
this guarantee. The failure tests demonstrate continued execution after the named
unwind faults, not universal survival of stock-egui OOM.


### Restart capability and optional guidance

`Shell::restart_with(|| build_shell(ctx))` closes the old inbox before invoking the
application's factory. Success installs a fresh shell and disposes of the old one.
Queued commands and pending shell events are not transferred; old handles stay
disconnected, so producers need the new `shell.handle()`. The new shell's builder
supplies its configuration, services, authorization, registrations and recovery view.
The application chooses which state to restore. The egui context, external business
state and runner callback captures are not reset by this operation.

A construction error, a factory unwind or an already quarantined replacement returns
`RestartError` and leaves dispatch blocked. If disposing of the old shell unwinds,
the new shell is also quarantined. A later call can attempt replacement again; there
is no required delay, retry count, confirmation or automatic/manual policy. Error
formatting uses generic messages; `RestartError::Build` and its error source expose
construction details for application diagnostics. Ordinary Rust destructor and abort
limits still apply.

Guidance is off by default. Use `ShellBuilder::recovery_ui(RecoveryUi { ... })` or
`Shell::set_recovery_ui(Some(view))` to opt in, and `set_recovery_ui(None)` to hide it.
`heading` and `message` are developer-supplied text. `restart_label: Some(label)`
adds a button; `None` shows guidance only. A click records a coalesced request for
`take_restart_request()` and does not replace the shell. Changing the view clears
an unconsumed request. This view uses ordinary egui rendering and allocation.

The supplied `runner::run_shell_with_recovery` and `run_app_with_recovery` expose an
application recovery callback on each logic tick that starts with a faulted shell.
It receives the egui context, the mutable shell, and (for `run_app`) mutable app state.
Only this recovery callback and the optional guidance run while the shell is faulted;
normal shell actions and event delivery remain blocked. A recovery callback unwind
quarantines even a shell it just replaced. Custom hosts can call the same shell APIs.

For example, an application can condition replacement on the optional button:

```rust
use fairing::{recovery::RestartError, Shell, ShellConfig};
fn recover(ctx: &fairing::egui::Context, shell: &mut Shell) -> Result<(), RestartError> {
    if shell.take_restart_request() {
        // Build/register the replacement appropriate to this application.
        shell.restart_with(|| Shell::builder(ShellConfig::default()).build(ctx))?;
    }
    Ok(())
}
```

An automatic policy can call `restart_with` from that callback without checking a
button. The application owns retry scheduling, logging, state reconciliation and
any recovery route; the library installs none of these policies. A policy that waits
for a deadline can use `ctx.request_repaint_after(delay)` to arrange another tick.
A successful replacement resumes normal dispatch on a subsequent runner callback.
