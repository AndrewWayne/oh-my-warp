#!/usr/bin/env bash
# Build the omw-local preview Linux GUI binary.
#
# Usage: bash scripts/build-linux-gui.sh [version]
#   default version: 0.0.1
#
# Produces: dist/omw-warp-oss-v<version>-<arch>-unknown-linux-gnu/warp-oss
#
# Pre-reqs: vendor/warp-stripped/script/linux/install_build_deps, plus rustup.
# Does not touch vendor/warp-stripped/ source.

set -euo pipefail

VERSION="${1:-0.0.1}"

# Resolve repo root so the script works from anywhere.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
VENDOR_DIR="${REPO_ROOT}/vendor/warp-stripped"
DIST_DIR="${REPO_ROOT}/dist"

fail() {
    echo "ERROR: $*" >&2
    exit 2
}

HOST_OS="$(uname -s)"
HOST_ARCH="$(uname -m)"

if [[ "${HOST_OS}" != "Linux" ]]; then
    fail "this script targets Linux only (host: ${HOST_OS} ${HOST_ARCH})"
fi

case "${HOST_ARCH}" in
    x86_64) TARGET_TRIPLE="x86_64-unknown-linux-gnu" ;;
    aarch64|arm64) TARGET_TRIPLE="aarch64-unknown-linux-gnu" ;;
    *) fail "unsupported Linux architecture: ${HOST_ARCH}" ;;
esac

[[ -d "${VENDOR_DIR}" ]] || fail "missing vendor checkout at ${VENDOR_DIR}"
[[ -f "${REPO_ROOT}/LICENSE" ]] || fail "missing LICENSE at ${REPO_ROOT}/LICENSE"

# cargo on PATH.
[[ -f "${HOME}/.cargo/env" ]] && source "${HOME}/.cargo/env"
command -v cargo >/dev/null || fail "cargo not on PATH"

if [[ -z "${PROTOC:-}" ]]; then
    PROTOC="$(command -v protoc || true)"
    if [[ -z "${PROTOC}" && -x "${REPO_ROOT}/.tmp/tools/protoc-25.1/bin/protoc" ]]; then
        PROTOC="${REPO_ROOT}/.tmp/tools/protoc-25.1/bin/protoc"
    fi
fi
[[ -n "${PROTOC}" && -x "${PROTOC}" ]] || fail "PROTOC not executable; install protoc or set PROTOC=/path/to/protoc"
export PROTOC

# On a 16 GB host the warp lib's opt-level=3 codegen has been observed to
# get OOM-killed by the kernel (rustc dies with `signal: 9, SIGKILL`) when
# cargo runs the default $(nproc) parallel compile jobs. Capping at 4
# concurrent jobs keeps peak RSS under ~12 GB and lets the build finish.
# Override via CARGO_BUILD_JOBS=N if you have more or less headroom.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

echo "==> Building omw_local Linux GUI binary (version ${VERSION}, target ${TARGET_TRIPLE}) ..."
# option_env!("GIT_RELEASE_TAG") is captured at Rust compile time, so this
# export MUST happen before `cargo build`, not just before staging.
export GIT_RELEASE_TAG="omw-local-preview-v${VERSION}"
(
    cd "${VENDOR_DIR}"
    cargo build -p warp --bin warp-oss --no-default-features --features omw_local
)
BINARY="${VENDOR_DIR}/target/debug/warp-oss"
[[ -f "${BINARY}" ]] || { echo "ERROR: build did not produce ${BINARY}" >&2; exit 1; }

LINUX_DIST_DIR="${DIST_DIR}/omw-warp-oss-v${VERSION}-${TARGET_TRIPLE}"
LINUX_BINARY="${LINUX_DIST_DIR}/warp-oss"

echo "==> Staging Linux GUI binary at ${LINUX_DIST_DIR} ..."
rm -rf "${LINUX_DIST_DIR}"
mkdir -p "${LINUX_DIST_DIR}"
cp "${BINARY}" "${LINUX_BINARY}"
chmod +x "${LINUX_BINARY}"
cp "${REPO_ROOT}/LICENSE" "${LINUX_DIST_DIR}/LICENSE"

(
    cd "${VENDOR_DIR}"
    bash scripts/audit-no-cloud.sh "${BINARY}"
)
(
    cd "${LINUX_DIST_DIR}"
    shasum -a 256 "warp-oss" > "warp-oss.sha256"
)

echo
echo "==> Done."
echo "Artifact: ${LINUX_BINARY}"
echo "Run:      ${LINUX_BINARY}"
echo "Size:     $(du -h "${LINUX_BINARY}" | cut -f1)"
echo "SHA256:   $(shasum -a 256 "${LINUX_BINARY}" | awk '{print $1}')"
