# Runs cargo with its arguments once one of a few machine-wide build slots is free, so the worktrees' builds queue
# instead of all compiling at once and running the machine out of memory or CPU. A slot is a lock file held open while
# cargo runs; Windows closes it when the process ends, so a killed run never keeps its slot.
#   powershell -NoProfile -File tools/cargo-slot.ps1 run --profile trace -p mvs-sim-trace --bin sim-trace -- ...
# PowerShell drops a bare `--` from a script's arguments: write `__` for it (`clippy -p mvs-sim __ -D warnings`).
# The user works on this machine while agents build: never the whole CPU. The slots and jobs are sized from the
# logical processors so that all slots together use at most about half of them, and cargo with everything it starts
# (rustc, the test binaries, sim-trace, the exe) runs at Idle priority, so the desktop always comes first.
# MVS_CARGO_SLOTS sets the number of slots, MVS_CARGO_JOBS the jobs each build may use (and the test threads).
# MVS_CARGO_FOREGROUND=1 is the user's own build, which they wait on (play.bat): all the logical
# processors at normal priority, still in a slot so it never runs beside two other builds.
$ErrorActionPreference = 'Stop'
$cpus = [Environment]::ProcessorCount
$foreground = $env:MVS_CARGO_FOREGROUND -eq '1'
$slots = if ($env:MVS_CARGO_SLOTS) { [int]$env:MVS_CARGO_SLOTS } else { 2 }
$jobs = if ($env:MVS_CARGO_JOBS) { [int]$env:MVS_CARGO_JOBS }
    elseif ($foreground) { $cpus }
    else { [Math]::Max(1, [Math]::Floor($cpus / (2 * $slots))) }
$dir = Join-Path $env:LOCALAPPDATA 'mvs-cargo-slots'
New-Item -ItemType Directory -Force $dir | Out-Null

# Idle priority class: inherited by every process cargo starts (Windows passes Idle and BelowNormal on to children).
if (-not $foreground) {
    (Get-Process -Id $PID).PriorityClass = [System.Diagnostics.ProcessPriorityClass]::Idle
}

$held = $null
$waited = $false
while ($null -eq $held) {
    for ($i = 0; $i -lt $slots -and $null -eq $held; $i++) {
        try {
            $held = [System.IO.File]::Open((Join-Path $dir "slot$i.lock"), 'OpenOrCreate', 'ReadWrite', 'None')
        } catch [System.IO.IOException] { }
    }
    if ($null -eq $held) {
        if (-not $waited) { [Console]::Error.WriteLine("cargo-slot: all $slots slots busy, waiting"); $waited = $true }
        Start-Sleep -Seconds 2
    }
}
try {
    $env:CARGO_BUILD_JOBS = "$jobs"
    $env:RUST_TEST_THREADS = "$jobs"
    # sim-trace and other tools that size their own pool read this (see testing.md)
    $env:MVS_MAX_THREADS = "$jobs"
    $cargoArgs = @($args | ForEach-Object { if ($_ -eq '__') { '--' } else { $_ } })
    & cargo @cargoArgs
    $code = $LASTEXITCODE
} finally {
    $held.Dispose()
}
exit $code
