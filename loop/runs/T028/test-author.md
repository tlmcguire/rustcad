I wrote 12 acceptance tests for T028 in `crates/cad-math/tests/vec3_distance_midpoint.rs`. Right now they don't compile, because `Vec3::distance` and `Vec3::midpoint` don't exist yet. That's the expected state until the builder adds them.

By acceptance criterion:

1. **Distance basics:** the distance from (0,0,0) to (3,4,0) is 5, and the distance from a point to itself is 0. I added one more case, (1,2,2) between two points away from the origin, so all three components count.
2. **Distance symmetry:** `a.distance(b)` equals `b.distance(a)` for four non-trivial pairs of points. Two extra checks go a bit beyond the spec: the distance between distinct points is positive, and it matches `(a - b).length()`.
3. **Midpoint basics:** the midpoint of (0,0,0) and (2,4,-6) is (1,2,-3), and `a.midpoint(a)` is `a`. I added an offset case that checks the midpoint is the average of each coordinate.
4. **Midpoint symmetry and distance:** `a.midpoint(b)` equals `b.midpoint(a)`, and the midpoint is the same distance from both points. One extra check requires that distance to be exactly half of `a.distance(b)`. Without it, any point equally far from both ends would pass.

All float comparisons go through `Tolerance`. I made no other changes and did not commit.

TESTS: The tests cover Euclidean distance (a known value, zero for the same point, symmetry, agreement with `(a - b).length()`) and the midpoint (known values, `a.midpoint(a)` is `a`, symmetry, and being half the distance from each end).