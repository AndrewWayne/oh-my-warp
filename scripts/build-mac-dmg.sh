#!/usr/bin/env bash
# Build the omw-local preview .dmg for aarch64-apple-darwin.
#
# Usage: bash scripts/build-mac-dmg.sh [version]
#   default version: 0.0.1
#
# Produces: dist/omw-warp-oss-v<version>-aarch64-apple-darwin.dmg
#
# Pre-reqs: full Xcode + Homebrew protoc + rustup (see vendor/warp-stripped/OMW_LOCAL_BUILD.md).
# Does not touch vendor/warp-stripped/ source.

set -euo pipefail

VERSION="${1:-0.0.1}"
TARGET_TRIPLE="aarch64-apple-darwin"

# Resolve repo root so the script works from anywhere.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
VENDOR_DIR="${REPO_ROOT}/vendor/warp-stripped"
DIST_DIR="${REPO_ROOT}/dist"
ICON_SRC="${REPO_ROOT}/assets/omw-warp-oss-icon.png"
PLIST_TEMPLATE="${REPO_ROOT}/scripts/release/Info.plist.template"
OMW_AGENT_DIR="${REPO_ROOT}/apps/omw-agent"

# Host check: aarch64-apple-darwin only.
if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
    echo "ERROR: this script targets aarch64-apple-darwin only (host: $(uname -s) $(uname -m))." >&2
    exit 2
fi

# Sanity: required source files.
[[ -f "${ICON_SRC}" ]] || { echo "ERROR: missing icon at ${ICON_SRC}" >&2; exit 2; }
[[ -f "${PLIST_TEMPLATE}" ]] || { echo "ERROR: missing plist template at ${PLIST_TEMPLATE}" >&2; exit 2; }
[[ -d "${VENDOR_DIR}" ]] || { echo "ERROR: missing vendor at ${VENDOR_DIR}" >&2; exit 2; }
[[ -d "${OMW_AGENT_DIR}" ]] || { echo "ERROR: missing omw-agent at ${OMW_AGENT_DIR}" >&2; exit 2; }

# cargo on PATH.
[[ -f "${HOME}/.cargo/env" ]] && source "${HOME}/.cargo/env"
command -v cargo >/dev/null || { echo "ERROR: cargo not on PATH" >&2; exit 2; }

export PROTOC="${PROTOC:-/opt/homebrew/bin/protoc}"
[[ -x "${PROTOC}" ]] || { echo "ERROR: PROTOC not executable at ${PROTOC}" >&2; exit 2; }

echo "==> Building omw_local release binary (version ${VERSION}) ..."
(
    cd "${VENDOR_DIR}"
    cargo build --release -p warp --bin warp-oss --no-default-features --features omw_local
)
BINARY="${VENDOR_DIR}/target/release/warp-oss"
[[ -f "${BINARY}" ]] || { echo "ERROR: build did not produce ${BINARY}" >&2; exit 1; }

echo "==> Building omw-agent kernel (TypeScript -> dist/) ..."
command -v node >/dev/null || { echo "ERROR: node not on PATH (required for omw-agent build)" >&2; exit 2; }
command -v npm  >/dev/null || { echo "ERROR: npm not on PATH (required for omw-agent build)"  >&2; exit 2; }
(
    cd "${OMW_AGENT_DIR}"
    # Always run npm install. The prior `[[ ! -d node_modules ]]` guard
    # was satisfied by a stray `node_modules/.vite/` directory (`rm -rf
    # node_modules/*` doesn't match dotfiles), which silently shipped
    # v0.0.3-rev2 with no runtime deps and ENOENTed @mariozechner/pi-ai
    # at the first /agent/sessions POST. npm install is idempotent and
    # cache-fast when nothing has changed.
    npm install --no-fund --no-audit
    npm run build
)
[[ -f "${OMW_AGENT_DIR}/dist/src/serve.js" ]] \
    || { echo "ERROR: omw-agent build did not produce dist/src/serve.js" >&2; exit 1; }

