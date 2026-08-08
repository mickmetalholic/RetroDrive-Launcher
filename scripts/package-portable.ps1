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
$skillsDir = Join-Path $launcherDir 'skills'
$targetDir = Join-Path $PSScriptRoot '..\src-tauri\target\release'
$projectDir = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$skillsSourceDir = Join-Path $projectDir 'skills'
$materialsDir = (Resolve-Path -LiteralPath (Join-Path $projectDir '..\materials\packages\prerequisites\microsoft')).Path
$launcherSource = Join-Path $targetDir 'RetroDriveLauncher.exe'
$startSource = Join-Path $targetDir 'START.exe'
$updaterSource = Join-Path $targetDir 'Updater.exe'
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
& pnpm --dir $projectDir skills:validate
if ($LASTEXITCODE -ne 0) {
    throw 'Skill validation failed. Run pnpm skills:manifest after changing a skill.'
}
$buildMarker = Get-Content -Raw -LiteralPath $buildMarkerPath | ConvertFrom-Json
$launcherSourceHash = Get-Sha256 $launcherSource
$startSourceHash = Get-Sha256 $startSource
$updaterSourceHash = Get-Sha256 $updaterSource
if ($buildMarker.LauncherSHA256 -ne $launcherSourceHash) {
    throw 'RetroDriveLauncher.exe no longer matches the Tauri production build marker. Run pnpm build:release again.'
}
if ($buildMarker.StartSHA256 -ne $startSourceHash) {
    throw 'START.exe no longer matches the production build marker. Run pnpm build:release again.'
}
if ($buildMarker.UpdaterSHA256 -ne $updaterSourceHash) {
    throw 'Updater.exe no longer matches the production build marker. Run pnpm build:release again.'
}

New-Item -ItemType Directory -Force -Path $launcherDir, $releaseDir, $assetsDir, $runtimeDir, $skillsDir, (Join-Path $launcherDir 'data') | Out-Null
Copy-Item -LiteralPath $launcherSource -Destination (Join-Path $launcherDir 'RetroDriveLauncher.exe') -Force
Copy-Item -LiteralPath $startSource -Destination (Join-Path $rootPath 'START.exe') -Force
Copy-Item -LiteralPath $updaterSource -Destination (Join-Path $launcherDir 'Updater.exe') -Force
Copy-Item -LiteralPath (Join-Path $projectDir 'public\assets\icons\retrobat.png') -Destination $assetsDir -Force
Copy-Item -LiteralPath (Join-Path $projectDir 'public\assets\icons\retroarch.png') -Destination $assetsDir -Force
Copy-Item -LiteralPath (Join-Path $projectDir 'docs\USER-GUIDE.md') -Destination $releaseDir -Force
if (Test-Path -LiteralPath $skillsDir) {
    Remove-Item -LiteralPath $skillsDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $skillsDir | Out-Null
Get-ChildItem -LiteralPath $skillsSourceDir | Copy-Item -Destination $skillsDir -Recurse -Force

$packageVersion = (Get-Content -Raw -LiteralPath (Join-Path $projectDir 'package.json') | ConvertFrom-Json).version
$versionRecord = [ordered]@{
    SchemaVersion = 1
    Version = $packageVersion
    UpdateChannel = 'stable'
    UpdateManifest = 'https://github.com/mickmetalholic/RetroDrive-Launcher/releases/latest/download/latest.json'
}
[System.IO.File]::WriteAllText(
    (Join-Path $releaseDir 'VERSION.json'),
    ($versionRecord | ConvertTo-Json),
    [System.Text.UTF8Encoding]::new($false)
)

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
    (Join-Path $launcherDir 'Updater.exe'),
    (Join-Path $rootPath 'launcher.json'),
    (Join-Path $assetsDir 'retrobat.png'),
    (Join-Path $assetsDir 'retroarch.png'),
    (Join-Path $releaseDir 'VERSION.json')
)
$files += Get-ChildItem -LiteralPath $skillsDir -File -Recurse | ForEach-Object FullName
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
$gitTag = (& git -C $projectDir tag --points-at HEAD | Select-Object -First 1)
if ($LASTEXITCODE -ne 0) {
    throw 'Unable to resolve the Launcher Git tag.'
}
$gitTag = if ($gitTag) { $gitTag.Trim() } else { $null }
$provenance = [ordered]@{
    SchemaVersion = 1
    GitCommit = $gitCommit
    GitTag = $gitTag
    GitClean = ($gitStatus.Count -eq 0)
    RustToolchain = '1.97.1'
    NodeEngine = '24.x'
    Pnpm = '10.33.2'
    LauncherBuildCommand = $buildMarker.LauncherBuildCommand
    HelperBuildCommand = $buildMarker.HelperBuildCommand
    BuiltAtUtc = $buildMarker.BuiltAtUtc
    LauncherSHA256 = $launcherSourceHash
    StartSHA256 = $startSourceHash
    UpdaterSHA256 = $updaterSourceHash
}
$provenanceJson = $provenance | ConvertTo-Json
$provenancePath = Join-Path $releaseDir 'BUILD-PROVENANCE.json'
[System.IO.File]::WriteAllText($provenancePath, $provenanceJson, [System.Text.UTF8Encoding]::new($false))

Write-Output "Portable launcher copied to $rootPath"
# Do not leak a successful packaging operation's Git probe status to callers.
$global:LASTEXITCODE = 0
