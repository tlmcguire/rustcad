# T006 report: topological naming spike

Date: 2026-11-13. Kernel: OCCT 7.9.3 through `cad-kernel-occt`. Code: `cad_doc::naming`.
Tests: 10 unit tests in `cad-doc`, 9 scenario tests in `crates/cad-testkit/tests/naming.rs`.

## Result

The core of section 8.4 holds: IDs derived from feature and role, carried through history
maps, survive parameter changes and kernel reordering. One rule in it is unsafe as written:
numbering split children by position (rule 3) produces a silent rebind when the number of
pieces changes. Section 8.4 should change before the document model is built on it.

| Acceptance point | Status | Evidence |
|---|---|---|
| 1. Stable face IDs from creating feature and role, propagated through `HistoryMap` | Met | `ids_come_from_the_creating_feature_and_role`; unit tests on `propagate` |
| 2. Box, pocket, reference a face, change the box size: reference resolves to the same face | Met | `references_survive_changes_to_the_box_size`: all 11 faces, four sizes |
| 3. Split face: deterministic child IDs; parent reference reports candidates, no silent rebind | Met for this scenario | `a_split_face_gets_ordered_children_...`, `child_ids_are_the_same_on_every_rebuild_...` |
| 4. Report | This file | |

Scope: faces only, planar faces only, boxes and booleans only. That is all the Phase 0
`Kernel` trait can make.

## What was built

- `StableFaceId`: creating feature, role in that feature, then a list of splits.
  `F1/box:+z` is the top of the box made by feature 1. `F1/box:+z/split@F3#1` is the second
  piece of it after feature 3 split it. `F3/new#0` is a face feature 3 created from nothing.
- Box roles come from the direction each face points, measured from a tessellation, not
  from the kernel's face order.
- `propagate` applies a `HistoryMap`: a modified face keeps its ID, split children get the
  parent ID plus a child number, faces with no ancestor get the operation's ID.
- `FaceRef` stores the ID plus a description (normal and a point on the face).
- `resolve` returns `Exact`, `Rebound` with a reason, `Ambiguous` with candidates, or
  `Missing`. Only `Exact` is quiet.

## Cases that work

1. **Parameter changes that keep the topology.** Resizing the box under a pocket: every
   reference resolves exactly, and the IDs sit on the faces a person would mean (the +X face
   is at the new length, the pocket floor has not moved).
2. **Tool faces keep the tool's name.** The pocket floor is `F2/box:-z`, through one cut or
   two. It faces up in the result; the ID records where it came from, not which way it points.
3. **Split faces.** A slot through the box splits four faces in two. Children are numbered
   along X, the same on every rebuild in a fresh kernel, and stay attached to the right
   pieces when the box is lengthened.
4. **Reference to a face that has since been split.** Both pieces are offered as
   candidates. Neither is chosen.
5. **A split that stops happening.** A reference to one piece, after the slot is shortened
   into a pocket, is rebound to the whole face and flagged `SplitUndone`.
6. **A deleted face with a lookalike elsewhere.** When a pocket wall is removed, the only
   other face with that normal is 30 mm away. It is offered, not chosen. This needed a
   stricter rule than the plan's "nearest to point P"; see change 3.
7. **Pieces with the same centre.** A groove cut as a rectangular frame leaves an island
   inside a ring, both centred on the same point. Area breaks the tie.
8. **Ties are errors.** If two pieces cannot be ordered by position or area, naming fails
   with `IndistinguishableFaces` instead of falling back to kernel order.

## Cases that do not work

1. **Child numbers shift when the piece count changes. This is a silent rebind.**
   One cut with a two-slab tool splits the top into three pieces: `#0`, `#1`, `#2`. Move the
   first slab off the part and there are two pieces, numbered `#0` and `#1`. A reference to
   the old middle piece, which spanned x 12 to 25, now resolves `Exact` to a face spanning
   x 27 to 40. The ID exists, so nothing is flagged.
   `known_limitation_child_numbers_shift_when_the_piece_count_changes` asserts this
   behaviour so that fixing it is a visible change.
   The same edit made as two cut features in a row is safe, because each split is recorded
   against its own feature
   (`sequential_cuts_do_not_rebind_when_an_earlier_split_disappears`).
2. **Roles are recovered from geometry.** Works for an axis-aligned box. A rotated box, or
   any primitive whose faces cannot be told apart by direction, cannot be named this way.
3. **Merged faces.** When two input faces become one output face, the first keeps the name
   and the second is recorded as merged; a reference to it is offered the merged face. Only
   exercised with a synthetic history. OCCT does not merge coplanar faces unless asked.
4. **Description fallback is planar only.** Normal plus point says nothing useful about a
   cylinder or a fillet.
5. **Tolerance-based ordering is not a strict order.** "Equal within tolerance" is not
   transitive, so three nearly coincident pieces could sort inconsistently. Not observed.
6. **Not attempted:** edges and vertices, generated faces from a real kernel operation
   (booleans of boxes make none), sketch-derived roles, the fuzz test in the 8.4 test plan.

## Proposed changes to section 8.4

1. **Replace rule 3.** Do not number split children by position. Name each child by what
   bounds it: the set of stable IDs of the faces it shares an edge with. In the failing
   case the middle piece is "the piece of `F1/box:+z` between `F2/box:+x` and `F3/box:-x`";
   when the first slab leaves, no piece has that description, the ID is gone, and the
   reference falls through to candidates. Position stays as a last tiebreak only, and a tie
   stays an error. This needs edge adjacency from the kernel, which the trait does not have.
2. **Kernels should report roles.** `make_box` and later primitives and sweeps should return
   the role of each face they create, the way modifying operations return a history map.
   Recovering roles from geometry is a stopgap.
3. **Tighten rule 5's "confident match".** One match by normal is not enough. The spike
   requires the candidate's plane to pass through the described point. The plan should say
   what confident means per surface type.
4. **Say what a merged face is called.** Rule 2 covers modified and split. It should also
   cover two faces becoming one: first operand's ID wins, the other is recorded as an alias
   that resolves with a warning.
5. **Extend `HistoryMap` to edges and vertices** before any feature references them
   (fillet and chamfer do). Change 1 needs edge adjacency for the same reason.
6. **Prefer one split per feature where the UI can.** Sequential features gave stable
   names where a single multi-body tool did not. This is a mitigation, not a fix.

## Open items for the document model

- `FeatureId` is a bare number here. The real one must be permanent and never reused.
- `NamedFaces` is rebuilt from a tessellation at the default tolerance on every operation.
  That is fine for planar faces and too slow and too approximate for the real thing; face
  geometry should come from the kernel.
- Section 8.4's test plan (corpus plus fuzz) is still to do once there is more than one
  kind of feature.
