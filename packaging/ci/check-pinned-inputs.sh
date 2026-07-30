#!/usr/bin/env bash
#
# check-pinned-inputs.sh — reject release/packaging inputs fetched from a
# MUTABLE ref (master, main, HEAD, continuous, :latest, or a bare image
# tag like :freedesktop-NN) unless a pin/verification marker vouches for
# the line. Immutable refs (40-hex commits, vX.Y.Z tags, @sha256:<digest>)
# are always allowed; a mutable ref is allowed only when the same line or
# one of the two lines directly above it carries
#   # pinned-verified: <why>
# meaning a nearby checksum step content-pins it. KEY-001 regression guard.
set -euo pipefail

if [ "$#" -gt 0 ]; then
  files=("$@")
else
  files=(
    ".github/workflows/linux-bundles.yml"
    "packaging/appimage/build-appimage.sh"
  )
fi

# Mutable-ref shapes we refuse in a fetched/executed input.
patterns='/(master|main|HEAD)/|/continuous/|:latest([^0-9a-zA-Z]|$)|:freedesktop-[0-9]'

rc=0
for f in "${files[@]}"; do
  [ -f "$f" ] || { echo "check-pinned-inputs: missing ${f}" >&2; rc=1; continue; }
  # Slurp once so we can look back at the two preceding lines for the marker
  # (checksum guards conventionally sit just above the fetch they vouch for).
  mapfile -t lines < "$f"
  while IFS= read -r hit; do
    lineno="${hit%%:*}"
    text="${hit#*:}"
    marked=0
    for back in 0 1 2; do
      idx=$((lineno - 1 - back))
      [ "$idx" -ge 0 ] || break
      case "${lines[$idx]}" in
        *pinned-verified:*) marked=1; break ;;
      esac
    done
    [ "$marked" -eq 1 ] && continue
    echo "::error file=${f},line=${lineno}::mutable release input not pinned/verified: ${text}"
    rc=1
  done < <(grep -nE "$patterns" "$f" || true)
done

if [ "$rc" -ne 0 ]; then
  echo "check-pinned-inputs: mutable release inputs found (see errors)." >&2
  echo "Pin to a commit/tag/@sha256 digest, or add '# pinned-verified: <why>' where a checksum step guards it." >&2
fi
exit "$rc"
