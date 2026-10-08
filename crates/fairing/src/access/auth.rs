//! Authentication — the shell asks, the integrator answers.
//!
//! # The crate checks no credential of its own
//!
//! How a credential is stored, how it is compared and what a run of wrong guesses costs are the
//! device's decisions, not the crate's. So the crate defines one [`Authenticator`] trait
//! and draws whatever it asks for: a keypad for [`AuthMethod::Pin`], a square of dots for
//! [`AuthMethod::Pattern`], a field and the on-screen keyboard for [`AuthMethod::Password`], a
//! waiting card for [`AuthMethod::External`]. What the person entered goes to
//! [`Authenticator::submit`] by value and the shell keeps no copy — not in a log, not in an event,
//! not in its own state.
//!
//! [`PinTable`] is the one implementation that ships: level name → PIN and level name → pattern
//! tables read from `[access.pin_table]` and `[access.pattern_table]`, with an attempt limit kept
//! in memory. It is a reference, deliberately small. Accounts, hashes, a lockout that survives a
//! restart, a card reader — all of that is an `Authenticator` of your own, and the shell draws it
//! the same way.
//!
//! # Time is the shell's
//!
//! [`Authenticator::submit`] and [`Authenticator::poll`] take `now`, the shell's monotonic time —
//! the value [`Backend::poll_at`](crate::services::Backend::poll_at) and
//! [`KnockInput::now`](super::KnockInput::now) receive. A lockout computed from `Instant::now()`
//! would run on a clock nothing can move: headless, the shell's time advances one frame at a time,
//! and a test that waits out a 60-second lockout should take 3,600 frames, not a minute.

use super::{Gate, Level, LevelTable, Subject};
use crate::config::{PatternTableConfig, PinTableConfig};
use crate::error::{Error, Result};
use crate::i18n::{tr_key, LabelKey};
use crate::services::Waker;
use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, Instant};

/// One way of proving who you are — what the prompt draws for it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthMethod {
    /// A keypad of digits.
    Pin {
        /// How many digits a PIN has: the prompt submits by itself when the last one goes in.
        /// `0` means the lengths differ — or are not to be given away — and the keypad's OK key
        /// submits.
        len: u8,
        /// The most digits the keypad takes while `len` is 0. `0` is the keypad's own
        /// [`MAX_PIN_LEN`].
        max_len: u8,
        /// Lay the digits out in a fresh order each time the prompt opens, so neither the smudges
        /// on the glass nor someone watching the finger learns the PIN from where it pressed.
        shuffle: bool,
    },
    /// A path drawn through a square of dots. What arrives is [`Credential::Pattern`].
    Pattern {
        /// Dots on a side, 3 to 5.
        grid: u8,
        /// The fewest dots a path submits with. A shorter one is answered on the spot ("Connect
        /// at least 4 dots") and costs no attempt — it never reaches [`Authenticator::submit`].
        min_points: u8,
        /// Draw the path as the finger draws it. Off leaves nothing on the glass to read over a
        /// shoulder — the pattern's counterpart of `shuffle`.
        show_path: bool,
    },
    /// A secret typed on the on-screen keyboard, after a user name when `needs_user`.
    Password {
        /// Ask for a user name as well.
        needs_user: bool,
    },
    /// Something off the screen — a card reader, a key, a fingerprint sensor. The prompt shows
    /// `label` and waits; the answer arrives through [`Authenticator::poll`].
    ///
    /// A reader that types like a keyboard (most badge readers do) needs nothing more: while this
    /// method is showing, the prompt collects what is typed and submits it on Enter as
    /// [`Credential::External`].
    External {
        /// What it is called and what to do — "Badge", "Hold your badge to the reader". It is the
        /// method's tab when there are several and the line the waiting card shows.
        label: LabelKey,
    },
}

impl AuthMethod {
    /// What the prompt cannot draw as asked, a line each. It draws the nearest it can, which is
    /// never what was meant — a 20-digit `len` submits on the keypad's 16th digit, so every try is
    /// wrong until the lockout — and nothing on the screen says why. The shell logs these when
    /// the prompt opens.
    pub(crate) fn unhonoured(&self) -> Vec<String> {
        let mut lines = Vec::new();
        match self {
            Self::Pin { len, max_len, .. } => {
                if usize::from(*len) > MAX_PIN_LEN {
                    lines.push(format!(
                        "Pin len = {len}: the keypad holds {MAX_PIN_LEN} digits and submits on \
                         the last of them"
                    ));
                }
                if usize::from(*max_len) > MAX_PIN_LEN {
                    lines.push(format!(
                        "Pin max_len = {max_len}: the keypad holds {MAX_PIN_LEN} digits"
                    ));
                }
            }
            Self::Pattern {
                grid, min_points, ..
            } => {
                let (low, high) = (crate::widgets::MIN_GRID, crate::widgets::MAX_GRID);
                let drawn = (*grid).clamp(low, high);
                if drawn != *grid {
                    lines.push(format!(
                        "Pattern grid = {grid}: the pad draws {low} to {high} dots a side, so \
                         {drawn} x {drawn}"
                    ));
                }
                let dots = usize::from(drawn) * usize::from(drawn);
                if usize::from(*min_points) > dots {
                    lines.push(format!(
                        "Pattern min_points = {min_points}: a {drawn} x {drawn} pad has {dots} \
                         dots, so a path through all of them submits"
                    ));
                }
            }
            Self::Password { .. } | Self::External { .. } => {}
        }
        lines
    }
}

