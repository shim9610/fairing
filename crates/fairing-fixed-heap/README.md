# fairing-fixed-heap — diagnostic only

Unpublished failure-injection allocator. Neither product library depends on it.
The former fairing/fixed-heap production feature was withdrawn because ordinary
Rust/egui allocation failure can abort instead of allowing continued work.

```sh
FAIRING_HEAP_BYTES=16777216 cargo run -p fairing-fixed-heap \
  --example allocation_pressure --features global
```

The executable links one static Talc arena, with no System fallback. The build
budget is 128 KiB through 1 GiB; the development default is 512 MiB. It is not a
hard real-time allocator. Stacks, native C, OS/driver/GPU and explicitly different
allocators are outside its budget. Do not select it as a device recovery policy.

The example requires real fallible display operations to refuse memory, release
reservations and resume after memory is freed, while preallocated work continues.
The exhaustion checks include cache hits, cold-icon refusal and cache reclamation,
notification dismissal deferral/retry, fallible command factories and rejection paths
that must not invoke an application logger. They keep shell panic quarantine clear.

Separate infallible-allocation and stock-egui-frame controls demonstrate the remaining
abort. The frame control prepares `RawInput` before exhausting the arena, then calls
`Context::run_ui` on the warmed context. That failure is a documented limitation,
not a passing production recovery test. Run both controls through
`python3 verification/memory.py` (Linux); the main pressure trial also runs on Windows.

Only this diagnostic crate has a scoped unsafe static-arena ownership transfer;
allocator mechanics and spinlock synchronization are upstream. Both UI crates
retain `forbid(unsafe_code)`. The integrity gate checks the scoped exception;
CODEOWNERS assigns its reviewer, with enforcement controlled by repository rules.
The product APIs and limitations are in [memory limits](../../docs/memory-limits.md).
