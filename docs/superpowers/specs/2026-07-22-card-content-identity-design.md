# Card-content device identity (serial + vendor) — design

Fixes #83 (Token2 smartcards show only the 8-digit OpenPGP serial) at its
root: keyroost currently derives a smart-card device's **vendor** from the
reader name and gates its **full-serial read** on the reader name containing
"token2". Both are wrong signals — the reader is incidental; the same card
gives a different reader name in an Alcor vs. a Token2 reader. Identity must
come from the **card's own content**.

## Root cause (three symptoms, one cause)

Identity keyed on the reader instead of the card, in `probe_readers` /
`correlate` (`crates/keyroost-transport/src/lib.rs`, `crates/keyroost-resolve/
src/device.rs`):

1. **Wrong vendor name.** `vendor = reader_name.split_whitespace().next()`
   (`device.rs:293`) → a Token2 card in an "Alcor Micro…" reader displays
   vendor **"Alcor"**.
2. **Full serial never read on generic readers.** The full-serial read
   (`lib.rs:822`) is gated on `reader_name.contains("token2")`, so a Token2
   card in a generic reader falls through to the 8-digit OpenPGP-AID serial.
3. **Full serial breaks on T=0 readers even when triggered.** The probe's
   `transmit_apdu` (`lib.rs:1039`) handles neither `61xx` nor `6Cxx` cleanly;
   the caller patches `61xx` inline, nothing handles `6Cxx`, so a generic T=0
   reader (Alcor/SCM/Realtek) fails the read at the wrong-Le status word — the
   same root cause fixed in the session path for #84, in a second copy here.

## Design decisions

