//! **A fact written in two places is checked against itself** — the settings screen ids live
//! in `settings::screens::ALL`, and the lists that repeat them (the home screen's entries, the
//! registrations) have to agree with it; the status bar's built-in ids go through `builtin_id`
//! and `from_id` and back. A list that drifts fails here rather than in a screen nobody opens.
#![cfg(all(feature = "settings", feature = "mock"))]

use fairing::settings::{add_all, screens, SettingsConfig};
use fairing::testing::{single_level_access, Harness};
use fairing::Shell;

/// **`add_all` registers every id in `ALL`, and nothing under `settings.` besides** — with the
/// capability and administrator conditions waived (`SettingsConfig::ignoring_capabilities`), so the set it
/// *can* produce is the set `ALL` says it can.
#[test]
fn add_all_registers_exactly_the_listed_screens() -> fairing::Result<()> {
    let h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access())
            .services(fairing::services::mock::services())
            .build(ctx)?;
        add_all(
            &mut shell,
            &SettingsConfig::default().ignoring_capabilities(),
        );
        Ok(shell)
    })?;
    let registry = h.shell.registry();
    for id in screens::ALL {
        assert!(
            registry.has_screen(id),
            "{id} is in ALL but was not registered"
        );
    }
    let registered: Vec<&str> = registry
        .screens()
        .iter()
        .map(fairing::ScreenDecl::id)
        .filter(|id| id.starts_with("settings."))
        .collect();
    for id in &registered {
        assert!(
            screens::ALL.contains(id),
            "{id} was registered but is not in ALL"
        );
    }
    Ok(())
}

/// **The home screen's entries lead to screens in `ALL`.**
#[test]
fn the_home_entries_lead_to_listed_screens() {
    for entry in screens::entries() {
        assert!(
            screens::ALL.contains(&entry.id.as_str()),
            "entry {} leads nowhere in ALL",
            entry.id
        );
    }
}

/// **Every built-in status item id round-trips** through `from_id` and `builtin_id`.
#[test]
fn the_status_ids_round_trip() {
    use fairing::chrome::{StatusItem, BUILTIN_IDS};
    for id in BUILTIN_IDS {
        let item = StatusItem::from_id(id, fairing::time::ClockFormat::default());
        assert!(
            item.is_some(),
            "{id} is listed but from_id does not know it"
        );
        if let Some(item) = item {
            assert_eq!(item.builtin_id(), *id, "{id} comes back as another id");
        }
    }
}
