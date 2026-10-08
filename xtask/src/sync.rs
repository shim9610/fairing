//! The blocking/sync scan — a gate that **does not compile**.
//!
//! It enforces the no-locks and no-blocking principles by scanning the source:
//!
//! - No lock-based shared state (`Mutex` · `RwLock` · `Condvar` · `Barrier` · `OnceLock` ·
//!   `LazyLock` · `parking_lot`) **in any crate**. Between threads, only `std::sync::mpsc`
//!   channels and `std::sync::atomic`. egui's `Context::tex_manager()` hands out an
//!   `Arc<RwLock<..>>` as it stands, so it is blocked by name (the calling side has no `RwLock`
//!   in its text).
//! - The UI thread makes no blocking call such as `.recv(` · `.recv_timeout(` · `.join()` ·
//!   `thread::sleep`. Its code is `crates/fairing` and `crates/fairing-widgets` (the controls draw
//!   on it too — the crate split had left them outside this rule). A backend's worker, which may
//!   wait on its own queue, lives in the integrator's crate, not here.
//!
//! The root `clippy.toml`'s `disallowed-types` blocks the same lock types at compile time (stage
//! 2), but that alone is not enough: an alias such as `use std::sync::RwLock as Lock;` catches only
//! where the type is actually used, `parking_lot` is a crate rather than a type, and a blocking
//! call on the UI thread is not a type problem at all. This stage catches them in text, before the
//! compile.
//!
//! The check is a text scanner. `//` line comments, `/* */` block comments and string and character
//! literals are blanked out and only the code left is looked at. Lock tokens are found on word
//! boundaries and blocking calls by their literal spelling. A legitimate exception is passed by
//! putting `// sync-check: allow: <reason>` on the same line (the reason is required).

use crate::util::{self, Error, Result};
use std::path::{Path, PathBuf};

/// The identifiers that must appear in no crate. **The allow list turned inside out**: what
/// `std::sync` and `std::cell` offer, minus what the no-locks rule permits.
///
/// Permitted, so absent here: `Arc` · `Weak` · `atomic::*` (the rule names atomics as the answer),
/// `mpsc::{channel, Sender, Receiver}` (the shell's own command queue), and `Cell` · `RefCell` ·
/// `OnceCell` (single-threaded; the rule allows `RefCell` by name).
///
/// Everything else in those two modules is here, the pieces the original six missed included:
///
/// - `Once` — the initialisation race `OnceLock` was banned for, without the value attached.
/// - `ReentrantLock` — a lock.
/// - `LazyCell` — single-threaded, but it panics where the initialiser re-enters.
/// - `UnsafeCell` — unreachable under `unsafe_code = "forbid"` anyway, named so the intent is on record.
/// - `SyncSender` · `sync_channel` — a bounded channel's `send` **blocks**, and at capacity 0 it is a
///   rendezvous. The blocking list below cannot reach it: `Sender::send` is spelt the same.
/// - `Exclusive` · `mpmc` — unstable today. Here so they cannot arrive quietly.
/// - The guard types — identifiers match whole, so `MutexGuard` is **not** caught by `Mutex`, and a
///   signature can name a guard without naming its lock.
///
/// `tex_manager` is not a type of ours but egui's accessor — `Context::tex_manager()` hands an
/// `Arc<RwLock<TextureManager>>` back as it stands (`egui-0.36.1/src/context.rs`), so the calling
/// side's source has no `RwLock` in it and clippy's type-based `disallowed-types` cannot catch it
/// either. Blocking it by name is the only defence, so it lives here.
const LOCK_TOKENS: [&str; 20] = [
    "Mutex",
    "MutexGuard",
    "RwLock",
    "RwLockReadGuard",
    "RwLockWriteGuard",
    "ReentrantLock",
    "ReentrantLockGuard",
    "Condvar",
    "Barrier",
    "Once",
    "OnceLock",
    "LazyLock",
    "LazyCell",
    "UnsafeCell",
    "SyncSender",
    "sync_channel",
    "Exclusive",
    "mpmc",
    "parking_lot",
    "tex_manager",
];

