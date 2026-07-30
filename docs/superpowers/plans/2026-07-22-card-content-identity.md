# Card-content device identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Derive a smart-card device's vendor and full serial from the card's own content (OpenPGP manufacturer ID + a self-validating vendor serial read) instead of the reader name, fixing #83 and the wrong-vendor class bug, and unify the probe's APDU reads onto the status-word-complete exchange from #84.

**Architecture:** A pure OpenPGP manufacturer-ID registry in keyroost-openpgp (the standard GnuPG table — general across vendors). A `exchange_apdu` helper in keyroost-transport that handles `61xx` and `6Cxx` via the existing `classify_sw`/`resend_with_le`, replacing the probe's half-complete inline reads. The probe parses the OpenPGP manufacturer ID and reads the Token2 full serial gated on card content (`has_fido`), not the reader name. `correlate()` stamps the registry vendor name + full serial into the device model.

**Tech Stack:** Rust workspace — keyroost-openpgp (pure byte layer), keyroost-transport (PC/SC probe), keyroost-resolve (device model). No new dependencies.

## Global Constraints

- Commit UNSIGNED: `git commit --no-gpg-sign`, footer `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`. Never push main, never create tags. Guard-tripping messages → `git commit -F <scratch-file>`. Stage files EXPLICITLY by name (`git add <path>`) — never `git add -A`/`.` (untracked plan docs live in the tree).
- Gates before every commit: `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo fmt --all --check`, `cargo test --workspace --offline`. MSRV 1.85.
- No new dependencies. Read-only, no auth.
- OpenPGP AID layout (verbatim): `D2 76 00 01 24 01`(6-byte prefix) · `version`(2) · `manufacturer`(2 = bytes 8..10) · `serial`(4 = bytes 10..14) · `00 00`. Manufacturer ID is `u16` big-endian from bytes 8..10.
- Manufacturer registry (verbatim from GnuPG `scd/app-openpgp.c`, so keyroost matches `gpg --card-status`): `0x0001` PPC Card Systems, `0x0002` Prism, `0x0003` OpenFortress, `0x0004` Wewid, `0x0005` ZeitControl, `0x0006` Yubico, `0x0007` OpenKMS, `0x0008` LogoEmail, `0x0009` Fidesmo, `0x000A` VivoKey, `0x000B` Feitian Technologies, `0x000D` Dangerous Things, `0x000E` Excelsecu, `0x000F` Nitrokey, `0x0010` NeoPGP, `0x0011` Token2, `0x002A` Magrathea, `0x0042` GnuPG e.V., `0x1337` Warsaw Hackerspace, `0x2342` warpzone, `0x4354` Confidential Technologies, `0x4D52` Miralium Research, `0x5343` SSE Carte à puce, `0x5443` TIF-IT e.V., `0x63AF` Trustica, `0xBA53` c-base e.V., `0xBD0E` Paranoidlabs, `0xCA05` Atos CardOS, `0xF1D0` CanoKeys, `0xF517` FSIJ, `0xF5EC` F-Secure. Special ranges → `None`: `0x0000`/`0xFFFF` = test card, `0xFF00`–`0xFFFE` = unmanaged S/N range.
- Token2 full serial: SELECT FIDO applet `keyroost_token2otp::FIDO_APPLET_AID`, then `keyroost_token2otp::read_serial_request()`, parse with `keyroost_token2otp::parse_serial`.
- Branch: `fix/card-content-identity` (created off origin/main; the design spec is committed on it).

---

### Task 1: OpenPGP manufacturer registry (keyroost-openpgp)

**Files:**
- Modify: `crates/keyroost-openpgp/src/lib.rs` — add two pure functions near `AID_PREFIX` (~line 36) and a `#[cfg(test)]` block.

**Interfaces:**
- Consumes: `AID_PREFIX` (existing, `[0xD2,0x76,0x00,0x01,0x24,0x01]`).
- Produces:
  - `pub fn aid_manufacturer_id(aid: &[u8]) -> Option<u16>` — the 2-byte manufacturer ID (bytes 8..10, big-endian) from a well-formed OpenPGP AID; `None` if `aid` doesn't start with `AID_PREFIX` or is too short.
  - `pub fn manufacturer_name(id: u16) -> Option<&'static str>` — vendor name per the registry; `None` for unknown / test / unmanaged-S/N ranges.

