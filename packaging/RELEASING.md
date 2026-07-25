# Release-day playbook

The whole cut, in order, from a clean tree to every channel verified. Run it
top to bottom; nothing here is optional unless marked so. Written after
v0.7.5/v0.7.6 — the traps called out below all actually happened.

Conventions: the maintainer runs everything that signs or publishes; an agent
may prepare branches and approve **build-only probe** gates, never a
publishing gate. Version placeholder below: `vX.Y.Z`.

## 1. Pre-flight (no version bump yet)

- [ ] `git fetch origin` — main clean, no unlanded branches you meant to ship.
- [ ] CI green on main (includes the CHANGELOG/Cargo.toml drift guard and the
      pinned-inputs check).
- [ ] `cargo audit` green (the audit workflow runs on pushes; check the last
      run) and the deps-outdated report reviewed.
- [ ] **New-crate check:** any crate added to the workspace since the last
      release needs a one-time manual `cargo publish` and a crates.io
      Trusted Publishing entry BEFORE the release run — the OIDC job cannot
      create a brand-new crate.
- [ ] **Packaging probe** (mandatory, one command):
      `gh workflow run linux-bundles.yml --ref main`
      No tag input = build-only; approve the gate (probe-safe). Both bundle
      jobs must go green, with "wrote N `<release>` entries" in their logs.
      Packaging pulls from upstreams that drift on their own schedule — the
      v0.7.3 flatpak broke at release time because an upstream source was
      pruned. Probes catch that; release runs must not.
- [ ] **Packaged-crate asset check** — every file a crate *references* must be
      a file it *ships*:
      `cargo package -p keyroost --no-verify --offline`
      `tar tzf target/package/keyroost-*.crate | grep -i '\.ico'`
      Confirm every path `build.rs` reads (and any `include_str!`/
      `include_bytes!` across the workspace) is inside the tarball. Cargo
      packages only files beneath the package root, so a path reaching outside
      it — `../../packaging/...` — silently vanishes from the published crate
      while still resolving fine in a git checkout. That is how the Windows
      icon broke `cargo install keyroost` before v0.7.7: nothing catches it,
      because `cargo publish` runs its verification build on Linux where
      `build.rs` is a `#[cfg(not(windows))]` no-op, and no workflow builds the
      packaged tarball at all. Hence `crates/keyroost/assets/keyroost.ico`.
      Note you cannot *build* the unpacked tarball at this stage: it resolves
      its sibling `keyroost-*` deps from crates.io at the new version, which is
      not published yet. Contents are the check here; the build is step 6.

## 2. Version bump + changelog (prep branch)

- [ ] Branch off main. Bump the workspace version: every
      `version = "<old>"` in the Cargo.tomls (the workspace field plus the
      inter-crate path-dep pins — `grep -rn 'version = "<old>"' --include=Cargo.toml .`).
- [ ] `cargo update --workspace` at the root AND in `fuzz/` (its own lock).
- [ ] CHANGELOG: add the `## [X.Y.Z] - date` section and the compare links.
      The top entry MUST match the new workspace version —
      `python3 packaging/flatpak/gen-metainfo-releases.py --check` proves it
      (CI enforces the same).
- [ ] Full gates: clippy `-D warnings`, fmt, workspace tests.
- [ ] Land on main via the signing flow (rebase over origin/main re-creates
      the commit signed; push `HEAD:main`).

## 3. Tag and watch the build

- [ ] `git tag -s vX.Y.Z -m "keyroost vX.Y.Z" && git push origin vX.Y.Z`
      (`v*` tags are admin-only by ruleset.)
- [ ] Two workflows start on the tag: `release.yml` (platform archives +
      GitHub Release) and `linux-bundles.yml` (AppImage + flatpak).
      **Approve both release-publish gates promptly and together** — the
      bundle attach steps wait for the Release that release.yml creates.
      The retry window is 10 minutes (v0.7.6 lost the old 2-minute window
      by 16 seconds); if it still expires, re-run the failed job once the
      Release exists — attach is idempotent (`--clobber`).
