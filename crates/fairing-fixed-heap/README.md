# fairing-fixed-heap

Optional process-wide fixed Rust heap for a device binary. Enable
`fairing = { features = ["fixed-heap"], ... }`, or this crate's `global` feature.
Do not combine it with another global allocator.

```sh
FAIRING_HEAP_BYTES=67108864 cargo build --release --features fixed-heap
```

The selected allocator has one static arena and no system-heap fallback.
Vec/String/Box allocations routed through the Rust global allocator share this
budget, including fairing and egui. It does not bound thread stacks, native C
allocations, OS/graphics-driver allocations or GPU memory. It is not a hard
real-time allocator guarantee.

The 512 MiB default supports the repository's parallel development tests; set a
measured budget for the actual device. Valid sizes are 128 KiB through 1 GiB.
Talc reuses freed blocks. Infallible Rust allocations may abort on exhaustion:
use bounded admission, inline object pools and fallible allocation where recovery
is required. This option constrains the heap; it cannot make every existing
allocation recoverable or eliminate heap activity.

Both UI crates retain their unsafe-code prohibition. This separate crate has one
reviewed unsafe static-arena initialization, delegating allocator mechanics and
locking to pinned upstream dependencies.
