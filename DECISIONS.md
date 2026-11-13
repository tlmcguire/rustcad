# Decision log

Binding on all later tasks. To reverse one, add a new record that names it and the evidence that changed.

| # | Decision | Status |
|---|---|---|
| D1 | Kernel: OpenCascade behind the `Kernel` trait. Pure Rust backend is experimental. | Accepted |
| D2 | License: MIT OR Apache-2.0. LGPL native libraries are linked dynamically. No GPL dependencies. | Accepted |
| D3 | Paradigm: direct interaction on top of a recorded feature history. | Accepted |
| D4 | Source of truth: the feature graph. Geometry is a cache. | Accepted |
| D5 | One typed command registry serves GUI, plugins, MCP, and CLI. | Accepted |
| D6 | Plugins: WASM components with declared capabilities. | Accepted |
| D7 | MCP: generated from the command registry. Transactional. No code execution tool. | Accepted |
| D8 | Rendering: wgpu. | Accepted |
| D9 | Development: autonomous loop behind scripted merge gates. | Accepted |
| D10 | Units: lengths are millimetres and angles are radians internally. | Accepted |
| D11 | Golden files: strict line-based `key = value` text, parsed in `cad-testkit`. No serialization dependency. Unknown, duplicate, or missing keys are errors. | Accepted |
