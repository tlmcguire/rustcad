# T005 Viewport and picking spike

**Crate:** `cad-render`
**Goal:** Prove wgpu rendering and ID-buffer picking, including offscreen.
**Acceptance:**
1. Renders a `Mesh` with shaded faces and edges to a window and to a PNG.
2. Picking at a pixel returns the `FaceIndex` under it.
3. Offscreen rendering works in CI without a display.
4. Report: wgpu version, license check, frame time for 1M triangles.
**Depends on:** T001.
