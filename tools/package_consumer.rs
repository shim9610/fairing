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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = fairing::egui::Context::default();
    let mut protected_config = fairing::ShellConfig::default();
    protected_config.access.session_tokens = true;
    let mut protected = fairing::Shell::builder(protected_config).build_protected(&ctx)?;
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
