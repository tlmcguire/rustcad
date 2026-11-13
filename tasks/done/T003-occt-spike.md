# T003 OCCT build and minimal binding spike

**Crate:** `cad-kernel-occt`
**Goal:** Prove OCCT can be built from pinned source and called from Rust on all three platforms.
**Acceptance:**
1. Implements the Phase 0 `Kernel` trait: box, boolean, face count, tessellate, mass properties.
2. `boolean` fills `HistoryMap` from OCCT's history API.
3. Passes the T002 golden cases plus box-minus-box.
4. Written report: build time, binary size, binding approach (own shim versus opencascade-rs), license check.
**Depends on:** T002.