/// The blocking calls banned on the UI thread. They are found by their literal spelling rather than
/// as identifiers. `std::thread::sleep` is covered by `thread::sleep`, so it is not listed
/// separately, and `thread::park` covers `park_timeout` the same way.
///
/// `thread::scope` is here because it **joins at the end of the scope** — a wait with no `.join()`
/// anywhere in the source to catch.
///
/// `.join()` is looked for **in its argument-less form only**. `JoinHandle::join` takes no
/// arguments, and a `.join(..)` with one is something unrelated to blocking, such as `Path::join`,
/// `PathBuf::join` or `slice::join` — catching broadly on `.join(` would mean an exception marker on
/// every path assembly.
///
/// **What a text scan cannot reach — and who holds it instead**. Iterating a `Receiver`
/// blocks, and neither `rx.iter()` nor `for msg in rx` can be told from a slice's spelling here. So
/// that hole is closed twice over, above this scan:
///
/// - `clippy.toml` names `Receiver::{recv, recv_timeout, recv_deadline, iter}` in
///   `disallowed-methods` and `Receiver` itself in `disallowed-types`. Clippy resolves them **by
///   type**, so an alias or a re-export cannot slip past the way it can past a text match.
/// - `for msg in rx` is the one form no lint reaches: it desugars to `IntoIterator::into_iter`, and
///   banning that would ban every `for` loop. `fairing::inbox::Inbox` owns the receiver, hands out
///   `try_recv` and a non-blocking `drain`, and never lends it out — so in this workspace there is
///   no receiver left to write that loop over.
///
/// The list below still earns its place: it is crate-scoped, so it can ban `thread::sleep` and
/// `.join()` on the UI thread while leaving a backend on its own thread alone, which a
/// workspace-wide `clippy.toml` cannot.
const BLOCKING_PATTERNS: [&str; 7] = [
    ".recv(",
    ".recv_timeout(",
    ".recv_deadline(",
    ".join()",
    "thread::sleep",
    "thread::park",
    "thread::scope",
];

/// The crate directories the UI thread rule applies to: the shell and its controls.
const UI_CRATES: &[&str] = &["fairing", "fairing-widgets"];

/// The exception marker. A reason must follow it.
const ALLOW_MARK: &str = "// sync-check: allow:";

/// The lock rule's name (attached to a violating line).
const LOCK_RULE: &str = "no lock-based shared state";

/// The UI thread rule's name.
const BLOCKING_RULE: &str = "no blocking calls on the UI thread";

/// The rule name attached to an exception marker with no reason.
const MARK_RULE: &str = "the exception marker has no reason";

/// The scan's result.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Report {
    /// How many `.rs` files were checked.
    pub(crate) files: usize,
    /// How many lines were passed by an exception marker with a reason.
    pub(crate) allows: usize,
    /// The violations, as `file:line: <token> — rule`.
    pub(crate) violations: Vec<String>,
}

/// Stage 13's body. `true` on a pass.
pub(crate) fn check(root: &Path) -> Result<bool> {
    let report = scan(root)?;
    println!(
        "blocking/sync: checked {} .rs files under crates/**/src ({} exception markers)",
        report.files, report.allows
    );
    if report.violations.is_empty() {
        println!("No lock types. No blocking calls on the UI thread.");
        return Ok(true);
    }
    for violation in &report.violations {
        println!("{violation}");
    }
    println!(
        "{} violation(s). Between threads, use only std::sync::mpsc channels and std::sync::atomic. \n\
         A legitimate exception carries `{ALLOW_MARK} <reason>` on the same line.",
        report.violations.len()
    );
    Ok(false)
}

/// The rule an orphan token breaks.
const ORPHAN_RULE: &str =
    "a public metrics field nothing reads — a knob that does not turn is worse than no knob";

/// It walks all of `crates/*/src/**/*.rs`. With no `crates/`, the result is empty.
pub(crate) fn scan(root: &Path) -> Result<Report> {
    let mut report = Report::default();
    let crates = root.join("crates");
    if !crates.is_dir() {
        return Ok(report);
    }
    let mut sources: Vec<(PathBuf, String)> = Vec::new();
    for crate_dir in sorted_dirs(&crates)? {
        let src = crate_dir.join("src");
        if !src.is_dir() {
            continue;
        }
        let ui = crate_dir
            .file_name()
            .is_some_and(|name| UI_CRATES.iter().any(|ui| name == std::ffi::OsStr::new(ui)));
        let mut files = Vec::new();
        collect_rs(&src, &mut files)?;
        files.sort();
        for file in &files {
            report.files += 1;
            scan_file(root, file, ui, &mut report)?;
            sources.push((file.clone(), std::fs::read_to_string(file)?));
        }
    }
    orphan_tokens(root, &sources, &mut report);
    Ok(report)
}

