# Security

## Supported versions

| Version | Security fixes |
|---|---|
| 0.1.x | Yes |

Before 1.0, fixes land on the latest minor release only.

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
- **The crate writes no files and makes no network connections.** Backends, storage and
  networking are the integrator's code, so their security is too.

A report about any of these behaving differently from what is written here is welcome.
