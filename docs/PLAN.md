# FOSS Parametric CAD in Rust: Planning Document

Date: 2026-11-13
Status: Draft 1
Working name: "the app" (no name chosen yet)

## How to read this document

Sections 1 to 4 cover what we are building and why. Sections 5 to 12 cover the technical design. Sections 13 to 16 cover delivery: the roadmap, testing, the autonomous development loop, and security. Section 17 lists risks and everything that still needs verification. Section 18 lists sources.

Claims about other CAD products come from research done on 2026-11-13 and are listed in the sources. Claims about Rust crates and software licenses marked **[verify]** come from general knowledge and were not checked during that research. Check them before committing.

---

## 1. Summary

### 1.1 What this is

A desktop parametric CAD application for mechanical part design. It is written in Rust. It is free and open source, with no dependency on proprietary software. It combines direct modeling (push and pull geometry) with a parametric feature history. It ships with a built-in MCP server so AI agents can drive it, and a sandboxed plugin system so others can extend it.

### 1.2 Goals

1. Feel as fast and approachable as Shapr3D and Plasticity.
2. Keep a full parametric history, so designs stay editable.
3. Run on Linux, macOS, and Windows with no account and no network requirement.
4. Expose every modeling operation to AI agents through typed MCP tools.
5. Support third-party plugins in a sandbox with a stable, versioned API.
6. Use only FOSS dependencies, assets, and build tooling.
7. Be built by a fully autonomous agent loop with automated quality gates.

### 1.3 Non-goals for version 1

1. Assemblies with joints and motion. Planned for a later phase.
2. 2D manufacturing drawings. Planned for a later phase.
3. CAM, simulation, and rendering. Left to plugins.
4. Native import of proprietary formats such as SLDPRT, X_T, and DWG.
5. Cloud storage, accounts, and real-time collaboration.
6. Mobile and tablet builds.
7. Mesh sculpting and subdivision modeling.

### 1.4 Key decisions

| # | Decision | Choice | Section |
|---|---|---|---|
| D1 | Geometry kernel | OpenCascade (OCCT) behind a `Kernel` trait. Pure Rust backend kept as an experiment. | 6 |
| D2 | App license | GPL-3.0-or-later (D12 in `DECISIONS.md`; originally MIT OR Apache-2.0). OCCT linked dynamically. | 4 |
| D3 | Modeling paradigm | Direct interaction on top of a recorded feature history. | 3 |
| D4 | Source of truth | The feature graph. Geometry is a cache. | 8 |
| D5 | Command layer | One typed command registry serves GUI, plugins, MCP, and CLI. | 5 |
| D6 | Plugins | WASM components with declared capabilities. | 12 |
| D7 | MCP | Generated from the command registry. Transactional. No code execution tool. | 11 |
| D8 | Rendering | wgpu. | 9 |
| D9 | Development | Fully autonomous planner, test author, builder, reviewer loop behind scripted merge gates. | 15 |

### 1.5 Target users

1. Hobbyists and makers who design parts for 3D printing and CNC.
2. Students learning CAD.
3. Engineers who want open files and scriptable tools.
4. Developers building agent workflows that produce real geometry.

---

## 2. Landscape research

### 2.1 Shapr3D

**What it does well**

- It integrates direct and history-based workflows in one minimal interface.
- Its adaptive UI shows tools based on what is selected. Selection also filters the history steps related to that geometry.
- History was added in 2023 as a sidebar. Users who leave it closed barely notice it exists.
- Direct edits, such as moving or offsetting a face, appear as steps at the end of the history.
- Operations such as loft and sweep show live previews while being adjusted.
- When a reference or constraint breaks, the app notifies the user with guidance on fixing it.
- It runs on Siemens Parasolid, which gives it robust geometry.

**Weaknesses relevant to us**

- It is proprietary and paid for full use.
- Its kernel is proprietary, so the approach cannot be copied directly.

**Lessons**

1. Select first, then show tools. Never make users hunt through menus.
2. History should be present but optional to look at.
3. Direct edits must be recorded as features.
4. Every operation needs a live preview.
5. Failures need plain explanations and suggested fixes.

### 2.2 Plasticity

**What it does well**

- It offers direct editing without a history tree. Users push, pull, and adjust geometry instantly.
- It uses Parasolid plus xNURBS for precise surfaces and complex solids.
- It runs on macOS, Windows, and Linux.
- It is positioned as a complement to other CAD tools, with strong surfacing.

**Weaknesses relevant to us**

- No parametric history means earlier decisions cannot be edited later.
- It is proprietary, with a proprietary kernel. Some features are limited to higher tiers.

**Lessons**

1. Keyboard-first command flow keeps experts fast.
2. Gizmo-driven direct manipulation makes modeling feel immediate.
3. Cross-platform support, including Linux, is expected.

### 2.3 Autodesk Fusion 360

**What it does well**

- Reviewers praise its user-friendly interface and combined CAD and CAM.
- It is seen as strong for product concept development and complex surfaces.

**What users complain about**

- Random crashes and heavy reliance on the cloud.
- Sign-in and cloud features depend on an internet connection.
- Poor performance with large assemblies and slow 2D drawing updates.
- UI freezes during which the program uses little CPU or GPU.
- Timeline features that break and block rollback.

**Lessons**

1. Local-first. Files live on disk. No account.
2. The UI thread must never wait on geometry computation.
3. Use all available cores.
4. A timeline is a good history metaphor, but it needs robust references.

### 2.4 SolidWorks

**What it does well**

- It has the deepest feature set and is an industry standard.
- It runs on Parasolid.

**What users complain about**

- It is only officially supported on Windows.
- Rebuilds are largely single-threaded, so performance depends on single-core speed.
- Crashes are common enough that resellers publish long troubleshooting guides.
- History trees break easily, which is a well-known frustration.

**Lessons**

1. Cross-platform from day one.
2. Parallel, incremental rebuild.
3. Crash isolation and reliable autosave.
4. Reference robustness is a core feature, not a detail.

### 2.5 FreeCAD

**What it does well**

- It is free, open source, and scriptable with Python.
- Users own their files.
- Version 1.0 (November 2024) added an integrated assembly workbench and UI improvements.

**What users complain about**

- The topological naming problem. A shape's internal name can change after a modeling operation, which breaks later features.
- Version 1.0 adopted Realthunder's algorithm to mitigate it. The project's own documentation says the algorithm does not fix every failure.
- Fillets and chamfers placed mid-design are still fragile.
- The UI and workbench structure are widely criticized as harder to learn than commercial tools.

**Lessons**

1. Design stable topological naming into the document model from the first commit. Retrofitting took FreeCAD years.
2. Avoid workbenches and mode switches.
3. Open, user-owned files are a real selling point.

### 2.6 Existing CAD MCP servers

**What exists**

- Third-party MCP servers exist for FreeCAD, Fusion 360, SolidWorks, Onshape, and AutoCAD.
- The most popular FreeCAD server has around ten tools and relies on an `execute_code` tool that exposes the full Python API.
- Fusion servers usually run an external MCP process that forwards commands to an add-in inside Fusion, which executes them on the main thread.

**Weaknesses**

