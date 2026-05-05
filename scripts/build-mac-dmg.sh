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

# Host check: aarch64-apple-darwin only.
if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
    echo "ERROR: this script targets aarch64-apple-darwin only (host: $(uname -s) $(uname -m))." >&2
    exit 2
fi

# Sanity: required source files.
[[ -f "${ICON_SRC}" ]] || { echo "ERROR: missing icon at ${ICON_SRC}" >&2; exit 2; }
[[ -f "${PLIST_TEMPLATE}" ]] || { echo "ERROR: missing plist template at ${PLIST_TEMPLATE}" >&2; exit 2; }
[[ -d "${VENDOR_DIR}" ]] || { echo "ERROR: missing vendor at ${VENDOR_DIR}" >&2; exit 2; }

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
