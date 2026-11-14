# T028 Vec3 distance and midpoint

**Crate:** `cad-math`
**Goal:** Two small helpers that later tasks (sketch midpoint constraints, naming fallbacks) would otherwise write inline. Also the first task run end to end by the loop (Phase 0 exit criterion).
**Public API:** `Vec3::distance(self, other: Vec3) -> f64`, `Vec3::midpoint(self, other: Vec3) -> Vec3`.
**Acceptance:**
1. `distance` from (0, 0, 0) to (3, 4, 0) is 5 within `Tolerance`, and from a point to itself is 0.
2. `distance` is symmetric: `a.distance(b)` equals `b.distance(a)` within `Tolerance` for a few non-trivial points.
3. `midpoint` of (0, 0, 0) and (2, 4, -6) is (1, 2, -3), and `a.midpoint(a)` is `a`.
4. `midpoint` is symmetric, and the midpoint is at the same distance from both points within `Tolerance`.
**Out of scope:** any other `Vec3` change, interpolation with a parameter, other types.
**Size:** under 40 lines of implementation.
**Dependencies:** none.
**Depends on:** none.