- [ ] **Step 1: Write failing tests**

Add a `#[cfg(test)] mod manufacturer_tests { use super::*; … }` at the end of `crates/keyroost-openpgp/src/lib.rs` (or into the existing test module if one is at file scope — grep `#[cfg(test)]` first):

```rust
#[test]
fn aid_manufacturer_id_extracts_bytes_8_9() {
    // D2 76 00 01 24 01 | 03 04 (version) | 00 11 (Token2) | AA BB CC DD (serial) | 00 00
    let aid = [
        0xD2, 0x76, 0x00, 0x01, 0x24, 0x01, 0x03, 0x04, 0x00, 0x11, 0xAA, 0xBB, 0xCC, 0xDD,
        0x00, 0x00,
    ];
    assert_eq!(aid_manufacturer_id(&aid), Some(0x0011));
    // Yubico example: manufacturer 00 06.
    let mut yk = aid;
    yk[8] = 0x00;
    yk[9] = 0x06;
    assert_eq!(aid_manufacturer_id(&yk), Some(0x0006));
    // Wrong prefix -> None.
    let mut bad = aid;
    bad[0] = 0x00;
    assert_eq!(aid_manufacturer_id(&bad), None);
    // Too short (no manufacturer bytes) -> None.
    assert_eq!(aid_manufacturer_id(&aid[..9]), None);
}

#[test]
fn manufacturer_name_maps_registry_and_rejects_special_ranges() {
    assert_eq!(manufacturer_name(0x0011), Some("Token2"));
    assert_eq!(manufacturer_name(0x0006), Some("Yubico"));
    assert_eq!(manufacturer_name(0x0005), Some("ZeitControl"));
    assert_eq!(manufacturer_name(0x000F), Some("Nitrokey"));
    assert_eq!(manufacturer_name(0xF1D0), Some("CanoKeys"));
    // Unknown, test, and unmanaged-S/N ranges -> None (caller falls back).
    assert_eq!(manufacturer_name(0x1234), None);
    assert_eq!(manufacturer_name(0x0000), None); // test card
    assert_eq!(manufacturer_name(0xFFFF), None); // test card
    assert_eq!(manufacturer_name(0xFF42), None); // unmanaged S/N range
}
```

- [ ] **Step 2: Verify fail** — `cargo test -p keyroost-openpgp --offline manufacturer` → FAIL (functions missing).

- [ ] **Step 3: Implement**

```rust
/// The 2-byte manufacturer ID (big-endian) from an OpenPGP card AID, i.e.
/// bytes 8..10 of `D2 76 00 01 24 01 <version:2> <manufacturer:2>
/// <serial:4> 00 00`. `None` when `aid` doesn't begin with [`AID_PREFIX`] or
/// is shorter than 10 bytes.
#[must_use]
pub fn aid_manufacturer_id(aid: &[u8]) -> Option<u16> {
    if aid.len() < 10 || aid[0..6] != AID_PREFIX {
        return None;
    }
    Some(u16::from_be_bytes([aid[8], aid[9]]))
}

