//! Credentials settings screens and forms.

use super::{
    form_field, tr, tr_key, ui, AdminNote, ColorRole, CredentialEntry, CredentialOp, Cx, IconRef,
    Level, ScreenDecl, Secret, SecretKind, Strings, Ui,
};

/// Users & access — the authenticator's own entries, behind the
/// `settings.credentials` gate. Registered only where the authenticator manages its entries here
/// ([`Authenticator::admin`](crate::access::Authenticator::admin)); the reference `PinTable` does
/// not — its file is the source of truth.
///
/// A screen of its own rather than [`credentials_body`] in a page, so that what is typed goes when
/// the screen does: stopped, closed, or the session changes, the form and its secrets are dropped
/// there and then. Back closes an open form before it leaves the screen.
#[must_use]
pub fn credentials() -> ScreenDecl {
    crate::screen::screen_of(CREDENTIALS, CredentialsScreen::default())
        .title(tr_key!("Users & access"))
        .icon(IconRef::Builtin("key"))
}
/// The id, and the page's scroll salt.
const CREDENTIALS: &str = "settings.credentials";
/// The body of [`credentials`]: the entries, and a form to change one or add one.
///
/// **The screen keeps no store of its own.** It draws the shell's last
/// [`CredentialAdmin::list`](crate::access::CredentialAdmin::list) and leaves every change with the
/// shell, which hands it to the authenticator — where an entry lives and how its secret is kept
/// never reach the screen. A form stays up until the authenticator has answered its Apply: what
/// went through closes it, and what did not is said on it with everything typed still there.
///
/// Put in a screen of your own, the form in progress is that screen's alone, and it is dropped the
/// first frame the body is not drawn (closed, covered, another entry on the settings home's right)
/// or is drawn for another subject — the next person never finds a half-filled form to apply.
pub fn credentials_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    let key = egui::Id::unique((CREDENTIALS, cx.pane.instance.0));
    let mut state: CredentialsState = ui.data_mut(|d| d.remove_temp(key)).unwrap_or_default();
    state.ui(ui, cx);
    ui.data_mut(|d| d.insert_temp(key, state));
}
/// [`credentials`]: the state, and the lifecycle that drops it.
#[derive(Default)]
struct CredentialsScreen(CredentialsState);
impl crate::screen::Screen for CredentialsScreen {
    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx<'_>) {
        let state = &mut self.0;
        ui::page(ui, cx, CREDENTIALS, |ui, cx| state.ui(ui, cx));
    }

    /// Back closes the form first — what was typed goes with it.
    fn on_back(&mut self, _cx: &mut Cx<'_>) -> crate::screen::BackAction {
        if self.0.form.take().is_some() {
            crate::screen::BackAction::Consumed
        } else {
            crate::screen::BackAction::Pop
        }
    }

    fn on_lifecycle(&mut self, ev: crate::screen::Lifecycle, _cx: &mut Cx<'_>) {
        use crate::screen::Lifecycle;
        if matches!(
            ev,
            Lifecycle::Stopped | Lifecycle::Destroyed | Lifecycle::AccessChanged
        ) {
            self.0 = CredentialsState::default();
        }
    }
}
/// `settings.credentials` between frames.
#[derive(Clone, Default)]
struct CredentialsState {
    form: Option<CredentialForm>,
    /// The frame it was last drawn on.
    drawn: Option<u64>,
    /// Whom it was drawn for.
    subject: Option<crate::access::Subject>,
    /// The answer already there when the screen came into view. Only a newer one is shown — an
    /// "Added operator-2" from last week is not news.
    since: u64,
}
impl CredentialsState {
    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx<'_>) {
        let frame = cx.frame();
        let shown = self
            .drawn
            .is_some_and(|last| frame.saturating_sub(last) <= 1)
            && self.subject.as_ref() == Some(&cx.session.subject);
        if !shown {
            // Into view: nothing typed before carries over, and the entries are listed again —
            // on the settings home's right the screen is drawn without ever being opened.
            *self = Self {
                subject: Some(cx.session.subject.clone()),
                since: cx.admin_note().map_or(0, |n| n.seq),
                ..Self::default()
            };
            cx.list_credentials();
            ui.ctx().request_repaint();
        }
        self.drawn = Some(frame);
        credential_rows(ui, cx, &mut self.form, self.since);
    }
}
/// A change in progress on `settings.credentials`. The secrets are what is being typed or drawn,
/// and they go with the form when it closes.
#[derive(Clone, PartialEq, Eq)]
struct CredentialForm {
    /// The entry being changed; `None` adds one.
    editing: Option<CredentialEntry>,
    /// A new entry's id, as typed.
    id: String,
    /// The level chosen, as an index into the level table.
    level: usize,
    /// The kind of secret being set, as an index into the authenticator's kinds.
    kind: usize,
    secret: String,
    confirm: String,
    /// The pattern pad's path while a finger draws it.
    pattern: Vec<u8>,
    /// The first drawing of a new pattern, waiting for the second.
    first: Option<Vec<u8>>,
    /// A pattern drawn twice the same — what Apply sets.
    drawn: Option<Vec<u8>>,
    /// Why the last Apply did not go through, as the form found it.
    error: Option<String>,
    /// Applied, and waiting for the answer: the last answer there was when it went.
    sent: Option<u64>,
    /// What the authenticator answered to an Apply that did not all go through.
    answer: Option<AdminNote>,
}
impl std::fmt::Debug for CredentialForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialForm")
            .field("editing", &self.editing.as_ref().map(|e| &e.id))
            .field("level", &self.level)
            .finish_non_exhaustive()
    }
}
impl CredentialForm {
    fn edit(entry: &CredentialEntry) -> Self {
        Self {
            editing: Some(entry.clone()),
            id: entry.id.clone(),
            level: usize::from(entry.level.0),
            ..Self::add(0)
        }
    }

