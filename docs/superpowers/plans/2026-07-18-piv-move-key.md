# PIV move-key Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose the Yubico slot-to-slot MOVE KEY operation (`0xF6`, fw 5.7+) with first-class retired-slot support, through byte layer / transport / CLI / GUI, per `docs/superpowers/specs/2026-07-18-piv-move-key-design.md`.

**Architecture:** Extend `keyroost_piv::Slot` with a `Retired(u8)` variant (1–20). Add a `move_key` byte builder and a `PivSession::move_key` that pre-checks (same-slot, firmware, destination-occupied) before sending. CLI `piv move-key` and a GUI Overview→PIV-pane "Move key…" flow consume it; retired slots render in a collapsible section with lazily-read occupancy.

**Tech Stack:** Rust workspace (keyroost-piv byte layer, keyroost-transport PC/SC, keyroostctl clap CLI, keyroost egui GUI). No new dependencies.

## Global Constraints

- Commit UNSIGNED: `git commit --no-gpg-sign`, footer `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`. Never push main, never create tags. Guard-tripping messages → `git commit -F <scratch-file>`. When staging, name files explicitly on `git add` (never `git add -A`/`.` — untracked plan docs live in the tree).
- Gates before every commit: `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo fmt --all --check`, `cargo test --workspace --offline`. MSRV 1.85 libs/CLI, 1.92 GUI.
- No new dependencies.
- Retired slots: `Retired(n)` for n in 1..=20 → key_ref `0x81 + n` (0x82..=0x95), cert-object-tag `[0x5F, 0xC1, 0x0C + n]` (`5F C1 0D`..=`5F C1 20`).
- Move bytes: `00 F6 <dest key_ref> <src key_ref>` (the move variant of the shipped `delete_key` = `00 F6 FF <src>`).
- Safety: move is non-destructive; refuse occupied destinations (GET METADATA pre-check), refuse same-slot, refuse firmware < 5.7 with a clear message. No `--yes`/typed confirm for the safe empty-destination case.
- Move relocates the KEY ONLY; the source slot's certificate object stays put — surface a note. (Moving the cert too is a recorded future goal, not built here.)
- Branch: `feat/piv-move-key` (already created off origin/main; the design spec is committed on it).

---

### Task 1: Slot model — add `Retired(u8)`

**Files:**
- Modify: `crates/keyroost-piv/src/lib.rs` — `Slot` enum (116–125), `key_ref` (127–137), `cert_object_tag` (140–148), `label` (151–159), `Slot::all` (161–170); tests near 969.

**Interfaces:**
- Produces: `Slot::Retired(u8)` variant; `Slot::retired(n: u8) -> Option<Slot>` checked constructor (1..=20 only); `Slot::retired_all() -> [Slot; 20]`; extended `key_ref`/`cert_object_tag`/`label`. `Slot::all()` stays `[Slot; 4]` (the four standard slots — status() must stay cheap).

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)] mod tests` in `crates/keyroost-piv/src/lib.rs`:

```rust
#[test]
fn retired_slot_refs_and_tags_across_the_range() {
    let r1 = Slot::retired(1).unwrap();
    let r20 = Slot::retired(20).unwrap();
    assert_eq!(r1.key_ref(), 0x82);
    assert_eq!(r20.key_ref(), 0x95);
    assert_eq!(r1.cert_object_tag(), [0x5F, 0xC1, 0x0D]);
    assert_eq!(r20.cert_object_tag(), [0x5F, 0xC1, 0x20]);
    // Out-of-range rejected by the constructor.
    assert!(Slot::retired(0).is_none());
    assert!(Slot::retired(21).is_none());
    // retired_all() is the 20 retired slots in order.
    let all = Slot::retired_all();
    assert_eq!(all.len(), 20);
    assert_eq!(all[0], r1);
    assert_eq!(all[19], r20);
    // The standard Slot::all() is unchanged (still 4).
    assert_eq!(Slot::all().len(), 4);
}

