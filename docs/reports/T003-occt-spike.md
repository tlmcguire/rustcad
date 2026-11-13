# T003 report: OCCT build and minimal binding spike

Date: 2026-11-13. Machine: Apple M2 (8 cores), macOS 27.0.1, Apple clang 21.

## Result

OCCT builds from pinned source and is callable from Rust on macOS arm64. The Phase 0
`Kernel` trait is implemented, booleans return a face history taken from OCCT's history API,
and the golden cases pass. Linux and Windows were not built or run.

| Acceptance point | Status | Evidence |
|---|---|---|
| 1. Phase 0 `Kernel` trait: box, boolean, face count, tessellate, mass properties | Met | `OcctKernel` in `crates/cad-kernel-occt`. 11 tests in `crates/cad-testkit/tests/occt.rs`. |
| 2. `boolean` fills `HistoryMap` from OCCT's history API | Met | Pocket cut and through-slot tests check every face's fate and that every output face is accounted for. |
| 3. Passes the T002 golden cases plus box-minus-box | Met | `box_40x20x10.golden` and the new `box_40x20x10_minus_pocket.golden` pass with OCCT. |
| 4. Report | This file | |
| Goal: "on all three platforms" | macOS only | See "Open items", 1. |

## Pinned version

OCCT 7.9.3, tag `V7_9_3`, commit `a016080bf6738d6aeae020badee4e888ad1540a5`. The recipe is
`crates/cad-kernel-occt/native/build-occt.sh`; it refuses any other commit. 8.0.1 exists; 7.9
was chosen as the last release of the long-established 7.x line. Moving to 8.x is a separate
decision.

## Build time and size

`build-occt.sh` builds three modules only (FoundationClasses, ModelingData,
ModelingAlgorithms), Release, shared libraries, no optional third-party libraries.

| Measurement | Value |
|---|---|
| Configure | 3 s |
| Build and install, 2,132 steps, 8 cores | 319 s |
| Shallow source checkout | 351 MB |
| Build tree | 130 MB |
| Install prefix | 57 MB (31 MB libraries, 21 MB headers) |
| Shared libraries the backend needs at run time | 12 libraries, 22.8 MB |
| All 18 libraries the recipe builds | 32.8 MB |
| Compiled shim (`libcad_occt_shim.a`, debug) | 1.0 MB |
| Compile the shim | a few seconds |

The 12 run-time libraries are TKernel, TKMath, TKG2d, TKG3d, TKGeomBase, TKBRep, TKGeomAlgo,
TKTopAlgo, TKPrim, TKShHealing, TKBO, TKMesh. Six built libraries are not needed yet (TKBool,
TKFillet, TKOffset, TKFeat, TKHLR, TKXMesh); fillet, shell, and offset will want the first four.

## Binding approach: own shim

A hand-written C++ file (`native/shim.cpp`, about 330 lines) exposes a C interface, compiled
by the `cc` crate from `build.rs`. Rust declares the functions in one module, `ffi.rs`, which
is the only place `unsafe` appears and exports only safe functions.

Why not opencascade-rs:

- `opencascade-sys` 0.3.0 and `occt-sys` 7.8.1 are themselves licensed LGPL-2.1, which a
  MIT OR Apache-2.0 crate cannot depend on under D2 and `deny.toml`.
- `occt-sys` describes itself as a static build of OCCT. D2 requires dynamic linking.
- It tracks OCCT 7.8.1, not the version pinned here.

These are from crate metadata. The code of opencascade-rs was not evaluated.

Why not `cxx` (PLAN 6.3, item 1): the interface is 14 functions over three opaque types and
flat arrays. A C interface needs one build dependency (`cc`) and no generated code, and the
boundary can be read in one sitting. If the interface grows to pass rich C++ types, revisit.

Rules the shim follows: no exception crosses the boundary; failures return null and a
message; every returned object has one owner and one free function; face indices follow
`TopExp::MapShapes` order, zero-based.

## Thread safety

OCCT is not thread safe. `ffi.rs` has one process-wide lock, and every safe wrapper takes a
guard for it as an argument, so calling OCCT without the lock does not compile. `OcctKernel`
is `Send + Sync` and serialises all calls. Multi-core rebuild needs the worker processes of
T004.

## History

`BRepAlgoAPI_BooleanOperation` gives `Modified`, `IsDeleted`, and `Generated` per input face.
The shim maps each input face of both operands to its output faces:

- not deleted and nothing in `Modified`: the face passes through unchanged, found in the
  output by identity (`Modified`, one result);
- one or more in `Modified`: `Modified` or `Split` by count;
- deleted, or no surviving output: `Deleted`.

Output faces with no ancestor are reported as generated. For booleans of boxes there are none.

Observed: box minus pocket gives 6 + 5 faces with one tool face deleted; a through-slot splits
four box faces in two each. History is identical across repeat runs and across kernel
instances, which T006 depends on.

## License check

- OCCT is LGPL-2.1 with the Open CASCADE exception 1.0 (`LICENSE_LGPL_21.txt` and
  `OCCT_LGPL_EXCEPTION.txt` in the source tree). The exception allows object code that
  incorporates material from OCCT headers to be distributed under terms of our choice, given
  prominent notice that it uses OCCT. The shim and crate docs carry that notice.
- OCCT is linked dynamically: the test binary references 11 `libTK*.dylib` files. No OCCT
  code is in the repository; the recipe fetches it.
- New Rust build dependency: `cc` 1.11.2 with `shlex` and `find-msvc-tools`, all
  MIT OR Apache-2.0. `cargo deny check` passes.
- Recorded in `NATIVE_DEPS.md` as PLAN 4.4 requires.

This is a reading of the license files, not legal advice.

## Open items

1. **Linux and Windows are untested.** The recipe is plain CMake and Ninja, and `build.rs`
   has an MSVC branch, but neither has run there. On Linux the test binaries will also need
   the library directory on `LD_LIBRARY_PATH` or an rpath: Cargo does not pass link
   arguments from a library crate to the binaries that use it. On macOS this is avoided by
   building OCCT with absolute install names.
2. **OCCT is optional in the gates.** Without an install the crate builds with no backend
   and the tests print `SKIPPED` and pass. `OCCT_REQUIRED=1` turns that into a failure. CI
   should set it once it builds OCCT; `.github/` is a protected path.
3. **Shipping.** Absolute install names are right for development and wrong for a
   distributed app, which needs `@rpath` and the libraries next to the binary (PLAN 6.3,
   item 5).
4. **`Kernel::tessellate` takes the wrong parameter.** It receives the comparison
   `Tolerance`. The backend uses its linear value as the chord deflection and a fixed 0.5
   rad angular deflection. With the default 1e-6 mm that is harmless for planar faces and
   would produce enormous meshes for curved ones. The trait needs a mesh quality argument
   before curved geometry lands.
5. **Coplanar faces are not merged.** A union of two boxes that share a face plane keeps the
   seam. `SimplifyResult` exists but changes history; decide in T006.
6. **Edges and vertices have no history yet.** The trait only models faces.
7. **Crashes are not contained.** A fault inside OCCT takes the process down. That is T004.
8. **PLAN 17.3 items 1 and 9** are answered for this scope: the license terms are as above,
   and the history API covers the one modifying operation in the Phase 0 trait. Fillet,
   shell, and offset use different builder classes and need their own check.
