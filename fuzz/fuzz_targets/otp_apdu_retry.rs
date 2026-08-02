#![no_main]
//! Fuzz the ISO 7816-4 `6C xx` ("wrong Le") retry classifier.
//!
//! The audit noted the fuzz harness was parser-only; this target covers the
//! first piece of *command-construction* logic on the retry path.
//! `resend_with_le` rebuilds the APDU the host must reissue after a `6C xx`
//! status word, classifying the original as ISO 7816-4 case 1/2/3/4 by
//! structure. Getting the classification wrong either overwrites the last
//! *data* byte (case 3 mistaken for case 4 — seed ciphertext corruption) or
//! produces a malformed `… Le_old Le_new` tail (case 2 mistaken for case 1).
//!
//! Properties under fuzz, for ANY byte buffer split into (apdu, le):
//! never panics; the result ends in the requested Le; every byte before that
//! Le is the original APDU prefix untouched; and a structurally valid
//! short-form input always yields a structurally valid case-2/4 APDU.
use libfuzzer_sys::fuzz_target;

use keyroost_proto::apdu::resend_with_le;

fuzz_target!(|data: &[u8]| {
    // Last input byte is the device's demanded Le; the rest is the APDU.
    let Some((&le, apdu)) = data.split_last() else {
        return;
    };
    let out = resend_with_le(apdu, le);

    // The rebuilt APDU ends in the requested Le, via either replacing the
    // original trailing Le or appending one — never anything else.
    assert_eq!(out.last(), Some(&le));
    assert!(out.len() == apdu.len() || out.len() == apdu.len() + 1);

    // Everything before the trailing Le is the original, byte for byte:
    // data (and header) must never be clobbered.
    assert_eq!(&out[..out.len() - 1], &apdu[..out.len() - 1]);

    // A structurally valid short-form input (case 1/2/3/4) must come back as
    // a structurally valid case-2 (header + Le) or case-4 (header + Lc +
    // data + Le) APDU.
    let input_is_short_form = match apdu.len() {
        4 | 5 => true,
        n if n > 5 => {
            let lc = apdu[4] as usize;
            n == 5 + lc || n == 5 + lc + 1
        }
        _ => false,
    };
    if input_is_short_form {
        let is_case_2 = out.len() == 5;
        let is_case_4 = out.len() > 5 && out.len() == 5 + out[4] as usize + 1;
        assert!(
            is_case_2 || is_case_4,
            "valid short-form input produced a malformed retry APDU: {out:02x?}"
        );
    }
});
