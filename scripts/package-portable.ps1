param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$Root
)

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
$rootPath = (Resolve-Path -LiteralPath $Root).Path
$launcherDir = Join-Path $rootPath 'Launcher'
$releaseDir = Join-Path $launcherDir 'release'
$assetsDir = Join-Path $launcherDir 'assets\icons'
$runtimeDir = Join-Path $launcherDir 'runtime'
$targetDir = Join-Path $PSScriptRoot '..\src-tauri\target\release'
$projectDir = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$materialsDir = (Resolve-Path -LiteralPath (Join-Path $projectDir '..\materials\packages\prerequisites\microsoft')).Path
$launcherSource = Join-Path $targetDir 'RetroDriveLauncher.exe'
$startSource = Join-Path $targetDir 'START.exe'
$buildMarkerPath = Join-Path $targetDir 'RETRODRIVE-PRODUCTION-BUILD.json'

if (-not (Test-Path -LiteralPath (Join-Path $rootPath 'RetroBat\RetroBat.exe') -PathType Leaf)) {
    throw 'RetroBat\RetroBat.exe is missing from the target root.'
}
if (-not (Test-Path -LiteralPath (Join-Path $rootPath 'RetroArch\retroarch.exe') -PathType Leaf)) {
    throw 'RetroArch\retroarch.exe is missing from the target root.'
}
if (-not (Test-Path -LiteralPath $buildMarkerPath -PathType Leaf)) {
    throw 'Production build marker is missing. Run pnpm build:release before packaging.'
}
$buildMarker = Get-Content -Raw -LiteralPath $buildMarkerPath | ConvertFrom-Json
$launcherSourceHash = Get-Sha256 $launcherSource
$startSourceHash = Get-Sha256 $startSource
if ($buildMarker.LauncherSHA256 -ne $launcherSourceHash) {
    throw 'RetroDriveLauncher.exe no longer matches the Tauri production build marker. Run pnpm build:release again.'
}
if ($buildMarker.StartSHA256 -ne $startSourceHash) {
    throw 'START.exe no longer matches the production build marker. Run pnpm build:release again.'
}

New-Item -ItemType Directory -Force -Path $launcherDir, $releaseDir, $assetsDir, $runtimeDir, (Join-Path $launcherDir 'data') | Out-Null
Copy-Item -LiteralPath $launcherSource -Destination (Join-Path $launcherDir 'RetroDriveLauncher.exe') -Force
Copy-Item -LiteralPath $startSource -Destination (Join-Path $rootPath 'START.exe') -Force
Copy-Item -LiteralPath (Join-Path $projectDir 'public\assets\icons\retrobat.png') -Destination $assetsDir -Force
Copy-Item -LiteralPath (Join-Path $projectDir 'public\assets\icons\retroarch.png') -Destination $assetsDir -Force
Copy-Item -LiteralPath (Join-Path $projectDir 'docs\USER-GUIDE.md') -Destination $releaseDir -Force

$runtimeFiles = @(
    'MicrosoftEdgeWebView2RuntimeInstallerX64.exe',
    'VC_redist.x64.exe',
    'VC_redist.x86.exe',
    'directx_Jun2010_redist.exe'
)
foreach ($runtimeFile in $runtimeFiles) {
    Copy-Item -LiteralPath (Join-Path $materialsDir $runtimeFile) -Destination $runtimeDir -Force
}

$files = @(
    (Join-Path $rootPath 'START.exe'),
    (Join-Path $launcherDir 'RetroDriveLauncher.exe'),
    (Join-Path $rootPath 'launcher.json'),
    (Join-Path $assetsDir 'retrobat.png'),
    (Join-Path $assetsDir 'retroarch.png')
)
$files += $runtimeFiles | ForEach-Object { Join-Path $runtimeDir $_ }
$hashes = $files | ForEach-Object {
    [pscustomobject]@{
        Path = $_.Substring($rootPath.Length).TrimStart('\').Replace('\', '/')
        Algorithm = 'SHA256'
        Hash = Get-Sha256 $_
    }
}
$hashJson = $hashes | ConvertTo-Json
$hashPath = Join-Path $releaseDir 'BINARY-HASHES.json'
[System.IO.File]::WriteAllText($hashPath, $hashJson, [System.Text.UTF8Encoding]::new($false))

$gitCommit = (& git -C $projectDir rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) {
    throw 'Unable to resolve the Launcher Git commit.'
}
$gitStatus = @(& git -C $projectDir status --porcelain)
if ($LASTEXITCODE -ne 0) {
    throw 'Unable to read the Launcher Git working-tree status.'
}
$gitTag = (& git -C $projectDir describe --tags --exact-match HEAD 2>$null)
if ($LASTEXITCODE -ne 0) {
    $gitTag = $null
}
$provenance = [ordered]@{
    SchemaVersion = 1
    GitCommit = $gitCommit
    GitTag = if ($gitTag) { $gitTag.Trim() } else { $null }
    GitClean = ($gitStatus.Count -eq 0)
    RustToolchain = '1.97.1'
    NodeEngine = '24.x'
    Pnpm = '10.33.2'
    LauncherBuildCommand = $buildMarker.LauncherBuildCommand
    StartBuildCommand = $buildMarker.StartBuildCommand
    BuiltAtUtc = $buildMarker.BuiltAtUtc
    LauncherSHA256 = $launcherSourceHash
    StartSHA256 = $startSourceHash
}
$provenanceJson = $provenance | ConvertTo-Json
$provenancePath = Join-Path $releaseDir 'BUILD-PROVENANCE.json'
[System.IO.File]::WriteAllText($provenancePath, $provenanceJson, [System.Text.UTF8Encoding]::new($false))

Write-Output "Portable launcher copied to $rootPath"
