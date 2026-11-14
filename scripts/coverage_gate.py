#!/usr/bin/env python3
"""Coverage on changed lines (docs/PLAN.md section 15.5, gate 15).

Usage: coverage_gate.py <base rev> <lcov file> <minimum percent>

Counts the lines added or changed since <base rev> in crates/*/src/**/*.rs that the
coverage run instrumented, and fails when fewer than <minimum percent> of them ran.
Lines the compiler did not instrument (comments, declarations) do not count.

Protected path: loop agents may not edit this file.
"""

import re
import subprocess
import sys
from pathlib import Path


def changed_lines(base: str) -> dict[str, set[int]]:
    # In a git pathspec `*` also matches `/`, so this covers every file under src.
    diff = subprocess.run(
        ["git", "diff", "-U0", "--no-color", base, "--", "crates/*/src/*.rs"],
        check=True, capture_output=True, text=True,
    ).stdout
    lines: dict[str, set[int]] = {}
    current = None
    for line in diff.splitlines():
        if line.startswith("+++ "):
            path = line[4:]
            current = None if path == "/dev/null" else path.removeprefix("b/")
        elif line.startswith("@@") and current:
            match = re.search(r"\+(\d+)(?:,(\d+))?", line)
            if match:
                start, count = int(match[1]), int(match[2] or "1")
                lines.setdefault(current, set()).update(range(start, start + count))
    return lines


def covered(lcov: Path, root: Path) -> dict[str, dict[int, int]]:
    hits: dict[str, dict[int, int]] = {}
    current = None
    for line in lcov.read_text().splitlines():
        if line.startswith("SF:"):
            path = Path(line[3:])
            try:
                current = path.resolve().relative_to(root).as_posix()
            except ValueError:
                current = None
        elif line.startswith("DA:") and current:
            number, count = line[3:].split(",")[:2]
            hits.setdefault(current, {})[int(number)] = int(count)
    return hits


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__)
        return 2
    base, lcov, minimum = sys.argv[1], Path(sys.argv[2]), float(sys.argv[3])
    root = Path.cwd().resolve()
    changes = changed_lines(base)
    hits = covered(lcov, root)
    total = ran = 0
    missed: list[str] = []
    for path, numbers in sorted(changes.items()):
        file_hits = hits.get(path, {})
        for number in sorted(numbers):
            if number in file_hits:
                total += 1
                if file_hits[number] > 0:
                    ran += 1
                else:
                    missed.append(f"{path}:{number}")
    if total == 0:
        print("no instrumented lines changed")
        return 0
    percent = 100.0 * ran / total
    print(f"{ran} of {total} changed lines ran ({percent:.1f}%, minimum {minimum:.0f}%)")
    if percent < minimum:
        for place in missed[:40]:
            print(f"  not run: {place}")
        if len(missed) > 40:
            print(f"  ... and {len(missed) - 40} more")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
