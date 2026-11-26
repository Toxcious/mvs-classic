#!/bin/bash
# Builds MVS Classic for Linux inside the Steam Runtime "sniper" SDK container, so what it builds needs glibc 2.31 at
# most and runs on a Steam Deck and on ordinary desktop distributions. Runs anywhere podman or docker does, WSL
# included; the repo is mounted at /src.
#
#   tools/linux-build.sh deps            build DXVK-native (SDL3 window system) into target/linux-deps/dxvk
#   tools/linux-build.sh cargo <args...> run `cargo <args>` in the container, target dir target/linux-sniper
#   tools/linux-build.sh release         build mvs and mvs-launcher for release and lay out target/linux-release
#
# MVS_VERSION, when set, is passed into the container: the programs' version (unset, the workspace's). `release`
# needs `deps` run first; it writes game/linux-build.json (the version the game prints, the SHA-256 of both
# programs) and fails if any program or library in the folder needs a glibc newer than 2.31.
#
# The container has no Rust: the first run installs rustup and the toolchain that rust-toolchain.toml names into
# the named volumes mvs-classic-rustup and mvs-classic-cargo, which later runs reuse. In `cargo` mode pkg-config
# finds DXVK's libraries in target/linux-deps/dxvk and test binaries load them from there (LD_LIBRARY_PATH).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# The sniper SDK, pinned by digest (the manifest of `:latest` on 2026-11-26): the cache keys of build.yml read this line
# and name the digest, so a newer image is a change here, and a cold build in CI. To move to a newer one:
#   docker buildx imagetools inspect registry.gitlab.steamos.cloud/steamrt/sniper/sdk:latest
IMAGE=registry.gitlab.steamos.cloud/steamrt/sniper/sdk@sha256:bb30053d3cd19d35c76ae69b2790dbaa1430448006f8240cb6d7e723f2d1940e
DXVK_REPO=https://github.com/doitsujin/dxvk.git
# The tag, and the commit it names (the tag is annotated: `git ls-remote <repo> 'refs/tags/<tag>^{}'`). `deps`
# checks the clone out at the tag and refuses it unless its HEAD is this commit, so a moved tag builds nothing.
DXVK_TAG=v3.1.1
DXVK_COMMIT=b1a1c99ab52b687cf950d62c88bc2fa316b41663
# The start of a container script that runs Cargo: rustup and the toolchain rust-toolchain.toml names, DXVK found by
# pkg-config, the target dir.
RUST='
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
    export CARGO_TARGET_DIR=target/linux-sniper'

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
    # (only when the caller set it: unset, the crates take the workspace's version)
    local env=()
    if [ -n "${MVS_VERSION+set}" ]; then
        env=(-e MVS_VERSION)
    fi
    # (made here, so they belong to the caller whatever user the container runs as)
    mkdir -p "$ROOT/target/linux-deps" "$ROOT/target/linux-sniper" "$ROOT/target/linux-release"
    # label=disable rather than :Z, which would relabel the files of the checkout itself.
    "$engine" run --rm "${tty[@]}" --security-opt label=disable \
        -v "$ROOT:/src" -w /src \
        -v mvs-classic-rustup:/rustup -v mvs-classic-cargo:/cargo \
        -e RUSTUP_HOME=/rustup -e CARGO_HOME=/cargo "${env[@]}" \
        "$IMAGE" bash -euc '
            # Rootless podman maps the container root to the caller; a root daemon (docker) does not, so what the
            # run made in target/ is handed back to the checkout owner when it ends.
            owner=$(stat -c %u:%g /src)
            if [ "$owner" != 0:0 ]; then
                trap "chown -R $owner target/linux-deps target/linux-sniper target/linux-release" EXIT
            fi
            '"$script" bash "$@"
}

case "${1:-}" in
deps)
    in_container '
        repo=$1 tag=$2 commit=$3 D=target/linux-deps
        if [ -d $D/src ] && [ "$(git -C $D/src rev-parse HEAD)" != "$commit" ]; then
            rm -rf $D/src
        fi
        if [ ! -d $D/src ]; then
            git clone --depth 1 --branch "$tag" --recurse-submodules --shallow-submodules "$repo" $D/src
            head=$(git -C $D/src rev-parse HEAD)
            if [ "$head" != "$commit" ]; then
                rm -rf $D/src
                echo "deps: DXVK $tag is at $head, not the pinned $commit" >&2
                exit 1
            fi
        fi
        rm -rf $D/build $D/dxvk
        # The libdir is pinned so every later path is dxvk/lib, never lib64 or lib/x86_64-linux-gnu.
        meson setup --buildtype release -Dnative_sdl3=enabled -Dnative_sdl2=disabled -Dnative_glfw=disabled \
            --prefix /src/$D/dxvk --libdir lib $D/build $D/src
        ninja -C $D/build install
        # The newest glibc symbol each library needs: 2.31 or older runs on a Steam Deck.
        for f in $D/dxvk/lib/libdxvk_d3d11.so.0 $D/dxvk/lib/libdxvk_dxgi.so.0; do
            echo "$f $(objdump -T $f | grep -o "GLIBC_[0-9.]*" | sort -Vu | tail -1)"
        done' "$DXVK_REPO" "$DXVK_TAG" "$DXVK_COMMIT"
    ;;
