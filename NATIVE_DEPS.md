# Native dependencies

One entry per native library: source, version, license, and how it is linked
(`docs/PLAN.md`, section 4.4).

| Library | Version | Source | License | Linking | Used by |
|---|---|---|---|---|---|
| OpenCascade Technology (OCCT) | 7.9.3, tag `V7_9_3`, commit `a016080bf6738d6aeae020badee4e888ad1540a5` | https://github.com/Open-Cascade-SAS/OCCT | LGPL-2.1 with Open CASCADE exception 1.0 | Dynamic (shared libraries) | `cad-kernel-occt` |

OCCT is not vendored. `crates/cad-kernel-occt/native/build-occt.sh` fetches and builds the
pinned commit. This software uses facilities provided by the Open CASCADE Technology software.
