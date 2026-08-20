# Build a self-contained omw-local preview ZIP for x86_64 Windows.
#
# Usage:
#   pwsh -File scripts/build-windows-zip.ps1 -Version 0.0.11
#
# The optional CargoConfig/Offline switches are useful for an audited local
# source cache; release CI normally leaves both unset.

#requires -Version 5.1

[CmdletBinding()]
param(
    [ValidatePattern("^[0-9A-Za-z][0-9A-Za-z._-]*$")]
    [string]$Version = "0.0.1",

    [ValidatePattern("^[0-9]+\.[0-9]+\.[0-9]+$")]
    [string]$NodeVersion = "22.11.0",

    [uri]$NodeMirror = "https://nodejs.org/dist",

    [string]$CargoConfig,

    [switch]$Offline
)

$ErrorActionPreference = "Stop"

$TargetTriple = "x86_64-pc-windows-msvc"
$NodeArchitecture = "win-x64"

$scriptRoot = $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $scriptRoot "..")).Path
$vendorDir = Join-Path $repoRoot "vendor\warp-stripped"
$agentDir = Join-Path $repoRoot "apps\omw-agent"
$distDir = Join-Path $repoRoot "dist"

function Invoke-CheckedCommand {
    param(
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$ArgumentList,
        [Parameter(Mandatory = $true)][string]$FailureMessage
    )

    Push-Location $WorkingDirectory
    try {
        & $FilePath @ArgumentList
        if ($LASTEXITCODE -ne 0) {
            throw "$FailureMessage (exit $LASTEXITCODE)"
        }
    }
    finally {
        Pop-Location
    }
}

function Get-NodeDistribution {
    param(
        [Parameter(Mandatory = $true)][string]$RequestedVersion,
        [Parameter(Mandatory = $true)][string]$Architecture,
        [Parameter(Mandatory = $true)][uri]$ArchiveMirror
    )

    if (-not $env:LOCALAPPDATA) {
        throw "LOCALAPPDATA is not set"
    }

    $packageName = "node-v$RequestedVersion-$Architecture"
    $archiveName = "$packageName.zip"
    $officialBaseUrl = "https://nodejs.org/dist/v$RequestedVersion"
    $archiveBaseUrl = "$($ArchiveMirror.AbsoluteUri.TrimEnd('/'))/v$RequestedVersion"
    $cacheDir = Join-Path $env:LOCALAPPDATA "oh-my-warp\node"
    $archivePath = Join-Path $cacheDir $archiveName
    $checksumsPath = Join-Path $cacheDir "SHASUMS256-v$RequestedVersion.txt"
    $packageDir = Join-Path $cacheDir $packageName
    $nodeExe = Join-Path $packageDir "node.exe"

    New-Item -ItemType Directory -Path $cacheDir -Force | Out-Null

    if (-not (Test-Path -LiteralPath $checksumsPath -PathType Leaf)) {
        $partial = "$checksumsPath.partial"
        Remove-Item -LiteralPath $partial -Force -ErrorAction SilentlyContinue
        Invoke-WebRequest -UseBasicParsing -Uri "$officialBaseUrl/SHASUMS256.txt" -OutFile $partial
        Move-Item -LiteralPath $partial -Destination $checksumsPath
    }

    if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) {
        $partial = "$archivePath.partial"
        Remove-Item -LiteralPath $partial -Force -ErrorAction SilentlyContinue
        Write-Host "    downloading $archiveName ..."
        $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
        if ($curl) {
            & $curl.Source --fail --location --retry 3 --silent --show-error --output $partial "$archiveBaseUrl/$archiveName"
            if ($LASTEXITCODE -ne 0) {
                throw "Node archive download failed (curl exit $LASTEXITCODE)"
            }
        }
        else {
            Invoke-WebRequest -UseBasicParsing -Uri "$archiveBaseUrl/$archiveName" -OutFile $partial
        }
        Move-Item -LiteralPath $partial -Destination $archivePath
    }

    $escapedArchiveName = [regex]::Escape($archiveName)
    $expectedHash = $null
    foreach ($line in Get-Content -LiteralPath $checksumsPath) {
        $match = [regex]::Match($line, "^([0-9a-fA-F]{64})\s+\*?$escapedArchiveName$")
        if ($match.Success) {
            $expectedHash = $match.Groups[1].Value
            break
        }
    }
    if (-not $expectedHash) {
        throw "no checksum for $archiveName in $checksumsPath"
    }

    $actualHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash
    if ($actualHash -ine $expectedHash) {
        throw "Node archive SHA-256 mismatch (expected $expectedHash, got $actualHash)"
    }

    if (-not (Test-Path -LiteralPath $nodeExe -PathType Leaf)) {
        if (Test-Path -LiteralPath $packageDir) {
            Remove-Item -LiteralPath $packageDir -Recurse -Force
        }
        Expand-Archive -LiteralPath $archivePath -DestinationPath $cacheDir
    }
    if (-not (Test-Path -LiteralPath $nodeExe -PathType Leaf)) {
        throw "Node distribution did not contain $nodeExe"
    }

    return $packageDir
}

