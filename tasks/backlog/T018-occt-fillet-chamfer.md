# T018 OCCT fillet and chamfer with edge history

**Crate:** `cad-kernel-occt`
**Goal:** Implement T017 on OCCT, and edge history for `boolean`.
**Acceptance:**
1. Box 40 x 20 x 10, fillet one vertical edge r = 2: volume `8000 - (4 - π) * 10` within golden tolerance; one generated face.
2. Fillet all 12 edges r = 1: face count 26.
3. Chamfer one edge d = 2: volume `8000 - 2 * 10`; one generated face.
4. Radius larger than the adjacent faces allow returns `OperationFailed`, not a crash, in process and through the worker.
5. Edge history: every input edge not touched is reported `Modified` or unchanged with its output index; the filleted edge is reported deleted.
6. Boolean now fills edge history too; the T003 pocket case lists the edges it generated.
**Out of scope:** shell and offset.
**Dependencies:** links TKFillet.
**Depends on:** T017, T014.
