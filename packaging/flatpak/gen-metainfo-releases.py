#!/usr/bin/env python3
"""Fill the AppStream <releases> block from CHANGELOG.md (issue #80).

CHANGELOG.md is the single source of release history. The committed
metainfo keeps an EMPTY <releases> element; this script fills it at bundle
build time, so the shipped AppStream data can never drift from the
changelog again (the hand-maintained list was missed three releases
running, which left flatpak clients believing 0.7.2 was current).

It also hard-fails when the newest changelog entry disagrees with the
workspace version in Cargo.toml — the exact mismatch that caused #80 —
so a bundle build cannot proceed from an un-updated changelog.

Stdlib only. Two modes:
    gen-metainfo-releases.py            rewrite the metainfo in place
    gen-metainfo-releases.py --check    validate only (CI backstop)
"""

import re
import sys

# stdlib ElementTree is deliberate here despite the usual defusedxml advice:
# the only XML this script ever parses is this repository's own committed
# metainfo plus entries derived from this repository's CHANGELOG — there is
# no untrusted input path — and the vendor-over-depend rule bars a new
# dependency for a build-time well-formedness assert.
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHANGELOG = ROOT / "CHANGELOG.md"
CARGO_TOML = ROOT / "Cargo.toml"
METAINFO = ROOT / "packaging" / "flatpak" / "io.github.framefilter.keyroost.metainfo.xml"

HEADING_RE = re.compile(r"^## \[(\d+\.\d+\.\d+)\] - (\d{4}-\d{2}-\d{2})\s*$")
VERSION_RE = re.compile(r'^version = "(\d+\.\d+\.\d+)"\s*$')
# Anchored to line starts so the literal "<releases>" mention in the
# explanatory comment above the block can never match — only the real,
# two-space-indented element does (empty or previously filled, so a second
# run is idempotent).
RELEASES_RE = re.compile(r"^  <releases>.*?^  </releases>", re.DOTALL | re.MULTILINE)


def fail(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def changelog_releases():
    releases = []
    for line in CHANGELOG.read_text(encoding="utf-8").splitlines():
        m = HEADING_RE.match(line)
        if m:
            releases.append((m.group(1), m.group(2)))
    if not releases:
        fail(f"no '## [X.Y.Z] - YYYY-MM-DD' headings found in {CHANGELOG}")
    return releases


def workspace_version():
    for line in CARGO_TOML.read_text(encoding="utf-8").splitlines():
        m = VERSION_RE.match(line)
        if m:
            return m.group(1)
    fail(f'no version = "X.Y.Z" line found in {CARGO_TOML}')


def rendered_metainfo():
    releases = changelog_releases()
    ws = workspace_version()
    if releases[0][0] != ws:
        fail(
            f"newest CHANGELOG.md release is {releases[0][0]} but the workspace "
            f"version is {ws} — update CHANGELOG.md before building bundles "
            "(this mismatch is how #80 shipped)"
        )
    body = "\n".join(
        f'    <release version="{v}" date="{d}"/>' for v, d in releases
    )
    text = METAINFO.read_text(encoding="utf-8")
    if len(RELEASES_RE.findall(text)) != 1:
        fail(f"expected exactly one <releases>…</releases> block in {METAINFO}")
    new_text = RELEASES_RE.sub(f"  <releases>\n{body}\n  </releases>", text)
    try:
        ET.fromstring(new_text)
    except ET.ParseError as e:
        fail(f"generated metainfo is not well-formed XML: {e}")
    return new_text


def main():
    check_only = sys.argv[1:] == ["--check"]
    if sys.argv[1:] not in ([], ["--check"]):
        fail(f"usage: {sys.argv[0]} [--check]")
    new_text = rendered_metainfo()
    if check_only:
        n = len(changelog_releases())
        print(f"ok: {n} releases derivable; newest matches workspace version")
        return
    METAINFO.write_text(new_text, encoding="utf-8")
    print(f"wrote {len(changelog_releases())} <release> entries to {METAINFO}")


if __name__ == "__main__":
    main()