/// **A theme token that nothing reads.**
///
/// The crate's own component-token test opens with "a knob that does not turn is worse than no
/// knob", and it checks that the tokens it names reach a widget. Nothing checked that a token
/// reaches *anything*, so when the desktop badge moved to `CountBadge` its three old metrics stayed
/// behind — declared, defaulted, documented in the customization guide as tunable, and read by no
/// one. An integrator setting them saw nothing happen.
///
/// The check is deliberately narrow: a `pub` field on a struct whose name ends in `Metrics`, read
/// nowhere in `crates/**/src` outside the file that declares it. Reads from tests and examples do
/// not count — a token exercised only by a test still does not turn anything.
fn orphan_tokens(root: &Path, sources: &[(PathBuf, String)], report: &mut Report) {
    for (file, text) in sources {
        let mut inside = false;
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if let Some(rest) = line.strip_prefix("pub struct ") {
                inside = rest
                    .split_whitespace()
                    .next()
                    .is_some_and(|name| name.trim_end_matches('{').ends_with("Metrics"));
                continue;
            }
            if line == "}" {
                inside = false;
                continue;
            }
            if !inside {
                continue;
            }
            let Some(field) = line.strip_prefix("pub ").and_then(|r| r.split(':').next()) else {
                continue;
            };
            if field.is_empty() || !field.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                continue;
            }
            // The field's own name, not `.name`: `has_token` wants a word boundary on both
            // sides, and the `.` in `c.pad` has a word character before it, so every nested
            // component token came back an orphan on the first run. A bare name over-accepts for
            // short ones (a local called `pad` counts as a read), which is the safe direction for
            // a blocking rule — what it is here to catch is a token mentioned **nowhere**.
            //
            // A read **in the declaring file counts**: a token the theme itself derives another
            // token from does turn something. What does not count is the declaration and the
            // default that fills it in, which are the two lines every orphan still has.
            let read = sources.iter().any(|(_, body)| {
                body.lines().any(|other| {
                    let text = other.trim();
                    has_token(other, field)
                        && !text.starts_with("pub ")
                        && !text.starts_with(&format!("{field}:"))
                        && !text.starts_with("///")
                        && !text.starts_with("//!")
                })
            });
            if !read {
                let (shown, number) = (display(root, file), index + 1);
                report
                    .violations
                    .push(format!("{shown}:{number}: {field} — {ORPHAN_RULE}"));
            }
        }
    }
}

/// The subdirectories immediately below a directory, in name order.
fn sorted_dirs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in read_dir(dir)? {
        if entry.is_dir() {
            out.push(entry);
        }
    }
    out.sort();
    Ok(out)
}

/// The paths of a directory's entries.
fn read_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(dir)
        .map_err(|err| Error::new(format!("cannot read {}: {err}", dir.display())))?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            Error::new(format!("cannot read an entry of {}: {err}", dir.display()))
        })?;
        out.push(entry.path());
    }
    Ok(out)
}

