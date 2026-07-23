# v0.7.5 TODO

Deferred items that are too large or too risky to fold into a same-day patch.
Captured here so they don't get lost. Unchecked = not started.

## v0.7.7 — new items from the v0.7.6 release run

- [ ] **Full README + Pages documentation audit before the v0.7.7 release.**
      A lot has shipped since the docs were last swept (factory reset, PIV
      move-key incl. retired slots, SSH-cert extract, the OATH applet reset,
      the v0.7.6 fixes), so expect a fair number of stale items. Go through:
      the root `README.md` (feature list, the CLI command reference / all
      `keyroostctl` subcommands incl. the new `piv move-key`, `factory-reset`,
      `fido ssh-cert`, `oath reset`; install instructions incl. the libpcsclite
      note for tarball/binstall users; the Windows signed-binary framing); and
      the GitHub **Pages / Learn site** (served from `docs/`). Check for:
      commands that no longer exist or were renamed, missing new commands,
      outdated screenshots, version numbers, stale capability claims, and dead
      links. Cross-check against `cargo run -p keyroostctl -- --help` and the
      GUI so nothing documented is gone and nothing shipped is undocumented.
- [x] **Release-attach retry window widened to 20×30s — DONE, on main.** Both
      attach loops (AppImage and .flatpak) retry 6×20s waiting for release.yml
      to create the GitHub Release; v0.7.6 lost that race by 16 seconds (the
      AppImage was left as a workflow artifact until a manual re-run). Stretch
      to ~20×30s — `--clobber` makes waiting free, and ten minutes covers any
      realistic gap between the two workflows' builds and approval gates.
- [x] **OATH applet reset — DONE, on main** (keyroostctl oath reset + GUI reset card + OathSession::factory_reset). The
      capability-gating audit behind the #81 fix found the OATH pane's locked
      view dead-ends at the password prompt, and keyroost implements no OATH
      reset at any layer — so a forgotten password currently means reaching
      for ykman. The Yubico/Trussed OATH RESET instruction deliberately works
      without the password (it wipes all credentials; that's the recovery
      trade-off). Add it through the stack: keyroost-oath byte layer,
      transport, `keyroostctl oath reset` (destructive-confirm conventions),
      and a reset card on the OATH pane's locked view — same never-hide-the-
      recovery-action principle as the FIDO reset fix in 0.7.6.
- [ ] **#82 follow-up: native support for the no-status-word HID dialect.**
      v0.7.6 ships the same-device HID→CCID fallback, which unbreaks the
      affected keys; the underlying firmware answers GET_INFO over HID with
      `80 bf 00 01 05` and no trailing `90 00`. Once the reporter names the
      model (asked in the issue), check with Token2 what that dialect is and
      whether the HID path should recognize it directly — vendor input first,
      no empirical probing (the Solo 2 HOTP lesson).

### Factory-reset fix-later batch (from the `feat/factory-reset` final review)

These are the defer-grade findings the whole-branch review surfaced; the
feature shipped without them (none block correctness or the wrong-key-safety
core). Fold into a follow-up on the factory-reset branch or a later pass.

- [x] **GUI OTP reset step now has the #82 HID→CCID fallback** (fix/factory-reset-batch).
      In `run_card_reset_step` (crates/keyroost/src/main.rs) the OTP step opens
      only the HID path when present, while the CLI's `open_otp` uses the
      `HidThenReader` same-device fallback. On the quirky #82 firmware the GUI
      factory reset reports OTP `Failed` where the CLI succeeds. Continue-on-
      error still holds; this is a cross-frontend inconsistency, not a wipe
      hazard. Give the GUI step the same fallback.
- [x] **CLI factory-reset now refuses a `--reader` + `--device` conflict** (fix/factory-reset-batch). The card
      steps resolve `reader.or(by_name)`, so passing both contradictorily opens
      the `--reader` key while the banner prints the `--device` serial.
      `resolve_fido_path` already rejects the analogous `--path`+`--device`
      combo; the card path should refuse the conflict too rather than pick one.
