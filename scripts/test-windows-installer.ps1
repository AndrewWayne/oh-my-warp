# Exercise the production installer in a fresh per-user path.

#requires -Version 5.1

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [Parameter(Mandatory = $true)][string]$StagingDirectory,
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$InstallDirectory
)

$ErrorActionPreference = "Stop"

$InstallerPath = (Resolve-Path -LiteralPath $InstallerPath).Path
$StagingDirectory = (Resolve-Path -LiteralPath $StagingDirectory).Path
if (-not $InstallDirectory) {
    # Keep the generated name short. NSIS extracts through the Win32 APIs without
    # \\?\ prefixes, so "$InstallDirectory\app.new\<payload path>" is capped at
    # MAX_PATH. A full 32-hex GUID here put the deepest node_modules entry at
    # 259-262 characters on hosted runners and the install failed with error 13.
    $suffix = [guid]::NewGuid().ToString('N').Substring(0, 8)
    $InstallDirectory = Join-Path ([IO.Path]::GetTempPath()) "omw-smoke-$suffix"
}
$InstallDirectory = [IO.Path]::GetFullPath($InstallDirectory)
$installRoot = [IO.Path]::GetPathRoot($InstallDirectory)
if ($InstallDirectory -eq $installRoot -or $InstallDirectory.Length -le $installRoot.Length + 8) {
    throw "refusing unsafe installer test directory: $InstallDirectory"
}

# Fail here, with the numbers, rather than as an opaque installer error 13.
$maxPathLength = 259
$stagingManifest = Join-Path $StagingDirectory 'SHA256SUMS'
if (-not (Test-Path -LiteralPath $stagingManifest -PathType Leaf)) {
    throw "staging manifest is missing: $stagingManifest"
}
$longestPayloadPath = 0
foreach ($line in Get-Content -LiteralPath $stagingManifest) {
    $match = [regex]::Match($line, '^[0-9a-fA-F]{64}  ([^\r\n]+)$')
    if ($match.Success -and $match.Groups[1].Value.Length -gt $longestPayloadPath) {
        $longestPayloadPath = $match.Groups[1].Value.Length
    }
}
$longestInstalledPath = $InstallDirectory.Length + '\app.new\'.Length + $longestPayloadPath
if ($longestInstalledPath -gt $maxPathLength) {
    throw ("installer test directory is too deep: '$InstallDirectory' plus the longest payload " +
        "path ($longestPayloadPath chars) reaches $longestInstalledPath characters, over the " +
        "$maxPathLength-character MAX_PATH limit. Pass a shorter -InstallDirectory.")
}
if (Test-Path -LiteralPath $InstallDirectory) {
    throw "installer test directory already exists: $InstallDirectory"
}

$uninstallRegistryPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\omw.local.warpOss'
$startMenuDirectory = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\omw'
if (Test-Path -LiteralPath $uninstallRegistryPath) {
    throw "an omw installer registration already exists; run this smoke test in a clean account"
}
if (Test-Path -LiteralPath $startMenuDirectory) {
    throw "an omw Start Menu folder already exists; run this smoke test in a clean account"
}

function Invoke-InstallerProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    $process = Start-Process -FilePath $FilePath -ArgumentList $Arguments -Wait -PassThru
    return $process.ExitCode
}

