$ErrorActionPreference = 'Stop'
$projectDir = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$toolDir = Join-Path $projectDir 'src-tauri\tools'
$enginePath = Join-Path $toolDir 'restic.exe'
$engineHash = '40576f77c1d40245a9f4af92a0b37b0d2514e6be0dffbf16ca8855820c13693e'
function Get-Sha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $sha.Dispose() }
}
if ((Test-Path -LiteralPath $enginePath -PathType Leaf) -and (Get-Sha256 $enginePath) -eq $engineHash) {
    Write-Output 'Verified restic 0.19.0.'
    exit 0
}
New-Item -ItemType Directory -Force -Path $toolDir | Out-Null
$archive = Join-Path $toolDir 'restic-0.19.0.zip'
Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/restic/restic/releases/download/v0.19.0/restic_0.19.0_windows_amd64.zip' -OutFile $archive
if ((Get-Sha256 $archive) -ne '6fa4219a70b1b5d1c429bb106a7f97f3d2a5aab74494db2e490b625edc486d8f') {
    throw 'Restic archive SHA-256 mismatch.'
}
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($archive)
try {
    $entry = $zip.GetEntry('restic_0.19.0_windows_amd64.exe')
    if (-not $entry) { throw 'Restic executable missing from archive.' }
    [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $enginePath, $true)
} finally { $zip.Dispose() }
if ((Get-Sha256 $enginePath) -ne $engineHash) { throw 'Restic executable SHA-256 mismatch.' }
Write-Output 'Prepared verified restic 0.19.0.'
