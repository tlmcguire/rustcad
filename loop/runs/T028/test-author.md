I wrote the acceptance tests in `crates/cad-math/tests/vec3_distance_midpoint.rs`, one new file and no other changes. As expected, they don't compile yet: `cargo test -p cad-math --test vec3_distance_midpoint` fails with 13 errors because `Vec3::distance` and `Vec3::midpoint` don't exist.

There are 10 tests covering the four acceptance criteria. All float comparisons go through `Tolerance::eq_len`, `is_zero_len` or `eq_point`.

1. **Known distances:** (0,0,0) to (3,4,0) is 5. An extra case uses all three axes from a non-origin start. A point's distance to itself is 0.
2. **Symmetric distance:** `a.distance(b)` equals `b.distance(a)` for every pair of five non-trivial points. Another test checks that distance is never negative.
3. **Known midpoints:** the midpoint of (0,0,0) and (2,4,-6) is (1,2,-3). An extra case uses two non-origin points, and `a.midpoint(a)` is `a` for every sample point.
4. **Symmetric midpoint:** the midpoint is the same either way round and the same distance from both points. It also checks that each half is half the full distance.

My attempt to run rustfmt was denied. I made the one change `cargo fmt --check` had reported by hand, copying its suggested output exactly, but I haven't re-run the check.

TESTS: Known values, the zero-distance and `a.midpoint(a)` cases, symmetry of both helpers, and equal distance from the midpoint to both points.