function Assert-InstalledPayloadMatches {
    param(
        [Parameter(Mandatory = $true)][string]$ExpectedRoot,
        [Parameter(Mandatory = $true)][string]$ActualRoot
    )

    $expectedManifest = Join-Path $ExpectedRoot 'SHA256SUMS'
    $actualManifest = Join-Path $ActualRoot 'SHA256SUMS'
    $expectedManifestHash = (Get-FileHash -LiteralPath $expectedManifest -Algorithm SHA256).Hash
    $actualManifestHash = (Get-FileHash -LiteralPath $actualManifest -Algorithm SHA256).Hash
    if ($actualManifestHash -cne $expectedManifestHash) {
        throw "installed SHA256SUMS differs from the verified staging manifest"
    }

    $expectedHashes = @{}
    foreach ($line in Get-Content -LiteralPath $expectedManifest) {
        $match = [regex]::Match($line, '^([0-9a-fA-F]{64})  ([^\r\n]+)$')
        if (-not $match.Success) {
            throw "invalid verified staging manifest line"
        }
        $expectedHashes[$match.Groups[2].Value.Replace('/', '\')] = $match.Groups[1].Value
    }

    $actualFiles = @(Get-ChildItem -LiteralPath $ActualRoot -Recurse -File |
        Where-Object { $_.FullName -ne $actualManifest })
    if ($expectedHashes.Count -ne $actualFiles.Count) {
        throw "installed payload file count mismatch: expected $($expectedHashes.Count), got $($actualFiles.Count)"
    }

    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        foreach ($actual in $actualFiles) {
            $relative = $actual.FullName.Substring($ActualRoot.Length + 1)
            if (-not $expectedHashes.ContainsKey($relative)) {
                throw "installed payload contains unexpected file $relative"
            }
            $stream = [IO.File]::Open(
                $actual.FullName,
                [IO.FileMode]::Open,
                [IO.FileAccess]::Read,
                [IO.FileShare]::Read
            )
            try {
                $hashBytes = $sha256.ComputeHash($stream)
            }
            finally {
                $stream.Dispose()
            }
            $actualHash = [BitConverter]::ToString($hashBytes).Replace('-', '')
            if ($actualHash -ine $expectedHashes[$relative]) {
                throw "installed payload hash mismatch for $relative"
            }
        }
    }
    finally {
        $sha256.Dispose()
    }

    foreach ($relative in $expectedHashes.Keys) {
        if (-not (Test-Path -LiteralPath (Join-Path $ActualRoot $relative) -PathType Leaf)) {
            throw "installed payload is missing $relative"
        }
    }
}

$appDirectory = Join-Path $InstallDirectory 'app'
$mainExecutable = Join-Path $appDirectory 'omw-warp-oss.exe'
$uninstaller = Join-Path $InstallDirectory 'Uninstall.exe'
$startMenuShortcut = Join-Path $startMenuDirectory 'omw.lnk'
$cleanupRequired = $false

Write-Host "==> Install directory: $InstallDirectory"
Write-Host "    longest installed payload path: $longestInstalledPath of $maxPathLength characters"

try {
    Write-Host "==> Silent first install"
    $installExit = Invoke-InstallerProcess $InstallerPath @('/S', "/D=$InstallDirectory")
    if ($installExit -ne 0) {
        throw "installer exited with $installExit"
    }
    $cleanupRequired = $true

    foreach ($required in @($mainExecutable, $uninstaller, $startMenuShortcut)) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
            throw "installation is missing $required"
        }
    }
    Assert-InstalledPayloadMatches $StagingDirectory $appDirectory

    $registration = Get-ItemProperty -LiteralPath $uninstallRegistryPath
    if ($registration.DisplayName -ne 'omw' -or $registration.DisplayVersion -ne $Version) {
        throw "Apps & Features registration contains unexpected product metadata"
    }
    if ([IO.Path]::GetFullPath($registration.InstallLocation) -ne $InstallDirectory) {
        throw "Apps & Features registration contains the wrong install location"
    }
    if ($registration.UninstallString -ne ('"{0}"' -f $uninstaller)) {
        throw "Apps & Features registration contains the wrong uninstaller"
    }

    $shell = New-Object -ComObject WScript.Shell
    try {
        $shortcut = $shell.CreateShortcut($startMenuShortcut)
        if ([IO.Path]::GetFullPath($shortcut.TargetPath) -ne $mainExecutable) {
            throw "Start Menu shortcut targets the wrong executable"
        }
    }
    finally {
        [Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell) | Out-Null
    }

    Write-Host "==> Locked upgrade must fail without damaging the current install"
    $mainHashBefore = (Get-FileHash -LiteralPath $mainExecutable -Algorithm SHA256).Hash
    $lock = [IO.File]::Open($mainExecutable, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        $lockedExit = Invoke-InstallerProcess $InstallerPath @('/S', "/D=$InstallDirectory")
    }
    finally {
        $lock.Dispose()
    }
    if ($lockedExit -ne 12) {
        throw "locked upgrade returned $lockedExit instead of 12"
    }
    $mainHashAfter = (Get-FileHash -LiteralPath $mainExecutable -Algorithm SHA256).Hash
    if ($mainHashAfter -cne $mainHashBefore) {
        throw "locked upgrade changed the installed executable"
    }

    Write-Host "==> Successful replacement upgrade"
    $staleSentinel = Join-Path $appDirectory 'installer-upgrade-stale-sentinel.txt'
    Set-Content -LiteralPath $staleSentinel -Value 'must be removed by replacement upgrade' -Encoding Ascii
    $upgradeExit = Invoke-InstallerProcess $InstallerPath @('/S', "/D=$InstallDirectory")
    if ($upgradeExit -ne 0) {
        throw "replacement upgrade exited with $upgradeExit"
    }
    if (Test-Path -LiteralPath $staleSentinel) {
        throw "replacement upgrade retained a stale payload file"
    }
    Assert-InstalledPayloadMatches $StagingDirectory $appDirectory

    Write-Host "==> Silent uninstall"
    $uninstallExit = Invoke-InstallerProcess $uninstaller @('/S')
    if ($uninstallExit -ne 0) {
        throw "uninstaller exited with $uninstallExit"
    }
    $cleanupRequired = $false

    foreach ($removed in @($InstallDirectory, $uninstallRegistryPath, $startMenuDirectory)) {
        if (Test-Path -LiteralPath $removed) {
            throw "uninstaller retained $removed"
        }
    }

    Write-Host "==> Installer smoke PASS"
}
finally {
    if ($cleanupRequired -and (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
        $null = Invoke-InstallerProcess $uninstaller @('/S')
    }
    if (Test-Path -LiteralPath $InstallDirectory) {
        Remove-Item -LiteralPath $InstallDirectory -Recurse -Force
    }
    if (Test-Path -LiteralPath $uninstallRegistryPath) {
        Remove-Item -LiteralPath $uninstallRegistryPath -Recurse -Force
    }
    if (Test-Path -LiteralPath $startMenuDirectory) {
        Remove-Item -LiteralPath $startMenuDirectory -Recurse -Force
    }
}
