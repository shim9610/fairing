# fairing-process

Optional Linux process supervision for a UI whose failure must not terminate its
independent work engine. This crate has no egui dependency. The `fairing/process`
feature re-exports it as `fairing::process`; default fairing builds do not include it.

The engine owns acquisition/control, application state, sessions and authorization.
The renderer owns egui, fonts, textures and window/driver initialization. A renderer
allocation abort closes its private stream; the supervisor reaps it and starts a
fresh generation using the retained latest read-only model. The engine continues
its own work loop throughout. Repeated failure exhausts a finite retry budget and
leaves the display quarantined while engine work continues.

## Integration

```rust,no_run
use fairing_process::{Config, Error, Host, WorkerCommand};
let mut display = Host::new(
    WorkerCommand::new("/opt/device/bin/display"), Config::default()
)?;
// Call from the independent work loop. This model is not a device command queue.
match display.try_publish(&42u64.to_le_bytes()) {
    Ok(_) | Err(Error::Full) => {}, // Keep/retry the latest view in bounded application storage.
    Err(error) => return Err(error.into()),
}
while let Some(intent) = display.try_action() {
    // Validate schema, sample/state revision, current engine authorization and
    // operation id/idempotency before executing any device action.
    let _ = (intent.bytes(), intent.model_sequence(), intent.context());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

The renderer calls `Worker::connect()` before creating egui or loading application
data. It receives `try_snapshot()`, validates/applies its own model schema and uses
that snapshot's opaque token in `frame(Some(token))`. Stage intents using
`Frame::try_action` and call `Worker::try_commit` after the CPU-side pass completes.
`Full` returns ownership of the entire frame; use one bounded pending frame or
explicitly discard it. A model-applied receipt does not acknowledge pixels being
presented by the compositor. `set_waker` can call `egui::Context::request_repaint`
from the I/O thread; the hook must return promptly.

On logout, sample replacement or another authority boundary, call
`Host::invalidate_context()` before accepting more intents. This refuses queued
old-context intents immediately, stops the previous worker and requires a fresh
snapshot. Generations, authority contexts, model sequences and action sequences
never wrap into reused identities within the engine process lifetime. UI intents still need engine authorization:
transport identity does not establish permission, model freshness or exactly-once
execution across crashes. Include application revision and operation identifiers.

This is an explicit application architecture. A `Shell` closure, `&mut Any`, device
backend or an existing callback capture cannot be transmitted to another process.
Recreate screens in the worker around a read-only model and forward intents to the
engine. Do not put the only device state or device credentials in the renderer.

## Transport and bounds

A private inherited `std::os::unix::net::UnixStream::pair` connects the two
processes. There is no listener, named socket, network port, serde format or IPC
framework. stdin and stdout of the renderer are reserved for framed IPC; stderr
is discarded unless the engine explicitly selects `inherit_stderr()`.

Frames have a versioned 40-byte header. Peer-controlled lengths are checked before
body reads into fixed storage; partial reads/writes resume without allocating.
Every peer packet must match the current generation/context. Actions are ordered,
carry their originating model sequence, and follow its completion receipt. Bad
lengths/order/context disconnect the renderer. Duplicated receipts never extend
an in-flight deadline. Deadlines are checked before accepting a late packet.

| Resource | Bound |
| --- | --- |
| Snapshot payload | 4,096 bytes |
| Intent payload | 256 bytes |
| Intents per committed pass | 8 |
| Engine snapshot mailbox / retained latest model | 1 / 1; views may coalesce |
| Renderer snapshot mailbox | 1; one model in flight |
| Renderer committed-frame mailbox | 4 |
| Engine intent mailbox | 16; overflow fails the renderer |
| Protocol work per thread turn | 16 packets; at most 128 read/write attempts per codec call |
| Background threads | One supervisor in engine; one I/O thread in renderer |
| Background thread stack reservations | 2 MiB each; committed memory is separate |
| Retry defaults | 3 retries, 100 ms minimum backoff, doubling up to 2 s |
| Watchdog defaults | 5 s startup, 2 s per in-flight model; no idle redraw requirement |

Publishing, polling intents/snapshots and committing frames use try APIs and never
wait for IPC, capacity, a process start or a process join. Process and stream work
runs on the owner thread. Startup/channel/thread/process creation can allocate in
std; this is not a fixed-heap or hard real-time guarantee. OS socket buffers,
thread/runtime storage and graphics/native allocations need their own budgets.
The only external Rust dependency is already-approved rustix 1.1.5 and its existing
locked dependencies, used for safe process limits and polling. No registry package
or version was added for this feature.

The supervisor applies a child-only hard virtual address-space limit (`RLIMIT_AS`)
and disables core dumps before permitting cooperative renderer initialization.
The default is 256 MiB; valid limits range from 16 MiB through 1 TiB. Calibrate it
against the actual driver: a large virtual mapping need not mean comparable RAM.
Only a small GUI/locale environment whitelist is inherited, including Xauthority
and optional Mesa thread settings. Backend/cloud credentials are not inherited by
default. The executable and startup arguments are trusted engine configuration.

## Device scope and executable evidence

This implementation requires Linux processes and virtual-memory isolation. It is
suitable for evaluation on embedded Linux; it cannot provide this boundary on a
bare-metal MCU or an RTOS without equivalent process isolation. It supervises
trusted renderer code, not a malicious-process sandbox. Kernel-wide physical OOM,
GPU/shared-memory exhaustion, scheduling guarantees, native driver behavior and
application correctness need device-specific evaluation. A process limit does
not reserve RAM for the engine or cap GPU memory; deployments may also need
cgroup memory/CPU limits and a protected engine budget.

A minimal visible integration is included:

```sh
cargo build --locked -p fairing --example process_renderer --features runner-x11,process
cargo run --locked -p fairing-process --example work_engine -- \
  "$PWD/target/debug/examples/process_renderer" 30
