[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$SourcePath,

    [string]$RetroDriveRoot,

    [ValidateNotNullOrEmpty()]
    [ValidateScript({ $_ -notmatch '[/\\\x00-\x1F]' })]
    [string]$DisplayName,

    [ValidateSet('auto', 'psvita', 'ps3', 'xbox', 'xbox360', 'switch', 'generic')]
    [string]$Platform = 'auto',

    [switch]$Apply
)

$ErrorActionPreference = 'Stop'

function Find-RetroDriveRoot([string]$StartPath) {
    $directory = Get-Item -LiteralPath $StartPath -Force -ErrorAction Stop
    if (-not $directory.PSIsContainer) { $directory = $directory.Directory }
    while ($directory) {
        if ((Test-Path -LiteralPath (Join-Path $directory.FullName 'START.exe') -PathType Leaf) -and
            (Test-Path -LiteralPath (Join-Path $directory.FullName 'launcher.json') -PathType Leaf)) {
            return $directory.FullName
        }
        $directory = $directory.Parent
    }
}

function Resolve-RetroDriveRoot {
    param([string]$RequestedRoot)

    if ($RequestedRoot) {
        $resolved = (Resolve-Path -LiteralPath $RequestedRoot -ErrorAction Stop).Path
        if (-not (Test-Path -LiteralPath (Join-Path $resolved 'START.exe') -PathType Leaf) -or
            -not (Test-Path -LiteralPath (Join-Path $resolved 'launcher.json') -PathType Leaf)) {
            throw "The requested RetroDrive root is missing START.exe or launcher.json: $resolved"
        }
        return $resolved
    }

    # The installed skill belongs to its own drive, even if another drive is mounted.
    foreach ($startPath in @($PSScriptRoot, (Get-Location).ProviderPath)) {
        $discovered = Find-RetroDriveRoot $startPath
        if ($discovered) { return $discovered }
    }

    $candidates = @(
        Get-PSDrive -PSProvider FileSystem | ForEach-Object {
            $candidate = $_.Root
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
        '.xex' { 'xbox360'; break }
        default { 'generic' }
    }
}

function New-Plan([string]$Root, [string]$Source, [string]$ResolvedPlatform) {
    $stats = Get-ContentStats $Source
    $titleIds = @(Get-TitleIds $Source)
    $sourceName = Split-Path -Leaf $Source
    $baseName = if ($DisplayName) {
        Get-SafeName $DisplayName
    } elseif (Test-Path -LiteralPath $Source -PathType Leaf) {
        Get-SafeName ([IO.Path]::GetFileNameWithoutExtension($Source))
    } else {
        Get-SafeName $sourceName
    }
    if ($ResolvedPlatform -eq 'psvita') {
        $vitaRoot = Join-Path $Root 'RetroBat\saves\psvita\vita3k'
        $vitaExe = Join-Path $Root 'RetroBat\emulators\vita3k\Vita3K.exe'
        if (-not (Test-Path -LiteralPath $vitaExe -PathType Leaf)) {
            throw "Vita3K is missing from the RetroDrive root: $vitaExe"
        }
        if ($titleIds.Count -ne 1 -or
            -not (Test-Path -LiteralPath (Join-Path $Source "app\$($titleIds[0])\sce_sys\param.sfo") -PathType Leaf)) {
            throw 'PSVita import requires one unpacked game with app/<TITLE_ID>/sce_sys/param.sfo; handle packages, multiple games, and standalone patches separately.'
        }
        return [pscustomobject]@{
            Mode = 'vita3k-install'
            Platform = $ResolvedPlatform
            Source = $Source
            SourceName = $sourceName
            DisplayName = $baseName
            Root = $Root
            Destination = $vitaRoot
            TitleIds = $titleIds
            Files = $stats.Files
            Bytes = $stats.Bytes
            Sha256 = $stats.Sha256
            Emulator = $vitaExe
            Backup = Join-Path $Root 'Launcher\data\imports\psvita\<timestamp>\'
            Staging = Join-Path $Root 'Launcher\data\imports\psvita\<timestamp>\staging\'
            Metadata = @($titleIds | ForEach-Object { Join-Path $Root "roms\psvita\$baseName [$($_)].psvita" })
        }
    }

    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) {
        throw "Generic import currently accepts a single ROM file; use a platform adapter for content directories: $Source"
    }
    $extension = [IO.Path]::GetExtension($Source).ToLowerInvariant()
    if ($extension -in @('.m3u', '.cue', '.gdi', '.ccd', '.mds')) {
        throw 'Linked disc formats require manual preparation; keep the descriptor and all referenced discs or tracks together.'
    }
    if ($extension -in @('.pkg', '.rap', '.edat')) {
        throw 'Package and license files require emulator-owned installation; this helper will not copy them into roms.'
    }
    if ($Platform -eq 'auto' -and $extension -eq '.iso') {
        throw 'Identify the disc image platform first, then supply -Platform explicitly.'
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
        SourceName = $sourceName
        DisplayName = $baseName
        Root = $Root
        Destination = Join-Path $destination ($baseName + [IO.Path]::GetExtension($Source))
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
        SourceName = $Plan.SourceName
        DisplayName = $Plan.DisplayName
        TargetName = if ($Plan.Mode -eq 'copy') {
            [IO.Path]::GetFileName($Plan.Destination)
        } else {
            @($Plan.Metadata | ForEach-Object { [IO.Path]::GetFileName($_) })
        }
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
    $backupFile = Join-Path $backup ([IO.Path]::GetFileName($Plan.Destination))
    $backedUp = $false
    $installed = $false
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    try {
        $stagedFile = Join-Path $stage ([IO.Path]::GetFileName($Plan.Destination))
        Copy-Item -LiteralPath $Plan.Source -Destination $stagedFile
        if ((Get-Sha256 $Plan.Source) -ne (Get-Sha256 $stagedFile)) {
            throw "Staged SHA-256 verification failed: $stagedFile"
        }
        if (Test-Path -LiteralPath $Plan.Destination) {
            New-Item -ItemType Directory -Force -Path $backup | Out-Null
            Move-Item -LiteralPath $Plan.Destination -Destination $backupFile
            $backedUp = $true
        }
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Plan.Destination) | Out-Null
        Move-Item -LiteralPath $stagedFile -Destination $Plan.Destination
        $installed = $true
        if ((Get-Sha256 $Plan.Source) -ne (Get-Sha256 $Plan.Destination)) {
            throw "Post-copy SHA-256 verification failed: $($Plan.Destination)"
        }
        Write-Output "Copied and SHA-256 verified: $($Plan.Destination)"
    } catch {
        $failure = $_
        try {
            if ($installed -and (Test-Path -LiteralPath $Plan.Destination)) {
                New-Item -ItemType Directory -Force -Path $failed | Out-Null
                Move-Item -LiteralPath $Plan.Destination -Destination $failed
            }
            if ($backedUp) {
                Move-Item -LiteralPath $backupFile -Destination $Plan.Destination
            }
        } catch {
            throw "$($failure.Exception.Message) Recovery failed: $($_.Exception.Message) Workspace: $workspace"
        }
        throw
    } finally {
        if ((Test-Path -LiteralPath $stage) -and
            @(Get-ChildItem -LiteralPath $stage -Force).Count -eq 0) {
            Remove-Item -LiteralPath $stage
        }
    }
}

