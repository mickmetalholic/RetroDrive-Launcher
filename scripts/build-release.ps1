$ErrorActionPreference = 'Stop'

function Get-Sha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try {
            return ([System.BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '')
        }
        finally {
            $sha.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

$projectDir = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$manifestPath = Join-Path $projectDir 'src-tauri\Cargo.toml'
$targetDir = Join-Path $projectDir 'src-tauri\target\release'
$launcherPath = Join-Path $targetDir 'RetroDriveLauncher.exe'
$startPath = Join-Path $targetDir 'START.exe'
$updaterPath = Join-Path $targetDir 'Updater.exe'
$manifestToolPath = Join-Path $targetDir 'release-manifest.exe'
$markerPath = Join-Path $targetDir 'RETRODRIVE-PRODUCTION-BUILD.json'
$rustBin = Join-Path $env:USERPROFILE '.cargo\bin'
if ((Test-Path -LiteralPath (Join-Path $rustBin 'cargo.exe') -PathType Leaf) -and
    -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:Path = "$rustBin;$env:Path"
}

Push-Location $projectDir
try {
    & pnpm tauri:build
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri production build failed with exit code $LASTEXITCODE."
    }
    if (-not (Test-Path -LiteralPath $launcherPath -PathType Leaf)) {
        throw 'Tauri production build did not produce RetroDriveLauncher.exe.'
    }
    $launcherHashBefore = Get-Sha256 $launcherPath

    & cargo build --offline --release --manifest-path $manifestPath --bin START --bin Updater --bin release-manifest
    if ($LASTEXITCODE -ne 0) {
        throw "START.exe build failed with exit code $LASTEXITCODE."
    }
    if (-not (Test-Path -LiteralPath $startPath -PathType Leaf)) {
        throw 'START.exe build did not produce START.exe.'
    }
    if (-not (Test-Path -LiteralPath $updaterPath -PathType Leaf)) {
        throw 'Updater.exe build did not produce Updater.exe.'
    }
    if (-not (Test-Path -LiteralPath $manifestToolPath -PathType Leaf)) {
        throw 'Release build did not produce release-manifest.exe.'
    }

    $launcherHashAfter = Get-Sha256 $launcherPath
    if ($launcherHashBefore -ne $launcherHashAfter) {
        throw 'Building START.exe unexpectedly changed RetroDriveLauncher.exe.'
    }

    $marker = [ordered]@{
        SchemaVersion = 2
        BuiltAtUtc = [DateTime]::UtcNow.ToString('o')
        LauncherSHA256 = $launcherHashAfter
        StartSHA256 = Get-Sha256 $startPath
        UpdaterSHA256 = Get-Sha256 $updaterPath
        LauncherBuildCommand = 'pnpm tauri:build'
        HelperBuildCommand = 'cargo build --offline --release --manifest-path src-tauri/Cargo.toml --bin START --bin Updater --bin release-manifest'
    }
    $markerJson = $marker | ConvertTo-Json
    [System.IO.File]::WriteAllText($markerPath, $markerJson, [System.Text.UTF8Encoding]::new($false))
    Write-Output "Production binaries built and recorded at $markerPath"
}
finally {
    Pop-Location
}