/// What the person entered, on its way to [`Authenticator::submit`].
///
/// The shell hands it over **by value** and keeps nothing. `Debug` prints the shape and
/// never the contents — so a `{:?}` in an integrator's log line cannot leak a secret either.
#[non_exhaustive]
pub enum Credential {
    /// The digits from the keypad.
    Pin(String),
    /// The dots of a pattern in the order drawn, numbered row by row from **0** at the top left
    /// (the config file counts from 1, the way a person does; [`PinTable`] converts).
    Pattern(Vec<u8>),
    /// The fields of the password card.
    Password {
        /// The user name, where [`AuthMethod::Password::needs_user`] asked for one.
        user: Option<String>,
        /// The secret.
        secret: String,
    },
    /// What an external method produced — the bytes a keyboard-wedge reader typed, or whatever
    /// an integrator hands over.
    External(Vec<u8>),
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Not even the user name: the field takes whatever is typed into it, and a password typed
        // into the wrong field is still a password.
        match self {
            Self::Pin(_) => f.write_str("Pin(<redacted>)"),
            Self::Pattern(_) => f.write_str("Pattern(<redacted>)"),
            Self::Password { user, .. } => f
                .debug_struct("Password")
                .field("user", &user.as_ref().map(|_| "<redacted>"))
                .field("secret", &"<redacted>")
                .finish(),
            Self::External(bytes) => write!(f, "External(<{} bytes>)", bytes.len()),
        }
    }
}

/// An authenticator's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthOutcome {
    /// Who this is. The shell applies it per `[access] unlock_mode`.
    Granted(Subject),
    /// No. The prompt shows `message` **as it is** — whether to say that a user does not exist is
    /// the authenticator's call, so the shell never words a refusal itself.
    Denied {
        /// The line to show.
        message: LabelKey,
    },
    /// No more tries until `until` (the shell's time). The keypad greys out and counts down.
    Locked {
        /// When it accepts again.
        until: Instant,
        /// The line to show.
        message: LabelKey,
    },
    /// Not decided yet — a card being read, a server being asked. The shell keeps calling
    /// [`Authenticator::poll`] while the prompt is up and says "Checking…", but the keys still
    /// work: a new attempt calls [`Authenticator::cancel`] and goes in fresh.
    ///
    /// The shell sets no time limit on the answer. One the device wants — a server that ought to
    /// answer within ten seconds — is the authenticator's: answer `Denied` from `poll` when it
    /// runs out.
    Pending,
}

/// The integrator's authentication. The shell draws [`Authenticator::methods`], hands
/// what was entered to [`Authenticator::submit`], and applies the [`AuthOutcome`].
///
/// ```
/// use fairing::access::{AuthMethod, AuthOutcome, Authenticator, Credential, Subject};
/// use fairing::Level;
/// use std::time::Instant;
///
/// /// One service PIN, checked however the device likes — a hash, a TPM, a file.
/// struct ServicePin;
///
/// impl Authenticator for ServicePin {
///     fn methods(&self) -> Vec<AuthMethod> {
///         vec![AuthMethod::Pin { len: 6, max_len: 0, shuffle: true }]
///     }
///
///     fn submit(&mut self, credential: Credential, _now: Instant) -> AuthOutcome {
///         match credential {
///             Credential::Pin(pin) if my_check(&pin) => AuthOutcome::Granted(Subject {
///                 level: Level(1),
///                 ..Subject::default()
///             }),
///             _ => AuthOutcome::Denied { message: "That is not the service PIN".into() },
///         }
///     }
/// }
/// # fn my_check(pin: &str) -> bool { pin == "246810" }
/// ```
pub trait Authenticator {
    /// The ways in, in the order the prompt offers them. Read each time the prompt opens.
    ///
    /// An empty list leaves the shell nothing to draw: the request then goes out as
    /// [`AccessEvent::UnlockRequested`](super::AccessEvent::UnlockRequested) only, as in
    /// `routing` mode.
    fn methods(&self) -> Vec<AuthMethod>;

    /// Check what was entered. **Return at once** — it runs on the UI thread, inside a frame. A
    /// check that takes time answers [`AuthOutcome::Pending`] and finishes through
    /// [`Authenticator::poll`]. Where one is still under way, [`Authenticator::cancel`] comes
    /// first: a new attempt replaces it.
    fn submit(&mut self, credential: Credential, now: Instant) -> AuthOutcome;

    /// Finish what [`AuthOutcome::Pending`] started, or report an external method's result. The
    /// shell calls it once a frame while the prompt is up; `None` means nothing new.
    ///
    /// Whatever it reports, the shell applies as it comes — it follows the authenticator's
    /// results and does not second-guess them. So a check [`Authenticator::cancel`] called off must
    /// not be reported: the request is the authenticator's, and so is dropping its answer.
    ///
    /// A frame only comes when something asks for one, so a reader thread that has an answer
    /// should call the [`Waker`] from [`Authenticator::attach`].
    fn poll(&mut self, _now: Instant) -> Option<AuthOutcome> {
        None
    }

    /// The check under way is off: the prompt closed without an answer, the lock screen is coming
    /// up over it, or a new attempt is going in. Stop waiting for a card, drop a half-entered
    /// state — whatever [`Authenticator::poll`] reports after this answers what comes next, never
    /// the check that was stopped.
    ///
    /// It is called on the UI thread like everything here, so there is nothing to race: once it
    /// returns, that check is over. A wait ends with an answer or with `cancel`, never otherwise.
    fn cancel(&mut self) {}

