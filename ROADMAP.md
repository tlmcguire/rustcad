# Roadmap

Full phases and exit criteria: `docs/PLAN.md`, section 13.

## Phase 0: Foundation (current)

- [x] Workspace skeleton, all crates stubbed
- [x] Layer, unsafe, format, lint, and test gates (`scripts/gates.sh`)
- [x] `cargo-deny` policy
- [x] CI workflow for Linux, macOS, Windows (not yet run on a host)
- [x] `cad-math`: `Vec3`, `Tolerance`
- [x] `cad-kernel`: Phase 0 `Kernel` trait and `HistoryMap` types
- [x] License: GPL-3.0-or-later, `LICENSE` added (D12)
- [ ] Verify the items in `docs/PLAN.md`, section 17.3
- [x] T001 Reference kernel for analytic boxes (test double)
- [x] T002 Golden geometry harness
- [x] T003 OCCT build and minimal binding spike
- [x] T004 Worker process spike with latency numbers
- [x] T005 wgpu viewport and ID-buffer picking spike
- [x] T006 Topological naming spike
- [x] T007 Snapshot harness: headless renders checked against reference images under `cargo test`
- [ ] Determinism, performance, coverage, mutation, and API-diff gates
- [ ] Loop orchestrator

Exit: all spikes have reports. A STEP round trip runs in CI. The loop completes one trivial task unaided.
