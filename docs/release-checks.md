# Release archive checks

The current candidate is `0.1.0-beta.1`, an unpublished prerelease. Merging the
candidate does not publish it to crates.io. When publishing a beta, publish
`fairing-widgets` first, then `fairing` at the same version; record the release
commit and retain its verification artifacts. Stable 0.1.0 remains a separate release.

## Publish a reviewed beta

The [Publish crates workflow](../.github/workflows/publish.yml) is manually
dispatched from `main`. Enter the release tag and choose a mode:

| Mode | Action |
| --- | --- |
| `verify` (default) | Check the tag and CI, build both archives with `cargo publish --dry-run`, retain the preflight artifacts. No registry credential is used. |
| `bootstrap` | Run verification, then publish with the `crates-io` environment's token. Use for the first release. |
| `trusted` | Run verification, then publish with a short-lived OIDC token. Configure both crates as trusted publishers first. |

The tag must match the workspace version and point to a commit already merged
into `main`. Audit, MSRV, GUI tours, UI histories, proofs, Miri, assurance-gate
and the three CodeQL analyses must have succeeded on that exact commit. A pending
or failed rerun blocks publication. The workflow resolves the tag once and uses
that commit for verification and publication. Existing tags are not moved.

Cargo publishes only `fairing-widgets` and `fairing`, in dependency order, with
the lockfile and all features. `xtask`, `fairing-fixed-heap` and `verification`
remain unpublished. The workflow serializes publication runs and does not publish
on pushes or tag creation.

### First publication: account and environment