function Get-VcRuntime {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
        throw "vswhere.exe not found; install the Visual C++ build tools"
    }

    $installRoot = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1)
    if (-not $installRoot) {
        throw "Visual C++ x64 build tools not found"
    }

    $redistRoot = Join-Path $installRoot "VC\Redist\MSVC"
    $runtime = Get-ChildItem -LiteralPath $redistRoot -Directory |
        Sort-Object Name -Descending |
        ForEach-Object {
            Join-Path $_.FullName "x64\Microsoft.VC143.CRT\vcruntime140.dll"
        } |
        Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
        Select-Object -First 1
    if (-not $runtime) {
        throw "vcruntime140.dll not found below $redistRoot"
    }

    return $runtime
}

function Invoke-ProbeProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string]$Arguments,
        [Parameter(Mandatory = $true)][hashtable]$Environment,
        [int]$TimeoutMilliseconds = 15000
    )

    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $FilePath
    $startInfo.Arguments = $Arguments
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($entry in $Environment.GetEnumerator()) {
        $startInfo.EnvironmentVariables[$entry.Key] = $entry.Value
    }

    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "failed to start $FilePath"
    }
    $process.StandardInput.Close()
    if (-not $process.WaitForExit($TimeoutMilliseconds)) {
        $process.Kill()
        throw "process timed out after $TimeoutMilliseconds ms: $FilePath $Arguments"
    }

    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $exitCode = $process.ExitCode
    $process.Dispose()

    return [pscustomobject]@{
        ExitCode = $exitCode
        Stdout = $stdout
        Stderr = $stderr
    }
}

foreach ($requiredDir in @($vendorDir, $agentDir)) {
    if (-not (Test-Path -LiteralPath $requiredDir -PathType Container)) {
        throw "missing required directory $requiredDir"
    }
}
foreach ($command in @("cargo", "node", "npm.cmd")) {
    if (-not (Get-Command $command -ErrorAction SilentlyContinue)) {
        throw "$command not on PATH"
    }
}

if ($CargoConfig) {
    $CargoConfig = (Resolve-Path -LiteralPath $CargoConfig).Path
}

if (-not $env:PROTOC) {
    $protocCmd = Get-Command protoc -ErrorAction SilentlyContinue
    if ($protocCmd) {
        $env:PROTOC = $protocCmd.Source
    }
}
if (-not $env:PROTOC -or -not (Test-Path -LiteralPath $env:PROTOC -PathType Leaf)) {
    throw "PROTOC env var not set or protoc not found on PATH"
}

$cargoNetworkArgs = @("--locked")
if ($Offline) {
    $cargoNetworkArgs += "--offline"
}
if (-not $env:CARGO_BUILD_JOBS) {
    $env:CARGO_BUILD_JOBS = "4"
}

$env:GIT_RELEASE_TAG = "omw-local-preview-v$Version"