- [x] **Failed PIV RESET after a block now says the card is recoverable** (fix/factory-reset-batch). When
      `force_reset` blocks the PIN and PUK but the final RESET then fails, the
      report row says only `PIV failed: <e>`. The card is NOT bricked — that
      exact blocked state is what `keyroostctl piv reset` needs — but the
      message should say so (append "PIN and PUK are now blocked; recover with
      `keyroostctl piv reset`") so the state is fully honest. Also soften the
      GUI's "Live per-step report" comment (it populates on completion, not
      streaming) and have the CLI's "no resettable applet" error on a Molto2
      hint at `keyroostctl molto reset`.

## Release-day playbook (SOP)

- [x] Write a **release-day playbook** — a single checklist doc the release
      runs from, start to finish. Releases so far have been ad-hoc; as the UI
      matures the cut needs to be boring and repeatable. Should cover, in
      order, at least:
  - pre-flight: clean main, CI green, `cargo audit` green, deps-outdated scan
    reviewed;
  - **packaging test branch**: flatpak + AppImage built green BEFORE the tag
    (rule already in CLAUDE.md "Release process");
  - new-crate check: any crate added since the last release needs its one-time
    manual `cargo publish` + Trusted Publishing entry first;
  - version bump + changelog + tag (signed, `v*` tags are admin-only);
  - `release.yml` → `publish.yml` fanout watch (GH Release artifacts, crates.io
    OIDC job, env approval). **Verify each channel actually PUBLISHED, not just
    that the job went green** — the crates.io / Homebrew / winget / AUR jobs
    skip-with-a-notice (and still exit `success`) when their secret is absent, so
    a green check can mask a no-op. Confirm concretely: crates.io shows the new
    version, the Homebrew tap formula bumped, and a winget PR was opened. winget
    needs the `WINGET_TOKEN` secret (classic PAT, `public_repo` scope, held by
    the maintainer — it **silently skips again when the PAT expires**); AUR
    (`keyroost-bin`) went live with v0.7.5 — confirm the package version bumped;
  - **signed binaries (manual, Token2)**: Token2 signs the Windows + macOS
    builds on their DigiCert hardware token, which **cannot be automated** in
    CI (physical token access required; see #77). So after the release is cut,
    obtain their signed builds, attach them as the recommended Windows/macOS
    assets, and keep keyroost's own attested CI builds alongside — label
    signed-vs-attested in the release notes so users know who signed what.
    Folds into the pipeline if/when keyroost gets its own signing identity.
  - post-release: install-matrix spot check (`cargo install`, flatpak,
    AppImage, Homebrew tap, winget manifest refresh), GUI/CLI version sanity;
  - announcement/notes if any.
- [x] Decided + written (v0.7.7): it lives at `packaging/RELEASING.md`. The
      pre-tag packaging proof already became a workflow-dispatch dry-run
      (the linux-bundles build-only probe); the rest stays prose because the
      remaining steps are approvals and out-of-band vendor loops.

## Windows signing + winget flow (Token2 signed assets)

The v0.7.5 winget PR ([microsoft/winget-pkgs#402508](https://github.com/microsoft/winget-pkgs/pull/402508))
hit `Validation-Defender-Error` on the unsigned CI zip; a false-positive
report was filed with Microsoft (WDSI, developer queue).

Root-cause note (researched 2026-07-17): no "unsigned variant of
normally-signed software" heuristic exists in Defender/SmartScreen —
reputation keys on signing-cert identity and per-file hash only, so
Token2's signed builds neither hurt nor help our CI zip. The v0.7.5
failure was the well-documented prevalence-based false positive on any
brand-new low-prevalence binary (winget's own docs: reproduce locally,
report to WDSI, or wait for a pipeline re-run; it can hit signed
binaries too). Holding winget for the signed zip is still the right
call — signed bytes carry SmartScreen reputation across releases and the
submission moves off the release-day critical path — it just doesn't
*guarantee* validation passes.

Decision: keep signing with Token2 (no own signing identity for
now — Azure Artifact Signing / Certum would put the maintainer's legal
name in the cert CN; SignPath OSS rejected the project as too new,
re-apply in 6–12 months).

- [ ] **Signed-asset convention:** when Token2 returns signed builds,
      attach them as NEW assets (`keyroost-vX.Y.Z-windows-x86_64-signed.zip`
      + `.sha256` sidecar). **Never replace** the CI-built assets — that
      would invalidate `SHA256SUMS`, the provenance attestations, and any
      open winget PR's hash. The two variants have complementary trust
      chains (CI provenance vs Authenticode); label them in release notes.
- [x] **Rework the `publish.yml` winget job** — RESOLVED (branch
      `feat/winget-signed-flow`, 2026-07-17): the job submits only the
      `-signed.zip` (Authenticode-verified first, signer logged), skips
      with a notice when it isn't attached yet, no-ops when the version is
      already live in winget-pkgs (so re-dispatching publish.yml is safe),
      and an expired `WINGET_TOKEN` now FAILS loudly instead of skipping
      silently. On-demand path = existing `publish.yml` dispatch: attach
      the signed asset, `gh workflow run publish.yml -f tag=vX.Y.Z`.
- [x] **Explore: hold the winget submission for the Token2-signed binary**
      — DECIDED 2026-07-17: **always wait for signed** (encoded in the
      rework above). Rationale: signed bytes carry SmartScreen cert
      reputation across releases (better for users); the submission moves
      off the release-day critical path (Defender validation FPs self-heal
      by re-run and can hit signed binaries too, so decoupling — not
      signing — is the real fix); and winget publishes no install stats,
      so a few days' lag has no measurable adoption cost. Submitting
      unsigned-first-then-refresh was rejected: it doubles validation
      exposure (two PRs per release).
- [ ] **Manual fallback from Linux:** `komac update Framefilter.Keyroost
      --version <V> --urls <signed-asset-url> --submit` (komac is the
      Rust winget-manifest tool; wingetcreate is Windows-only).
- [x] **v0.7.5 specifically: RESOLVED without author action** — a winget
      moderator reran validation on 2026-07-15, it passed on updated
      Defender definitions (transient false positive), and #402508 merged;
      0.7.5 is live in the catalog serving the unsigned CI zip. The
      signed-asset flow above starts with the next release. Still do NOT
      host Token2's signed **v0.7.4** — it predates the v0.7.5 security
      fixes; ask them to sign v0.7.5 (or whatever is current) instead.
- [ ] **README + pages once signed assets exist:** point the Windows
      direct-download instructions at the signed asset and add a short
      note that signed Windows builds may trail the release by a few days
      (vendor signs out-of-band on a hardware token). Softer framing than
      originally planned — winget shipped v0.7.5 unsigned after moderator
      re-validation, so the delay affects the signed download, not winget
      availability.

## Embed the Windows app icon in the GUI build

Token2's signed v0.7.5 `keyroost.exe` carries an app icon they injected
during signing (an added `.rsrc` ICON/GROUP_ICON section); our CI GUI
binary ships with none. Nice addition — we should own it so it's in
**every** `keyroost.exe` (CI, `cargo install`, and future signed builds
become our-exact-bytes + signature with nothing injected).

- [x] Done (v0.7.7): `winresource` build-dep host-gated to Windows in the
      `keyroost` crate; `packaging/icons/gen-ico.py` (stdlib) packs the
      hicolor PNGs into the committed multi-resolution
      `packaging/icons/keyroost.ico`, embedded via `build.rs`.
- [x] VS_VERSION_INFO embedded alongside (ProductName / FileDescription /
      OriginalFilename; versions track CARGO_PKG_VERSION).
- [ ] Verify on a Windows build that the icon shows in Explorer/taskbar
      (compile-proof also pending: CI's Windows leg runs on main/PRs, so the
      first Windows build of build.rs happens at landing);
      confirm the Linux/macOS builds are unaffected (build-dep is
      cfg-gated). Once landed, future signed GUI binaries should be
      byte-identical to CI + signature (no injected `.rsrc`).

## Hardware verification pass for the v0.7.5 security work

The v0.7.5 remediation shipped with all automated gates green, but the
plan's manual two-key hardware steps were deliberately deferred (no keys
in-session). Run these with the disposable test keys before building
anything big on top:

- [ ] **Wrong-device bindings (GUI, Tasks 15–18):** with two keys plugged
      in, switch the selection mid-operation and confirm the stale
      completion is discarded for: Molto2 session open/write, the FIDO
      advanced dialog (typed PIN must die with the dialog), OATH delete
      confirmation, OpenPGP reset modal, fingerprint enroll, large-blob ops.
- [ ] **Armed FIDO reset (Task 19):** arm reset for key A, replug key A →
      fires; arm for key A, insert same-model key B during the window → must
      NOT fire; a serial-less key must refuse to arm (points at the CLI flow).
- [ ] **OATH password carry (Task 17):** on a password-protected key,
      unlock + list, then "Read code" on an HOTP credential — must succeed
      without retyping; switch devices and confirm the retained password is
      dropped.
- [ ] **CLI targeting (Tasks 21–23):** with two OTP-capable keys,
      `keyroostctl --device <name> otp list` / `molto info` hit the named
      key only; ambiguous/duplicate-serial setups fail closed with the
      ambiguity error.
- [ ] **GUI OTP pane binding (Task 31):** two OTP-capable keys, confirm
      list/add/delete operate on the selected key only, and the fail-closed
      error shows when the transport pick can't be satisfied.
- [ ] **Duplicate-serial advisory (Task 32):** if two same-serial keys are
      available, confirm the sidebar advisory appears and both keys stay
      separately selectable.
- [ ] **Linux hidraw bounded reads (Task 8):** unplug a key mid-`fido info`
      — the command must error out within the read budget, not hang.
- [ ] **OATH applet reset (new in v0.7.7):** on a password-PROTECTED test
      key, reset from the GUI locked view and via `keyroostctl oath reset
      --yes`; confirm credentials wiped, password cleared, and the pane
      re-lists empty and unlocked.
- [ ] **Factory reset (all applets), flagship (v0.7.7):** one full run on a
      disposable multi-applet test key — confirm each applet's reset fires in
      order, the PIV PIN+PUK retry burn completes and RESET succeeds, the FIDO
      replug+touch finale works, and the per-step summary matches reality
      (including a deliberately-induced mid-sequence failure showing
      continue-on-error).
- [ ] **v0.7.6 field fixes:** Settings tab + Reset on a CTAP 2.0-only key
      (#81 — reporter's device class), and the standalone Reset card on a
      key with a blocked/absent PIN. (#82's HID→CCID fallback only engages
      on the quirky firmware — reporter confirmation stands in for local
      hardware.)
- [ ] **PIV move-key (v0.7.7):** on the YubiKey 5.7, a rotate-and-archive
      round trip — move the Key-Management (9D) key to a retired slot, confirm
      it's gone from 9D and present in the retired slot, confirm the cert
      stayed in 9D, and that an occupied-destination move is refused. GUI:
      retired-slots section shows the archived key.
- [ ] **SSH-cert extract, interop proof (v0.7.7):** on the YubiKey 5.7, store
      a cert with `fido2-token -S -b -n ssh:… cert.pub`, extract it with
      `keyroostctl fido ssh-cert extract` (and the GUI), and confirm the
      output -cert.pub is byte-identical to the original — the real
      cross-implementation interop check the round-trip KAT can't provide.
      Also verify extraction of a cert stored by an OLDER libfido2/fido2-token
      (pre-~2021), which wrote ZLIB-wrapped (RFC 1950) largeBlob data rather
      than raw DEFLATE — keyroost's `inflate_raw` accepts raw only, so an
      old-format blob won't extract. Confirm whether any target keys carry
      old-format blobs and decide whether to also accept zlib (as libfido2's
      reader does).
- [ ] **Card-content identity (#83), v0.7.7:** with a Token2 PIN+ smartcard in
      a GENERIC reader (Alcor/SCM/Realtek), confirm keyroost shows vendor
      "Token2" and the FULL serial (not the 8-digit one); in the Token2 dual
      reader it still does; a non-Token2 OpenPGP card (e.g. Nitrokey) shows its
      correct registry vendor name; a model that rejects GET_INFO over contact
      falls back to the 8-digit serial with no error.

## PC/SC: load libpcsclite at runtime, degrade gracefully (the real #47 fix)

- [ ] Stop hard-linking libpcsclite; **`dlopen` it at runtime** in
      `keyroost-transport` (and wherever the `pcsc` crate is used), so:
  - the **host's** libpcsclite is always used — the only client guaranteed to
    match the host's `pcscd` daemon (fixes the version-mismatch root cause of
    [#47](https://github.com/framefilter/keyroost/issues/47) for **every**
    distribution channel, not just the AppImage); and
  - when libpcsclite is **absent**, keyroost still launches and FIDO/USB-HID
    keeps working — the PC/SC panes show a clear "PC/SC unavailable" state
    instead of the binary failing to start.
- [ ] This **removes the AppImage limitation** noted in
      `packaging/appimage/build-appimage.sh` (the 0.7.x AppImage drops the
      bundled libpcsclite and so needs the host's to even launch).
- [ ] The `pcsc` crate links at build time; check whether it exposes a
      dlopen/dynamic-load path or whether we wrap libpcsclite via a thin FFI
      loader ourselves. Design before implementing; verify on a host WITH and a
      host WITHOUT libpcsclite.

## egui / eframe version bump — **DONE**

- [x] Bump egui / eframe — shipped: the GUI is on **eframe/egui 0.35**
      (crate `rust-version` 1.92). Overtaken since this item was written
      (it targeted 0.34.3 as "current latest").
- [x] Wayland text-input regression
      [#48](https://github.com/framefilter/keyroost/issues/48) — closed.

## Molto2 — slot overview (titles, occupancy, per-slot delete)

Superseded by `docs/superpowers/specs/2026-07-03-molto2-slot-overview-design.md`
and its implementation plan. The old read-back assumption here was wrong:
hardware probing found `80 41 00 <profile> 01 70` returns title, occupancy,
and config in the clear (no key), and `80 E6 00 <profile> 00` deletes a
seed keylessly. Wire format now in `docs/PROTOCOL.md`.

## Hygiene follow-ups from the slot-overview branch

The user reviewed the branch's review findings and chose which to fix now vs
defer. Fixed on-branch: serial sanitization in the refusal messages, the
PROTOCOL empty-slot note, and the GUI slot-list refresh (factory reset clears
the stale list; a write re-sweeps when the list was blank). Promoted to its
own follow-up branch: the EPIPE panic. Remaining deferred items below.

- [ ] `impl std::error::Error for PublicDataError` so
      `TransportError::PublicData` chains via `source()` like its
      OATH/OpenPGP siblings.
- [ ] `molto slots`: on a mid-sweep read failure, print the slots already
      read plus an error row instead of aborting the whole command.
- [x] Repo-wide: keyroostctl panicked on EPIPE when stdout was piped to
      `head`/early-closing consumers. **Fixed** on `fix/cli-broken-pipe` via a
      panic hook that intercepts the broken-pipe panic and exits 0, guarded by
      `tests/broken_pipe.rs`. See the stabilization-watch item below for the
      cleaner replacement.
- [ ] **Watch for stable Rust to land the SIGPIPE fix and swap the workaround
      out.** The clean fix — libstd resetting `SIGPIPE` to `SIG_DFL` itself, no
      `unsafe` and no dep in our code — exists only on nightly today as the
      `-Zon-broken-pipe=kill` compiler flag (formerly the `#[unix_sigpipe]`
      attribute; tracking issue rust-lang/rust#97889, Unstable Book:
      `compiler-flags/on-broken-pipe`). When it (or an equivalent) reaches
      **stable**, delete `install_broken_pipe_guard()` in `keyroostctl/src/main.rs`
      and its `tests/broken_pipe.rs` guard, and adopt the built-in. Check
      periodically — it will leave nightly eventually. (Same applies to the
      `keyroost` GUI binary if it ever grows piped stdout.)
- [ ] GUI (optional, user's call): an explicit "Refresh slots" control by the
      slot-list header for on-demand re-read — deferred to avoid worsening the
      already-crowded six-button action row.

## GUI — Text-size control polish ([#42](https://github.com/framefilter/keyroost/issues/42), @token2) — **DONE**

- [x] Discrete "−" / "+" steppers flank the zoom slider (with a
      preview-then-commit debounce so the buttons don't rescale out from
      under a run of clicks). #42 is closed.
- [x] Light theme slider visibility — fixed with the #59 light-mode pass
      (dedicated darker control gray for the rail/handle/steppers on light;
      see the styling comment at the slider). #59 is closed.
