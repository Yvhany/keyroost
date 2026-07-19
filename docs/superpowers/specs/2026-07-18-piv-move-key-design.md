# PIV move-key — design

Decided with the user, 2026-07-18. Expose the slot-to-slot MOVE KEY operation
(Yubico `0xF6` extension, firmware 5.7+) which keyroost's byte layer already
carries but currently uses only for the delete-sentinel variant. Sibling to the
shipped PIV generate / import / delete / reset work.

## Decisions log

| Question | Decision |
|---|---|
| Retired slots (0x82–0x95) in the model? | **Yes, first-class (A).** The rotate-and-archive use case is the whole point of move-key. Fallback if A becomes a technical/UI dead end: **C** — retired slots as move *destinations* only, not full first-class slots. |
| Certificate handling on move | **Key only, mirror ykman, warn about the split (A).** MOVE KEY moves only the private key; the cert stays in the source slot. Surface a clear note. **Future goal:** optionally move the cert with the key (B) — recorded below, not built now. |
| Destination-occupied policy / confirmation weight | **Refuse occupied destinations; no ceremony for the safe case (A).** Pre-check via GET METADATA and refuse before sending; an empty-destination move is non-destructive, so no `--yes`/typed confirm — same weight as generate-key/import-cert. |
| GUI presentation of 20 retired slots | **Four standard always; retired in a collapsible section, occupied shown by default, empties revealed on demand (A).** Pixel-level polish deferred to the separate v0.7.7 UI-polish pass. |

## Slot model (keyroost-piv)

Extend `Slot` with a retired variant:

```rust
pub enum Slot {
    Authentication,   // 9A
    Signature,        // 9C
    KeyManagement,    // 9D
    CardAuthentication, // 9E
    Retired(u8),      // 1..=20  ->  key_ref 0x82..=0x95
}
```

- `key_ref()` for `Retired(n)` = `0x81 + n` (Retired(1)=0x82 … Retired(20)=0x95).
  Construction must reject `n` outside 1..=20 (a `Retired(0)`/`Retired(21)` has
  no valid ref) — via a checked constructor `Slot::retired(n) -> Option<Slot>`
  used by all parsing; the enum stays the representation.
- `cert_object_tag()` for `Retired(n)` = `[0x5F, 0xC1, 0x0C + n]`
  (Retired(1)=`5F C1 0D` … Retired(20)=`5F C1 20`).
- The four standard variants are unchanged.

This is the ripple root: the CLI slot parser, the GUI slot list, and status
read-back all consume the extended model.

## Byte layer (keyroost-piv)

```rust
/// Yubico MOVE KEY: relocate a slot's private key to another slot.
/// `00 F6 <dest key_ref> <src key_ref>`. The move variant of the same 0xF6
/// opcode whose 0xFF-sentinel form deletes (see `delete_key`). Moves ONLY the
/// private key — the source slot's certificate object is untouched. Requires
/// firmware 5.7+ and prior management-key authentication.
pub fn move_key(src: Slot, dest: Slot) -> Vec<u8>
```

Known-answer tests: representative pairs (standard→retired, retired→standard,
standard→standard), plus KATs for the new retired `key_ref`s and
`cert_object_tag`s across the 1/20 boundaries.

## Transport (`PivSession::move_key`)

```rust
pub fn move_key(&mut self, src: Slot, dest: Slot) -> Result<(), TransportError>
```

Order of checks (all before any destructive-looking APDU — though move is not
destructive):
1. `src == dest` → `Err` (nothing to do; refuse rather than round-trip).
2. Firmware < 5.7 (from cached status version) → `Err` with a clear
   "MOVE KEY needs firmware 5.7+" message.
3. GET METADATA on `dest`; if it reports a key present → `Err`
   "slot <dest> already holds a key — delete it first or pick an empty slot".
   (Belt-and-suspenders with the card's own refusal.)
4. Send `move_key(src, dest)`; map a card refusal to a typed error.

Requires management-key auth (reuse the existing auth path — same precondition
as generate/import/delete). Status read-back gains retired-slot occupancy,
read **lazily**: only when the retired section is expanded or a move dialog
opens, so a normal PIV-pane load does not fire 20 extra GET METADATA calls.

## CLI

```
keyroostctl piv move-key --from <slot> --to <slot>
```

- `<slot>` accepts `9a` / `9c` / `9d` / `9e` / `82`–`95` (extend `CliPivSlot`
  with the 20 retired hex values, matching keyroost's existing hex-ref naming
  — not ykman's `retired1` names).
- No `--yes` (safe operation).
- On success, prints the cert-stays-behind note:
  "moved the private key <src> → <dest>; the certificate remains in <src>".

## GUI (PIV pane)

- A collapsible **"Retired slots"** section below the four standard slots.
  Occupied retired slots render by default; a reveal shows the empty ones when
  the user needs a move destination.
- Any occupied slot (standard or retired) gets a **"Move key…"** button opening
  a device-bound dialog (KEY-008 posture, mirroring the existing PIV modals)
  with a destination picker listing only **empty** slots.
- On success the affected panes refresh and the cert-split note is shown.
- Functional layout only here; pixel-level polish lands in the v0.7.7
  UI-polish pass.

## Errors & safety

Non-destructive by construction — the key is relocated, never erased. Occupied
destinations are refused (card + our pre-check); same-slot refused; pre-5.7
firmware refused with guidance. The only state change is the key's location and
the (documented) orphaned cert left in the source slot.

## Testing

- Pure / KAT: `move_key` bytes, retired `key_ref`/`cert_object_tag` (incl. the
  1 and 20 boundaries and rejection of out-of-range `n`), the occupancy-refusal
  decision logic, CLI grammar (`--from`/`--to` decode, retired hex values).
- Hardware verification (deferred hardware session, flagship item): a real
  rotate-and-archive round trip on the YubiKey 5.7 — move the Key-Management
  key to a retired slot, confirm it's gone from 9D and present in the retired
  slot, confirm the cert stayed in 9D, and that an occupied-destination move is
  refused.

## Future goal (recorded, not built)

- **Move the certificate with the key (Q2/B):** an option to also carry the
  X.509 cert to the destination (read cert from src, PUT DATA to dest, clear
  src cert) so the destination becomes a complete usable slot and the source is
  fully cleared. Diverges from ykman and isn't wanted by the archival use case,
  so deferred; revisit if users ask for a one-step "relocate everything".

## Out of scope

- The attestation slot (`F9`) is not in keyroost's `Slot` model and stays out.
- Overwriting occupied destinations stays refused (no delete-then-move path).
