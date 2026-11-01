#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    fairing_assurance::api::memory_contract(data);
    fairing_assurance::api::access_contract(data);
    fairing_assurance::api::authentication_contract(data);
    fairing_assurance::api::notification_contract(data);
    if data.len() >= 12 {
        let utc = u64::from_le_bytes(data[..8].try_into().unwrap());
        let offset = i32::from_le_bytes(data[8..12].try_into().unwrap());
        fairing_assurance::api::time_contract(utc, offset);
        fairing_assurance::api::units_contract(offset as u32, utc as u16, (utc >> 16) as u16);
        fairing_assurance::api::configuration_contract(offset as u32, data[0]);
    }
});
