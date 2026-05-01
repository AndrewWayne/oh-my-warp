#requires -Version 5.1

[CmdletBinding()]
param(
    [switch]$BuildOnly,
    [switch]$BinaryOnly
)

$ErrorActionPreference = "Stop"

if ($BuildOnly -and $BinaryOnly) {
    throw "Use either -BuildOnly or -BinaryOnly, not both."
}

$scriptRoot = $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $scriptRoot "..\..")).Path
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$toolchain = "1.92.0-x86_64-pc-windows-msvc"
$targetTriple = "x86_64-pc-windows-msvc"
$protocVersion = "29.3"
$protocDir = Join-Path $repoRoot ".tmp\tools\protoc-$protocVersion"
$protocExe = Join-Path $protocDir "bin\protoc.exe"
$binaryPath = Join-Path $scriptRoot "target\debug\warp-oss.exe"

function Ensure-CargoPath {
    if (-not (($env:PATH -split ";") -contains $cargoBin)) {
        $env:PATH = "$cargoBin;$env:PATH"
    }

    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "cargo was not found. Expected rustup shims under $cargoBin."
    }

    if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
        throw "rustup was not found. Expected rustup shims under $cargoBin."
    }
}

function Ensure-RustTarget {
    $installedTargets = @(& rustup target list --toolchain $toolchain --installed)
    if ($installedTargets -notcontains $targetTriple) {
        Write-Host "Installing Rust target $targetTriple for $toolchain..."
        & rustup target add $targetTriple --toolchain $toolchain
    }
}

function Ensure-Protoc {
    if (Test-Path $protocExe) {
        return
    }

    $toolsDir = Split-Path -Parent $protocDir
    $zipPath = Join-Path $toolsDir "protoc-$protocVersion-win64.zip"
    $downloadUrl = "https://github.com/protocolbuffers/protobuf/releases/download/v$protocVersion/protoc-$protocVersion-win64.zip"

    New-Item -ItemType Directory -Force -Path $toolsDir | Out-Null

    Write-Host "Downloading protoc $protocVersion..."
    Invoke-WebRequest -Uri $downloadUrl -OutFile $zipPath

    if (Test-Path $protocDir) {
        Remove-Item $protocDir -Recurse -Force
    }

    Expand-Archive -Path $zipPath -DestinationPath $protocDir

    if (-not (Test-Path $protocExe)) {
        throw "Failed to provision protoc at $protocExe."
    }
}

Ensure-CargoPath
Ensure-RustTarget
Ensure-Protoc

$env:PROTOC = $protocExe

Push-Location $scriptRoot
try {
    if ($BinaryOnly) {
        if (-not (Test-Path $binaryPath)) {
            throw "Missing built binary at $binaryPath. Run this script without -BinaryOnly first."
        }

        Start-Process -FilePath $binaryPath -WorkingDirectory $scriptRoot | Out-Null
        Write-Host "Started $binaryPath"
        return
    }

    if ($BuildOnly) {
        & cargo build -p warp --bin warp-oss --no-default-features --features omw_local
        return
    }

    & cargo run -p warp --bin warp-oss --no-default-features --features omw_local
}
finally {
    Pop-Location
}
