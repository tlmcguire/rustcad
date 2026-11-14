# T014 OCCT topology and face roles

**Crate:** `cad-kernel-occt`
**Goal:** Implement `topology` from T013 on OCCT, in process and through the worker, replacing the default `Unsupported`.
**Acceptance:**
1. OCCT box: same counts and adjacency as the reference kernel, and the same role for the face at each side.
2. Box minus pocket: every edge has two adjacent faces; planar faces report point and normal matching `measure_faces` within `Tolerance`.
3. Face and edge order is stable across two runs and across a worker restart.
4. The worker protocol carries `Topology`, roles included.
5. With OCCT reporting roles, the T006 scenarios no longer take the geometry fallback; a test asserts this.
**Out of scope:** curved surfaces beyond reporting `Other`.
**Dependencies:** none.
**Depends on:** T013.
