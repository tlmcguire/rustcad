# Decision log

Binding on all later tasks. To reverse one, add a new record that names it and the evidence that changed.

| # | Decision | Status |
|---|---|---|
| D1 | Kernel: OpenCascade behind the `Kernel` trait. Pure Rust backend is experimental. | Accepted |
| D2 | License: MIT OR Apache-2.0. LGPL native libraries are linked dynamically. No GPL dependencies. | Superseded by D12 |
| D3 | Paradigm: direct interaction on top of a recorded feature history. | Accepted |
| D4 | Source of truth: the feature graph. Geometry is a cache. | Accepted |
| D5 | One typed command registry serves GUI, plugins, MCP, and CLI. | Accepted |
| D6 | Plugins: WASM components with declared capabilities. | Accepted |
| D7 | MCP: generated from the command registry. Transactional. No code execution tool. | Accepted |
| D8 | Rendering: wgpu. | Accepted |
| D9 | Development: autonomous loop behind scripted merge gates. | Accepted |
| D10 | Units: lengths are millimetres and angles are radians internally. | Accepted |
| D11 | Golden files: strict line-based `key = value` text, parsed in `cad-testkit`. No serialization dependency. Unknown, duplicate, or missing keys are errors. | Accepted |
| D12 | License: GPL-3.0-or-later for all first-party code. Supersedes D2. Evidence: the owner decided on 2026-11-13 that anything built on the project must carry the same license. LGPL native libraries are still linked dynamically. Dependencies must be compatible with GPL-3.0. | Accepted |
| D13 | Toolchain: pinned in `rust-toolchain.toml` and CI (Rust 1.93.1 at first). Every gate, locally and in CI, uses the same compiler and lints. Bumping it is a commit of its own, and only after the gates pass on the new version. Evidence: on 2026-11-13 CI on the latest stable (1.100) failed on a clippy lint the local 1.93 toolchain did not have, and 1.100 under-reports coverage (lines that run show zero hits), so it cannot run the coverage gate. | Accepted |
| D14 | Priority: a robust, high-quality renderer comes before further Phase 1 modeling work. Scope in `docs/PLAN.md` section 9.1: physically based shading, shadows and AO, baked lighting, ray tracing with an offline path tracer, technical view modes, and a torture suite. Evidence: the owner decided on 2026-11-14 that people must be able to view their models in the utmost quality. | Accepted |
