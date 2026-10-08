//! Access control — the levels are yours to define.
//!
//! - **The model**: [`Level`], [`LevelTable`], [`Gate`], [`Subject`].
//! - **The decision**: [`AccessPolicy`], by default [`LevelPolicy`]'s ordered comparison, and the
//!   "one level table means everything passes" skeleton ([`Access::allows`]).
//! - **Authentication**: [`Authenticator`], with [`PinTable`] as the reference implementation.
//! - **The session**: [`Session`], temporary unlocks ([`UnlockMode`], [`Elevation`]), the timeout
//!   and the idle lock.
//!
//! A launch that fails its gate does not open. It emits [`AccessEvent::UnlockRequested`] and, in
//! `prompt` mode with an authenticator, the shell's prompt opens over the screen; a granted
//! unlock runs the launch again, through the same gate.

mod auth;
mod knock;
mod painters;
pub(crate) mod prompt;

pub use auth::{
    AdminResult, AuthMethod, AuthOutcome, Authenticator, Credential, CredentialAdmin,
    CredentialEntry, PinTable, MAX_PIN_LEN,
};
pub use knock::{
    Corner, HiddenEntry, KnockInput, KnockStep, KnockTrigger, Tap, TapKnock, Zone, ZoneKnock,
};
pub use painters::{
    LockScreenCx, LockScreenPainter, PromptPiece, UnlockPromptCx, UnlockPromptPainter,
};

use crate::config::AccessConfig;
use crate::error::{Error, Result};
#[cfg(feature = "settings")]
use crate::i18n::tr_key;
use crate::i18n::LabelKey;
use crate::screen::LaunchAction;
use crate::theme::ColorRole;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// An index into the integrator's level table. Low to high. No constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Level(pub u16);

/// A level definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelDef {
    /// The name in the config file (`"operator"`).
    pub name: String,
    /// The display label.
    pub label: LabelKey,
    /// The level's colour (the status bar user dot, and so on).
    pub color: Option<ColorRole>,
}

/// The level table. A length of 1 means no authentication. No upper bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelTable(Vec<LevelDef>);

impl LevelTable {
    /// Build one from a list of names (the label is the name).
    #[must_use]
    pub fn from_names(names: &[String]) -> Self {
        Self(
            names
                .iter()
                .map(|n| LevelDef {
                    name: n.clone(),
                    label: n.clone(),
                    color: None,
                })
                .collect(),
        )
    }

    /// How many levels.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it is empty (a config error state).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// By index.
    #[must_use]
    pub fn get(&self, level: Level) -> Option<&LevelDef> {
        self.0.get(usize::from(level.0))
    }

    /// Name → level.
    #[must_use]
    pub fn index_of(&self, name: &str) -> Option<Level> {
        self.0
            .iter()
            .position(|d| d.name == name)
            .and_then(|i| u16::try_from(i).ok())
            .map(Level)
    }

    /// The lowest.
    #[must_use]
    pub fn bottom(&self) -> Level {
        Level(0)
    }

    /// The highest.
    #[must_use]
    pub fn top(&self) -> Level {
        Level(u16::try_from(self.0.len().saturating_sub(1)).unwrap_or(u16::MAX))
    }

    /// All of them.
    #[must_use]
    pub fn defs(&self) -> &[LevelDef] {
        &self.0
    }
}

/// A name for one capability. The built-in gates are a fixed set of names; integrator gates are free-form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Gate(pub Cow<'static, str>);

impl Gate {
    /// From a static name — **no heap allocation**. For the built-in gate constants
    /// (`overlay.open`, `chrome.emergency`, …) and anywhere the decision runs every frame
    /// (`From<&str>` is always `Owned`).
    #[must_use]
    pub const fn borrowed(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Gate {
    fn from(value: &str) -> Self {
        Self(Cow::Owned(value.to_owned()))
    }
}

impl From<String> for Gate {
    fn from(value: String) -> Self {
        Self(Cow::Owned(value))
    }
}

impl From<&String> for Gate {
    fn from(value: &String) -> Self {
        Self(Cow::Owned(value.clone()))
    }
}

/// "Who is this", as an authenticator returns it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Subject {
    /// A user identifier (optional).
    pub id: Option<String>,
    /// The level.
    pub level: Level,
    /// Arbitrary attributes for the integrator's policy.
    pub attrs: BTreeMap<String, String>,
}

/// The decision. The default implementation compares order ([`LevelPolicy`]). An integrator can replace it entirely.
pub trait AccessPolicy {
    /// Does this subject pass this gate.
    fn allows(&self, subject: &Subject, gate: &Gate) -> bool;
    /// The line beside the padlock badge ("needs the service level").
    fn hint(&self, _gate: &Gate) -> Option<LabelKey> {
        None
    }
}

/// What happens to an unassigned gate. With two or more levels this **must** be stated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultGate {
    /// The lowest level (always open).
    Bottom,
    /// The highest level.
    Top,
    /// A specific level.
    Level(Level),
}

/// The fixed names of the built-in gates. Ones whose name comes from
/// an integrator declaration, like `tile.<name>`, are left out — a missing-assignment warning
/// only means something for a fixed name. Status bar items (`status.*`) have fixed names
/// ([`crate::chrome::BUILTIN_IDS`]) so they are included below: they are usually assigned to
/// bottom, and in a `default_gate = "top"` setup forgetting them makes **the whole status bar
/// look empty** with no way to tell why.
const BUILTIN_GATES: &[&str] = &[
    "settings.wifi",
    "settings.bluetooth",
    "settings.display",
    "settings.sound",
    "settings.locale",
    "settings.power",
    "settings.network",
    "settings.network.edit",
    "settings.datetime",
    "settings.datetime.set",
    "settings.credentials",
    "overlay.open",
    "nav.recents",
    "workspace.split",
    "desktop.edit",
    "chrome.emergency",
    "session.lock",
    "session.logout",
];

/// The ordered policy: pass if the level assigned to the gate is ≤ the subject's level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelPolicy {
    gates: BTreeMap<String, Level>,
    default_gate: DefaultGate,
    table: LevelTable,
}

impl LevelPolicy {
    /// Build one from the gate assignments and the unassigned handling.
    #[must_use]
    pub fn new(
        table: LevelTable,
        gates: BTreeMap<String, Level>,
        default_gate: DefaultGate,
    ) -> Self {
        Self {
            gates,
            default_gate,
            table,
        }
    }

    /// The level a gate requires.
    #[must_use]
    pub fn required(&self, gate: &Gate) -> Level {
        self.gates
            .get(gate.as_str())
            .copied()
            .unwrap_or(match self.default_gate {
                DefaultGate::Bottom => self.table.bottom(),
                DefaultGate::Top => self.table.top(),
                DefaultGate::Level(level) => level,
            })
    }
}

impl AccessPolicy for LevelPolicy {
    fn allows(&self, subject: &Subject, gate: &Gate) -> bool {
        subject.level >= self.required(gate)
    }

    fn hint(&self, gate: &Gate) -> Option<LabelKey> {
        self.table.get(self.required(gate)).map(|d| d.label.clone())
    }
}

