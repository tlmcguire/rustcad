# T011 2D points, planes, and axes

**Crate:** `cad-math`
**Goal:** The geometric types that sketches and profiles need before they can be placed in 3D.
**Public API:** `Vec2`, `Axis { origin: Vec3, dir: Vec3 }`, `Plane { origin, x_dir, normal }` with `Plane::xy() / yz() / zx()`, `Plane::to_world(Vec2) -> Vec3`, `Plane::to_local(Vec3) -> Vec2`.
**Acceptance:**
1. `Plane::new` rejects a zero normal and an `x_dir` not perpendicular to the normal, with typed errors. It normalises both.
2. `to_local(to_world(p))` returns `p` within `Tolerance` for random points on non-axis-aligned planes.
3. The three standard planes are right-handed and match the conventions used by `cad-render`'s camera.
4. `Axis::new` rejects a zero direction.
**Out of scope:** transforms and matrices, curves.
**Dependencies:** none.
**Depends on:** none.
