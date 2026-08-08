[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$SourcePath,

    [string]$RetroDriveRoot,

    [ValidateSet('auto', 'psvita', 'ps3', 'xbox', 'xbox360', 'switch', 'generic')]
    [string]$Platform = 'auto',

    [switch]$Apply
)

$ErrorActionPreference = 'Stop'

function Resolve-RetroDriveRoot {
    param([string]$RequestedRoot)

    if ($RequestedRoot) {
        $resolved = (Resolve-Path -LiteralPath $RequestedRoot -ErrorAction Stop).Path
        if (-not (Test-Path -LiteralPath (Join-Path $resolved 'START.exe') -PathType Leaf) -or
            -not (Test-Path -LiteralPath (Join-Path $resolved 'launcher.json') -PathType Leaf)) {
            throw "The requested RetroDrive root is missing START.exe or launcher.json: $resolved"
        }
        return $resolved.TrimEnd('\')
    }

    $candidates = @(
        Get-PSDrive -PSProvider FileSystem | ForEach-Object {
            $candidate = $_.Root.TrimEnd('\')
            if ((Test-Path -LiteralPath (Join-Path $candidate 'START.exe') -PathType Leaf) -and
                (Test-Path -LiteralPath (Join-Path $candidate 'launcher.json') -PathType Leaf)) {
                $candidate
            }
        }
    ) | Select-Object -Unique

    if ($candidates.Count -ne 1) {
        throw "Unable to uniquely discover RetroDrive root. Supply -RetroDriveRoot explicitly. Candidates: $($candidates -join ', ')"
    }
    return $candidates[0]
}

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash
}

function Get-ContentStats([string]$Path) {
    $files = if (Test-Path -LiteralPath $Path -PathType Container) {
        @(Get-ChildItem -LiteralPath $Path -Recurse -File -ErrorAction Stop)
    } else {
        @((Get-Item -LiteralPath $Path -ErrorAction Stop))
    }
    [pscustomobject]@{
        Files = $files.Count
        Bytes = [int64](($files | Measure-Object -Property Length -Sum).Sum)
        Sha256 = if ($files.Count -eq 1) { Get-Sha256 $files[0].FullName } else { $null }
    }
}

function New-ImportWorkspace([string]$Root, [string]$Kind) {
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
    $workspace = Join-Path $Root "Launcher\data\imports\$Kind\$stamp-$([guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Force -Path $workspace | Out-Null
    return $workspace
}

function Get-SafeName([string]$Name) {
    $safe = [IO.Path]::GetFileName($Name) -replace '[<>:"/\\|?*\x00-\x1F]', '_'
    $safe = $safe.Trim().TrimEnd('.')
    if ([string]::IsNullOrWhiteSpace($safe)) { $safe = 'Untitled' }
    if ($safe -match '^(?i)(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?$') { $safe = "_$safe" }
    return $safe
}

function Get-TitleIds([string]$Path) {
    @(
        Get-ChildItem -LiteralPath $Path -Recurse -File -Filter 'param.sfo' -ErrorAction SilentlyContinue |
            ForEach-Object {
                if ($_.FullName -match '\\(?:app|patch)\\(?<id>[A-Z]{4}\d{5})\\sce_sys\\param\.sfo$') {
                    $Matches['id']
                }
            }
    ) | Select-Object -Unique
}

function Resolve-Platform([string]$Requested, [string]$Path) {
    if ($Requested -ne 'auto') { return $Requested }
    if ((Get-TitleIds $Path).Count -gt 0) { return 'psvita' }
    $extension = if (Test-Path -LiteralPath $Path -PathType Leaf) { [IO.Path]::GetExtension($Path).ToLowerInvariant() } else { '' }
    switch ($extension) {
        '.psvita' { 'psvita'; break }
        '.vpk' { 'psvita'; break }
        '.nsp' { 'switch'; break }
        '.xci' { 'switch'; break }
        '.xiso' { 'xbox'; break }
        default { 'generic' }
    }
}

function New-Plan([string]$Root, [string]$Source, [string]$ResolvedPlatform) {
    $stats = Get-ContentStats $Source
    $titleIds = @(Get-TitleIds $Source)
    if ($ResolvedPlatform -eq 'psvita') {
        $vitaRoot = Join-Path $Root 'RetroBat\saves\psvita\vita3k'
        $vitaExe = Join-Path $Root 'RetroBat\emulators\vita3k\Vita3K.exe'
        if (-not (Test-Path -LiteralPath $vitaExe -PathType Leaf)) {
            throw "Vita3K is missing from the RetroDrive root: $vitaExe"
        }
        if ($titleIds.Count -eq 0) {
            throw 'PSVita import requires an unpacked app/<TITLE_ID>/sce_sys/param.sfo content directory.'
        }
        return [pscustomobject]@{
            Mode = 'vita3k-install'
            Platform = $ResolvedPlatform
            Source = $Source
            Root = $Root
            Destination = $vitaRoot
            TitleIds = $titleIds
            Files = $stats.Files
            Bytes = $stats.Bytes
            Sha256 = $stats.Sha256
            Emulator = $vitaExe
            Backup = Join-Path $Root 'Launcher\data\imports\psvita\<timestamp>\'
            Staging = Join-Path $Root 'Launcher\data\imports\psvita\<timestamp>\staging\'
            Metadata = Join-Path $Root 'roms\psvita\<Display Name> [<TITLE_ID>].psvita'
        }
    }

    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) {
        throw "Generic import currently accepts a single ROM file; use a platform adapter for content directories: $Source"
    }
    $extension = [IO.Path]::GetExtension($Source).ToLowerInvariant()
    if ($extension -eq '.m3u') {
        throw 'Multi-disc playlists require the multi-disc adapter; keep the playlist and all referenced discs together.'
    }
    if ($ResolvedPlatform -eq 'ps3' -and $extension -in @('.pkg', '.rap', '.edat')) {
        throw 'PS3 packages and license files must be installed through RPCS3-owned storage; this adapter will not copy them into roms.'
    }
    if ($ResolvedPlatform -in @('xbox', 'xbox360') -and $extension -in @('.iso', '.xiso', '.xex')) {
        throw 'Xbox disc and executable content requires a format/readability decision before import; preserve the original and use the Xbox adapter.'
    }
    $system = switch ($ResolvedPlatform) {
        'ps3' { 'ps3' }
        'xbox' { 'xbox' }
        'xbox360' { 'xbox360' }
        'switch' { 'switch' }
        default { 'inbox' }
    }
    $destination = Join-Path (Join-Path $Root 'roms') $system
    [pscustomobject]@{
        Mode = 'copy'
        Platform = $ResolvedPlatform
        Source = $Source
        Root = $Root
        Destination = Join-Path $destination ([IO.Path]::GetFileName($Source))
        TitleIds = $titleIds
        Files = $stats.Files
        Bytes = $stats.Bytes
        Sha256 = $stats.Sha256
        Emulator = $null
        Backup = Join-Path $Root 'Launcher\data\imports\generic\<timestamp>\'
        Staging = Join-Path $Root 'Launcher\data\imports\generic\<timestamp>\staging\'
        Metadata = $null
    }
}

