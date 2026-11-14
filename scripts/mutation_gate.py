#!/usr/bin/env python3
"""Mutation testing sample on changed code (docs/PLAN.md section 15.5, gate 16).

Usage: mutation_gate.py <outcomes.json> <minimum caught share>

Reads the outcomes file that `cargo mutants --in-diff` writes and fails when the tests
caught (or timed out on) less than the minimum share of viable mutants. Mutants that do
not compile are not viable and do not count. This catches tests that assert nothing.

Protected path: loop agents may not edit this file.
"""

import json
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    path, minimum = Path(sys.argv[1]), float(sys.argv[2])
    data = json.loads(path.read_text())
    outcomes = data.get("outcomes", [])
    counts = {"caught": 0, "missed": 0, "timeout": 0, "unviable": 0}
    missed = []
    for outcome in outcomes:
        summary = outcome.get("summary", "")
        scenario = outcome.get("scenario", {})
        if not isinstance(scenario, dict) or "Mutant" not in scenario:
            continue  # the baseline run
        key = {
            "CaughtMutant": "caught",
            "MissedMutant": "missed",
            "Timeout": "timeout",
            "Unviable": "unviable",
        }.get(summary)
        if key:
            counts[key] += 1
        if key == "missed":
            mutant = scenario["Mutant"]
            where = mutant.get("file") or mutant.get("source_file", {}).get("path", "?")
            span = mutant.get("span", {}).get("start", {}).get("line", "?")
            missed.append(f"{where}:{span}: {mutant.get('name', mutant.get('replacement', ''))}")

    viable = counts["caught"] + counts["missed"] + counts["timeout"]
    if viable == 0:
        print(f"no viable mutants in the changed code ({counts['unviable']} unviable)")
        return 0
    share = (counts["caught"] + counts["timeout"]) / viable
    print(
        f"caught {counts['caught']}, timed out {counts['timeout']}, missed {counts['missed']}, "
        f"unviable {counts['unviable']}: {share:.0%} caught (minimum {minimum:.0%})"
    )
    if share < minimum:
        for line in missed[:40]:
            print(f"  missed: {line}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
