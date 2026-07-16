#![no_main]
//! Fuzz the Windows HID interface-detail path parser (KEY-020 surface).
//!
//! `parse_detail_path` reads a UTF-16LE device path out of the raw bytes of an
//! `SP_DEVICE_INTERFACE_DETAIL_DATA_W` buffer returned by SetupAPI. The audit
//! flagged the original scan as relying on alignment/extent guarantees the
//! byte buffer does not provide; Task 28 rewrote it as this extent-bounded,
//! alignment-free reader. Property under fuzz: for ANY byte buffer — short,
//! unterminated, odd-length, or hostile — the parser returns a String and
//! never panics or reads past the slice.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = keyroost_winwebauthn::parse_detail_path(data);
});
