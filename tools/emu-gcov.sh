#!/bin/sh
# Line coverage of a libsigrok driver, from the oracle built with
# --coverage (docs/DEVICES.md, "Oracle"; docs/ROADMAP.md, criterion 4
# of "What counts as emulator-verified").
#
#   tools/emu-gcov.sh DRIVER [zero|report]
#
# DRIVER is a directory under libsigrok's src/hardware. 'zero' deletes
# its .gcda files, so that what the next sigrok-cli runs reach is all a
# report counts. 'report' (the default) runs gcov on the driver's
# sources and prints the share of executable lines reached, per file and
# in total, then the executable lines never reached, as ranges per
# function. LIBSIGROK names the build tree (default
# ~/work/sigrok/libsigrok; a relative path or one through a symlink is
# resolved first, since gcov records the real path it compiled in). A
# report that finds no executable line of the driver is an error.
#
# libtool builds every source twice: the PIC object that goes into
# libsigrok.so, and so writes the counters sigrok-cli produces, is
# .libs/NAME.o with .libs/NAME.gcno and .libs/NAME.gcda; the static
# NAME.o next to the source has a .gcno but never runs. Objects named
# libsigrok_la-NAME.o, as other libtool setups write them, are looked
# for too. A file with no .gcda counts as never run.

set -eu
LIBSIGROK=${LIBSIGROK:-$HOME/work/sigrok/libsigrok}
[ $# -ge 1 ] || { echo "usage: $0 DRIVER [zero|report]" >&2; exit 2; }
driver=$1
tree=$LIBSIGROK
LIBSIGROK=$(cd "$tree" 2>/dev/null && pwd -P) || { echo "$0: no libsigrok tree $tree" >&2; exit 2; }
dir=$LIBSIGROK/src/hardware/$driver
[ -d "$dir" ] || { echo "$0: no driver directory $dir" >&2; exit 2; }
dir=$(cd "$dir" && pwd -P)

case "${2:-report}" in
    zero)
        find "$dir" -name '*.gcda' -delete
        ;;
    report)
        version=$(git -C "$LIBSIGROK" describe --always --dirty 2>/dev/null || echo unknown)
        work=$(mktemp -d)
        trap 'rm -rf "$work"' EXIT
        : >"$work/json"
        for src in "$dir"/*.c; do
            base=$(basename "$src" .c)
            obj=""
            for cand in ".libs/$base" ".libs/libsigrok_la-$base" "$base" "libsigrok_la-$base"; do
                if [ -f "$dir/$cand.gcda" ]; then obj="$dir/$cand"; break; fi
                if [ -z "$obj" ] && [ -f "$dir/$cand.gcno" ]; then obj="$dir/$cand"; fi
            done
            if [ -z "$obj" ]; then
                echo "$0: no .gcno for $src (was libsigrok built with --coverage?)" >&2
                exit 1
            fi
            # gcov wants the object's directory and the object's name
            # stem as the source's: the 'FILE' form of -o covers both.
            if ! (cd "$work" && gcov -j -t -o "$obj.o" "$src" 2>"$work/err") >>"$work/json"; then
                echo "$0: gcov failed on $src:" >&2; cat "$work/err" >&2
                exit 1
            fi
            echo >>"$work/json"
        done
        python3 -I - "$work/json" "$dir" "$driver" "$version" <<'PY'
import json, os, sys

path, root, driver, version = sys.argv[1:]
files = {}  # name -> {line: (count, function)}
for chunk in open(path, encoding="utf-8").read().split("\n"):
    if not chunk.strip():
        continue
    doc = json.loads(chunk)
    cwd = doc["current_working_directory"]
    for f in doc["files"]:
        full = os.path.realpath(os.path.join(cwd, f["file"]))
        if os.path.dirname(full) != os.path.realpath(root):
            continue  # headers of libsigrok itself, inlined
        lines = files.setdefault(os.path.basename(full), {})
        for ln in f["lines"]:
            n = ln["line_number"]
            count, fn = lines.get(n, (0, ln.get("function_name", "")))
            lines[n] = (count + ln["count"], fn or ln.get("function_name", ""))

if not any(files.values()):
    sys.exit(f"emu-gcov.sh: gcov reported no executable line in {root}")
print(f"gcov {driver} (libsigrok {version}): executable lines reached")
width = max([len(n) for n in files] + [5])
tot_hit = tot_all = 0
for name in sorted(files):
    lines = files[name]
    hit = sum(1 for c, _ in lines.values() if c > 0)
    tot_hit += hit
    tot_all += len(lines)
    pct = 100.0 * hit / len(lines) if lines else 100.0
    print(f"  {name:<{width}}  {pct:5.1f}%  {hit} of {len(lines)}")
pct = 100.0 * tot_hit / tot_all if tot_all else 100.0
print(f"  {'total':<{width}}  {pct:5.1f}%  {tot_hit} of {tot_all}")

print("never reached, by function (line ranges hold no reached line):")
for name in sorted(files):
    lines = files[name]
    by_fn = {}
    for n in sorted(lines):
        by_fn.setdefault(lines[n][1], []).append(n)
    out = []
    for fn, nums in sorted(by_fn.items(), key=lambda kv: kv[1][0]):
        ranges, start, prev = [], None, None
        for n in nums:
            if lines[n][0] == 0:
                if start is None:
                    start = n
                prev = n
            elif start is not None:
                ranges.append((start, prev))
                start = None
        if start is not None:
            ranges.append((start, prev))
        if not ranges:
            continue
        if all(lines[n][0] == 0 for n in nums):
            out.append(f"    {fn or '?'}: never called ({nums[0]}-{nums[-1]})")
        else:
            text = ", ".join(f"{a}" if a == b else f"{a}-{b}" for a, b in ranges)
            out.append(f"    {fn or '?'}: {text}")
    if out:
        print(f"  {name}")
        print("\n".join(out))
PY
        ;;
    *)
        echo "usage: $0 DRIVER [zero|report]" >&2; exit 2 ;;
esac