    /// The shell refused a grant this authenticator gave: on the unlock prompt a level no higher
    /// than the session already holds, or a level the table does not have. To whoever is at the
    /// prompt it was not a way in, and an authenticator with an attempt limit counts it as a
    /// failure — otherwise a credential someone knows starts the count again between guesses at
    /// one they do not.
    ///
    /// `None` (the default) leaves the shell's own refusal, "That is not enough for this". A
    /// `Denied` or a `Locked` answered here is applied in its place; anything else is ignored.
    fn refused(&mut self, _now: Instant) -> Option<AuthOutcome> {
        None
    }

    /// The prompt opened for `gate` (the lock screen's is `session.lock`) — start listening now.
    /// Anything a reader picked up while no prompt was up belongs to nobody and should be dropped
    /// here: a badge swiped at a panel that asked for nothing must not unlock the next person's
    /// prompt hours later.
    fn begin(&mut self, _gate: &Gate, _now: Instant) {}

    /// The handle that wakes the UI thread, given once when the shell takes the authenticator —
    /// the same one every backend receives through
    /// [`Backend::attach`](crate::services::Backend::attach).
    fn attach(&mut self, _waker: Waker) {}

    /// The management side, where the authenticator offers one. With it the shell registers the
    /// `settings.credentials` screen behind the gate of the same name.
    fn admin(&mut self) -> Option<&mut dyn CredentialAdmin> {
        None
    }
}

/// What a management call answers: `Err` carries the line the screen shows.
pub type AdminResult = std::result::Result<(), String>;

/// The operations `settings.credentials` draws — and only these.
///
/// Each one is the authenticator's to carry out: where the entries live and how a secret is
/// stored do not reach the shell.
///
/// **The shell keeps a change within the level of whoever makes it.** Nobody gives a level above
/// their own, and an entry above their level is not theirs to change or remove — so a maintainer
/// cannot add an administrator, or set the administrator's PIN and then use it. Those calls are
/// refused before they get here. Who is making the rest is [`set_actor`](Self::set_actor).
pub trait CredentialAdmin {
    /// Who the calls after this one are made by: the session's subject, given before each change
    /// the screen applies. The default ignores it. An authenticator with rules of its own about who
    /// may change what checks them against it, and one that keeps an audit trail writes it down.
    fn set_actor(&mut self, _subject: &Subject) {}

    /// The entries, in the order the screen lists them.
    fn list(&self) -> Vec<CredentialEntry>;
    /// Replace an entry's secret.
    ///
    /// # Errors
    /// The line the screen shows — a secret that breaks the device's rules, say.
    fn set_secret(&mut self, id: &str, credential: Credential) -> AdminResult;
    /// Move an entry to another level.
    ///
    /// # Errors
    /// The line the screen shows.
    fn set_level(&mut self, id: &str, level: Level) -> AdminResult;
    /// Add an entry.
    ///
    /// # Errors
    /// The line the screen shows — an id that is taken, say.
    fn add(&mut self, id: &str, level: Level, credential: Credential) -> AdminResult;
    /// Remove an entry.
    ///
    /// # Errors
    /// The line the screen shows — refusing to remove the last way in, say.
    fn remove(&mut self, id: &str) -> AdminResult;
}

/// One row of `settings.credentials`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialEntry {
    /// The id the management calls take.
    pub id: String,
    /// What the row says.
    pub label: LabelKey,
    /// The level it grants.
    pub level: Level,
    /// Whether it is switched off (drawn dimmed).
    pub disabled: bool,
}

/// The longest PIN the table takes — the keypad's dot row has room for this many.
pub const MAX_PIN_LEN: usize = 16;

/// What a wrong PIN says.
const WRONG_PIN: &str = tr_key!("Wrong PIN");

/// What a wrong pattern says.
const WRONG_PATTERN: &str = tr_key!("Wrong pattern");

/// What the table says once the attempt limit is used up.
const TOO_MANY: &str = tr_key!("Too many attempts");

/// How long the table locks for when `attempt_limit` is set without `lock_secs`.
const DEFAULT_LOCK: Duration = Duration::from_secs(60);

