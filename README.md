# FOSS Parametric CAD in Rust

Working name pending. A direct-plus-parametric CAD app with a built-in MCP server and WASM plugins.

Status: Phase 0 (foundation). Nothing is usable yet.

- Full plan: `docs/PLAN.md`
- Rules for code: `ARCHITECTURE.md`
- Binding decisions: `DECISIONS.md`
- What is next: `ROADMAP.md` and `tasks/backlog/`

## Build

    cargo build --workspace
    bash scripts/gates.sh     # everything that must pass before a merge

## License

MIT OR Apache-2.0. `LICENSE-APACHE` still needs to be added (see ROADMAP.md).