/// `[access] mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessMode {
    /// The shell draws its own prompt and lock screen, through an [`Authenticator`]. With no
    /// authenticator (no `[access.pin_table]` and none from
    /// [`ShellBuilder::authenticator`](crate::ShellBuilder::authenticator)) there is nothing to
    /// draw, and it behaves as `Routing` does.
    #[default]
    Prompt,
    /// The decision and the session only. On a failure it emits [`AccessEvent::UnlockRequested`]
    /// and never draws a prompt — authentication is the integrator's.
    Routing,
    /// Ignore the gates.
    Off,
}

/// How an icon or item presents when its gate fails (`.visibility`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// Not rendered.
    Hidden,
    /// Greyed out with a padlock; a tap requests an unlock.
    #[default]
    Locked,
}

/// What a granted unlock does to the session (`[access] unlock_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnlockMode {
    /// The granted subject **becomes** the session, until a logout, a timeout or the integrator
    /// lowers it.
    Switch,
    /// The granted subject holds for `temporary_secs` **from the unlock**, then the subject from
    /// before comes back by itself.
    #[default]
    Temporary,
}

/// A temporary unlock in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elevation {
    /// Who comes back when it ends — the subject from before the first unlock, however many
    /// unlocks were stacked on top of it.
    pub restore: Subject,
    /// When it ends (the shell's time).
    pub until: Instant,
}

/// Why the session's subject changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChangeReason {
    /// The shell's prompt granted it.
    Unlock,
    /// [`LaunchAction::Logout`] in `prompt` mode — back to the starting subject.
    Logout,
    /// `session_timeout_secs` passed with no input.
    Timeout,
    /// A temporary unlock ran out.
    ElevationExpired,
    /// The panel locked — [`LaunchAction::Lock`], or `idle_lock_secs` with no input.
    Lock,
    /// [`ShellHandle::set_subject`](crate::ShellHandle::set_subject).
    Integrator,
}

/// The session.
#[derive(Debug, Clone)]
pub struct Session {
    /// The current subject.
    pub subject: Subject,
    /// When the session started.
    pub since: Instant,
    /// The last input activity.
    pub last_activity: Instant,
    /// A temporary unlock in progress, and who comes back after it.
    pub elevation: Option<Elevation>,
}

impl Session {
    /// Start with a subject.
    #[must_use]
    pub fn new(subject: Subject) -> Self {
        let now = Instant::now();
        Self {
            subject,
            since: now,
            last_activity: now,
            elevation: None,
        }
    }
}

/// A change of subject, for the shell to announce and pass on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionChange {
    pub(crate) from: Level,
    pub(crate) to: Level,
    pub(crate) reason: ChangeReason,
}

/// An access event. It never carries a credential value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AccessEvent {
    /// Something failed its gate. In `routing` mode — and in `prompt` mode with no authenticator
    /// — this is all that happens, and authentication is the integrator's; in `prompt` mode the
    /// shell's prompt opens as well.
    UnlockRequested {
        /// The gate.
        gate: Gate,
        /// What to run once authenticated.
        then: Option<LaunchAction>,
    },
    /// The prompt granted a subject. [`AccessEvent::SessionChanged`] follows.
    Unlocked {
        /// The gate the prompt opened for — `session.lock` for the lock screen.
        gate: Gate,
        /// The subject's id, where the authenticator gave one.
        subject_id: Option<String>,
        /// The level granted.
        level: Level,
        /// How it was applied.
        mode: UnlockMode,
    },
    /// The authenticator refused what was entered.
    Denied {
        /// The gate.
        gate: Gate,
    },
    /// The authenticator refused any more tries until `until` (the shell's time).
    Locked {
        /// The gate.
        gate: Gate,
        /// When it accepts again.
        until: Instant,
    },
    /// The session's subject changed.
    SessionChanged {
        /// The previous level.
        from: Level,
        /// The new level.
        to: Level,
        /// Why.
        reason: ChangeReason,
    },
    /// The shell's lock screen came up (`true`) or went away (`false`).
    LockScreenToggled(bool),
}

/// The screen that manages the authenticator's entries.
#[cfg(feature = "settings")]
pub(crate) const CREDENTIALS_SCREEN: &str = "settings.credentials";

/// A secret on its way from `settings.credentials` to the authenticator — `Debug` never prints it.
#[cfg(feature = "settings")]
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Secret {
    /// Digits.
    Pin(String),
    /// A pattern's dots, from 0.
    Pattern(Vec<u8>),
    /// A password — set under the entry's id, which is its user name.
    Password(String),
}

#[cfg(feature = "settings")]
impl Secret {
    /// The credential it is handed over as, for the entry `id`.
    fn into_credential(self, id: &str) -> Credential {
        match self {
            Self::Pin(digits) => Credential::Pin(digits),
            Self::Pattern(dots) => Credential::Pattern(dots),
            Self::Password(secret) => Credential::Password {
                user: Some(id.to_owned()),
                secret,
            },
        }
    }
}

#[cfg(feature = "settings")]
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// A kind of secret `settings.credentials` can set — one per way in the authenticator offers
/// that a person types or draws.
#[cfg(feature = "settings")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecretKind {
    /// Digits, at most `max_len` of them.
    Pin {
        /// Exactly this many digits, where the prompt submits on the last one — a longer or
        /// shorter PIN could never be entered there. `0` takes any number up to `max_len`.
        len: u8,
        /// The most digits.
        max_len: u8,
    },
    /// A path through `grid × grid` dots, `min_points` of them at least.
    Pattern {
        /// Dots on a side.
        grid: u8,
        /// The fewest dots.
        min_points: u8,
    },
    /// A password.
    Password,
}

#[cfg(feature = "settings")]
impl SecretKind {
    /// The kinds `methods` offer, in their order — a password where there is no other.
    pub(crate) fn of(methods: &[AuthMethod]) -> Vec<Self> {
        let mut kinds = Vec::new();
        for method in methods {
            let kind = match method {
                AuthMethod::Pin { len, max_len, .. } => {
                    let held = |n: u8| u8::try_from(usize::from(n).min(MAX_PIN_LEN)).unwrap_or(0);
                    let len = held(*len);
                    let max_len = match (len, *max_len) {
                        (0, 0) => held(u8::MAX),
                        (0, most) => held(most),
                        (exact, _) => exact,
                    };
                    Self::Pin { len, max_len }
                }
                AuthMethod::Pattern {
                    grid, min_points, ..
                } => Self::Pattern {
                    grid: (*grid).clamp(crate::widgets::MIN_GRID, crate::widgets::MAX_GRID),
                    min_points: *min_points,
                },
                AuthMethod::Password { .. } => Self::Password,
                // A badge is enrolled at its reader, not typed here.
                AuthMethod::External { .. } => continue,
            };
            let same = kinds
                .iter_mut()
                .find(|k| std::mem::discriminant(*k) == std::mem::discriminant(&kind));
            match (same, kind) {
                // Two keypads: the PIN set here has to go in on both — the fixed length where
                // either has one, and otherwise the shorter cap.
                (Some(Self::Pin { len, max_len }), Self::Pin { len: l, max_len: m }) => {
                    if *len == 0 {
                        *len = l;
                    }
                    *max_len = if *len > 0 { *len } else { (*max_len).min(m) };
                }
                (Some(_), _) => {}
                (None, kind) => kinds.push(kind),
            }
        }
        if kinds.is_empty() {
            kinds.push(Self::Password);
        }
        kinds
    }

