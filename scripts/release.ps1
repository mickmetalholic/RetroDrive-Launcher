param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('patch', 'minor', 'major')]
    [string]$Bump,

    [string]$Notes = '',

    [switch]$NoWait
)

$ErrorActionPreference = 'Stop'
$repository = 'mickmetalholic/RetroDrive-Launcher'
$projectDir = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw 'GitHub CLI is required. Install gh and run gh auth login.'
}
& gh auth status *> $null
if ($LASTEXITCODE -ne 0) {
    throw 'GitHub CLI is not authenticated. Run gh auth login.'
}

Push-Location $projectDir
try {
    $branch = (& git branch --show-current).Trim()
    if ($branch -ne 'main') {
        throw "Releases must be started from main; current branch is $branch."
    }
    if (@(& git status --porcelain).Count -ne 0) {
        throw 'The working tree must be clean before starting a release.'
    }
    & git fetch origin main --quiet
    if ($LASTEXITCODE -ne 0) {
        throw 'Unable to refresh origin/main.'
    }
    $localCommit = (& git rev-parse HEAD).Trim()
    $remoteCommit = (& git rev-parse origin/main).Trim()
    if ($localCommit -ne $remoteCommit) {
        throw 'Local main must exactly match origin/main before starting a release.'
    }

    $currentVersion = (& node scripts/version.mjs current).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw 'Unable to read the current release version.'
    }
    $currentTag = "v$currentVersion"
    $tagAtHead = @(& git tag --points-at HEAD --list $currentTag) -contains $currentTag
    $completeRelease = $false
    if ($tagAtHead) {
        $assetNames = @(& gh release view $currentTag --repo $repository --json assets --jq '.assets[].name' 2>$null)
        if ($LASTEXITCODE -eq 0) {
            $requiredAssets = @('latest.json', "RetroDrive-Launcher-$currentTag.zip", 'SHA256SUMS.txt')
            $completeRelease = @($requiredAssets | Where-Object { $_ -notin $assetNames }).Count -eq 0
        }
    }
    if ($tagAtHead -and -not $completeRelease) {
        $expectedVersion = $currentVersion
    }
    else {
        $expectedVersion = (& node scripts/version.mjs next $Bump).Trim()
        if ($LASTEXITCODE -ne 0) {
            throw 'Unable to calculate the next release version.'
        }
    }
    $startedAfter = [DateTime]::UtcNow.AddSeconds(-10)
    $arguments = @(
        'workflow', 'run', 'release.yml',
        '--repo', $repository,
        '--ref', 'main',
        '-f', "bump=$Bump"
    )
    if ($Notes) {
        $arguments += @('-f', "notes=$Notes")
    }
    & gh @arguments
    if ($LASTEXITCODE -ne 0) {
        throw 'Unable to start the GitHub release workflow.'
    }

    $run = $null
    for ($attempt = 0; $attempt -lt 15 -and -not $run; $attempt++) {
        Start-Sleep -Seconds 2
        $runs = & gh run list --repo $repository --workflow release.yml --event workflow_dispatch --limit 10 --json databaseId,createdAt,status,url
        if ($LASTEXITCODE -ne 0) {
            throw 'Unable to locate the GitHub release workflow run.'
        }
        $run = @($runs | ConvertFrom-Json) |
            Where-Object { [DateTime]$_.createdAt -ge $startedAfter } |
            Sort-Object { [DateTime]$_.createdAt } -Descending |
            Select-Object -First 1
    }
    if (-not $run) {
        throw 'The release workflow was accepted, but its run could not be located.'
    }
    Write-Output "Release v${expectedVersion}: $($run.url)"
    if ($NoWait) {
        return
    }
    & gh run watch $run.databaseId --repo $repository --interval 10 --exit-status
    if ($LASTEXITCODE -ne 0) {
        throw "Release v$expectedVersion failed. See $($run.url)"
    }
    & git pull --ff-only origin main
    if ($LASTEXITCODE -ne 0) {
        throw 'Release succeeded, but local main could not be fast-forwarded.'
    }
    $releaseUrl = (& gh release view "v$expectedVersion" --repo $repository --json url --jq .url).Trim()
    Write-Output "Published v${expectedVersion}: $releaseUrl"
}
finally {
    Pop-Location
}