function Show-Plan($Plan) {
    [pscustomobject]@{
        Action = if ($Apply) { 'APPLY' } else { 'PLAN ONLY' }
        Mode = $Plan.Mode
        Platform = $Plan.Platform
        Source = $Plan.Source
        Destination = $Plan.Destination
        Backup = $Plan.Backup
        Staging = $Plan.Staging
        Metadata = $Plan.Metadata
        TitleIds = ($Plan.TitleIds -join ', ')
        Files = $Plan.Files
        Bytes = $Plan.Bytes
        Sha256 = $Plan.Sha256
    } | Format-List
}

function Invoke-GenericCopy($Plan) {
    if (Test-Path -LiteralPath $Plan.Destination) {
        if ((Get-Sha256 $Plan.Source) -eq (Get-Sha256 $Plan.Destination)) {
            Write-Output "Already imported: $($Plan.Destination)"
            return
        }
    }
    $workspace = New-ImportWorkspace $Plan.Root 'generic'
    $stage = Join-Path $workspace 'staging'
    $backup = Join-Path $workspace 'backup'
    $failed = Join-Path $workspace 'failed'
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    try {
        $stagedFile = Join-Path $stage ([IO.Path]::GetFileName($Plan.Source))
        Copy-Item -LiteralPath $Plan.Source -Destination $stagedFile
        if ((Get-Sha256 $Plan.Source) -ne (Get-Sha256 $stagedFile)) {
            throw "Staged SHA-256 verification failed: $stagedFile"
        }
        if (Test-Path -LiteralPath $Plan.Destination) {
            New-Item -ItemType Directory -Force -Path $backup | Out-Null
            Move-Item -LiteralPath $Plan.Destination -Destination $backup
        }
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Plan.Destination) | Out-Null
        Move-Item -LiteralPath $stagedFile -Destination $Plan.Destination
        if ((Get-Sha256 $Plan.Source) -ne (Get-Sha256 $Plan.Destination)) {
            throw "Post-copy SHA-256 verification failed: $($Plan.Destination)"
        }
        Write-Output "Imported and verified: $($Plan.Destination)"
    } catch {
        if (Test-Path -LiteralPath $Plan.Destination) {
            New-Item -ItemType Directory -Force -Path $failed | Out-Null
            Move-Item -LiteralPath $Plan.Destination -Destination $failed -Force
        }
        if (Test-Path -LiteralPath $backup) {
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Plan.Destination) | Out-Null
            Move-Item -LiteralPath (Join-Path $backup ([IO.Path]::GetFileName($Plan.Destination))) -Destination $Plan.Destination -Force -ErrorAction SilentlyContinue
        }
        throw
    } finally {
        if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
    }
}

