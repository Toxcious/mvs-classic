#!/bin/bash
# Makes Cargo see a cached target/linux-sniper as up to date for every file that has not changed since the commit the
# cache was built from. CI restores the cache into a fresh checkout, which stamps every file with the time of the
# checkout: newer than every build in the cache, so Cargo would rebuild all the workspace's crates. Run from the
# checkout, after the cache is restored and before anything builds (.github/workflows/linux.yml).
#
# The cache's commit is in target/linux-sniper/.ci-commit, which the workflow writes only after a run whose builds
# all succeeded. With that commit in this checkout's history, the script sets every tracked file, and every
# directory that holds one, to one old date, then sets to now each file whose content differs from that commit
# (by `git diff`, never by commit dates) and the directories above it, so a build script that watches a directory
# sees a file added or removed there. Cargo then rebuilds exactly what reads a changed file.
#
# It changes nothing, so everything rebuilds, when there is no .ci-commit, when its commit is not in this checkout,
# or when a file that decides how or what Cargo builds has changed (a manifest, the lock, the toolchain, Cargo's
# config, the build script, this workflow): then a crate's build from an older commit, with other features or
# settings, could be in the cache and be taken as up to date.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
stamp=target/linux-sniper/.ci-commit
old='2000-01-01 00:00:00 UTC'

if [ ! -f "$stamp" ]; then
    echo "ci-mtimes: no cached build; everything builds"
    exit 0
fi
commit=$(tr -d '[:space:]' < "$stamp")
if ! [[ $commit =~ ^[0-9a-f]{40}$ ]] || ! git cat-file -e "$commit^{commit}" 2> /dev/null; then
    echo "ci-mtimes: the cache's commit ($commit) is not in this checkout; everything builds"
    exit 0
fi
# (to the working tree, not HEAD: what is on disk is what builds)
settings=$(git diff --name-only --no-renames "$commit" -- Cargo.lock rust-toolchain.toml tools/linux-build.sh \
    .github/workflows/linux.yml ':(glob)**/Cargo.toml' ':(glob)**/.cargo/**')
if [ -n "$settings" ]; then
    echo "ci-mtimes: what Cargo builds has changed since $commit; everything builds:"
    echo "$settings"
    exit 0
fi

# Each NUL-ended path on stdin's directories, from the top ("." included), once each.
dirs_of() {
    awk 'BEGIN { RS = "\0"; ORS = "\0"; print "." }
        { n = split($0, p, "/"); d = ""
          for (i = 1; i < n; i++) { d = d (i > 1 ? "/" : "") p[i]; if (!(d in seen)) { seen[d]; print d } } }'
}

# (listed before any time changes)
changed=$(mktemp)
trap 'rm -f "$changed"' EXIT
git diff --name-only --no-renames -z "$commit" > "$changed"

git ls-files -z | xargs -0 -r touch -c -h -d "$old" --
git ls-files -z | dirs_of | xargs -0 -r touch -c -h -d "$old" --
# (a file the commit had and this one does not: -c skips it, and its directories still change)
xargs -0 -r touch -c -h -- < "$changed"
dirs_of < "$changed" | xargs -0 -r touch -c -h --
echo "ci-mtimes: cache from $commit; $(tr -cd '\0' < "$changed" | wc -c) files changed since"