1. Sign into [crates.io](https://crates.io/) with `shim9610` and verify the account's
   email. Check that both crate names are available or owned by this account.
2. In [repository environments](https://github.com/shim9610/fairing/settings/environments),
   configure `crates-io` to permit deployment from `main` only.
3. Create a short-lived token in [crates.io token settings](https://crates.io/settings/tokens)
   with publication rights for the two crate names. New names need permission
   to create those crates. Store it as the environment secret
   `CARGO_REGISTRY_TOKEN`; do not put it in a workflow input, Git or a chat message.
4. Run `Publish crates` from `main` with mode `verify`. Inspect the
   `release-preflight` artifact, including the source commit, archive hashes,
   toolchain and dry-run log. Then run the same tag with mode `bootstrap` to publish.
5. Confirm both versions on crates.io and check their docs.rs builds. Update the
   README and getting-started installation sections from Git dependencies to
   registry dependencies after publication. For this beta, the exact requirements
   are `fairing = "=0.1.0-beta.1"` and `fairing-widgets = "=0.1.0-beta.1"`.

Publication cannot overwrite an existing version. If only widgets was published
before an interruption, do not rerun the two-package upload blindly. Confirm its
version and archive checksum against the retained preflight archive, then publish
only `fairing` from the same clean release commit with
`cargo publish --locked --registry crates-io --all-features -p fairing`.

### Later releases: trusted publishing

After both crates exist, add a GitHub trusted publisher in each crate's settings:

| Field | Value |
| --- | --- |
| Repository owner | `shim9610` |
| Repository | `fairing` |
| Workflow filename | `publish.yml` |
| Environment | `crates-io` |

Use mode `trusted` for subsequent releases. Only the publication job has
`id-token: write`; the pinned authentication action obtains and revokes the
temporary registry token. Revoke the bootstrap token and remove its environment
secret after switching. A failed OIDC login does not fall back to that secret.

See the [crates.io trusted publishing documentation](https://crates.io/docs/trusted-publishing)
and [Cargo publication reference](https://doc.rust-lang.org/cargo/commands/cargo-publish.html).
The initial publication requires an API token because trusted publisher
configuration requires an existing crate.

## Local archive checks

Run from the repository root with Python 3.12+, rustup and the normal all-feature
Rust/system build inputs:

```sh
python3 tools/package_check.py
```

For an uncommitted local review, add `--allow-dirty`. CI runs without that option
after the strict audit and rejects dirty package inputs. This command builds and
tests archives; it does not publish to a registry.

## What is checked

1. `cargo package --locked -p fairing-widgets -p fairing --all-features` creates
   and verifies both release archives. Cargo stages the sibling crate locally
   while checking the unpublished pair.
2. The script extracts the `.crate` files into a temporary directory outside the
   checkout. It runs `cargo test` on the extracted widgets crate and all-feature
   `cargo test` on the extracted shell, including unit, integration and API doctests.
3. A standalone application is created beside those extracts. Its source is
   `tools/package_consumer.rs`: it uses the public bridge traits, submits a shell
   command, and verifies rendered screen text through the headless harness.
4. The consumer runs with no default features, the default feature set, and all
   public features. Cargo metadata must identify both dependencies as the extracted
   archives, never the source checkout. The external project also checks formatting.

Before the first release, `fairing-widgets` is unavailable on crates.io. Temporary
manifests therefore patch that sibling to its extracted archive and update only
temporary lockfiles, seeded from the package lockfile. Other dependencies keep their
pinned versions and registry identities. Package construction uses the repository
lockfile, and extracted tests/consumers use offline,
locked resolution after their temporary lockfiles are prepared. Repository manifests,
lockfiles and Cargo source configuration are not rewritten.
The script rejects new third-party identities/versions in temporary resolution and
checks that the repository lockfile hash is unchanged.

## Fresh consumer resolution on the minimum Rust version

Install the declared minimum toolchain and the pinned `cargo-deny`, then run:

```sh
python3 tools/fresh_consumer_check.py
```

This separate CI gate extracts both archives and creates an external application
without a lockfile. It resolves permitted registry versions online, runs the
minimal/default/all-feature consumer with the declared minimum Rust version,
and checks advisories, bans, licences and sources with `deny.toml` for its
configured Linux targets. Only the unpublished sibling uses a local patch.
The repository lockfile is unchanged. Logs, the newly resolved lockfile, metadata,
archive hashes and results are retained under `target/fresh-consumer-check/`.
Use `--allow-dirty` for local uncommitted reviews.

## Guide tests and package tests

The full guide reads `docs/guide/` outside the `fairing` crate directory. Those
pages and `src/guide_probe.rs` are intentionally absent from the release archive.
The audit sets `RUSTDOCFLAGS="--cfg fairing_repo_docs"` for its all-feature tests;
the Ferrocene compatibility test command uses the same explicit gate.

For a focused repository-guide run:

```sh
RUSTDOCFLAGS="--cfg fairing_repo_docs" \
  cargo test --locked -p fairing --all-features --doc
```

Ordinary archive doctests do not enable that cfg, and still test the public API
examples. The package script overrides inherited repository rustdoc flags with
`-D warnings` so excluded pages cannot be enabled accidentally. Font-warning tests
register valid egui-bundled font data in the Strong family and also exercise
unreadable-font rejection; they do not require the repository's example fonts.

## Evidence and scope

Logs, archive SHA-256 hashes, toolchain identity and command exit codes are retained
under `target/package-check/`, including a `summary.json`. The CI job uploads this
directory as the `package-check` artifact even on failure.

CI artifacts are temporary (14 days for package/assurance results and 7 days for
tours). For a release or customer verification delivery, export the selected run's
logs and artifacts to the project's retained evidence storage before they expire.
Include the source commit, lockfile and toolchain identities, feature/platform
configuration, test results, and applicable proof receipts and unresolved anomalies.
Preserve that bundle with the delivered version; ordinary development CI retention
is not the release evidence archive.

These checks cover the current host toolchain, packaged test suite and headless
consumer. They do not open a window, validate GPU presentation, publish either
crate, or test downloading the unpublished sibling from crates.io. For the first
release, publish the widgets dependency before the shell.
