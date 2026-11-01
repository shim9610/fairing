# Bounded queues and a fixed Rust heap

Fairing provides three independent controls: a bounded command inbox, inline
object storage and an optional process-wide fixed Rust heap. Queue/pool limits
bound item counts; the heap option also bounds their Rust-allocated payloads.
These controls do not establish medical-device suitability or hard real-time
behavior.

## Command admission and update work

```rust
use fairing::{Shell, ShellConfig};
fn build(ctx: &fairing::egui::Context, config: ShellConfig) -> fairing::Result<Shell> {
    Shell::builder(config)
        .command_queue_capacity(64)
        .max_commands_per_update(16)
        .build(ctx)
}
```

The defaults are 1,024 command slots and 256 commands per UI/logic update.
Both options accept 1 through 65,536; invalid values fail `build` with
`Error::Config`. This limits command count, not callback duration or bytes in
Strings, images or other payloads. Accepted commands retain transport order and
are authorized when executed. Queue admission is not an acknowledgement that
an action executed or that a backend completed it.

Use `ShellHandle::try_send(command)` to obtain `Full` or `Disconnected` and
recover the original command with `error.into_inner()`. Nothing already accepted
is evicted to make space. Do not spin retrying on the UI thread: choose a bounded
retry, explicit refusal or worker-side coalescing policy appropriate to the
operation. Critical commands, including authentication/session changes, need an
explicit rejection and completion policy. The older `send` returns `false` for
either rejection; void convenience methods log a redacted rejection and cannot
report it to their caller.

`handle.try_send_with(|| command)` reserves a slot before invoking the closure.
An already-full or closed queue skips the closure, avoiding unnecessary payload
allocation. If the receiver closes concurrently after reservation, the closure
may run and its result may be dropped; keep unrelated side effects out of it.
An unwinding closure releases its reservation. Debug/Display of send errors
prints only the rejection reason, never the payload.

`handle.queue_stats()` samples `capacity`, `outstanding`, `high_water`,
`rejected_full` and `receiver_open`. Fields are sampled separately, not as an
atomic snapshot. Outstanding work includes in-progress payload factories and
queued messages, but excludes a command already received for execution. Full
rejection accounting saturates. Counters do not contain payload data.

## Backend and worker channels

```rust
use fairing::inbox::Inbox;
let (sender, receiver) = Inbox::<u32>::bounded(8)?;
sender.try_send_with(|| 42)?;
assert_eq!(receiver.try_recv()?, 42);
# Ok::<(), Box<dyn std::error::Error>>(())
```

All cloned senders share the same capacity. Native transport preallocates its
slots using `sync_channel`; only `try_send` is exposed. `drain()` receives at most
capacity messages, and `drain_up_to(n)` at most n, even while producers refill.
Dropping/receiving messages returns reservations. Capacity zero is deliberately
rejected. Construction validates capacity/layout, but allocation itself can
still exhaust memory. Standard channels may synchronize internally: no operation
waits *for capacity*, but these APIs are not wait-free or hard real-time.

`Inbox::pair()` remains an unbounded compatibility API. Existing backend and
screenshot channels are not automatically converted. A full fixed heap still
limits their routed Rust allocations, but does not give them a recoverable
queue-full protocol. Integrators should select bounded channels at their own
worker/service boundaries.

## Inline object pools

```rust
use fairing::memory::FixedPool;
let mut pool = FixedPool::<u32, 8>::new()?;
let key = pool.try_insert(42).map_err(|_| "pool full")?;
assert_eq!(pool.remove(key), Some(42));
assert!(pool.get(key).is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

`FixedPool<T, N>` stores N inline slots and never allocates container storage.
`try_insert` returns the original value when full; `get`, `get_mut`, `remove` and
`clear` support reuse. Opaque keys reject another pool's identity and removed or
replaced generations. Moving the pool preserves keys. Identities never wrap;
`new` can fail on identity exhaustion. A slot at generation `u64::MAX` is retired
after removal, reducing `available()` without changing `capacity()`.

A `String` stored in the pool still owns its own heap buffer. Choose inline
payloads or also select the fixed heap. Large N can overflow a thread's stack;
place large pools in suitable static/application storage or allocate them in the
selected heap. A payload's destructor remains application code.

## Opt-in fixed global Rust heap

```toml
[dependencies]
fairing = { version = "0.1", features = ["fixed-heap"] }
```

```sh
FAIRING_HEAP_BYTES=67108864 cargo build --release --features fixed-heap
# Repository headless trial, with real egui frames:
FAIRING_HEAP_BYTES=67108864 cargo run -p fairing --example memory_limits --features fixed-heap
```

This explicitly installs a global allocator backed by one static arena. Its sole
memory source is that arena: it cannot grow or fall back to the system heap.
Vec/String/Box and other allocations routed through Rust's global allocator in
fairing, egui, dependencies and the application all share it. Freed blocks are
reused. `fairing::fixed_heap::try_stats()` returns allocation/free-space counters
or `None` when the allocator is busy, without waiting for its spinlock.

`FAIRING_HEAP_BYTES` is a decimal build-time byte count, 128 KiB through 1 GiB.
The default is 512 MiB for the repository's parallel development tests; select a
measured device budget explicitly. Allocator metadata/alignment consume part of
the arena, and fragmentation can prevent a request even when the sum of free
bytes is large enough. The feature is off by default and cannot coexist with
another global allocator. Cargo `--all-features` also selects this allocator.

**Exhaustion is not universally recoverable.** `Vec::try_reserve*` can return an
allocation error, but existing infallible Vec/Box/egui allocations may abort the
process. `catch_unwind` cannot recover from allocator OOM aborts. This option
limits routed heap growth; it does not make the renderer allocation-free, turn
every allocation into `Result`, or guarantee progress when memory is exhausted.
Provision headroom and design the device's fault response independently.

Thread stacks, native C allocations, OS resources, graphics-driver/GPU memory and
explicit alternative allocators are outside this arena. There is no claim that
total resident memory equals the configured byte budget.

The separate `fairing-fixed-heap` crate uses pinned Talc 5.1.1 and spinning_top
0.3.1, with allocator-api2 0.4.0 transitively. Talc handles allocation; access uses
a spinlock and has no latency bound here. The only local unsafe block transfers
exclusive ownership of the static arena to Talc. Both UI crates retain
`forbid(unsafe_code)`. The integrity gate allows this one path-specific boundary;
its source, dependencies and exception require owner review in the PR.

## Verification and limits

`python3 verification/memory.py` runs the normal allocator plus 16 MiB and 64 MiB
arenas. It requires oversized fallible requests to fail, reuses 10,000 queue/pool
operations with zero additional allocations, churns more cumulative bytes than
the arena, and draws 120 actual egui frames. Isolated negative controls require
infallible exhaustion to terminate, documenting the failure mode.

Public API state models compare FIFO contents, rejected factories, ownership and
live/stale/foreign pool handles under proptest, Miri and the API fuzzer. Kani
checks bounded pool sequences and all u64 generation values; Verus checks the
exact generation function. Both reject a deliberate generation-wrap defect.
Loom checks the production atomic admission protocol with a substitute channel
because Loom 0.7.2 has no `sync_channel`. Its channel stub cannot reliably model
failed sends racing receiver destruction: close races use real channels natively
and under Miri. These are bounded checks, not an exhaustive proof of the
allocator, egui or the entire application.
