# largeBlob SSH-certificate extract — design

Decided with the user, 2026-07-18. Extract an OpenSSH certificate stored in a
FIDO2 key's per-credential largeBlob (the interoperable `fido2-token` scheme)
and write it to a standard `-cert.pub` file. Read/extract only — the write
side stays with `fido2-token` for now (option A, full read+write interop, is a
recorded future goal).

## Background — the interop reality (researched 2026-07-18)

Yubico's documented workflow stores the cert with
`fido2-token -S -b -n ssh:demo id_ecdsa-cert.pub`: the cert is **DEFLATE-
compressed, then AES-256-GCM encrypted with the resident SSH credential's
largeBlobKey**, and stored as a CTAP largeBlob-array entry associated with
that credential (CTAP §6.10). This is per-credential encrypted, not plaintext.

keyroost today: its largeBlob *write* path stores plaintext "notes" (zero
nonce), and its `ssh_cert` display parses entries as plaintext — so it only
decodes keyroost's own note writes and would show a real `fido2-token` cert as
an opaque encrypted blob. keyroost-ctap has **none** of the three pieces the
real scheme needs: no largeBlobKey getAssertion extension, no AES-256-GCM, no
DEFLATE. This feature builds the read/decrypt front-end so the existing
`ssh_cert` parser can handle real, standard-tool-written certs.

## Decisions log

| Question | Decision |
|---|---|
| Ambition level | **B — extract-only interop.** Read the real per-credential scheme; no write side. Full read+write (A) is a recorded future goal. |
| What extraction produces | **A — just the SSH certificate**, written as a standard OpenSSH `-cert.pub` file (optionally echoed to stdout). `ssh-keygen -K` already recovers the key/pubkey; keyroost fills the one gap — the cert. |
| CLI + GUI surface | New `keyroostctl fido ssh-cert {list,extract}` subgroup (SSH-specific, distinct from raw `fido large-blob`); GUI per-credential "Save certificate…" action in the FIDO2 Storage tab. Credential-first. |
| Multiple resident SSH creds | Enumerate; identify SSH creds by the OpenSSH `ssh:…` RP-ID convention; user picks (`--credential <rp-id>` or interactive; GUI list). Fail closed on ambiguity. |
| Dependencies (user-approved) | `aes-gcm` (RustCrypto, AES-256-GCM decrypt) + `miniz_oxide` (pure-Rust DEFLATE inflate), scoped to keyroost-ctap. Inflate is mandatory (the scheme compresses before encrypting). |
| Output-file safety | Write `<name>-cert.pub` (OpenSSH format); refuse to overwrite an existing file unless `--force` (CLI) / explicit confirm (GUI); default path from the credential name, override with `--out`. |

## Extraction pipeline (the core)

Credential-first, because entries are per-credential encrypted:

1. **Enumerate** resident credentials (existing cred-mgmt); keep those whose
   RP ID matches the OpenSSH SSH convention (`ssh:*`).
2. **Select** one (explicit `--credential`/GUI pick; interactive/error on
   ambiguity).
3. **getAssertion with the `largeBlobKey` extension** on that credential —
   requires PIN/UV + a touch, prompted like other FIDO ops. The authenticator
   returns the credential's 32-byte largeBlobKey.
4. **Read** the world-readable largeBlob array (no PIN needed to read the raw
   array; the existing large_blobs `get` path already reassembles + verifies
   the checksum).
5. **Trial-decrypt** each array entry with the largeBlobKey (AES-256-GCM;
   nonce = entry field 2, AAD per CTAP §6.10.4 = `blob` context || uint64LE
   origSize). The entry whose GCM tag authenticates is this credential's blob.
6. **Inflate** the decrypted bytes (DEFLATE, `miniz_oxide`); `origSize` (entry
   field 3) is the expected uncompressed length — validate against it.
7. **Parse** the inflated bytes with the existing `ssh_cert` parser. If it is
   a valid OpenSSH certificate, **write** `<name>-cert.pub`.

