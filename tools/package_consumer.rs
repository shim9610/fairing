//! External-consumer fixture: only public APIs from the extracted release archive.
use fairing::bridge::{CommandSink, UpdateSource};
use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx, LaunchAction, Services};

// An application-owned adapter checks the optional transport surface without
// choosing a queue implementation for library users.
struct Slot(Option<Vec<u8>>);

impl CommandSink<Vec<u8>> for Slot {
    type Error = ();

    fn try_send(&mut self, command: Vec<u8>) -> Result<(), (Vec<u8>, ())> {
        if self.0.is_some() {
            return Err((command, ()));
        }
        self.0 = Some(command);
        Ok(())
    }
}

impl UpdateSource<Vec<u8>> for Slot {
    type Error = ();

    fn try_recv(&mut self) -> Result<Option<Vec<u8>>, ()> {
        Ok(self.0.take())
    }
}

// A downstream crate owns its permission vocabulary and element implementation.
#[derive(Clone, Copy)]
enum ConsumerPermission {
    View,
    Edit,
}
impl fairing::access::Permission for ConsumerPermission {
    fn gate(self) -> fairing::Gate {
        fairing::Gate::borrowed(match self {
            Self::View => "consumer.view",
            Self::Edit => "consumer.edit",
        })
    }
}
struct ConsumerElement(ConsumerPermission);
impl fairing::access::PermissionBound for ConsumerElement {
    type Required = ConsumerPermission;
    fn required_permission(&self) -> ConsumerPermission {
        self.0
    }
}
impl fairing::elements::Element for ConsumerElement {
    fn measure(&mut self, _: fairing::egui::Vec2, _: &mut Cx<'_>) -> fairing::egui::Vec2 {
        fairing::egui::vec2(30.0, 20.0)
    }
    fn paint(&mut self, ui: &mut fairing::egui::Ui, _: &mut Cx<'_>) {
        ui.label("Consumer element");
    }
}

// Unknown future read errors must remain refusals for downstream callers.
fn read_error_kind(error: fairing::access::SessionReadError) -> &'static str {
    use fairing::access::SessionReadError;
    match error {
        SessionReadError::Revoked => "revoked",
        SessionReadError::Expired => "expired",
        SessionReadError::Updating => "updating",
        _ => "denied",
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        read_error_kind(fairing::access::SessionReadError::Revoked),
        "revoked"
    );
    assert_eq!(
        read_error_kind(fairing::access::SessionReadError::Expired),
        "expired"
    );
    assert_eq!(
        read_error_kind(fairing::access::SessionReadError::Updating),
        "updating"
    );
    let mut protected_host = Harness::from_builder(|ctx| {
        let protected_config =
            fairing::ShellConfig::default().configure(|c| c.access.session_tokens = true);
        let mut protected = fairing::Shell::builder(protected_config).build_protected(ctx)?;
        protected.add_elements(
            "consumer-tree",
            fairing::elements::ElementTree::row(
                ConsumerPermission::View,
                4.0,
                vec![
                    fairing::elements::ElementTree::leaf(ConsumerElement(ConsumerPermission::View)),
                    fairing::elements::ElementTree::leaf(ConsumerElement(ConsumerPermission::Edit)),
                ],
            )?,
        );
        Ok(protected)
    })?;
    protected_host.frame();
    assert!(protected_host.shell.observe().has_screen("consumer-tree"));
    assert!(protected_host.shell.observe().session_token().is_none());
    let old = protected_host.shell.handle();
    protected_host.shell.restart_with(|| {
        let config = fairing::ShellConfig::default().configure(|c| c.access.session_tokens = true);
        fairing::Shell::builder(config).build_protected(&protected_host.ctx)
    })?;
    assert!(old.try_send(fairing::shell::Command::Home).is_err());
    assert!(!protected_host.shell.is_faulted());
    let grant = fairing::access::SessionGrant::new("consumer.view", true, None);
    assert_eq!(grant.gate().as_str(), "consumer.view");
    assert!(grant.is_base());
    assert_eq!(grant.temporary_until(), None);
    let _grants = [
        fairing::access::SessionGrant::base_for(ConsumerPermission::View),
        fairing::access::SessionGrant::inactive_for(ConsumerPermission::Edit),
    ];
    let mut port = Slot(None);
    let buffer = vec![1, 2, 3];
    let original = buffer.as_ptr();
    assert!(port.try_send(buffer).is_ok());
    let received = port
        .try_recv()
        .map_err(|()| "receive refused")?
        .ok_or("no update")?;
    assert_eq!(received.as_ptr(), original);

    let mut config = single_level_access();
    config.motion.reduce = true;
    let mut harness = Harness::new(config, Services::null())?;
    harness
        .shell
        .set_wallpaper(fairing::desktop::Wallpaper::Solid(
            fairing::theme::ColorRole::Surface,
        ));
    harness.shell.set_desktop_page(0);
    harness.shell.set_nav_enabled(true);
    assert!(harness.shell.set_nav_height(48.0));
    let mut metrics = harness.shell.metrics_spec();
    metrics.type_scale[1] = fairing::unit::Span::fixed(fairing::unit::Dim::du(23.0)).pinned();
    harness.shell.set_metrics_spec(metrics);
    let mut clock = harness
        .shell
        .status_item_spec("status.clock")
        .ok_or("missing clock")?;
    clock.min_width = Some(84.0);
    assert!(harness.shell.set_status_item_spec("status.clock", clock));
    harness
        .shell
        .set_wifi_backend(fairing::services::null::NullWifi);
    assert!(harness
        .shell
        .request_service(fairing::services::ServiceRequest::WifiScan)
        .is_err());
    #[cfg(feature = "full")]
    {
        harness
            .shell
            .set_osk_layout(fairing::osk::OskLayout::Qwerty);
        harness.shell.hide_osk();
    }
    harness.frames(2);
    assert_eq!(harness.shell.theme().metrics.type_scale.body, 23.0);
    assert_eq!(harness.shell.nav_bar().height, 48.0);

    harness.shell.add(
        screen("packaged", |ui: &mut fairing::egui::Ui, _cx: &mut Cx| {
            ui.label("Packaged consumer");
        })
        .title("Packaged")
        .icon(fairing::icon::GAUGE)
        .desktop(),
    );
    harness.frames(2);
    harness
        .shell
        .handle()
        .try_send(fairing::shell::Command::Launch(LaunchAction::open(
            "packaged",
        )))?;
    harness.frames(4);
    assert!(!harness.shell.is_faulted());
    assert!(harness
        .texts()
        .iter()
        .any(|(text, _)| text == "Packaged consumer"));
    println!("extracted package accepted a command and rendered the registered screen");
    Ok(())
}