/// **The reference authenticator**: level name → PIN and level name → pattern tables from
/// `[access.pin_table]` and `[access.pattern_table]`.
///
/// ```toml
/// [access.pin_table]
/// operator = "1234"
/// maintainer = "987654"
/// attempt_limit = 5   # optional: this many wrong in a row locks the prompt...
/// lock_secs = 60      # ...for this long (60 when left out)
/// shuffle = true      # optional: a fresh digit layout each time the prompt opens
/// max_len = 8         # optional: the most digits a PIN may have (16 when left out)
///
/// [access.pattern_table]
/// maintainer = "1-2-3-5-7-8-9"   # the dots in the order drawn, row by row from 1
/// grid = 3                       # optional: dots on a side, 3 to 5
/// min_points = 4                 # optional: the fewest dots a pattern has
/// ```
///
/// - A right PIN or pattern grants `Subject { id: None, level }` for its level. A level may have
///   either or both, and the prompt offers each kind the tables hold, PIN first.
/// - When every PIN has the same length the keypad submits by itself on the last digit;
///   otherwise its OK key submits, and it takes no more than `max_len` digits.
/// - A pattern must be one a finger can draw: a stroke from one dot to another across a third
///   takes the third, so `"1-3"` cannot be drawn — the finger records `"1-2-3"`, and the table
///   says so at start-up.
/// - Wrong PINs and wrong patterns count against **one** attempt limit — two ways in are not two
///   allowances. The count lives **in memory** and a restart clears it. The only input is the
///   touchscreen and there is no remote API, so that is as far as a reference goes.
/// - The secrets sit in the config file in plain text, and keeping that file unreadable is the
///   integrator's job — the shell says so once at start-up.
/// - There is no [`Authenticator::admin`]: the file is the source of truth, so the screen does
///   not edit it.
///
/// The fixed keys of each table are its own (see [`PinTableConfig`] and [`PatternTableConfig`]),
/// so no level can be named after one of them.
#[derive(Clone)]
pub struct PinTable {
    /// Level → PIN.
    pins: Vec<(Level, String)>,
    /// The common length, or 0.
    len: u8,
    /// The most digits a PIN may have.
    max_len: u8,
    shuffle: bool,
    /// Level → pattern, dots from 0.
    patterns: Vec<(Level, Vec<u8>)>,
    grid: u8,
    min_points: u8,
    show_path: bool,
    attempt_limit: Option<u32>,
    /// The lockout, where a table set one.
    lock: Option<Duration>,
    /// Wrong PINs in a row since the last right one the shell took, or the last lockout.
    failures: u32,
    /// The count the last right PIN cleared, until the shell takes the grant: a grant it refuses
    /// puts the count back, so a PIN someone knows does not start it again.
    cleared: u32,
    locked_until: Option<Instant>,
}

impl fmt::Debug for PinTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let levels: Vec<Level> = self.pins.iter().map(|(level, _)| *level).collect();
        let pattern_levels: Vec<Level> = self.patterns.iter().map(|(level, _)| *level).collect();
        f.debug_struct("PinTable")
            .field("levels", &levels)
            .field("len", &self.len)
            .field("max_len", &self.max_len)
            .field("shuffle", &self.shuffle)
            .field("pattern_levels", &pattern_levels)
            .field("grid", &self.grid)
            .field("attempt_limit", &self.attempt_limit)
            .field("failures", &self.failures)
            .field("locked_until", &self.locked_until)
            .finish_non_exhaustive()
    }
}

impl PinTable {
    /// Build one from `[access.pin_table]` against the level table — PINs only. See
    /// [`PinTable::from_tables`].
    ///
    /// # Errors
    /// As [`PinTable::from_tables`].
    pub fn from_config(cfg: &PinTableConfig, levels: &LevelTable) -> Result<Self> {
        Self::from_tables(cfg, &PatternTableConfig::default(), levels)
    }

    /// Build one from `[access.pin_table]` and `[access.pattern_table]` against the level table.
    ///
    /// # Errors
    /// [`Error::Config`] for a key that is not a level; a PIN that is not 1 to `max_len` digits,
    /// or a `max_len` outside 1–16; a pattern that is not dot numbers on the grid, takes a dot
    /// twice, is shorter than `min_points` or cannot be drawn as written (the message gives the
    /// path a finger would record); a `grid` outside 3–5 or a `min_points` outside the grid; two
    /// levels sharing one PIN or one pattern (the table could not tell them apart); an
    /// `attempt_limit` or `lock_secs` of 0.
    pub fn from_tables(
        pins_cfg: &PinTableConfig,
        patterns_cfg: &PatternTableConfig,
        levels: &LevelTable,
    ) -> Result<Self> {
        let max_len = match pins_cfg.max_len {
            None => MAX_PIN_LEN,
            Some(n) if (1..=MAX_PIN_LEN).contains(&usize::from(n)) => usize::from(n),
            Some(n) => {
                return Err(Error::Config(format!(
                    "[access.pin_table] max_len = {n} must be 1 to {MAX_PIN_LEN} - the keypad's dot row has room for {MAX_PIN_LEN}"
                )))
            }
        };
        let mut pins: Vec<(Level, String)> = Vec::with_capacity(pins_cfg.pins.len());
        for (name, pin) in &pins_cfg.pins {
            let level = level_of(levels, "pin_table", name)?;
            if pin.is_empty() || pin.len() > max_len || !pin.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::Config(format!(
                    "[access.pin_table] \"{name}\" must be 1 to {max_len} digits - the keypad has nothing else"
                )));
            }
            if let Some((other, _)) = pins.iter().find(|(_, p)| p == pin) {
                let other = levels.get(*other).map_or("?", |d| d.name.as_str());
                return Err(Error::Config(format!(
                    "[access.pin_table] \"{other}\" and \"{name}\" have the same PIN - the shell could not tell them apart"
                )));
            }
            pins.push((level, pin.clone()));
        }
        let patterns = patterns_from(patterns_cfg, levels)?;
        for (section, limit, lock) in [
            ("pin_table", pins_cfg.attempt_limit, pins_cfg.lock_secs),
            (
                "pattern_table",
                patterns_cfg.attempt_limit,
                patterns_cfg.lock_secs,
            ),
        ] {
            if limit == Some(0) {
                return Err(Error::Config(format!(
                    "[access.{section}] attempt_limit = 0 would lock the prompt before the first try"
                )));
            }
            if lock == Some(0) {
                return Err(Error::Config(format!(
                    "[access.{section}] lock_secs must be more than 0 - leave attempt_limit out for no lockout"
                )));
            }
            if let Some(secs) = lock.filter(|&secs| secs > super::MAX_TIMER_SECS) {
                return Err(Error::Config(format!(
                    "[access.{section}] lock_secs = {secs} is longer than a year"
                )));
            }
        }
        // One count for both ways in: the stricter limit and the longer lockout.
        let attempt_limit = match (pins_cfg.attempt_limit, patterns_cfg.attempt_limit) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let lock = pins_cfg
            .lock_secs
            .into_iter()
            .chain(patterns_cfg.lock_secs)
            .max()
            .map(Duration::from_secs);
        if lock.is_some() && attempt_limit.is_none() {
            log::warn!("[access] lock_secs does nothing without attempt_limit");
        }
        let len = match pins.first() {
            Some((_, first)) if pins.iter().all(|(_, p)| p.len() == first.len()) => {
                u8::try_from(first.len()).unwrap_or(0)
            }
            _ => 0,
        };
        if !pins.is_empty() || !patterns.is_empty() {
            log::warn!(
                "[access.pin_table] and [access.pattern_table] hold secrets in clear text - file permissions are the integrator's responsibility"
            );
        }
        Ok(Self {
            pins,
            len,
            max_len: u8::try_from(max_len).unwrap_or(0),
            shuffle: pins_cfg.shuffle,
            patterns,
            grid: patterns_cfg.grid,
            min_points: patterns_cfg.min_points,
            show_path: patterns_cfg.show_path,
            attempt_limit,
            lock,
            failures: 0,
            cleared: 0,
            locked_until: None,
        })
    }

    /// Whether the table has no PIN and no pattern at all (and so can grant nothing).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pins.is_empty() && self.patterns.is_empty()
    }

    /// A wrong attempt: count it, and lock once the limit is reached. `wrong` is what a refusal
    /// says.
    fn fail(&mut self, now: Instant, wrong: &str) -> AuthOutcome {
        self.failures += 1;
        match self.attempt_limit {
            Some(limit) if self.failures >= limit => {
                let lock = self.lock.unwrap_or(DEFAULT_LOCK);
                let until = now.checked_add(lock).unwrap_or(now);
                self.locked_until = Some(until);
                // A fresh allowance once the lock runs out, not one more try.
                self.failures = 0;
                AuthOutcome::Locked {
                    until,
                    message: TOO_MANY.to_owned(),
                }
            }
            _ => AuthOutcome::Denied {
                message: wrong.to_owned(),
            },
        }
    }
}