    /// What the form calls it — a key, looked up where it is drawn.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Pin { .. } => tr_key!("PIN"),
            Self::Pattern { .. } => tr_key!("Pattern"),
            Self::Password => tr_key!("Password"),
        }
    }
}

/// One management call `settings.credentials` asks the shell to make.
#[cfg(feature = "settings")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CredentialOp {
    /// [`CredentialAdmin::set_secret`].
    SetSecret {
        /// The entry.
        id: String,
        /// The new secret, as typed.
        secret: Secret,
    },
    /// [`CredentialAdmin::set_level`].
    SetLevel {
        /// The entry.
        id: String,
        /// The new level.
        level: Level,
    },
    /// [`CredentialAdmin::add`].
    Add {
        /// The new entry's id.
        id: String,
        /// Its level.
        level: Level,
        /// Its secret, as typed.
        secret: Secret,
    },
    /// [`CredentialAdmin::remove`].
    Remove {
        /// The entry.
        id: String,
    },
}

/// What one Apply on `settings.credentials` came to: a line for each call, done or refused.
///
/// One Apply can be two calls — a new level and a new secret — and each is answered on its own.
/// A single "last answer" let the second hide the first, so a refused level under a saved
/// secret showed only the green line.
#[cfg(feature = "settings")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdminNote {
    /// Counts up with every batch, so the screen can tell the answer to its own Apply from one
    /// left over from before.
    pub(crate) seq: u64,
    /// What was done.
    pub(crate) done: Vec<NoteLine>,
    /// What was not, and why — the authenticator's words, or the shell's key; the screen looks
    /// either up.
    pub(crate) refused: Vec<(NoteLine, String)>,
}

/// One line of an [`AdminNote`]: a key with `{id}` where the entry goes, worded where it is drawn
/// — so a note written before a change of language reads in the new one.
#[cfg(feature = "settings")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoteLine {
    pub(crate) key: &'static str,
    pub(crate) id: String,
}

#[cfg(feature = "settings")]
impl NoteLine {
    fn new(key: &'static str, id: &str) -> Self {
        Self {
            key,
            id: id.to_owned(),
        }
    }

    /// The line in the active language.
    pub(crate) fn text(&self, strings: &crate::i18n::Strings) -> String {
        strings.get(self.key).replace("{id}", &self.id)
    }
}

/// Why the shell refused a change on its own.
#[cfg(feature = "settings")]
mod admin_says {
    use crate::i18n::tr_key;

    pub(super) const NOT_CHANGED: &str = tr_key!("Not changed");
    pub(super) const NO_ADMIN: &str =
        tr_key!("The authenticator does not manage its entries here.");
    pub(super) const NOT_ALLOWED: &str = tr_key!("Managing entries needs a higher level.");
    pub(super) const ENTRY_ABOVE: &str = tr_key!("It belongs to a level above yours.");
    pub(super) const LEVEL_ABOVE: &str = tr_key!("That level is above yours.");
}

/// The access-control state the shell owns: the level table, the policy, the mode, the session
/// and the authenticator.
// Independent switches, not a state machine: whether Continue is offered, whether the idle lock
// already fired, whether the entries need listing again, what kind of secret they take.
#[allow(clippy::struct_excessive_bools)]
pub struct Access {
    table: LevelTable,
    policy: Box<dyn AccessPolicy>,
    mode: AccessMode,
    session: Session,
    /// The starting subject — where a logout, a timeout and a lock return to.
    initial: Subject,
    unlock_mode: UnlockMode,
    temporary: Duration,
    session_timeout: Option<Duration>,
    idle_lock: Option<Duration>,
    lock_screen_continue: bool,
    /// Whether the idle lock already fired in this stretch without input.
    idle_locked: bool,
    authenticator: Option<Box<dyn Authenticator>>,
    /// `settings.credentials`' copy of the authenticator's entries, refreshed when they may have
    /// changed — so the screen never calls `list()` itself, and not once a frame.
    #[cfg(feature = "settings")]
    credentials: Vec<CredentialEntry>,
    #[cfg(feature = "settings")]
    credentials_dirty: bool,
    /// The kinds of secret the screen can set, from the authenticator's methods.
    #[cfg(feature = "settings")]
    secret_kinds: Vec<SecretKind>,
    /// What the last batch of management calls came to.
    #[cfg(feature = "settings")]
    admin_note: Option<AdminNote>,
}

impl std::fmt::Debug for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Access")
            .field("table", &self.table)
            .field("mode", &self.mode)
            .field("session", &self.session)
            .field("unlock_mode", &self.unlock_mode)
            .field("authenticator", &self.authenticator.is_some())
            .finish_non_exhaustive()
    }
}

/// Seconds → an optional span, where 0 means "off".
fn secs_or_off(secs: u64) -> Option<Duration> {
    (secs > 0).then(|| Duration::from_secs(secs))
}

/// The longest any `[access]` timer may be: a year. Anything longer is a typo, and an `Instant`
/// pushed that far out could overflow — a panic on the first frame, from a config value.
pub(crate) const MAX_TIMER_SECS: u64 = 365 * 24 * 60 * 60;

/// The `[access]` timers in range: none past [`MAX_TIMER_SECS`].
fn check_timers(cfg: &AccessConfig) -> Result<()> {
    for (key, secs) in [
        ("temporary_secs", cfg.temporary_secs),
        ("session_timeout_secs", cfg.session_timeout_secs),
        ("idle_lock_secs", cfg.idle_lock_secs),
    ] {
        if secs > MAX_TIMER_SECS {
            return Err(Error::Config(format!(
                "[access] {key} = {secs} is longer than a year ({MAX_TIMER_SECS} s)"
            )));
        }
    }
    Ok(())
}

/// `[access] unlock_mode`, with the length a temporary unlock needs.
fn unlock_mode(cfg: &AccessConfig) -> Result<UnlockMode> {
    let mode = match cfg.unlock_mode.as_str() {
        "switch" => UnlockMode::Switch,
        "temporary" => UnlockMode::Temporary,
        other => {
            return Err(Error::Config(format!(
                "[access] unlock_mode = \"{other}\" must be switch or temporary"
            )))
        }
    };
    if mode == UnlockMode::Temporary && cfg.temporary_secs == 0 {
        return Err(Error::Config(
            "[access] temporary_secs must be more than 0 with unlock_mode = \"temporary\" - use \"switch\" for an unlock that lasts"
                .to_owned(),
        ));
    }
    Ok(mode)
}

/// The reference implementation, built only when there is something to check against. It
/// needs the level names, so it happens here rather than in config.rs.
fn reference_authenticator(
    cfg: &AccessConfig,
    table: &LevelTable,
) -> Result<Option<Box<dyn Authenticator>>> {
    if cfg.pin_table.pins.is_empty() && cfg.pattern_table.patterns.is_empty() {
        return Ok(None);
    }
    let pins = PinTable::from_tables(&cfg.pin_table, &cfg.pattern_table, table)?;
    Ok(Some(Box::new(pins)))
}

