#!/bin/bash
# Makes Cargo see a cached target dir as up to date for every file that has not changed since the commit the cache
# was built from. CI restores the cache into a fresh checkout, which stamps every file with the time of the checkout:
# newer than every build in the cache, so Cargo would rebuild all the workspace's crates. Run from the checkout, after
# the cache is restored and before anything builds.
#
#   tools/ci-mtimes.sh [<target dir>]   the cached target dir (default target/linux-sniper)
#
# The cache's commit is in <target dir>/.ci-commit, which the workflow writes only after a run whose builds all
# succeeded. With that commit in this checkout's history, the script sets every tracked file, and every directory that
# holds one, to one old date, then sets to now each file whose content differs from that commit (by `git diff`, never
# by commit dates) and the directories above it, so a build script that watches a directory sees a file added or
# removed there. Cargo then rebuilds exactly what reads a changed file.
#
# The file times stand in only for the sources, the one input Cargo judges by time. Everything else that decides how
# or what Cargo builds is in each unit's fingerprint, whatever the times: the manifests' features and dependencies,
# the lock's versions, rustc's version, the profile, RUSTFLAGS, [lints] and the environment variables a crate reads
# (option_env!, rerun-if-env-changed). A change to any of them rebuilds what it touches, so a manifest, the lock or
# the toolchain changing is no reason to rebuild everything. What Cargo does not see (the C compiler, the linker,
# native libraries found by path, the build container) belongs in the cache's key.
#
# It changes nothing, so everything rebuilds, when there is no .ci-commit or when its commit is not in this checkout.
set -euo pipefail

target=${1:-}
# (a relative target dir is the caller's, before the cd)
if [ -n "$target" ] && [ "${target#/}" = "$target" ]; then
    target=$PWD/$target
fi
cd "$(git rev-parse --show-toplevel)"
stamp=${target:-target/linux-sniper}/.ci-commit
old='2000-01-01 00:00:00 UTC'

if [ ! -f "$stamp" ]; then
    echo "ci-mtimes: no cached build ($stamp); everything builds"
    exit 0
fi
commit=$(tr -d '[:space:]' < "$stamp")
if ! [[ $commit =~ ^[0-9a-f]{40}$ ]] || ! git cat-file -e "$commit^{commit}" 2> /dev/null; then
    echo "ci-mtimes: the cache's commit ($commit) is not in this checkout; everything builds"
    exit 0
fi

# Each NUL-ended path on stdin's directories, from the top ("." included), once each.
dirs_of() {
    awk 'BEGIN { RS = "\0"; ORS = "\0"; print "." }
        { n = split($0, p, "/"); d = ""
          for (i = 1; i < n; i++) { d = d (i > 1 ? "/" : "") p[i]; if (!(d in seen)) { seen[d]; print d } } }'
}

# (listed before any time changes; to the working tree, not HEAD: what is on disk is what builds)
changed=$(mktemp)
trap 'rm -f "$changed"' EXIT
git diff --name-only --no-renames -z "$commit" > "$changed"

git ls-files -z | xargs -0 -r touch -c -h -d "$old" --
git ls-files -z | dirs_of | xargs -0 -r touch -c -h -d "$old" --
# (a file the commit had and this one does not: -c skips it, and its directories still change)
xargs -0 -r touch -c -h -- < "$changed"
dirs_of < "$changed" | xargs -0 -r touch -c -h --
echo "ci-mtimes: cache from $commit; $(tr -cd '\0' < "$changed" | wc -c) files changed since"
