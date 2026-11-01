// A concrete entry point for inspecting the validation boundary of real library APIs.
// No item is marked prevalidated: that annotation requires its own evidence.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = fairing::ShellConfig::default();
    config.validate()?;
    if fairing_widgets::unit::round_u8(8.5) != 9 {
        return Err("unexpected unit conversion result".into());
    }
    Ok(())
}