cargo)
    shift
    in_container "$RUST"'
        export LD_LIBRARY_PATH=/src/target/linux-deps/dxvk/lib
        cargo "$@"' "$@"
    ;;
release)
    for lib in libdxvk_d3d11.so.0 libdxvk_dxgi.so.0; do
        if [ ! -e "$ROOT/target/linux-deps/dxvk/lib/$lib" ]; then
            echo "release needs DXVK's libraries: run tools/linux-build.sh deps first." >&2
            exit 1
        fi
    done
    # (this folder only: the release is laid out afresh)
    rm -rf "$ROOT/target/linux-release"
    in_container "$RUST"'
        cargo build --release --locked -p mvs-app --bin mvs --bin mvs-launcher
        B=target/linux-sniper/release R=target/linux-release
        G=$R/game
        mkdir -p $G/licenses
        # The names come from mvs_release: LINUX_LAUNCHER_EXE at the top, LINUX_GAME_EXE in GAME_DIR. Each program
        # finds the libraries in game/ through the RPATH mvs-app/build.rs gives it.
        cp $B/mvs-launcher "$R/MVSC Launcher"
        cp $B/mvs "$G/MVS Classic"
        # Without their debug info, which on Windows goes to a .pdb the zip does not carry (about 90 MB, not 400);
        # the symbols stay, so a backtrace still names functions. The builds in target/linux-sniper keep everything.
        strip --strip-debug "$R/MVSC Launcher" "$G/MVS Classic"
        # Each library a regular file named by its soname (the zip writer has no symlinks); SDL3 from the runtime.
        cp -L target/linux-deps/dxvk/lib/libdxvk_d3d11.so.0 target/linux-deps/dxvk/lib/libdxvk_dxgi.so.0 $G/
        cp -L /usr/lib/x86_64-linux-gnu/libSDL3.so.0 $G/libSDL3.so.0
        cp online.json $G/online.json
        cp target/linux-deps/src/LICENSE $G/licenses/LICENSE-DXVK.txt
        cp /usr/share/doc/libsdl3-0/copyright $G/licenses/LICENSE-SDL3.txt
        # The version the game reports, which its compatibility id hashes, not the variable.
        line=$("$G/MVS Classic" --version)
        version=${line#mvs }
        if [ "$line" = "$version" ] || [ -z "$version" ]; then
            echo "release: the game printed \"$line\" for --version, not \"mvs <version>\"" >&2
            exit 1
        fi
        echo "version $version"
        exe=$(sha256sum "$G/MVS Classic" | cut -d " " -f 1)
        launcher=$(sha256sum "$R/MVSC Launcher" | cut -d " " -f 1)
        printf "{\n  \"version\": \"%s\",\n  \"exe\": \"%s\",\n  \"launcher\": \"%s\"\n}\n" \
            "$version" "$exe" "$launcher" > $G/linux-build.json
        # The newest glibc symbol each program and library needs: 2.31 or older runs on a Steam Deck.
        newer=0
        while IFS= read -r -d "" f; do
            readelf -h "$f" > /dev/null 2>&1 || continue
            glibc=$(objdump -T "$f" | grep -o "GLIBC_[0-9.]*" | sort -Vu | tail -1)
            echo "$f ${glibc:-no GLIBC_ symbols}"
            if [ -n "$glibc" ] && [ "$(printf "%s\n" "${glibc#GLIBC_}" 2.31 | sort -V | tail -1)" != 2.31 ]; then
                newer=1
            fi
        done < <(find $R -type f -print0 | sort -z)
        if [ $newer = 1 ]; then
            echo "release: a file above needs a glibc newer than 2.31" >&2
            exit 1
        fi'
    ;;
-h | --help)
    usage
    ;;
*)
    usage >&2
    exit 2
    ;;
esac