/// Vendor display name for an OpenPGP manufacturer ID, using the registry
/// GnuPG's scdaemon publishes (`scd/app-openpgp.c`), so keyroost's vendor
/// label matches `gpg --card-status`. `None` for unknown IDs and the reserved
/// test (`0x0000`, `0xFFFF`) and unmanaged-serial (`0xFF00`..=`0xFFFE`)
/// ranges, so the caller can fall back to another signal.
#[must_use]
pub fn manufacturer_name(id: u16) -> Option<&'static str> {
    let name = match id {
        0x0001 => "PPC Card Systems",
        0x0002 => "Prism",
        0x0003 => "OpenFortress",
        0x0004 => "Wewid",
        0x0005 => "ZeitControl",
        0x0006 => "Yubico",
        0x0007 => "OpenKMS",
        0x0008 => "LogoEmail",
        0x0009 => "Fidesmo",
        0x000A => "VivoKey",
        0x000B => "Feitian Technologies",
        0x000D => "Dangerous Things",
        0x000E => "Excelsecu",
        0x000F => "Nitrokey",
        0x0010 => "NeoPGP",
        0x0011 => "Token2",
        0x002A => "Magrathea",
        0x0042 => "GnuPG e.V.",
        0x1337 => "Warsaw Hackerspace",
        0x2342 => "warpzone",
        0x4354 => "Confidential Technologies",
        0x4D52 => "Miralium Research",
        0x5343 => "SSE Carte à puce",
        0x5443 => "TIF-IT e.V.",
        0x63AF => "Trustica",
        0xBA53 => "c-base e.V.",
        0xBD0E => "Paranoidlabs",
        0xCA05 => "Atos CardOS",
        0xF1D0 => "CanoKeys",
        0xF517 => "FSIJ",
        0xF5EC => "F-Secure",
        _ => return None, // unknown, test (0x0000/0xFFFF), or unmanaged range
    };
    Some(name)
}
```

- [ ] **Step 4: Verify pass** — `cargo test -p keyroost-openpgp --offline manufacturer`.

- [ ] **Step 5: Commit**

```bash
cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check
git add crates/keyroost-openpgp/src/lib.rs
git commit --no-gpg-sign -m "feat(openpgp): OpenPGP AID manufacturer-ID registry (GnuPG table)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Unified exchange + card-content probe (keyroost-transport)

**Files:**
- Modify: `crates/keyroost-transport/src/lib.rs` — add `exchange_apdu` (near `transmit_apdu` ~1039); add `openpgp_manufacturer` to `ReaderProbe` (~631) and its two constructor sites (~712, ~730); rewrite the Token2 full-serial block (~822) and the OpenPGP-AID fallback block (~852).

