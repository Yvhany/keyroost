# v0.7.5 TODO

Deferred items that are too large or too risky to fold into a same-day patch.
Captured here so they don't get lost. Unchecked = not started.

## Release-day playbook (SOP)

- [ ] Write a **release-day playbook** — a single checklist doc the release
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
- [ ] Decide where it lives (likely `packaging/RELEASING.md`) and whether any
      steps can become a workflow-dispatch dry-run instead of prose.

## Windows signing + winget flow (Token2 signed assets)

The v0.7.5 winget PR ([microsoft/winget-pkgs#402508](https://github.com/microsoft/winget-pkgs/pull/402508))
hit `Validation-Defender-Error` on the unsigned CI zip; a false-positive
report was filed with Microsoft (WDSI, developer queue). Working theory:
Token2's *signed* keyroost builds circulate widely, so Defender's
"unsigned variant of normally-signed software" heuristic trips on the CI
zip — which means this recurs every release until winget points at signed
binaries. Decision: keep signing with Token2 (no own signing identity for
now — Azure Artifact Signing / Certum would put the maintainer's legal
name in the cert CN; SignPath OSS rejected the project as too new,
re-apply in 6–12 months).

- [ ] **Signed-asset convention:** when Token2 returns signed builds,
      attach them as NEW assets (`keyroost-vX.Y.Z-windows-x86_64-signed.zip`
      + `.sha256` sidecar). **Never replace** the CI-built assets — that
      would invalidate `SHA256SUMS`, the provenance attestations, and any
      open winget PR's hash. The two variants have complementary trust
      chains (CI provenance vs Authenticode); label them in release notes.
- [ ] **Rework the `publish.yml` winget job:** prefer the `-signed.zip`
      asset when present; when absent, **skip cleanly instead of submitting
      the unsigned zip** (no more racing Defender at release time). Add a
      `workflow_dispatch` path so the winget leg runs on demand after the
      signed asset lands. New rhythm: tag → fanout (minus winget) → Token2
      signs at their pace → attach signed asset → dispatch winget.
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

- [ ] Add a `winresource` (or `winres`) build-dependency to the `keyroost`
      crate, gated to Windows targets, compiling a `.ico` into the binary
      via `build.rs`. Icon source already exists (`packaging/icons/`, and
      `Keyroost icon design.zip`); produce a multi-resolution `.ico`.
- [ ] Optionally embed `VS_VERSION_INFO` (product name, version, company)
      at the same time — the CI GUI exe currently has none.
- [ ] Verify on a Windows build that the icon shows in Explorer/taskbar;
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

## egui / eframe version bump

- [ ] Bump **egui / eframe / egui-winit 0.29.1 → 0.34.3** (current latest).
      Five minor versions of breaking API changes across the ~11k-line GUI —
      treat as its own project with a full pass + regression check (zoom/slider,
      modals, layout, light/dark themes).
- [ ] **winit stays 0.30.13** either way (0.31 is beta only; egui 0.34 still
      rides the 0.30.x line), so this is **not** guaranteed to fix the Wayland
      text-input regression in
      [#48](https://github.com/framefilter/keyroost/issues/48) — but check
      whether egui-winit's glue changes incidentally resolve it on Fedora-44 KWin
      while we're here.

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

## GUI — Text-size control polish ([#42](https://github.com/framefilter/keyroost/issues/42), @token2)

- [ ] Add discrete **"−" / "+" buttons** on the ends of the zoom slider; mouse
      dragging is unpredictable near the boundaries.
- [ ] **Light theme:** the slider track/handle is almost invisible — restyle it
      so it reads on the light palette (it's currently tuned for dark only).
