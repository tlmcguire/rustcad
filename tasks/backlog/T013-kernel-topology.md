# T013 Topology and face roles in the kernel trait

**Crate:** `cad-kernel` (trait), `cad-testkit` (reference kernel).
**Goal:** Recommendations 2 and 5 of the T006 report: the kernel reports face roles at creation and exposes edges with their adjacent faces, so naming no longer recovers them from a tessellation.
**Public API:** `EdgeIndex`, `Topology { faces: Vec<FaceInfo>, edges: Vec<EdgeInfo> }` where `FaceInfo` carries a surface kind (plane with point and normal, or other) and `EdgeInfo` carries its two adjacent faces and endpoints. `FaceInfo` also carries `role: Option<FaceRole>` for faces the shape's creating operation named; `FaceRole` and `BoxSide` move down from `cad-doc` to `cad-kernel`. `Kernel::topology(ShapeId) -> KResult<Topology>` is a new trait method with a default body returning `Unsupported`, so `OcctKernel` and the worker client still build unchanged. `make_box` keeps its signature.
**Acceptance:**
1. Reference kernel box: 6 planar faces, 12 edges, each edge adjacent to exactly two distinct faces, and each `BoxSide` reported on exactly one face.
2. `face_count` equals `topology().faces.len()`.
3. Faces and edges are listed in a deterministic order.
4. `cad-doc` naming uses the reported roles when `topology` succeeds and falls back to the T006 geometry path when it returns `Unsupported`; the T006 scenario tests pass unchanged against both kernels.
**Out of scope:** the OCCT side (T014), vertices, edge history (T017), curved surface parameters.
**Dependencies:** none.
**Depends on:** T006.
