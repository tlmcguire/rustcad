# Phase 0: items to verify (PLAN 17.3)

Checked on 2026-11-13. Each item says what was checked, how, and what it means for the plan.
"Verified" means checked against the source itself (the licence file, the header, the crates.io
registry entry). "Secondary" means only secondary sources were found and the item should be
rechecked before a decision depends on it.

## 1. OCCT licence and dynamic linking — verified

Source: `LICENSE_LGPL_21.txt` and `OCCT_LGPL_EXCEPTION.txt` in the pinned checkout (7.9.3).

OCCT is LGPL-2.1 with the Open CASCADE exception 1.0. The exception lets object code that
includes material from OCCT headers (inline code, templates) be distributed under any terms,
provided the documentation gives prominent notice that it uses OCCT. The project is
GPL-3.0-or-later (D12); LGPL-2.1 is compatible with it in either direction of linking.

What the plan already does is enough: OCCT is linked dynamically (`build.rs`), users can
replace the shared libraries, and `NATIVE_DEPS.md`, `README.md`, and the shim carry the notice.
Keep the notice in any binary distribution's documentation.

## 2. truck, monstertruck, planegcs, SolveSpace — verified (planegcs and SolveSpace partly)

| Project | Licence | Source | Fits GPL-3.0? |
|---|---|---|---|
| truck (`truck-modeling` 0.6, `truck-shapeops` 0.4) | Apache-2.0 | crates.io | Yes |
| monstertruck 0.4.2 | Apache-2.0 | crates.io | Yes |
| planegcs (FreeCAD Sketcher solver) | LGPL-2.1-or-later | FreeCAD licence page; the `planegcs` Python package | Yes, if linked as a separate library |
| SolveSpace solver (libslvs) | GPLv3 (official page); GPL-3.0-or-later per Wikipedia and wrappers | solvespace.com library page | Yes since D12 |

