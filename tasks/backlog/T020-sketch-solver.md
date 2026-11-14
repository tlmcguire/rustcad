# T020 Solver trait and a native solver for basic constraints

**Crate:** `cad-sketch`
**Goal:** Solve sketches (PLAN 7.3) with a Rust Newton or Levenberg-Marquardt solver behind a `Solver` trait.
**Public API:** `trait Solver { fn solve(&self, sketch: &Sketch, tol: Tolerance) -> SolveResult }`, `SolveResult { positions, dof: usize, status }`, `NativeSolver`.
**Acceptance:**
1. Constraints covered: coincident, horizontal, vertical, fix, distance, point on line.
2. A fully constrained rectangle (fix one corner, horizontal and vertical sides, two distances) solves to the exact corners and reports dof 0.
3. An unconstrained line reports dof 4. Each added independent constraint lowers it by one.
4. The solution keeps unconstrained entities near their prior positions (minimum change from the start state).
5. Deterministic: same sketch, same positions bit for bit.
6. Non-convergence returns a status, not a panic, within a fixed iteration cap.
**Out of scope:** the remaining constraints and conflict reporting (T021), planegcs (see backlog README).
**Dependencies:** none. Dense linear algebra written in-crate; if that proves too slow, a new task justifies a crate.
**Depends on:** T019.
