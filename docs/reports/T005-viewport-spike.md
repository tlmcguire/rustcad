# T005 report: viewport and picking spike

Date: 2026-11-13. Machine: Apple M2, macOS 27.0.1, Metal backend.

## Result

wgpu rendering and ID-buffer picking both work, in a window and offscreen. The approach in
`docs/PLAN.md` sections 9.1 and 9.2 holds for faces. Nothing here was run on Linux or Windows.

| Acceptance point | Status | Evidence |
|---|---|---|
| 1. Shaded faces and edges, to a window and to a PNG | Met on macOS | `examples/viewport.rs` presented 120 frames at 1600 x 1200. `examples/snapshot.rs` and the `renders_to_a_png_file` test write PNGs. The PNG was inspected by eye; the window was not. |
| 2. Picking at a pixel returns the `FaceIndex` under it | Met | Six axis views each return the face that faces the camera; a corner view shows exactly the three near faces. |
| 3. Offscreen rendering works in CI without a display | Not verified | The offscreen path creates no window, surface, or display handle, and the tests use only that path. It has not been run on a CI host. See "Open items". |
| 4. Report | This file | |

## wgpu version

| Crate | Version | Role |
|---|---|---|
| wgpu | 30.0.2 | Renderer |
| pollster | 1.0.1 | Blocks on wgpu's two async setup calls |
| bytemuck | 1.25.2 | Casts vertex data to bytes without `unsafe` in this crate |
| png | 0.18.1 | PNG encoding |
| winit | 0.30.13 | Dev-dependency only, for the window example |

`cad-render` itself does not depend on a windowing library. `Renderer::new` takes a device,
so the UI crate chooses how the surface is made.

## License check

`cargo deny check` passes: advisories ok, bans ok, licenses ok, sources ok.

- wgpu, naga, wgpu-core, wgpu-hal, png: MIT OR Apache-2.0. pollster: Apache-2.0 or MIT.
  bytemuck: Zlib OR Apache-2.0 OR MIT. winit: Apache-2.0.
- The resolved graph has 255 third-party packages across all platforms (93 in the normal
  dependency tree of `cad-render` on this machine). All are covered by the existing
  allowlist in `deny.toml`. No GPL or LGPL.
- winit's default features pull in `sctk-adwaita`, which depends on `ttf-parser`, flagged
  unmaintained (RUSTSEC-2026-0192). The `wayland-csd-adwaita` feature is switched off to
  avoid it. Wayland windows on compositors without server-side decorations will have no
  title bar in the example. This needs a decision before `cad-ui` adopts winit.
- Four packages resolve in two versions (`hashbrown`, `jni-sys`, `miniz_oxide`, `syn`).
  `deny.toml` only warns about that.

### Gate fix that came with this task

With `cargo-deny` installed for the first time, the `bans` check failed on the untouched
workspace: `wildcards = "deny"` treats a path dependency with no version as a wildcard.
Every workspace path dependency now carries `version = "0.0.0"`. `deny.toml` is a protected
path and was not changed.

## Frame time for 1M triangles

`cargo run --release -p cad-render --example bench`

| Measurement | Value |
|---|---|
| Triangles | 1,002,528 (wavy sheet, 484 faces, 65,136 edge segments) |
| Target | 1920 x 1080, colour + face ID + depth |
| Frame time | median 2.04 ms, min 1.91 ms, max 4.57 ms over 200 frames |
| Upload, including edge extraction | 187 ms |
| Pick one pixel | 0.52 ms |
| Offscreen render with full colour and ID readback | 8.5 ms |

Frame time is draw, submit, and wait for the GPU to finish, with no presentation. It is well
inside the 8 ms navigation budget in section 9.5 on this machine, which is not "mid-range
hardware" by itself. The window example ran at the display's refresh rate (about 16.7 ms per
frame with vsync) for the 12-triangle box.

## How it works

- One render pass with two colour attachments: the picture, and an `R32Uint` texture holding
  face index plus one (zero is background). Picking copies one pixel of that texture back.
- Faces are expanded to three vertices per triangle so each carries its face index. That is
  28 bytes per vertex, 84 MB for 1M triangles.
- Edges are the boundary edges of each face, found by vertex index. They are drawn as
  one-pixel lines, pulled slightly toward the camera, and do not write IDs.
- Shading is a headlight term on a fixed colour.

## Open items

1. **CI without a display.** `Renderer::headless` asks for a hardware adapter, then a
   software one. A Linux runner needs a software Vulkan driver (lavapipe) or similar. If no
   adapter exists the tests fail with `NoAdapter`; they do not skip. `.github/` is a
   protected path, so the workflow change is left to a maintainer.
2. **Edges are not crisp.** One-pixel lines with no anti-aliasing; silhouette edges look
   dotted where the line and the face behind it disagree on coverage. Section 9.1 item 2
   needs screen-space quads or a post-process pass.
3. **Edge and vertex picking** (section 9.2 item 2) is not attempted.
4. **Memory.** The expanded vertex layout is simple, not compact. An indexed layout with a
   per-triangle ID lookup would cut it by about four times.
5. **wgpu validation errors panic.** Bad sizes, cameras, and meshes are caught first and
   returned as `RenderError`. Anything else wgpu rejects still goes to its default handler,
   which panics. Error scopes should wrap the calls before this leaves spike status.
6. **Images differ across GPUs.** Tests assert on picked faces and coarse brightness, not
   on exact pixels. The snapshot gate in the roadmap needs a tolerance or a pinned software
   renderer.
7. **Stable IDs.** The ID buffer holds `FaceIndex`, which is not stable across operations.
   Mapping to stable topology IDs waits for T006.