- They are bridges bolted onto apps that were not designed for agents.
- Arbitrary code execution is a security problem.
- There is no transaction or preview model for agent edits.
- Agents must refer to geometry by fragile indices.

**Lessons**

1. Build MCP into the core. Do not bolt it on.
2. Provide typed tools, stable geometry IDs, transactions, and visual feedback.
3. Do not ship a code execution tool.

### 2.7 Rust CAD kernel ecosystem

- **truck.** A pure Rust B-rep kernel with NURBS. It has crates for geometry, topology, modeling, boolean operations with shape healing, and wgpu-based rendering.
- **monstertruck.** An active fork of truck that adds fillets, healing, and more STEP support.
- **arris.** A newer pure Rust B-rep kernel with booleans, blends, tessellation, and STEP. Licensed MIT or Apache-2.0.
- **Fornjot.** An early-stage Rust B-rep kernel. Its repository was archived in June 2026 and it is no longer developed. It never reached boolean operations.
- **opencascade-rs.** Rust bindings to OpenCascade built with cxx. The author describes them as a major work in progress.

**Lesson:** pure Rust kernels exist but none is proven for production fillets and booleans. OCCT is the only mature FOSS B-rep kernel.

### 2.8 Summary table

| Tool | Copy | Avoid |
|---|---|---|
| Shapr3D | Adaptive UI, optional history, direct edits as features | Proprietary kernel |
| Plasticity | Speed, keyboard flow, gizmos | No history |
| Fusion 360 | Timeline, integrated workflow | Cloud dependence, UI freezes |
| SolidWorks | Feature depth | Windows only, single-threaded rebuild, fragile trees |
| FreeCAD | Open files, scripting | Workbenches, late topological naming fix |
| CAD MCP servers | The idea | Bridges, code execution, index-based references |

---

## 3. Product principles and UX model

### 3.1 Principles

1. **Direct feel, parametric memory.** The user manipulates geometry. The app records features.
2. **Selection drives the UI.** What is selected decides what tools appear.
3. **No modes.** There are no workbenches. Sketching happens in place in the 3D view.
4. **Never block.** Input stays responsive during any computation. Every long operation can be cancelled.
5. **Always preview.** No operation commits without showing its result first.
6. **Explain failures.** A failed feature says what failed, where, and what to try.
7. **Local and open.** Files are on disk, readable, and diffable.
8. **One command layer.** GUI, plugins, CLI, and MCP all do the same things in the same way.
9. **Progressive disclosure.** Beginners see a few tools. Experts reach everything from the keyboard.

### 3.2 Screen layout

1. **Viewport.** Fills the window.
2. **Adaptive toolbar.** A short strip on the left. Contents change with the selection.
3. **History panel.** Collapsible, on the right. Closed by default for new users.
4. **Items panel.** Bodies, sketches, planes, and parameters. Collapsible.
5. **Command palette.** Opens with a key. Fuzzy search over every command.
6. **Inspector.** A small floating card near the cursor for the active operation's inputs.
7. **Status line.** Rebuild state, units, selection count.

### 3.3 Adaptive toolbar rules

| Selection | Tools offered |
|---|---|
| Nothing | Sketch, primitive bodies, import |
| Plane or planar face | Sketch on it, offset plane |
| Closed sketch profile | Extrude, revolve, sweep, loft |
| One or more faces | Push/pull, offset, draft, shell, delete face, sketch on face |
| One or more edges | Fillet, chamfer, move edge |
| One body | Move, copy, mirror, pattern, shell, export |
| Two or more bodies | Union, subtract, intersect, align |
| A history step | Edit, suppress, roll back to here, delete |

The rules live in data, not code. Plugins can add rows.

### 3.4 Interaction details

1. **Gizmos.** Drag handles appear on the selection. Dragging an arrow on a face starts push/pull.
2. **Numeric entry.** Typing a number during any drag sets the exact value. Expressions and parameter names are accepted.
3. **Snapping.** Snaps to vertices, midpoints, centers, edges, and grid. Holding a modifier disables snapping.
4. **Inference.** While sketching, the app proposes constraints (horizontal, tangent, coincident) and shows them before they apply.
5. **Hotkeys.** Single-key shortcuts for common tools. All are rebindable.
6. **Selection filters.** Keys to cycle face, edge, vertex, and body picking. Box and lasso select.
7. **Navigation.** Orbit, pan, zoom presets matching common CAD tools. 3D mouse support through the open spacenav library **[verify]**.
8. **Units.** Per document. Mixed-unit input is accepted and converted ("1in + 5mm").

### 3.5 History UX

1. Every command appends a step. Direct edits are steps too.
2. Selecting geometry highlights the steps that created or changed it.
3. Double-clicking a step reopens its inspector with a live preview.
4. A rollback marker can be dragged to any point in the history.
5. Steps can be reordered when dependencies allow. Illegal moves are refused with a reason.
6. A broken step turns red. Downstream steps that still work stay valid. Hovering the step shows the cause and candidate fixes.
7. Steps can be grouped and named.

### 3.6 Onboarding

1. First run opens a short interactive tutorial that builds a real part.
2. Tooltips show the hotkey and a short animation.
3. The command palette shows each command's description and shortcut.
4. Sample files ship with the app.

### 3.7 Accessibility and settings

1. Full keyboard operation.
2. Scalable UI and high-contrast theme.
3. Light and dark themes.
4. Translatable strings from the start.
5. Settings stored in a plain text file.

---

## 4. Licensing and FOSS policy

Nothing in this section is legal advice. Confirm each license before relying on it.

### 4.1 App license options

1. **MIT OR Apache-2.0.** The Rust ecosystem norm. Easiest for contributors and plugin authors. Requires dynamic linking to LGPL libraries. Cannot include GPL code.
2. **GPL-3.0-or-later.** Allows any FOSS dependency, including GPL solvers. Forks must stay open. Some companies avoid it.
3. **LGPL.** A middle path. Awkward with Rust's static linking model.

**Choice: GPL-3.0-or-later** for all first-party crates. The first draft chose MIT OR Apache-2.0. The owner reversed that on 2026-11-13 (D12) so that anything built on the project stays under the same license.

### 4.2 Dependency licenses

| Dependency | Role | License | Notes |
|---|---|---|---|
| OpenCascade (OCCT) | Kernel | LGPL-2.1 with exception **[verify]** | Link dynamically. Ship as a shared library. |
| truck | Experimental kernel | Apache-2.0 **[verify]** | |
| arris | Experimental kernel | MIT or Apache-2.0 | Confirmed in research. |
| planegcs (from FreeCAD) | Sketch solver option | LGPL **[verify]** | Link dynamically. |
| SolveSpace solver | Sketch solver option | GPL-3.0 **[verify]** | Usable since D12. Was excluded under D2. |
| wgpu | Rendering | MIT or Apache-2.0 **[verify]** | |
| egui | UI | MIT or Apache-2.0 **[verify]** | |
| wasmtime | Plugin runtime | Apache-2.0 with LLVM exception **[verify]** | |
| Rust MCP SDK (rmcp) | MCP | Permissive **[verify]** | |
| rayon, serde, and similar | General | MIT or Apache-2.0 **[verify]** | |

