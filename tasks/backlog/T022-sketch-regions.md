# T022 Closed region detection

**Crate:** `cad-sketch`
**Goal:** Turn solved sketch geometry into profiles automatically (PLAN 7.4.4).
**Public API:** `Sketch::regions(&SolveResult, Tolerance) -> Vec<Region>`, where a `Region` has an outer loop and holes as lists of `(EntityId, reversed)`.
**Acceptance:**
1. A rectangle gives one region with no holes. A rectangle with a circle inside gives two: the ring, and the disc.
2. Two overlapping rectangles give three regions.
3. Construction entities never bound a region. A dangling line does not either.
4. Region order and loop direction (outer counter-clockwise, holes clockwise) are deterministic.
5. A conversion to `cad_kernel::Profile` is not in this crate (layers); the region holds enough to build one in `cad-eval`.
**Out of scope:** splines, self-intersecting curves beyond line-line, line-arc, and arc-arc.
**Dependencies:** none.
**Depends on:** T020.