Write-Host "==> Building omw_local release binary (version $Version) ..."
$warpBuildArgs = @(
    "build", "--release", "-p", "warp", "--bin", "warp-oss",
    "--no-default-features", "--features", "omw_local"
) + $cargoNetworkArgs
# The upstream release profile embeds full debug information. On Windows the
# monolithic warp crate exhausts a 16 GB host during LLVM's final codegen with
# debuginfo=2. Keep dependency fingerprints intact, limit the monolithic
# package's parallel LLVM units, and disable debug info only for the shipped
# warp package; runtime behavior and optimization stay release.
$warpBuildArgs += @(
    "--config", "profile.release.package.warp.debug=0",
    "--config", "profile.release.package.warp.codegen-units=4"
)
if ($CargoConfig) {
    $warpBuildArgs += @("--config", $CargoConfig)
}
Invoke-CheckedCommand $vendorDir "cargo" $warpBuildArgs "Warp cargo build failed"

$binary = Join-Path $vendorDir "target\release\warp-oss.exe"
if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
    throw "build did not produce $binary"
}

Write-Host "==> Building omw-agent kernel (TypeScript -> dist/) ..."
Invoke-CheckedCommand $repoRoot "npm.cmd" @(
    "ci", "--no-fund", "--no-audit", "--workspace", "@oh-my-warp/omw-agent",
    "--include-workspace-root=false"
) "omw-agent dependency install failed"
Invoke-CheckedCommand $repoRoot "npm.cmd" @(
    "run", "build", "--workspace", "@oh-my-warp/omw-agent"
) "omw-agent build failed"
$agentEntry = Join-Path $agentDir "dist\src\serve.js"
if (-not (Test-Path -LiteralPath $agentEntry -PathType Leaf)) {
    throw "omw-agent build did not produce $agentEntry"
}

Write-Host "==> Building native Windows keychain helper ..."
$helperBuildArgs = @("build", "--release", "-p", "omw-keychain-helper") + $cargoNetworkArgs
Invoke-CheckedCommand $repoRoot "cargo" $helperBuildArgs "omw-keychain-helper cargo build failed"
$helperBinary = Join-Path $repoRoot "target\release\omw-keychain-helper.exe"
if (-not (Test-Path -LiteralPath $helperBinary -PathType Leaf)) {
    throw "build did not produce $helperBinary"
}

Write-Host "==> Acquiring Node v$NodeVersion for the package ..."
$nodeDistribution = Get-NodeDistribution $NodeVersion $NodeArchitecture $NodeMirror
$nodeBinary = Join-Path $nodeDistribution "node.exe"
$nodeLicense = Join-Path $nodeDistribution "LICENSE"
if (-not (Test-Path -LiteralPath $nodeLicense -PathType Leaf)) {
    throw "Node distribution license missing at $nodeLicense"
}

Write-Host "==> Auditing binary for forbidden hostnames ..."
$bash = Get-Command bash.exe -ErrorAction SilentlyContinue
if (-not $bash) {
    throw "bash.exe not on PATH (required for audit-no-cloud.sh)"
}
Invoke-CheckedCommand $vendorDir $bash.Source @(
    "scripts/audit-no-cloud.sh", "target/release/warp-oss.exe"
) "audit-no-cloud failed"

Write-Host "==> Staging self-contained ZIP payload ..."
New-Item -ItemType Directory -Path $distDir -Force | Out-Null
$staging = Join-Path $distDir "staging-v$Version-windows"
$isolatedAgent = Join-Path $distDir "isolated-omw-agent-v$Version-windows"
foreach ($path in @($staging, $isolatedAgent)) {
    if (Test-Path -LiteralPath $path) {
        Remove-Item -LiteralPath $path -Recurse -Force
    }
}
New-Item -ItemType Directory -Path $staging -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $staging "bin") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $staging "x64") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $staging "third_party\node") -Force | Out-Null