### 4.3 Policy rules

1. Allowed licenses: MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0, Unicode, CC0. LGPL only for dynamically linked native libraries on an explicit list.
2. Dependencies must be compatible with GPL-3.0 (D12). The `cargo-deny` allowlist still lists permissive licenses only; a GPL dependency needs it extended first.
3. No dependency may require an account, a license key, or a network call to function.
4. No proprietary SDKs. This rules out native SLDPRT, X_T, DWG, and similar formats.
5. Fonts and icons must be OFL, Apache, MIT, or CC0.
6. Sample models and test data must be original or CC0.
7. Build and CI tooling must be FOSS and self-hostable.

### 4.4 Enforcement

1. `cargo-deny` runs in CI with the license allowlist. A disallowed license fails the build.
2. A `THIRD_PARTY_LICENSES` file is generated on every release.
3. Each native library has a recorded source, version, and license in `NATIVE_DEPS.md`.
4. Agents cannot edit the allowlist. See section 15.

### 4.5 Contribution policy

1. Developer Certificate of Origin sign-off on commits. No CLA.
2. Contributions are licensed under the same license.
3. Agent-authored commits are labeled as such in a trailer.

### 4.6 Development models

The app has no runtime dependency on any AI model. The autonomous loop depends on whichever LLM drives it. The loop must be model-agnostic and configurable, so it can run on open-weight models. Hosted models can be used when available, but nothing in the repo may assume one.

---

## 5. Architecture overview

### 5.1 Layers

```
            GUI          CLI         MCP server      Plugins (WASM)
             \            |              |              /
              +-----------+--------------+-------------+
                                  |
                          Command registry
                                  |
                +-----------------+-----------------+
                |                 |                 |
           Document model    Rebuild engine     Query engine
                |                 |                 |
                +--------+--------+--------+--------+
                         |                 |
                  Sketch solver      Geometry kernel
                    (trait)             (trait)
                                           |
                              OCCT worker / Rust backend
```

Rendering reads tessellated geometry from the cache and never calls the kernel directly.

### 5.2 Workspace layout

| Crate | Responsibility | Depends on |
|---|---|---|
| `cad-math` | Vectors, transforms, tolerances, units, expressions | none |
| `cad-kernel` | `Kernel` trait, shape handles, tessellation types | `cad-math` |
| `cad-kernel-occt` | OCCT backend, FFI, worker process | `cad-kernel` |
| `cad-kernel-rs` | Experimental pure Rust backend | `cad-kernel` |
| `cad-sketch` | Sketch entities, constraints, `Solver` trait, solver backends | `cad-math` |
| `cad-doc` | Feature graph, parameters, topology IDs, transactions, undo | `cad-kernel`, `cad-sketch` |
| `cad-eval` | Incremental parallel rebuild, caching, error reporting | `cad-doc` |
| `cad-query` | Geometry queries and selection by description | `cad-doc` |
| `cad-io` | Native file format, STEP, IGES, STL, 3MF, OBJ, glTF | `cad-doc` |
| `cad-commands` | Command registry, schemas, validation, previews | `cad-doc`, `cad-eval`, `cad-query`, `cad-io` |
| `cad-render` | wgpu renderer, picking, offscreen rendering | `cad-kernel` types |
| `cad-ui` | Application shell and widgets | `cad-commands`, `cad-render` |
| `cad-plugin` | WASM host, capability checks, WIT interface | `cad-commands` |
| `cad-mcp` | MCP server | `cad-commands`, `cad-render` |
| `cad-cli` | Headless runner | `cad-commands`, `cad-render` |
| `cad-testkit` | Golden test harness, fixtures, snapshot tools | all |

Rules:

1. Dependencies point downward only.
2. `unsafe` is allowed only in `cad-kernel-occt` and solver FFI modules.
3. `cad-ui` contains no modeling logic. If the CLI cannot do it, it is in the wrong crate.

### 5.3 Command registry

A command is a struct with:

1. A stable ID, such as `feature.extrude`.
2. A typed, serializable argument struct with a generated JSON schema.
3. A `validate` step that checks arguments against the document.
4. A `preview` step that returns transient geometry.
5. An `apply` step that mutates the document inside a transaction.
6. Metadata: label, description, icon, hotkey, selection rules for the adaptive toolbar.

From this single definition the build generates the GUI inspector, the MCP tool, the plugin WIT binding, the CLI subcommand, and the documentation page.

### 5.4 Threading model

1. **UI thread.** Input, layout, draw calls. Never blocks.
2. **Rebuild pool.** A rayon thread pool evaluates dirty features.
3. **Kernel workers.** One or more OCCT worker processes. See 6.4.
4. **I/O thread.** File load, save, autosave, import, export.
5. **Server tasks.** MCP and plugin hosts run on an async runtime.

All document mutation goes through a single writer. Readers get immutable snapshots. Cancelling an operation drops its snapshot and discards its results.

### 5.5 State and events

1. The document is an immutable, structurally shared snapshot. Each transaction produces a new snapshot.
2. Undo is a stack of snapshots.
3. Subscribers (UI, MCP, plugins) receive change events with the set of affected features and bodies.

### 5.6 Error handling

1. Library crates return typed errors. No panics in normal flow.
2. A kernel failure is data. It carries the operation, the inputs by stable ID, and a category.
3. A crash in a kernel worker is caught, reported on the feature, and the worker restarts.
4. The app writes a crash-safe journal so no more than one transaction is lost.

---

## 6. Geometry kernel

### 6.1 Options considered

| Option | Maturity | Memory safe | FOSS | Verdict |
|---|---|---|---|---|
| OCCT through FFI | High | No, C++ | Yes | Primary |
| truck family | Medium to low | Yes | Yes | Experimental backend |
| arris | New | Yes | Yes | Watch, test in spikes |
| Fornjot | Archived | Yes | Yes | Rejected |
| Parasolid | Highest | n/a | No | Rejected: proprietary |
| Write our own | None | Yes | Yes | Rejected, too large |

### 6.2 The `Kernel` trait

The trait is the only way the rest of the app touches B-rep geometry. A sketch of its shape:

```rust
pub trait Kernel: Send + Sync {
    // Construction
    fn extrude(&self, profile: &Profile, dir: Vec3, dist: f64) -> KResult<Shape>;
    fn revolve(&self, profile: &Profile, axis: Axis, angle: f64) -> KResult<Shape>;
    fn sweep(&self, profile: &Profile, path: &Wire) -> KResult<Shape>;
    fn loft(&self, sections: &[Profile], opts: LoftOpts) -> KResult<Shape>;

    // Modification
    fn boolean(&self, op: BoolOp, a: &Shape, b: &Shape) -> KResult<Shape>;
    fn fillet(&self, s: &Shape, edges: &[EdgeRef], r: f64) -> KResult<Shape>;
    fn chamfer(&self, s: &Shape, edges: &[EdgeRef], d: f64) -> KResult<Shape>;
    fn shell(&self, s: &Shape, open: &[FaceRef], t: f64) -> KResult<Shape>;
    fn offset_face(&self, s: &Shape, faces: &[FaceRef], d: f64) -> KResult<Shape>;

    // Inspection
    fn topology(&self, s: &Shape) -> Topology;
    fn tessellate(&self, s: &Shape, tol: Tolerance) -> Mesh;
    fn mass_props(&self, s: &Shape, density: f64) -> MassProps;
    fn check(&self, s: &Shape) -> Vec<ShapeIssue>;

    // Exchange
    fn to_brep_bytes(&self, s: &Shape) -> Vec<u8>;
    fn from_brep_bytes(&self, b: &[u8]) -> KResult<Shape>;
}
```

