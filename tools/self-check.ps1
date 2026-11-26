# Makes self-check.json, the release's self check reference, for the release commit (before its tag): builds mvs for
# the version and plays the check match on a store of this version's import. The self-test refuses a store holding
# kinds this version would import again and a reference for another id than a fresh import's (`mvs --compat-id`).
# The release workflow packs the file as it is; mvs-dist holds it to the game's id again and refuses it when the code
# changed after the commit that holds it. So: commit the version and every code change, run this on that clean tree,
# commit self-check.json on its own (a commit holding code and it together is refused), then tag.
#   Windows: powershell -NoProfile -File tools/self-check.ps1 -Version 0.2.0 [-Store <dir>]
#   Linux:   pwsh -NoProfile -File tools/self-check.ps1 -Version 0.2.0 [-Store <dir>]
# On Linux it builds mvs with tools/linux-build.sh (into target/linux-sniper; CARGO_TARGET_DIR does not apply) and
# runs it with DXVK's libraries from target/linux-deps/dxvk, so tools/linux-build.sh deps must have run first. The
# Windows exe and the Linux program make the same file.
[CmdletBinding(PositionalBinding = $false)]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Store = 'content/.store'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
    throw "self-check: version $Version is not x.y.z or x.y.z-suffix"
}

Push-Location $root
$had = $env:MVS_VERSION
try {
    # (the reference must be of the committed code: mvs-dist compares the code with the commit that holds the file)
    git diff --quiet HEAD -- crates Cargo.toml Cargo.lock rust-toolchain.toml
    if ($LASTEXITCODE -ne 0) { throw 'self-check: the code has uncommitted changes: commit them first' }
    # (the id holds the version: the exe is built for the release's)
    $env:MVS_VERSION = $Version
    $out = Join-Path $root 'self-check.json'
    # (Windows PowerShell 5.1 has no $IsLinux: there it reads as false)
    if ($IsLinux) {
        # (the Linux mvs links DXVK, which pkg-config finds in the container that linux-build.sh sets up)
        $dxvk = Join-Path $root 'target/linux-deps/dxvk/lib'
        foreach ($lib in 'libdxvk_d3d11.so.0', 'libdxvk_dxgi.so.0') {
            if (-not (Test-Path (Join-Path $dxvk $lib))) {
                throw "self-check: mvs needs DXVK's libraries: run tools/linux-build.sh deps first"
            }
        }
        & (Join-Path $root 'tools/linux-build.sh') cargo build --release --locked -p mvs-app --bin mvs
        if ($LASTEXITCODE -ne 0) { throw "self-check: the build of mvs failed (exit $LASTEXITCODE)" }
        $hadLib = $env:LD_LIBRARY_PATH
        $env:LD_LIBRARY_PATH = $dxvk
        try {
            & (Join-Path $root 'target/linux-sniper/release/mvs') --self-test --store $Store --check-out $out
        } finally {
            $env:LD_LIBRARY_PATH = $hadLib
        }
    } else {
        cargo build --release --locked -p mvs-app --bin mvs
        if ($LASTEXITCODE -ne 0) { throw "self-check: the build of mvs failed (exit $LASTEXITCODE)" }
        $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
        # (mvs.exe is a windowed program, which PowerShell does not wait for unless its output is piped: without the
        # pipe the script went on with cargo's exit code while the self-test was still running)
        & (Join-Path $target 'release/mvs.exe') --self-test --store $Store --check-out $out | Out-Host
    }
    if ($LASTEXITCODE -ne 0) { throw "self-check: the self-test failed (exit $LASTEXITCODE)" }
} finally {
    $env:MVS_VERSION = $had
    Pop-Location
}