Copy-Item -LiteralPath $binary -Destination (Join-Path $staging "omw-warp-oss.exe")
Copy-Item -LiteralPath $helperBinary -Destination (Join-Path $staging "omw-keychain-helper.exe")
Copy-Item -LiteralPath $nodeBinary -Destination (Join-Path $staging "bin\node.exe")
Copy-Item -LiteralPath $nodeLicense -Destination (Join-Path $staging "third_party\node\LICENSE")
Copy-Item -LiteralPath (Join-Path $agentDir "bin\omw-agent.mjs") -Destination (Join-Path $staging "bin\omw-agent.mjs")
Copy-Item -LiteralPath (Join-Path $agentDir "package.json") -Destination (Join-Path $staging "package.json")
Copy-Item -LiteralPath (Join-Path $agentDir "dist") -Destination (Join-Path $staging "dist") -Recurse
Copy-Item -LiteralPath (Join-Path $agentDir "vendor") -Destination (Join-Path $staging "vendor") -Recurse
Copy-Item -LiteralPath (Join-Path $repoRoot "LICENSE") -Destination (Join-Path $staging "LICENSE")

$runtimeFiles = @(
    @{ Source = "conpty.dll"; Destination = "conpty.dll" },
    @{ Source = "dxcompiler.dll"; Destination = "dxcompiler.dll" },
    @{ Source = "dxil.dll"; Destination = "dxil.dll" },
    @{ Source = "x64\OpenConsole.exe"; Destination = "x64\OpenConsole.exe" }
)
foreach ($runtimeFile in $runtimeFiles) {
    $source = Join-Path (Join-Path $vendorDir "target\release") $runtimeFile.Source
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Warp build did not produce runtime file $source"
    }
    Copy-Item -LiteralPath $source -Destination (Join-Path $staging $runtimeFile.Destination)
}

$vcRuntime = Get-VcRuntime
Copy-Item -LiteralPath $vcRuntime -Destination (Join-Path $staging "vcruntime140.dll")

$notesPath = Join-Path $repoRoot "RELEASE_NOTES_v$Version.md"
if (Test-Path -LiteralPath $notesPath -PathType Leaf) {
    Copy-Item -LiteralPath $notesPath -Destination (Join-Path $staging "README.md")
}

Write-Host "==> Materializing isolated production node_modules ..."
New-Item -ItemType Directory -Path $isolatedAgent -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $agentDir "package.json") -Destination (Join-Path $isolatedAgent "package.json")
Invoke-CheckedCommand $isolatedAgent "npm.cmd" @(
    "install", "--no-fund", "--no-audit", "--omit=dev", "--package-lock=false"
) "isolated omw-agent production install failed"
$isolatedNodeModules = Join-Path $isolatedAgent "node_modules"
if (-not (Test-Path -LiteralPath $isolatedNodeModules -PathType Container)) {
    throw "isolated install produced no node_modules at $isolatedNodeModules"
}
$packagedNodeModules = Join-Path $staging "node_modules"
New-Item -ItemType Directory -Path $packagedNodeModules -Force | Out-Null
& robocopy.exe $isolatedNodeModules $packagedNodeModules /E /COPY:DAT /DCOPY:DAT /R:2 /W:1 /NFL /NDL /NJH /NJS /NP
$robocopyExit = $LASTEXITCODE
if ($robocopyExit -gt 7) {
    throw "copying production node_modules failed (robocopy exit $robocopyExit)"
}

$packageJson = Get-Content -LiteralPath (Join-Path $agentDir "package.json") -Raw | ConvertFrom-Json
$declaredDependencies = @($packageJson.dependencies.PSObject.Properties.Name)
if ($declaredDependencies.Count -eq 0) {
    throw "omw-agent package.json declares no runtime dependencies"
}
foreach ($dependency in $declaredDependencies) {
    $dependencyPath = Join-Path $packagedNodeModules $dependency
    if (-not (Test-Path -LiteralPath $dependencyPath -PathType Container)) {
        throw "declared dependency $dependency missing from packaged node_modules"
    }
}