impl Authenticator for PinTable {
    fn methods(&self) -> Vec<AuthMethod> {
        let mut methods = Vec::new();
        if !self.pins.is_empty() {
            methods.push(AuthMethod::Pin {
                len: self.len,
                max_len: self.max_len,
                shuffle: self.shuffle,
            });
        }
        if !self.patterns.is_empty() {
            methods.push(AuthMethod::Pattern {
                grid: self.grid,
                min_points: self.min_points,
                show_path: self.show_path,
            });
        }
        methods
    }

    /// A refused grant is a failure like a wrong PIN, on top of the count it cleared; only a
    /// lockout replaces the shell's words.
    fn refused(&mut self, now: Instant) -> Option<AuthOutcome> {
        self.failures = self
            .failures
            .saturating_add(std::mem::take(&mut self.cleared));
        match self.fail(now, "") {
            locked @ AuthOutcome::Locked { .. } => Some(locked),
            _ => None,
        }
    }

    fn submit(&mut self, credential: Credential, now: Instant) -> AuthOutcome {
        self.cleared = 0;
        if let Some(until) = self.locked_until {
            if now < until {
                return AuthOutcome::Locked {
                    until,
                    message: TOO_MANY.to_owned(),
                };
            }
            self.locked_until = None;
        }
        // Every entry is compared, so the time taken does not depend on which level matched. The
        // table offers a keypad and a pattern only; anything else can only be an integrator
        // calling `submit` directly, and it is still an attempt.
        let (granted, wrong) = match &credential {
            Credential::Pin(pin) => (
                self.pins.iter().fold(None, |hit, (level, expected)| {
                    if same(expected.as_bytes(), pin.as_bytes()) {
                        Some(*level)
                    } else {
                        hit
                    }
                }),
                WRONG_PIN,
            ),
            Credential::Pattern(dots) => (
                self.patterns.iter().fold(None, |hit, (level, expected)| {
                    if same(expected, dots) {
                        Some(*level)
                    } else {
                        hit
                    }
                }),
                WRONG_PATTERN,
            ),
            _ => (None, WRONG_PIN),
        };
        match granted {
            Some(level) => {
                self.cleared = std::mem::take(&mut self.failures);
                AuthOutcome::Granted(Subject {
                    id: None,
                    level,
                    attrs: BTreeMap::new(),
                })
            }
            None => self.fail(now, wrong),
        }
    }
}

/// The level `name` stands for in `[access.<section>]`.
fn level_of(levels: &LevelTable, section: &str, name: &str) -> Result<Level> {
    levels.index_of(name).ok_or_else(|| {
        Error::Config(format!(
            "[access.{section}] \"{name}\" is not a level in levels"
        ))
    })
}

