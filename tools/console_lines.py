#!/usr/bin/env python3
"""Repair serial-console lines split by asynchronous kernel messages.

The kernel console and userspace share one serial line. A kernel message
(printk, or a program writing /dev/kmsg such as udevd) can be printed in the
middle of a line a program is writing:

    [INIT] info: udev: /sbin[    1.637959] udevd[77]: starting version 3.2.14
    /udevd started (pid 78)

A kernel console line always starts with "[ seconds.microseconds] ". When
one appears after the start of a line, this tool moves it to its own line
and joins the interrupted text with its continuation (the next line):

    [    1.637959] udevd[77]: starting version 3.2.14
    [INIT] info: udev: /sbin/udevd started (pid 78)

Nothing is dropped. Every kernel line is kept, and the number of repairs is
reported, so boot tests match whole lines without hiding kernel output.

Usage: console_lines.py LOG > REPAIRED   (repairs are reported on stderr)
       console_lines.py --self-test
"""

import re
import sys

KERNEL_LINE = re.compile(r"\[ *\d+\.\d{6}\] ")
# Terminal control sequences (the firmware clears the screen before the
# kernel prints its first line) and whitespace are not interrupted text.
NOT_TEXT = re.compile(r"(\x1b\[[0-9;=?]*[A-Za-z]|\s)*")


def repair(text):
    """Returns (repaired_text, number_of_repairs)."""
    lines = [l.rstrip("\r") for l in text.split("\n")]
    out, repairs, i = [], 0, 0
    while i < len(lines):
        line = lines[i]
        m = KERNEL_LINE.search(line)
        prefix = line[:m.start()] if m else ""
        if m and NOT_TEXT.fullmatch(prefix):
            line = line[m.start():]
        elif m and i + 1 < len(lines):
            out.append(line[m.start():])
            lines[i + 1] = prefix + lines[i + 1]
            repairs += 1
            i += 1
            continue
        out.append(line)
        i += 1
    return "\n".join(out), repairs


def self_test():
    cases = [
        # CI run 36191209030: udevd's kmsg line inside a wana-init line.
        ("[INIT] info: udev: /sbin[    1.637959] udevd[77]: starting version 3.2.14\n"
         "/udevd started (pid 78)\n",
         "[    1.637959] udevd[77]: starting version 3.2.14\n"
         "[INIT] info: udev: /sbin/udevd started (pid 78)\n", 1),
        # CI run 36157632246: a printk inside a wana-init line.
        ("[INIT] info: /us[    6.501234] wana-kms (78) used greatest stack depth: 12 bytes left\n"
         "r/bin/wana-kms exited successfully\n",
         "[    6.501234] wana-kms (78) used greatest stack depth: 12 bytes left\n"
         "[INIT] info: /usr/bin/wana-kms exited successfully\n", 1),
        # Two kernel lines inside one userspace line.
        ("abc[    1.000001] k1\n[    1.000002] k2\ndef\n",
         "[    1.000001] k1\n[    1.000002] k2\nabcdef\n", 2),
        ("ab[    1.000001] k1\nc[    1.000002] k2\nd\n",
         "[    1.000001] k1\n[    1.000002] k2\nabcd\n", 2),
        # Whole kernel lines and ordinary lines are untouched, CRs removed.
        ("[    0.000000] Linux version 6.18.33-wana\r\n[INIT] info: ready\r\n",
         "[    0.000000] Linux version 6.18.33-wana\n[INIT] info: ready\n", 0),
        # Firmware screen-control codes before the first kernel line: not a
        # split, the codes are dropped and nothing is carried forward.
        ("\x1b[2J\x1b[01;01H\x1b[=3h[    0.000000] Linux version 6.18.33-wana\n"
         "[    0.000001] Command line: console=ttyS0\n",
         "[    0.000000] Linux version 6.18.33-wana\n[    0.000001] Command line: console=ttyS0\n", 0),
        # A timestamp-like text without microseconds is not a kernel line.
        ("value [1.5] here\n", "value [1.5] here\n", 0),
    ]
    for n, (given, want, repairs) in enumerate(cases):
        got = repair(given)
        if got != (want, repairs):
            print(f"[CHECK] error: console_lines case {n}: got {got!r}, want {(want, repairs)!r}",
                  file=sys.stderr)
            return 1
    print(f"[CHECK] console line repair: {len(cases)} cases: PASS")
    return 0


def main():
    if sys.argv[1:] == ["--self-test"]:
        return self_test()
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[-2], file=sys.stderr)
        return 2
    with open(sys.argv[1], "rb") as f:
        text = f.read().decode(errors="replace")
    fixed, repairs = repair(text)
    sys.stdout.write(fixed)
    if repairs:
        print(f"[BOOT] info: repaired {repairs} console line(s) split by kernel messages",
              file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