Every modifying operation also returns a **history map**: for each output face and edge, which input entities it came from and whether it was generated, modified, or split. Topological naming depends on this. See 8.4.

### 6.3 OCCT backend

1. Bind with `cxx`. Evaluate the existing opencascade-rs bindings in a spike. They are a work in progress, so expect to extend or fork them.
2. Write a thin C++ shim that exposes only what the trait needs. Keep it small and reviewed by hand.
3. Use OCCT's history APIs on each operation to build the history map.
4. Pin one OCCT version. Build it from source in CI for all three platforms.
5. Ship OCCT as shared libraries next to the binary.

### 6.4 Process isolation

OCCT is C++ and can crash or hang on bad input. The plan:

1. Run OCCT in a pool of worker processes.
2. Communicate over shared memory or pipes with a compact binary protocol.
3. Each request has a timeout. A hung worker is killed and restarted.
4. A crash becomes a feature error, not an app crash.
5. Workers run in parallel, which gives multi-core rebuild even where OCCT is not thread safe.

Cost: serialization overhead on each call. Measure it in a Phase 0 spike. If it is too high for interactive previews, run previews in-process and commits in workers.

### 6.5 Pure Rust backend

1. Implement the trait for one of truck, monstertruck, or arris.
2. Run the same golden tests against both backends.
3. Publish a pass-rate table. Promote the Rust backend when it passes the Phase 1 suite.
4. It may become the default for WASM or web builds later.

### 6.6 Tolerances

1. One document-wide linear tolerance and one angular tolerance.
2. All comparisons go through `cad-math` helpers.
3. Imported shapes are healed and checked on import.

---

## 7. Sketcher and constraint solver

### 7.1 Entities

Point, line, arc, circle, ellipse, spline, construction geometry, projected reference geometry, text (later).

### 7.2 Constraints

Coincident, horizontal, vertical, parallel, perpendicular, tangent, equal, concentric, midpoint, symmetric, fix, point on curve, distance, angle, radius, diameter.

### 7.3 Solver

1. Define a `Solver` trait: input is entities, constraints, and current positions. Output is new positions, degrees of freedom, and conflicting constraints.
2. Backend options:
   1. **planegcs** from FreeCAD. Mature. LGPL. Dynamically linked. **[verify licensing and how cleanly it extracts]**
   2. **Rust-native solver.** Newton or Levenberg-Marquardt over a constraint Jacobian. More work, fully owned.
   3. **SolveSpace solver.** GPL-3.0. Excluded under D2, allowed since D12.
3. Plan: start with planegcs behind the trait. Build the Rust-native solver in parallel as a loop task with planegcs as the reference oracle.

### 7.4 Behavior requirements

1. Dragging a point re-solves at interactive rates and keeps geometry close to its prior position.
2. The sketch shows remaining degrees of freedom by color.
3. Over-constrained sketches name the conflicting constraints.
4. Closed regions are detected automatically and become selectable profiles.
5. Sketches reference outside geometry by stable ID.

---

## 8. Document model

### 8.1 Structure

```
Document
  Parameters        name, expression, unit
  Features          ordered list forming a DAG
    Sketch          plane reference, entities, constraints
    Extrude         profile refs, distance expr, direction, boolean mode
    Fillet          edge refs, radius expr
    ...
  Bodies            derived, cached
  Metadata          units, tolerance, materials, appearance
```

The feature list is the source of truth. Bodies are derived and can be thrown away.

### 8.2 Features

Each feature has:

1. A unique, permanent ID.
2. A type and typed arguments.
3. References to parameters and to geometry by stable ID.
4. A suppressed flag.
5. A cached result and a status: ok, warning, failed, or stale.

Order is explicit for display. Dependencies are computed from references and form the DAG used for rebuild.

### 8.3 Parameters and expressions

1. Named parameters with units: `width = 40 mm`.
2. Expressions with arithmetic, functions, and references to other parameters.
3. Unit checking. Adding a length to an angle is an error.
4. Cycle detection.
5. Any numeric field in any feature accepts an expression.

### 8.4 Topological naming

This is the hardest design problem in the app. FreeCAD's history shows the cost of leaving it late.

**Approach**

1. **Stable IDs are derived, not counted.** A face ID encodes the feature that created it and its role in that feature. Example: `Extrude3 / side / from SketchEdge(Sketch2, e5)`.
2. **History maps propagate IDs.** After each kernel operation, the history map says which outputs came from which inputs. Modified entities keep their ID. Split entities get the parent ID plus a disambiguator.
3. **Disambiguation uses geometry, deterministically.** When one face splits into several, order the children by a stable geometric key, such as position along a reference direction.
4. **References store an ID plus a fallback description.** The description is geometric: "planar face, normal +Z, nearest to point P". If the ID is gone, the description finds candidates.
5. **No silent rebinding.** If the ID fails and the fallback finds exactly one confident match, rebind and flag a warning. If it finds several, mark the feature failed and offer the candidates.
6. **Sketch entities have permanent IDs.** Deleting and redrawing a line is a different entity. Editing it is not.

**Test plan**

1. A corpus of models with scripted edits to early features.
2. For each edit, assert that every downstream reference still resolves to the intended entity.
3. Fuzz: random parameter changes on random models. Assert no silent rebinding.

### 8.5 Rebuild engine

1. A change marks its feature dirty. Dirtiness propagates along the DAG.
2. Features are evaluated in topological order. Independent features run in parallel.
3. Results are cached by a hash of inputs: arguments, resolved parameters, and input shape hashes. Changing a parameter back restores the cached result instantly.
4. Rebuild is cancellable. New input cancels a running rebuild.
5. Previews use coarse tessellation. Commits refine it.
6. A failed feature does not stop independent branches.

### 8.6 Direct edits as features

1. Push/pull on a face created by an extrude edits that extrude's distance when unambiguous.
2. Otherwise it appends an `OffsetFace` or `MoveFace` feature.
3. Direct edits on imported bodies always append features.
4. A setting lets users choose "always append" for predictability.

### 8.7 Transactions and undo

1. Every command runs in a transaction.
2. A transaction can be previewed, committed, or rolled back.
3. Transactions nest. An agent session or a plugin command groups many steps into one undo entry.
4. Undo restores the previous document snapshot. Caches make it instant.

---

## 9. Rendering and UI

### 9.1 Renderer

Built on wgpu **[verify current API]**.

