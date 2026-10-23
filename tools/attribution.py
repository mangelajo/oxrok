#!/usr/bin/env python3
"""Regenerate the "Counterpart in sigrok" comments of the Rust sources.

Each Rust file that corresponds to a sigrok source gets a comment naming
that source, the copyright holders from its header, and everyone else
who committed to it according to the git history, so that the credit
covers the people whose work defined the behaviour oxrok reproduces.

    tools/attribution.py [--ascii] [SIGROK_SRC]

SIGROK_SRC holds the libsigrok and sigrok-cli checkouts (default: the
sibling sigrok-intronix-driver directory). --ascii transliterates names
to ASCII; by default they are kept as written in the git history.
"""
import os
import re
import subprocess
import sys
import unicodedata

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
INDEPENDENT = "a port of the sigrok counterpart, GPL-3.0-or-later like it"
SAME_AUTHOR = "a port by the same author, GPL-3.0-or-later like it"

# Rust file -> (counterparts, relation). A counterpart is a path under
# SIGROK_SRC, optionally with a note in parentheses, or free text when it
# is not a file in the checkouts.
MAP = {
    "src/core/key.rs": (["libsigrok/src/hwdriver.c (the key table)",
                                "libsigrok/include/libsigrok/libsigrok.h (the key definitions)"], INDEPENDENT),
    "src/core/option.rs": (["libsigrok/src/hwdriver.c", "libsigrok/src/std.c (the option list helpers)"], INDEPENDENT),
    "src/core/value.rs": (["sigrok-cli/parsers.c", "libsigrok/src/strutil.c"], INDEPENDENT),
    "src/core/util.rs": (["libsigrok/src/strutil.c"], INDEPENDENT),
    "src/core/session.rs": (["libsigrok/src/session.c"], INDEPENDENT),
    "src/core/packet.rs": (["libsigrok/src/session.c (the datafeed packets)"], INDEPENDENT),
    "src/core/device.rs": (["libsigrok/src/device.c", "libsigrok/src/hwdriver.c"], INDEPENDENT),
    "src/core/channel.rs": (["libsigrok/src/device.c"], INDEPENDENT),
    "src/core/softtrig.rs": (["libsigrok/src/soft-trigger.c"], INDEPENDENT),
    "src/core/firmware.rs": (["libsigrok/src/resource.c"], INDEPENDENT),
    "src/core/analog.rs": (["libsigrok/src/analog.c"], INDEPENDENT),
    "src/core/trigger.rs": (["libsigrok/src/trigger.c"], INDEPENDENT),
    "src/core/error.rs": (["libsigrok/src/error.c"], INDEPENDENT),
    "src/formats/output/mod.rs": (["libsigrok/src/output/output.c"], INDEPENDENT),
    "src/formats/output/bits.rs": (["libsigrok/src/output/bits.c"], INDEPENDENT),
    "src/formats/output/hex.rs": (["libsigrok/src/output/hex.c"], INDEPENDENT),
    "src/formats/output/ascii.rs": (["libsigrok/src/output/ascii.c"], INDEPENDENT),
    "src/formats/output/analog.rs": (["libsigrok/src/output/analog.c"], INDEPENDENT),
    "src/formats/output/csv.rs": (["libsigrok/src/output/csv.c"], INDEPENDENT),
    "src/formats/output/vcd.rs": (["libsigrok/src/output/vcd.c"], INDEPENDENT),
    "src/formats/output/binary.rs": (["libsigrok/src/output/binary.c"], INDEPENDENT),
    "src/formats/output/srzip.rs": (["libsigrok/src/output/srzip.c"], INDEPENDENT),
    "src/formats/sr.rs": (["libsigrok/src/session_file.c", "libsigrok/src/session_driver.c",
                                  "libsigrok/src/output/srzip.c"], INDEPENDENT),
    "src/formats/input/mod.rs": (["libsigrok/src/input/input.c"], INDEPENDENT),
    "src/formats/input/vcd.rs": (["libsigrok/src/input/vcd.c"], INDEPENDENT),
    "src/formats/input/binary.rs": (["libsigrok/src/input/binary.c"], INDEPENDENT),
    "src/usb/mod.rs": (["libsigrok/src/usb.c"], INDEPENDENT),
    "src/ftdi/mod.rs": (["libftdi1 (ftdi.c), by Intra2net AG and the libftdi developers"],
                               "written for oxrok from FTDI's application notes; "
                               "libftdi was the reference for which requests a driver needs"),
    "src/ezusb/mod.rs": (["libsigrok/src/ezusb.c"], INDEPENDENT),
    "src/drivers/demo/mod.rs": (["libsigrok/src/hardware/demo/api.c",
                                        "libsigrok/src/hardware/demo/protocol.c"], INDEPENDENT),
    "src/drivers/demo/patterns.rs": (["libsigrok/src/hardware/demo/protocol.c"],
                                            "a port of the sigrok counterpart, GPL-3.0-or-later like it; "
                                            "the logo bitmap was replaced by oxrok's own pattern"),
    "src/drivers/fx2lafw/mod.rs": (["libsigrok/src/hardware/fx2lafw/api.c",
                                           "libsigrok/src/hardware/fx2lafw/protocol.c",
                                           "sigrok-firmware-fx2lafw (the command set)"], INDEPENDENT),
    "src/drivers/logicport/mod.rs": (["libsigrok/src/hardware/intronix-logicport/api.c"], SAME_AUTHOR),
    "src/drivers/logicport/protocol.rs": (["libsigrok/src/hardware/intronix-logicport/protocol.c"], SAME_AUTHOR),
    "src/drivers/logicport/readout.rs": (["libsigrok/src/hardware/intronix-logicport/protocol.c (the readout)"], SAME_AUTHOR),
    "src/drivers/logicport/firmware.rs": (["libsigrok/src/hardware/intronix-logicport/protocol.c (the ccf loader)"], SAME_AUTHOR),
    "src/cli/mod.rs": (["sigrok-cli/main.c"], INDEPENDENT),
    "src/cli/args.rs": (["sigrok-cli/options.c"], INDEPENDENT),
    "src/cli/parse.rs": (["sigrok-cli/parsers.c"], INDEPENDENT),
    "src/cli/show.rs": (["sigrok-cli/show.c"], INDEPENDENT),
    "src/cli/run.rs": (["sigrok-cli/session.c"], INDEPENDENT),
    "src/cli/pipeline.rs": (["sigrok-cli/session.c"], INDEPENDENT),
    "src/cli/input.rs": (["sigrok-cli/input.c"], INDEPENDENT),
    "src/cli/logger.rs": (["libsigrok/src/log.c"], INDEPENDENT),
}


