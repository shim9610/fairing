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
An isolated infallible-allocation control demonstrates the remaining abort.
That failure is a documented limitation, not a passing production recovery test.

Only this diagnostic crate has a scoped unsafe static-arena ownership transfer;
allocator mechanics and spinlock synchronization are upstream. Both UI crates
retain forbid(unsafe_code). CODEOWNERS and the integrity exception protect this
boundary. The product APIs and limitations are in docs/memory-limits.md.