Not verified: the SPDX headers of the planegcs files in FreeCAD's own repository, and whether
planegcs builds outside FreeCAD without pulling in more of it (it depends on Eigen and Boost).
Check both before choosing it (see the backlog's open question 1).

## 3. wgpu, egui, iced, Slint — verified

| Crate | Latest | Licence | MSRV |
|---|---|---|---|
| wgpu | 30.0.2 | MIT OR Apache-2.0 | 1.87 |
| egui / eframe | 0.37.0 | MIT OR Apache-2.0 | 1.92 |
| iced | 0.14.0 | MIT | 1.88 |
| slint | 1.18.1 | GPL-3.0-only OR royalty-free OR commercial | 1.92 |

All fit GPL-3.0-or-later. Slint's GPL option is GPL-3.0-*only*, which would stop the project
from ever moving to a later GPL version while it depends on Slint; its royalty-free licence has
conditions of its own. egui (PLAN 9.4) has no such issue. The workspace's `cad-render` already
builds on wgpu.

## 4. wasmtime and the component model — secondary

wasmtime 45.0.3 (51.0.0-rc.1 in pre-release) is Apache-2.0 WITH LLVM-exception, MSRV 1.93
(crates.io, verified). WASI 0.2, the first release defined in WIT on the component model, has
been stable since January 2024 and is supported by wasmtime; wasi.dev lists 0.3 as its
successor, with runtimes that support 0.3 still running 0.2 components. wit-bindgen generates
Rust bindings. These statements come from wasi.dev and secondary write-ups. Recheck the
component model docs when the plugin host task (D6) is specified.

## 5. The official Rust MCP SDK — verified (spec coverage secondary)

The official SDK is `rmcp`, in `modelcontextprotocol/rust-sdk`, latest 3.10.0, Apache-2.0,
MSRV 1.88 (crates.io). Its 1.5.0 release notes added protocol version 2025-11-25. Which features
of that version it covers in full was not checked. Fits GPL-3.0-or-later.

## 6. 3MF, DXF, glTF crates — verified

| Format | Crate | Latest | Licence |
|---|---|---|---|
| 3MF | `threemf` | 0.8.1 | 0BSD |
| 3MF | `lib3mf` (Rust port) | 0.1.7 | MIT |
| DXF | `dxf` | 0.6.2 | MIT |
| glTF | `gltf` | 1.4.1 | MIT OR Apache-2.0 |

All fit. Feature coverage (3MF materials and beam lattices, DXF entity types) was not checked.

## 7. FOSS 3D mouse support — secondary

Linux: spacenavd and libspnav (BSD-3-Clause; Rust binding `libspnav` 0.1, BSD-3-Clause) are the
mature open route. macOS and Windows: there is no FOSS driver; tools there either use
3Dconnexion's proprietary driver or read the device as raw HID. `hidapi` (2.6.7, MIT) makes the
HID route possible on all three platforms without the vendor driver, at the cost of mapping each
device model ourselves. Plan: libspnav on Linux, raw HID elsewhere, behind one input trait.

## 8. Mutation testing and API diff tooling — verified by use

Installed and wired into `scripts/gates.sh` in this phase:

| Tool | Version | Licence | Used for |
|---|---|---|---|
| cargo-mutants | 27.1.0 | MIT | Mutation sample on changed code (`--in-diff`, sharded to about 40 mutants) |
| cargo-public-api | 0.52.0 | MIT | Public API at the base and at HEAD; needs a nightly toolchain for rustdoc JSON |
| cargo-llvm-cov | 0.9.1 | Apache-2.0 OR MIT | Coverage of changed lines |
| cargo-semver-checks | 0.51.0 | Apache-2.0 OR MIT | Not used: semver rules mean little at version 0.0.0, and the gate needs "matches the spec", not "is compatible" |

## 9. OCCT history APIs for every Kernel operation — verified from headers

Checked in the OCCT 7.9.3 headers which algorithms override the `BRepBuilderAPI_MakeShape`
history queries:

| Kernel operation (PLAN 6.2) | OCCT algorithm | Modified | Generated | IsDeleted |
|---|---|---|---|---|
| boolean | `BRepAlgoAPI_BuilderAlgo` (Fuse, Cut, Common) | yes | yes | yes |
| fillet | `BRepFilletAPI_MakeFillet` | yes | yes | yes |
| chamfer | `BRepFilletAPI_MakeChamfer` | yes | yes | yes |
| shell, offset_face | `BRepOffsetAPI_MakeThickSolid`, `BRepOffsetAPI_MakeOffsetShape` | yes | yes | yes |
| extrude | `BRepPrimAPI_MakePrism` | — | yes | yes |
| revolve | `BRepPrimAPI_MakeRevol` | — | yes | yes |
| sweep | `BRepOffsetAPI_MakePipe`, `MakePipeShell` | — | yes | — |
| loft | `BRepOffsetAPI_ThruSections` | — | yes | — |

Every modifying operation can report where its faces came from. The sweeps (extrude, revolve,
sweep, loft) only generate faces from profile edges, plus caps reachable through `FirstShape`
and `LastShape`; they modify nothing, so "Generated" plus the cap queries is the whole history
they need, and it is enough for the roles in T015. Not yet tested by running them; T016 and T018
will.

## 10. FreeCAD headless as a CI oracle — secondary

`FreeCADCmd script.py` runs FreeCAD without a GUI, and the `Part` module reports `.Volume` for
shapes, which is what a cross-check needs. A community Docker image exists but appears dated.
FreeCAD itself uses OCCT, so it is not an independent check of our OCCT results, only of our
use of them. Recommendation: not a merge gate. If wanted, a weekly job that imports our STEP
exports into FreeCAD and compares volumes, after the 25 reference parts of Phase 1 exist.

## Sources

- OCCT licence files in the pinned source checkout, and the crates.io registry (`cargo info`).
- FreeCAD licence: https://wiki.freecad.org/License ; planegcs package: https://pypi.org/project/planegcs/0.1.0/
- SolveSpace library: https://solvespace.com/library.pl ; https://en.wikipedia.org/wiki/SolveSpace
- rmcp 1.5.0 release notes: https://newreleases.io/project/github/modelcontextprotocol/rust-sdk/release/rmcp-v1.5.0
- WASI releases: https://wasi.dev/releases ; https://bytecodealliance.org/articles/WASI-0.2
- 3D mouse: https://wiki.freecad.org/3Dconnexion_input_devices ; https://github.com/jakubandrysek/PySpaceMouse
- Headless FreeCAD: https://wiki.freecad.org/Headless_FreeCAD ; https://wiki.freecad.org/FreeCAD_Docker_CLI_mode
