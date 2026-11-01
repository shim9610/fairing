# Bounded admission and allocation failure

Display pressure must refuse or defer optional display work while preserving
accepted state and unrelated commands. Fairing supplies bounded queues, inline
object storage and fallible display admission. **A process-wide fixed heap is
not a supported production recovery policy.** The earlier `fairing/fixed-heap`
feature was removed: egui/epaint and remaining infallible Rust allocations can
abort, so that feature could not provide continued operation under exhaustion.
The diagnostic allocator is isolated from the product dependency graph below.

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
the factory. `Inbox` senders additionally expose `try_send_fallible_with`: a
factory returning Err releases the reservation without enqueuing anything.
A concurrent receiver close can occur after construction starts. Avoid unrelated
side effects in factories. Send errors redact payloads in Debug/Display.

`queue_stats()` samples capacity, outstanding reservations, high-water count,
full rejections and receiver-open state separately. Outstanding excludes an
already received command. Accepted order is retained and no accepted command is
evicted. Admission is not authorization, execution or backend completion.
Critical work needs an explicit refusal/retry and completion policy. Do not spin
retrying on the UI thread or move rejected work into an unbounded retry list.

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
`Shell::try_notify` and `NotificationCenter::try_push` return rejected notification
ownership. Errors distinguish Full, TooLarge and AllocationFailed. A queued
notification/toast command may later be refused by its display retention budget;
command admission alone does not acknowledge display.

The waiting toast queue defaults to 64 items, excluding the configured visible
slots. `toast_queue_capacity(0)` disables new toasts; values through 65,536 are
accepted. `ToastQueue::with_pending_capacity` configures a standalone queue.
`try_push` preserves rejected ownership; `try_push_text` checks capacity before
copying borrowed text. Duplicate text/severity renews or deduplicates existing
items. Text plus owned glyph capacity is limited to 4,096 bytes per toast.

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

Existing `new`/Into constructors and user callbacks may still allocate
infallibly. Use the explicit try APIs before copying untrusted borrowed input;
passing an already-created String cannot recover an earlier caller allocation.
Same-process renderer recovery from complete exhaustion remains unsupported.
The optional Linux [process supervisor](../crates/fairing-process/README.md)
keeps an independent work engine running when the renderer aborts, without
modifying egui. It requires an explicit application model/intent boundary;
it does not transparently move existing shell callbacks to another process.

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

An isolated infallible-allocation child still aborts. That negative diagnostic
records why the global production option was withdrawn; an abort is not an
acceptable production recovery result. Native tests, Miri, Loom, generated
inputs and the API fuzzer cover the bounded contracts with their documented
limits. No whole-application or medical-device certification follows from them.
