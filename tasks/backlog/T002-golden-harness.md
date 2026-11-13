# T002 Golden geometry harness

**Crate:** `cad-testkit`
**Goal:** Load golden files and compare kernel results within tolerance.
**Acceptance:**
1. A golden file records expected volume, area, bounding box, face count, a tolerance, and how the values were established.
2. `assert_golden(kernel, shape, path)` fails with a message naming each mismatched field.
3. The harness runs the same case against any `&dyn Kernel`.
4. One analytic golden case for the T001 box passes.
**Decision needed first:** golden file format and whether to add a serialization dependency.
**Depends on:** T001.