| Decision | Choice |
|---|---|
| Vendor identity source | **General, card-content: the OpenPGP AID manufacturer ID** (bytes 8–9), mapped via the standard registry (GnuPG's table). Not a Token2 special-case — every OpenPGP card vendor gets its correct name from the card. |
| Full-serial read trigger | **Self-validating, not a perfect gate.** Attempt the vendor serial read on a card that genuinely has the applet (`has_fido` for Token2's FIDO GET_INFO); a well-formed parse proves the vendor+support, a failed parse falls back. |
| Per-vendor serial reads | **Inherent and extensible.** No standard command reads a card's full printed serial; each vendor's is proprietary (Token2 `80 33 00 00`; YubiKey management GET SERIAL — already wired). Structured as a best-effort list of vendor serial probes; adding a vendor = one drop-in function, no identity rewire. |
| APDU exchange | **One status-word-complete path.** Route the probe's reads through the same `classify_sw`/`resend_with_le` logic landed for #84 so `61xx`+`6Cxx` live in one place; retire the probe's half-complete copy. General, all vendors, all T=0 readers. |
| Auth / scope | Read-only, no auth, no new deps. Molto2 readers stay untouched (never connected during probe). |

## Component 1 — OpenPGP manufacturer registry (general, vendor-neutral)

A new pure module in `keyroost-openpgp` (byte layer): parse the manufacturer
ID from an OpenPGP AID and map it to a vendor name via the canonical registry.

```rust
/// The 2-byte manufacturer ID from an OpenPGP card AID
/// (`D2 76 00 01 24 01 <version:2> <manufacturer:2> <serial:4> 00 00`).
/// Returns None if `aid` is not a well-formed OpenPGP AID.
pub fn aid_manufacturer_id(aid: &[u8]) -> Option<u16>;

/// Vendor display name for an OpenPGP manufacturer ID, per the registry
/// GnuPG's scdaemon uses (scd/app-openpgp.c). None for unknown/unmanaged/test
/// ranges so callers can fall back to another signal.
pub fn manufacturer_name(id: u16) -> Option<&'static str>;
```

The registry (verbatim from GnuPG `app-openpgp.c`, so keyroost matches
`gpg --card-status` output): `0x0001` PPC Card Systems, `0x0002` Prism,
`0x0003` OpenFortress, `0x0004` Wewid, `0x0005` ZeitControl, `0x0006` Yubico,
`0x0007` OpenKMS, `0x0008` LogoEmail, `0x0009` Fidesmo, `0x000A` VivoKey,
`0x000B` Feitian Technologies, `0x000D` Dangerous Things, `0x000E` Excelsecu,
`0x000F` Nitrokey, `0x0010` NeoPGP, `0x0011` **Token2**, `0x002A` Magrathea,
`0x0042` GnuPG e.V., `0x1337` Warsaw Hackerspace, `0x2342` warpzone, `0x4354`
Confidential Technologies, `0x4d52` Miralium Research, `0x5343` SSE Carte à
puce, `0x5443` TIF-IT e.V., `0x63AF` Trustica, `0xBA53` c-base e.V., `0xBD0E`
Paranoidlabs, `0xCA05` Atos CardOS, `0xF1D0` CanoKeys, `0xF517` FSIJ, `0xF5EC`
F-Secure. Special ranges → None (caller falls back): `0x0000`/`0xFFFF` = test
card, `0xFF00`–`0xFFFE` = unmanaged S/N range.

Pure, fully unit-tested: known IDs → name, Token2 = `0x0011`, unknown → None,
test/unmanaged ranges → None, malformed AID → None.

## Component 2 — vendor serial probes (extensible; Token2 + YubiKey today)

In `probe_readers`, on the single card connection, after reading the OpenPGP
AID (which we already do), try the known vendor full-serial reads best-effort:

- **YubiKey:** management-applet GET SERIAL — *already wired* (`yubikey_serial`).
  Left as-is; it's the existing first vendor serial probe.
- **Token2:** SELECT FIDO applet (`A0000006472F0001`) + GET_INFO
  (`80 33 00 00`, `D1 10` + 16 zero bytes) → `parse_serial`. Attempted when the
  card has a FIDO applet (`has_fido`). A well-formed `D1 <len> <ascii-hex>`
  result is self-validating proof it's a Token2 device supporting the full
  serial.

These are a clearly-marked best-effort sequence: the first that yields a valid
serial wins; a future vendor is one more entry. The serial reads go through the
unified APDU exchange (Component 3), so they survive T=0 readers.

`ReaderProbe` gains `openpgp_manufacturer: Option<u16>` (parsed from the AID it
already fetches). Its existing `serial` field carries the full serial when a
vendor probe succeeds, else the 4-byte OpenPGP-AID serial, else empty — the
current fallback ladder, now reader-agnostic.

## Component 3 — one status-word-complete APDU exchange

The `classify_sw` (MoreData / WrongLe / Done) + `resend_with_le` logic added
for #84 lives in the session path (`transmit_applet`). Extract the core
exchange so the probe's serial/AID reads use it too (handling `61xx` **and**
`6Cxx`), and delete the probe's inline `61xx`-only patch + the half-complete
`transmit_apdu`. One correct exchange, every applet, every vendor, every T=0
reader. This is what makes the Token2 read actually work on the reporter's
Alcor/SCM/Realtek readers.

## Component 4 — correlate() stamps card-content identity

In `correlate` (`device.rs`): when `openpgp_manufacturer` maps to a known
vendor, set `Device.vendor` to that name (replacing the reader-name guess);
`Device.serial` uses the probe's serial (full when a vendor probe succeeded).
YubiKey keeps its existing dedicated path. A card with no known manufacturer ID
and no vendor serial degrades exactly as today (reader-name vendor, 8-digit or
empty serial) — never an error.

## Data flow

`probe_readers` (one connection/card) → SELECT OpenPGP, read AID → parse
`openpgp_manufacturer` (0x0011 ⇒ Token2, etc.) → run vendor serial probes
best-effort through the unified exchange (YubiKey serial; Token2 FIDO GET_INFO
when `has_fido`) → `ReaderProbe { openpgp_manufacturer, serial, … }` →
`correlate` stamps `Device.vendor` (from the registry) + `Device.serial` (full
when available).

## Error handling / etiquette

Every card read is best-effort and ignored on failure — unchanged posture. A
non-Token2 FIDO card that gets the Token2 GET_INFO simply fails `parse_serial`
and is skipped (harmless read). A card model that won't answer its vendor
serial over contact/NFC degrades to the 8-digit serial. Non-OpenPGP,
non-FIDO cards are unaffected. Molto2 readers are never connected.

## Testing

- **Pure/unit (no hardware):** `aid_manufacturer_id` (well-formed AID → id;
  malformed → None), `manufacturer_name` (Token2 = 0x0011, several other
  registry entries, unknown/test/unmanaged → None), `parse_serial` (existing
  KAT stays), and the `classify_sw`/`resend_with_le` exchange over `61xx` and
  `6Cxx` (added for #84, extended for the shared path).
- **Hardware verification (deferred `docs/superpowers` item):** a Token2 PIN+
  card in a **generic** reader (Alcor/SCM/Realtek) shows vendor "Token2" and
  the **full** serial; a Token2 card in the Token2 dual reader still does;
  a non-Token2 OpenPGP card (e.g. a Nitrokey) shows its correct vendor name
  from the registry; a card model that rejects GET_INFO over contact falls back
  to the 8-digit serial without error.

## Scope / non-goals

- Read-only; no auth; no new deps.
- Does **not** touch the #84 session-path fix (already landed); this is the
  probe/identity path.
- No speculative multi-vendor serial infrastructure — the extensible seam is a
  best-effort list with an obvious insertion point, not a plugin system. A new
  vendor's serial is added when its command is known and there's hardware to
  verify, not before.
- FIDO-only Token2 devices without OpenPGP: handled by the self-validating
  GET_INFO leg (no OpenPGP AID / manufacturer ID to read on those, but the FIDO
  serial read still applies).