/// `[access.pattern_table]`'s patterns, checked against the grid and each other.
fn patterns_from(cfg: &PatternTableConfig, levels: &LevelTable) -> Result<Vec<(Level, Vec<u8>)>> {
    if cfg.patterns.is_empty() {
        return Ok(Vec::new());
    }
    let (min, max) = (crate::widgets::MIN_GRID, crate::widgets::MAX_GRID);
    if !(min..=max).contains(&cfg.grid) {
        return Err(Error::Config(format!(
            "[access.pattern_table] grid = {} must be {min} to {max}",
            cfg.grid
        )));
    }
    let dots = cfg.grid * cfg.grid;
    if !(1..=dots).contains(&cfg.min_points) {
        return Err(Error::Config(format!(
            "[access.pattern_table] min_points = {} must be 1 to {dots} on a {g} × {g} grid",
            cfg.min_points,
            g = cfg.grid
        )));
    }
    let mut patterns: Vec<(Level, Vec<u8>)> = Vec::with_capacity(cfg.patterns.len());
    for (name, text) in &cfg.patterns {
        let level = level_of(levels, "pattern_table", name)?;
        let path = parse_pattern(text, cfg.grid, cfg.min_points).map_err(|why| {
            Error::Config(format!(
                "[access.pattern_table] \"{name}\" = \"{text}\": {why}"
            ))
        })?;
        if let Some((other, _)) = patterns.iter().find(|(_, p)| *p == path) {
            let other = levels.get(*other).map_or("?", |d| d.name.as_str());
            return Err(Error::Config(format!(
                "[access.pattern_table] \"{other}\" and \"{name}\" have the same pattern - the shell could not tell them apart"
            )));
        }
        patterns.push((level, path));
    }
    Ok(patterns)
}

/// A pattern as the config file writes it — dots from 1, row by row, between `-`, `,` or spaces,
/// or bare digits on the 3 × 3 grid — to dots from 0, checked as a finger would have to draw it.
/// `Err` is why not, in words for the config error.
pub(crate) fn parse_pattern(
    text: &str,
    grid: u8,
    min_points: u8,
) -> std::result::Result<Vec<u8>, String> {
    let dots = grid.saturating_mul(grid);
    let separated = text.contains(['-', ',', ' ']);
    let tokens: Vec<String> = if separated {
        text.split(['-', ',', ' '])
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect()
    } else if grid == 3 {
        text.chars().map(String::from).collect()
    } else {
        return Err(format!(
            "separate the dots with '-' on a {grid} × {grid} grid (\"1-2-3\")"
        ));
    };
    let mut path = Vec::with_capacity(tokens.len());
    for token in &tokens {
        let dot = token
            .parse::<u8>()
            .ok()
            .filter(|d| (1..=dots).contains(d))
            .ok_or_else(|| {
                format!("\"{token}\" is not a dot - number them 1 to {dots}, row by row")
            })?;
        if path.contains(&(dot - 1)) {
            return Err(format!(
                "dot {dot} comes twice - a path takes each dot once"
            ));
        }
        path.push(dot - 1);
    }
    if path.len() < usize::from(min_points.max(1)) {
        return Err(format!("{} dots - min_points is {min_points}", path.len()));
    }
    let drawn = crate::widgets::PatternPad::as_drawn(&path, grid);
    if drawn != path {
        return Err(format!(
            "a finger cannot draw it as written - a stroke across a dot takes that dot, so it records \"{}\"",
            pattern_text(&drawn)
        ));
    }
    Ok(path)
}

/// Dots from 0 as the config file writes them: from 1, joined by `-`.
pub(crate) fn pattern_text(dots: &[u8]) -> String {
    let mut text = String::new();
    for (i, dot) in dots.iter().enumerate() {
        if i > 0 {
            text.push('-');
        }
        text.push_str(&(u16::from(*dot) + 1).to_string());
    }
    text
}

