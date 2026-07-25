# keyroost app icon

The shipped app icon — the **dark-on-amber** `k` monogram (IBM Plex Sans Bold,
outlined, no font dependency), built to the freedesktop icon spec.

```
io.github.framefilter.keyroost.svg        scalable master (vector)
io.github.framefilter.keyroost-256.png    256px raster (spare copy; NOT the AppImage source)
hicolor/<size>/apps/io.github.framefilter.keyroost.png   16…1024px PNGs
hicolor/scalable/apps/io.github.framefilter.keyroost.svg scalable
```

One more artifact is generated **from** these PNGs but deliberately does not
live here:

```
crates/keyroost/assets/keyroost.ico       Windows app icon (NOT under packaging/)
packaging/icons/gen-ico.py                writes it; run after changing the PNGs
```

`gen-ico.py` (stdlib only) packs the 16…256px hicolor PNGs into an ICO — Windows
Vista+ reads PNG-compressed ICO entries directly, so it is an index over the
existing files, no re-encoding. `crates/keyroost/build.rs` embeds it into
`keyroost.exe` and **panics if it is missing**, rather than silently shipping an
icon-less binary.

**Why it lives inside the crate, not here:** cargo packages only files beneath
the package root, so an `.ico` kept in `packaging/icons/` would resolve fine in
a git checkout but be absent from the published `.crate` — and every
`cargo install keyroost` on Windows would fail at that panic. This is exactly
how the Windows icon broke `cargo install` before v0.7.7. Nothing catches it
earlier: `cargo publish` runs its verification build on Linux, where `build.rs`
is a `#[cfg(not(windows))]` no-op. **Do not "tidy" the `.ico` back under
`packaging/` — that re-creates the bug.** There is one copy and `gen-ico.py`
writes straight to it; regenerate and commit the result:

```bash
python3 packaging/icons/gen-ico.py
```

The Flatpak manifest installs the whole `hicolor/` tree into
`${FLATPAK_DEST}/share/icons/hicolor/`; the AppImage build passes
`hicolor/256x256/apps/io.github.framefilter.keyroost.png` to
`linuxdeploy --icon-file` — **not** the `-256.png` beside it. The filename stem
**must** stay the app-id `io.github.framefilter.keyroost`, with no size suffix
(Flatpak / AppStream / desktop-file icon resolution keys on it, and linuxdeploy
rejects a suffixed name with "Could not find suitable icon").

An **alternate colorway** also exists — amber-on-dark (amber glyph on the dark
surface, matching the in-app title-bar mark). It plus the original design bundle
were kept out of the published tree; recover them from git history (the commit
that added `docs/app_icons/`) to switch colorways.

For the auto-update Flatpak remote, also place a copy of the SVG in the **root**
of the `framefilter/keyroost-flatpak` repo as `keyroost-icon.svg` (see
[`../LINUX-BUNDLES.md`](../LINUX-BUNDLES.md), setup step 3).

## Who references these paths

- `packaging/flatpak/io.github.framefilter.keyroost.yml` — installs the hicolor tree.
- `packaging/appimage/build-appimage.sh` — passes `hicolor/256x256/apps/io.github.framefilter.keyroost.png`
  to `linuxdeploy --icon-file` (the name must have no size suffix, or linuxdeploy
  reports "Could not find suitable icon").
- `packaging/flatpak/io.github.framefilter.keyroost.desktop` — `Icon=` key.
- `packaging/flatpak/io.github.framefilter.keyroost.metainfo.xml` — AppStream
  resolves the icon by app-id from the installed hicolor theme.
- `packaging/icons/gen-ico.py` — reads `hicolor/<size>/apps/*.png`, writes
  `crates/keyroost/assets/keyroost.ico`.
- `crates/keyroost/build.rs` — embeds that `.ico` into `keyroost.exe`
  (Windows-only; panics if absent).
