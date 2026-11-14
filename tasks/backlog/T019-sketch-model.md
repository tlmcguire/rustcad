# T019 Sketch entities and constraints as data

**Crate:** `cad-sketch`
**Goal:** The data model the solver works on (PLAN 7.1, 7.2, 8.4.6). No solving yet.
**Public API:** `Sketch` with `add_point / add_line / add_circle / add_arc -> EntityId`, `remove(EntityId)`, `add_constraint(Constraint) -> ConstraintId`, a `construction` flag per entity, and `Constraint` as an enum of every constraint in PLAN 7.2.
**Acceptance:**
1. Entity and constraint IDs are permanent: removing an entity never frees its ID for reuse, and adding after a removal gives a new ID.
2. Removing an entity removes the constraints that reference it and reports which.
3. A constraint that references a missing entity, or an entity of the wrong kind (radius on a line), is rejected with a typed error.
4. Lines and arcs refer to their endpoints by point ID, so coincidence is shared points or a constraint, never copied coordinates.
**Out of scope:** solving (T020), ellipses, splines, projected geometry, text.
**Dependencies:** none.
**Depends on:** T011.
