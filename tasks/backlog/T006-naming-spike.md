# T006 Topological naming spike

**Crate:** `cad-doc`
**Goal:** Validate the stable ID design in `docs/PLAN.md`, section 8.4, before the document model is built on it.
**Acceptance:**
1. Stable face IDs derived from creating feature and role, propagated through `HistoryMap`.
2. Scenario: box, cut a pocket, reference a face, change the box size. The reference still resolves to the same face.
3. Scenario: a face is split by a cut. Children get deterministic IDs. A reference to the parent reports candidates, with no silent rebinding.
4. Report: cases that work, cases that do not, and proposed changes to section 8.4.
**Depends on:** T003.