```

The example supplies a 1 GiB virtual-address budget; it is not a RAM requirement.
On software Mesa, `LP_NUM_THREADS=1` reduces driver thread/mapping cost. The example
acknowledges model application in `App::logic`, including when UI callbacks stop.
Its button only emits an intent; the separate engine validates the example schema.
A device application must supply its own model, permissions and command semantics.

Run `python3 verification/process.py` for unchanged egui 0.37 UI/tessellation OOM
negative controls and engine-survival/restart checks, plus watchdog, truncated or
oversized frame, replay, wrong-context, queue-overflow and context-change cases.
For the real-window integration, run `verification/window.py` inside `xvfb-run`
with Mesa installed. CI archives both reports under `target/assurance/process`.
The IPC/lifecycle checks are native executions; existing Kani/Verus/Miri claims
for other APIs do not establish an exhaustive proof of this process supervisor.

A local 2026-11-01 `x86_64` debug run measured the following peaks. The visible test
uses a minimal 480×240 window, Xvfb and software Mesa with `LP_NUM_THREADS=1`;
X-server memory is excluded. Values describe these fixtures, not a complete device
application or an ARM/board measurement.

| Fixture | Engine RSS | Renderer RSS | Renderer virtual space | Threads (engine / renderer) |
| --- | ---: | ---: | ---: | ---: |
| Headless real-egui model/intent round trip | 1.84 MiB | 11.71 MiB | 46.46 MiB | 2 / 2 |
| Minimal eframe window | 1.75 MiB | 88.42 MiB | 653.17 MiB | 2 / 6 |

The window run applied 150 models over three seconds without a restart. The
UI/tessellation exhaustion cases each observed SIGABRT in the renderer, continued
engine ticks during unavailability and a successful model/intent from a fresh
generation. This supports embedded-Linux evaluation; the graphics footprint must
still fit the device's RAM alongside its work engine, OS and display server.
