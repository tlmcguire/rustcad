# T016 OCCT extrude

**Crate:** `cad-kernel-occt`
**Goal:** Implement `extrude` from T015 on OCCT.
**Acceptance:**
1. Rectangle 40 x 20 by 10 matches `box_40x20x10.golden`.
2. A circle of radius 5 (two arcs) by 10: volume 250π within the golden tolerance, 3 faces.
3. A rectangle with a circular hole: volume and face count checked by a new golden.
4. A profile on a tilted plane gives the same volume as on `Plane::xy()`.
5. Roles: each side face reports the segment it came from; start and end faces report their roles.
6. Works through the worker.
**Out of scope:** splines, draft.
**Dependencies:** none.
**Depends on:** T015, T014.
