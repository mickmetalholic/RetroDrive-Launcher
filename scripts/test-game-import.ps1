$ErrorActionPreference = 'Stop'
$projectDir = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$sourceScript = Join-Path $projectDir 'skills\retrodrive-import-game\scripts\Import-RetroDriveGame.ps1'
$powershellExe = (Get-Command pwsh.exe -ErrorAction Stop).Source
$tempParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$testRoot = Join-Path $tempParent "retrodrive-import-test-$([guid]::NewGuid().ToString('N'))"

function Assert-That([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function New-DriveFixture([string]$Name) {
    $root = Join-Path $testRoot $Name
    New-Item -ItemType Directory -Path $root -Force | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $root 'START.exe'), [byte[]]@())
    [IO.File]::WriteAllText((Join-Path $root 'launcher.json'), '{}')
    return $root
}

function Invoke-Import([string]$ScriptPath, [string]$Source, [string]$Name, [string]$Root, [switch]$Apply) {
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $ScriptPath, '-SourcePath', $Source, '-DisplayName', $Name)
    if ($Root) { $arguments += @('-RetroDriveRoot', $Root) }
    if ($Apply) { $arguments += '-Apply' }
    $output = & $powershellExe @arguments 2>&1
    if ($LASTEXITCODE -ne 0) { throw "Import script failed: $($output -join [Environment]::NewLine)" }
    return ($output -join [Environment]::NewLine)
}

function Assert-ImportRejected([string]$ScriptPath, [string]$Source, [string]$MessagePattern) {
    $failure = $null
    try { Invoke-Import $ScriptPath $Source 'Blocked input' -Apply | Out-Null } catch { $failure = $_ }
    Assert-That ($null -ne $failure -and $failure.Exception.Message -match $MessagePattern) "Input was not rejected as expected: $Source"
}

