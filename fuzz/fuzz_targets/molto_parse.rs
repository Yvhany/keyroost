//! Molto2 device-response parsers — the `get info` body and the per-profile
//! public block, both fed raw device bytes (status word already stripped).
//! These are the "first-contact" byte layouts CLAUDE.md flags as soft spots.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = keyroost_proto::commands::parse_info(data);
    let _ = keyroost_proto::commands::parse_public_data(data);
});
