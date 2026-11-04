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

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
