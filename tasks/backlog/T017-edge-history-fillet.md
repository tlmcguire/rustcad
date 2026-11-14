# T017 Edge history, fillet, and chamfer in the trait

**Crate:** `cad-kernel` (trait), `cad-testkit` (reference kernel).
**Goal:** Fillet and chamfer reference edges, so the history map must cover edges first (T006 report, recommendation 5).
**Public API:** `HistoryMap` gains `edges: Vec<EdgeHistory>` and `generated_edges`. `Kernel::fillet(ShapeId, &[EdgeIndex], radius) -> KResult<OpResult>`, `Kernel::chamfer(ShapeId, &[EdgeIndex], distance) -> KResult<OpResult>`.
**Compatibility:** New trait methods get default bodies returning `Unsupported`, so other implementations build unchanged until their own task.
**Acceptance:**
1. Reference kernel returns `Unsupported` for fillet and chamfer, and fills edge history for nothing (it has no modifying operations).
2. An empty edge list, a non-positive radius, or an edge index out of range is `InvalidInput`, checked in a shared validation helper every backend calls.
3. `OcctKernel::boolean` fills `edges` with an empty list until T018. Existing face-history tests and the T006 scenarios pass unchanged.
**Out of scope:** OCCT (T018), variable radius, setback.
**Dependencies:** none.
**Depends on:** T013.
