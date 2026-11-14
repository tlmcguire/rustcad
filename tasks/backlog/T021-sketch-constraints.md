# T021 Remaining constraints and conflict reporting

**Crate:** `cad-sketch`
**Goal:** Cover all of PLAN 7.2 and name conflicting constraints (PLAN 7.4.3).
**Acceptance:**
1. Adds parallel, perpendicular, tangent (line-arc, arc-arc), equal, concentric, midpoint, symmetric, point on circle, angle, radius, diameter. Each has a test that solves a small sketch to known coordinates.
2. Over-constrained: a square with a fifth, contradictory distance returns `status: Conflict` with a list of constraint IDs that contains the contradictory one and is minimal (removing any one makes it solvable).
3. Redundant but consistent constraints (two horizontals on one line) are reported as redundant, not as a conflict.
4. A dof-0 sketch stays dof 0 after one more redundant constraint.
**Out of scope:** dragging, ellipse and spline constraints.
**Dependencies:** none.
**Depends on:** T020.