function New-VitaBackup($Plan) {
    $workspace = New-ImportWorkspace $Plan.Root 'psvita'
    $backup = Join-Path $workspace 'backup'
    $moved = @()
    foreach ($titleId in $Plan.TitleIds) {
        foreach ($relative in @("RetroBat\saves\psvita\vita3k\ux0\app\$titleId", "RetroBat\saves\psvita\vita3k\ux0\app\${titleId}_dec", "RetroBat\saves\psvita\vita3k\ux0\license\$titleId")) {
            $path = Join-Path $Plan.Root $relative
            if (Test-Path -LiteralPath $path) {
                $target = Join-Path $backup $relative
                New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
                Move-Item -LiteralPath $path -Destination $target
                $moved += [pscustomobject]@{ Source = $path; Backup = $target }
            }
        }
    }
    [pscustomobject]@{ Workspace = $workspace; Backup = $backup; Moved = $moved }
}

function Restore-VitaBackup($Backup) {
    foreach ($item in @($Backup.Moved | Sort-Object Source -Descending)) {
        if (Test-Path -LiteralPath $item.Source) {
            Remove-Item -LiteralPath $item.Source -Recurse -Force
        }
        if (Test-Path -LiteralPath $item.Backup) {
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $item.Source) | Out-Null
            Move-Item -LiteralPath $item.Backup -Destination $item.Source
        }
    }
}

function Preserve-VitaFailure($Plan, $Backup) {
    $failed = Join-Path $Backup.Workspace 'failed'
    foreach ($titleId in $Plan.TitleIds) {
        foreach ($relative in @("RetroBat\saves\psvita\vita3k\ux0\app\$titleId", "RetroBat\saves\psvita\vita3k\ux0\app\${titleId}_dec", "RetroBat\saves\psvita\vita3k\ux0\license\$titleId")) {
            $path = Join-Path $Plan.Root $relative
            if (Test-Path -LiteralPath $path) {
                $target = Join-Path $failed $relative
                New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
                Move-Item -LiteralPath $path -Destination $target
            }
        }
    }
}

function New-VitaStubs($Plan) {
    $stubRoot = Join-Path $Plan.Root 'roms\psvita'
    New-Item -ItemType Directory -Force -Path $stubRoot | Out-Null
    $baseName = Get-SafeName (Split-Path -Leaf $Plan.Source)
    $stubs = @($Plan.TitleIds | ForEach-Object { Join-Path $stubRoot "$baseName [$($_)].psvita" })
    foreach ($stub in $stubs) {
        if (Test-Path -LiteralPath $stub) {
            if (-not (Test-Path -LiteralPath $stub -PathType Leaf) -or (Get-Item -LiteralPath $stub).Length -ne 0) {
                throw "Vita launch stub is not a zero-length file: $stub"
            }
        }
    }
    foreach ($stub in $stubs) {
        if (-not (Test-Path -LiteralPath $stub)) {
            [IO.File]::WriteAllBytes($stub, [byte[]]@())
        }
    }
}