- [ ] When both finish, the Release must hold: 3 platform archives,
      `SHA256SUMS`, `keyroost-x86_64.AppImage` (+ `.sha256`, `.zsync`),
      `keyroost.flatpak` (+ `.sha256`). Check:
      `gh release view vX.Y.Z --json assets --jq '[.assets[].name]'`

## 4. Fanout (publish.yml)

- [ ] Approve the fanout's release-publish gate.
- [ ] **Verify each channel actually PUBLISHED — a green job can mask a
      no-op** (missing secrets skip-with-notice; caches lag):
  - crates.io: the two binaries publish last, so this one check covers the
    dependency chain —
    `curl -fsSL -H "User-Agent: keyroost-release" https://crates.io/api/v1/crates/keyroostctl/X.Y.Z`
  - Homebrew: `curl -fsSL https://raw.githubusercontent.com/framefilter/homebrew-keyroost/main/Formula/keyroost.rb | grep version`
  - AUR: check the **push line in the job log** first
    (`master -> master` to aur.archlinux.org); the RPC
    (`https://aur.archlinux.org/rpc/v5/info?arg[]=keyroost-bin`) lags a few
    minutes behind and reads stale right after the push.
  - Flatpak remote (a machine with the remote configured):
    `flatpak update --appstream && flatpak remote-info keyroost io.github.framefilter.keyroost`
    must show the new version.
  - **winget: a skip with the "HOLDING for the Token2-signed build" notice
    is the DESIGNED outcome at this stage** — see step 5. A hard failure
    here means the `WINGET_TOKEN` PAT died (the job fails loudly on an
    expired token; renew the classic PAT, `public_repo` scope).

## 5. Signed Windows build (out-of-band, Token2)

- [ ] Ask Token2 to sign the **current** release's Windows build (never an
      older version — it would predate shipped fixes).
- [ ] When it arrives: attach as **NEW** assets
      `keyroost-vX.Y.Z-windows-x86_64-signed.zip` + `.sha256`. **Never
      replace the CI-built assets** — that invalidates `SHA256SUMS`,
      provenance attestations, and any open winget PR's hash.
- [ ] `gh workflow run publish.yml -f tag=vX.Y.Z` and approve the gate.
      Every channel no-ops (idempotent); the winget job Authenticode-verifies
      every PE in the signed zip (signer logged in the run) and submits the
      manifest. Confirm the winget-pkgs PR opened and (eventually) merged —
      Defender validation false positives on fresh binaries do happen; the
      documented remedies are a pipeline re-run (~18h cycles) or a WDSI
      false-positive report, not resigning.
- [ ] Manual fallback from Linux if wingetcreate misbehaves:
      `komac update Framefilter.Keyroost --version X.Y.Z --urls <signed-asset-url> --submit`

## 6. Post-release

- [ ] Install-matrix spot check as machines allow: `cargo install keyroostctl`,
      flatpak update on a real install, AppImage launch, brew upgrade, winget
      after step 5.
- [ ] **`cargo install keyroost` on Windows** — the one configuration that
      actually compiles `build.rs` and embeds the icon from the published
      tarball, and the only place a missing packaged asset shows up. Only
      possible here, once the sibling crates are live (see the pre-flight
      asset check for why it cannot run earlier). If this ever fails, the fix
      belongs under the crate root, not in a wider `include`: cargo cannot
      package paths above the package directory.
- [ ] `keyroostctl --version` / GUI About shows X.Y.Z.
- [ ] Close/comment the issues the release fixes (drafts usually prepared
      during the work); announcement if any.
- [ ] Out-of-band corrections later (metadata fixes, asset re-attach): use the
      dispatch republish — `packaging/LINUX-BUNDLES.md` "Out-of-band runs".
      A republish builds from the dispatched ref into the tag's release and
      is version-guarded against tree/tag mismatch.
