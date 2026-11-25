# Packs a release from the programs the release workflow (.github/workflows/release.yml) built for its tag. Runs on
# the maintainer's machine, not on GitHub: finds the tag's run, downloads its programs (the Windows game, the Linux
# game and the relay), checks each one's attestation (built by that workflow, from that tag, in that repo) and runs
# `mvs-dist build --programs`, which adds the self check's reference (played on this machine's store), Epic's SDK and
# credentials and the notices, zips and guards. Out: target/dist, the Linux game's zip among the others.
# With --eos (the default; -NoEos leaves it out), Epic's files come from three variables naming files on this
# machine: MVS_EOS_DLL (the Windows library), MVS_EOS_SO (the Linux library, needed now that the Linux zip is packed
# too) and MVS_EOS_CREDENTIALS.
#   powershell -NoProfile -File tools/release-fetch.ps1 -Version 0.2.0 [-Wait] [-NoEos] [-Repo <owner>/<repo>]
# -Wait waits for a run still going. Arguments after the named ones go to mvs-dist build (`--store <dir>`).
# (no positional binding: an argument for mvs-dist such as `--store` must not land in -Repo)
[CmdletBinding(PositionalBinding = $false)]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Repo = 'Toxcious/mvs-classic',
    [switch]$Wait,
    [switch]$NoEos,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$tag = "v$Version"
$workflow = 'release.yml'

function Invoke-Native {
    param([string]$What)
    if ($LASTEXITCODE -ne 0) { throw "release-fetch: $What failed (exit $LASTEXITCODE)" }
}

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "release-fetch: version $Version is not x.y.z" }

# the tag's run of the workflow (a tag's runs carry the tag as their branch)
$runs = gh run list -R $Repo -w $workflow -b $tag -L 1 --json databaseId,status,conclusion | ConvertFrom-Json
Invoke-Native 'gh run list'
if (-not $runs -or $runs.Count -eq 0) { throw "release-fetch: no $workflow run for $tag in $Repo (is the tag pushed?)" }
$run = $runs[0]
if ($run.status -ne 'completed') {
    if (-not $Wait) { throw "release-fetch: the run $($run.databaseId) for $tag is $($run.status); give -Wait to wait for it" }
    gh run watch $run.databaseId -R $Repo --exit-status --interval 60
    Invoke-Native "the run $($run.databaseId)"
} elseif ($run.conclusion -ne 'success') {
    throw "release-fetch: the run $($run.databaseId) for $tag ended $($run.conclusion)"
}

# its programs, in a folder of this script's own
$dir = Join-Path $root "target/ci-programs/$Version"
if (Test-Path $dir) { Remove-Item -LiteralPath $dir -Recurse -Force }
New-Item -ItemType Directory -Force $dir | Out-Null
gh run download $run.databaseId -R $Repo -D $dir
Invoke-Native 'gh run download'
$windows = Join-Path $dir 'programs-windows'
$linuxRelay = Join-Path $dir 'programs-relay-linux/mvs-relay'
# the Linux game's folder as tools/linux-build.sh lays it out: the launcher at the top, the game in game/
$gameLinux = Join-Path $dir 'programs-game-linux'
$files = @('mvs.exe', 'mvs-launcher.exe', 'mvs-relay.exe' | ForEach-Object { Join-Path $windows $_ }) + $linuxRelay +
    @('MVSC Launcher', 'game/MVS Classic' | ForEach-Object { Join-Path $gameLinux $_ })

# each one built by the release workflow, from the tag, in the repo
foreach ($f in $files) {
    if (-not (Test-Path -LiteralPath $f)) { throw "release-fetch: the run has no $f" }
    gh attestation verify $f -R $Repo --signer-workflow "$Repo/.github/workflows/$workflow" --source-ref "refs/tags/$tag" | Out-Null
    Invoke-Native "the attestation of $f"
    Write-Output "release-fetch: attested $(Split-Path -Leaf $f) $((Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash.ToLower())"
}

# the zips
$dist = @('build', '--version', $Version, '--feed', "https://api.github.com/repos/$Repo/releases/latest",
    '--programs', $windows, '--relay-linux', $linuxRelay, '--game-linux', $gameLinux)
if (-not $NoEos) { $dist += '--eos' }
if ($Rest) { $dist += $Rest }
Push-Location $root
try {
    # (built, then run: Windows PowerShell may drop the `--` of `cargo run --`)
    cargo build --release --locked -p mvs-dist --bin mvs-dist
    Invoke-Native 'the build of mvs-dist'
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    & (Join-Path $target 'release/mvs-dist.exe') @dist
    Invoke-Native 'mvs-dist build'
} finally {
    Pop-Location
}
