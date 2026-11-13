# T001 Reference kernel for analytic boxes

**Crate:** `cad-testkit`
**Goal:** A tiny `Kernel` implementation that handles axis-aligned boxes only. It gives the golden harness and higher layers something to run against before OCCT lands.
**Acceptance:**
1. `make_box` rejects non-positive sizes with `InvalidInput`.
2. `mass_props` of a 40 x 20 x 10 box returns volume 8000, area 2800, centre (20, 10, 5).
3. `face_count` returns 6. `tessellate` returns 12 triangles, each tagged with a face.
4. `boolean` returns `Unsupported`.
5. A released handle returns `UnknownShape`.
**Out of scope:** any non-box geometry.
**Needs:** `cad-testkit` to depend on `cad-kernel` directly.