    /// A new entry starts one level up from the bottom — the bottom needs no credential.
    fn add(levels: usize) -> Self {
        Self {
            editing: None,
            id: String::new(),
            level: usize::from(levels > 1),
            kind: 0,
            secret: String::new(),
            confirm: String::new(),
            pattern: Vec::new(),
            first: None,
            drawn: None,
            error: None,
            sent: None,
            answer: None,
        }
    }

    /// Forget the secret typed or drawn so far — another kind was picked.
    fn clear_secret(&mut self) {
        self.secret.clear();
        self.confirm.clear();
        self.pattern.clear();
        self.first = None;
        self.drawn = None;
        self.error = None;
        self.answer = None;
    }

    /// Hand the calls to the shell and wait for the answer.
    fn send(&mut self, ui: &Ui, cx: &mut Cx<'_>, ops: Vec<CredentialOp>) {
        self.sent = Some(cx.admin_note().map_or(0, |n| n.seq));
        self.error = None;
        self.answer = None;
        cx.credential_ops(ops);
        // The answer is there next frame; nothing else may ask for one.
        ui.ctx().request_repaint();
    }

    /// The answer to the form's own Apply, once it is there: `true` when it all went through.
    /// What did not stays on the form with everything typed, and what did is the starting point
    /// for the next Apply — the entry as listed again.
    fn take_answer(&mut self, cx: &Cx<'_>) -> bool {
        let Some(sent) = self.sent else {
            return false;
        };
        let Some(note) = cx.admin_note().filter(|n| n.seq > sent) else {
            return false;
        };
        self.sent = None;
        if note.refused.is_empty() {
            return true;
        }
        self.answer = Some(note.clone());
        if let Some(id) = self.editing.as_ref().map(|e| e.id.clone()) {
            if let Some(listed) = cx.credentials().iter().find(|e| e.id == id) {
                self.editing = Some(listed.clone());
            }
        }
        false
    }
}
/// A kind of secret as a sentence says it: "PIN", "pattern", "password".
/// The form's words for one kind of secret — whole sentences, so a language never has to fit a
/// noun into a sentence shaped for English (a Korean particle follows the noun's last sound).
struct SecretWords {
    /// The heading over a changed entry's secret.
    new: &'static str,
    /// How to leave the secret as it is.
    keep: &'static str,
    /// Why a new entry cannot go without one.
    needed: &'static str,
}
fn secret_words(kind: SecretKind) -> SecretWords {
    match kind {
        SecretKind::Pin { .. } => SecretWords {
            new: tr_key!("New PIN"),
            keep: tr_key!("Leave both empty to keep the current PIN."),
            needed: tr_key!("A new entry needs a PIN."),
        },
        SecretKind::Pattern { .. } => SecretWords {
            new: tr_key!("New pattern"),
            keep: tr_key!("Draw nothing to keep the current pattern."),
            needed: tr_key!("A new entry needs a pattern."),
        },
        SecretKind::Password { .. } => SecretWords {
            new: tr_key!("New password"),
            keep: tr_key!("Leave both empty to keep the current password."),
            needed: tr_key!("A new entry needs a password."),
        },
    }
}
/// The level table's labels, in order.
fn level_labels(cx: &Cx<'_>) -> Vec<String> {
    cx.levels().defs().iter().map(|d| d.label.clone()).collect()
}
fn credential_rows(ui: &mut Ui, cx: &mut Cx<'_>, form: &mut Option<CredentialForm>, since: u64) {
    let s = cx.strings;
    if let Some(open) = form.as_mut() {
        if credential_form(ui, cx, open) {
            *form = None;
            // The list comes up next frame rather than under the form's last one.
            ui.ctx().request_repaint();
        }
        return;
    }
    if let Some(note) = cx.admin_note().filter(|n| n.seq > since).cloned() {
        note_cards(ui, cx, &note);
    }
    let entries = cx.credentials().to_vec();
    let levels = level_labels(cx);
    ui::section(ui, cx, tr!(s, "Entries"));
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
        if entries.is_empty() {
            ui::info_row(ui, cx, tr!(s, "No entries"), "");
        }
        for entry in &entries {
            let level = levels
                .get(usize::from(entry.level.0))
                .map_or("", |label| s.get(label));
            let trailing = if entry.disabled {
                tr!(s, "{level} · off").replace("{level}", level)
            } else {
                level.to_owned()
            };
            if !cx.may_grant(entry.level) {
                // Above the session's own level: listed, not changed.
                ui::info_row(ui, cx, &entry.label, &trailing);
            } else if ui::nav_row(ui, cx, &entry.label, Some(&trailing)).clicked() {
                *form = Some(CredentialForm::edit(entry));
            }
        }
    });
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
        if ui::icon_row(
            ui,
            cx,
            IconRef::Builtin("plus"),
            tr!(s, "Add an entry"),
            None,
            None,
        )
        .clicked()
        {
            *form = Some(CredentialForm::add(grantable(cx, levels.len())));
        }
    });
}
/// What an Apply came to, a card a line: what did not go through, then what did.
fn note_cards(ui: &mut Ui, cx: &mut Cx<'_>, note: &AdminNote) {
    let s = cx.strings;
    for (what, why) in &note.refused {
        // The reason is the shell's key or the authenticator's own words — looked up either way.
        ui::status_card(
            ui,
            cx,
            &what.text(s),
            Some(s.get(why)),
            Some(ColorRole::Danger),
        );
    }
    for done in &note.done {
        ui::status_card(ui, cx, &done.text(s), None, Some(ColorRole::Success));
    }
}
/// How many levels from the bottom the session may give: up to its own.
fn grantable(cx: &Cx<'_>, levels: usize) -> usize {
    (0..levels)
        .take_while(|&i| u16::try_from(i).is_ok_and(|i| cx.may_grant(Level(i))))
        .count()
}
/// The form. `true` when it is finished — applied, removed, or cancelled.
fn credential_form(ui: &mut Ui, cx: &mut Cx<'_>, form: &mut CredentialForm) -> bool {
    if form.take_answer(cx) {
        return true;
    }
    let kinds = cx.secret_kinds().to_vec();
    let kind = kinds
        .get(form.kind)
        .copied()
        .unwrap_or(SecretKind::Password {
            keys: crate::widgets::Keys::Latin,
        });
    let s = cx.strings;
    let mut levels = level_labels(cx);
    // Only what the session may give is offered.
    levels.truncate(grantable(cx, levels.len()).max(1));
    if let Some(entry) = &form.editing {
        ui::title(ui, cx, &entry.label);
    } else {
        ui::title(ui, cx, tr!(s, "New entry"));
        form_field(ui, cx, tr!(s, "Id"), &mut form.id, "operator-2", "cred.id");
    }
    ui::section(ui, cx, tr!(s, "Level"));
    let options: Vec<(&str, Option<&str>)> = levels.iter().map(|l| (s.get(l), None)).collect();
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
        if let Some(picked) = ui::choice_rows(ui, cx, &options, form.level) {
            form.level = picked;
        }
    });
    let words = secret_words(kind);
    let heading = match (form.editing.is_some(), kinds.len() > 1) {
        (true, true) => tr!(s, "New secret"),
        (false, true) => tr!(s, "Secret"),
        (true, false) => s.get(words.new),
        (false, false) => s.get(kind.name()),
    };
    ui::section(ui, cx, heading);
    if kinds.len() > 1 {
        // The authenticator takes more than one kind: the form sets whichever is picked.
        let labels: Vec<&str> = kinds.iter().map(|k| s.get(k.name())).collect();
        let pick =
            crate::widgets::SegmentedControl::new(&labels, form.kind).show(ui, &mut cx.widgets());
        if let Some(picked) = pick.picked {
            form.kind = picked;
            form.clear_secret();
        }
    }
    match kind {
        SecretKind::Pattern { grid, min_points } => {
            pattern_entry(ui, cx, form, grid, min_points);
        }
        // A PIN goes in on the keypad's digits, so it is set on the digits too — whatever layout
        // this app's keyboard runs.
        SecretKind::Pin { len: 0, max_len } => {
            let hint = tr!(s, "Up to {n} digits").replace("{n}", &max_len.to_string());
            secret_fields(ui, cx, form, &hint, crate::widgets::Keys::Digits);
        }
        SecretKind::Pin { len, .. } => {
            let hint = tr!(s, "{n} digits").replace("{n}", &len.to_string());
            secret_fields(ui, cx, form, &hint, crate::widgets::Keys::Digits);
        }
        // A password on the keys the prompt asks for.
        SecretKind::Password { keys } => secret_fields(ui, cx, form, "", keys),
    }
    if let Some(entry) = form.editing.clone() {
        ui::note(ui, cx, s.get(words.keep));
        // Removing is the one change that cannot be taken back, so it is held, not tapped.
        let remove = crate::widgets::BigButton::new(tr!(s, "Remove"))
            .kind(crate::widgets::ButtonKind::Danger)
            .long_press(cx.theme.motion.long_press)
            .show(ui, &mut cx.widgets());
        if remove.completed && form.sent.is_none() {
            form.send(ui, cx, vec![CredentialOp::Remove { id: entry.id }]);
        }
    }
    credential_foot(ui, cx, form, kind)
}
/// The foot of the form: what the last Apply came to, and Apply and Cancel. `true` when the form
/// is finished.
fn credential_foot(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    form: &mut CredentialForm,
    kind: SecretKind,
) -> bool {
    let s = cx.strings;
    if let Some(error) = &form.error {
        ui::status_card(
            ui,
            cx,
            tr!(s, "Not saved"),
            Some(error),
            Some(ColorRole::Danger),
        );
    }
    if let Some(answer) = form.answer.clone() {
        note_cards(ui, cx, &answer);
    }
    if form.sent.is_some() {
        ui::note(ui, cx, tr!(s, "Saving…"));
    }
    let (mut apply, mut cancel) = (false, false);
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
        apply = ui::icon_row(
            ui,
            cx,
            IconRef::Builtin("check"),
            tr!(s, "Apply"),
            None,
            None,
        )
        .clicked();
        cancel = ui::icon_row(
            ui,
            cx,
            IconRef::Builtin("close"),
            tr!(s, "Cancel"),
            None,
            None,
        )
        .clicked();
    });
    if cancel {
        return true;
    }
    if apply && form.sent.is_none() {
        match credential_ops(form, kind, cx.credentials(), s) {
            // Nothing changed: there is nothing to wait for.
            Ok(ops) if ops.is_empty() => return true,
            Ok(ops) => form.send(ui, cx, ops),
            Err(error) => {
                form.error = Some(error);
                form.answer = None;
            }
        }
    }
    false
}
/// The secret and its repeat, typed.
fn secret_fields(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    form: &mut CredentialForm,
    hint: &str,
    keys: crate::widgets::Keys,
) {
    let again = tr!(cx.strings, "Again");
    for (text, hint, salt) in [
        (&mut form.secret, hint, "cred.secret"),
        (&mut form.confirm, again, "cred.confirm"),
    ] {
        crate::widgets::TextField::new(text)
            .password(true)
            .keys(keys)
            .hint(hint)
            .id_salt(salt)
            .show(ui, &mut cx.widgets());
    }
}
/// A new pattern, drawn twice: the first drawing waits for the second, the two must match, and a
/// stroke after a match starts over.
fn pattern_entry(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    form: &mut CredentialForm,
    grid: u8,
    min_points: u8,
) {
    let s = cx.strings;
    let step = match (&form.drawn, &form.first) {
        (Some(_), _) => tr!(
            s,
            "Drawn twice the same - Apply saves it. A new stroke starts over."
        ),
        (None, Some(_)) => tr!(s, "Draw it again to confirm."),
        (None, None) => tr!(s, "Draw the new pattern."),
    };
    ui::note(ui, cx, step);
    let target = crate::theme::control_height(&cx.theme.metrics, &cx.theme.control);
    let side = ui.available_width().min(target * 5.0);
    let pad = ui
        .allocate_ui_with_layout(
            egui::Vec2::splat(side),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                crate::widgets::PatternPad::new(&mut form.pattern)
                    .grid(grid)
                    .min_points(min_points)
                    .show(ui, &mut cx.widgets())
            },
        )
        .inner;
    if pad.changed && !form.pattern.is_empty() {
        form.error = None;
        if form.drawn.take().is_some() {
            form.first = None;
        }
    }
    if pad.too_short {
        form.pattern.clear();
        form.error = Some(
            s.get(crate::access::prompt::labels::MIN_DOTS)
                .replace("{n}", &min_points.to_string()),
        );
    }
    if pad.submitted {
        let path = std::mem::take(&mut form.pattern);
        pattern_submitted(form, path, s);
    }
}
/// A finished drawing on the form: the first waits for the second, a match is what Apply sets,
/// and a mismatch starts over.
fn pattern_submitted(form: &mut CredentialForm, path: Vec<u8>, strings: &Strings) {
    match form.first.take() {
        None => form.first = Some(path),
        Some(first) if first == path => form.drawn = Some(path),
        Some(_) => {
            form.error =
                Some(tr!(strings, "The two patterns do not match - draw it again.").to_owned());
        }
    }
}
/// What Apply asks for, checked first: the two secrets match, a PIN is digits and no longer than
/// the authenticator takes, a pattern was drawn twice the same, a new entry has an id nobody has
/// and a secret. The authenticator still has the last word on all of it.
fn credential_ops(
    form: &CredentialForm,
    kind: SecretKind,
    existing: &[CredentialEntry],
    strings: &Strings,
) -> Result<Vec<CredentialOp>, String> {
    let level = Level(u16::try_from(form.level).unwrap_or(0));
    let typed = form.secret.as_str();
    let secret = match kind {
        SecretKind::Pattern { .. } => {
            if form.first.is_some() && form.drawn.is_none() {
                return Err(tr!(strings, "Draw the pattern again to confirm it.").to_owned());
            }
            form.drawn.clone().map(Secret::Pattern)
        }
        SecretKind::Pin { len, max_len } => {
            if typed != form.confirm {
                return Err(tr!(strings, "The two PINs do not match.").to_owned());
            }
            let digits = typed.bytes().all(|b| b.is_ascii_digit());
            if !typed.is_empty() {
                // A fixed length is the only one the prompt can take: it submits on the last digit.
                if len > 0 && (typed.len() != usize::from(len) || !digits) {
                    return Err(
                        tr!(strings, "A PIN is {n} digits.").replace("{n}", &len.to_string())
                    );
                }
                if typed.len() > usize::from(max_len) || !digits {
                    return Err(tr!(strings, "A PIN is 1 to {n} digits.")
                        .replace("{n}", &max_len.to_string()));
                }
            }
            (!typed.is_empty()).then(|| Secret::Pin(typed.to_owned()))
        }
        SecretKind::Password { .. } => {
            if typed != form.confirm {
                return Err(tr!(strings, "The two passwords do not match.").to_owned());
            }
            (!typed.is_empty()).then(|| Secret::Password(typed.to_owned()))
        }
    };
    let Some(entry) = &form.editing else {
        let id = form.id.trim();
        if id.is_empty() {
            return Err(tr!(strings, "A new entry needs an id.").to_owned());
        }
        if existing.iter().any(|e| e.id == id) {
            return Err(tr!(strings, "`{id}` is taken.").replace("{id}", id));
        }
        let Some(secret) = secret else {
            return Err(strings.get(secret_words(kind).needed).to_owned());
        };
        return Ok(vec![CredentialOp::Add {
            id: id.to_owned(),
            level,
            secret,
        }]);
    };
    let mut ops = Vec::new();
    if level != entry.level {
        ops.push(CredentialOp::SetLevel {
            id: entry.id.clone(),
            level,
        });
    }
    if let Some(secret) = secret {
        ops.push(CredentialOp::SetSecret {
            id: entry.id.clone(),
            secret,
        });
    }
    Ok(ops)
}

