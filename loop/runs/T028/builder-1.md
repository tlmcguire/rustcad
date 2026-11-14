I added `Vec3::distance` and `Vec3::midpoint` to `crates/cad-math/src/vec3.rs`. That file is the only change, about 12 lines. All 12 acceptance tests pass, and so does the rest of `cargo test --workspace`; clippy with `-D warnings` is clean.

- `distance(self, other)` returns `(self - other).length()`.
- `midpoint(self, other)` returns `(self + other) * 0.5`.

**Decision for you:** the test author's file isn't fully `rustfmt`-formatted, and I left it that way. Running `cargo fmt --all` rewrapped one line in `crates/cad-math/tests/vec3_distance_midpoint.rs`, and I put that line back by hand because builders may not edit that file. The working tree now matches the committed version, so if `scripts/gates.sh` runs `cargo fmt --check`, it will fail on that one line. I didn't run `gates.sh`. The test author or the orchestrator would need to fix the formatting: running `cargo fmt` on that file would do it.

COMMIT: Add Vec3 distance and midpoint helpers

Add `Vec3::distance`, the Euclidean distance between two points, and `Vec3::midpoint`, the point halfway between them.