1. Shaded faces with a clean matcap-style default. Optional PBR later.
2. Crisp silhouette and feature edges.
3. Hidden-line and wireframe modes.
4. Section planes with capped cuts.
5. Selection and hover highlighting.
6. Sketch overlay with constraint glyphs and dimensions.
7. Gizmos and construction geometry.
8. Order-independent transparency for ghosted previews.
9. Anti-aliasing.
10. Offscreen rendering to PNG for the CLI, MCP, and tests.

### 9.2 Picking

1. GPU ID buffer: each face, edge, and vertex renders its stable ID to an offscreen target.
2. Edge and vertex picking use a pixel tolerance.
3. Picking results map straight to stable topology IDs.

### 9.3 Tessellation management

1. Per-body meshes are cached and keyed by shape hash and tolerance.
2. Only changed bodies re-upload to the GPU.
3. Level of detail scales with zoom. Refinement happens in the background.

### 9.4 UI toolkit

Options:

1. **egui.** Immediate mode. Fast to build. Easy to embed with wgpu. Weaker text layout and accessibility. **[verify current state]**
2. **Retained-mode Rust toolkits** (iced, Slint, and others). Better structure. More integration work with a custom viewport. **[verify licenses, Slint in particular]**
3. **Custom UI on wgpu.** Full control. Largest cost.

**Choice:** egui for Phase 1. Keep UI logic thin so the toolkit can be replaced. Review at the end of Phase 1.

### 9.5 Performance budgets

| Metric | Target |
|---|---|
| Frame time during navigation | Under 8 ms on mid-range hardware |
| Input to first preview | Under 50 ms for simple features |
| Cold start to usable window | Under 1.5 s |
| Open a 200-feature part | Under 2 s with warm cache |
| Idle memory, empty document | Under 300 MB |

Benchmarks enforce these in CI. See section 14.

---

## 10. File format and interoperability

### 10.1 Native format

A zip container with a custom extension:

```
manifest.json        format version, app version, units
document.json        parameters, features, references
sketches/*.json      one file per sketch
cache/*.brep         kernel-native shape cache, optional
cache/*.mesh         tessellation cache, optional
thumbnail.png
```

Rules:

1. Everything outside `cache/` is text and deterministic. Keys are sorted. Floats are written canonically.
2. The cache can be deleted. The app rebuilds it.
3. An "unpacked" mode stores the same files in a directory for git.
4. A format version field and a migration chain. Old files always open.
5. Unknown feature types from missing plugins are preserved and shown as locked.

### 10.2 Import and export

| Format | Import | Export | Via |
|---|---|---|---|
| STEP (AP203, AP214, AP242) | Yes | Yes | OCCT |
| IGES | Yes | Yes | OCCT |
| BREP | Yes | Yes | OCCT |
| STL | Yes, as mesh | Yes | Rust |
| 3MF | Yes, as mesh | Yes | Rust |
| OBJ | Yes, as mesh | Yes | Rust |
| glTF | No | Yes | Rust |
| DXF (2D) | Later | Later | Rust crate **[verify]** |
| SVG (2D) | Later | Later | Rust |
| SLDPRT, X_T, IPT, DWG | No | No | Proprietary. Convert through STEP. |

### 10.3 Imported geometry

1. An imported solid becomes an `Import` feature holding the shape.
2. Stable IDs for imported faces come from a geometric hash.
3. Direct edit tools work on imported bodies and append features.
4. Feature recognition (holes, fillets) is a later plugin.

### 10.4 Autosave and recovery

1. A journal of transactions is written continuously.
2. On crash, the app replays the journal on next start.
3. Autosave never overwrites the user's file. It writes beside it.

---

## 11. MCP server

### 11.1 Design goals

1. An agent can do anything a user can do.
2. An agent can see what it did.
3. An agent cannot damage anything outside the document.
4. A user can watch, approve, or reject agent work.

### 11.2 Transports

1. **stdio.** For headless use. An MCP client launches `cad-cli mcp`.
2. **HTTP on localhost.** For connecting to the running GUI. Requires a per-session token shown in the app. Bound to loopback only.

Use the official Rust MCP SDK **[verify name, license, and current spec support]**.

### 11.3 Tools

Generated from the command registry. Grouped:

| Group | Examples |
|---|---|
| Document | `doc.new`, `doc.open`, `doc.save`, `doc.info` |
| Parameters | `param.list`, `param.set`, `param.create`, `param.delete` |
| Sketch | `sketch.create`, `sketch.add_entities`, `sketch.add_constraints`, `sketch.solve_status` |
| Features | `feature.extrude`, `feature.revolve`, `feature.fillet`, `feature.chamfer`, `feature.shell`, `feature.boolean`, `feature.pattern`, `feature.edit`, `feature.suppress`, `feature.delete`, `feature.reorder` |
| Query | `query.bodies`, `query.faces`, `query.edges`, `query.measure`, `query.mass_props`, `query.check` |
| View | `view.render`, `view.section` |
| Transaction | `tx.begin`, `tx.preview`, `tx.commit`, `tx.rollback` |
| Exchange | `import.file`, `export.step`, `export.stl`, `export.3mf` |
| Meta | `commands.search`, `commands.describe` |

To keep the tool list small for clients, the server can expose a compact core set plus `commands.search` and `commands.describe` for discovery of the rest.

### 11.4 Resources

1. `doc://features`: the feature list with status.
2. `doc://parameters`.
3. `doc://bodies`: bodies with face and edge summaries and stable IDs.
4. `doc://sketch/{id}`: entities, constraints, degrees of freedom.
5. `doc://errors`: current failures with explanations.
6. Change notifications when the document updates.

### 11.5 Geometry references

1. Agents never use numeric indices.
2. `query.faces` accepts filters: by feature, surface type, normal direction, area range, adjacency, nearest to a point.
3. Results return stable IDs plus a short description of each entity.
4. Tools accept either stable IDs or an inline query that must match exactly the expected count.

### 11.6 Feedback for agents

1. `view.render` returns a PNG from a named or custom camera, with optional ID overlay labels.
2. Every mutating tool returns a summary: bodies changed, volume delta, new face and edge counts, warnings.
3. Errors are structured: category, failing entities, and suggested next actions.
4. `query.check` validates solids for manufacturability basics: closed, manifold, minimum wall (later).

### 11.7 Transactions and user control

1. By default, agent edits happen inside a transaction that the GUI shows as a pending change.
2. The user sees a ghosted preview and a list of steps, and accepts or rejects.
3. A setting allows auto-accept for trusted sessions and for headless runs.
4. An agent session is one undo group.

### 11.8 Safety

1. No tool executes arbitrary code or shell commands.
2. File access is limited to directories the user granted.
3. Resource limits per call: time, memory, and maximum entity counts.
4. All tool calls are logged to a session log the user can inspect.
5. Text from imported files (names, metadata) is returned as data and marked as untrusted.

### 11.9 Prompts

Ship a small set of MCP prompts: "model a part from a description", "modify a parameter-driven part", "review a model for printability". These document good usage patterns for clients.

---

## 12. Plugin system

### 12.1 Runtime

1. Plugins are WebAssembly components run by wasmtime **[verify component model status]**.
2. The interface is defined in WIT and generated from the command registry plus a small host API.
3. Plugins can be written in any language that targets WASM components. Rust gets a first-party SDK crate.