**Interfaces:**
- Consumes: `classify_sw`/`SwAction` and `keyroost_proto::apdu::resend_with_le` (added for #84); `keyroost_openpgp::{aid_manufacturer_id, select}`; `keyroost_token2otp::{FIDO_APPLET_AID, build_select, read_serial_request, parse_serial}`.
- Produces: `ReaderProbe.openpgp_manufacturer: Option<u16>`; `ReaderProbe.serial` now populated reader-agnostically (full serial when a vendor probe succeeds, else the 4-byte AID serial, else empty).

- [ ] **Step 1: Add `exchange_apdu` (handles 61xx + 6Cxx)**

Add next to `transmit_apdu` in `crates/keyroost-transport/src/lib.rs`:

```rust
/// A body-returning applet exchange for the probe: send `apdu`, follow `61xx`
/// (GET RESPONSE via `get_response`) and `6Cxx` (reissue the same command with
/// the corrected Le — the ISO 7816-4 case that a raw T=0 reader surfaces and
/// broke Token2 serial reads on generic readers, #83/#84), and return
/// `(data, sw)`. Shares the status-word decision with the session path via
/// `classify_sw` + `resend_with_le`, so `6Cxx` handling lives in one place.
fn exchange_apdu(
    card: &Card,
    apdu: &[u8],
    more_data_sw: u8,
    get_response: fn() -> Vec<u8>,
) -> Result<(Vec<u8>, u16), TransportError> {
    let mut acc = Vec::new();
    let original = apdu.to_vec();
    let mut to_send = apdu.to_vec();
    let mut steps = 0usize;
    loop {
        let (data, s1, s2) = transmit_apdu(card, &to_send)?;
        steps += 1;
        if steps > 32 {
            return Err(TransportError::MalformedResponse(
                "applet exchange exceeded the continuation limit",
            ));
        }
        match classify_sw(s1, more_data_sw, s2) {
            SwAction::MoreData => {
                acc.extend_from_slice(&data);
                to_send = get_response();
            }
            SwAction::WrongLe(le) => {
                acc.clear();
                to_send = keyroost_proto::apdu::resend_with_le(&original, le);
            }
            SwAction::Done => {
                acc.extend_from_slice(&data);
                return Ok((acc, u16::from_be_bytes([s1, s2])));
            }
        }
    }
}
```

(`transmit_apdu` stays for the SELECT-and-ignore-SW calls, which never
continue. `exchange_apdu` is for the two body reads below.)

- [ ] **Step 2: Add the `openpgp_manufacturer` field**

In `struct ReaderProbe` (grep `pub struct ReaderProbe`), add after `serial`:

```rust
    /// OpenPGP card manufacturer ID (bytes 8..10 of the AID), when an OpenPGP
    /// applet was read. Maps to a vendor name via
    /// `keyroost_openpgp::manufacturer_name` — the card-content vendor signal
    /// that replaces the reader-name guess (#83).
    pub openpgp_manufacturer: Option<u16>,
```

Add `openpgp_manufacturer: None,` to BOTH `ReaderProbe { … }` literals (grep
`ReaderProbe {` — the Molto2-by-name path ~712 and the connected-reader path
~730; both must include the field or it won't compile).

- [ ] **Step 3: Rewrite the OpenPGP-AID read to also capture the manufacturer, via `exchange_apdu`**

Replace the existing `if probe.has_openpgp && probe.serial.is_none() { … }`
block (the AID fallback, ~852) with a version that reads the AID once through
`exchange_apdu`, captures the manufacturer, and keeps the 4-byte serial as the
fallback. NOTE: read the AID whenever OpenPGP is present (not only when serial
is empty) so the manufacturer is always captured; keep the serial assignment
gated on `serial.is_none()`:

```rust
            // OpenPGP present: read the AID once. Capture the manufacturer ID
            // (card-content vendor signal, #83) always; use the embedded
            // 4-byte serial only as a fallback if a vendor full-serial read
            // didn't already populate one.
            if probe.has_openpgp {
                let _ = transmit_apdu(&card, &keyroost_openpgp::select());
                let get_aid = [0x00u8, 0xCA, 0x00, 0x4F, 0x00];
                if let Ok((resp, _sw)) =
                    exchange_apdu(&card, &get_aid, 0x61, || vec![0x00, 0xC0, 0x00, 0x00, 0x00])
                {
                    // Response may be the raw AID or a `4F len …` TLV wrapper.
                    let aid = if resp.len() >= 2 && resp[0] == 0x4F {
                        &resp[2..]
                    } else {
                        &resp[..]
                    };
                    probe.openpgp_manufacturer = keyroost_openpgp::aid_manufacturer_id(aid);
                    if trace {
                        eprintln!(
                            "[probe]   openpgp manufacturer -> {:?}",
                            probe.openpgp_manufacturer
                        );
                    }
                    if probe.serial.is_none()
                        && aid.len() >= 14
                        && aid[0..6] == [0xD2, 0x76, 0x00, 0x01, 0x24, 0x01]
                    {
                        let sn = &aid[10..14];
                        probe.serial =
                            Some(sn.iter().map(|b| format!("{b:02x}")).collect::<String>());
                        if trace {
                            eprintln!("[probe]   openpgp serial (aid) -> {:?}", probe.serial);
                        }
                    }
                }
            }
```

- [ ] **Step 4: Rewrite the Token2 full-serial read: gate on `has_fido`, use `exchange_apdu`**

Replace the existing `if probe.reader_name.to_ascii_lowercase().contains("token2") { … }`
block (~822) with a card-content gate and the unified exchange. This block
runs BEFORE the AID block above (it populates the preferred full serial; the
AID block only fills serial when still `None`):

```rust
            // Token2 full serial via the FIDO applet's GET_INFO (spec §6.10) —
            // longer than the 4-byte OpenPGP-AID serial. Gate on card content
            // (the card actually has a FIDO applet), NOT the reader name, so a
            // Token2 card in a generic reader is read too (#83). Self-
            // validating: a well-formed `D1 len ascii-hex` parse proves it is a
            // Token2 device; anything else falls through harmlessly. SELECT is
            // best-effort (some PIN+ firmware answers 6A81 yet switches applets).
            if probe.has_fido {
                let _ = transmit_apdu(
                    &card,
                    &keyroost_token2otp::build_select(&keyroost_token2otp::FIDO_APPLET_AID),
                );
                if let Ok((body, _sw)) = exchange_apdu(
                    &card,
                    &keyroost_token2otp::read_serial_request(),
                    0x61,
                    || vec![0x00, 0xC0, 0x00, 0x00, 0x00],
                ) {
                    if let Ok(sn) = keyroost_token2otp::parse_serial(&body) {
                        let hex: String = sn.iter().map(|b| format!("{b:02x}")).collect();
                        if !hex.is_empty() {
                            probe.serial = Some(hex);
                            if trace {
                                eprintln!("[probe]   token2 full serial -> {:?}", probe.serial);
                            }
                        }
                    } else if trace {
                        eprintln!("[probe]   token2 serial parse failed (not a Token2 card)");
                    }
                }
            }
```

Implementation notes (grep to confirm before editing):
- Confirm the two blocks' current order and that both are inside the same
  per-reader `if let Ok(card) = …` connection scope. The Token2 block must run
  first (sets the preferred serial); the AID block second (manufacturer always,
  serial only if still `None`).
- Confirm `trace` is the in-scope debug flag name (it is used by the existing
  `eprintln!` lines).
- `keyroost_token2otp::build_select` / `FIDO_APPLET_AID` / `read_serial_request`
  / `parse_serial` are all `pub` (used by the existing block).

- [ ] **Step 5: Gates + commit**

```bash
cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check && cargo test --workspace --offline
git add crates/keyroost-transport/src/lib.rs
git commit --no-gpg-sign -m "fix(transport): read card-content identity in the probe (#83)

Gate the Token2 full-serial read on the card having a FIDO applet rather
than the reader name, capture the OpenPGP manufacturer ID from the AID,
and route both reads through a unified 61xx/6Cxx exchange so they work on
generic T=0 readers. Fixes the full serial not being read for a Token2
smartcard in a third-party reader.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: correlate() stamps card-content vendor + serial (keyroost-resolve)

**Files:**
- Modify: `crates/keyroost-resolve/src/device.rs` — the connected-reader device build (~289–312), where `vendor` is currently derived from the reader name.

**Interfaces:**
- Consumes: `ReaderProbe.openpgp_manufacturer` (Task 2), `keyroost_openpgp::manufacturer_name` (Task 1). `ReaderProbe.serial` (already used).
- Produces: `Device.vendor` from the manufacturer registry when known; unchanged serial plumbing (the probe already put the full serial in `ReaderProbe.serial`).

- [ ] **Step 1: Write a failing test for the vendor-selection helper**

Extract the vendor decision into a pure, testable helper. Add to a
`#[cfg(test)]` module in `crates/keyroost-resolve/src/device.rs`:

```rust
#[test]
fn card_vendor_prefers_openpgp_manufacturer_over_reader_name() {
    // Known manufacturer id -> registry vendor name, regardless of reader.
    assert_eq!(
        card_vendor(Some(0x0011), "Alcor Micro Corp. AU9540 00 00"),
        "Token2"
    );
    assert_eq!(
        card_vendor(Some(0x000F), "SCM Microsystems Inc. reader 00"),
        "Nitrokey"
    );
    // No/unknown manufacturer id -> fall back to the reader-name first word.
    assert_eq!(card_vendor(None, "Feitian ePass 00"), "Feitian");
    assert_eq!(card_vendor(Some(0x1234), "SCM Micro 00"), "SCM");
    // Empty reader name with no manufacturer -> the existing "Key" default.
    assert_eq!(card_vendor(None, ""), "Key");
}
```

- [ ] **Step 2: Verify fail** — `cargo test -p keyroost-resolve --offline card_vendor` → FAIL (helper missing).

- [ ] **Step 3: Implement the helper + wire it in**

Add the pure helper near the top of `device.rs` (module scope):

```rust
/// The vendor name for a PC/SC-reader device: the OpenPGP manufacturer ID
/// mapped through the registry when known (card-content identity, #83), else
/// the first word of the reader name (the pre-existing guess), else "Key".
fn card_vendor(openpgp_manufacturer: Option<u16>, reader_name: &str) -> String {
    if let Some(name) = openpgp_manufacturer.and_then(keyroost_openpgp::manufacturer_name) {
        return name.to_string();
    }
    reader_name
        .split_whitespace()
        .next()
        .unwrap_or("Key")
        .to_string()
}
```

Replace the existing reader-name vendor derivation (the
`let vendor = if p.yubikey_serial.is_some() { "Yubico" … } else { p.reader_name.split_whitespace()… }`
block ~289) so the YubiKey path is unchanged and the else branch uses the
helper:

```rust
        let vendor = if p.yubikey_serial.is_some() {
            "Yubico".to_string()
        } else {
            card_vendor(p.openpgp_manufacturer, &p.reader_name)
        };
```

(The serial already flows from `p.serial` — the probe now put the full serial
there when available, so no serial change is needed here. Confirm the `serial`
let binding a few lines up still reads `p.yubikey_serial … .or_else(|| p.serial.clone())`.)

- [ ] **Step 4: Verify pass** — `cargo test -p keyroost-resolve --offline card_vendor` + `cargo test --workspace --offline`.

- [ ] **Step 5: Commit**

```bash
cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check
git add crates/keyroost-resolve/src/device.rs
git commit --no-gpg-sign -m "fix(resolve): vendor from OpenPGP manufacturer id, not the reader name (#83)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Docs + hardware verification item

**Files:** `CHANGELOG.md` (`[Unreleased]`), `TODO-v0.7.5.md` (hardware list).

- [ ] **Step 1: Edits**

CHANGELOG `### Fixed` (or add one) under `[Unreleased]`:

```markdown
- **Smart-card vendor and serial now come from the card, not the reader**
  ([#83]). A Token2 smartcard in a third-party reader now shows the full
  device serial (read over any reader, including T=0 readers) and the vendor
  "Token2" — previously it showed only the 8-digit OpenPGP serial and the
  reader's name. OpenPGP cards from any vendor now get their correct vendor
  name via the standard manufacturer-ID registry.
```

Add the `[#83]` link reference next to the others at the bottom of CHANGELOG.md
if that section exists (grep `[#8`).

TODO-v0.7.5 hardware list:

```markdown
- [ ] **Card-content identity (#83), v0.7.7:** with a Token2 PIN+ smartcard in
      a GENERIC reader (Alcor/SCM/Realtek), confirm keyroost shows vendor
      "Token2" and the FULL serial (not the 8-digit one); in the Token2 dual
      reader it still does; a non-Token2 OpenPGP card (e.g. Nitrokey) shows its
      correct registry vendor name; a model that rejects GET_INFO over contact
      falls back to the 8-digit serial with no error.
```

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md TODO-v0.7.5.md
git commit --no-gpg-sign -m "docs: card-content identity — changelog + deferred hardware check (#83)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** Component 1 registry (Task 1) ✅; Component 3 unified `exchange_apdu` 61xx+6Cxx (Task 2 Step 1) ✅; Component 2 reader-agnostic Token2 serial gated on `has_fido`, self-validating, + manufacturer capture (Task 2 Steps 2–4) ✅; Component 4 `correlate` vendor from registry (Task 3) ✅; YubiKey serial path left untouched (Task 3 keeps the `yubikey_serial` branch) ✅; graceful fallback preserved (AID serial when no full read; reader-name vendor when no known manufacturer) ✅; docs + hardware item (Task 4) ✅; read-only/no-deps ✅.
- **Placeholder scan:** none — every code step carries complete code; the grep-to-confirm notes name real existing symbols (`trace`, the two `ReaderProbe {` literals, `keyroost_token2otp::*`).
- **Type consistency:** `aid_manufacturer_id`/`manufacturer_name` (Task 1) consumed verbatim in Tasks 2/3; `ReaderProbe.openpgp_manufacturer: Option<u16>` defined in Task 2, read in Task 3; `card_vendor(Option<u16>, &str) -> String` and `exchange_apdu(&Card, &[u8], u8, fn()->Vec<u8>)` consistent within their tasks. Registry hex values match the spec's Global Constraints table exactly.
- **Testability note:** Tasks 1 and 3 are pure and unit-tested. Task 2 is PC/SC I/O (the probe) — not unit-testable without hardware, matching the existing probe's zero-unit-test posture; its correctness rests on the tested pure pieces (`classify_sw`, `resend_with_le`, `aid_manufacturer_id`) plus the deferred hardware item. Flagged, not a gap.
