#!/bin/bash
# Builds MVS Classic for Linux inside the Steam Runtime "sniper" SDK container, so what it builds needs glibc 2.31 at
# most and runs on a Steam Deck and on ordinary desktop distributions. Runs anywhere podman or docker does, WSL
# included; the repo is mounted at /src.
#
#   tools/linux-build.sh deps            build DXVK-native (SDL3 window system) into target/linux-deps/dxvk
#   tools/linux-build.sh cargo <args...> run `cargo <args>` in the container, target dir target/linux-sniper
#
# The container has no Rust: the first run installs rustup and the toolchain that rust-toolchain.toml names into
# the named volumes mvs-classic-rustup and mvs-classic-cargo, which later runs reuse. In `cargo` mode pkg-config
# finds DXVK's libraries in target/linux-deps/dxvk and test binaries load them from there (LD_LIBRARY_PATH).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
IMAGE=registry.gitlab.steamos.cloud/steamrt/sniper/sdk:latest
DXVK_REPO=https://github.com/doitsujin/dxvk.git
DXVK_TAG=v3.1.1

usage() {
    sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

# Runs the bash script $1 in the container, the remaining arguments as its "$@".
in_container() {
    local engine script=$1
    shift
    engine=$(command -v podman || command -v docker || true)
    if [ -z "$engine" ]; then
        echo "Building for Linux needs podman or docker." >&2
        exit 1
    fi
    local tty=()
    if [ -t 0 ] && [ -t 1 ]; then
        tty=(-it)
    fi
    # (made here, so they belong to the caller whatever user the container runs as)
    mkdir -p "$ROOT/target/linux-deps" "$ROOT/target/linux-sniper"
    # label=disable rather than :Z, which would relabel the files of the checkout itself.
    "$engine" run --rm "${tty[@]}" --security-opt label=disable \
        -v "$ROOT:/src" -w /src \
        -v mvs-classic-rustup:/rustup -v mvs-classic-cargo:/cargo \
        -e RUSTUP_HOME=/rustup -e CARGO_HOME=/cargo \
        "$IMAGE" bash -euc '
            # Rootless podman maps the container root to the caller; a root daemon (docker) does not, so what the
            # run made in target/ is handed back to the checkout owner when it ends.
            owner=$(stat -c %u:%g /src)
            if [ "$owner" != 0:0 ]; then
                trap "chown -R $owner target/linux-deps target/linux-sniper" EXIT
            fi
            '"$script" bash "$@"
}

case "${1:-}" in
deps)
    in_container '
        repo=$1 tag=$2 D=target/linux-deps
        if [ -d $D/src ] && [ "$(git -C $D/src rev-parse HEAD)" != "$(git -C $D/src rev-parse "$tag^{commit}")" ]; then
            rm -rf $D/src
        fi
        if [ ! -d $D/src ]; then
            git clone --depth 1 --branch "$tag" --recurse-submodules --shallow-submodules "$repo" $D/src
        fi
        rm -rf $D/build $D/dxvk
        # The libdir is pinned so every later path is dxvk/lib, never lib64 or lib/x86_64-linux-gnu.
        meson setup --buildtype release -Dnative_sdl3=enabled -Dnative_sdl2=disabled -Dnative_glfw=disabled \
            --prefix /src/$D/dxvk --libdir lib $D/build $D/src
        ninja -C $D/build install
        # The newest glibc symbol each library needs: 2.31 or older runs on a Steam Deck.
        for f in $D/dxvk/lib/libdxvk_d3d11.so.0 $D/dxvk/lib/libdxvk_dxgi.so.0; do
            echo "$f $(objdump -T $f | grep -o "GLIBC_[0-9.]*" | sort -Vu | tail -1)"
        done' "$DXVK_REPO" "$DXVK_TAG"
    ;;
cargo)
    shift
    in_container '
        export PATH=/cargo/bin:$PATH
        if ! command -v rustup > /dev/null; then
            curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs |
                sh -s -- -y --no-modify-path --profile minimal --default-toolchain none
        fi
        channel=$(sed -n "s/^channel *= *\"\(.*\)\"/\1/p" rust-toolchain.toml)
        if [ ! -x "$RUSTUP_HOME/toolchains/$channel-x86_64-unknown-linux-gnu/bin/cargo" ]; then
            # (with no argument, the toolchain and components rust-toolchain.toml names)
            rustup toolchain install
        fi
        export PKG_CONFIG_PATH=/src/target/linux-deps/dxvk/lib/pkgconfig
        export LD_LIBRARY_PATH=/src/target/linux-deps/dxvk/lib
        export CARGO_TARGET_DIR=target/linux-sniper
        cargo "$@"' "$@"
    ;;
-h | --help)
    usage
    ;;
*)
    usage >&2
    exit 2
    ;;
esac