### 12.2 Why WASM over native or scripting

| Option | Sandboxed | Any language | Performance | Verdict |
|---|---|---|---|---|
| WASM components | Yes | Yes | Good | Chosen |
| Native dynamic libraries | No | Mostly Rust and C | Best | Rejected, unsafe and ABI-fragile |
| Embedded Python or Lua | Partly | No | Lower | Rejected for the core, possible as a plugin |

### 12.3 Capabilities

A plugin manifest declares what it needs:

1. `document.read`
2. `document.write`
3. `fs.read` and `fs.write`, scoped to user-chosen paths
4. `network`, scoped to named hosts
5. `ui.panel`
6. `kernel.direct`, for custom geometry features

The user approves capabilities on install. The host enforces them. Default is none.

### 12.4 Extension points

1. **Commands.** New entries in the palette and adaptive toolbar.
2. **Feature types.** A plugin defines arguments and an evaluation function. The feature lives in history like any other.
3. **Importers and exporters.**
4. **Analysis tools.** Read the model and report results, such as draft analysis or printability.
5. **Panels.** Declarative UI described as data and rendered by the host.
6. **MCP tools.** Plugin commands appear in the MCP server automatically.
7. **Adaptive toolbar rules.**

### 12.5 Plugin packaging

1. A plugin is a single archive: `manifest.toml`, the `.wasm` component, and assets.
2. The manifest lists name, version, license, API version, and capabilities.
3. Plugins install from a local file or a git URL. A community index can come later.
4. Plugins must declare a FOSS license to be listed in any official index.

### 12.6 API stability

1. The plugin API has a semantic version separate from the app version.
2. The host supports the current major version and the previous one.
3. Deprecations get one full minor cycle of warnings.
4. Documents record which plugin and version created each custom feature.

### 12.7 Limits

1. Fuel and memory limits per call.
2. Custom features must be deterministic. Same inputs give the same output.
3. A misbehaving plugin is stopped and its features are marked failed. The app keeps running.

---

## 13. Roadmap

Phases are ordered by dependency. No calendar estimates are given, because loop throughput is unknown until Phase 0 finishes. Each phase has exit criteria that the loop can check.

### Phase 0: Foundation and spikes

Set up by hand, once. The loop cannot build its own gates.

1. Workspace skeleton with all crates stubbed.
2. CI on Linux, macOS, and Windows.
3. Gate scripts, protected paths, and `cargo-deny` configuration.
4. OCCT build from source and a minimal binding: box, boolean, fillet, tessellate, STEP export.
5. Worker process spike with latency measurements.
6. wgpu viewport spike with ID-buffer picking.
7. Topological naming spike: extrude, fillet, edit the sketch, check references.
8. Golden test harness and headless renderer.
9. Loop orchestration scripts and the first twenty task specs.

**Exit criteria:** all spikes have written reports. The golden harness runs a STEP round trip in CI. The loop completes one trivial task end to end without help.

### Phase 1: Part modeler MVP

1. `cad-math` complete with units and expressions.
2. Kernel trait and OCCT backend for extrude, revolve, boolean, fillet, chamfer, shell.
3. Sketcher with all listed entities and constraints.
4. Document model, stable IDs, transactions, undo.
5. Rebuild engine with caching and parallelism.
6. Native file format, autosave, journal.
7. STEP, STL, and 3MF import and export.
8. Viewport, picking, gizmos, sketch overlay.
9. Adaptive toolbar, command palette, inspector, history panel.
10. Push/pull with direct-edit-as-feature rules.
11. Parameters panel.
12. Headless CLI.

**Exit criteria:** a scripted benchmark set of 25 reference parts can be modeled through the CLI and matches golden volumes within tolerance. Topological naming corpus passes. Performance budgets met.

### Phase 2: Agents and extensions

1. MCP server over stdio and localhost HTTP.
2. Query engine with selection filters.
3. `view.render` with ID overlays.
4. Agent transaction review UI.
5. WASM plugin host, capability system, Rust SDK.
6. Two reference plugins: a gear generator and a printability check.
7. Generated documentation site for commands, MCP tools, and the plugin API.

**Exit criteria:** a user-simulation agent completes the 25 reference parts through MCP alone. Both reference plugins pass their tests under the sandbox limits.

### Phase 3: Modeling depth

1. Sweep, loft, draft, rib, hole wizard, thread (cosmetic first).
2. Linear, circular, and mirror patterns.
3. Offset, move, and delete face on imported bodies.
4. Reference planes, axes, and points.
5. Measure tool, section views, mass properties.
6. Multi-body workflows and body-level booleans.
7. Materials and appearance.
8. Configurations (named parameter sets).
9. Rust-native sketch solver reaches parity with the reference.

**Exit criteria:** an extended corpus of 100 reference parts passes. The fuzz suite runs clean for a set duration.

### Phase 4: Assemblies and drawings

1. Component instances and external part references.
2. Joints and mates with a 3D constraint solver.
3. Interference check and exploded views.
4. 2D drawings: projected views, sections, dimensions, PDF and DXF export.
5. Large-model performance work.

### Phase 5: Advanced and ecosystem

1. Surfacing tools.
2. Sheet metal as a plugin.
3. Plugin index.
4. Pure Rust kernel promoted if it passes the suite.
5. Web build through WASM and WebGPU if the Rust kernel is ready.

---

## 14. Testing and quality

### 14.1 Test layers

| Layer | What it checks | Tooling |
|---|---|---|
| Unit | Math, expressions, solver steps, ID derivation | `cargo test` |
| Property | Rebuild determinism, undo round trips, solver convergence | `proptest` **[verify]** |
| Golden geometry | Volume, area, bounding box, face and edge counts, validity | `cad-testkit` |
| Round trip | Save and load equality. STEP export then import equivalence. | `cad-testkit` |
| Topological naming | Reference survival under upstream edits | Corpus and fuzz |
| Snapshot | Rendered PNG diffs under a threshold | Headless renderer |
| MCP contract | Schemas, error shapes, transaction semantics | Scripted client |
| Plugin sandbox | Capability enforcement, fuel and memory limits | Hostile test plugins |
| Performance | Budgets in 9.5 | `criterion` **[verify]** |
| Fuzz | File parsers, MCP input, expression parser | `cargo-fuzz` **[verify]** |
| End to end | Reference part corpus through CLI and MCP | Scripts |

### 14.2 Golden test rules

1. Golden values are stored as data files with tolerances.
2. Only the test author role may add or change golden files. See 15.
3. Each golden file records how its value was established: analytic formula, or cross-check against a second tool.
4. Analytic cases come first. A 40 by 20 by 10 box has volume 8000. No kernel is needed to know that.
5. Complex cases are cross-checked against FreeCAD run headless, which uses a different code path to OCCT features.

### 14.3 Cross-backend testing

The same golden suite runs against the OCCT backend and the Rust backend. Differences are reported, not hidden.

### 14.4 Determinism

1. Rebuild output must be identical across runs and thread counts.
2. File saves must be byte-identical for identical documents.
3. A CI job runs the suite twice and compares.

### 14.5 Quality gates

Listed in 15.5. Nothing merges without all of them.

---

