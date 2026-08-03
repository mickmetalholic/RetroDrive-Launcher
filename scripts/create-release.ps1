param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version,

    [string]$Tag = "v$Version",

    [string]$Notes = "Portable launcher update $Version"
)

$ErrorActionPreference = 'Stop'

function Get-Sha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try {
            return ([System.BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
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
$targetDir = Join-Path $projectDir 'src-tauri\target\release'
$markerPath = Join-Path $targetDir 'RETRODRIVE-PRODUCTION-BUILD.json'
$outputDir = Join-Path $projectDir 'artifacts\release'
$stagingDir = Join-Path $outputDir 'staging'
$archiveName = "RetroDrive-Launcher-v$Version.zip"
$archivePath = Join-Path $outputDir $archiveName
$manifestPath = Join-Path $outputDir 'latest.json'
$hashPath = Join-Path $outputDir 'SHA256SUMS.txt'
$packageVersion = (Get-Content -Raw -LiteralPath (Join-Path $projectDir 'package.json') | ConvertFrom-Json).version
$tauriVersion = (Get-Content -Raw -LiteralPath (Join-Path $projectDir 'src-tauri\tauri.conf.json') | ConvertFrom-Json).version
$cargoVersion = if ((Get-Content -Raw -LiteralPath (Join-Path $projectDir 'src-tauri\Cargo.toml')) -match '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"') { $Matches[1] } else { $null }

if ($Version -ne $packageVersion -or $Version -ne $tauriVersion -or $Version -ne $cargoVersion) {
    throw "Version mismatch: requested=$Version package=$packageVersion tauri=$tauriVersion cargo=$cargoVersion"
}
if (-not $env:RETRODRIVE_SIGNING_PRIVATE_KEY) {
    throw 'RETRODRIVE_SIGNING_PRIVATE_KEY is required to create a release.'
}
if (-not (Test-Path -LiteralPath $markerPath -PathType Leaf)) {
    throw 'Production build marker is missing. Run pnpm build:release first.'
}
$marker = Get-Content -Raw -LiteralPath $markerPath | ConvertFrom-Json
$sources = [ordered]@{
    'START.exe' = Join-Path $targetDir 'START.exe'
    'Launcher\RetroDriveLauncher.exe' = Join-Path $targetDir 'RetroDriveLauncher.exe'
    'Launcher\Updater.exe' = Join-Path $targetDir 'Updater.exe'
    'Launcher\assets\icons\retrobat.png' = Join-Path $projectDir 'public\assets\icons\retrobat.png'
    'Launcher\assets\icons\retroarch.png' = Join-Path $projectDir 'public\assets\icons\retroarch.png'
    'Launcher\release\USER-GUIDE.md' = Join-Path $projectDir 'docs\USER-GUIDE.md'
}
foreach ($entry in $sources.GetEnumerator()) {
    if (-not (Test-Path -LiteralPath $entry.Value -PathType Leaf)) {
        throw "Release source is missing: $($entry.Value)"
    }
}
if ((Get-Sha256 $sources['START.exe']) -ne $marker.StartSHA256.ToLowerInvariant() -or
    (Get-Sha256 $sources['Launcher\RetroDriveLauncher.exe']) -ne $marker.LauncherSHA256.ToLowerInvariant() -or
    (Get-Sha256 $sources['Launcher\Updater.exe']) -ne $marker.UpdaterSHA256.ToLowerInvariant()) {
    throw 'Release binaries do not match the production build marker.'
}

if (Test-Path -LiteralPath $outputDir) {
    $resolvedOutput = (Resolve-Path -LiteralPath $outputDir).Path
    $expectedOutput = [System.IO.Path]::GetFullPath((Join-Path $projectDir 'artifacts\release'))
    if ($resolvedOutput -ne $expectedOutput -or -not $resolvedOutput.StartsWith($projectDir, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to clear unexpected output directory: $resolvedOutput"
    }
    Remove-Item -LiteralPath $resolvedOutput -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $stagingDir | Out-Null
foreach ($entry in $sources.GetEnumerator()) {
    $destination = Join-Path $stagingDir $entry.Key
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination) | Out-Null
    Copy-Item -LiteralPath $entry.Value -Destination $destination
}
$versionRecord = [ordered]@{
    schemaVersion = 1
    version = $Version
    updateChannel = 'stable'
    updateManifest = 'https://github.com/mickmetalholic/RetroDrive-Launcher/releases/latest/download/latest.json'
}
$versionPath = Join-Path $stagingDir 'Launcher\release\VERSION.json'
[System.IO.File]::WriteAllText($versionPath, ($versionRecord | ConvertTo-Json), [System.Text.UTF8Encoding]::new($false))

Compress-Archive -LiteralPath (Get-ChildItem -LiteralPath $stagingDir | ForEach-Object FullName) -DestinationPath $archivePath -CompressionLevel Optimal
$publishedAt = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
& (Join-Path $targetDir 'release-manifest.exe') create `
    --version $Version `
    --tag $Tag `
    --repository 'mickmetalholic/RetroDrive-Launcher' `
    --archive $archivePath `
    --output $manifestPath `
    --notes $Notes `
    --published-at $publishedAt
if ($LASTEXITCODE -ne 0) {
    throw "Signed manifest generation failed with exit code $LASTEXITCODE."
}
$hashLines = @(
    "$(Get-Sha256 $archivePath)  $archiveName"
    "$(Get-Sha256 $manifestPath)  latest.json"
)
[System.IO.File]::WriteAllText($hashPath, ($hashLines -join "`n") + "`n", [System.Text.UTF8Encoding]::new($false))
Remove-Item -LiteralPath $stagingDir -Recurse -Force
Write-Output "Release assets created in $outputDir"
