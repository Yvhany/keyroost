#!/usr/bin/env python3
"""Pack the hicolor PNGs into crates/keyroost/assets/keyroost.ico (stdlib only).

Windows Vista+ reads PNG-compressed ICO entries directly, so the .ico is
just an ICONDIR index over the existing hicolor PNGs — no re-encoding, no
imaging dependency. Re-run after changing the PNG set and commit the result;
`build.rs` in crates/keyroost embeds the .ico into keyroost.exe.

The output deliberately lands inside the `keyroost` crate rather than next to
this script: cargo packages only files under the package root, so an .ico kept
here would be absent from the published .crate and every `cargo install` on
Windows would fail. This is the only copy — nothing under packaging/ mirrors it.
"""

import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent  # packaging/icons -> packaging -> repo root
APP_ID = "io.github.framefilter.keyroost"
SIZES = [16, 24, 32, 48, 64, 128, 256]  # 512 exceeds the ICO size field
OUT = REPO / "crates" / "keyroost" / "assets" / "keyroost.ico"


def main() -> None:
    blobs = []
    for size in SIZES:
        p = ROOT / "hicolor" / f"{size}x{size}" / "apps" / f"{APP_ID}.png"
        if not p.is_file():
            print(f"error: missing {p}", file=sys.stderr)
            sys.exit(1)
        blobs.append((size, p.read_bytes()))

    header = struct.pack("<HHH", 0, 1, len(blobs))
    entries = b""
    offset = len(header) + 16 * len(blobs)
    for size, data in blobs:
        dim = 0 if size == 256 else size  # 0 encodes 256 in the u8 fields
        entries += struct.pack(
            "<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset
        )
        offset += len(data)
    OUT.write_bytes(header + entries + b"".join(data for _, data in blobs))
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes, {len(blobs)} entries)")


if __name__ == "__main__":
    main()