## 15. Fully autonomous development loop

### 15.1 Design stance

There is no human approval step. Every human checkpoint is replaced by a machine-checkable gate. This creates two known failure modes, and the design is built around them:

1. **Gate gaming.** An agent that writes both code and tests will make tests pass without making code correct. Countered by separating roles and protecting test data.
2. **Unmeasured quality decays.** Anything without a gate, especially UX and architecture, drifts. Countered by simulation agents and periodic audits, which are weaker than the hard gates.

### 15.2 Roles

| Role | Type | Input | Output | Cannot |
|---|---|---|---|---|
| Planner | Model | Roadmap, decisions, backlog, shelved list | One task spec | Write code |
| Critic | Model | A proposed decision record | Objections and a verdict | Propose its own design |
| Test author | Model | Task spec only | Acceptance tests and golden data | See implementation |
| Builder | Model | Spec, tests, repo | A branch with a change | Edit tests, goldens, gates, CI |
| Reviewer | Model, fresh context | Spec and diff | Approve or findings | Edit code |
| Verifier | Script | Branch | Gate results, merge or reject | Use judgment |
| Auditor | Model | Whole repo, metrics | Refactor and debt tasks | Merge |
| User simulator | Model | Built app, task scripts | UX issues and metrics | Change code |
| Orchestrator | Script | State files | Starts and stops roles, enforces budgets | Use judgment |

Use different model configurations for builder and reviewer where possible, so they do not share blind spots.

### 15.3 State on disk

All loop state is plain files in the repo or beside it:

1. `ROADMAP.md`: phases and exit criteria.
2. `ARCHITECTURE.md`: the rules in section 5.
3. `DECISIONS.md`: numbered decision records. Binding on later tasks.
4. `tasks/backlog/`, `tasks/active/`, `tasks/done/`, `tasks/shelved/`: one file per task.
5. `AGENTS.md` per crate: conventions, pitfalls, how to test.
6. `loop/metrics.jsonl`: one line per task with rounds, tokens, time, outcome.
7. `loop/HALT`: if this file exists, the orchestrator stops.

### 15.4 Task lifecycle

1. **Plan.** The planner picks the next unblocked task by dependency order. It writes a spec: goal, crate, public API, acceptance criteria that tests can check, out-of-scope list, size limit.
2. **Size check.** The orchestrator rejects specs over a diff size estimate. The planner splits them.
3. **Tests first.** The test author writes failing acceptance tests and golden data on a branch. The verifier confirms they compile and fail for the right reason.
4. **Build.** The builder implements in an isolated worktree until local checks pass.
5. **Review.** The reviewer reads spec and diff. It returns approval or numbered findings.
6. **Fix.** The builder addresses findings. Maximum three rounds.
7. **Verify.** The verifier runs every gate on all platforms.
8. **Merge.** On pass, the verifier merges and moves the task to done.
9. **Shelve.** On three failed rounds or a blown budget, the task moves to shelved with a failure report. The planner later splits or re-specifies it.
10. **Record.** Metrics are appended. The planner updates the roadmap.

### 15.5 Merge gates

All are required. All are scripts.

1. Build on Linux, macOS, and Windows.
2. `cargo test` across the workspace.
3. `cargo clippy` with warnings denied. `cargo fmt` check.
4. Golden geometry suite.
5. Topological naming corpus.
6. Snapshot diffs under threshold.
7. Determinism check.
8. Performance within regression budget.
9. `cargo-deny`: licenses and advisories.
10. No new `unsafe` outside allowed crates.
11. No dependency direction violations between crates.
12. Public API changes match the spec. Checked by an API diff tool.
13. The change does not touch protected paths.
14. The builder's commits do not modify test or golden files written by the test author.
15. Test coverage on changed lines above a threshold.
16. Mutation testing sample on changed code. Tests must kill a minimum share of mutants. This catches tests that assert nothing.
17. Reviewer approval recorded.

### 15.6 Protected paths

Agents in the loop cannot modify:

1. CI configuration.
2. Gate scripts and the orchestrator.
3. The `cargo-deny` configuration and license allowlist.
4. `loop/` control files.
5. Branch protection settings.

Changes to these happen outside the loop. This is the one place the system is not self-modifying, and that is deliberate.

### 15.7 Architecture decisions without a human

1. When a task needs a decision not covered by `DECISIONS.md`, the planner writes a decision record with at least two options and tradeoffs.
2. The critic argues against the preferred option and checks it against existing decisions.
3. If the critic finds a conflict with a prior decision, the proposal is rejected.
4. Otherwise it is recorded and binding.
5. Reversing a decision requires a new record that names the old one and the evidence that changed.

### 15.8 UX quality without a human

1. The user simulator drives the built app through MCP and screenshots.
2. It runs a fixed script set, such as "model this bracket from the drawing" and "change the hole diameter".
3. It reports step counts, dead ends, errors, and confusing responses as issues.
4. Metrics are tracked over time: steps per task, error rate, time per task.
5. A regression in these metrics opens a task.

This is a proxy. It tests whether the app is usable by an agent, which correlates with clarity but is not the same as being pleasant for a person. See risk R6.

### 15.9 Auditor

Runs every N merges and at each phase boundary:

1. Checks the repo against `ARCHITECTURE.md`.
2. Looks for duplicated logic, dead code, oversized modules, and inconsistent naming.
3. Reviews all `unsafe` blocks.
4. Reviews shelved tasks for patterns.
5. Compares progress to phase exit criteria.
6. Files refactor tasks. These go through the normal loop.

### 15.10 Regression handling

1. A nightly job runs the full suite, long fuzz runs, and the complete reference corpus on main.
2. If it fails, a script bisects, reverts the offending merge, and reopens its task with the failure attached.

### 15.11 Guardrails

1. Each role runs in a sandbox with no secrets.
2. Network access is limited to package registries and the repo host.
3. New dependencies require a task that states why. `cargo-deny` and `cargo-vet` must pass.
4. Token, time, and cost budgets per task and per day. The orchestrator stops at the limit.
5. A loop-rate limit: if the shelve rate or revert rate crosses a threshold, the orchestrator halts and writes a report.
6. The `HALT` file stops everything.
7. Every merge links to its spec, tests, review, and gate results.

### 15.12 Bootstrapping

Phase 0 is done by hand because the loop needs gates to exist before it can be trusted. After Phase 0, the loop runs without approval steps. Reading the audit reports and the shelved list is optional but is where problems will appear first.

### 15.13 Parallelism

1. Several builder tasks can run at once if they touch different crates.
2. The planner tags each task with the crates it touches. The orchestrator schedules non-overlapping tasks together.
3. Merges are serialized through a queue. Each is re-verified against current main.

### 15.14 What the loop is expected to handle well and badly

**Well:** features with clear geometric acceptance tests, file importers and exporters, the MCP surface, plugins, documentation, refactors with good test coverage.

**Badly:** kernel robustness bugs with no clean reproduction, interaction design, visual polish, and anything that needs taste.

---

## 16. Security and supply chain

### 16.1 Threat model

