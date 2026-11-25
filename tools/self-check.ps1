# Makes self-check.json, the release's self check reference, for the release commit (before its tag): builds mvs for
# the version and plays the check match on a store of this version's import. The self-test refuses a store holding
# kinds this version would import again and a reference for another id than a fresh import's (`mvs --compat-id`).
# The release workflow packs the file as it is; mvs-dist holds it to the game's id again and refuses it when the code
# changed after the commit that holds it. So: commit the version and every code change, run this on that clean tree,
# commit self-check.json, then tag.
#   powershell -NoProfile -File tools/self-check.ps1 -Version 0.2.0 [-Store <dir>]
[CmdletBinding(PositionalBinding = $false)]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Store = 'content/.store'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "self-check: version $Version is not x.y.z" }

Push-Location $root
$had = $env:MVS_VERSION
try {
    # (the reference must be of the committed code: mvs-dist compares the code with the commit that holds the file)
    git diff --quiet HEAD -- crates Cargo.toml Cargo.lock rust-toolchain.toml
    if ($LASTEXITCODE -ne 0) { throw 'self-check: the code has uncommitted changes: commit them first' }
    # (the id holds the version: the exe is built for the release's)
    $env:MVS_VERSION = $Version
    cargo build --release --locked -p mvs-app --bin mvs
    if ($LASTEXITCODE -ne 0) { throw "self-check: the build of mvs failed (exit $LASTEXITCODE)" }
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    & (Join-Path $target 'release/mvs.exe') --self-test --store $Store --check-out (Join-Path $root 'self-check.json')
    if ($LASTEXITCODE -ne 0) { throw "self-check: the self-test failed (exit $LASTEXITCODE)" }
} finally {
    $env:MVS_VERSION = $had
    Pop-Location
}
