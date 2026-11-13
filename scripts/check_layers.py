#!/usr/bin/env python3
"""Fails if a workspace crate depends on a crate in the same or a higher layer."""
import json, pathlib, subprocess, sys

root = pathlib.Path(__file__).resolve().parent.parent
layer = {}
for line in (root / "scripts" / "layers.txt").read_text().splitlines():
    line = line.split("#")[0].split()
    if line:
        for name in line[1:]:
            layer[name] = int(line[0])

meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=root))
errors = []
for pkg in meta["packages"]:
    name = pkg["name"]
    if name not in layer:
        errors.append(f"{name}: not listed in scripts/layers.txt")
        continue
    for dep in pkg["dependencies"]:
        d = dep["name"]
        if d in layer and layer[d] >= layer[name]:
            errors.append(f"{name} (layer {layer[name]}) may not depend on {d} (layer {layer[d]})")
if errors:
    print("\n".join(errors)); sys.exit(1)
print(f"layers ok: {len(meta['packages'])} crates")