| Threat | Mitigation |
|---|---|
| Malicious model file | Fuzzed parsers. Kernel in a separate process. Size and time limits. |
| Malicious plugin | WASM sandbox. Capabilities. Fuel and memory limits. |
| Malicious or confused agent over MCP | No code execution. Scoped file access. Transactions with user review. Session log. |
| Local process connecting to the MCP port | Loopback only. Session token. |
| Prompt injection through model metadata | Metadata returned as marked data. Never interpreted as instructions by the app. |
| Compromised dependency | `cargo-deny`, `cargo-audit`, `cargo-vet`, lockfile, pinned native builds. |
| Compromised loop agent | Sandbox. No secrets. Protected paths. Gates. |
| Tampered release | Reproducible builds where possible. Signed artifacts. Published checksums and SBOM. |

### 16.2 Supply chain practices

1. `Cargo.lock` is committed.
2. Advisory checks run on every merge and nightly.
3. New dependencies need justification and a license check.
4. Native libraries are built from pinned source in CI, never downloaded as binaries.
5. Releases include an SBOM.
6. Minimal dependency count is a stated goal. The auditor reports growth.

### 16.3 Privacy

1. No telemetry by default.
2. No network calls at runtime unless a plugin with network capability makes them.
3. Crash reports are written locally. The user chooses whether to share them.

---

## 17. Risks, open questions, and items to verify

### 17.1 Risks

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | OCCT fillets and booleans fail on real models | High | High | Process isolation, clear errors, golden corpus, healing passes, Rust backend as a long-term path |
| R2 | Topological naming design proves inadequate | Medium | High | Phase 0 spike, corpus and fuzz tests, fallback descriptions, no silent rebinding |
| R3 | Worker process overhead hurts interactivity | Medium | Medium | Measure in Phase 0. In-process previews as fallback. |
| R4 | Autonomous loop games its gates | High | High | Role separation, protected goldens, mutation testing, audits |
| R5 | Architecture drifts without human judgment | Medium | High | Binding decision log, critic, auditor, dependency direction gate |
| R6 | App is not intuitive for people | High | High | User simulator metrics. Accept that real user testing is the only true check. |
| R7 | Scope growth | High | Medium | Phase exit criteria. Non-goals list. Task size limits. |
| R8 | Rust OCCT bindings need heavy work | High | Medium | Own a thin C++ shim. Expose only what the trait needs. |
| R9 | Solver licensing or extraction problems | Medium | Medium | Solver trait. Rust-native solver in parallel. |
| R10 | UI toolkit limits polish | Medium | Medium | Thin UI layer. Review after Phase 1. |
| R11 | Loop cost exceeds value | Medium | Medium | Budgets, metrics, halt thresholds |
| R12 | Cross-platform OCCT builds are fragile | Medium | Medium | Pinned source builds in CI from Phase 0 |
| R13 | Dependence on a hosted model for development | Medium | Low | Model-agnostic loop configuration |

### 17.2 Open questions

1. Project name and file extension.
2. Whether previews run in-process or in workers. Decided by the Phase 0 spike.
3. Which pure Rust kernel to back for the experimental backend.
4. planegcs versus a Rust-native solver as the Phase 1 default.
5. Whether to expose all MCP tools directly or a core set plus discovery.
6. Hosting: GitHub, or a self-hosted forge for full FOSS infrastructure.
7. Whether real human UX testing gets added at phase boundaries despite the autonomy goal.

### 17.3 Items to verify before committing

These were not checked during research:

1. OCCT license terms and the dynamic linking requirement.
2. Licenses of truck, monstertruck, planegcs, and the SolveSpace solver.
3. Current state and license of wgpu, egui, iced, and Slint.
4. wasmtime component model maturity and WIT tooling.
5. The official Rust MCP SDK: name, license, spec coverage.
6. Availability and licenses of Rust crates for 3MF, DXF, and glTF.
7. FOSS 3D mouse support on all three platforms.
8. Mutation testing and API diff tooling for Rust.
9. Whether OCCT's history APIs cover every operation in the `Kernel` trait.
10. Whether FreeCAD headless is practical as a cross-check oracle in CI.

---

## 18. Sources

Research was done on 2026-11-13.

**Shapr3D**
- Shapr3D product site: https://www.shapr3d.com/
- Shapr3D, history-based parametric modeling: https://www.shapr3d.com/blog/shapr3d-history-based-parametric-modeling
- Shapr3D, industrial design and prototyping: https://www.shapr3d.com/features/prototyping
- Fabbaloo, "With History-Based Modeling, Shapr3D Checks Off Another Box": https://fabbaloo.com/?p=216783
- Engineering.com, same article: https://www.engineering.com/?p=51724

**Plasticity**
- Toolradar, Plasticity overview: https://toolradar.com/tools/plasticity

**Fusion 360**
- Autodesk forum, complaints thread: https://forums.autodesk.com/t5/fusion-support-forum/why-is-fusion-360-such-junk/m-p/13783010
- Autodesk forum, performance thread: https://forums.autodesk.com/t5/fusion-design-validate-document/fusion360-is-still-really-slow/m-p/9644644
- Autodesk forum, large assemblies thread: https://forums.autodesk.com/t5/fusion-design-validate-document/fusion-360-unable-to-handle-quot-large-quot-assemblies/m-p/7740612
- Autodesk support, slow performance article: https://autodesk.com/support/technical/article/How-to-optimize-settings-in-Fusion-360-for-performance
- G2, Autodesk Fusion reviews: https://www.g2.com/products/autodesk-fusion

**SolidWorks**
- Solid Solutions, "Why is SOLIDWORKS Crashing": https://www.solidsolutions.co.uk/why-is-solidworks-crashing/
- Hone, SolidWorks performance guide: https://hone.gg/blog/?p=3330

**FreeCAD**
- FreeCAD wiki, topological naming problem: https://wiki.freecad.org/Topological_naming_problem/en
- Hackaday, FreeCAD 1.0 released: https://hackaday.com/2024/11/20/freecad-version-1-0-released
- Hackaday, FreeCAD near 1.0: https://hackaday.com/2024/09/21/freecad-is-near-1-0
- AlternativeTo, FreeCAD 1.0 launch: https://alternativeto.net/news/2024/11/freecad-1-0-launches-with-enhanced-ui-ux-built-in-assembly-workbench-and-tnp-fixes

**Kernels**
- truck: https://github.com/ricosjp/truck
- arris, including notes on monstertruck and Fornjot: https://docs.rs/crate/arris/0.9.0
- monstertruck: https://docs.rs/monstertruck/0.3.2
- Fornjot: https://github.com/hannobraun/fornjot
- opencascade-rs: https://github.com/mkovaxx/opencascade-rs
- Tech Soft 3D, Parasolid: https://www.techsoft3d.com/products/parasolid

**CAD MCP servers**
- DEV Community roundup: https://dev.to/grove_chatforest/cad-3d-modeling-mcp-servers-blender-freecad-autocad-kicad-solidworks-and-more-20ja
- Fusion360 MCP server: https://glama.ai/mcp/servers/@faust-machines/fusion360-mcp-server
- Snyk, nine MCP servers for CAD: https://snyk.io/articles/9-mcp-servers-for-computer-aided-drafting-cad-with-ai
