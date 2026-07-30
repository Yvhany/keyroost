#!/usr/bin/env bash
# Smoke test for check-pinned-inputs.sh. Runs it against a bad fixture
# (must fail) and a good fixture (must pass). No hardware, no network.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="${here}/check-pinned-inputs.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

# BAD: a continuous download with no pin marker must be rejected.
cat > "${tmp}/bad.sh" <<'EOF'
curl -fsSL -o linuxdeploy.AppImage \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
EOF

# GOOD: a continuous download guarded by a checksum step + marker passes.
cat > "${tmp}/good.sh" <<'EOF'
# pinned-verified: sha256 checked against LD_SHA256 below before chmod/exec
curl -fsSL -o linuxdeploy.AppImage \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
EOF

fail=0
if bash "${script}" "${tmp}/bad.sh" >/dev/null 2>&1; then
  echo "FAIL: bad fixture was accepted"; fail=1
else
  echo "ok: bad fixture rejected"
fi
if bash "${script}" "${tmp}/good.sh" >/dev/null 2>&1; then
  echo "ok: good fixture accepted"
else
  echo "FAIL: good fixture rejected"; fail=1
fi
exit "${fail}"