try {
    $first = New-DriveFixture 'first'
    $second = New-DriveFixture 'second'
    $installedScript = Join-Path $first '.agents\skills\retrodrive-import-game\scripts\Import-RetroDriveGame.ps1'
    New-Item -ItemType Directory -Path (Split-Path -Parent $installedScript) -Force | Out-Null
    Copy-Item -LiteralPath $sourceScript -Destination $installedScript
    $source = Join-Path $testRoot 'upload_123.nsp'
    [IO.File]::WriteAllText($source, 'original ROM fixture')
    $sourceHash = (Get-FileHash -LiteralPath $source).Hash
    $name = 'Standard Game (USA) (Rev 1)'
    $target = Join-Path $first "roms\switch\$name.nsp"

    Push-Location $second
    try {
        # An installed skill uses its own drive even when another drive is the CWD.
        $plan = Invoke-Import $installedScript $source $name
        Assert-That ($plan.Contains('SourceName') -and $plan.Contains('TargetName')) 'Plan must show original and target names.'
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $first 'roms'))) 'Plan mode wrote ROM files.'
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $first 'Launcher\data'))) 'Plan mode created import data.'
        Invoke-Import $installedScript $source $name -Apply | Out-Null
        Assert-That (Test-Path -LiteralPath $target -PathType Leaf) 'Installed skill did not import to its own drive under the standard name.'
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $second 'roms'))) 'Installed skill selected the wrong drive.'
        Assert-That ((Get-FileHash -LiteralPath $target).Hash -eq $sourceHash) 'Imported content changed.'
        Assert-That ((Get-FileHash -LiteralPath $source).Hash -eq $sourceHash) 'Source was renamed or changed.'

        # Explicit root selection takes precedence over both script location and CWD.
        Invoke-Import $installedScript $source $name $second -Apply | Out-Null
        Assert-That (Test-Path -LiteralPath (Join-Path $second "roms\switch\$name.nsp")) 'Explicit target root was ignored.'

        # Identical normalized targets are skipped, and conflicts get recoverable backups.
        $imports = Join-Path $first 'Launcher\data\imports\generic'
        $before = @(Get-ChildItem -LiteralPath $imports -Directory).Count
        $duplicate = Invoke-Import $installedScript $source $name -Apply
        Assert-That ($duplicate.Contains('Already imported')) 'Duplicate content was not skipped.'
        Assert-That (@(Get-ChildItem -LiteralPath $imports -Directory).Count -eq $before) 'Duplicate import created a transaction.'
        [IO.File]::WriteAllText($target, 'previous target fixture')
        $oldHash = (Get-FileHash -LiteralPath $target).Hash
        Invoke-Import $installedScript $source $name -Apply | Out-Null
        $backups = @(Get-ChildItem -LiteralPath $imports -Recurse -File | Where-Object { $_.Directory.Name -eq 'backup' })
        Assert-That ($backups.Count -eq 1) 'Normalized target conflict was not backed up.'
        Assert-That ((Get-FileHash -LiteralPath $backups[0].FullName).Hash -eq $oldHash) 'Conflict backup does not match the old target.'
        Assert-That ((Get-FileHash -LiteralPath $target).Hash -eq $sourceHash) 'Replacement content is invalid.'

        # The repository copy falls back to the current drive workspace.
        Invoke-Import $sourceScript $source 'Workspace Game' -Apply | Out-Null
        Assert-That (Test-Path -LiteralPath (Join-Path $second 'roms\switch\Workspace Game.nsp')) 'Working-directory root discovery failed.'

        # Vita plans name the real launch entries before invoking an emulator.
        $vitaSource = Join-Path $testRoot 'uploaded-vita'
        $sfo = Join-Path $vitaSource 'app\PCSE00120\sce_sys\param.sfo'
        New-Item -ItemType Directory -Path (Split-Path -Parent $sfo) -Force | Out-Null
        [IO.File]::WriteAllText($sfo, 'metadata fixture')
        $vitaExe = Join-Path $first 'RetroBat\emulators\vita3k\Vita3K.exe'
        New-Item -ItemType Directory -Path (Split-Path -Parent $vitaExe) -Force | Out-Null
        [IO.File]::WriteAllBytes($vitaExe, [byte[]]@())
        $vitaPlan = Invoke-Import $installedScript $vitaSource 'Persona 4 Golden'
        Assert-That ($vitaPlan.Contains('Persona 4 Golden [PCSE00120].psvita')) 'Vita plan did not expose the standardized launch filename.'
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $first 'roms\psvita'))) 'Vita plan wrote launch entries.'

        foreach ($extension in @('.m3u', '.cue', '.gdi', '.ccd', '.mds')) {
            $inputPath = Join-Path $testRoot "linked$extension"
            [IO.File]::WriteAllText($inputPath, 'linked-content fixture')
            Assert-ImportRejected $installedScript $inputPath 'Linked disc formats'
        }
        foreach ($extension in @('.pkg', '.rap', '.edat')) {
            $inputPath = Join-Path $testRoot "package$extension"
            [IO.File]::WriteAllText($inputPath, 'package fixture')
            Assert-ImportRejected $installedScript $inputPath 'emulator-owned installation'
        }
        foreach ($extension in @('.xiso', '.xex')) {
            $inputPath = Join-Path $testRoot "xbox$extension"
            [IO.File]::WriteAllText($inputPath, 'Xbox fixture')
            Assert-ImportRejected $installedScript $inputPath 'format/readability decision'
        }
        $iso = Join-Path $testRoot 'unknown.iso'
        [IO.File]::WriteAllText($iso, 'unknown image fixture')
        Assert-ImportRejected $installedScript $iso 'Identify the disc image platform'
        $patch = Join-Path $testRoot 'patch-only\patch\PCSE00120\sce_sys\param.sfo'
        New-Item -ItemType Directory -Path (Split-Path -Parent $patch) -Force | Out-Null
        [IO.File]::WriteAllText($patch, 'patch metadata')
        Assert-ImportRejected $installedScript (Join-Path $testRoot 'patch-only') 'one unpacked game'
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $first 'roms\inbox'))) 'Rejected input was copied to inbox.'

        # Inject a staging failure: a pre-existing target must remain untouched.
        & {
            . $sourceScript -SourcePath $source -DisplayName 'Staging failure'
            $plan = New-Plan $first $source 'switch'
            [IO.File]::WriteAllText($plan.Destination, 'existing target')
            function Get-Sha256([string]$Path) {
                if ($Path -match '\\staging\\') { throw 'Injected staging failure' }
                (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash
            }
            $failure = $null
            try { Invoke-GenericCopy $plan } catch { $failure = $_ }
            Assert-That ($failure.Exception.Message -match 'Injected staging failure') 'Staging failure was not reported.'
            Assert-That ([IO.File]::ReadAllText($plan.Destination) -eq 'existing target') 'A staging failure moved the original target.'
        }

        # Inject a post-copy failure: restore the backup and preserve the new file.
        & {
            . $sourceScript -SourcePath $source -DisplayName 'Final check failure'
            $plan = New-Plan $first $source 'switch'
            [IO.File]::WriteAllText($plan.Destination, 'recoverable original')
            $script:targetHashCalls = 0
            function Get-Sha256([string]$Path) {
                if ($Path -eq $plan.Destination) {
                    $script:targetHashCalls++
                    if ($script:targetHashCalls -eq 2) { throw 'Injected final check failure' }
                }
                (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash
            }
            $failure = $null
            try { Invoke-GenericCopy $plan } catch { $failure = $_ }
            Assert-That ($failure.Exception.Message -match 'Injected final check failure') 'Final check failure was not reported.'
            Assert-That ([IO.File]::ReadAllText($plan.Destination) -eq 'recoverable original') 'Failed import did not restore the old target.'
            $failedFiles = @(Get-ChildItem -LiteralPath $imports -Recurse -File | Where-Object { $_.Directory.Name -eq 'failed' })
            Assert-That ($failedFiles.Count -eq 1) 'Failed imported content was not preserved.'
        }

        # Exercise the Vita installation checks with a fake installer and process.
        & {
            . $sourceScript -SourcePath $vitaSource -DisplayName 'Persona 4 Golden'
            function Get-Process { }
            function Start-Sleep { }
            function Start-VitaInstaller($Plan) {
                $app = Join-Path $Plan.Destination 'ux0\app\PCSE00120'
                $license = Join-Path $Plan.Destination 'ux0\license\PCSE00120'
                New-Item -ItemType Directory -Path $app, $license -Force | Out-Null
                [IO.File]::WriteAllBytes((Join-Path $app 'eboot.bin'), [byte[]]@(0x53, 0x43, 0x45, 0))
                [IO.File]::WriteAllBytes((Join-Path $license 'fixture.rif'), [byte[]]::new(512))
                [IO.File]::AppendAllText((Join-Path (Split-Path -Parent $Plan.Emulator) 'vita3k.log'), "Successfully installed 1 content!`n")
                return [pscustomobject]@{ HasExited = $true }
            }
            $plan = New-Plan $first $vitaSource 'psvita'
            Invoke-VitaInstall $plan | Out-Null
            $stub = Join-Path $first 'roms\psvita\Persona 4 Golden [PCSE00120].psvita'
            Assert-That ((Get-Item -LiteralPath $stub).Length -eq 0) 'Verified Vita installation did not create an empty launch entry.'

            # A same-size unrelated file must not satisfy the license check.
            $license = Join-Path $plan.Destination 'ux0\license\PCSE00120\fixture.rif'
            Move-Item -LiteralPath $license -Destination ($license + '.txt')
            $failure = $null
            try { Test-VitaInstall $plan } catch { $failure = $_ }
            Assert-That ($failure.Exception.Message -match '\.rif file is missing') 'An unrelated 512-byte file was accepted as a license.'
            Move-Item -LiteralPath ($license + '.txt') -Destination $license
        }

        # An exited failed installer preserves partial content and restores the old app.
        & {
            . $sourceScript -SourcePath $vitaSource -DisplayName 'Persona 4 Golden'
            function Get-Process { }
            function Start-Sleep { }
            $root = New-DriveFixture 'vita-exited'
            $exe = Join-Path $root 'RetroBat\emulators\vita3k\Vita3K.exe'
            $app = Join-Path $root 'RetroBat\saves\psvita\vita3k\ux0\app\PCSE00120'
            New-Item -ItemType Directory -Path (Split-Path -Parent $exe), $app -Force | Out-Null
            Copy-Item -LiteralPath $vitaExe -Destination $exe
            [IO.File]::WriteAllText((Join-Path $app 'eboot.bin'), 'old app')
            function Start-VitaInstaller($Plan) {
                $partial = Join-Path $Plan.Destination 'ux0\app\PCSE00120_dec'
                New-Item -ItemType Directory -Path $partial -Force | Out-Null
                [IO.File]::WriteAllText((Join-Path $partial 'partial.bin'), 'failed install')
                return [pscustomobject]@{ HasExited = $true }
            }
            $plan = New-Plan $root $vitaSource 'psvita'
            $failure = $null
            try { Invoke-VitaInstall $plan } catch { $failure = $_ }
            Assert-That ($failure.Exception.Message -match 'did not report successful') 'Failed Vita install was not reported.'
            Assert-That ([IO.File]::ReadAllText((Join-Path $app 'eboot.bin')) -eq 'old app') 'Failed Vita install did not restore the old application.'
            $partials = @(Get-ChildItem -LiteralPath (Join-Path $root 'Launcher\data\imports\psvita') -Recurse -File -Filter 'partial.bin')
            Assert-That ($partials.Count -eq 1 -and $partials[0].FullName -match '\\failed\\') 'Failed Vita contents were not preserved.'
        }

        # A partially completed backup also restores all files already moved.
        & {
            . $sourceScript -SourcePath $vitaSource -DisplayName 'Persona 4 Golden'
            $root = New-DriveFixture 'vita-backup-failure'
            $exe = Join-Path $root 'RetroBat\emulators\vita3k\Vita3K.exe'
            $app = Join-Path $root 'RetroBat\saves\psvita\vita3k\ux0\app\PCSE00120'
            $license = Join-Path $root 'RetroBat\saves\psvita\vita3k\ux0\license\PCSE00120'
            New-Item -ItemType Directory -Path (Split-Path -Parent $exe), $app, $license -Force | Out-Null
            Copy-Item -LiteralPath $vitaExe -Destination $exe
            [IO.File]::WriteAllText((Join-Path $app 'eboot.bin'), 'app before backup')
            [IO.File]::WriteAllText((Join-Path $license 'fixture.rif'), 'license before backup')
            $script:backupMoves = 0
            function Move-Item([string]$LiteralPath, [string]$Destination) {
                $script:backupMoves++
                if ($script:backupMoves -eq 2) { throw 'Injected Vita backup failure' }
                Microsoft.PowerShell.Management\Move-Item -LiteralPath $LiteralPath -Destination $Destination
            }
            $plan = New-Plan $root $vitaSource 'psvita'
            $failure = $null
            try { New-VitaBackup $plan | Out-Null } catch { $failure = $_ }
            Assert-That ($failure.Exception.Message -match 'Injected Vita backup failure') 'Backup failure was not reported.'
            Assert-That ([IO.File]::ReadAllText((Join-Path $app 'eboot.bin')) -eq 'app before backup') 'Partial backup left the application missing.'
            Assert-That ([IO.File]::ReadAllText((Join-Path $license 'fixture.rif')) -eq 'license before backup') 'Partial backup changed the license.'
        }

        # A timed-out active installer keeps its process and partial files in place.
        & {
            . $sourceScript -SourcePath $vitaSource -DisplayName 'Persona 4 Golden'
            function Get-Process { }
            $script:clockReads = 0
            function Get-Date([string]$Format) {
                if ($Format) { return Microsoft.PowerShell.Utility\Get-Date -Format $Format }
                $script:clockReads++
                return [DateTime]::UtcNow.AddMinutes(21 * ($script:clockReads - 1))
            }
            function Start-VitaInstaller($Plan) {
                $partial = Join-Path $Plan.Destination 'ux0\app\PCSE00120_dec'
                New-Item -ItemType Directory -Path $partial -Force | Out-Null
                [IO.File]::WriteAllText((Join-Path $partial 'partial.bin'), 'in progress')
                return [pscustomobject]@{ HasExited = $false }
            }
            $plan = New-Plan $first $vitaSource 'psvita'
            $failure = $null
            try { Invoke-VitaInstall $plan } catch { $failure = $_ }
            Assert-That ($failure.Exception.Message -match 'still running') 'Active Vita timeout did not report the unresolved process.'
            $partial = Join-Path $plan.Destination 'ux0\app\PCSE00120_dec\partial.bin'
            Assert-That (Test-Path -LiteralPath $partial) 'Active Vita installation files were moved or deleted.'
            $backupFiles = @(Get-ChildItem -LiteralPath (Join-Path $first 'Launcher\data\imports\psvita') -Recurse -File -Filter 'eboot.bin')
            Assert-That ($backupFiles.Count -eq 1) 'Active Vita timeout did not retain the previous installation backup.'
        }
    } finally {
        Pop-Location
    }
    Write-Output 'Game-import checks passed: discovery, naming, format routing, copy recovery, Vita installation checks, and active-install preservation (mocked emulator).'
} finally {
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    if ((Split-Path -Parent $resolvedTestRoot) -ne $tempParent -or
        (Split-Path -Leaf $resolvedTestRoot) -notmatch '^retrodrive-import-test-[0-9a-f]{32}$') {
        throw "Refusing to remove unexpected test workspace: $resolvedTestRoot"
    }
    if (Test-Path -LiteralPath $resolvedTestRoot) {
        Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
    }
}