# Build the keychain helper from the umbrella workspace. The agent kernel
# (Node) spawns this binary to resolve `keychain:omw/<provider>` refs into
# API keys. Without it, the v0.0.3 Settings → Apply flow leaves the agent
# unable to read keys it just wrote — so the .app bundle is not self-
# contained. omw_inproc_server.rs locates it at
# <exe_dir>/../Resources/omw-keychain-helper.
echo "==> Building omw-keychain-helper release binary ..."
(
    cd "${REPO_ROOT}"
    cargo build --release -p omw-keychain-helper
)
KEYCHAIN_HELPER_BIN="${REPO_ROOT}/target/release/omw-keychain-helper"
[[ -f "${KEYCHAIN_HELPER_BIN}" ]] \
    || { echo "ERROR: omw-keychain-helper build did not produce ${KEYCHAIN_HELPER_BIN}" >&2; exit 1; }

# Fetch a Node interpreter to bundle. Without this the .app's bare
# `Command::new("node")` ENOENTs when launched from Finder, because
# LaunchServices hands .apps a minimal PATH that excludes Homebrew.
# Pinned to a known LTS; verified against the official SHASUMS256.txt
# from nodejs.org. Cached under ~/.cache/oh-my-warp/node so subsequent
# builds skip the download.
NODE_VERSION="${NODE_VERSION:-22.11.0}"
NODE_PKG="node-v${NODE_VERSION}-darwin-arm64"
NODE_DIST_URL="https://nodejs.org/dist/v${NODE_VERSION}"
NODE_CACHE_DIR="${HOME}/.cache/oh-my-warp/node"
NODE_TARBALL="${NODE_CACHE_DIR}/${NODE_PKG}.tar.gz"
NODE_SHASUMS="${NODE_CACHE_DIR}/SHASUMS256-v${NODE_VERSION}.txt"
NODE_EXTRACTED="${NODE_CACHE_DIR}/${NODE_PKG}"

mkdir -p "${NODE_CACHE_DIR}"

echo "==> Acquiring Node v${NODE_VERSION} for bundle (cache: ${NODE_CACHE_DIR}) ..."
if [[ ! -f "${NODE_SHASUMS}" ]]; then
    curl -fsSL "${NODE_DIST_URL}/SHASUMS256.txt" -o "${NODE_SHASUMS}.partial"
    mv "${NODE_SHASUMS}.partial" "${NODE_SHASUMS}"
fi
if [[ ! -f "${NODE_TARBALL}" ]]; then
    echo "    downloading ${NODE_PKG}.tar.gz ..."
    curl -fsSL "${NODE_DIST_URL}/${NODE_PKG}.tar.gz" -o "${NODE_TARBALL}.partial"
    mv "${NODE_TARBALL}.partial" "${NODE_TARBALL}"
fi
NODE_EXPECTED_SHA="$(awk -v p="${NODE_PKG}.tar.gz" '$2 == p { print $1 }' "${NODE_SHASUMS}")"
[[ -n "${NODE_EXPECTED_SHA}" ]] \
    || { echo "ERROR: no checksum for ${NODE_PKG}.tar.gz in SHASUMS256.txt" >&2; exit 1; }
NODE_ACTUAL_SHA="$(shasum -a 256 "${NODE_TARBALL}" | awk '{print $1}')"
[[ "${NODE_EXPECTED_SHA}" == "${NODE_ACTUAL_SHA}" ]] \
    || { echo "ERROR: Node tarball SHA256 mismatch (expected ${NODE_EXPECTED_SHA}, got ${NODE_ACTUAL_SHA}); delete ${NODE_TARBALL} and retry" >&2; exit 1; }
if [[ ! -d "${NODE_EXTRACTED}" ]]; then
    tar -xzf "${NODE_TARBALL}" -C "${NODE_CACHE_DIR}"
fi
NODE_BIN_SRC="${NODE_EXTRACTED}/bin/node"
[[ -x "${NODE_BIN_SRC}" ]] \
    || { echo "ERROR: extracted Node binary missing or not executable at ${NODE_BIN_SRC}" >&2; exit 1; }

echo "==> Auditing binary for forbidden hostnames ..."
(
    cd "${VENDOR_DIR}"
    bash scripts/audit-no-cloud.sh "target/release/warp-oss"
)

echo "==> Scaffolding .app bundle ..."
APP_NAME="omw-warp-oss.app"
STAGING="${DIST_DIR}/staging-v${VERSION}"
APP_DIR="${STAGING}/${APP_NAME}"
rm -rf "${STAGING}"
mkdir -p "${APP_DIR}/Contents/MacOS" "${APP_DIR}/Contents/Resources"

