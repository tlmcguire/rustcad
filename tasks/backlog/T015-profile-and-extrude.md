# T015 Profiles and extrude in the trait

**Crate:** `cad-kernel` (trait), `cad-testkit` (reference kernel).
**Goal:** The first sketch-driven operation. A profile is a closed planar loop the kernel can sweep.
**Public API:** `Profile { plane: Plane, outer: Loop, holes: Vec<Loop> }`, `Loop` as a list of `Segment::Line` and `Segment::Arc` in plane coordinates. `Kernel::extrude(&Profile, distance: f64) -> KResult<ShapeId>` along the plane normal. `FaceRole` gains `Start`, `End`, and `Side(segment index)`, reported through `topology`.
**Compatibility:** New trait methods get default bodies returning `Unsupported`, so other implementations build unchanged until their own task.
**Acceptance:**
1. `Profile::new` rejects an open loop, a self-intersecting loop, and a zero-length segment, with typed errors.
2. Reference kernel: an axis-aligned rectangle on a standard plane extruded by a positive distance equals the matching `make_box` (golden volume, area, centre). Anything else returns `Unsupported`.
3. A negative distance extrudes against the normal. Zero is `InvalidInput`.
4. Side roles name the segment index of the outer loop, so naming can build `Extrude / side / from segment n`.
**Out of scope:** OCCT (T016), draft, two-sided and symmetric extrude, extrude to a face.
**Dependencies:** none.
**Depends on:** T011, T013.