Honest failures at each stage: no SSH credential found; no entry decrypts
(nothing stored for this credential); decrypts but isn't a certificate (some
other tool's data).

## New capability: largeBlobKey getAssertion extension (keyroost-ctap)

Add `largeBlobKey` extension support to the getAssertion request/response:
request `{ "largeBlobKey": true }` in the extensions map; parse the returned
32-byte key from the assertion response. Scoped, additive — no change to the
existing assertion callers.

## Crypto (keyroost-ctap, new deps)

- **AES-256-GCM** via `aes-gcm` (RustCrypto). Decrypt-only usage here.
- **DEFLATE inflate** via `miniz_oxide`. Decompress-only.
- KAT tests for both against known vectors, and an end-to-end fixture: a
  captured real `fido2-token`-written entry + its largeBlobKey → expected
  plaintext cert (see Testing).

## CLI

```
keyroostctl fido ssh-cert list [--path <dev>]
    # resident SSH credentials (ssh:* RP IDs) and whether each has a cert blob

keyroostctl fido ssh-cert extract [--credential <rp-id>] [--out <file>] [--force] [--path <dev>]
    # authenticate (touch + PIN), decrypt, inflate, parse, write <name>-cert.pub
```

- `--credential` selects among multiple SSH creds; interactive pick or a
  fail-closed ambiguity error when omitted and several exist.
- Refuses to overwrite `--out` (or the derived default) unless `--force`.
- PIN via the existing `--pin-env`/`--pin-stdin` convention (never argv).

## GUI (FIDO2 Storage tab)

- Per SSH-credential entry: a **"Save certificate…"** action that runs the
  authenticate → decrypt → inflate → parse flow and opens a save dialog
  (native file chooser, `rfd`, already a dep) defaulting to `<name>-cert.pub`.
- Device-bound like the other FIDO flows (KEY-008 posture); touch/PIN prompts
  reuse the existing FIDO unlock machinery.
- On success, show the parsed cert summary (the existing `ssh_cert` display)
  next to the saved path.

## Error handling & safety

- The cert is public data — no secret-handling concerns on the cert itself;
  the PIN follows the existing never-in-argv rules.
- Refuse to overwrite an existing output file without `--force`/confirm.
- Bounded/validated: `origSize` caps the inflate output (reject a blob whose
  inflated size disagrees with `origSize`, so a hostile entry can't balloon
  memory); the largeBlob `get` path already enforces the array size cap and
  checksum.
- Never verify the CA signature (server's job — the existing `ssh_cert`
  module's stance, preserved).

## Testing

- **Pure/KAT:** AES-256-GCM decrypt vector; DEFLATE inflate vector; the
  trial-decrypt entry-matching logic (right entry authenticates, wrong key
  rejects); `origSize` mismatch rejected; cert `-cert.pub` formatting;
  largeBlobKey extension request/response encode/decode; CLI grammar
  (`--credential`/`--out`/`--force` decode).
- **End-to-end fixture (no hardware):** commit a real captured `fido2-token`
  largeBlob entry for a test SSH cert plus its largeBlobKey, assert the full
  pipeline reproduces the original `-cert.pub` byte-for-byte. This is the
  interop proof that doesn't need a key in-session.
- **Hardware verification (deferred session, flagship):** on the YubiKey 5.7,
  store a cert with `fido2-token -S -b -n ssh:… cert.pub`, extract with
  keyroost, confirm the output `-cert.pub` is byte-identical to the original.

## Out of scope (recorded)

- **The write side** — storing a cert into largeBlob from keyroost. `fido2-
  token` does this today; it becomes option **A** (full read+write interop)
  as a future goal, and reuses this feature's largeBlobKey + AES-GCM +
  DEFLATE plumbing plus a compress + AEAD-encrypt + write-state-machine.
- **Public-key reconstruction** from the resident credential — `ssh-keygen -K`
  already does this; keyroost only fills the cert gap.
- **CA signature verification** — never; that is the relying server's job.