cp "${BINARY}" "${APP_DIR}/Contents/MacOS/omw-warp-oss"
chmod +x "${APP_DIR}/Contents/MacOS/omw-warp-oss"

# Substitute __VERSION__ and write Info.plist.
sed "s/__VERSION__/${VERSION}/g" "${PLIST_TEMPLATE}" > "${APP_DIR}/Contents/Info.plist"

echo "==> Generating AppIcon.icns from ${ICON_SRC} ..."
ICONSET="${STAGING}/AppIcon.iconset"
rm -rf "${ICONSET}"
mkdir -p "${ICONSET}"
sips -z 16 16     "${ICON_SRC}" --out "${ICONSET}/icon_16x16.png"     >/dev/null
sips -z 32 32     "${ICON_SRC}" --out "${ICONSET}/icon_16x16@2x.png"  >/dev/null
sips -z 32 32     "${ICON_SRC}" --out "${ICONSET}/icon_32x32.png"     >/dev/null
sips -z 64 64     "${ICON_SRC}" --out "${ICONSET}/icon_32x32@2x.png"  >/dev/null
sips -z 128 128   "${ICON_SRC}" --out "${ICONSET}/icon_128x128.png"   >/dev/null
sips -z 256 256   "${ICON_SRC}" --out "${ICONSET}/icon_128x128@2x.png">/dev/null
sips -z 256 256   "${ICON_SRC}" --out "${ICONSET}/icon_256x256.png"   >/dev/null
sips -z 512 512   "${ICON_SRC}" --out "${ICONSET}/icon_256x256@2x.png">/dev/null
sips -z 512 512   "${ICON_SRC}" --out "${ICONSET}/icon_512x512.png"   >/dev/null
sips -z 1024 1024 "${ICON_SRC}" --out "${ICONSET}/icon_512x512@2x.png">/dev/null
iconutil -c icns "${ICONSET}" -o "${APP_DIR}/Contents/Resources/AppIcon.icns"

# Bundle the omw-agent kernel into Contents/Resources/ so the in-process
# server (vendor/warp-stripped/app/src/ai_assistant/omw_inproc_server.rs)
# can lazy-spawn `node Resources/bin/omw-agent.mjs --serve-stdio` without
# touching $PATH or the user's filesystem outside the .app.
#
# The .mjs entry point dynamic-imports `../dist/src/serve.js` and
# `../dist/src/keychain.js`, so the layout under Resources/ must mirror
# the apps/omw-agent/ source tree exactly: bin/, dist/, vendor/,
# node_modules/, and package.json (the latter for "type": "module").
echo "==> Bundling omw-agent kernel into Resources/ ..."
KERNEL_RESOURCES="${APP_DIR}/Contents/Resources"
mkdir -p "${KERNEL_RESOURCES}/bin"
cp "${OMW_AGENT_DIR}/bin/omw-agent.mjs" "${KERNEL_RESOURCES}/bin/omw-agent.mjs"
cp "${OMW_AGENT_DIR}/package.json" "${KERNEL_RESOURCES}/package.json"
# Use ditto to preserve symlinks inside node_modules (some packages have
# symlinked binaries) and to avoid the resource-fork warnings cp emits.
ditto "${OMW_AGENT_DIR}/dist"          "${KERNEL_RESOURCES}/dist"
ditto "${OMW_AGENT_DIR}/vendor"        "${KERNEL_RESOURCES}/vendor"
# Materialize a non-hoisted node_modules for the bundle. The repo is an
# npm workspace ("workspaces": ["apps/*"]) so a normal install in
# apps/omw-agent hoists every dep up to <repo_root>/node_modules,
# leaving apps/omw-agent/node_modules empty (apart from .vite/ left by
# Vite tooling). v0.0.3-rev2 shipped exactly that empty dir, which
# ENOENTed @mariozechner/pi-ai at the first /agent/sessions POST. We
# install into a throwaway dir outside the workspace context to force a
# real local node_modules, then ditto that into Resources/.
echo "==> Materializing isolated node_modules for kernel bundle ..."
ISOLATED_AGENT="${STAGING}/isolated-omw-agent"
rm -rf "${ISOLATED_AGENT}"
mkdir -p "${ISOLATED_AGENT}"
cp "${OMW_AGENT_DIR}/package.json" "${ISOLATED_AGENT}/package.json"
(
    cd "${ISOLATED_AGENT}"
    npm install --no-fund --no-audit --no-package-lock --omit=dev
)
[[ -d "${ISOLATED_AGENT}/node_modules" ]] \
    || { echo "ERROR: isolated install produced no node_modules at ${ISOLATED_AGENT}/node_modules" >&2; exit 1; }