function New-VitaBackup($Plan) {
    $workspace = New-ImportWorkspace $Plan.Root 'psvita'
    $backup = Join-Path $workspace 'backup'
    $moved = @()
    try {
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
    } catch {
        $failure = $_
        try {
            Restore-VitaBackup ([pscustomobject]@{ Moved = $moved })
        } catch {
            throw "$($failure.Exception.Message) Backup recovery failed: $($_.Exception.Message) Workspace: $workspace"
        }
        throw
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
    $stubs = @($Plan.Metadata)
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
            throw "Installed eboot.bin is missing the expected Vita SELF header: $eboot"
        }
        $licenseDir = Join-Path $Plan.Destination "ux0\license\$titleId"
        $licenses = @(Get-ChildItem -LiteralPath $licenseDir -File -Filter '*.rif' -ErrorAction SilentlyContinue | Where-Object Length -eq 512)
        if ($licenses.Count -eq 0) { throw "A 512-byte Vita .rif file is missing for $titleId" }
    }
}

function Start-VitaInstaller($Plan) {
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Plan.Emulator
    $startInfo.WorkingDirectory = Split-Path -Parent $Plan.Emulator
    $startInfo.UseShellExecute = $false
    foreach ($argument in @('-z', '-l', '2', '-w', '-f', '-c', (Join-Path $startInfo.WorkingDirectory 'config.yml'), $Plan.Source)) {
        $startInfo.ArgumentList.Add($argument)
    }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) { throw 'Unable to start Vita3K.' }
    return $process
}

function Invoke-VitaInstall($Plan) {
    if (Get-Process -Name 'Vita3K' -ErrorAction SilentlyContinue) {
        throw 'Close the existing Vita3K process before installing content.'
    }
    $logPath = Join-Path (Split-Path -Parent $Plan.Emulator) 'vita3k.log'
    [string]$beforeLog = if (Test-Path -LiteralPath $logPath) { Get-Content -LiteralPath $logPath -Raw -ErrorAction SilentlyContinue } else { '' }
    $backup = New-VitaBackup $Plan
    $process = $null
    $deadline = (Get-Date).AddMinutes(20)
    $completed = $false
    try {
        $process = Start-VitaInstaller $Plan
        while ((Get-Date) -lt $deadline) {
            Start-Sleep -Seconds 2
            if (Test-Path -LiteralPath $logPath) {
                [string]$current = Get-Content -LiteralPath $logPath -Raw -ErrorAction SilentlyContinue
                $delta = if ($current.StartsWith($beforeLog)) { $current.Substring($beforeLog.Length) } else { $current }
                if ($delta -match 'Successfully installed \d+ content!') { $completed = $true; break }
            }
            if ($process.HasExited) { break }
        }
        if (-not $completed) { throw 'Vita3K did not report successful content installation before the timeout.' }
        if (-not $process.HasExited) {
            $process.CloseMainWindow() | Out-Null
            if (-not $process.WaitForExit(5000)) { throw 'Vita3K did not close normally after installation.' }
        }
        Test-VitaInstall $Plan
        New-VitaStubs $Plan
        Write-Output "Vita3K installation checks passed (runtime not tested): $($Plan.TitleIds -join ', ')"
    } catch {
        $failure = $_
        if ($process -and -not $process.HasExited) {
            throw "$($failure.Exception.Message) Vita3K is still running; leave its files in place and close it normally before recovery. Backup workspace: $($backup.Workspace)"
        }
        try {
            Preserve-VitaFailure $Plan $backup
            Restore-VitaBackup $backup
        } catch {
            throw "$($failure.Exception.Message) Recovery failed: $($_.Exception.Message) Workspace: $($backup.Workspace)"
        }
        throw
    }
}

if ($MyInvocation.InvocationName -eq '.') { return }

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
