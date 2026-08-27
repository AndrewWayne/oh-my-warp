# Build a per-user Windows installer from an already verified portable payload.
#
# Usage:
#   pwsh -File scripts/build-windows-installer.ps1 -Version 0.0.12

#requires -Version 5.1

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern("^[0-9A-Za-z][0-9A-Za-z._-]*$")]
    [string]$Version,

    [string]$StagingDirectory,

    [string]$MakensisPath
)

$ErrorActionPreference = "Stop"

$TargetTriple = "x86_64-pc-windows-msvc"
$scriptRoot = $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $scriptRoot "..")).Path
$distDir = Join-Path $repoRoot "dist"
$installerSource = Join-Path $scriptRoot "windows\omw-installer.nsi"
$licensePath = Join-Path $repoRoot "LICENSE"
$installerIcon = Join-Path $repoRoot "vendor\warp-stripped\app\channels\local\icon\no-padding\icon.ico"

function Resolve-Makensis {
    param([string]$RequestedPath)

    if ($RequestedPath) {
        return (Resolve-Path -LiteralPath $RequestedPath).Path
    }

    $command = Get-Command makensis.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    $candidates = @()
    if (${env:ProgramFiles(x86)}) {
        $candidates += Join-Path ${env:ProgramFiles(x86)} "NSIS\makensis.exe"
    }
    if ($env:ProgramFiles) {
        $candidates += Join-Path $env:ProgramFiles "NSIS\makensis.exe"
    }
    if ($env:LOCALAPPDATA) {
        $candidates += Join-Path $env:LOCALAPPDATA "Programs\NSIS\makensis.exe"
    }

    $resolved = $candidates |
        Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
        Select-Object -First 1
    if (-not $resolved) {
        throw "makensis.exe was not found; install NSIS 3.x or pass -MakensisPath"
    }
    return (Resolve-Path -LiteralPath $resolved).Path
}

function Assert-SafeDefineValue {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Value
    )

    if ($Value.IndexOf('"') -ge 0 -or $Value.IndexOf("`r") -ge 0 -or $Value.IndexOf("`n") -ge 0) {
        throw "$Name contains a character that cannot be passed safely to makensis"
    }
}

function Test-PayloadManifest {
    param([Parameter(Mandatory = $true)][string]$PayloadRoot)

    $manifestPath = Join-Path $PayloadRoot "SHA256SUMS"
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw "payload manifest is missing: $manifestPath"
    }

    $expectedPaths = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
    $lineNumber = 0
    foreach ($line in Get-Content -LiteralPath $manifestPath) {
        $lineNumber++
        $match = [regex]::Match($line, '^([0-9a-fA-F]{64})  ([^\r\n]+)$')
        if (-not $match.Success) {
            throw "invalid SHA256SUMS line $lineNumber"
        }

        $expectedHash = $match.Groups[1].Value
        $relative = $match.Groups[2].Value.Replace('/', '\')
        if ([IO.Path]::IsPathRooted($relative) -or $relative.Split('\') -contains '..') {
            throw "unsafe path in SHA256SUMS: $relative"
        }
        if (-not $expectedPaths.Add($relative)) {
            throw "duplicate path in SHA256SUMS: $relative"
        }

        $file = Join-Path $PayloadRoot $relative
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
            throw "manifest entry is missing from the payload: $relative"
        }
        $actualHash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash
        if ($actualHash -ine $expectedHash) {
            throw "payload hash mismatch for $relative"
        }
    }

    $payloadFiles = @(Get-ChildItem -LiteralPath $PayloadRoot -Recurse -File |
        Where-Object { $_.FullName -ne $manifestPath })
    foreach ($file in $payloadFiles) {
        $relative = $file.FullName.Substring($PayloadRoot.Length + 1)
        if (-not $expectedPaths.Contains($relative)) {
            throw "payload file is absent from SHA256SUMS: $relative"
        }
    }
    if ($payloadFiles.Count -ne $expectedPaths.Count) {
        throw "payload file count does not match SHA256SUMS"
    }

    return [pscustomobject]@{
        FileCount = $payloadFiles.Count
        ByteCount = ($payloadFiles | Measure-Object -Property Length -Sum).Sum
    }
}

foreach ($requiredPath in @($installerSource, $licensePath, $installerIcon)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "missing installer input: $requiredPath"
    }
}