Write-Host "==> Probing packaged helper and kernel with a developer-free PATH ..."
$cleanEnvironment = @{
    PATH = "$env:SystemRoot\System32;$env:SystemRoot"
    OMW_KEYCHAIN_BACKEND = "os"
}
$helperProbeName = "keychain:omw/package-probe-$([guid]::NewGuid().ToString('N'))"
$helperProbe = Invoke-ProbeProcess (Join-Path $staging "omw-keychain-helper.exe") "get $helperProbeName" $cleanEnvironment
if ($helperProbe.ExitCode -ne 1 -or $helperProbe.Stdout.Length -ne 0 -or $helperProbe.Stderr.TrimEnd("`r", "`n") -ne "not found") {
    throw "packaged helper probe failed (exit=$($helperProbe.ExitCode), stdout='$($helperProbe.Stdout)', stderr='$($helperProbe.Stderr)')"
}

$kernelPath = Join-Path $staging "bin\omw-agent.mjs"
$kernelArguments = '"' + $kernelPath.Replace('"', '\"') + '" --serve-stdio'
$kernelProbe = Invoke-ProbeProcess (Join-Path $staging "bin\node.exe") $kernelArguments $cleanEnvironment
if ($kernelProbe.ExitCode -ne 0) {
    throw "packaged kernel probe failed (exit=$($kernelProbe.ExitCode), stderr='$($kernelProbe.Stderr)')"
}

$manifestPath = Join-Path $staging "SHA256SUMS"
$manifestLines = New-Object System.Collections.Generic.List[string]
$sha256 = [System.Security.Cryptography.SHA256]::Create()
try {
    foreach ($file in Get-ChildItem -LiteralPath $staging -File -Recurse | Sort-Object FullName) {
        $stream = [System.IO.File]::Open(
            $file.FullName,
            [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::Read,
            [System.IO.FileShare]::Read
        )
        try {
            $hashBytes = $sha256.ComputeHash($stream)
        }
        finally {
            $stream.Dispose()
        }
        $relative = $file.FullName.Substring($staging.Length + 1).Replace("\", "/")
        $hash = [System.BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant()
        $manifestLines.Add("$hash  $relative")
    }
}
finally {
    $sha256.Dispose()
}
Set-Content -LiteralPath $manifestPath -Value $manifestLines -Encoding Ascii

$zipPath = Join-Path $distDir "omw-warp-oss-v$Version-$TargetTriple.zip"
if (Test-Path -LiteralPath $zipPath) {
    Remove-Item -LiteralPath $zipPath -Force
}

Write-Host "==> Creating ZIP at $zipPath ..."
Compress-Archive -Path (Join-Path $staging "*") -DestinationPath $zipPath

Add-Type -AssemblyName System.IO.Compression.FileSystem
$requiredZipEntries = @(
    "omw-warp-oss.exe",
    "omw-keychain-helper.exe",
    "vcruntime140.dll",
    "conpty.dll",
    "dxcompiler.dll",
    "dxil.dll",
    "x64/OpenConsole.exe",
    "bin/node.exe",
    "bin/omw-agent.mjs",
    "dist/src/serve.js",
    "vendor/pi-agent-core/LICENSE",
    "node_modules/@iarna/toml/package.json",
    "node_modules/@mariozechner/pi-ai/package.json",
    "node_modules/typebox/package.json",
    "third_party/node/LICENSE",
    "SHA256SUMS"
)
$archive = [System.IO.Compression.ZipFile]::OpenRead($zipPath)
try {
    $entryNames = @($archive.Entries | ForEach-Object { $_.FullName.Replace("\", "/") })
    foreach ($requiredEntry in $requiredZipEntries) {
        if ($entryNames -notcontains $requiredEntry) {
            throw "ZIP is missing required entry $requiredEntry"
        }
    }
}
finally {
    $archive.Dispose()
}

Remove-Item -LiteralPath $isolatedAgent -Recurse -Force

$size = "{0:N2} MB" -f ((Get-Item -LiteralPath $zipPath).Length / 1MB)
$hash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()

Write-Host ""
Write-Host "==> Done."
Write-Host "Artifact: $zipPath"
Write-Host "Size:    $size"
Write-Host "SHA256:  $hash"
Write-Host "Payload: Warp + ConPTY/OpenConsole/DXC + VC runtime + Node v$NodeVersion + omw-agent + keychain helper"
