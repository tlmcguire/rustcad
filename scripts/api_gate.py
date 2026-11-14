#!/usr/bin/env python3
"""Public API changes match the spec (docs/PLAN.md section 15.5, gate 12).

Usage: api_gate.py <base rev>

For every crate under crates/ with changes since <base rev>, lists its public API at the
base and at HEAD with `cargo public-api` (which needs a nightly toolchain for rustdoc
JSON) and diffs the two. Every item added or removed must be named in the active task spec
(tasks/active/*.md): its own name must appear there, in backticks or plain. With no active
task the diff is printed for the reviewer and the gate passes, since only the owner merges
without a task.

Trait implementations and standard trait methods (`clone`, `fmt`, `default`, ...) are not
checked: implementing a standard trait is not an API decision a spec needs to name.

Protected path: loop agents may not edit this file.
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


def run(cmd: list[str], cwd: Path, env_target: Path) -> subprocess.CompletedProcess:
    env = {**os.environ, "CARGO_TARGET_DIR": str(env_target)}
    # The base checkout has no target/occt of its own: build it against this tree's OCCT.
    local_occt = Path.cwd().resolve() / "target" / "occt" / "install"
    if "OCCT_ROOT" not in env and local_occt.is_dir():
        env["OCCT_ROOT"] = str(local_occt)
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env=env)


def changed_crates(base: str) -> list[str]:
    names = subprocess.run(
        ["git", "diff", "--name-only", base, "--", "crates/"],
        check=True, capture_output=True, text=True,
    ).stdout.split()
    crates = sorted({n.split("/")[1] for n in names if n.count("/") >= 2})
    return [c for c in crates if (Path("crates") / c / "Cargo.toml").exists()]


def public_api(crate: str, root: Path, target: Path) -> set[str] | None:
    """The crate's public items, or None when the crate does not exist in that tree."""
    if not (root / "crates" / crate / "Cargo.toml").exists():
        return None
    # -sss leaves out blanket, auto-trait, and derived implementations.
    result = run(["cargo", "public-api", "-sss", "-p", crate], root, target)
    if result.returncode != 0:
        raise RuntimeError(f"cargo public-api -p {crate} failed in {root}:\n{result.stderr}")
    return {line.strip() for line in result.stdout.splitlines() if line.strip()}


# Methods of standard traits: implementing one is not an API decision a spec names.
STD_TRAIT_METHODS = {
    "add", "as_mut", "as_ref", "borrow", "clone", "cmp", "default", "deref", "deref_mut",
    "div", "drop", "eq", "fmt", "from", "from_str", "hash", "into", "into_iter", "mul",
    "ne", "neg", "next", "partial_cmp", "sub", "Output", "Err", "Item", "Target", "try_from", "try_into",
}


def item_name(line: str) -> str | None:
    """The last path segment of the item a line declares, e.g. `export_step`, or None for
    trait implementations and standard trait methods, which are not checked."""
    if line.startswith("impl"):
        return None
    match = re.search(r"([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+)", line)
    if not match:
        return None
    name = match[1].split("::")[-1]
    return None if name in STD_TRAIT_METHODS else name


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    base = sys.argv[1]
    root = Path.cwd().resolve()
    specs = sorted(Path("tasks/active").glob("*.md"))
    spec_text = "\n".join(p.read_text() for p in specs)
    crates = changed_crates(base)
    if not crates:
        print("no crates changed")
        return 0

    target = root / "target" / "gates" / "public-api"
    work = Path(tempfile.mkdtemp(prefix="api-base-"))
    failed = False
    try:
        subprocess.run(["git", "worktree", "add", "--detach", str(work), base],
                       check=True, capture_output=True)
        for crate in crates:
            # Separate target directories: cargo does not tell the two copies of a
            # workspace crate apart, and would mix their rustdoc output.
            before = public_api(crate, work, target / "base") or set()
            after = public_api(crate, root, target / "head") or set()
            added, removed = sorted(after - before), sorted(before - after)
            if not added and not removed:
                print(f"{crate}: public API unchanged")
                continue
            print(f"{crate}: {len(added)} added, {len(removed)} removed")
            for sign, items in (("+", added), ("-", removed)):
                for line in items:
                    name = item_name(line)
                    if name is None:
                        verdict = "(trait impl, not checked)"
                    elif not specs:
                        verdict = "(no active task)"
                    elif re.search(rf"\b{re.escape(name)}\b", spec_text):
                        verdict = "in spec"
                    else:
                        verdict = "NOT IN SPEC"
                        failed = True
                    print(f"  {sign} {line}  {verdict}")
    finally:
        subprocess.run(["git", "worktree", "remove", "--force", str(work)], capture_output=True)
        shutil.rmtree(work, ignore_errors=True)
    if failed:
        names = ", ".join(str(p) for p in specs)
        print(f"public API changes not named in {names}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