impl Access {
    /// Built from `[access]`. Validation ("an unassigned gate"):
    /// - An empty level table is an error.
    /// - Two or more levels with no `default_gate` is an error.
    /// - A `default_gate`, `initial` or `gates` value not in the level table is an error.
    /// - An `unlock_mode` other than `switch` or `temporary`, or `temporary_secs = 0` with
    ///   `temporary`, is an error.
    /// - `[access.pin_table]` and `[access.pattern_table]` are checked by
    ///   [`PinTable::from_tables`], and with any PIN or pattern in them the table becomes the
    ///   authenticator.
    ///
    /// # Errors
    /// [`Error::Config`] on any of the above.
    pub fn from_config(cfg: &AccessConfig) -> Result<Self> {
        if cfg.levels.is_empty() {
            return Err(Error::Config("[access] levels is empty".to_owned()));
        }
        let table = LevelTable::from_names(&cfg.levels);
        let mode = match cfg.mode.as_str() {
            "prompt" => AccessMode::Prompt,
            "routing" => AccessMode::Routing,
            "off" => AccessMode::Off,
            other => {
                return Err(Error::Config(format!(
                    "[access] mode = \"{other}\" must be one of prompt, routing or off"
                )))
            }
        };
        let default_gate = match cfg.default_gate.as_deref() {
            Some("top") => DefaultGate::Top,
            Some("bottom") => DefaultGate::Bottom,
            Some(name) => DefaultGate::Level(table.index_of(name).ok_or_else(|| {
                Error::Config(format!(
                    "[access] default_gate = \"{name}\" is not in levels"
                ))
            })?),
            None if table.len() >= 2 => {
                return Err(Error::Config(
                    "[access] default_gate is required once levels has more than one entry"
                        .to_owned(),
                ))
            }
            None => DefaultGate::Bottom,
        };
        let mut gates = BTreeMap::new();
        for (gate, level_name) in &cfg.gates {
            let level = match level_name.as_str() {
                "top" => table.top(),
                "bottom" => table.bottom(),
                name => table.index_of(name).ok_or_else(|| {
                    Error::Config(format!(
                        "[access.gates] \"{gate}\" = \"{name}\" - no such level"
                    ))
                })?,
            };
            gates.insert(gate.clone(), level);
        }
        let initial = match cfg.initial.as_deref() {
            Some(name) => table.index_of(name).ok_or_else(|| {
                Error::Config(format!("[access] initial = \"{name}\" is not in levels"))
            })?,
            None => table.bottom(),
        };
        check_timers(cfg)?;
        let unlock_mode = unlock_mode(cfg)?;
        let authenticator = reference_authenticator(cfg, &table)?;
        // The built-in gates left unassigned are announced once at startup so an integrator can see
        // what they missed. With one level table everything passes anyway, so it means nothing there.
        if table.len() > 1 {
            let missing: Vec<&str> = BUILTIN_GATES
                .iter()
                .chain(crate::chrome::BUILTIN_IDS)
                .copied()
                .filter(|gate| !gates.contains_key(*gate))
                .collect();
            if !missing.is_empty() {
                // `Vec::join` trips `xtask sync-check`'s `.join(` token check, so it is written in Debug form.
                log::info!(
                    "{} built-in gates are unassigned in [access.gates], so default_gate applies: {missing:?}",
                    missing.len()
                );
            }
        }
        let policy = LevelPolicy::new(table.clone(), gates, default_gate);
        let initial = Subject {
            id: None,
            level: initial,
            attrs: BTreeMap::new(),
        };
        Ok(Self {
            table,
            policy: Box::new(policy),
            mode,
            session: Session::new(initial.clone()),
            initial,
            unlock_mode,
            temporary: Duration::from_secs(cfg.temporary_secs),
            session_timeout: secs_or_off(cfg.session_timeout_secs),
            idle_lock: secs_or_off(cfg.idle_lock_secs),
            lock_screen_continue: cfg.lock_screen.allow_continue,
            idle_locked: false,
            authenticator,
            #[cfg(feature = "settings")]
            credentials: Vec::new(),
            #[cfg(feature = "settings")]
            credentials_dirty: true,
            #[cfg(feature = "settings")]
            secret_kinds: Vec::new(),
            #[cfg(feature = "settings")]
            admin_note: None,
        })
    }

    /// Replace the policy (the integrator's `AccessPolicy`).
    pub fn set_policy(&mut self, policy: Box<dyn AccessPolicy>) {
        self.policy = policy;
    }

    /// The decision skeleton: with one level, or `mode = off`, **everything passes**. Otherwise it delegates to the policy.
    #[must_use]
    pub fn allows(&self, gate: &Gate) -> bool {
        if self.levels_open() {
            return true;
        }
        self.policy.allows(&self.session.subject, gate)
    }

    /// The decision by name — for the fallback sites where the gate is `None` and **the id is
    /// the gate**. The result matches [`Access::allows`], and with one level or
    /// `mode = off` it passes before building a [`Gate`], so there is **no heap allocation**
    /// (`Gate::from(&str)` is always `Owned`). Somewhere called every frame is
    /// still better off **fixing the gate at declaration time** (`IconSlot` fills it in when it
    /// is placed).
    #[must_use]
    pub(crate) fn allows_name(&self, name: &str) -> bool {
        if self.levels_open() {
            return true;
        }
        self.policy.allows(&self.session.subject, &Gate::from(name))
    }

    /// The lock hint.
    #[must_use]
    pub fn hint(&self, gate: &Gate) -> Option<LabelKey> {
        self.policy.hint(gate)
    }

    /// The session.
    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// What the bars call whoever is at the panel: the subject's id, which is a name and stays as
    /// it is, or else its level's label, which is a key and is looked up. Empty with neither.
    #[must_use]
    pub fn subject_name<'a>(&'a self, strings: &'a crate::i18n::Strings) -> &'a str {
        let subject = &self.session.subject;
        subject
            .id
            .as_deref()
            .or_else(|| {
                self.table
                    .get(subject.level)
                    .map(|def| strings.get(&def.label))
            })
            .unwrap_or("")
    }

    /// The level table.
    #[must_use]
    pub fn table(&self) -> &LevelTable {
        &self.table
    }

    /// The mode.
    #[must_use]
    pub fn mode(&self) -> AccessMode {
        self.mode
    }

    /// The starting subject (`[access] initial`) — where a logout, a timeout and a lock return to.
    #[must_use]
    pub fn initial_subject(&self) -> &Subject {
        &self.initial
    }

    /// What a granted unlock does (`[access] unlock_mode`).
    #[must_use]
    pub fn unlock_mode(&self) -> UnlockMode {
        self.unlock_mode
    }

