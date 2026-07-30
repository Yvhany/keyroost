# largeBlob SSH-cert extract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract an OpenSSH certificate stored in a FIDO2 key's per-credential largeBlob and write it to a standard `-cert.pub` file, per `docs/superpowers/specs/2026-07-18-ssh-cert-extract-design.md`. Read-only interop; the largeBlobKey comes from credentialManagement (not getAssertion).

**Architecture:** Extend keyroost-ctap's shipped credentialManagement to parse each credential's largeBlobKey (CTAP response key 0x0B). Add AES-256-GCM decrypt + raw-DEFLATE inflate helpers (crates already in the lockfile). An extract function ties them together: enumerate SSH creds → get the key → read the largeBlob array → trial-decrypt → inflate → parse → `to_cert_pub`. CLI `fido ssh-cert {list,extract}` and a GUI Storage-tab "Save certificate…" drive it. PIN only, no per-op touch.

**Tech Stack:** Rust workspace. New *direct* deps in keyroost-ctap: `aes-gcm` (0.10, already resolved via keyroost-import) + `miniz_oxide` (already resolved transitively). No new-to-the-tree crates.

## Global Constraints

- Commit UNSIGNED: `git commit --no-gpg-sign`, footer `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`. Never push main, never create tags. Guard-tripping messages → `git commit -F <scratch-file>`. Stage files EXPLICITLY by name (`git add <path>`) — never `git add -A`/`.` (untracked plan docs live in the tree).
- Gates before every commit: `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo fmt --all --check`, `cargo test --workspace --offline`. MSRV 1.85 libs/CLI, 1.92 GUI.
- Dependencies: only `aes-gcm` + `miniz_oxide` as new direct deps of keyroost-ctap, both already in Cargo.lock (aes-gcm via keyroost-import's `encrypted` feature; miniz_oxide transitively). Do NOT add anything else. Match aes-gcm's existing feature set (`default-features = false, features = ["aes", "alloc"]`).
- CTAP largeBlob AEAD (CTAP §6.10.4 — the implementer MUST verify each value against the spec): each array entry `{1: ciphertext(+16B GCM tag), 2: nonce(12B), 3: origSize(uint)}`. Plaintext = AES-256-GCM-decrypt(key = credential's 32-byte largeBlobKey, nonce, ciphertext, AAD = `b"blob"` ‖ origSize as 8-byte little-endian). Then raw-DEFLATE-inflate to origSize bytes.
- largeBlobKey is credentialManagement enumerate response key **0x0B** (0x0A is credProtect — do not confuse them).
- SSH credentials are identified by RP ID starting with `ssh:` (OpenSSH convention).
- Cert is PUBLIC data — no secret-zeroizing needed on the cert; PIN follows the existing never-in-argv `--pin-env`/`--pin-stdin` rules.
- Never verify the CA signature (server's job — the existing `ssh_cert` module's stance).
- Branch: `feat/ssh-cert-extract` (created off origin/main; the design spec is committed on it).

---

### Task 1: AES-256-GCM decrypt + raw-DEFLATE inflate helpers (keyroost-ctap)

**Files:**
- Modify: `crates/keyroost-ctap/Cargo.toml` (add the two deps)
- Modify: `crates/keyroost-ctap/src/large_blobs.rs` (add the two pure helpers + tests), or a new small `crates/keyroost-ctap/src/large_blob_crypto.rs` module — prefer adding to `large_blobs.rs` since that's where they're consumed.

**Interfaces:**
- Produces:
  - `pub(crate) fn gcm_decrypt(key: &[u8; 32], nonce: &[u8], ciphertext: &[u8], orig_size: u64) -> Option<Vec<u8>>` — AES-256-GCM decrypt with AAD = `b"blob"` ‖ `orig_size.to_le_bytes()`. Returns None if the tag fails.
  - `pub(crate) fn inflate_raw(compressed: &[u8], orig_size: u64) -> Option<Vec<u8>>` — raw DEFLATE inflate, bounded so output cannot exceed `orig_size`; returns None on malformed input or a length that disagrees with `orig_size`.

- [ ] **Step 1: Add deps**

In `crates/keyroost-ctap/Cargo.toml` `[dependencies]` (near the existing `aes`/`cbc`):

```toml
# Per-credential largeBlob AEAD (CTAP §6.10.4): AES-256-GCM. Already resolved
# in the workspace lockfile via keyroost-import's Aegis path; direct use here
# for reading a cert out of largeBlob storage.
aes-gcm = { version = "0.10", default-features = false, features = ["aes", "alloc"] }
# Raw-DEFLATE inflate for the compressed largeBlob plaintext. Pure Rust;
# already transitively in the tree.
miniz_oxide = "0.8"
```

Run `cargo build -p keyroost-ctap --offline` to confirm they resolve without a network fetch.

- [ ] **Step 2: Write failing tests**

Add to a `#[cfg(test)] mod` in `large_blobs.rs`:

```rust
#[test]
fn gcm_roundtrip_and_wrong_key_fails() {
    use aes_gcm::{aead::{Aead, KeyInit, Payload}, Aes256Gcm, Nonce};
    let key = [7u8; 32];
    let nonce = [3u8; 12];
    let plaintext = b"hello blob";
    let orig_size = plaintext.len() as u64;
    let mut aad = b"blob".to_vec();
    aad.extend_from_slice(&orig_size.to_le_bytes());
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad: &aad })
        .unwrap();
    // Right key decrypts.
    assert_eq!(gcm_decrypt(&key, &nonce, &ct, orig_size).as_deref(), Some(&plaintext[..]));
    // Wrong key fails the tag.
    assert_eq!(gcm_decrypt(&[9u8; 32], &nonce, &ct, orig_size), None);
    // Wrong orig_size changes the AAD → tag fails.
    assert_eq!(gcm_decrypt(&key, &nonce, &ct, orig_size + 1), None);
}

#[test]
fn inflate_raw_roundtrip_and_bounds() {
    let data = b"the quick brown fox ".repeat(50);
    let compressed = miniz_oxide::deflate::compress_to_vec(&data, 6);
    assert_eq!(inflate_raw(&compressed, data.len() as u64).as_deref(), Some(&data[..]));
    // Garbage input → None, not a panic.
    assert_eq!(inflate_raw(&[0xff, 0x00, 0x13, 0x37], 100), None);
    // A declared orig_size the inflated data doesn't match → None.
    assert_eq!(inflate_raw(&compressed, (data.len() + 1) as u64), None);
}
```

- [ ] **Step 3: Verify fail** — `cargo test -p keyroost-ctap --offline gcm_roundtrip inflate_raw` → FAIL (helpers missing).

- [ ] **Step 4: Implement**

```rust
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};

/// AES-256-GCM decrypt of one largeBlob entry (CTAP §6.10.4). AAD is
/// `b"blob"` followed by the 8-byte little-endian original (uncompressed)
/// size. `ciphertext` includes the trailing 16-byte GCM tag. Returns None on
/// any authentication failure — the caller trial-decrypts entries and treats
/// None as "not this credential's blob".
pub(crate) fn gcm_decrypt(
    key: &[u8; 32],
    nonce: &[u8],
    ciphertext: &[u8],
    orig_size: u64,
) -> Option<Vec<u8>> {
    if nonce.len() != 12 {
        return None;
    }
    let mut aad = Vec::with_capacity(12);
    aad.extend_from_slice(b"blob");
    aad.extend_from_slice(&orig_size.to_le_bytes());
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ciphertext, aad: &aad })
        .ok()
}

/// Raw-DEFLATE inflate (RFC 1951 — NOT zlib-wrapped) of the decrypted
/// plaintext, bounded by `orig_size`. Returns None on malformed input or when
/// the inflated length does not equal `orig_size`.
pub(crate) fn inflate_raw(compressed: &[u8], orig_size: u64) -> Option<Vec<u8>> {
    let limit = usize::try_from(orig_size).ok()?;
    // Use the RAW (non-zlib) limited inflate. Verify the exact miniz_oxide fn
    // name — `miniz_oxide::inflate::decompress_to_vec_with_limit` is raw
    // DEFLATE with a size cap (the `_zlib` variants expect a zlib header,
    // which the largeBlob plaintext does NOT have).
    let out = miniz_oxide::inflate::decompress_to_vec_with_limit(compressed, limit).ok()?;
    (out.len() as u64 == orig_size).then_some(out)
}
```

Implementation note: confirm the miniz_oxide raw-inflate function name and signature (`decompress_to_vec_with_limit` returns `Result<Vec<u8>, _>` in 0.8). If the raw variant differs, use the raw (non-`_zlib`) one and adapt the error handling to return None.

- [ ] **Step 5: Verify pass** — `cargo test -p keyroost-ctap --offline gcm_roundtrip inflate_raw`; `cargo build --workspace --offline`.

- [ ] **Step 6: Gates + commit**

```bash
cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check && cargo test --workspace --offline
git add crates/keyroost-ctap/Cargo.toml crates/keyroost-ctap/src/large_blobs.rs Cargo.lock
git commit --no-gpg-sign -m "feat(ctap): AES-256-GCM decrypt + raw-DEFLATE inflate for largeBlob reads

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Parse the per-credential largeBlobKey (keyroost-ctap)

**Files:**
- Modify: `crates/keyroost-ctap/src/cred_mgmt.rs` — the `Credential` struct (~107–113), the `RESP_*` const block (~47–55), `parse_credential` (~329–364); tests.

**Interfaces:**
- Consumes: existing CBOR decode + the enumerate response.
- Produces: `Credential` gains `pub large_blob_key: Option<[u8; 32]>`, populated from response key **0x0B**.

- [ ] **Step 1: Failing test**

Add to `cred_mgmt.rs` tests — a `parse_credential` case whose CBOR includes key 0x0B (a 32-byte bstr):

```rust
#[test]
fn parse_credential_reads_large_blob_key() {
    // Build the response map the authenticator returns for one credential:
    // 0x06 user, 0x07 credentialId, 0x0B largeBlobKey (32 bytes). Reuse the
    // existing test's map-building helper/shape; add the 0x0B entry.
    // (Grep the existing parse_credential test for the map construction and
    // extend it; assert the parsed Credential.large_blob_key == Some([..32]).)
    let key = [0x5Au8; 32];
    // ... construct Value::Map with (UInt(0x0B), Bytes(key.to_vec())) plus the
    // minimal required 0x07 credentialId ...
    // let cred = parse_credential(&map).unwrap();
    // assert_eq!(cred.large_blob_key, Some(key));
    // A response WITHOUT 0x0B → None.
}
```

(Fill in the map construction by mirroring the existing `parse_credential` test in the file — grep it first.)

- [ ] **Step 2: Verify fail** — `cargo test -p keyroost-ctap --offline parse_credential_reads_large_blob_key` → FAIL (no field).

- [ ] **Step 3: Implement**

- Add the const near the other `RESP_*` (verify 0x0B against the CTAP 2.1 credentialManagement enumerate response — 0x0A is credProtect, 0x0B is largeBlobKey):
  ```rust
  const RESP_LARGE_BLOB_KEY: u64 = 0x0B;
  ```
- Add the field to `Credential`:
  ```rust
  /// The credential's 32-byte largeBlobKey, when the authenticator returned
  /// one (present only for credentials created with the largeBlob extension).
  /// Used to decrypt this credential's per-credential largeBlob entry.
  pub large_blob_key: Option<[u8; 32]>,
  ```
- In `parse_credential`, read key `0x0B` as a 32-byte bstr → `[u8; 32]` (`try_into().ok()`), default None. Set the field on the returned `Credential`. Update any struct-literal construction of `Credential` elsewhere to include the field (grep for `Credential {` — the parse fn is likely the only builder).

- [ ] **Step 4: Verify pass** — `cargo test -p keyroost-ctap --offline` + `cargo build --workspace --offline` (fixes any other `Credential {` literal).

- [ ] **Step 5: Commit**

```bash
git add crates/keyroost-ctap/src/cred_mgmt.rs
git commit --no-gpg-sign -m "feat(ctap): parse the per-credential largeBlobKey from enumerateCredentials

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Extract orchestration + round-trip KAT (keyroost-ctap)

**Files:**
- Modify: `crates/keyroost-ctap/src/large_blobs.rs` (or a new `ssh_cert_extract.rs` module — prefer large_blobs.rs) — the pure decrypt-and-find function + tests.

**Interfaces:**
- Consumes: `gcm_decrypt`, `inflate_raw` (Task 1), `LargeBlobEntry` (fields `ciphertext`, `nonce`, `orig_size`), `ssh_cert::{parse_wire, to_cert_pub}`.
- Produces:
  - `pub fn extract_cert_from_entries(large_blob_key: &[u8; 32], entries: &[LargeBlobEntry]) -> Option<Vec<u8>>` — trial-decrypt each entry with the key; for the one that authenticates, inflate + verify it parses as an SSH cert; return the cert **wire bytes** (so callers can `ssh_cert::to_cert_pub` or inspect). None if no entry is this credential's, or it isn't a cert.

Keeping this a pure function over already-read entries (not doing device I/O) makes it fully unit-testable and keeps the transport/enumerate glue in the CLI/GUI layers.

- [ ] **Step 1: Failing round-trip KAT**

```rust
#[test]
fn extract_cert_from_entries_roundtrips_a_real_cert() {
    use aes_gcm::{aead::{Aead, KeyInit, Payload}, Aes256Gcm, Nonce};
    // A known-good OpenSSH cert fixture already exists for the ssh_cert tests.
    let cert_pub = crate::ssh_cert::tests_fixture::FIXTURE_CERT_PUB; // grep the real path/name
    let (_, wire) = crate::ssh_cert::parse_text(cert_pub.trim()).unwrap();

    // Build the largeBlob entry the way fido2-token would: raw-DEFLATE then
    // AES-256-GCM with AAD = "blob" || origSize LE.
    let key = [0x11u8; 32];
    let nonce = [0x22u8; 12];
    let orig_size = wire.len() as u64;
    let compressed = miniz_oxide::deflate::compress_to_vec(&wire, 6);
    let mut aad = b"blob".to_vec();
    aad.extend_from_slice(&orig_size.to_le_bytes());
    let ct = Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: &compressed, aad: &aad })
        .unwrap();
    let entry = LargeBlobEntry { ciphertext: ct, nonce: nonce.to_vec(), orig_size };
    // A decoy entry encrypted with a different key must be skipped.
    let decoy = LargeBlobEntry { ciphertext: vec![0u8; 40], nonce: vec![0u8; 12], orig_size: 8 };

    let got = extract_cert_from_entries(&key, &[decoy, entry]).expect("cert found");
    assert_eq!(got, wire);
    // Full round trip back to -cert.pub reproduces the original.
    assert_eq!(crate::ssh_cert::to_cert_pub(&got).unwrap().trim(), cert_pub.trim());
    // Wrong key → nothing.
    assert!(extract_cert_from_entries(&[0x99u8; 32], &[entry_clone]).is_none());
}
```

(Grep `crates/keyroost-ctap/src/ssh_cert.rs` for the real fixture name — the recon noted `ssh_cert::tests_fixture::FIXTURE_CERT_PUB` used by an existing large_blobs test at ~line 662. Reuse it. Clone the entry as needed for the wrong-key assertion.)

- [ ] **Step 2: Verify fail** — `cargo test -p keyroost-ctap --offline extract_cert_from_entries` → FAIL.

- [ ] **Step 3: Implement**

```rust
/// Find and decode this credential's SSH certificate from the largeBlob
/// array. Trial-decrypts each entry with the credential's largeBlobKey (the
/// GCM tag identifies the matching entry), inflates the raw-DEFLATE plaintext,
/// and returns the certificate **wire bytes** when the result parses as an
/// OpenSSH certificate. None when no entry is this credential's, or the
/// decrypted blob is not a certificate. Pure over already-read entries.
pub fn extract_cert_from_entries(
    large_blob_key: &[u8; 32],
    entries: &[LargeBlobEntry],
) -> Option<Vec<u8>> {
    for e in entries {
        let Some(plain) = gcm_decrypt(large_blob_key, &e.nonce, &e.ciphertext, e.orig_size)
        else {
            continue; // not this credential's entry (tag failed)
        };
        let Some(wire) = inflate_raw(&plain, e.orig_size) else {
            continue; // decrypted but not valid DEFLATE / size mismatch
        };
        if crate::ssh_cert::parse_wire(&wire).is_some() {
            return Some(wire);
        }
        // Decrypted + inflated but not a cert: it's this credential's blob but
        // holds non-cert data. Stop — this IS the credential's entry.
        return None;
    }
    None
}
```

- [ ] **Step 4: Verify pass** — `cargo test -p keyroost-ctap --offline`.

- [ ] **Step 5: Commit**

```bash
git add crates/keyroost-ctap/src/large_blobs.rs
git commit --no-gpg-sign -m "feat(ctap): extract an SSH cert from a credential's largeBlob entry

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: CLI `fido ssh-cert {list, extract}`

**Files:**
- Modify: `crates/keyroostctl/src/main.rs` — `FidoCmd` enum (~1633), a new `SshCert { cmd: SshCertCmd }` group + `SshCertCmd` enum, the dispatch (~5647 area), handler fns; `cli_tests`.

**Interfaces:**
- Consumes: `with_credential_manager` (~6623) pattern (open + PIN token + `CredentialManager`), `CredentialManager::{list_relying_parties, list_credentials}`, `Credential.large_blob_key`, `large_blobs::read` + `extract_cert_from_entries`, `ssh_cert::to_cert_pub`, `read_secret`, `resolve_fido_path`, `sanitize_terminal`.
- Produces: `keyroostctl fido ssh-cert list` and `keyroostctl fido ssh-cert extract [--credential <rp-id>] [--out <file>] [--force]`, each with `--pin-env`/`--pin-stdin` + `--path`.

- [ ] **Step 1: Failing grammar test**

```rust
#[test]
fn fido_ssh_cert_extract_grammar() {
    match parse(&["keyroostctl", "fido", "ssh-cert", "extract",
                  "--credential", "ssh:demo", "--out", "id-cert.pub", "--force", "--pin-stdin"])
        .unwrap().command
    {
        Some(Cmd::Fido { cmd: FidoCmd::SshCert { cmd: SshCertCmd::Extract {
            credential, out, force, pin_stdin, .. } } }) => {
            assert_eq!(credential.as_deref(), Some("ssh:demo"));
            assert_eq!(out.as_deref(), Some(std::path::Path::new("id-cert.pub")));
            assert!(force && pin_stdin);
        }
        _ => panic!("expected fido ssh-cert extract"),
    }
}
```

- [ ] **Step 2: Verify fail** — → FAIL (no variant).

- [ ] **Step 3: Implement**

- `SshCertCmd`:
  ```rust
  #[derive(clap::Subcommand)]
  enum SshCertCmd {
      /// List resident SSH credentials (ssh:* RP IDs) and whether each has a
      /// certificate stored in its largeBlob.
      List {
          #[arg(long, value_name = "VAR", conflicts_with = "pin_stdin")]
          pin_env: Option<String>,
          #[arg(long)]
          pin_stdin: bool,
          #[arg(long)]
          path: Option<std::path::PathBuf>,
      },
      /// Extract an SSH certificate from its largeBlob to a -cert.pub file.
      Extract {
          /// RP ID of the SSH credential (e.g. ssh:demo). Required only to
          /// disambiguate when several SSH credentials are present.
          #[arg(long)]
          credential: Option<String>,
          /// Output file (default: <rp-id-sanitised>-cert.pub).
          #[arg(long)]
          out: Option<std::path::PathBuf>,
          /// Overwrite the output file if it exists.
          #[arg(long)]
          force: bool,
          #[arg(long, value_name = "VAR", conflicts_with = "pin_stdin")]
          pin_env: Option<String>,
          #[arg(long)]
          pin_stdin: bool,
          #[arg(long)]
          path: Option<std::path::PathBuf>,
      },
  }
  ```
- Add `FidoCmd::SshCert { #[command(subcommand)] cmd: SshCertCmd }` and a dispatch arm `FidoCmd::SshCert { cmd } => run_fido_ssh_cert(cmd)`.
- `run_fido_ssh_cert`: read the PIN via `read_secret`, then open + build the `CredentialManager` (mirror `with_credential_manager`), enumerate RPs, keep those with `rp.id.starts_with("ssh:")`, and for each list its credentials.
  - **list:** print each SSH credential's RP ID (sanitised) and whether it has a `large_blob_key` AND a decodable cert (read the array once, run `extract_cert_from_entries` per credential; "cert stored" iff Some). Support `--json` if the file has a json_out shape (recon noted `FidoLargeBlobSshCertJson`).
  - **extract:** select the SSH credential — if `--credential` given, match its RP ID; else if exactly one SSH cred, use it; else error listing the choices (fail closed). Require `credential.large_blob_key` present (else "no certificate blob stored for this credential"). `read` the largeBlob array, `extract_cert_from_entries(&key, &entries)` → cert wire; `ssh_cert::to_cert_pub(&wire)`; resolve the output path (default `<sanitised rp-id>-cert.pub`), refuse to overwrite unless `--force`, write the file, print the saved path.

Grep the exact shapes of `with_credential_manager`, `list_relying_parties`/`list_credentials` returns, `large_blobs::read`'s signature, and `RelyingParty`/`Credential` fields before wiring; adapt to reality. If the enumerate path doesn't surface `large_blob_key` per credential cleanly (e.g. list_credentials drops it), STOP and report NEEDS_CONTEXT.

- [ ] **Step 4: Verify pass** — `cargo test -p keyroostctl --offline fido_ssh_cert` + full `cargo test --workspace --offline`.

- [ ] **Step 5: Commit**

```bash
git add crates/keyroostctl/src/main.rs
git commit --no-gpg-sign -m "feat(cli): fido ssh-cert list/extract — pull a cert out of largeBlob

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: GUI "Save certificate…" in the Storage tab

**Files:**
- Modify: `crates/keyroost/src/main.rs` — the Storage subview / `render_large_blobs` (~9388) or the credential list it can reach, a `FileTarget` variant (~1609), `drain_file_dialogs` (~1695), a `spawn_job`-driven extract, reusing `fido_session` (~2847) / `refresh_with_token` (~2884) shape.

**Interfaces:**
- Consumes: the cached `UnlockedSession` PIN token (`fido_session`), `CredentialManager`, `large_blobs::read` + `extract_cert_from_entries`, `ssh_cert::to_cert_pub`, `spawn_file_dialog`/`FileTarget`.
- Produces: a per-SSH-credential "Save certificate…" button that decrypts off the UI thread and opens a save dialog defaulting to `<name>-cert.pub`.

- [ ] **Step 1: Failing test** — a pure default-filename helper:

```rust
#[test]
fn ssh_cert_default_filename_sanitised() {
    assert_eq!(ssh_cert_default_filename("ssh:demo"), "ssh_demo-cert.pub");
    assert_eq!(ssh_cert_default_filename("ssh:"), "ssh-cert.pub");
    // control/path chars are stripped so the default can't escape a dir
    assert!(!ssh_cert_default_filename("ssh:../evil").contains('/'));
}
```

- [ ] **Step 2: Verify fail** — → FAIL.

- [ ] **Step 3: Implement**

- `ssh_cert_default_filename(rp_id) -> String`: sanitise the RP ID (replace `:` and any non-`[A-Za-z0-9._-]` with `_`, collapse) and append `-cert.pub`. Pure, tested.
- In the Storage subview, where credentials/entries render: for a credential whose RP ID is `ssh:*` and which has a cert stored, add a "Save certificate…" button (mirror the existing "Export…" button at ~9628, but this one runs a background job first because it needs device I/O + decrypt, then opens the save dialog).
- The job (mirror `refresh_with_token`/`refresh_credentials`, ~2884): take the session token, build a `CredentialManager`, get the credential's `large_blob_key`, `read` the array, `extract_cert_from_entries` → wire → `to_cert_pub` string, and stash the result; then open the save dialog (`spawn_file_dialog` with a new `FileTarget::SshCertSave { ... }`, default filename from the helper). The dialog resolution in `drain_file_dialogs` writes the string to the chosen path.
- Device-bound: guard the apply closure with `completion_still_valid` (or the FIDO-session equivalent used by the other FIDO jobs). PIN only, no touch.
- On success show the parsed cert summary (reuse the existing `ssh_cert`/`EntryKind::SshCert` display) beside the saved path.

Grep the real signatures (`spawn_file_dialog`, `FileTarget`, `drain_file_dialogs`, `fido_session`, `refresh_with_token`) and mirror them. If the Storage subview lists blob *entries* rather than *credentials* (so there's no per-credential button site), the button can live on the credential list in the FIDO2 pane instead — pick whichever surface already enumerates SSH credentials; state the choice in the report. If unworkable, STOP + NEEDS_CONTEXT.

- [ ] **Step 4: Verify pass** — `cargo test --workspace --offline`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo fmt --all --check`, `cargo build --release -p keyroost --offline`.

- [ ] **Step 5: Commit**

```bash
git add crates/keyroost/src/main.rs
git commit --no-gpg-sign -m "feat(gui): Save certificate… — extract an SSH cert from largeBlob storage

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: Docs

**Files:** `CHANGELOG.md` (`[Unreleased]`), `README.md` (fido command list), `TODO-v0.7.5.md` (hardware list).

- [ ] **Step 1: Edits**

CHANGELOG `### Added` under `[Unreleased]`:

```markdown
- **Extract an SSH certificate from a FIDO2 key** (`keyroostctl fido ssh-cert
  extract` and a GUI "Save certificate…" action): pulls the OpenSSH
  certificate a tool like `fido2-token` stored in a resident SSH credential's
  largeBlob and writes it as a standard `-cert.pub` file — so the cert travels
  with the key. Read/extract only; PIN required, no touch.
```

README: add `fido ssh-cert list`/`extract` to the FIDO command list.

TODO-v0.7.5 hardware list:

```markdown
- [ ] **SSH-cert extract, interop proof (v0.7.7):** on the YubiKey 5.7, store
      a cert with `fido2-token -S -b -n ssh:… cert.pub`, extract it with
      `keyroostctl fido ssh-cert extract` (and the GUI), and confirm the
      output -cert.pub is byte-identical to the original — the real
      cross-implementation interop check the round-trip KAT can't provide.
```

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md README.md TODO-v0.7.5.md
git commit --no-gpg-sign -m "docs: SSH-cert extract — changelog, README, deferred interop check

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** AES-GCM + inflate helpers (Task 1) ✅; largeBlobKey via credMgmt key 0x0B (Task 2) ✅; credential-first trial-decrypt → inflate → parse pipeline (Task 3) ✅; CLI list/extract with `--credential`/`--out`/`--force`, fail-closed ambiguity, PIN-only (Task 4) ✅; GUI Save certificate…, spawn_job off-thread, default filename sanitised (Task 5) ✅; docs + flagship interop hardware item (Task 6) ✅; write side + getAssertion + pubkey-reconstruction + CA-verify all spec-recorded out-of-scope, no tasks.
- **Placeholder scan:** the crypto-value verify-notes (AAD, key 0x0B, miniz raw-inflate fn name) name exact spec values with "verify against spec" — not vague; the grep-for-signature notes name real existing functions. Task 2/4/5 tests have "grep the existing fixture/map shape" fill-ins because those depend on the current test helpers — acceptable, the concrete assertions are specified.
- **Type consistency:** `gcm_decrypt`/`inflate_raw`/`extract_cert_from_entries` signatures, `Credential.large_blob_key: Option<[u8;32]>`, `SshCertCmd::{List,Extract}` fields, `ssh_cert_default_filename` are consistent across tasks. `LargeBlobEntry` fields (`ciphertext`, `nonce`, `orig_size`) match the recon.
- **Risk notes:** the two crypto details most likely to bite — the AAD construction (`b"blob"` ‖ origSize LE) and the raw-vs-zlib inflate variant — are called out for spec verification in Task 1; the round-trip KAT (Task 3) catches an AAD/inflate mistake immediately (it round-trips through the real cert fixture). The one thing the KAT can't prove is cross-implementation interop (our encrypt == fido2-token's encrypt) — that's the deferred hardware item, explicitly flagged.
