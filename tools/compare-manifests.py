#!/usr/bin/env python3
"""Compare the artifacts of two Wana OS build manifests.

Reproducibility check: two builds of the same commit must produce the same
SHA-256 for every artifact. Prints one line per artifact and exits 1 if any
artifact differs or exists in only one build.

Usage: compare-manifests.py A/build-manifest.json B/build-manifest.json
"""

import json
import sys


def load(path):
    with open(path) as f:
        m = json.load(f)
    arts = {a["path"]: a for a in m["artifacts"]}
    return m, arts


def main(argv) -> int:
    if len(argv) != 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    (ma, a), (mb, b) = load(argv[1]), load(argv[2])

    problems = 0
    if ma["git"]["commit"] != mb["git"]["commit"]:
        print(f"[BUILD] error: different commits: {ma['git']['commit']} vs {mb['git']['commit']}")
        problems += 1
    for key in ("buildroot", "kernel", "toolchain", "resolved_configs", "packages", "source_date_epoch"):
        if ma.get(key) != mb.get(key):
            print(f"[BUILD] error: inputs differ: {key}")
            problems += 1

    same = 0
    for path in sorted(set(a) | set(b)):
        x, y = a.get(path), b.get(path)
        if x is None or y is None:
            print(f"DIFFERENT  {path}  (only in {'B' if x is None else 'A'})")
            problems += 1
        elif x.get("sha256") != y.get("sha256") or x.get("symlink") != y.get("symlink"):
            print(f"DIFFERENT  {path}  {x.get('sha256', x.get('symlink'))[:16]} vs {y.get('sha256', y.get('symlink'))[:16]}")
            problems += 1
        else:
            print(f"identical  {path}")
            same += 1

    total = len(set(a) | set(b))
    if problems:
        print(f"[BUILD] reproducibility: FAIL ({same}/{total} artifacts identical, {problems} problem(s))")
        return 1
    print(f"[BUILD] reproducibility: PASS ({same}/{total} artifacts identical)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