/// Byte equality with no early exit, so the time taken does not say how many leading digits (or
/// dots) were right. The threat model has no remote timing channel; not having a local
/// one costs nothing. The length is not hidden — the keypad shows it as dots anyway.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::{
        parse_pattern, pattern_text, same, AuthMethod, AuthOutcome, Authenticator, Credential,
        PinTable,
    };
    use crate::access::{Level, LevelTable};
    use crate::config::{PatternTableConfig, PinTableConfig};
    use std::time::{Duration, Instant};

    fn levels() -> LevelTable {
        LevelTable::from_names(&[
            "viewer".to_owned(),
            "operator".to_owned(),
            "maintainer".to_owned(),
        ])
    }

    fn table(pins: &[(&str, &str)]) -> PinTableConfig {
        PinTableConfig {
            pins: pins
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..PinTableConfig::default()
        }
    }

    fn patterns(entries: &[(&str, &str)]) -> PatternTableConfig {
        PatternTableConfig {
            patterns: entries
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..PatternTableConfig::default()
        }
    }

    fn granted(outcome: &AuthOutcome) -> Option<Level> {
        match outcome {
            AuthOutcome::Granted(subject) => Some(subject.level),
            _ => None,
        }
    }

    /// What the pads cannot draw as asked is named, one line each — and a method they can
    /// draw says nothing.
    #[test]
    fn what_the_pads_cannot_draw_is_named() {
        let pin = |len, max_len| AuthMethod::Pin {
            len,
            max_len,
            shuffle: false,
        };
        let pattern = |grid, min_points| AuthMethod::Pattern {
            grid,
            min_points,
            show_path: true,
        };
        for fits in [
            pin(4, 0),
            pin(0, 16),
            pin(16, 0),
            pattern(3, 4),
            pattern(5, 25),
            AuthMethod::Password { needs_user: true },
        ] {
            assert_eq!(fits.unhonoured(), Vec::<String>::new(), "{fits:?}");
        }
        assert_eq!(pin(20, 0).unhonoured().len(), 1, "len over the keypad");
        assert_eq!(pin(0, 30).unhonoured().len(), 1, "max_len over the keypad");
        assert_eq!(pattern(6, 4).unhonoured().len(), 1, "grid over five");
        assert_eq!(pattern(2, 4).unhonoured().len(), 1, "grid under three");
        assert_eq!(pattern(3, 10).unhonoured().len(), 1, "more dots than nine");
        assert_eq!(pattern(6, 30).unhonoured().len(), 2, "both");
    }

    #[test]
    fn a_right_pin_grants_its_level_and_a_wrong_one_is_denied() -> crate::Result<()> {
        let mut pins = PinTable::from_config(
            &table(&[("operator", "1234"), ("maintainer", "987654")]),
            &levels(),
        )?;
        let now = Instant::now();
        assert_eq!(
            granted(&pins.submit(Credential::Pin("987654".to_owned()), now)),
            Some(Level(2))
        );
        assert_eq!(
            granted(&pins.submit(Credential::Pin("1234".to_owned()), now)),
            Some(Level(1))
        );
        assert!(matches!(
            pins.submit(Credential::Pin("0000".to_owned()), now),
            AuthOutcome::Denied { .. }
        ));
        Ok(())
    }

    #[test]
    fn mixed_lengths_need_an_ok_key_and_equal_ones_submit_themselves() -> crate::Result<()> {
        let mixed = PinTable::from_config(
            &table(&[("operator", "1234"), ("maintainer", "987654")]),
            &levels(),
        )?;
        assert_eq!(
            mixed.methods(),
            vec![AuthMethod::Pin {
                len: 0,
                max_len: 16,
                shuffle: false
            }]
        );
        let mut cfg = table(&[("operator", "1234"), ("maintainer", "9876")]);
        cfg.shuffle = true;
        cfg.max_len = Some(6);
        let equal = PinTable::from_config(&cfg, &levels())?;
        assert_eq!(
            equal.methods(),
            vec![AuthMethod::Pin {
                len: 4,
                max_len: 6,
                shuffle: true
            }]
        );
        Ok(())
    }

    #[test]
    fn max_len_caps_the_pins_the_table_takes() {
        let levels = levels();
        let mut cfg = table(&[("operator", "1234567")]);
        cfg.max_len = Some(6);
        assert!(
            PinTable::from_config(&cfg, &levels).is_err(),
            "seven digits over a cap of six"
        );
        cfg.max_len = Some(7);
        assert!(PinTable::from_config(&cfg, &levels).is_ok());
        for out_of_range in [0, 17] {
            cfg.max_len = Some(out_of_range);
            assert!(
                PinTable::from_config(&cfg, &levels).is_err(),
                "{out_of_range}"
            );
        }
    }

    #[test]
    fn a_pattern_is_written_from_1_and_kept_from_0() {
        assert_eq!(parse_pattern("1-2-3-6-9", 3, 4), Ok(vec![0, 1, 2, 5, 8]));
        assert_eq!(parse_pattern("12369", 3, 4), Ok(vec![0, 1, 2, 5, 8]));
        assert_eq!(parse_pattern("1, 2,3 6 9", 3, 4), Ok(vec![0, 1, 2, 5, 8]));
        assert_eq!(parse_pattern("1-6-11-16", 4, 4), Ok(vec![0, 5, 10, 15]));
        assert_eq!(pattern_text(&[0, 1, 2, 5, 8]), "1-2-3-6-9");
        assert!(parse_pattern("1234", 4, 4).is_err(), "bare digits on 4 × 4");
        assert!(parse_pattern("1-2-3-10", 3, 4).is_err(), "off the grid");
        assert!(parse_pattern("1-2-a-4", 3, 4).is_err(), "not a number");
        assert!(parse_pattern("1-2-1-4", 3, 4).is_err(), "a dot twice");
        assert!(parse_pattern("1-2-3", 3, 4).is_err(), "under min_points");
        // 1 → 3 crosses 2: the finger records 1-2-3, and the error says so.
        let why = parse_pattern("1-3-6-9", 3, 4).err().unwrap_or_default();
        assert!(why.contains("\"1-2-3-6-9\""), "{why}");
        // Across a dot already taken is fine.
        assert_eq!(parse_pattern("2-1-3-6", 3, 4), Ok(vec![1, 0, 2, 5]));
    }

    #[test]
    fn a_right_pattern_grants_its_level_next_to_the_pins() -> crate::Result<()> {
        let mut table = PinTable::from_tables(
            &table(&[("operator", "1234")]),
            &patterns(&[("maintainer", "1-2-3-5-7-8-9")]),
            &levels(),
        )?;
        assert_eq!(
            table.methods(),
            vec![
                AuthMethod::Pin {
                    len: 4,
                    max_len: 16,
                    shuffle: false
                },
                AuthMethod::Pattern {
                    grid: 3,
                    min_points: 4,
                    show_path: true
                }
            ]
        );
        let now = Instant::now();
        assert_eq!(
            granted(&table.submit(Credential::Pattern(vec![0, 1, 2, 4, 6, 7, 8]), now)),
            Some(Level(2))
        );
        assert!(matches!(
            table.submit(Credential::Pattern(vec![0, 1, 2, 4]), now),
            AuthOutcome::Denied { message } if message == "Wrong pattern"
        ));
        // A pattern table alone is an authenticator too.
        let only = PinTable::from_tables(
            &PinTableConfig::default(),
            &patterns(&[("operator", "7415963")]),
            &levels(),
        )?;
        assert!(!only.is_empty());
        assert!(matches!(
            only.methods().as_slice(),
            [AuthMethod::Pattern { .. }]
        ));
        Ok(())
    }

    #[test]
    fn the_pattern_table_refuses_what_it_could_never_check() {
        let levels = levels();
        let bad = |cfg: &PatternTableConfig| {
            PinTable::from_tables(&PinTableConfig::default(), cfg, &levels).is_err()
        };
        assert!(bad(&patterns(&[("nobody", "1-2-3-6")])), "not a level");
        assert!(
            bad(&patterns(&[
                ("operator", "1-2-3-6"),
                ("maintainer", "1236")
            ])),
            "two levels, one pattern"
        );
        let mut grid = patterns(&[("operator", "1-2-3-6")]);
        grid.grid = 6;
        assert!(bad(&grid), "a grid of six");
        let mut min = patterns(&[("operator", "1-2-3-6")]);
        min.min_points = 10;
        assert!(bad(&min), "more points than dots");
        let mut zero = patterns(&[("operator", "1-2-3-6")]);
        zero.attempt_limit = Some(0);
        assert!(bad(&zero));
    }

    #[test]
    fn a_wrong_pin_and_a_wrong_pattern_share_one_allowance() -> crate::Result<()> {
        let mut pins = table(&[("operator", "1234")]);
        pins.attempt_limit = Some(3);
        let mut dots = patterns(&[("maintainer", "1-2-3-6")]);
        dots.attempt_limit = Some(2);
        dots.lock_secs = Some(90);
        let mut table = PinTable::from_tables(&pins, &dots, &levels())?;
        let now = Instant::now();
        assert!(matches!(
            table.submit(Credential::Pin("0000".to_owned()), now),
            AuthOutcome::Denied { .. }
        ));
        // The stricter limit (two) and the longer lockout (90 s) hold.
        assert!(matches!(
            table.submit(Credential::Pattern(vec![8, 7, 6, 3]), now),
            AuthOutcome::Locked { until, .. } if until == now + Duration::from_secs(90)
        ));
        Ok(())
    }

    #[test]
    fn the_attempt_limit_locks_and_then_gives_a_fresh_allowance() -> crate::Result<()> {
        let mut cfg = table(&[("operator", "1234")]);
        cfg.attempt_limit = Some(2);
        cfg.lock_secs = Some(30);
        let mut pins = PinTable::from_config(&cfg, &levels())?;
        let start = Instant::now();
        let wrong = || Credential::Pin("9999".to_owned());
        assert!(matches!(
            pins.submit(wrong(), start),
            AuthOutcome::Denied { .. }
        ));
        let until = match pins.submit(wrong(), start) {
            AuthOutcome::Locked { until, .. } => until,
            other => return Err(crate::Error::Config(format!("expected a lock: {other:?}"))),
        };
        assert_eq!(until, start + Duration::from_secs(30));
        // Even the right PIN is refused while locked.
        assert!(matches!(
            pins.submit(
                Credential::Pin("1234".to_owned()),
                start + Duration::from_secs(29)
            ),
            AuthOutcome::Locked { .. }
        ));
        // Afterwards there are two fresh tries, not one.
        let later = start + Duration::from_secs(31);
        assert!(matches!(
            pins.submit(wrong(), later),
            AuthOutcome::Denied { .. }
        ));
        assert_eq!(
            granted(&pins.submit(Credential::Pin("1234".to_owned()), later)),
            Some(Level(1))
        );
        Ok(())
    }

    #[test]
    fn the_table_refuses_what_it_could_never_check() {
        let levels = levels();
        let bad = |pins: &[(&str, &str)]| PinTable::from_config(&table(pins), &levels).is_err();
        assert!(bad(&[("operator", "12a4")]), "a letter on a digit keypad");
        assert!(bad(&[("operator", "")]), "an empty PIN");
        assert!(bad(&[("operator", "12345678901234567")]), "17 digits");
        assert!(bad(&[("nobody", "1234")]), "not a level");
        assert!(
            bad(&[("operator", "1234"), ("maintainer", "1234")]),
            "two levels, one PIN"
        );
        let mut zero = table(&[("operator", "1234")]);
        zero.attempt_limit = Some(0);
        assert!(PinTable::from_config(&zero, &levels).is_err());
        let mut no_lock = table(&[("operator", "1234")]);
        no_lock.lock_secs = Some(0);
        assert!(PinTable::from_config(&no_lock, &levels).is_err());
    }

    #[test]
    fn neither_debug_prints_a_secret() -> crate::Result<()> {
        let pins = PinTable::from_config(&table(&[("operator", "1234")]), &levels())?;
        assert!(!format!("{pins:?}").contains("1234"));
        let pin = Credential::Pin("1234".to_owned());
        assert!(!format!("{pin:?}").contains("1234"));
        let pattern = Credential::Pattern(vec![4, 7, 1]);
        assert!(!format!("{pattern:?}").contains('4'));
        let password = Credential::Password {
            user: Some("ada".to_owned()),
            secret: "hunter2".to_owned(),
        };
        let printed = format!("{password:?}");
        assert!(!printed.contains("hunter2") && !printed.contains("ada"));
        Ok(())
    }

    #[test]
    fn equality_does_not_stop_at_the_first_difference() {
        assert!(same(b"1234", b"1234"));
        assert!(!same(b"1234", b"1235"));
        assert!(!same(b"1234", b"12345"));
        assert!(!same(b"", b"0"));
    }
}