/// Gather the `.rs` files recursively.
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for path in read_dir(dir)? {
        if path.is_dir() {
            collect_rs(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// One file. With `ui`, the UI thread rule applies as well.
fn scan_file(root: &Path, file: &Path, ui: bool, report: &mut Report) -> Result<()> {
    let text = util::read_file(file)?;
    let shown = display(root, file);
    let mut scanner = Scanner::new();
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = scanner.strip(raw);
        match allow_reason(line.comment.as_deref()) {
            Some("") => {
                report
                    .violations
                    .push(format!("{shown}:{number}: {ALLOW_MARK} — {MARK_RULE}"));
            }
            Some(_) => {
                report.allows += 1;
                continue;
            }
            None => {}
        }
        for token in LOCK_TOKENS {
            if has_token(&line.code, token) {
                report
                    .violations
                    .push(format!("{shown}:{number}: {token} — {LOCK_RULE}"));
            }
        }
        if ui {
            for pattern in BLOCKING_PATTERNS {
                if line.code.contains(pattern) {
                    report
                        .violations
                        .push(format!("{shown}:{number}: {pattern} — {BLOCKING_RULE}"));
                }
            }
        }
    }
    Ok(())
}

/// Take the exception marker's reason out of a line comment. `None` with no marker, `Some("")` with an empty reason.
fn allow_reason(comment: Option<&str>) -> Option<&str> {
    let comment = comment?;
    let at = comment.find(ALLOW_MARK)?;
    let rest = comment.get(at + ALLOW_MARK.len()..)?;
    Some(rest.trim())
}

/// The path relative to the repository root.
fn display(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string()
}

/// Find an identifier token on word boundaries. `MyMutex` and `mutex_free` are not caught.
fn has_token(code: &str, token: &str) -> bool {
    let bytes = code.as_bytes();
    let mut from = 0;
    while let Some(rel) = code.get(from..).and_then(|tail| tail.find(token)) {
        let start = from + rel;
        let end = start + token.len();
        let before = start == 0 || !bytes.get(start - 1).copied().is_some_and(is_ident_byte);
        let after = !bytes.get(end).copied().is_some_and(is_ident_byte);
        if before && after {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// One line with its comments and literals blanked out.
struct Line {
    /// The code with the comments' and literals' contents replaced by spaces.
    code: String,
    /// The `//` line comment that started on this line (where there is one, from the `//` to the end of the line).
    comment: Option<String>,
}

/// A line-by-line scanner carrying the state that spans lines (block comments, strings).
///
/// Not a complete Rust lexer — only as much as this gate needs. What it blanks out is `//` line
/// comments, `/* */` block comments (nested ones included), ordinary and byte strings, `r"…"` /
/// `r#"…"#` raw strings, and character literals. Lifetime notation (`'a`) is left as code.
struct Scanner {
    /// The block comment's nesting depth.
    block: usize,
    /// Inside a raw string, the number of opening `#`s.
    raw: Option<usize>,
    /// Whether it is inside an ordinary string.
    string: bool,
}

impl Scanner {
    fn new() -> Self {
        Self {
            block: 0,
            raw: None,
            string: false,
        }
    }

    /// Blank one line out. The state carries on to the next.
    fn strip(&mut self, line: &str) -> Line {
        let chars: Vec<char> = line.chars().collect();
        let mut code = String::new();
        let mut comment = None;
        let mut at = 0;
        while at < chars.len() {
            if let Some(step) = self.inside(&chars, at, &mut code) {
                at += step;
                continue;
            }
            let current = chars.get(at).copied().unwrap_or(' ');
            if current == '/' && chars.get(at + 1) == Some(&'/') {
                comment = Some(chars.get(at..).unwrap_or_default().iter().collect());
                break;
            }
            if current == '/' && chars.get(at + 1) == Some(&'*') {
                self.block = 1;
                code.push_str("  ");
                at += 2;
                continue;
            }
            if let Some(step) = self.open_raw(&chars, at, &mut code) {
                at += step;
                continue;
            }
            if current == '"' {
                self.string = true;
                code.push(' ');
                at += 1;
                continue;
            }
            if current == '\'' {
                if let Some(step) = char_literal(&chars, at) {
                    blank(&mut code, step);
                    at += step;
                    continue;
                }
            }
            code.push(current);
            at += 1;
        }
        Line { code, comment }
    }

    /// Inside a block comment or a string, consume one step and hand back the step count.
    fn inside(&mut self, chars: &[char], at: usize, code: &mut String) -> Option<usize> {
        let current = chars.get(at).copied()?;
        let next = chars.get(at + 1).copied();
        if self.block > 0 {
            if current == '*' && next == Some('/') {
                self.block -= 1;
            } else if current == '/' && next == Some('*') {
                self.block += 1;
            } else {
                code.push(' ');
                return Some(1);
            }
            code.push_str("  ");
            return Some(2);
        }
        if let Some(hashes) = self.raw {
            if current == '"' && closes_raw(chars, at + 1, hashes) {
                self.raw = None;
                blank(code, hashes + 1);
                return Some(hashes + 1);
            }
            code.push(' ');
            return Some(1);
        }
        if self.string {
            if current == '\\' {
                blank(code, 2);
                return Some(2);
            }
            if current == '"' {
                self.string = false;
            }
            code.push(' ');
            return Some(1);
        }
        None
    }

    /// Consume a raw string where one opens with `r"`, `r#"` or `br"`.
    fn open_raw(&mut self, chars: &[char], at: usize, code: &mut String) -> Option<usize> {
        let start = match chars.get(at).copied()? {
            'r' => at,
            'b' if chars.get(at + 1) == Some(&'r') => at + 1,
            _ => return None,
        };
        // Where an identifier character precedes it, the `r` is part of a name.
        if at > 0
            && chars
                .get(at - 1)
                .copied()
                .is_some_and(|prev| prev.is_alphanumeric() || prev == '_')
        {
            return None;
        }
        let mut cursor = start + 1;
        while chars.get(cursor) == Some(&'#') {
            cursor += 1;
        }
        if chars.get(cursor) != Some(&'"') {
            return None;
        }
        self.raw = Some(cursor - start - 1);
        let step = cursor + 1 - at;
        blank(code, step);
        Some(step)
    }
}

/// Whether there are enough `#`s after a raw string's closing `"`.
fn closes_raw(chars: &[char], at: usize, hashes: usize) -> bool {
    (0..hashes).all(|offset| chars.get(at + offset) == Some(&'#'))
}

/// The length where it is a character literal, `None` where it is lifetime notation.
fn char_literal(chars: &[char], at: usize) -> Option<usize> {
    if chars.get(at + 1).copied()? == '\\' {
        // An escaped character is at+2 — that place can itself be a quote, as in `'\''`, so the
        // closing quote is looked for from at+3.
        let end = (at + 3..=at + 12).find(|index| chars.get(*index) == Some(&'\''))?;
        return Some(end - at + 1);
    }
    if chars.get(at + 2) == Some(&'\'') {
        return Some(3);
    }
    None
}

/// Append `count` spaces (the length is matched to preserve the column positions).
fn blank(code: &mut String, count: usize) {
    for _ in 0..count {
        code.push(' ');
    }
}

#[cfg(test)]
mod tests {
    use super::{scan, Report, Scanner};
    use crate::util::{Error, Result};
    use std::path::PathBuf;

    /// A temporary tree for the tests. With no dev-dependency, only `std::env::temp_dir()` is used.
    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new(tag: &str) -> Result<Self> {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|err| Error::new(err.to_string()))?
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "fairing-sync-check-{tag}-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).map_err(Error::from)?;
            Ok(Self { root })
        }

        fn file(&self, rel: &str, text: &str) -> Result<()> {
            crate::util::write_file(&self.root.join(rel), text)
        }

        fn scan(&self) -> Result<Report> {
            scan(&self.root)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Take just the token out of a violation line, `file:line: <token> — rule`.
    fn tokens(report: &Report) -> Vec<&str> {
        report
            .violations
            .iter()
            .filter_map(|line| line.split_once(": "))
            .filter_map(|(_, rest)| rest.split(" — ").next())
            .collect()
    }

    /// **The gaps the original six left.** Each of these is a way to block, or to panic, that the
    /// first list walked past — a bounded channel's `send`, `Once`'s initialisation race,
    /// `LazyCell`'s re-entrant initialiser, a guard named without its lock, and the two thread waits
    /// that never spell `.join()`.
    #[test]
    fn the_widened_list_catches_what_the_first_six_missed() -> Result<()> {
        let tree = Tree::new("widened")?;
        tree.file(
            "crates/fairing/src/lib.rs",
            concat!(
                "use std::sync::Once;\n",
                "fn a(g: MutexGuard<u8>) {}\n",
                "fn b() { let (tx, rx) = std::sync::mpsc::sync_channel(0); }\n",
                "fn c(x: std::cell::LazyCell<u8>) {}\n",
                "fn d() { std::thread::park(); }\n",
                "fn e() { std::thread::scope(|s| {}); }\n",
            ),
        )?;
        let report = tree.scan()?;
        let got = tokens(&report);
        for want in [
            "Once",
            "MutexGuard",
            "sync_channel",
            "LazyCell",
            "thread::park",
            "thread::scope",
        ] {
            assert!(got.contains(&want), "`{want}` was not caught: {got:?}");
        }
        Ok(())
    }

    /// The permitted ones stay permitted — an allow list inverted is only useful if it does not
    /// swallow what the no-locks rule named as the answer.
    #[test]
    fn the_permitted_primitives_are_not_caught() -> Result<()> {
        let tree = Tree::new("permitted")?;
        tree.file(
            "crates/fairing/src/lib.rs",
            concat!(
                "use std::sync::atomic::{AtomicBool, Ordering};\n",
                "use std::sync::{mpsc, Arc};\n",
                "use std::cell::{Cell, OnceCell, RefCell};\n",
                "fn a(rx: &mpsc::Receiver<u8>) { while let Ok(_) = rx.try_recv() {} }\n",
                "fn b(c: &Cell<u8>, r: &RefCell<u8>, o: &OnceCell<u8>, x: &Arc<AtomicBool>) {}\n",
            ),
        )?;
        let report = tree.scan()?;
        assert!(
            report.violations.is_empty(),
            "a permitted primitive was caught: {:?}",
            report.violations
        );
        Ok(())
    }

    #[test]
    fn a_lock_type_anywhere_is_a_violation() -> Result<()> {
        let tree = Tree::new("lock")?;
        tree.file(
            "crates/fairing/src/lib.rs",
            "use std::sync::Mutex;\nfn f(m: &Mutex<u8>) {}\n",
        )?;
        tree.file(
            "crates/other/src/lib.rs",
            "static X: std::sync::OnceLock<u8> = std::sync::OnceLock::new();\n",
        )?;
        let report = tree.scan()?;
        assert_eq!(report.files, 2);
        assert_eq!(tokens(&report), vec!["Mutex", "Mutex", "OnceLock"]);
        Ok(())
    }

    #[test]
    fn an_aliased_use_is_still_a_violation() -> Result<()> {
        let tree = Tree::new("alias")?;
        tree.file(
            "crates/fairing/src/lib.rs",
            "use std::sync::RwLock as Shared;\nuse parking_lot::Mutex as Fast;\n",
        )?;
        let report = tree.scan()?;
        assert_eq!(tokens(&report), vec!["RwLock", "Mutex", "parking_lot"]);
        Ok(())
    }

    /// egui's `tex_manager()` hands an `Arc<RwLock<..>>` out as it stands — the calling side's source
    /// has no `RwLock` in it, so it can only be blocked by name.
    #[test]
    fn the_egui_texture_manager_accessor_is_a_lock() -> Result<()> {
        let tree = Tree::new("texman")?;
        tree.file(
            "crates/fairing/src/lib.rs",
            "fn f(ctx: &egui::Context) {\n    let _ = ctx.tex_manager();\n}\n",
        )?;
        let report = tree.scan()?;
        assert_eq!(tokens(&report), vec!["tex_manager"]);
        Ok(())
    }

    /// `.join()` is a thread wait but `.join("x")` is a path assembly — with an argument it is not caught.
    #[test]
    fn only_argument_less_join_is_blocking() -> Result<()> {
        let tree = Tree::new("join")?;
        tree.file(
            "crates/fairing/src/lib.rs",
            concat!(
                "fn paths(root: &std::path::Path) -> std::path::PathBuf {\n",
                "    root.join(\"a\").join(\"b\")\n",
                "}\n",
                "fn wait(h: std::thread::JoinHandle<()>) {\n",
                "    let _ = h.join();\n",
                "}\n",
            ),
        )?;
        let report = tree.scan()?;
        assert_eq!(
            tokens(&report),
            vec![".join()"],
            "a path assembly is not a violation: {:?}",
            report.violations
        );
        Ok(())
    }

    #[test]
    fn similar_identifiers_are_not_violations() -> Result<()> {
        let tree = Tree::new("boundary")?;
        tree.file(
            "crates/fairing/src/lib.rs",
            "struct MutexFree;\nfn mutexish() {}\nfn no_rwlocking() {}\n",
        )?;
        let report = tree.scan()?;
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        Ok(())
    }

    #[test]
    fn comments_and_strings_are_stripped() -> Result<()> {
        let tree = Tree::new("strip")?;
        tree.file(
            "crates/fairing/src/lib.rs",
            "//! Mutex is not used.\n\
             // RwLock likewise.\n\
             const WHY: &str = \"no Mutex\";\n\
             /* Condvar\n   Barrier */\n\
             const RAW: &str = r#\"LazyLock\"#;\n\
             fn f() { let _ = 'x'; }\n",
        )?;
        let report = tree.scan()?;
        assert_eq!(report.files, 1);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        Ok(())
    }

    #[test]
    fn the_ui_crate_may_not_block() -> Result<()> {
        let tree = Tree::new("ui")?;
        tree.file(
            "crates/fairing/src/shell.rs",
            "fn f() {\n\
             \x20   let _ = rx.recv();\n\
             \x20   let _ = rx.recv_timeout(d);\n\
             \x20   handle.join();\n\
             \x20   std::thread::sleep(d);\n\
             }\n",
        )?;
        let report = tree.scan()?;
        assert_eq!(
            tokens(&report),
            vec![".recv(", ".recv_timeout(", ".join()", "thread::sleep"]
        );
        Ok(())
    }

    /// The controls run on the UI thread as much as the shell does.
    #[test]
    fn the_widgets_crate_may_not_block() -> Result<()> {
        let tree = Tree::new("widgets")?;
        tree.file(
            "crates/fairing-widgets/src/lib.rs",
            "fn f(rx: Receiver<u8>) { let _ = rx.recv(); }\n",
        )?;
        let report = tree.scan()?;
        assert_eq!(tokens(&report), vec![".recv("]);
        Ok(())
    }

    /// A crate that is not UI code — a worker waiting on its own queue — is outside the rule.
    #[test]
    fn a_crate_outside_the_ui_may_block() -> Result<()> {
        let tree = Tree::new("worker")?;
        tree.file(
            "crates/worker/src/queue.rs",
            "fn worker(rx: Receiver<Cmd>) {\n\
             \x20   while let Ok(cmd) = rx.recv() {\n\
             \x20       std::thread::sleep(d);\n\
             \x20   }\n\
             }\n",
        )?;
        let report = tree.scan()?;
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        Ok(())
    }

    #[test]
    fn an_allow_mark_with_a_reason_skips_the_line() -> Result<()> {
        let tree = Tree::new("allow")?;
        tree.file(
            "crates/fairing/src/lib.rs",
            "fn f(m: Mutex<u8>) {} // sync-check: allow: the lock inside egui Context is excepted\n",
        )?;
        let report = tree.scan()?;
        assert_eq!(report.allows, 1);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        Ok(())
    }

    #[test]
    fn an_allow_mark_without_a_reason_is_itself_a_violation() -> Result<()> {
        let tree = Tree::new("allow-empty")?;
        tree.file(
            "crates/fairing/src/lib.rs",
            "fn f(m: Mutex<u8>) {} // sync-check: allow:\n",
        )?;
        let report = tree.scan()?;
        assert_eq!(report.allows, 0);
        assert_eq!(
            tokens(&report),
            vec!["// sync-check: allow:", "Mutex"],
            "with no reason it is not exempted"
        );
        Ok(())
    }

    #[test]
    fn only_src_of_a_crate_is_scanned() -> Result<()> {
        let tree = Tree::new("scope")?;
        tree.file("crates/fairing/tests/smoke.rs", "use std::sync::Mutex;\n")?;
        tree.file("crates/fairing/examples/demo.rs", "use std::sync::Mutex;\n")?;
        tree.file("crates/fairing/src/deep/inner.rs", "fn f() {}\n")?;
        let report = tree.scan()?;
        assert_eq!(report.files, 1);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        Ok(())
    }

    #[test]
    fn a_missing_crates_dir_is_an_empty_report() -> Result<()> {
        let tree = Tree::new("empty")?;
        assert_eq!(tree.scan()?, Report::default());
        Ok(())
    }

    #[test]
    fn the_scanner_keeps_lifetimes_and_carries_block_state() {
        let mut scanner = Scanner::new();
        assert!(scanner
            .strip("fn f<'a>(x: &'a Mutex) {}")
            .code
            .contains('\''));
        // `'\''` is 4 characters — counted as 3, one quote is left and swallows the code after it.
        let mut scanner = Scanner::new();
        let stripped = scanner.strip("let q = '\\''; let m = Mutex;").code;
        assert!(!stripped.contains('\''), "{stripped}");
        assert!(stripped.contains("Mutex"), "{stripped}");
        let mut scanner = Scanner::new();
        assert_eq!(scanner.strip("/* Mutex").code.trim(), "");
        assert_eq!(
            scanner.strip("RwLock */ let a = 1;").code.trim(),
            "let a = 1;"
        );
    }

    #[test]
    fn the_display_path_is_relative_to_the_root() -> Result<()> {
        let tree = Tree::new("path")?;
        tree.file("crates/fairing/src/lib.rs", "use std::sync::Mutex;\n")?;
        let report = tree.scan()?;
        let first = report
            .violations
            .first()
            .ok_or_else(|| Error::new("there is no violation"))?;
        assert!(
            first.starts_with(&format!(
                "crates{}fairing{}src{}lib.rs:1: Mutex — ",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR
            )),
            "{first}"
        );
        Ok(())
    }
}
