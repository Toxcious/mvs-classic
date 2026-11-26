#!/bin/bash
# Removes from a cached target dir what the job's builds no longer use, before the cache is saved. Every unit Cargo
# builds is named by a 16-hex hash (`<name>-<hash>`) of its package id with the version, its features, its profile
# and rustc; a version bump, a dependency's update or a changed feature builds the unit again under a new hash and
# leaves the old one in the dir, which would then be saved with every later cache.
#
#   tools/ci-prune.sh <target dir> <units file>...
#
# Each units file is the stdout of one cargo command the job ran with --message-format=json-render-diagnostics (the
# human diagnostics still go to stderr); give all of them, since a unit only one build named is still used. A unit is
# kept when any of them names it: the filenames of each compiler-artifact message (fresh units included) and the
# out_dir of each build-script-executed. A program Cargo copied out of deps/ (`release/mvs`) carries no hash: its
# twin in deps/ (the same inode, or the same bytes) gives it. On *-windows-msvc Cargo writes the programs to deps/
# without the hash too (`deps/mvs.exe`, so the PDB's name matches), so a program's .fingerprint dir is kept by the
# file `bin-<name>` in it instead: every such dir in the program's profile dir, a few stale ones of old builds (a few
# KB each) included. A unit whose hash it cannot find is named in one line, and the prune goes on.
#
# In each profile dir (<target dir>/release, /trace, ..., and <target dir>/<triple>/<profile> for a cross-build) it
# removes the files in deps/ and the dirs in .fingerprint/ and build/ whose hash no kept unit has, and incremental/
# entirely (CI never uses it). What has no hash is left. It prints the size before and after.
#
# It refuses, removing nothing, when it is given no units file or an empty one, or when the units name no unit: a
# prune that keeps nothing would empty the cache.
set -euo pipefail

if [ $# -lt 2 ]; then
    sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//' >&2
    exit 2
fi
target=${1%/}
shift
if [ ! -d "$target" ]; then
    echo "ci-prune: $target is not a directory" >&2
    exit 1
fi
for f in "$@"; do
    if [ ! -s "$f" ]; then
        echo "ci-prune: the units file $f is missing or empty; nothing removed" >&2
        exit 1
    fi
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# The profile dirs: each dir one or two levels down that holds a .fingerprint.
find "$target" -mindepth 2 -maxdepth 3 -type d -name .fingerprint -printf '%h\n' | sed "s|^$target/||" | sort -u \
    > "$work/profiles"
if [ ! -s "$work/profiles" ]; then
    echo "ci-prune: $target has no profile dir (no .fingerprint); nothing removed" >&2
    exit 1
fi

# Every path the units name (lines that are not JSON skipped).
jq -R -r 'fromjson? | if .reason == "compiler-artifact" then .filenames[]
    elif .reason == "build-script-executed" then .out_dir else empty end' "$@" | sort -u > "$work/paths"

# A path's hash: the last `-<16 hex>` before a dot, a slash or its end.
hash_of() {
    grep -oE -- '-[0-9a-f]{16}([./]|$)' | tail -n 1 | cut -c 2-17
}

: > "$work/keep"
while IFS= read -r path; do
    h=$(hash_of <<< "$path" || true)
    if [ -n "$h" ]; then
        echo "$h" >> "$work/keep"
        continue
    fi
    # (a program copied out of deps/: found in the profile dir whose path ends the units' path)
    name=${path##*/}
    while IFS= read -r p; do
        case $path in
        */"$p/$name") ;;
        *) continue ;;
        esac
        file=$target/$p/$name
        [ -f "$file" ] || continue
        twin=$(find "$target/$p/deps" -maxdepth 1 -type f -samefile "$file" -print -quit)
        if [ -z "$twin" ]; then
            stem=${name%.*} ext=
            [ "$stem" = "$name" ] || ext=.${name##*.}
            for c in "$target/$p/deps/${stem//-/_}"-*; do
                if [ -f "$c" ] && [[ ${c##*/} =~ ^${stem//-/_}-[0-9a-f]{16}${ext:-}$ ]] && cmp -s "$c" "$file"; then
                    twin=$c
                    break
                fi
            done
        fi
        if [ -n "$twin" ]; then
            # (a hashless twin, as on *-windows-msvc: the program's bin-<name> keeps its unit below)
            hash_of <<< "$twin" >> "$work/keep" || true
        else
            echo "ci-prune: no twin in $p/deps for $path; its unit may rebuild" >&2
        fi
    done < "$work/profiles"
done < "$work/paths"
sort -u -o "$work/keep" "$work/keep"
if [ ! -s "$work/keep" ]; then
    echo "ci-prune: the units files name no unit; nothing removed" >&2
    exit 1
fi

# Each program's .fingerprint dirs: those in its profile dir (the longest profile path its filenames' dir ends with)
# that hold a file bin-<name>.
jq -R -r 'fromjson? | select(.reason == "compiler-artifact" and (.target.kind | index("bin")))
    | .target.name as $n | .filenames[] | "\($n)\t\(.)"' "$@" | sort -u > "$work/bins"
awk '{ print length($0) "\t" $0 }' "$work/profiles" | sort -rn | cut -f 2- > "$work/profiles-longest"
: > "$work/bin-dirs"
while IFS=$'\t' read -r name path; do
    dir=${path%/*}
    dir=${dir%/deps}
    while IFS= read -r p; do
        case $dir in
        */"$p" | "$p") ;;
        *) continue ;;
        esac
        echo "$p"$'\t'"$name"
        break
    done < "$work/profiles-longest"
done < "$work/bins" | sort -u | while IFS=$'\t' read -r p name; do
    found=$(find "$target/$p/.fingerprint" -mindepth 2 -maxdepth 2 -type f -name "bin-$name" -printf '%h\n')
    if [ -z "$found" ]; then
        echo "ci-prune: no .fingerprint dir in $p holds bin-$name; that program may rebuild" >&2
    fi
    printf '%s\n' "$found"
done | sed '/^$/d' | sort -u > "$work/bin-dirs"

size() {
    du -sb "$target" | cut -f 1 | numfmt --to=iec --suffix=B
}
echo "ci-prune: $target is $(size) before; $(wc -l < "$work/keep") units kept"

# Each entry of the dir $1 (files when $2 is f, dirs when d) whose hash is not kept, NUL-ended.
stale() {
    [ -d "$1" ] || return 0
    find "$1" -mindepth 1 -maxdepth 1 -type "$2" -printf '%f\0' |
        awk -v dir="$1" -v keep="$work/keep" -v bins="$work/bin-dirs" '
            BEGIN { while ((getline k < keep) > 0) kept[k]; while ((getline b < bins) > 0) bin[b]
                    RS = "\0"; ORS = "\0"
                    re = "-"; for (i = 0; i < 16; i++) re = re "[0-9a-f]"; re = re "([./]|$)" }
            { h = ""; s = $0
              while (match(s, re)) { h = substr(s, RSTART + 1, 16); s = substr(s, RSTART + 17) }
              if (h != "" && !(h in kept) && !((dir "/" $0) in bin)) print dir "/" $0 }'
}

removed=0
while IFS= read -r p; do
    d=$target/$p
    {
        stale "$d/deps" f
        stale "$d/.fingerprint" d
        stale "$d/build" d
    } > "$work/stale"
    n=$(tr -cd '\0' < "$work/stale" | wc -c)
    removed=$((removed + n))
    xargs -0 -r rm -rf -- < "$work/stale"
    rm -rf -- "$d/incremental"
    echo "ci-prune: $p: $n removed"
done < "$work/profiles"
echo "ci-prune: $target is $(size) after; $removed removed"
