# Builds the relay for Linux x64 on Windows: one static file (musl, linked by Rust's rust-lld), no debug info.
# Out: target/linux/x86_64-unknown-linux-musl/release/mvs-relay. Deploying it: docs/relay.md "On a Linux server".
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
rustup target add x86_64-unknown-linux-musl | Out-Null
$env:CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = 'rust-lld'
$env:CARGO_PROFILE_RELEASE_DEBUG = '0'
$env:CARGO_PROFILE_RELEASE_STRIP = 'symbols'
$env:RUSTC_WRAPPER = ''
cargo build --release --target x86_64-unknown-linux-musl --target-dir "$root/target/linux" -p mvs-relay
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$out = "$root/target/linux/x86_64-unknown-linux-musl/release/mvs-relay"
Write-Output "$out $((Get-FileHash $out -Algorithm SHA256).Hash.ToLower())"
