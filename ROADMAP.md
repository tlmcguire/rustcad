# Roadmap

Full phases and exit criteria: `docs/PLAN.md`, section 13.

## Phase 0: Foundation (current)

- [x] Workspace skeleton, all crates stubbed
- [x] Layer, unsafe, format, lint, and test gates (`scripts/gates.sh`)
- [x] `cargo-deny` policy
- [ ] CI workflow for Linux, macOS, Windows (was ticked, but no workflow was ever committed)
- [x] `cad-math`: `Vec3`, `Tolerance`
- [x] `cad-kernel`: Phase 0 `Kernel` trait and `HistoryMap` types
- [x] License: GPL-3.0-or-later, `LICENSE` added (D12)
- [x] Verify the items in `docs/PLAN.md`, section 17.3 (`docs/reports/phase0-verification.md`)
- [x] T001 Reference kernel for analytic boxes (test double)
- [x] T002 Golden geometry harness
- [x] T003 OCCT build and minimal binding spike
- [x] T004 Worker process spike with latency numbers
- [x] T005 wgpu viewport and ID-buffer picking spike
- [x] T006 Topological naming spike
- [x] T007 Snapshot harness: headless renders checked against reference images under `cargo test`
- [x] T008 BRep and STEP exchange with a golden round trip (passes locally; CI not yet run on a host)
- [x] Determinism, performance, coverage, mutation, and API-diff gates
- [x] Loop orchestrator (`scripts/loop.py`, roles in `loop/roles/`); first run of T028 shelved by a loop bug, second run merged unaided ($0.60)
- [x] T028 Vec3 distance and midpoint: first task run end to end by the loop
- [x] First task specs drafted (`tasks/backlog/`, T009 to T027)

Exit: all spikes have reports. A STEP round trip runs in CI. The loop completes one trivial task unaided.

## Renderer track (current priority, D14)

Built out before more Phase 1 modeling work. Scope: `docs/PLAN.md`, section 9.1.

- [x] Torture demos: dense meshes, depth range, coplanar faces, slivers, many bodies (PR #2)
- [x] Engine-style view modes and reworked torture demos (PR #4)
- [x] Per-triangle-plane depth: flush faces resolve exactly (PR #5)
- [x] Orbit, pan, and zoom-to-cursor in every windowed view (`Navigation`, PR #7); fit-to-view and standard views still to come
- [x] Technical views: triangles, IDs, wireframe, overdraw, depth, normals, lighting, and a wire overlay (PR #4, PR #7)
- [ ] Physically based shading with image-based lighting
- [ ] Anti-aliasing, shadows, and ambient occlusion
- [ ] Hidden-line, section planes with caps, selection and hover highlighting
- [ ] Order-independent transparency
- [ ] Baked lighting per body (per vertex for a whole mesh, from CPU ray tracing: PR #7; per body and refreshed on change still to come)
- [ ] Ray-traced shadows and AO; offline path tracer for stills (CPU baking and a CPU path tracer: PR #7; GPU ray tracing and progressive stills still to come)
- [ ] Snapshot tests passing on all three CI platforms

## Phase 1: Part modeler MVP (started)

Specs in `tasks/backlog/` until they are activated. Order and dependencies: `tasks/backlog/README.md`.

- [x] T009 Units and quantities (`cad-math`)
- [ ] T010 Expression parser and evaluator (`cad-math`)
- [ ] T011 2D points, planes, and axes (`cad-math`)
- [ ] T012 Parameter table (`cad-doc`)
- [ ] T013 Topology and face roles in the kernel trait (`cad-kernel`)
- [ ] T014 OCCT topology and face roles (`cad-kernel-occt`)
- [ ] T015 Profiles and extrude in the trait (`cad-kernel`)
- [ ] T016 OCCT extrude (`cad-kernel-occt`)
- [ ] T017 Edge history, fillet, and chamfer in the trait (`cad-kernel`)
- [ ] T018 OCCT fillet and chamfer (`cad-kernel-occt`)
- [ ] T019 Sketch entities and constraints as data (`cad-sketch`)
- [ ] T020 Solver trait and a native solver (`cad-sketch`)
- [ ] T021 Remaining constraints and conflict reporting (`cad-sketch`)
- [ ] T022 Closed region detection (`cad-sketch`)
- [ ] T023 Document and feature model (`cad-doc`)
- [ ] T024 Naming revision from the T006 report (`cad-doc`)
- [ ] T025 Transactions and undo (`cad-doc`)
- [ ] T026 Rebuild engine, sequential (`cad-eval`)
- [ ] T027 Command registry skeleton (`cad-commands`)
- [ ] Headless CLI, native file format, STEP import into documents (not yet specified)

Exit: 25 reference parts modelled through the CLI match golden volumes. Naming corpus passes. Performance budgets met.
