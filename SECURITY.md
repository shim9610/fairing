# Security

## Supported versions

| Version | Security fixes |
|---|---|
| Development toward 0.1.0 (unreleased) | Fixes land on the current development branch |

Before 1.0, fixes land on the latest minor release only.
The first crates.io release has not been published yet.

## Reporting a vulnerability

Please report it privately, not in a public issue: open the repository's **Security** tab and
choose **Report a vulnerability**. Say what you found, which version, and how to reproduce it.
You will get an answer within a week, and a fix or a plan before anything is made public.

## What the crate does and does not protect

- **Access gates are a UI boundary, not an OS one.** Levels, gates and the unlock prompt decide
  what the shell shows and opens. They do not sandbox your process: code in your binary, and
  anyone with a shell on the device, is outside their reach.
- **`PinTable` keeps secrets in clear text.** It is the reference authenticator, read from
  `fairing.toml`, and it says so in a warning at start-up. Protecting that file is the device's
  job; for anything stronger, implement `Authenticator` yourself (guide 05).
  Configuration diagnostics omit PIN/pattern values; malformed TOML is reported by
  location without echoing its source or invalid values.
- **The crate writes no files and makes no network connections.** Backends, storage and
  networking are the integrator's code, so their security is too.
- **Authentication request tokens correlate trusted results.** Custom authenticators
  must retain the original `AuthRequestId` with asynchronous work. Tokens reject stale
  replies inside the shell; they do not authenticate network peers.
- **Recovery does not undo completed side effects.** Caught runtime unwinds quarantine
  the shell; the host controls replacement and reconciliation. Abort/OOM and panic-hook
  behavior have separate limits documented in [memory limits](docs/memory-limits.md#runtime-panics-and-sensitive-ui-state).

A report about any of these behaving differently from what is written here is welcome.
