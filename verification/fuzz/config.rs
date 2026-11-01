#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if data.len() > 4096 {
        return;
    }
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(config) = fairing::ShellConfig::from_toml(text) {
            assert!(config.validate().is_ok());
        }
    }
    let mut secret = String::from("FUZZ_PRIVATE_");
    for byte in data.iter().take(64) {
        use std::fmt::Write;
        let _ = write!(secret, "{byte:02x}");
    }
    let malformed = format!("[wifi]\npsk = \"{secret}");
    let error = fairing::ShellConfig::from_toml(&malformed).unwrap_err();
    assert!(!error.to_string().contains(&secret));
});