def ascii(name):
    return unicodedata.normalize("NFKD", name).encode("ascii", "ignore").decode()


def holders(path):
    out = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            m = re.match(r"\s*\*\s*(Copyright \(C\) .*?)\s*$", line)
            if m:
                out.append(m.group(1))
            elif out and "*/" in line:
                break
    return out


def committers(repo, rel):
    log = subprocess.run(["git", "-C", repo, "log", "--format=%an", "--", rel],
                         capture_output=True, text=True, check=True).stdout.split("\n")
    counts = {}
    for a in log:
        if a:
            counts[a] = counts.get(a, 0) + 1
    return [a for a, _ in sorted(counts.items(), key=lambda x: (-x[1], x[0]))]


def describe(repo):
    r = subprocess.run(["git", "-C", repo, "describe", "--always", "--dirty"],
                       capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else "unknown"


def wrap(prefix, items, sep, width=78):
    """Comment lines starting with `prefix`, items joined by `sep`."""
    lines, cur = [], prefix
    for i, item in enumerate(items):
        piece = item + (sep.rstrip() if i + 1 < len(items) else "")
        if len(cur) + len(piece) + 1 > width:
            lines.append(cur.rstrip())
            cur = "//   " + piece
        else:
            cur += " " + piece
    lines.append(cur.rstrip())
    return lines


def main():
    args = sys.argv[1:]
    to_ascii = "--ascii" in args
    args = [a for a in args if a != "--ascii"]
    src = args[0] if args else os.path.join(os.path.dirname(ROOT), "sigrok-intronix-driver")
    versions = {}
    for rust, (parts, relation) in MAP.items():
        lines = ["// Counterpart in sigrok:"]
        all_holders, all_committers = [], []
        for part in parts:
            path = part.split(" (")[0]
            full = os.path.join(src, path)
            lines.append("//   " + part + ("," if part != parts[-1] else ","))
            if not os.path.isfile(full):
                continue
            repo = os.path.join(src, path.split("/")[0])
            versions[path.split("/")[0]] = describe(repo)
            for h in holders(full):
                if h not in all_holders:
                    all_holders.append(h)
            for c in committers(repo, os.path.relpath(full, repo)):
                if c not in all_committers:
                    all_committers.append(c)
        names_in_holders = [re.sub(r".*\d{4}(?:-\d{4})?\s+([^<]+?)\s*<.*", r"\1", h) for h in all_holders]
        extra = [c for c in all_committers if c not in names_in_holders]
        if to_ascii:
            all_holders = [ascii(h) for h in all_holders]
            extra = [ascii(e) for e in extra]
        for h in all_holders:
            lines.append("//   " + h + ",")
        if extra:
            lines += wrap("//   with commits by", extra, ", ")
            lines[-1] += ","
        used = sorted({p.split("/")[0] for p in parts if p.split("/")[0] in versions})
        if used:
            lines[-1] = lines[-1].rstrip(",")
            lines.append("//   (" + ", ".join(f"{u} {versions[u]}" for u in used) + ").")
        else:
            lines[-1] = lines[-1].rstrip(",") + "."
        lines += wrap("// This file is", relation.split(" "), " ")
        lines[-1] += "."
        block = "\n".join(lines) + "\n\n"
        full_rust = os.path.join(ROOT, rust)
        text = open(full_rust, encoding="utf-8").read()
        if text.startswith("// Counterpart in sigrok"):
            end = text.index("\n\n") + 2
            text = text[end:]
        open(full_rust, "w", encoding="utf-8").write(block + text)
    print(f"{len(MAP)} files updated")


main()
