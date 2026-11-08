#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    fairing_assurance::exercise_protocol(data);
});