if (-not $StagingDirectory) {
    $StagingDirectory = Join-Path $distDir "staging-v$Version-windows"
}
$StagingDirectory = (Resolve-Path -LiteralPath $StagingDirectory).Path

$requiredPayloadFiles = @(
    "omw-warp-oss.exe",
    "omw-keychain-helper.exe",
    "bin\node.exe",
    "bin\omw-agent.mjs",
    "x64\OpenConsole.exe",
    "SHA256SUMS",
    "LICENSE"
)
foreach ($relative in $requiredPayloadFiles) {
    $path = Join-Path $StagingDirectory $relative
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "installer payload is missing required file $relative"
    }
}

$manifest = Test-PayloadManifest $StagingDirectory
$versionMatch = [regex]::Match($Version, '^(\d+)\.(\d+)\.(\d+)')
if (-not $versionMatch.Success) {
    throw "Windows installer versions must begin with major.minor.patch"
}
$numericParts = @(
    [int]$versionMatch.Groups[1].Value,
    [int]$versionMatch.Groups[2].Value,
    [int]$versionMatch.Groups[3].Value,
    0
)
if (@($numericParts | Where-Object { $_ -lt 0 -or $_ -gt 65535 }).Count -ne 0) {
    throw "Windows version components must be between 0 and 65535"
}
$numericVersion = $numericParts -join '.'
$estimatedSizeKb = [int64][Math]::Ceiling($manifest.ByteCount / 1KB) + 1024

New-Item -ItemType Directory -Path $distDir -Force | Out-Null
$outputPath = Join-Path $distDir "omw-warp-oss-v$Version-$TargetTriple-setup.exe"
$sidecarPath = "$outputPath.sha256"
$MakensisPath = Resolve-Makensis $MakensisPath

foreach ($pair in @(
    @{ Name = 'Version'; Value = $Version },
    @{ Name = 'Payload directory'; Value = $StagingDirectory },
    @{ Name = 'Output path'; Value = $outputPath },
    @{ Name = 'License path'; Value = $licensePath },
    @{ Name = 'Installer icon'; Value = $installerIcon }
)) {
    Assert-SafeDefineValue $pair.Name $pair.Value
}

Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $sidecarPath -Force -ErrorAction SilentlyContinue

Write-Host "==> Building Windows installer with $MakensisPath"
Write-Host "    version:  $Version ($numericVersion)"
Write-Host "    payload:  $($manifest.FileCount) files, $($manifest.ByteCount) bytes"
Write-Host "    output:   $outputPath"

$makensisArguments = @(
    "/V3",
    "/NOCD",
    "/DVERSION=$Version",
    "/DNUMERIC_VERSION=$numericVersion",
    "/DPAYLOAD_DIR=$StagingDirectory",
    "/DOUTPUT_FILE=$outputPath",
    "/DLICENSE_PATH=$licensePath",
    "/DINSTALLER_ICON=$installerIcon",
    "/DESTIMATED_SIZE_KB=$estimatedSizeKb",
    $installerSource
)
& $MakensisPath @makensisArguments
if ($LASTEXITCODE -ne 0) {
    throw "makensis failed with exit $LASTEXITCODE"
}
if (-not (Test-Path -LiteralPath $outputPath -PathType Leaf)) {
    throw "makensis did not produce $outputPath"
}

$stream = [IO.File]::OpenRead($outputPath)
try {
    if ($stream.ReadByte() -ne 0x4d -or $stream.ReadByte() -ne 0x5a) {
        throw "installer does not have a Windows PE MZ header"
    }
}
finally {
    $stream.Dispose()
}

$versionInfo = (Get-Item -LiteralPath $outputPath).VersionInfo
if ($versionInfo.ProductVersion -ne $Version -or $versionInfo.FileDescription -ne 'omw Windows installer') {
    throw "installer version metadata does not match the requested release"
}

$hash = (Get-FileHash -LiteralPath $outputPath -Algorithm SHA256).Hash.ToLowerInvariant()
$checksumLine = "$hash  $([IO.Path]::GetFileName($outputPath))`n"
[IO.File]::WriteAllText($sidecarPath, $checksumLine, [Text.Encoding]::ASCII)

Write-Host ""
Write-Host "==> Windows installer complete."
Write-Host "Artifact: $outputPath"
Write-Host "SHA256:  $hash"
Write-Host "Sidecar: $sidecarPath"
