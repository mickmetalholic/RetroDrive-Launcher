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

    & cargo build --offline --release --manifest-path $manifestPath --bin START
    if ($LASTEXITCODE -ne 0) {
        throw "START.exe build failed with exit code $LASTEXITCODE."
    }
    if (-not (Test-Path -LiteralPath $startPath -PathType Leaf)) {
        throw 'START.exe build did not produce START.exe.'
    }

    $launcherHashAfter = Get-Sha256 $launcherPath
    if ($launcherHashBefore -ne $launcherHashAfter) {
        throw 'Building START.exe unexpectedly changed RetroDriveLauncher.exe.'
    }

    $marker = [ordered]@{
        SchemaVersion = 1
        BuiltAtUtc = [DateTime]::UtcNow.ToString('o')
        LauncherSHA256 = $launcherHashAfter
        StartSHA256 = Get-Sha256 $startPath
        LauncherBuildCommand = 'pnpm tauri:build'
        StartBuildCommand = 'cargo build --offline --release --manifest-path src-tauri/Cargo.toml --bin START'
    }
    $markerJson = $marker | ConvertTo-Json
    [System.IO.File]::WriteAllText($markerPath, $markerJson, [System.Text.UTF8Encoding]::new($false))
    Write-Output "Production binaries built and recorded at $markerPath"
}
finally {
    Pop-Location
}