    /// Whether the session is above where it started — unlocked, temporarily or not. The status
    /// bar's `status.lock` shows exactly this.
    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.session.elevation.is_some() || self.session.subject.level > self.initial.level
    }

    /// Whether the lock screen offers "Continue" (`[access.lock_screen] allow_continue`).
    #[must_use]
    pub fn lock_screen_continue(&self) -> bool {
        self.lock_screen_continue
    }

    /// Whether there is an authenticator — `[access.pin_table]`'s, or one from
    /// [`ShellBuilder::authenticator`](crate::ShellBuilder::authenticator).
    #[must_use]
    pub fn has_authenticator(&self) -> bool {
        self.authenticator.is_some()
    }

    /// **Whether the shell draws its own prompt and lock screen** — `mode = "prompt"`, more than
    /// one level, and an authenticator to ask. Otherwise a failed gate is an
    /// [`AccessEvent::UnlockRequested`] and nothing more, as in `routing` mode.
    #[must_use]
    pub fn can_prompt(&self) -> bool {
        self.mode == AccessMode::Prompt && self.table.len() > 1 && self.authenticator.is_some()
    }

    /// Put an authenticator in (the shell attaches its waker first).
    pub(crate) fn install_authenticator(&mut self, authenticator: Box<dyn Authenticator>) {
        self.authenticator = Some(authenticator);
        #[cfg(feature = "settings")]
        {
            self.credentials_dirty = true;
        }
    }

    #[cfg(feature = "settings")]
    /// Whether the authenticator manages its entries here ([`Authenticator::admin`]) — and so
    /// whether `settings.credentials` has anything to do.
    pub(crate) fn has_admin(&mut self) -> bool {
        self.authenticator
            .as_deref_mut()
            .is_some_and(|auth| auth.admin().is_some())
    }

    #[cfg(feature = "settings")]
    /// The entries as last listed.
    pub(crate) fn credentials(&self) -> &[CredentialEntry] {
        &self.credentials
    }

    #[cfg(feature = "settings")]
    /// The kinds of secret a new one can be, in the authenticator's order.
    pub(crate) fn secret_kinds(&self) -> &[SecretKind] {
        &self.secret_kinds
    }

    #[cfg(feature = "settings")]
    /// What the last batch of management calls came to.
    pub(crate) fn admin_note(&self) -> Option<&AdminNote> {
        self.admin_note.as_ref()
    }

    #[cfg(feature = "settings")]
    /// Whether the session may give `level`, or change an entry at it: anything at or below its
    /// own. Levels mean nothing with one level or `mode = off`, and then everything may.
    pub(crate) fn may_grant(&self, level: Level) -> bool {
        self.levels_open() || level <= self.session.subject.level
    }

    /// One level, or `mode = off`: every gate passes.
    pub(crate) fn levels_open(&self) -> bool {
        self.table.len() <= 1 || self.mode == AccessMode::Off
    }

    #[cfg(feature = "settings")]
    /// The entries may have changed — list them again before the next frame draws.
    pub(crate) fn mark_credentials_dirty(&mut self) {
        self.credentials_dirty = true;
    }

    #[cfg(feature = "settings")]
    /// Stage 4: list the entries again where they may have changed.
    pub(crate) fn refresh_credentials(&mut self) {
        if !std::mem::take(&mut self.credentials_dirty) {
            return;
        }
        let Some(auth) = self.authenticator.as_deref_mut() else {
            self.credentials.clear();
            return;
        };
        self.secret_kinds = SecretKind::of(&auth.methods());
        self.credentials = auth.admin().map(|admin| admin.list()).unwrap_or_default();
    }

    #[cfg(feature = "settings")]
    /// Make the management calls of one Apply on `settings.credentials` (stage 14), and note what
    /// each came to. A secret becomes a [`Credential`] of its kind — a PIN, a pattern, or a
    /// password under the entry's id — and goes to the authenticator by value.
    ///
    /// `allowed` is whether the session still passes the screen's gate: the calls were asked for
    /// while it was drawn, and the session can drop between then and now. Above that, nothing is
    /// given or touched above the session's own level ([`Access::may_grant`]); the authenticator is
    /// told who is asking and has the last word on the rest.
    pub(crate) fn apply_admin(&mut self, ops: Vec<CredentialOp>, allowed: bool) {
        let seq = self
            .admin_note
            .as_ref()
            .map_or(0, |n| n.seq)
            .saturating_add(1);
        let mut note = AdminNote {
            seq,
            done: Vec::new(),
            refused: Vec::new(),
        };
        self.credentials_dirty = true;
        let ceiling = (!self.levels_open()).then_some(self.session.subject.level);
        let fits = |level: Level| ceiling.is_none_or(|top| level <= top);
        let admin = self.authenticator.as_deref_mut().and_then(|a| a.admin());
        let Some(admin) = admin.filter(|_| allowed) else {
            let why = if allowed {
                admin_says::NO_ADMIN
            } else {
                admin_says::NOT_ALLOWED
            };
            note.refused
                .push((NoteLine::new(admin_says::NOT_CHANGED, ""), why.to_owned()));
            self.admin_note = Some(note);
            return;
        };
        admin.set_actor(&self.session.subject);
        let entries = admin.list();
        // An entry the list does not have goes to the authenticator, which says so.
        let mine = |id: &str| {
            entries
                .iter()
                .find(|e| e.id == id)
                .is_none_or(|e| fits(e.level))
        };
        for op in ops {
            let (done, not, result) = match op {
                CredentialOp::SetSecret { id, secret } => (
                    NoteLine::new(tr_key!("New secret saved for {id}"), &id),
                    NoteLine::new(tr_key!("Secret for {id} not saved"), &id),
                    if mine(&id) {
                        admin.set_secret(&id, secret.into_credential(&id))
                    } else {
                        Err(admin_says::ENTRY_ABOVE.to_owned())
                    },
                ),
                CredentialOp::SetLevel { id, level } => (
                    NoteLine::new(tr_key!("Level changed for {id}"), &id),
                    NoteLine::new(tr_key!("Level for {id} not changed"), &id),
                    if !mine(&id) {
                        Err(admin_says::ENTRY_ABOVE.to_owned())
                    } else if !fits(level) {
                        Err(admin_says::LEVEL_ABOVE.to_owned())
                    } else {
                        admin.set_level(&id, level)
                    },
                ),
                CredentialOp::Add { id, level, secret } => (
                    NoteLine::new(tr_key!("Added {id}"), &id),
                    NoteLine::new(tr_key!("{id} not added"), &id),
                    if fits(level) {
                        admin.add(&id, level, secret.into_credential(&id))
                    } else {
                        Err(admin_says::LEVEL_ABOVE.to_owned())
                    },
                ),
                CredentialOp::Remove { id } => (
                    NoteLine::new(tr_key!("Removed {id}"), &id),
                    NoteLine::new(tr_key!("{id} not removed"), &id),
                    if mine(&id) {
                        admin.remove(&id)
                    } else {
                        Err(admin_says::ENTRY_ABOVE.to_owned())
                    },
                ),
            };
            match result {
                Ok(()) => note.done.push(done),
                Err(why) => note.refused.push((not, why)),
            }
        }
        self.admin_note = Some(note);
    }

    /// The authenticator, to ask.
    pub(crate) fn authenticator_mut(&mut self) -> Option<&mut (dyn Authenticator + 'static)> {
        self.authenticator.as_deref_mut()
    }

    /// Set the session's clock to the shell's at start-up — its times began at
    /// [`Instant::now`] when the config was read, and the shell's time is the one every timer
    /// is measured against.
    pub(crate) fn start_clock(&mut self, now: Instant) {
        self.session.since = now;
        self.session.last_activity = now;
    }

    /// Replace the subject (`ShellHandle::set_subject`). Returns the previous level — `Shell`
    /// uses that return value to decide about propagating `AccessChanged` and closing instances
    /// that no longer pass. A temporary unlock in progress ends with it:
    /// the integrator said who this is.
    pub fn set_subject(&mut self, subject: Subject) -> Level {
        self.set_subject_at(subject, Instant::now())
    }

    /// [`Access::set_subject`] at the shell's time `now`. Whoever the integrator vouched for is
    /// someone at the panel: the idle stretch starts again, as it does on a touch — a reader that
    /// grants after the panel sat idle must not be timed out or idle-locked in the same frame.
    pub(crate) fn set_subject_at(&mut self, subject: Subject, now: Instant) -> Level {
        let from = self.session.subject.level;
        self.session.subject = subject;
        self.session.elevation = None;
        self.session.since = now;
        self.session.last_activity = now;
        self.idle_locked = false;
        from
    }

    /// Apply a granted unlock per `unlock_mode`. Returns the previous level.
    ///
    /// A temporary unlock on top of another keeps the **first** one's `restore`: elevating an
    /// operator to a maintainer for five minutes and then to something else must still come back
    /// to the viewer who started.
    pub(crate) fn unlock(&mut self, granted: Subject, now: Instant) -> Level {
        let from = self.session.subject.level;
        match self.unlock_mode {
            UnlockMode::Switch => {
                self.session.elevation = None;
                self.session.subject = granted;
                self.session.since = now;
            }
            UnlockMode::Temporary => {
                let restore = self
                    .session
                    .elevation
                    .take()
                    .map_or_else(|| self.session.subject.clone(), |e| e.restore);
                self.session.elevation = Some(Elevation {
                    restore,
                    until: now.checked_add(self.temporary).unwrap_or(now),
                });
                self.session.subject = granted;
            }
        }
        // Someone just proved who they are at the panel: the idle stretch starts again.
        self.session.last_activity = now;
        self.idle_locked = false;
        from
    }

    /// Back to the starting subject (a logout, a lock). Returns the previous level.
    pub(crate) fn reset(&mut self, now: Instant) -> Level {
        let from = self.session.subject.level;
        self.session.elevation = None;
        self.session.subject = self.initial.clone();
        self.session.since = now;
        from
    }

    /// Whether anything can lower the session: more than one level, and the gates not off.
    fn session_matters(&self) -> bool {
        self.table.len() > 1 && self.mode != AccessMode::Off
    }

    /// Whether the subject is anything but the starting one.
    pub(crate) fn away_from_start(&self) -> bool {
        self.session.elevation.is_some() || self.session.subject != self.initial
    }

    /// Frame stage 4: record input, end a temporary unlock that ran out, and time the session out.
    /// Returns the change it made, for the shell to announce and propagate.
    pub(crate) fn tick(&mut self, now: Instant, input_activity: bool) -> Option<SessionChange> {
        if input_activity {
            self.session.last_activity = now;
            self.idle_locked = false;
        }
        if !self.session_matters() {
            return None;
        }
        if self
            .session
            .elevation
            .as_ref()
            .is_some_and(|e| now >= e.until)
        {
            let from = self.session.subject.level;
            let elevation = self.session.elevation.take()?;
            self.session.subject = elevation.restore;
            return Some(SessionChange {
                from,
                to: self.session.subject.level,
                reason: ChangeReason::ElevationExpired,
            });
        }
        let idle = now.saturating_duration_since(self.session.last_activity);
        if self.session_timeout.is_some_and(|t| idle >= t) && self.away_from_start() {
            let from = self.reset(now);
            return Some(SessionChange {
                from,
                to: self.initial.level,
                reason: ChangeReason::Timeout,
            });
        }
        None
    }

    /// Whether `idle_lock_secs` just ran out — once per stretch without input. Unlike the
    /// timeout it fires whatever the level table: what a lock means without a prompt is the
    /// integrator's ([`ShellEvent::LockRequested`](crate::ShellEvent::LockRequested)).
    pub(crate) fn idle_lock_due(&mut self, now: Instant) -> bool {
        let Some(after) = self.idle_lock else {
            return false;
        };
        if self.idle_locked || now.saturating_duration_since(self.session.last_activity) < after {
            return false;
        }
        self.idle_locked = true;
        true
    }

    /// The next moment the session changes by itself — for the idle repaint, so a timer
    /// runs out on time on a panel nobody is touching.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        let mut next: Option<Instant> = None;
        let mut take = |at: Instant| next = Some(next.map_or(at, |n| n.min(at)));
        if self.session_matters() {
            if let Some(elevation) = &self.session.elevation {
                take(elevation.until);
            }
            if let Some(timeout) = self.session_timeout {
                if self.away_from_start() {
                    take(self.session.last_activity + timeout);
                }
            }
        }
        if let Some(after) = self.idle_lock {
            if !self.idle_locked {
                take(self.session.last_activity + after);
            }
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::{Access, ChangeReason, Gate, Level, Subject, UnlockMode};
    use crate::config::AccessConfig;
    use std::time::{Duration, Instant};

    /// `settings.credentials` sets each kind of secret the methods offer, once, in their order
    /// — and a password where they offer nothing to type or draw.
    #[cfg(feature = "settings")]
    #[test]
    fn the_screen_sets_each_kind_of_secret_the_methods_offer() {
        use super::{AuthMethod, SecretKind};
        let kinds = SecretKind::of(&[
            AuthMethod::Pin {
                len: 4,
                max_len: 0,
                shuffle: false,
            },
            AuthMethod::External {
                label: "Badge".into(),
            },
            AuthMethod::Pattern {
                grid: 4,
                min_points: 5,
                show_path: true,
            },
            AuthMethod::Pin {
                len: 0,
                max_len: 0,
                shuffle: true,
            },
        ]);
        // The two keypads give one PIN: four digits go in on both.
        assert_eq!(
            kinds,
            vec![
                SecretKind::Pin { len: 4, max_len: 4 },
                SecretKind::Pattern {
                    grid: 4,
                    min_points: 5
                }
            ]
        );
        // A fixed length is what the form must ask for: the prompt submits on the last digit.
        assert_eq!(
            SecretKind::of(&[AuthMethod::Pin {
                len: 6,
                max_len: 8,
                shuffle: false,
            }]),
            vec![SecretKind::Pin { len: 6, max_len: 6 }]
        );
        assert_eq!(
            SecretKind::of(&[AuthMethod::External {
                label: "Badge".into()
            }]),
            vec![SecretKind::Password]
        );
    }

    fn cfg(levels: &[&str], default_gate: Option<&str>) -> AccessConfig {
        AccessConfig {
            levels: levels.iter().map(|s| (*s).to_owned()).collect(),
            default_gate: default_gate.map(str::to_owned),
            ..AccessConfig::default()
        }
    }

    #[test]
    fn single_level_passes_everything() {
        let access = Access::from_config(&cfg(&["only"], None));
        assert!(access.is_ok_and(|a| a.allows(&Gate::from("anything"))));
    }

    #[test]
    fn two_levels_need_default_gate() {
        assert!(Access::from_config(&cfg(&["viewer", "maintainer"], None)).is_err());
        let access = Access::from_config(&cfg(&["viewer", "maintainer"], Some("top")));
        assert!(access.is_ok_and(|a| !a.allows(&Gate::from("admin"))));
        let access = Access::from_config(&cfg(&["viewer", "maintainer"], Some("bottom")));
        assert!(access.is_ok_and(|a| a.allows(&Gate::from("admin"))));
    }

    #[test]
    fn allows_name_matches_allows() {
        // One level table: the name decision passes too (it returns early before building a `Gate` — no allocation).
        let single = Access::from_config(&cfg(&["only"], None));
        assert!(single.is_ok_and(|a| a.allows_name("anything")));
        // Two levels: the same answer as `allows(&Gate::from(name))`.
        for default_gate in ["top", "bottom"] {
            let access = Access::from_config(&cfg(&["viewer", "maintainer"], Some(default_gate)));
            assert!(access.is_ok_and(|a| a.allows_name("admin") == a.allows(&Gate::from("admin"))));
        }
    }

    #[test]
    fn gate_assigned_to_unknown_level_is_a_config_error() {
        let mut config = cfg(&["viewer", "maintainer"], Some("top"));
        config
            .gates
            .insert("settings.wifi".to_owned(), "root".to_owned());
        assert!(Access::from_config(&config).is_err());
    }

    #[test]
    fn pin_table_unknown_level_is_a_config_error() {
        let mut config = cfg(&["viewer", "maintainer"], Some("top"));
        config
            .pin_table
            .pins
            .insert("root".to_owned(), "0000".to_owned());
        assert!(Access::from_config(&config).is_err());
    }

    #[test]
    fn pin_table_known_levels_parse_fine() {
        let mut config = cfg(&["viewer", "maintainer"], Some("top"));
        config
            .pin_table
            .pins
            .insert("maintainer".to_owned(), "987654".to_owned());
        assert!(Access::from_config(&config).is_ok());
    }

    fn level(n: u16) -> Subject {
        Subject {
            level: Level(n),
            ..Subject::default()
        }
    }

    fn three(unlock_mode: &str) -> AccessConfig {
        AccessConfig {
            unlock_mode: unlock_mode.to_owned(),
            temporary_secs: 60,
            ..cfg(&["viewer", "operator", "maintainer"], Some("top"))
        }
    }

    /// What the [`Admin`] below was asked, and who it was told was asking.
    #[cfg(feature = "settings")]
    #[derive(Default)]
    struct Book {
        calls: Vec<String>,
        actor: Option<Level>,
    }

    /// An authenticator that manages `op` and `weak` (operators) and `boss` (a maintainer),
    /// refusing any change to `weak`.
    #[cfg(feature = "settings")]
    struct Admin(std::rc::Rc<std::cell::RefCell<Book>>);

    #[cfg(feature = "settings")]
    impl super::Authenticator for Admin {
        fn methods(&self) -> Vec<super::AuthMethod> {
            vec![super::AuthMethod::Password { needs_user: false }]
        }
        fn submit(&mut self, _: super::Credential, _: Instant) -> super::AuthOutcome {
            super::AuthOutcome::Pending
        }
        fn admin(&mut self) -> Option<&mut dyn super::CredentialAdmin> {
            Some(self)
        }
    }

    #[cfg(feature = "settings")]
    impl Admin {
        fn call(&self, what: String) -> super::AdminResult {
            let refused = what.contains("weak");
            self.0.borrow_mut().calls.push(what);
            if refused {
                Err("Too short.".to_owned())
            } else {
                Ok(())
            }
        }
    }

    #[cfg(feature = "settings")]
    impl super::CredentialAdmin for Admin {
        fn set_actor(&mut self, subject: &Subject) {
            self.0.borrow_mut().actor = Some(subject.level);
        }
        fn list(&self) -> Vec<super::CredentialEntry> {
            [("op", 1), ("weak", 1), ("boss", 2)]
                .iter()
                .map(|(id, level)| super::CredentialEntry {
                    id: (*id).to_owned(),
                    label: (*id).to_owned(),
                    level: Level(*level),
                    disabled: false,
                })
                .collect()
        }
        fn set_secret(&mut self, id: &str, _: super::Credential) -> super::AdminResult {
            self.call(format!("secret {id}"))
        }
        fn set_level(&mut self, id: &str, level: Level) -> super::AdminResult {
            self.call(format!("level {id} {}", level.0))
        }
        fn add(&mut self, id: &str, level: Level, _: super::Credential) -> super::AdminResult {
            self.call(format!("add {id} {}", level.0))
        }
        fn remove(&mut self, id: &str) -> super::AdminResult {
            self.call(format!("remove {id}"))
        }
    }

    /// An operator's session over an [`Admin`], and what the admin is asked.
    #[cfg(feature = "settings")]
    fn operator_admin() -> crate::Result<(Access, std::rc::Rc<std::cell::RefCell<Book>>)> {
        let book = std::rc::Rc::new(std::cell::RefCell::new(Book::default()));
        let mut access = Access::from_config(&three("switch"))?;
        access.install_authenticator(Box::new(Admin(std::rc::Rc::clone(&book))));
        access.unlock(level(1), Instant::now());
        Ok((access, book))
    }

    /// One Apply's calls are each answered, a change above the session's own level never reaches
    /// the authenticator, and the authenticator is told who is asking.
    #[cfg(feature = "settings")]
    #[test]
    fn an_apply_answers_every_call_and_stays_within_the_level() -> crate::Result<()> {
        use super::{admin_says, CredentialOp, Secret};
        let (mut access, book) = operator_admin()?;
        let pin = || Secret::Pin("1234".to_owned());
        let id = |id: &str| id.to_owned();
        access.apply_admin(
            vec![
                CredentialOp::SetLevel {
                    id: id("op"),
                    level: Level(2),
                },
                CredentialOp::SetSecret {
                    id: id("op"),
                    secret: pin(),
                },
                CredentialOp::SetSecret {
                    id: id("weak"),
                    secret: pin(),
                },
                CredentialOp::SetSecret {
                    id: id("boss"),
                    secret: pin(),
                },
                CredentialOp::Remove { id: id("boss") },
                CredentialOp::Add {
                    id: id("new"),
                    level: Level(2),
                    secret: pin(),
                },
                CredentialOp::Add {
                    id: id("new"),
                    level: Level(1),
                    secret: pin(),
                },
            ],
            true,
        );
        // The lines are keys worded where they are drawn; in English they read as written.
        let en = crate::i18n::Strings::new("en");
        let note = access.admin_note().cloned();
        assert_eq!(note.as_ref().map(|n| n.seq), Some(1));
        assert_eq!(
            note.as_ref()
                .map(|n| n.done.iter().map(|l| l.text(&en)).collect::<Vec<_>>()),
            Some(vec![id("New secret saved for op"), id("Added new")])
        );
        let refused: Vec<(String, String)> = [
            ("Level for op not changed", admin_says::LEVEL_ABOVE),
            ("Secret for weak not saved", "Too short."),
            ("Secret for boss not saved", admin_says::ENTRY_ABOVE),
            ("boss not removed", admin_says::ENTRY_ABOVE),
            ("new not added", admin_says::LEVEL_ABOVE),
        ]
        .iter()
        .map(|(what, why)| (id(what), id(why)))
        .collect();
        assert_eq!(
            note.map(|n| {
                n.refused
                    .iter()
                    .map(|(what, why)| (what.text(&en), why.clone()))
                    .collect::<Vec<_>>()
            }),
            Some(refused)
        );
        assert_eq!(
            book.borrow().calls,
            vec!["secret op", "secret weak", "add new 1"],
            "what was above the operator never got there"
        );
        // Worded where drawn: the same note reads in Korean on a Korean panel.
        let korean = crate::i18n::Strings::new("ko");
        assert_eq!(
            access
                .admin_note()
                .and_then(|n| n.done.first())
                .map(|l| l.text(&korean)),
            Some("op 인증 정보 저장됨".to_owned())
        );
        assert_eq!(book.borrow().actor, Some(Level(1)));
        Ok(())
    }

    /// The session dropped below the screen's gate between the frame that asked and the call:
    /// nothing goes, and the answer says why.
    #[cfg(feature = "settings")]
    #[test]
    fn an_apply_from_below_the_gate_changes_nothing() -> crate::Result<()> {
        use super::{admin_says, CredentialOp};
        let (mut access, book) = operator_admin()?;
        access.apply_admin(
            vec![CredentialOp::Remove {
                id: "op".to_owned(),
            }],
            false,
        );
        let en = crate::i18n::Strings::new("en");
        let note = access.admin_note().cloned();
        assert_eq!(
            note.map(|n| {
                let lines: Vec<(String, String)> = n
                    .refused
                    .iter()
                    .map(|(what, why)| (what.text(&en), why.clone()))
                    .collect();
                (n.seq, lines)
            }),
            Some((
                1,
                vec![(
                    admin_says::NOT_CHANGED.to_owned(),
                    admin_says::NOT_ALLOWED.to_owned()
                )]
            ))
        );
        assert_eq!(book.borrow().calls, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn unlock_mode_and_its_length_are_checked() {
        assert!(Access::from_config(&three("swap")).is_err());
        let mut zero = three("temporary");
        zero.temporary_secs = 0;
        assert!(
            Access::from_config(&zero).is_err(),
            "a temporary unlock of 0 s"
        );
        zero.unlock_mode = "switch".to_owned();
        assert!(Access::from_config(&zero).is_ok(), "switch has no length");
    }

    #[test]
    fn a_switch_unlock_stays_and_a_temporary_one_comes_back() -> crate::Result<()> {
        let now = Instant::now();
        let mut switch = Access::from_config(&three("switch"))?;
        assert_eq!(switch.unlock_mode(), UnlockMode::Switch);
        assert_eq!(switch.unlock(level(2), now), Level(0));
        assert!(switch.is_unlocked());
        assert_eq!(switch.tick(now + Duration::from_secs(3600), false), None);
        assert_eq!(switch.session().subject.level, Level(2));

        let mut temporary = Access::from_config(&three("temporary"))?;
        temporary.unlock(level(2), now);
        assert_eq!(temporary.tick(now + Duration::from_secs(59), true), None);
        let expired = temporary.tick(now + Duration::from_secs(60), false);
        assert_eq!(
            expired.map(|d| (d.from, d.to, d.reason)),
            Some((Level(2), Level(0), ChangeReason::ElevationExpired))
        );
        assert!(!temporary.is_unlocked());
        Ok(())
    }

    #[test]
    fn stacked_temporary_unlocks_return_to_the_first_subject() -> crate::Result<()> {
        let now = Instant::now();
        let mut access = Access::from_config(&three("temporary"))?;
        access.unlock(level(1), now);
        access.unlock(level(2), now + Duration::from_secs(30));
        let restore = access.session().elevation.as_ref().map(|e| e.restore.level);
        assert_eq!(restore, Some(Level(0)), "not the operator in between");
        // The second unlock restarted the clock.
        assert_eq!(access.tick(now + Duration::from_secs(80), true), None);
        assert!(access.tick(now + Duration::from_secs(90), false).is_some());
        Ok(())
    }

    #[test]
    fn the_session_times_out_only_away_from_its_start() -> crate::Result<()> {
        let now = Instant::now();
        let mut cfg = three("switch");
        cfg.session_timeout_secs = 10;
        let mut access = Access::from_config(&cfg)?;
        access.start_clock(now);
        assert_eq!(
            access.tick(now + Duration::from_secs(20), false),
            None,
            "nothing to drop"
        );
        access.unlock(level(1), now + Duration::from_secs(20));
        assert_eq!(access.next_deadline(), Some(now + Duration::from_secs(30)));
        // A touch puts the deadline back.
        assert_eq!(access.tick(now + Duration::from_secs(29), true), None);
        assert_eq!(access.tick(now + Duration::from_secs(38), false), None);
        let dropped = access.tick(now + Duration::from_secs(39), false);
        assert_eq!(
            dropped.map(|d| (d.to, d.reason)),
            Some((Level(0), ChangeReason::Timeout))
        );
        assert_eq!(access.next_deadline(), None);
        Ok(())
    }

    #[test]
    fn the_idle_lock_fires_once_per_quiet_stretch() -> crate::Result<()> {
        let now = Instant::now();
        let mut cfg = cfg(&["only"], None);
        cfg.idle_lock_secs = 5;
        let mut access = Access::from_config(&cfg)?;
        access.start_clock(now);
        assert!(!access.idle_lock_due(now + Duration::from_secs(4)));
        assert!(access.idle_lock_due(now + Duration::from_secs(5)));
        assert!(!access.idle_lock_due(now + Duration::from_secs(50)), "once");
        assert_eq!(access.next_deadline(), None);
        let _ = access.tick(now + Duration::from_secs(51), true);
        assert!(
            access.idle_lock_due(now + Duration::from_secs(56)),
            "again after a touch"
        );
        Ok(())
    }

    #[test]
    fn the_integrator_setting_a_subject_ends_a_temporary_unlock() -> crate::Result<()> {
        let now = Instant::now();
        let mut access = Access::from_config(&three("temporary"))?;
        access.unlock(level(2), now);
        access.set_subject(level(1));
        assert!(access.session().elevation.is_none());
        assert_eq!(access.tick(now + Duration::from_secs(3600), false), None);
        assert_eq!(access.session().subject.level, Level(1));
        Ok(())
    }

    #[test]
    fn the_shell_prompts_only_with_levels_a_mode_and_an_authenticator() -> crate::Result<()> {
        let mut config = three("temporary");
        assert!(
            !Access::from_config(&config)?.can_prompt(),
            "no authenticator"
        );
        config
            .pin_table
            .pins
            .insert("maintainer".to_owned(), "1234".to_owned());
        assert!(Access::from_config(&config)?.can_prompt());
        config.mode = "routing".to_owned();
        assert!(
            !Access::from_config(&config)?.can_prompt(),
            "routing never prompts"
        );
        let mut single = cfg(&["only"], None);
        single
            .pin_table
            .pins
            .insert("only".to_owned(), "1234".to_owned());
        assert!(
            !Access::from_config(&single)?.can_prompt(),
            "one level never prompts"
        );
        Ok(())
    }
}
