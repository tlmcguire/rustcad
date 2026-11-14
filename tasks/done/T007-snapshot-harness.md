# T007 Snapshot harness

**Crate:** `cad-testkit`, with PNG decoding added to `cad-render`.
**Goal:** Compare a rendered image with a stored reference under a threshold, so that a change in rendering fails `cargo test`.
**Acceptance:**
1. `Image::from_png` reads back what `Image::to_png` wrote, exactly. Data that is not an 8-bit RGBA PNG returns a typed error.
2. A snapshot is a PNG plus a spec file in the D11 format that records the per-channel tolerance, the number of pixels allowed to differ, and how the reference was established.
3. `Snapshot::check` returns the difference as data: the count of differing pixels and the largest channel difference. A size mismatch is its own error.
4. `assert_snapshot(image, path)` fails with a message naming the file, the differing pixel count, and the allowed count, and writes the actual image and a difference image outside the source tree.
5. One reference snapshot of the T001 box passes. The same box from another view fails against it.
6. The reference image is rewritten only on an explicit request (`CAD_SNAPSHOT_UPDATE=1`), never by a failing run.
**Out of scope:** changes to `scripts/gates.sh` or CI (protected paths; the check runs under the existing `cargo test` gate), a pinned software renderer, snapshots of OCCT shapes.
**Dependencies:** no new third-party crates. `png` is already a dependency of `cad-render`.
**Depends on:** T001, T005.
