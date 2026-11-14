# T023 Document and feature model

**Crate:** `cad-doc`
**Goal:** The source of truth (D4, PLAN 8.1, 8.2): an immutable document snapshot holding parameters and an ordered list of features.
**Public API:** `Document` (cheap to clone, structurally shared via `Arc`), `FeatureId` (permanent, never reused; replaces the T006 placeholder), `Feature { id, name, kind: FeatureKind, suppressed }`, `FeatureKind::{Box, Sketch, Extrude, Fillet, Chamfer, Boolean}` with typed arguments whose numbers are expression strings, `Document::dependencies(FeatureId) -> Vec<FeatureId>`.
**Acceptance:**
1. Feature IDs are never reused after deletion, including across clone.
2. Dependencies are computed from references (parameters by name, geometry by stable ID, sketches by feature ID), never from list order. A cycle is rejected when the feature is inserted.
3. Moving a feature above one it depends on is rejected.
4. Editing a clone does not change the original.
5. Cached results and status live outside the document (geometry is a cache, rule 5).
**Out of scope:** transactions and undo (T025), rebuild (T026), file format.
**Dependencies:** none.
**Depends on:** T012, T019.
