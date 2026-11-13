# Architecture rules

Short and binding. The reasoning is in `docs/PLAN.md`, section 5.

1. **Layers.** A crate may depend only on crates in a lower layer. The layers are in `scripts/layers.txt` and enforced by `scripts/check_layers.py`.
2. **unsafe.** Forbidden by workspace lint. Only `cad-kernel-occt` may opt out, when the FFI lands.
3. **One command layer.** All document edits go through `cad-commands`. GUI, CLI, MCP, and plugins call the same commands.
4. **No modeling logic in `cad-ui`.** If the CLI cannot do it, it is in the wrong crate.
5. **Geometry is a cache.** The feature graph in `cad-doc` is the source of truth.
6. **Kernel access.** Only through the `Kernel` trait in `cad-kernel`. Every modifying operation returns a `HistoryMap`.
7. **Float comparison.** Only through `cad_math::Tolerance`.
8. **Errors are data.** Library crates return typed errors. No `unwrap` or `panic!` in normal flow.
9. **Determinism.** Same inputs give the same output, across runs and thread counts.
10. **Dependencies.** New third-party crates need a task that says why, and must pass `cargo deny`.

## Protected paths

Loop agents may not modify: `.github/`, `scripts/`, `deny.toml`, `loop/`, and this list.