#[cfg(test)]
mod tests {
    use super::super::en;
    use super::*;
    const PIN: SecretKind = SecretKind::Pin {
        len: 0,
        max_len: 16,
    };
    const LATIN_PASSWORD: SecretKind = SecretKind::Password {
        keys: crate::widgets::Keys::Latin,
    };
    const PATTERN: SecretKind = SecretKind::Pattern {
        grid: 3,
        min_points: 4,
    };
    fn entry(id: &str, level: u16) -> CredentialEntry {
        CredentialEntry {
            id: id.to_owned(),
            label: id.to_owned(),
            level: Level(level),
            disabled: false,
        }
    }
    /// The form checks what it can before the authenticator sees anything.
    #[test]
    fn the_credential_form_checks_before_it_asks() {
        let existing = [entry("ada", 1)];
        let mut add = CredentialForm::add(3);
        add.id = "bob".to_owned();
        add.secret = "4321".to_owned();
        add.confirm = "4320".to_owned();
        assert!(
            credential_ops(&add, PIN, &existing, &en()).is_err(),
            "a mismatch"
        );
        add.confirm = "4321".to_owned();
        let ops = credential_ops(&add, PIN, &existing, &en());
        assert!(matches!(
            ops.as_deref(),
            Ok([CredentialOp::Add { id, level: Level(1), .. }]) if id == "bob"
        ));
        add.secret = "43a1".to_owned();
        add.confirm = "43a1".to_owned();
        assert!(
            credential_ops(&add, PIN, &existing, &en()).is_err(),
            "a letter in a PIN"
        );
        assert!(
            credential_ops(&add, LATIN_PASSWORD, &existing, &en()).is_ok(),
            "but fine in a password"
        );
        add.id = "ada".to_owned();
        assert!(
            credential_ops(&add, LATIN_PASSWORD, &existing, &en()).is_err(),
            "a taken id"
        );
    }
    /// Editing asks only for what changed — a level, a secret, both, or nothing.
    #[test]
    fn an_edit_asks_only_for_what_changed() {
        let ada = entry("ada", 1);
        let mut edit = CredentialForm::edit(&ada);
        assert_eq!(credential_ops(&edit, PIN, &[], &en()), Ok(Vec::new()));
        edit.level = 2;
        edit.secret = "9999".to_owned();
        edit.confirm = "9999".to_owned();
        let ops = credential_ops(&edit, PIN, &[], &en()).unwrap_or_default();
        assert!(matches!(
            ops.as_slice(),
            [
                CredentialOp::SetLevel {
                    level: Level(2),
                    ..
                },
                CredentialOp::SetSecret { .. }
            ]
        ));
        assert!(
            !format!("{ops:?}").contains("9999"),
            "no secret in a debug line"
        );
    }
    /// A PIN no longer than the authenticator takes, and a pattern drawn twice the same.
    #[test]
    fn the_form_follows_the_kind_of_secret() {
        let mut add = CredentialForm::add(3);
        add.id = "bob".to_owned();
        add.secret = "1234567".to_owned();
        add.confirm = "1234567".to_owned();
        let six = SecretKind::Pin { len: 0, max_len: 6 };
        assert!(
            credential_ops(&add, six, &[], &en()).is_err(),
            "seven over six"
        );
        add.secret.pop();
        add.confirm.pop();
        assert!(credential_ops(&add, six, &[], &en()).is_ok());
        // Where the prompt takes exactly four, six digits could never be entered there.
        let four = SecretKind::Pin { len: 4, max_len: 4 };
        assert!(
            credential_ops(&add, four, &[], &en()).is_err(),
            "six where four"
        );
        add.secret.truncate(4);
        add.confirm.truncate(4);
        assert!(credential_ops(&add, four, &[], &en()).is_ok());
        add.secret.truncate(3);
        add.confirm.truncate(3);
        assert!(
            credential_ops(&add, four, &[], &en()).is_err(),
            "three where four"
        );

        let mut add = CredentialForm::add(3);
        add.id = "bob".to_owned();
        assert!(
            credential_ops(&add, PATTERN, &[], &en()).is_err(),
            "nothing drawn"
        );
        add.first = Some(vec![0, 1, 2, 5]);
        assert!(
            credential_ops(&add, PATTERN, &[], &en()).is_err(),
            "drawn once, not confirmed"
        );
        add.first = None;
        add.drawn = Some(vec![0, 1, 2, 5]);
        let ops = credential_ops(&add, PATTERN, &[], &en()).unwrap_or_default();
        assert!(matches!(
            ops.as_slice(),
            [CredentialOp::Add { secret: Secret::Pattern(dots), .. }] if dots == &[0, 1, 2, 5]
        ));
        // Drawn once, then again: a match is set, a mismatch starts over.
        let mut form = CredentialForm::add(3);
        pattern_submitted(&mut form, vec![0, 1, 2, 5], &en());
        assert_eq!(form.first.as_deref(), Some(&[0, 1, 2, 5][..]));
        pattern_submitted(&mut form, vec![0, 1, 2, 4], &en());
        assert!(form.first.is_none() && form.drawn.is_none() && form.error.is_some());
        pattern_submitted(&mut form, vec![0, 1, 2, 5], &en());
        pattern_submitted(&mut form, vec![0, 1, 2, 5], &en());
        assert_eq!(form.drawn.as_deref(), Some(&[0, 1, 2, 5][..]));
        // Editing with nothing drawn keeps the pattern and asks for nothing.
        let mut edit = CredentialForm::edit(&entry("ada", 1));
        edit.kind = 1;
        assert_eq!(credential_ops(&edit, PATTERN, &[], &en()), Ok(Vec::new()));
    }
}
