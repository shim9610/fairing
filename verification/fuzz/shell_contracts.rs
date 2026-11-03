#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    fairing_assurance::transitions::contract(
        data,
        usize::from(data.first().copied().unwrap_or(0) % 3 + 1),
        usize::from(data.get(1).copied().unwrap_or(0) % 3 + 1),
    );
    fairing_assurance::api::shell_contract(data);
    fairing_assurance::api::backend_contract(data);
    if data.len() >= 5 {
        fairing_assurance::api::grant_contract(
            u16::from(data[0]),
            u16::from_le_bytes([data[1], data[2]]),
            data[3] & 1 != 0,
            data[4] & 1 != 0,
        );
        fairing_assurance::api::cancellation_contract(data[0]);
    }
});
