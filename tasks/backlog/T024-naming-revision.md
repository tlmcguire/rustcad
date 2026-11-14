# T024 Naming revision from the T006 report

**Crate:** `cad-doc`, plus a section 8.4 update in `docs/PLAN.md`.
**Goal:** Apply recommendations 1, 3, and 4 of the T006 report.
**Acceptance:**
1. Split children are named by the stable IDs of the faces they share an edge with (from T013 adjacency). Position is only a final tiebreak, and a tie stays an error.
2. The T006 failing case (first slab leaves) now falls through to candidates instead of rebinding to the wrong piece.
3. A confident match for a planar face requires matching normal and the described point lying on the plane.
4. Two faces merged into one: the first operand's ID wins and the other becomes an alias that resolves with a warning.
5. `docs/PLAN.md` section 8.4 rules 2, 3, and 5 are rewritten to match.
6. A corpus test runs every scenario from T006 plus the new ones.
**Out of scope:** edge naming, fuzzing.
**Dependencies:** none.
**Depends on:** T013, T014.
