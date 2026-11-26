#!/bin/bash
# Builds the Windows programs from Linux: runs cargo for x86_64-pc-windows-msvc against an xwin splat of Microsoft's
# CRT and Windows SDK, clang-cl compiling mvs-freetype's C and lld-link linking. mvs-dist builds the Windows half of a
# release through it on a Linux host.
#
#   tools/windows-cross.sh <cargo subcommand> [args...]   e.g. build --release -p mvs-app --bin mvs
#
# The target dir is target/xwin unless CARGO_TARGET_DIR names another; what it builds lands in
# <target dir>/x86_64-pc-windows-msvc/. The splat's -Lnative paths are added after any
# CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS already set (mvs-dist's +crt-static), never in its place.
#
# Needs the Rust target (rustup target add x86_64-pc-windows-msvc --toolchain <rust-toolchain.toml's>), clang-cl,
# llvm-lib, lld-link, and an xwin splat at $XWIN_SPLAT, else ~/.cache/xwin/splat. The splat is made by
# `xwin --accept-license splat`: running it is the user's own acceptance of Microsoft's license terms for those
# files, so this script never runs it, and stops with that instruction when the splat is missing.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
XWIN=${XWIN_SPLAT:-$HOME/.cache/xwin/splat}

usage() {
    sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

case "${1:-}" in
'')
    usage >&2
    exit 2
    ;;
-h | --help)
    usage
    exit 0
    ;;
esac
if [ ! -d "$XWIN/crt/include" ]; then
    echo "No xwin splat at $XWIN. Make one with \`xwin --accept-license splat\` (its --output, or move it there, or" \
        "set XWIN_SPLAT): running that accepts Microsoft's license terms for the CRT and SDK, which is yours to do." >&2
    exit 1
fi
# (RUSTFLAGS are split at spaces)
case "$XWIN" in
*[[:space:]]*)
    echo "The xwin splat's path has a space, which RUSTFLAGS cannot carry: $XWIN" >&2
    exit 1
    ;;
esac
for tool in clang-cl llvm-lib lld-link; do
    if ! command -v "$tool" > /dev/null; then
        echo "Building for Windows needs $tool (LLVM's)." >&2
        exit 1
    fi
done

T=x86_64-pc-windows-msvc
export CC_x86_64_pc_windows_msvc=clang-cl
export AR_x86_64_pc_windows_msvc=llvm-lib
export CFLAGS_x86_64_pc_windows_msvc="/imsvc$XWIN/crt/include /imsvc$XWIN/sdk/include/ucrt /imsvc$XWIN/sdk/include/um /imsvc$XWIN/sdk/include/shared"
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=lld-link
# (xwin names the library folders x86_64, or Microsoft's x64 when made with --preserve-ms-arch-notation)
arch=x86_64
if [ -d "$XWIN/crt/lib/x64" ]; then
    arch=x64
fi
if [ ! -d "$XWIN/crt/lib/$arch" ]; then
    echo "The xwin splat at $XWIN has no x64 libraries (crt/lib/x86_64 or crt/lib/x64): make it for x86_64." >&2
    exit 1
fi
lib="-Lnative=$XWIN/crt/lib/$arch -Lnative=$XWIN/sdk/lib/um/$arch -Lnative=$XWIN/sdk/lib/ucrt/$arch"
flags=${CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS:-}
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="${flags:+$flags }$lib"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target/xwin}

cd "$ROOT"
sub=$1
shift
exec cargo "$sub" --target "$T" "$@"
