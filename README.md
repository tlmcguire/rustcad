# RustCAD

Parametric CAD written in Rust to be intuitive, efficient, and powerful.

RustCAD is a desktop app for mechanical part design. You shape parts by pushing and pulling
geometry directly, and every step is recorded as an editable feature history, so a design
stays changeable long after it is drawn.

![A block with a pocket, a slot, and a boss, modelled and rendered by RustCAD](docs/images/demo.png)

*Modelled by the OpenCascade backend and rendered by the wgpu renderer:
`cargo run -p cad-testkit --example demo`.*

## Status

Early foundation work (Phase 0). **There is no usable application yet.** The geometry
kernel and the renderer work and are tested; the document model, the UI, and the command
line tool are not built.

### What works today

| Area | State |
|---|---|
| Geometry kernel | OpenCascade 7.9.3 behind a `Kernel` trait: boxes, union, subtract, intersect, face counts, tessellation, mass properties. Booleans report what happened to every face. |
| Rendering | wgpu renderer with shaded faces and edges, in a window or offscreen to PNG. About 2 ms per frame for one million triangles on an Apple M2. |
| Picking | Click a pixel, get the face under it, through a GPU ID buffer. |
| Stable face names | Faces are named by the feature that made them, so references survive edits to earlier steps. A spike, with one known flaw documented. |
| Testing | A golden-file harness that checks any kernel against expected volume, area, bounding box, and face count. A box-only reference kernel for tests. |
| Quality gates | One script runs formatting, build, tests, lints, crate layering, an `unsafe` check, and license and advisory checks. |

Tested on macOS (Apple Silicon) only so far. Linux and Windows are intended but unverified.

### Not built yet

Sketching, extrude and every other feature beyond boxes and booleans, the document model,
undo, file formats, import and export, the user interface, the command line tool, the MCP
server, and plugins. The kernel also runs in-process for now, so a crash inside it is not
contained.

## Planned features

**Modelling**

- Direct push and pull on top of a recorded, editable feature history
- 2D sketcher with constraints and dimensions
- Extrude, revolve, sweep, loft, fillet, chamfer, shell, draft, rib, holes, threads
- Linear, circular, and mirror patterns
- Named parameters and expressions with unit checking in every numeric field
- Reference planes, axes, and points; multi-body parts; configurations
- References that survive edits to earlier features, with no silent rebinding

**Interface**

- A fast viewport with crisp edges, section views, and selection highlighting
- An adaptive toolbar that shows the tools that fit the current selection
- Command palette, inspector, history panel, and parameters panel
- Measure tool and mass properties

**Files**

- A native file format with autosave and crash recovery
- STEP, STL, and 3MF import and export

**Automation and extension**

- A built-in MCP server, so AI agents can drive every modelling operation through typed,
  transactional tools
- Sandboxed WebAssembly plugins with declared capabilities and a versioned API
- A headless command line tool for batch work, tests, and export

**Later**

- Assemblies with joints, interference checks, and exploded views
- 2D drawings with PDF and DXF export
- Surfacing tools and sheet metal

Runs on Linux, macOS, and Windows, with no account and no network connection needed.

The order of work is in [ROADMAP.md](ROADMAP.md). The full design is in
[docs/PLAN.md](docs/PLAN.md).

## Building

You need a recent stable Rust toolchain.

    cargo build --workspace
    bash scripts/gates.sh     # everything that must pass before a merge

The OpenCascade backend is optional at build time. Without it the workspace still builds and
its tests are skipped. To build it you need git, CMake, Ninja, and a C++17 compiler:

    bash crates/cad-kernel-occt/native/build-occt.sh target/occt

That fetches and builds a pinned OpenCascade release (about five minutes on an Apple M2).
The next `cargo build` finds it in `target/occt/install`. Set `OCCT_ROOT` to use an install
somewhere else.

## Trying it

    # Model a part with OpenCascade and render it to a PNG (needs the backend above)
    cargo run -p cad-testkit --example demo -- part.png

    # Open a window with an orbiting box; click a face to print its index
    cargo run -p cad-render --example viewport

    # Render a box to a PNG without a window
    cargo run -p cad-render --example snapshot -- box.png

    # Time a one-million-triangle mesh
    cargo run --release -p cad-render --example bench

## Repository

| Path | Contents |
|---|---|
| `crates/` | The workspace. Each crate has one job and may depend only on lower layers. |
| `docs/PLAN.md` | The full design: product, architecture, roadmap, testing. |
| `docs/reports/` | Findings from each Phase 0 spike. |
| `ARCHITECTURE.md` | Short, binding rules for code. |
| `DECISIONS.md` | Decisions that later work must follow. |
| `ROADMAP.md` | What is done and what is next. |
| `tasks/` | One file per task: backlog, active, done. |
| `NATIVE_DEPS.md` | Native libraries, their versions, and their licenses. |

## License

MIT OR Apache-2.0. `LICENSE-APACHE` still needs to be added (see [ROADMAP.md](ROADMAP.md)).

OpenCascade is a separate library under LGPL-2.1 with the Open CASCADE exception. It is
fetched at build time and linked dynamically. This software uses facilities provided by the
Open CASCADE Technology software.