ditto "${ISOLATED_AGENT}/node_modules" "${KERNEL_RESOURCES}/node_modules"

# Verify every declared runtime dep is actually present in the bundled
# node_modules. Catches the v0.0.3-rev2 class of bug where the dir was
# present-but-empty and the kernel ENOENTed at first import.
echo "==> Verifying bundled node_modules has all declared dependencies ..."
DECLARED_DEPS=$(node --no-warnings -e \
    "console.log(Object.keys(require('${OMW_AGENT_DIR}/package.json').dependencies||{}).join(' '))")
[[ -n "${DECLARED_DEPS}" ]] \
    || { echo "ERROR: package.json declares no runtime dependencies — refusing to ship" >&2; exit 1; }
for dep in ${DECLARED_DEPS}; do
    if [[ ! -d "${KERNEL_RESOURCES}/node_modules/${dep}" ]]; then
        echo "ERROR: declared dep '${dep}' missing from bundled node_modules at ${KERNEL_RESOURCES}/node_modules/${dep}" >&2
        echo "       isolated install did not produce it; inspect ${ISOLATED_AGENT}/node_modules/" >&2
        exit 1
    fi
done

# Place the keychain helper at the path omw_inproc_server.rs probes for
# the .app bundle layout (<exe_dir>/../Resources/omw-keychain-helper).
cp "${KEYCHAIN_HELPER_BIN}" "${KERNEL_RESOURCES}/omw-keychain-helper"
chmod +x "${KERNEL_RESOURCES}/omw-keychain-helper"

# Place Node alongside the kernel script so omw_inproc_server.rs's
# locate_node() picks it up. Without this, .app launches from Finder
# spawn `node` from a minimal PATH and ENOENT (see locate_node docstring).
cp "${NODE_BIN_SRC}" "${KERNEL_RESOURCES}/bin/node"
chmod +x "${KERNEL_RESOURCES}/bin/node"

# Ad-hoc sign the bundle. Cargo's linker stamps a `linker-signed,adhoc` signature
# on the Mach-O that claims sealed resources are required, but without this step
# the bundle has no _CodeSignature/CodeResources — Gatekeeper on macOS 26+ rejects
# the quarantined bundle with "is damaged and can't be opened" instead of the
# user-bypassable "Apple cannot check it for malicious software" dialog.
echo "==> Ad-hoc signing bundle ..."
codesign --force --deep --sign - "${APP_DIR}"

echo "==> Staging .dmg payload ..."
DMG_PAYLOAD="${STAGING}/dmg-payload"
rm -rf "${DMG_PAYLOAD}"
mkdir -p "${DMG_PAYLOAD}"
cp -R "${APP_DIR}" "${DMG_PAYLOAD}/"
ln -s /Applications "${DMG_PAYLOAD}/Applications"
cp "${REPO_ROOT}/LICENSE" "${DMG_PAYLOAD}/LICENSE"
if [[ -f "${REPO_ROOT}/RELEASE_NOTES_v${VERSION}.md" ]]; then
    cp "${REPO_ROOT}/RELEASE_NOTES_v${VERSION}.md" "${DMG_PAYLOAD}/README.md"
fi

DMG_PATH="${DIST_DIR}/omw-warp-oss-v${VERSION}-${TARGET_TRIPLE}.dmg"
rm -f "${DMG_PATH}"

echo "==> Creating .dmg at ${DMG_PATH} ..."
hdiutil create \
    -volname "omw-warp-oss v${VERSION}" \
    -srcfolder "${DMG_PAYLOAD}" \
    -ov \
    -format UDZO \
    "${DMG_PATH}"

echo
echo "==> Done."
echo "Artifact: ${DMG_PATH}"
echo "Size:    $(du -h "${DMG_PATH}" | cut -f1)"
echo "SHA256:  $(shasum -a 256 "${DMG_PATH}" | awk '{print $1}')"