#[test]
fn retired_label_is_stable() {
    assert_eq!(Slot::retired(1).unwrap().label(), "retired 1 (82)");
    assert_eq!(Slot::retired(20).unwrap().label(), "retired 20 (95)");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p keyroost-piv --offline retired_slot`
Expected: FAIL — no `Retired` variant / `retired` / `retired_all`.

- [ ] **Step 3: Implement**

Add the variant to the enum:

```rust
pub enum Slot {
    Authentication,     // 9A
    Signature,          // 9C
    KeyManagement,      // 9D
    CardAuthentication, // 9E
    /// Yubico retired key-management slots (fw 5.7+). `Retired(n)` for
    /// n = 1..=20 → key_ref 0x82..=0x95. Construct via [`Slot::retired`],
    /// which rejects out-of-range n.
    Retired(u8),
}
```

Add the new arms + constructor + iterator:

```rust
// in key_ref():
Slot::Retired(n) => 0x81 + n,
// in cert_object_tag():
Slot::Retired(n) => [0x5F, 0xC1, 0x0C + n],
// in label():  (returns String today; keep whatever the existing return type is —
// if label() returns &'static str, change to String or use a small buffer.
// The existing four return &'static str; retired needs a formatted string, so
// change label() to return String and update the four standard arms to .into().)
Slot::Retired(n) => format!("retired {n} ({:02X})", 0x81 + n),
```

Constructor + iterator (near the impl):

```rust
/// Checked constructor for a retired slot: `Some` for n in 1..=20, else `None`.
/// All retired-slot construction must go through here so an invalid ref can
/// never be built.
#[must_use]
pub fn retired(n: u8) -> Option<Slot> {
    (1..=20).contains(&n).then_some(Slot::Retired(n))
}

/// The 20 retired slots in order (Retired(1)..=Retired(20)). Kept separate
/// from [`Slot::all`] so status() stays cheap — retired occupancy is read
/// lazily, not on every refresh.
#[must_use]
pub fn retired_all() -> [Slot; 20] {
    let mut out = [Slot::Retired(1); 20];
    let mut i = 0u8;
    while i < 20 {
        out[i as usize] = Slot::Retired(i + 1);
        i += 1;
    }
    out
}
```

Note: `label()` returning `String` ripples to its callers (CLI `PivSlotJson`, status print, GUI). They call `.label()` and use the value as a string — `String` works where `&str` did via deref, but `slot.label().to_string()` becomes `slot.label()`. Fix any resulting type mismatch the compiler flags; do not change call-site behavior.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p keyroost-piv --offline` — all pass (including the existing slot KATs). Then `cargo build --workspace --offline` to surface any `label()` return-type ripples and fix them.

- [ ] **Step 5: Commit**

```bash
cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check
git add crates/keyroost-piv/src/lib.rs crates/keyroostctl/src/main.rs crates/keyroost/src/main.rs
git commit --no-gpg-sign -m "feat(piv): add retired key-management slots to the Slot model

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
(Stage only the files the `label()` ripple actually touched; run `git status` first and add exactly those.)

---

### Task 2: `move_key` byte builder

**Files:**
- Modify: `crates/keyroost-piv/src/lib.rs` — near `delete_key` (777–785); tests near `delete_key_kat_all_slots` (~1196).

**Interfaces:**
- Consumes: `Slot::key_ref` (incl. retired).
- Produces: `pub fn move_key(src: Slot, dest: Slot) -> Vec<u8>` = `00 F6 <dest.key_ref()> <src.key_ref()>`.

- [ ] **Step 1: Failing test**

```rust
#[test]
fn move_key_kat() {
    // 00 F6 <dest> <src>. Standard -> retired (archive KeyManagement to Retired1).
    assert_eq!(
        move_key(Slot::KeyManagement, Slot::retired(1).unwrap()),
        vec![0x00, 0xF6, 0x82, 0x9D]
    );
    // Retired -> standard (restore).
    assert_eq!(
        move_key(Slot::retired(20).unwrap(), Slot::Authentication),
        vec![0x00, 0xF6, 0x9A, 0x95]
    );
    // Standard -> standard.
    assert_eq!(
        move_key(Slot::Signature, Slot::CardAuthentication),
        vec![0x00, 0xF6, 0x9E, 0x9C]
    );
}
```

- [ ] **Step 2: Verify fail** — `cargo test -p keyroost-piv --offline move_key_kat` → FAIL (no `move_key`).

- [ ] **Step 3: Implement** (next to `delete_key`):

```rust
/// Yubico MOVE KEY: relocate a slot's private key to another slot.
/// `00 F6 <dest key_ref> <src key_ref>`. The move variant of the same 0xF6
/// opcode whose 0xFF-sentinel form deletes (see [`delete_key`]). Moves ONLY
/// the private key — the source slot's certificate object is untouched.
/// Requires firmware 5.7+ and prior management-key authentication.
#[must_use]
pub fn move_key(src: Slot, dest: Slot) -> Vec<u8> {
    vec![0x00, Instruction::MoveKey.code(), dest.key_ref(), src.key_ref()]
}
```

- [ ] **Step 4: Verify pass** — `cargo test -p keyroost-piv --offline`.

- [ ] **Step 5: Commit**

```bash
cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check
git add crates/keyroost-piv/src/lib.rs
git commit --no-gpg-sign -m "feat(piv): move_key byte builder (00 F6 dest src)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: `PivSession::move_key` + lazy retired-slot occupancy

**Files:**
- Modify: `crates/keyroost-transport/src/piv.rs` — add methods to `impl PivSession` (near `reset`/`delete` and `slot_key`); `PivStatus`/`slot_status` at 18–48, 447+; tests.

**Interfaces:**
- Consumes: `piv::move_key`, existing `metadata`/`slot_key`, `slot_status`, the cached status `version`, `TransportError` variants.
- Produces:
  - `pub fn move_key(&mut self, src: Slot, dest: Slot) -> Result<(), TransportError>`
  - `pub fn retired_slot_occupancy(&mut self, slot: Slot) -> Result<bool, TransportError>` (true = a key is present in that retired slot; via GET METADATA, on demand — NOT called from `status()`).

- [ ] **Step 1: Failing test**

The transport methods need a card, so unit-test the pure precondition helper. Extract the firmware-gate check as a pure fn and test it:

```rust
#[test]
fn move_key_firmware_gate() {
    // MOVE KEY needs fw 5.7+. Below that -> refuse.
    assert!(!move_key_supported(Some((5, 6, 0))));
    assert!(move_key_supported(Some((5, 7, 0))));
    assert!(move_key_supported(Some((5, 7, 4))));
    assert!(move_key_supported(Some((6, 0, 0))));
    // Unknown version -> allow the attempt (card will reject if unsupported).
    assert!(move_key_supported(None));
}
```

- [ ] **Step 2: Verify fail** — `cargo test -p keyroost-transport --offline move_key_firmware_gate` → FAIL.

- [ ] **Step 3: Implement**

Pure gate (module scope):

```rust
/// Whether MOVE KEY is available given the reported firmware `(major, minor, _)`.
/// fw 5.7+ (Yubico). Unknown version → allow the attempt; the card refuses if
/// it truly can't (belt-and-suspenders with the pre-check).
fn move_key_supported(version: Option<(u8, u8, u8)>) -> bool {
    match version {
        Some((major, minor, _)) => major > 5 || (major == 5 && minor >= 7),
        None => true,
    }
}
```

`impl PivSession` methods (grep the exact existing helper names — `metadata`, `slot_key`, `status`, `transmit_full`, `ok_or_write`, and the version field on the session/status — before wiring; adapt if signatures differ):

```rust
/// Relocate a slot's private key to another slot (Yubico MOVE KEY). Refuses
/// a same-slot move, firmware below 5.7, and an occupied destination
/// (GET METADATA pre-check — the card also refuses, this gives a clear error
/// first). Moves ONLY the key; the source slot's certificate stays put.
/// Requires prior management-key auth (same as generate/import/delete).
pub fn move_key(&mut self, src: Slot, dest: Slot) -> Result<(), TransportError> {
    if src.key_ref() == dest.key_ref() {
        return Err(TransportError::MalformedResponse(
            "source and destination slots are the same".into(),
        ));
    }
    let version = self.status()?.version;
    if !move_key_supported(version) {
        return Err(TransportError::MalformedResponse(
            "MOVE KEY needs firmware 5.7+".into(),
        ));
    }
    if self.slot_has_key(dest)? {
        return Err(TransportError::MalformedResponse(format!(
            "slot {} already holds a key — delete it first or pick an empty slot",
            dest.label()
        )));
    }
    let (_, sw) = self.transmit_full(&piv::move_key(src, dest))?;
    ok_or_write("piv move key", sw)
}

/// Whether a slot holds a private key, via GET METADATA. Retired slots are
/// read only here (on demand), never inside `status()`, so a status refresh
/// stays 4 GET DATA calls rather than 24.
pub fn slot_has_key(&mut self, slot: Slot) -> Result<bool, TransportError> {
    // metadata() returns Ok with algorithm/pubkey when a key exists, and a
    // not-found status word when the slot is empty. Map that to a bool.
    match self.slot_key(slot) {
        Ok(Some(_)) => Ok(true),
        Ok(None) => Ok(false),
        Err(e) => Err(e),
    }
}
```

Implementation note: check the real return shape of the existing `slot_key`/`metadata` (Explore reported `slot_key` returns per-slot alg+pubkey via `metadata(slot.key_ref())`). If `slot_key` returns `Result<Option<..>, _>`, the above works; if it returns `Result<(alg, pubkey), _>` with a not-found error, adapt `slot_has_key` to treat the not-found error as `Ok(false)` and other errors as `Err`. Grep and match reality. Rename `retired_slot_occupancy` from the interface to the more general `slot_has_key` (works for any slot); the GUI calls it for retired slots.

- [ ] **Step 4: Verify pass** — `cargo test -p keyroost-transport --offline` + `cargo build --workspace --offline`.

- [ ] **Step 5: Commit**

```bash
cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check
git add crates/keyroost-transport/src/piv.rs
git commit --no-gpg-sign -m "feat(piv): PivSession::move_key with same-slot/firmware/occupied pre-checks

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: CLI `piv move-key`

**Files:**
- Modify: `crates/keyroostctl/src/main.rs` — `CliPivSlot` (457–483), `PivCmd` (near `DeleteKey` 783–798), the `run_piv` dispatch (near 5006), a `run_piv_move_key` fn; `cli_tests`.

**Interfaces:**
- Consumes: `PivSession::move_key`, `keyroost_piv::Slot`, the existing `open_piv`/mgmt-key auth path used by `PivCmd::DeleteKey`.
- Produces: `keyroostctl piv move-key --from <slot> --to <slot>`, slots accepting `9a`/`9c`/`9d`/`9e`/`82`–`95`. No `--yes`.

- [ ] **Step 1: Failing grammar test** (in `cli_tests`, near the other piv grammar tests):

```rust
#[test]
fn piv_move_key_parses_standard_and_retired_slots() {
    match parse(&["keyroostctl", "piv", "move-key", "--from", "9d", "--to", "82"])
        .unwrap()
        .command
    {
        Some(Cmd::Piv { cmd: PivCmd::MoveKey { from, to, .. } }) => {
            assert_eq!(from.to_slot().key_ref(), 0x9D);
            assert_eq!(to.to_slot().key_ref(), 0x82);
        }
        _ => panic!("expected piv move-key"),
    }
}
```

- [ ] **Step 2: Verify fail** — `cargo test -p keyroostctl --offline piv_move_key_parses` → FAIL (no `MoveKey`, and `CliPivSlot` has no `82`).

- [ ] **Step 3: Implement**

Extend `CliPivSlot` with the 20 retired values. Add variants `Retired1`..`Retired20` each with `#[value(name = "82")]` … `#[value(name = "95")]`, and extend `to_slot()`:

```rust
CliPivSlot::Retired1 => keyroost_piv::Slot::retired(1).unwrap(),
// … through …
CliPivSlot::Retired20 => keyroost_piv::Slot::retired(20).unwrap(),
```

(20 arms; the `unwrap()` is safe — literals 1..=20. Alternatively derive the retired index from the enum discriminant if cleaner, but explicit arms are fine and match the file's style.)

Add the `PivCmd::MoveKey` variant (mirror `DeleteKey`'s struct shape):

```rust
/// Move a slot's private key to another slot (Yubico MOVE KEY, fw 5.7+).
/// Non-destructive; refuses an occupied destination. The certificate stays
/// in the source slot.
MoveKey {
    /// Source slot (9a/9c/9d/9e/82–95).
    #[arg(long)]
    from: CliPivSlot,
    /// Destination slot (must be empty).
    #[arg(long)]
    to: CliPivSlot,
    /// PC/SC reader substring (skips auto-detection).
    #[arg(long)]
    reader: Option<String>,
},
```

Dispatch arm (near `PivCmd::DeleteKey =>`), calling a handler that mirrors the delete-key handler's open+auth pattern:

```rust
PivCmd::MoveKey { from, to, reader } => {
    let mut s = open_piv(reader.as_deref(), debug)?;
    // authenticate management key exactly as DeleteKey does (grep the
    // delete-key handler ~5006 for the mgmt-key auth call and reuse it).
    s.move_key(from.to_slot(), to.to_slot())?;
    println!(
        "moved the private key {} → {}; the certificate remains in {}",
        from.to_slot().label(),
        to.to_slot().label(),
        from.to_slot().label()
    );
    Ok(())
}
```

Grep the `DeleteKey` handler for the exact management-key auth sequence (it authenticates before the destructive op) and replicate it verbatim before the `move_key` call. No `--yes`.

- [ ] **Step 4: Verify pass** — `cargo test -p keyroostctl --offline` + full `cargo test --workspace --offline`.

- [ ] **Step 5: Commit**

```bash
git add crates/keyroostctl/src/main.rs
git commit --no-gpg-sign -m "feat(cli): piv move-key --from/--to across standard + retired slots

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: GUI move-key flow + retired-slot section

**Files:**
- Modify: `crates/keyroost/src/main.rs` — `PivCredKind` (928–1005), its 4 exhaustive methods + `piv_cred_mismatch` (6015) + `piv_cred_success` (6050) + submit dispatch (10508), `render_piv_cred_modal` DeleteKey arm (10434) as the template, `PivState` (1031–1159), a `piv_move_key` submit method (mirror `piv_delete_key` 5543–5570), the pane action card (11802 area) and the slot tab strip / a retired-slots sub-view (11567), `load_piv_status` (5196) for lazy retired occupancy; tests at 13403/13422.

**Interfaces:**
- Consumes: `PivSession::move_key`, `slot_has_key`, `Slot::retired_all`, the existing PIV modal/job machinery.
- Produces: a "Move key…" action on an occupied slot; a destination picker (empty slots only); a collapsible retired-slots section (occupied shown, empties on demand).

- [ ] **Step 1: Failing test** — extend the two `PivCredKind` enumeration tests (13403 `piv_cred_kind_mgmt_key_mapping`, 13422 `piv_cred_kind_gated_strings_are_present`) to include `MoveKey`, and add a pure destination-eligibility test:

```rust
#[test]
fn move_key_destinations_exclude_occupied_and_source() {
    // Given occupancy (slot -> has_key), the eligible destinations are the
    // empty slots that aren't the source.
    let occupied = |s: keyroost_piv::Slot| {
        s == keyroost_piv::Slot::Authentication // 9A occupied
            || s == keyroost_piv::Slot::retired(1).unwrap()
    };
    let src = keyroost_piv::Slot::KeyManagement;
    let dests = move_key_eligible_destinations(src, &occupied);
    assert!(!dests.contains(&src));
    assert!(!dests.contains(&keyroost_piv::Slot::Authentication)); // occupied
    assert!(!dests.contains(&keyroost_piv::Slot::retired(1).unwrap()));
    assert!(dests.contains(&keyroost_piv::Slot::retired(2).unwrap())); // empty
    assert!(dests.contains(&keyroost_piv::Slot::Signature)); // empty standard
}
```

- [ ] **Step 2: Verify fail** — `cargo test -p keyroost --offline move_key_destinations` and the two extended enum tests → FAIL.

- [ ] **Step 3: Implement**

- Add a pure helper (near the other pure PIV helpers) computing eligible destinations:

```rust
/// Slots a key may be moved to: every standard + retired slot that is empty
/// and is not the source. `occupied(slot)` reports current key presence.
fn move_key_eligible_destinations(
    src: keyroost_piv::Slot,
    occupied: &dyn Fn(keyroost_piv::Slot) -> bool,
) -> Vec<keyroost_piv::Slot> {
    keyroost_piv::Slot::all()
        .into_iter()
        .chain(keyroost_piv::Slot::retired_all())
        .filter(|&s| s != src && !occupied(s))
        .collect()
}
```

- Add `PivCredKind::MoveKey`; fill its arm in `title()` ("Move key"), `submit_label()` ("Move"), `busy_label()` ("Moving key…"), `needs_mgmt_key()` (include it — move needs mgmt-key auth), `piv_cred_mismatch` (no mismatch check → `None` arm), `piv_cred_success` ("Key moved"), and submit dispatch (`PivCredKind::MoveKey => self.piv_move_key()`).
- `PivState`: add a `move_dest: Option<keyroost_piv::Slot>` field (declared 1031–1100, defaulted 1123–1159) for the chosen destination, and a `retired_expanded: bool` for the collapsible section.
- `render_piv_cred_modal` MoveKey arm (mirror DeleteKey 10434): a warning-free label naming source→dest, a destination `egui::ComboBox` populated from `move_key_eligible_destinations(selected, &occupancy_closure)`, then the mgmt-key field via `piv_modal_mgmt_field`, then a `card_note` "the certificate stays in the source slot".
- `piv_move_key` submit method (mirror `piv_delete_key` 5543–5570): reader + `piv_current_mgmt_key()` + `selected_slot.to_slot()` source + `move_dest` destination; spawn a job that opens `PivSession`, authenticates mgmt, calls `move_key(src, dest)`, re-reads `status()`, routes through `apply_piv_write`/`apply_piv_cred_result`. Device-bound (`completion_still_valid`) like the other PIV jobs.
- Pane: a "Move key…" button on the per-slot action card (near Delete, 11802), enabled only when the selected slot holds a key; sets `open_move_key = true` applied to open the MoveKey modal (mirror `open_delete_key` at 11923).
- Retired-slots section: below the four-slot tab strip (11567), a collapsible "Retired slots" header; when expanded, lazily populate occupancy via `slot_has_key` for the 20 retired slots (call once on expand, cache in `PivState`), render occupied ones as selectable rows and reveal empties when a move is in progress.

Keep pixel-level polish minimal — the v0.7.7 UI-polish pass refines it.

- [ ] **Step 4: Verify pass** — `cargo test --workspace --offline`, then `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo fmt --all --check`, and `cargo build --release -p keyroost --offline` (PATH symlink).

- [ ] **Step 5: Commit**

```bash
git add crates/keyroost/src/main.rs
git commit --no-gpg-sign -m "feat(gui): PIV move-key flow + collapsible retired-slots section

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: Docs

**Files:** `CHANGELOG.md` (`[Unreleased]`), `README.md` (piv command list), `TODO-v0.7.5.md` (hardware list).

- [ ] **Step 1: Edits**

CHANGELOG `### Added` under `[Unreleased]`:

```markdown
- **PIV move-key:** relocate a private key between slots (`keyroostctl piv
  move-key --from <slot> --to <slot>` and a GUI "Move key…" action), including
  the 20 Yubico retired key-management slots (82–95) for key archival /
  rotation. Non-destructive — refuses an occupied destination; the certificate
  stays in the source slot. Requires firmware 5.7+.
```

README: add `piv move-key` to the PIV command list next to delete-key.

TODO-v0.7.5 hardware list:

```markdown
- [ ] **PIV move-key (v0.7.7):** on the YubiKey 5.7, a rotate-and-archive
      round trip — move the Key-Management (9D) key to a retired slot, confirm
      it's gone from 9D and present in the retired slot, confirm the cert
      stayed in 9D, and that an occupied-destination move is refused. GUI:
      retired-slots section shows the archived key.
```

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md README.md TODO-v0.7.5.md
git commit --no-gpg-sign -m "docs: PIV move-key — changelog, README, deferred hardware check

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** retired slots first-class (Task 1) ✅; move byte builder (Task 2) ✅; transport pre-checks — same-slot, fw 5.7, occupied-destination via GET METADATA, key-only move (Task 3) ✅; CLI `move-key --from/--to`, no `--yes`, hex slot names incl. 82–95, cert-stays note (Task 4) ✅; GUI move flow + destination picker (empty only) + collapsible retired section + lazy occupancy (Task 5) ✅; docs + flagship hardware item (Task 6) ✅; fallback-to-C and future-goal-B are spec notes, not code, so no task.
- **Placeholder scan:** the grep-for-signature notes in Tasks 3–5 name real existing functions (slot_key/metadata, piv_delete_key, the DeleteKey modal arm) with adapt-to-reality instructions — not vague placeholders.
- **Type consistency:** `Slot::Retired(u8)`, `Slot::retired`/`retired_all`, `move_key(src,dest)`, `PivSession::move_key`/`slot_has_key`, `move_key_supported`, `move_key_eligible_destinations`, `PivCredKind::MoveKey` names are consistent across tasks. `label()` return-type change (→ String) is flagged in Task 1 with its call-site ripple.
- **Blast-radius (from recon):** the 3 compile-forced arms (key_ref/cert_object_tag/label) are all in Task 1; `Slot::all()` deliberately stays `[Slot;4]` so status() stays cheap, with `retired_all()` added for the lazy path; the parallel enums (CliPivSlot, PivSlotSel, PivCmd, PivCredKind) are extended deliberately in their tasks.