function Test-VitaInstall($Plan) {
    foreach ($titleId in $Plan.TitleIds) {
        $appPath = Join-Path $Plan.Destination "ux0\app\$titleId"
        $decPath = Join-Path $Plan.Destination "ux0\app\${titleId}_dec"
        if (Test-Path -LiteralPath $decPath) { throw "Vita3K left an unfinished decrypt directory: $decPath" }
        $eboot = Join-Path $appPath 'eboot.bin'
        if (-not (Test-Path -LiteralPath $eboot -PathType Leaf)) { throw "Installed eboot.bin is missing: $eboot" }
        $head = [IO.File]::ReadAllBytes($eboot)
        if ($head.Length -lt 4 -or $head[0] -ne 0x53 -or $head[1] -ne 0x43 -or $head[2] -ne 0x45 -or $head[3] -ne 0) {
            throw "Installed eboot.bin is not a decrypted Vita SELF: $eboot"
        }
        $licenseDir = Join-Path $Plan.Destination "ux0\license\$titleId"
        $licenses = @(Get-ChildItem -LiteralPath $licenseDir -File -ErrorAction SilentlyContinue | Where-Object Length -eq 512)
        if ($licenses.Count -eq 0) { throw "A 512-byte Vita license is missing for $titleId" }
    }
}

function Invoke-VitaInstall($Plan) {
    $logPath = Join-Path (Split-Path -Parent $Plan.Emulator) 'vita3k.log'
    $beforeLog = if (Test-Path -LiteralPath $logPath) { Get-Content -LiteralPath $logPath -Raw -ErrorAction SilentlyContinue } else { '' }
    $arguments = @('-z', '-l', '2', '-w', '-f', '-c', (Join-Path (Split-Path -Parent $Plan.Emulator) 'config.yml'), $Plan.Source)
    $backup = New-VitaBackup $Plan
    $process = $null
    $deadline = (Get-Date).AddMinutes(20)
    $completed = $false
    try {
        $process = Start-Process -FilePath $Plan.Emulator -ArgumentList $arguments -WorkingDirectory (Split-Path -Parent $Plan.Emulator) -PassThru
        while ((Get-Date) -lt $deadline) {
            Start-Sleep -Seconds 2
            if (Test-Path -LiteralPath $logPath) {
                $current = Get-Content -LiteralPath $logPath -Raw -ErrorAction SilentlyContinue
                $delta = if ($current.StartsWith($beforeLog)) { $current.Substring($beforeLog.Length) } else { $current }
                if ($delta -match 'Successfully installed \d+ content!') { $completed = $true; break }
            }
            if ($process.HasExited) { break }
        }
        if (-not $completed) { throw 'Vita3K did not report successful content installation before the timeout.' }
        Test-VitaInstall $Plan
        New-VitaStubs $Plan
        Write-Output "Vita3K installed and verified: $($Plan.TitleIds -join ', ')"
    } catch {
        Preserve-VitaFailure $Plan $backup
        Restore-VitaBackup $backup
        throw
    } finally {
        if ($process -and -not $process.HasExited) {
            $process.CloseMainWindow() | Out-Null
            Start-Sleep -Seconds 3
            if (-not $process.HasExited) { $process | Stop-Process -Force }
        }
    }
}

$root = Resolve-RetroDriveRoot $RetroDriveRoot
$source = (Resolve-Path -LiteralPath $SourcePath -ErrorAction Stop).Path
$resolvedPlatform = Resolve-Platform $Platform $source
$plan = New-Plan $root $source $resolvedPlatform
Show-Plan $plan
if (-not $Apply) {
    Write-Output 'No files were changed. Re-run with -Apply after approving this plan.'
    exit 0
}

if ($plan.Mode -eq 'vita3k-install') { Invoke-VitaInstall $plan } else { Invoke-GenericCopy $plan }
