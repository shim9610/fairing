# Contributing

## Setting up

1. Install `rustup` (https://rustup.rs). `rust-toolchain.toml` installs the channel this repository
   builds with (1.99) for you. The crates' declared minimum Rust version is 1.95.
2. `rustup component add clippy rustfmt` / `rustup target add aarch64-unknown-linux-gnu`
3. Install the audit tools pinned to the versions in `xtask/tools.lock`:
   ```sh
   cargo install cargo-deny  --locked --version <the version in xtask/tools.lock>
   cargo install cargo-audit --locked --version <the version in xtask/tools.lock>
   ```
   `xtask/tools.lock` is the single source of truth for those versions. CI reads the same file.

## Running the audit locally

```sh
cargo xtask audit
cargo clippy -p fairing --no-default-features --features mock --all-targets -- -D warnings   # the build with `overlay`/`osk` off
```

That runs stage 0 plus 13 more stages, listed in
[guide 08 §9](docs/guide/08-troubleshooting.md#9-passing-the-audit): integrity, formatting, lints, tests,
docs, duplicate versions, the `deps.allow` comparison, `cargo audit`, `cargo deny`, making
updates visible, the notices file, generated files, the target build, and the blocking/lock scan.

They do not run in the order the table numbers them. The gates that do not compile anything
(0, 13, 5, 6, 10, 11, 7, 8) run first and the compiling stages come after — so that an
unapproved crate's `build.rs` cannot run before the approval gate does. If stage 0
(`cargo xtask integrity`) fails, the compiling stages do not run at all.

Stage 13 (`cargo xtask sync-check`) blocks lock-based shared state (`Mutex`, `RwLock`,
`Condvar`, `Barrier`, `OnceLock`, `LazyLock`, `parking_lot`) and blocking calls on the UI thread
(`crates/fairing/src` and `crates/fairing-widgets/src`: `.recv(`, `.recv_timeout(`, `.join(`,
`thread::sleep`), since threads talk over channels and the UI thread never blocks
([architecture §5](docs/architecture.md#5-threads)). A legitimate exception carries
`// sync-check: allow: <reason>` on the same line (the reason is required).

CI runs `cargo run -p xtask --locked -- audit --strict`, not the alias. The `cargo xtask` alias
is defined by `.cargo/config.toml` inside the repository, so routing CI through it would let one
line disable the audit itself.

## The approval procedure for adding a dependency

1. When a new crate appears in the tree — directly or transitively — its name, version range,
   licence and rationale go in `deps.allow`.
2. A PR touching `Cargo.toml` (root or member), `Cargo.lock`, `deps.allow`, `deny.toml`,
   `rust-toolchain.toml`, `xtask/**`, `.cargo/**`, `vendor/**`, `.github/**`, `**/build.rs`,
   `assets/**/LICENSE-*` or `THIRD_PARTY.md` requires an owner review, via `CODEOWNERS`.
3. Fill in the dependency checklist in the PR template: purpose, alternatives, licence,
   maintenance status, how much the tree grows, `build.rs` and `unsafe`, feature isolation,
   MSRV, and whether it pulls integrator territory into the core.
4. Once approved, leave `description (approver, YYYY-MM-DD)` in the rationale column of the
   `deps.allow` line. Without a date, or with a `TODO` left in, `cargo xtask deps-check` fails —
   so a `--write` dump cannot be merged on its own. If it is rejected, close the PR with the
   reason in it.
5. Assets (icon SVGs, fonts) follow the same procedure and must include an update to
   `THIRD_PARTY.md`. That file is generated, but the region between `<!-- assets:begin -->` and
   `<!-- assets:end -->` is hand-written and survives regeneration.

## The example tours are checks

`tools/tours.sh` runs every example's `--tour` script under Xvfb, stills only, and fails if any
step of any script could not do what it asked. A script presses what it names
(`Act::Tap(Spot::Text("Alerts"))` finds the label on the glass; there is no coordinate form) and says what it expects before each
picture (`Act::Expect(Expect::Text("Lamp hours"))`), so a change that moves a row, renames a
label or breaks a flow fails here rather than leaving a wrong picture under the right file name.
the public repository's CI runs it on every push. When you change what a screen shows, run it; when you add a step to a
script, name the thing you press and check what you expect to see — a coordinate written down
is right for one row height only.

## Rebuilding the README's animations

The GIFs and screenshots in `docs/images/` come from the `demo`, `console` and `kiosk` tours. If a change
alters what one of them shows, rebuild them and commit the result:

```sh
tools/readme-gifs.sh
```

It needs `xvfb-run`, Mesa and Pillow (`pip install pillow`). A tour that fails a check stops
the script, so the images are never rebuilt from a flow that went wrong. Each tour runs with `--record` on a
fixed 60 Hz clock, so the frames do not depend on how fast the machine draws, and
`tools/make_gif.py` stitches them the same way every time: rerun on the machine that made them,
the script reproduced the committed GIFs byte for byte.

The social preview card at the top of the README, `docs/images/social-preview.png`, comes from
`tools/social_preview.py` (Pillow again). After rebuilding it, upload it under the repository's
Settings, General, Social preview as well.

## Keeping the documents up to date

When the documents and the code disagree, fix the document in the same PR that changes the code.
Fixing the document later is not allowed.

The user guide (`docs/guide/*`, indexed by [`docs/guide/README.md`](docs/guide/README.md))
describes **what the code does today**, so a PR that changes a public API, a config key, a
default or an error message fixes the relevant section too. The guide's rust blocks are compiled
as doctests by the audit's test stage (`crates/fairing/src/guide_probe.rs`), so an example that
stops compiling fails CI. Its toml blocks are checked by nothing: keep them values that
`ShellConfig` parses and validates, by hand, when you edit an example. Something planned
but not in the code yet is written in the guide as "not there", and the
[roadmap](docs/roadmap.md) lists it. A change to how the crate is put together updates
[`docs/architecture.md`](docs/architecture.md), and a new limit goes in the roadmap's known limits.

The documents, the README and the comments and doc comments in the source are written in
English.
