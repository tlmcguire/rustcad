# Backlog

Drafted specs for the end of Phase 0 and the start of Phase 1, in dependency order. Nothing here is active. Move a spec to `tasks/active/` to start it.

| Task | Crate | Depends on |
|---|---|---|
| T008 BRep and STEP round trip (done) | cad-kernel, cad-kernel-occt, cad-testkit | T002, T003, T004 |
| T009 Units and quantities (done) | cad-math | |
| T010 Expressions (started, paused; code in git stash) | cad-math | T009 |
| T011 Planes and axes | cad-math | |
| T012 Parameter table | cad-doc | T010 |
| T013 Kernel topology and roles | cad-kernel, cad-testkit | T006 |
| T014 OCCT topology and roles | cad-kernel-occt | T013 |
| T015 Profiles and extrude | cad-kernel, cad-testkit | T011, T013 |
| T016 OCCT extrude | cad-kernel-occt | T014, T015 |
| T017 Edge history, fillet, chamfer | cad-kernel, cad-testkit | T013 |
| T018 OCCT fillet and chamfer | cad-kernel-occt | T014, T017 |
| T019 Sketch model | cad-sketch | T011 |
| T020 Sketch solver | cad-sketch | T019 |
| T021 Remaining constraints | cad-sketch | T020 |
| T022 Region detection | cad-sketch | T020 |
| T023 Document model | cad-doc | T012, T019 |
| T024 Naming revision | cad-doc | T013, T014 |
| T025 Transactions and undo | cad-doc | T023 |
| T026 Rebuild engine | cad-eval | T016, T018, T022, T023, T024 |
| T027 Command registry | cad-commands | T025, T026 |

T008 and T009 are done. T010 was started and paused for Phase 0 (its code is in `git stash`). Also ready: T011, T013.

## Open questions for the owner

1. **Solver backend.** PLAN 7.3 says start with planegcs. That needs C++ FFI in `cad-sketch`, but rule 2 allows `unsafe` only in `cad-kernel-occt`, and a new native dependency needs a task. T020 starts with the native Rust solver instead. Using planegcs would need a new decision record.
2. **Thread pool.** Parallel rebuild (PLAN 5.4, 8.5.2) needs `rayon` or similar. Not in these specs. Needs a dependency task.
3. **Serialization.** Command schemas for MCP, and the native file format, need either serde or a D11-style hand-written format. Not decided. T027 leaves it out.
4. **Next specs.** Headless CLI (`cad-cli`) is next after T027, then the native file format and STEP import into the document through `cad-io`.
