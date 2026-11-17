//! Protected runner, issuer-owned sessions, app state and typed settings.
//! Development demonstration: typing the public marker DEMO is deliberately not credential verification.
mod common;
#[path = "protected/tour.rs"]
mod tour;

use fairing::{
    access::{
        AuthMethod, AuthOutcome, AuthRequestId, Authenticator, Credential, Permission, Protected,
        SessionController, SessionGrant, SessionId, Subject,
    },
    services::Waker,
    Gate, LaunchAction, Shell, ShellConfig,
};
use std::time::Instant;

#[derive(Clone, Copy)]
enum Access {
    View,
    Operate,
}
impl Permission for Access {
    fn gate(self) -> Gate {
        Gate::borrowed(match self {
            Self::View => "demo.view",
            Self::Operate => "demo.operate",
        })
    }
}

struct DemoIssuer {
    controller: Option<SessionController>,
    waker: Waker,
}
impl Authenticator for DemoIssuer {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::External {
            label: "Development only: type DEMO and press Enter".into(),
        }]
    }
    fn submit(&mut self, _: AuthRequestId, credential: Credential, now: Instant) -> AuthOutcome {
        if !matches!(credential, Credential::External(bytes) if bytes == b"DEMO") {
            return AuthOutcome::Denied {
                message: "Type the public demonstration marker DEMO".into(),
            };
        }
        match SessionController::issue(
            Subject::default(),
            vec![
                SessionGrant::base_for(Access::View),
                SessionGrant::base_for(Access::Operate),
            ],
            None,
            now,
            self.waker.clone(),
        ) {
            Ok((controller, token)) => {
                self.controller = Some(controller);
                AuthOutcome::Session(token)
            }
            Err(_) => AuthOutcome::Denied {
                message: "Session issuance failed".into(),
            },
        }
    }
    fn session_ended(&mut self, session: SessionId, _: Instant) {
        if self
            .controller
            .as_ref()
            .is_some_and(|owner| owner.id() == session)
        {
            self.controller = None; // Dropping the issuer capability revokes every reader.
        }
    }
}

fn build(ctx: &egui::Context) -> fairing::Result<(fairing::ProtectedShell, usize)> {
    let config = ShellConfig::default().configure(|c| c.access.session_tokens = true);
    let mut shell = Shell::builder(config)
        .authenticator(DemoIssuer {
            controller: None,
            waker: Waker::new(ctx),
        })
        .build_protected(ctx)?;
    shell.add(Protected::new(
        Access::View,
        fairing::screen("dashboard", |ui, cx| {
            ui.heading("Protected application");
            ui.label("This example uses a public marker, not production authentication.");
            if let Some(count) = cx.app_mut::<usize>() {
                ui.label(format!("Operations: {count}"));
            }
            if ui.button("Operate").clicked() {
                cx.shell.launch(LaunchAction::run("operate"));
            }
            if ui.button("Read-only settings").clicked() {
                cx.open("settings.home");
            }
            if ui.button("Log out and request a new session").clicked() {
                cx.shell.logout();
                cx.shell
                    .request_unlock(Access::View.gate(), Some(LaunchAction::open("dashboard")));
            }
        })
        .title("Protected application")
        .icon(fairing::icon::GAUGE)
        .desktop(),
    ));
    shell.add(Protected::new(
        Access::Operate,
        fairing::action("operate", |cx| {
            if let Some(count) = cx.app_mut::<usize>() {
                *count += 1;
            }
        }),
    ));
    // Screen-view grants do not issue display/network/credential write grants.
    fairing::settings::add_all_protected(
        &mut shell,
        &fairing::settings::SettingsConfig::default(),
        |_| Access::View,
    );
    shell
        .handle()
        .request_unlock(Access::View.gate(), Some(LaunchAction::open("dashboard")));
    Ok((shell, 0usize))
}

fn main() -> fairing::Result<()> {
    common::init_logger();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = fairing::runner::Options::default();
    options.size = common::arg_size(&args);
    match common::arg_tour(&args) {
        Some(dir) => tour::run(options, dir),
        None => fairing::runner::run_app(options, build),
    }
}
