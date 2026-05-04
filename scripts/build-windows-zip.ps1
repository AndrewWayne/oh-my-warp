# Build the omw-local preview .zip for x86_64-pc-windows-msvc.
#
# Usage: pwsh -File scripts/build-windows-zip.ps1 -Version 0.0.2
#
# Produces: dist/omw-warp-oss-v<version>-x86_64-pc-windows-msvc.zip
#
# Pre-reqs: rustup with toolchain 1.92.0 (auto-fetched via rust-toolchain.toml)
# + protoc on PATH (or PROTOC env var) + MSVC build tools.
# Mirrors scripts/build-mac-dmg.sh; does not touch vendor/warp-stripped/ source.

#requires -Version 5.1

[CmdletBinding()]
param(
    [string]$Version = "0.0.1"
)

$ErrorActionPreference = "Stop"

$TargetTriple = "x86_64-pc-windows-msvc"

$scriptRoot = $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $scriptRoot "..")).Path
$vendorDir = Join-Path $repoRoot "vendor\warp-stripped"
$distDir = Join-Path $repoRoot "dist"

if (-not (Test-Path $vendorDir)) {
    throw "missing vendor at $vendorDir"
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo not on PATH"
}

if (-not $env:PROTOC) {
    $protocCmd = Get-Command protoc -ErrorAction SilentlyContinue
    if ($protocCmd) {
        $env:PROTOC = $protocCmd.Source
    }
}
if (-not $env:PROTOC -or -not (Test-Path $env:PROTOC)) {
    throw "PROTOC env var not set or protoc not found on PATH"
}

Write-Host "==> Building omw_local release binary (version $Version) ..."
Push-Location $vendorDir
try {
    cargo build --release -p warp --bin warp-oss --no-default-features --features omw_local
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
finally {
    Pop-Location
}

$binary = Join-Path $vendorDir "target\release\warp-oss.exe"
if (-not (Test-Path $binary)) {
    throw "build did not produce $binary"
}

Write-Host "==> Auditing binary for forbidden hostnames ..."
Push-Location $vendorDir
try {
    bash scripts/audit-no-cloud.sh "target/release/warp-oss.exe"
    if ($LASTEXITCODE -ne 0) { throw "audit-no-cloud failed" }
}
finally {
    Pop-Location
}

Write-Host "==> Staging .zip payload ..."
$staging = Join-Path $distDir "staging-v$Version-windows"
if (Test-Path $staging) {
    Remove-Item $staging -Recurse -Force
}
New-Item -ItemType Directory -Path $staging -Force | Out-Null

Copy-Item $binary (Join-Path $staging "omw-warp-oss.exe")
Copy-Item (Join-Path $repoRoot "LICENSE") (Join-Path $staging "LICENSE")

$notesPath = Join-Path $repoRoot "RELEASE_NOTES_v$Version.md"
if (Test-Path $notesPath) {
    Copy-Item $notesPath (Join-Path $staging "README.md")
}

if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir -Force | Out-Null
}

$zipPath = Join-Path $distDir "omw-warp-oss-v$Version-$TargetTriple.zip"
if (Test-Path $zipPath) {
    Remove-Item $zipPath -Force
}

Write-Host "==> Creating .zip at $zipPath ..."
Compress-Archive -Path (Join-Path $staging "*") -DestinationPath $zipPath

$size = "{0:N2} MB" -f ((Get-Item $zipPath).Length / 1MB)
$hash = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLower()

Write-Host ""
Write-Host "==> Done."
Write-Host "Artifact: $zipPath"
Write-Host "Size:    $size"
Write-Host "SHA256:  $hash"
