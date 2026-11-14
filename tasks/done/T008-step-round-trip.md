# T008 BRep and STEP exchange with a golden round trip

**Crate:** `cad-kernel` (trait), `cad-kernel-occt` (implementation), `cad-testkit` (golden test).
**Goal:** Meet the Phase 0 exit criterion that the golden harness runs a STEP round trip in CI.
**Public API:** on `Kernel`: `to_brep_bytes(ShapeId) -> KResult<Vec<u8>>`, `from_brep_bytes(&[u8]) -> KResult<ShapeId>`, `export_step(ShapeId) -> KResult<Vec<u8>>`, `import_step(&[u8]) -> KResult<ShapeId>`.
**Compatibility:** New trait methods get default bodies returning `Unsupported`, so other implementations build unchanged until their own task.
**Acceptance:**
1. `ReferenceKernel` returns `Unsupported` for all four.
2. OCCT: the T003 box minus pocket, exported to STEP and imported again, passes `box_40x20x10_minus_pocket.golden` (volume, area, centre, face count).
3. The same holds for the BRep bytes round trip, and BRep bytes of the same shape are identical across two runs.
4. Garbage input to either import returns `InvalidInput` or `OperationFailed`, never a crash. Through the worker, a crash surfaces as `BackendLost`.
5. Export and import work through the T004 worker protocol, not only in process.
**Out of scope:** healing imported shapes (PLAN 6.6.3), STL and 3MF, assemblies or colours in STEP, file paths (bytes in, bytes out).
**Dependencies:** no new third-party crates. Links the OCCT data exchange toolkits (TKDESTEP or the equivalent for the pinned version).
**Depends on:** T003, T004, T002.
