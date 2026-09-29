#!/usr/bin/env bash
# Regression test for scripts/build-linux-gui.sh staging.
#
# Usage: bash scripts/test-build-linux-gui.sh
#
# Runs the build script against a scratch copy of the repo with a stub
# `cargo` that only creates the expected binaries, then checks the staged
# dist folder. Linux only; takes a few seconds.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

FAKE_REPO="${WORK}/repo"
mkdir -p "${FAKE_REPO}/scripts" "${FAKE_REPO}/vendor/warp-stripped/scripts" "${WORK}/bin" "${WORK}/home"
cp "${REPO_ROOT}/scripts/build-linux-gui.sh" "${FAKE_REPO}/scripts/"
cp "${REPO_ROOT}/vendor/warp-stripped/scripts/audit-no-cloud.sh" "${FAKE_REPO}/vendor/warp-stripped/scripts/"
cp "${REPO_ROOT}/LICENSE" "${FAKE_REPO}/"

# Stub cargo: `cargo build ... -p <pkg> ...` creates target/debug/<bin>
# under the current directory.
cat > "${WORK}/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
pkg=""
while [[ $# -gt 0 ]]; do
    if [[ "$1" == "-p" ]]; then pkg="$2"; shift; fi
    shift
done
case "${pkg}" in
    warp) bin="warp-oss" ;;
    *) bin="${pkg}" ;;
esac
mkdir -p target/debug
printf '#!/bin/sh\n' > "target/debug/${bin}"
chmod +x "target/debug/${bin}"
EOF
chmod +x "${WORK}/bin/cargo"
printf '#!/bin/sh\n' > "${WORK}/bin/protoc"
chmod +x "${WORK}/bin/protoc"

# HOME points at an empty dir so the script's `source ~/.cargo/env`
# can't put the real cargo back in front of the stub.
HOME="${WORK}/home" PATH="${WORK}/bin:${PATH}" PROTOC="${WORK}/bin/protoc" \
    bash "${FAKE_REPO}/scripts/build-linux-gui.sh" 9.9.9 > "${WORK}/build.log" 2>&1 || {
    cat "${WORK}/build.log" >&2
    echo "FAIL: build-linux-gui.sh exited non-zero" >&2
    exit 1
}

DIST="$(find "${FAKE_REPO}/dist" -mindepth 1 -maxdepth 1 -type d -name 'omw-warp-oss-v9.9.9-*')"
fail=0
for f in warp-oss omw-keychain-helper LICENSE; do
    if [[ ! -f "${DIST}/${f}" ]]; then
        echo "FAIL: ${f} missing from staged dist" >&2
        fail=1
    fi
done
if [[ -f "${DIST}/omw-keychain-helper" && ! -x "${DIST}/omw-keychain-helper" ]]; then
    echo "FAIL: staged omw-keychain-helper is not executable" >&2
    fail=1
fi
[[ "${fail}" -eq 0 ]] || exit 1
echo "PASS: build-linux-gui.sh stages warp-oss, omw-keychain-helper and LICENSE"
