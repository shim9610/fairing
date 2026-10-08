## Any change

- [ ] `cargo xtask audit` passes locally (or the CI result is attached)
- [ ] Where the guide, the architecture page or the roadmap and the code disagreed, this PR
      fixes the document too

## Only for dependency changes (touching Cargo.toml / deps.allow / deny.toml / assets/**/LICENSE-*)

- [ ] The purpose, and at least two alternatives that were considered
- [ ] The licence is on the allowlist; the owner and maintenance status (a release within the
      last 18 months, or evidence of wide use)
- [ ] The tree growth from `xtask deps-check` output (crate count, duplicate versions) attached
- [ ] Whether `build.rs` uses the network or external tools, and whether there is any `unsafe`
- [ ] Whether it can be isolated behind a feature, and whether that feature is off by default
- [ ] The MSRV impact
- [ ] That it does not pull integrator territory (credentials, image decoding, network
      protocols) into the core
- [ ] On approval, the approver and date recorded in `deps.allow`
