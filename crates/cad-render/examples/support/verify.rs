//! Checks GPU renders against a CPU reference rasterizer that works in double precision.
//!
//! Shared by the torture examples of `cad-render` and `cad-testkit`. The reference samples
//! pixel centres like the GPU does, clips at the near and far planes, interpolates depth
//! perspective-correctly, and keeps the nearest face per pixel. It uses the mesh's `f64`
//! positions, so it is the ground truth the GPU picture is held to.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use cad_kernel::Mesh;
use cad_math::Vec3;
use cad_render::{Camera, Frame, Image, OFFSCREEN_FORMAT, Renderer, Scene, ViewMode, wgpu};

/// Errors from the examples are reported, not unwrapped.
pub type DynResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Two depths closer than this, relative to the depth, are a tie: the geometry itself does
/// not say which face is in front.
const TIE: f64 = 1e-9;

/// Most interior pixels that may disagree with the reference, as a fraction of covered pixels.
pub const MAX_INTERIOR_WRONG: f64 = 0.0002;
/// Most pixels next to a face boundary that may disagree, as a fraction of covered pixels.
pub const MAX_BOUNDARY_WRONG: f64 = 0.005;
/// Most tied pixels that may show the minority face of their pair.
pub const MAX_SPECKLE: f64 = 0.01;

/// The reference picture: the face under every pixel centre.
pub struct Reference {
    width: u32,
    height: u32,
    /// Face index plus one, zero for background, as in [`Frame::ids`].
    ids: Vec<u32>,
    depth: Vec<f64>,
    /// For a tied pixel, the other face (plus one) at the same depth.
    tie_with: Vec<u32>,
}

impl Reference {
    fn new(width: u32, height: u32) -> Self {
        let n = width as usize * height as usize;
        Self {
            width,
            height,
            ids: vec![0; n],
            depth: vec![f64::INFINITY; n],
            tie_with: vec![0; n],
        }
    }

    fn covered(&self) -> usize {
        self.ids.iter().filter(|id| **id != 0).count()
    }

    fn sample(&mut self, k: usize, depth: f64, id: u32) {
        let best = self.depth[k];
        let close = (depth - best).abs() <= TIE * best.min(depth);
        if close {
            if id != self.ids[k] {
                self.tie_with[k] = self.tie_with[k].max(id);
                if depth < best {
                    self.depth[k] = depth;
                }
            }
        } else if depth < best {
            self.tie_with[k] = 0;
            self.depth[k] = depth;
            self.ids[k] = id;
        }
    }
}

/// A vertex in view space: `x` right, `y` up, `z` distance in front of the eye.
#[derive(Clone, Copy)]
struct ViewPoint {
    x: f64,
    y: f64,
    z: f64,
}

/// How far, in pixels, a sample may move and still count as the same sample. GPUs snap
/// vertices to a sub-pixel grid and work in single precision, so a pixel centre this close to
/// an edge may land on either side of it.
pub const JITTER_PX: f64 = 0.01;

/// The reference at the pixel centres, and which faces it shows when the sample points move
/// by [`JITTER_PX`] in each of four directions.
pub struct Truth {
    centre: Reference,
    /// Faces (plus one) at the four moved sample points.
    nearby: Vec<[u32; 4]>,
}

impl Truth {
    fn ambiguous(&self, k: usize) -> bool {
        self.nearby[k].iter().any(|id| *id != self.centre.ids[k])
    }
}

/// Rasterizes a mesh in double precision at the pixel centres and at four points
/// [`JITTER_PX`] away from them.
///
/// # Errors
///
/// Fails when the camera is degenerate or a triangle refers to a missing vertex.
pub fn reference(mesh: &Mesh, camera: &Camera, width: u32, height: u32) -> DynResult<Truth> {
    let centre = rasterize(mesh, camera, width, height, (0.0, 0.0))?;
    let mut nearby = vec![[0; 4]; centre.ids.len()];
    let j = JITTER_PX;
    for (slot, shift) in [(j, 0.0), (-j, 0.0), (0.0, j), (0.0, -j)]
        .into_iter()
        .enumerate()
    {
        let moved = rasterize(mesh, camera, width, height, shift)?;
        for (n, id) in nearby.iter_mut().zip(&moved.ids) {
            n[slot] = *id;
        }
    }
    Ok(Truth { centre, nearby })
}

/// Rasterizes a mesh in double precision with the camera and image size the GPU uses,
/// sampling each pixel at its centre plus `shift`.
fn rasterize(
    mesh: &Mesh,
    camera: &Camera,
    width: u32,
    height: u32,
    shift: (f64, f64),
) -> DynResult<Reference> {
    let forward = (camera.target - camera.eye)
        .normalized()
        .ok_or("eye and target coincide")?;
    let side = forward
        .cross(camera.up)
        .normalized()
        .ok_or("up is parallel to the view")?;
    let up = side.cross(forward);
    let (w, h) = (f64::from(width), f64::from(height));
    let tan_y = (camera.fov_y / 2.0).tan();
    let tan_x = tan_y * w / h;
    let (near, far) = (camera.near, camera.far);

    let mut out = Reference::new(width, height);
    for (triangle, face) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        let mut polygon = Vec::with_capacity(4);
        for &index in triangle {
            let p = *mesh
                .positions
                .get(index as usize)
                .ok_or_else(|| format!("vertex {index} does not exist"))?;
            let v = p - camera.eye;
            polygon.push(ViewPoint {
                x: v.dot(side),
                y: v.dot(up),
                z: v.dot(forward),
            });
        }
        if polygon
            .iter()
            .any(|v| !(v.x.is_finite() && v.y.is_finite() && v.z.is_finite()))
        {
            continue;
        }
        if polygon.iter().all(|v| v.z > far) {
            continue;
        }
        let clipped = clip_near(&polygon, near);
        // Screen position in pixels, and 1/z for perspective-correct depth.
        let screen: Vec<[f64; 3]> = clipped
            .iter()
            .map(|v| {
                [
                    (v.x / (v.z * tan_x) + 1.0) / 2.0 * w - shift.0,
                    (1.0 - v.y / (v.z * tan_y)) / 2.0 * h - shift.1,
                    1.0 / v.z,
                ]
            })
            .collect();
        for i in 1..screen.len().saturating_sub(1) {
            raster_triangle(
                &mut out,
                [screen[0], screen[i], screen[i + 1]],
                near,
                far,
                face.0 + 1,
            );
        }
    }
    Ok(out)
}

/// Sutherland-Hodgman clip of a convex polygon to `z >= near`.
fn clip_near(polygon: &[ViewPoint], near: f64) -> Vec<ViewPoint> {
    let mut out = Vec::with_capacity(polygon.len() + 1);
    for (i, &a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        let (a_in, b_in) = (a.z >= near, b.z >= near);
        if a_in {
            out.push(a);
        }
        if a_in != b_in {
            let t = (near - a.z) / (b.z - a.z);
            out.push(ViewPoint {
                x: a.x + t * (b.x - a.x),
                y: a.y + t * (b.y - a.y),
                z: near,
            });
        }
    }
    out
}

fn edge(a: [f64; 3], b: [f64; 3], x: f64, y: f64) -> f64 {
    (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
}

/// Visits only the pixel centres on each row's span of the triangle, so long slivers cost
/// what they cover rather than their bounding box.
fn raster_triangle(out: &mut Reference, t: [[f64; 3]; 3], near: f64, far: f64, id: u32) {
    let [a, b, c] = t;
    let area = edge(a, b, c[0], c[1]);
    if area == 0.0 || !area.is_finite() {
        return;
    }
    let (w, h) = (out.width, out.height);
    let ymin = a[1].min(b[1]).min(c[1]);
    let ymax = a[1].max(b[1]).max(c[1]);
    let row_lo = (ymin - 0.5).ceil().max(0.0);
    let row_hi = (ymax - 0.5).floor().min(f64::from(h) - 1.0);
    if row_lo > row_hi {
        return;
    }
    for row in row_lo as u32..=row_hi as u32 {
        let y = f64::from(row) + 0.5;
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for (p, q) in [(a, b), (b, c), (c, a)] {
            if (p[1] - y) * (q[1] - y) <= 0.0 {
                if p[1] == q[1] {
                    lo = lo.min(p[0].min(q[0]));
                    hi = hi.max(p[0].max(q[0]));
                } else {
                    let x = p[0] + (y - p[1]) * (q[0] - p[0]) / (q[1] - p[1]);
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
        }
        if lo > hi {
            continue;
        }
        let col_lo = (lo - 0.5).ceil().max(0.0) as i64 - 1;
        let col_hi = ((hi - 0.5).floor().min(f64::from(w) - 1.0)) as i64 + 1;
        for col in col_lo.max(0)..=col_hi.min(i64::from(w) - 1) {
            let x = col as f64 + 0.5;
            let l0 = edge(b, c, x, y) / area;
            let l1 = edge(c, a, x, y) / area;
            let l2 = edge(a, b, x, y) / area;
            if l0 < 0.0 || l1 < 0.0 || l2 < 0.0 {
                continue;
            }
            let depth = 1.0 / (l0 * a[2] + l1 * b[2] + l2 * c[2]);
            if !(depth >= near && depth <= far) {
                continue;
            }
            let k = row as usize * w as usize + col as usize;
            out.sample(k, depth, id);
        }
    }
}

/// How a GPU frame compares with the reference.
#[derive(Debug, Default)]
pub struct Score {
    /// Pixels the reference covers.
    pub covered_ref: usize,
    /// Pixels the GPU covers.
    pub covered_gpu: usize,
    /// Disagreeing pixels whose four neighbours all show the same face in the reference.
    pub interior_wrong: usize,
    /// Disagreeing pixels next to a face boundary.
    pub boundary_wrong: usize,
    /// Pixels where two faces are at the same depth.
    pub ties: usize,
    /// Tied pixels showing the less common face of their pair, or neither face.
    pub speckle: usize,
    /// Pixels within [`JITTER_PX`] of an edge where the GPU chose the other side.
    pub subpixel: usize,
    /// Per pixel: 0 fine, 1 interior wrong, 2 boundary wrong, 3 tie speckle.
    marks: Vec<u8>,
}

impl Score {
    fn fraction(&self, n: usize) -> f64 {
        n as f64 / self.covered_ref.max(1) as f64
    }
}

/// Compares a frame with the reference, pixel by pixel.
pub fn score(frame: &Frame, truth: &Truth) -> Score {
    let reference = &truth.centre;
    let (w, h) = (reference.width as usize, reference.height as usize);
    let mut s = Score {
        covered_ref: reference.covered(),
        covered_gpu: frame.ids.iter().filter(|id| **id != 0).count(),
        marks: vec![0; w * h],
        ..Score::default()
    };
    // Tied pixels, grouped by the pair of faces, with how often the GPU showed each.
    let mut pairs: BTreeMap<(u32, u32), [usize; 2]> = BTreeMap::new();
    for k in 0..w * h {
        let (r, g, other) = (reference.ids[k], frame.ids[k], reference.tie_with[k]);
        if truth.ambiguous(k) {
            // Within a hundredth of a pixel of an edge: any face seen nearby is right.
            if g != r {
                if truth.nearby[k].contains(&g) {
                    s.subpixel += 1;
                } else {
                    s.boundary_wrong += 1;
                    s.marks[k] = 2;
                }
            }
            continue;
        }
        if other != 0 {
            s.ties += 1;
            let pair = (r.min(other), r.max(other));
            let counts = pairs.entry(pair).or_default();
            if g == pair.0 {
                counts[0] += 1;
            } else if g == pair.1 {
                counts[1] += 1;
            } else {
                s.speckle += 1;
                s.marks[k] = 3;
            }
            continue;
        }
        if r == g {
            continue;
        }
        let (x, y) = (k % w, k / w);
        let neighbours = [
            (x > 0).then(|| k - 1),
            (x + 1 < w).then(|| k + 1),
            (y > 0).then(|| k - w),
            (y + 1 < h).then(|| k + w),
        ];
        let boundary = neighbours
            .into_iter()
            .flatten()
            .any(|n| reference.ids[n] != r || reference.tie_with[n] != 0);
        if boundary {
            s.boundary_wrong += 1;
            s.marks[k] = 2;
        } else {
            s.interior_wrong += 1;
            s.marks[k] = 1;
        }
    }
    // Within each pair, the less common face is the speckle.
    for k in 0..w * h {
        let (r, g, other) = (reference.ids[k], frame.ids[k], reference.tie_with[k]);
        if other == 0 || s.marks[k] != 0 || truth.ambiguous(k) {
            continue;
        }
        let pair = (r.min(other), r.max(other));
        let [first, second] = pairs.get(&pair).copied().unwrap_or_default();
        // On an even split, count the second face as the speckle.
        let minority = if first >= second { pair.1 } else { pair.0 };
        if g == minority {
            s.speckle += 1;
            s.marks[k] = 3;
        }
    }
    s
}

/// One pass or fail line in a report.
pub struct Check {
    /// What was checked.
    pub name: String,
    /// Whether it held.
    pub passed: bool,
    /// The measurement behind it.
    pub detail: String,
}

impl Check {
    /// A check from a condition.
    pub fn new(name: impl Into<String>, passed: bool, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed,
            detail: detail.into(),
        }
    }
}

/// The checks every rendered case gets from its score.
pub fn picture_checks(s: &Score) -> Vec<Check> {
    let mut checks = vec![
        Check::new(
            "something was drawn",
            s.covered_gpu > 0,
            format!("{} pixels covered", s.covered_gpu),
        ),
        Check::new(
            "face IDs match the f64 reference away from face boundaries",
            s.fraction(s.interior_wrong) <= MAX_INTERIOR_WRONG,
            format!(
                "{} wrong of {} covered ({:.4}%, limit {:.2}%)",
                s.interior_wrong,
                s.covered_ref,
                100.0 * s.fraction(s.interior_wrong),
                100.0 * MAX_INTERIOR_WRONG
            ),
        ),
        Check::new(
            "face IDs match the f64 reference at face boundaries",
            s.fraction(s.boundary_wrong) <= MAX_BOUNDARY_WRONG,
            format!(
                "{} wrong ({:.4}%, limit {:.2}%)",
                s.boundary_wrong,
                100.0 * s.fraction(s.boundary_wrong),
                100.0 * MAX_BOUNDARY_WRONG
            ),
        ),
    ];
    if s.ties > 0 {
        let speckle = s.speckle as f64 / s.ties as f64;
        checks.push(Check::new(
            "coincident faces show one face, not a speckle of both",
            speckle <= MAX_SPECKLE,
            format!(
                "{} of {} tied pixels show the minority face ({:.2}%, limit {:.0}%)",
                s.speckle,
                s.ties,
                100.0 * speckle,
                100.0 * MAX_SPECKLE
            ),
        ));
    }
    checks
}

/// Timings and consistency of the GPU path for one scene.
pub struct GpuRun {
    /// The uploaded mesh, for further views.
    pub scene: Scene,
    /// Uploading the mesh, including edge extraction.
    pub upload: Duration,
    /// Median of draw, submit, and wait.
    pub frame: Duration,
    /// One full offscreen render with colour and ID readback.
    pub render: Duration,
    /// The frame from that render, in the main view mode.
    pub result: Frame,
    /// Consistency checks: determinism and single-pixel picking.
    pub checks: Vec<Check>,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Uploads a mesh, times frames, renders it offscreen twice in the main view mode, and picks
/// sample pixels.
///
/// # Errors
///
/// Any render error.
pub fn run_gpu(
    renderer: &Renderer,
    mesh: &Mesh,
    colors: &[[u8; 3]],
    light: Option<&[f32]>,
    camera: &Camera,
    (width, height): (u32, u32),
    main: ViewMode,
) -> DynResult<GpuRun> {
    renderer.set_view_mode(main);
    renderer.set_wire_overlay(false);
    let started = Instant::now();
    let scene = match light {
        Some(light) => renderer.upload_baked(mesh, colors, light)?,
        None => renderer.upload_with_colors(mesh, colors)?,
    };
    renderer.finish()?;
    let upload = started.elapsed();

    let frame = time_frames(renderer, &scene, camera, width, height)?;
    let started = Instant::now();
    let result = renderer.render(&scene, camera, width, height)?;
    let render = started.elapsed();
    let again = renderer.render(&scene, camera, width, height)?;

    let mut checks = vec![Check::new(
        "two renders give identical pixels and IDs",
        again == result,
        if again == result {
            "identical".to_string()
        } else {
            let ids = result.ids.iter().zip(&again.ids).filter(|(a, b)| a != b);
            format!("{} IDs differ", ids.count())
        },
    )];

    // Renderer::pick reads one pixel from the ID buffer of a fresh draw.
    let targets = renderer.create_targets(width, height)?;
    let color = color_target(renderer, width, height);
    renderer.draw(&scene, camera, &color, &targets)?;
    let mut disagree = 0;
    let samples = 64u32;
    for i in 0..samples {
        // A spread of pixels that favours covered ones: walk a lattice, then a covered pixel.
        let x = (i * 7919 + 13) % width;
        let y = (i * 104_729 + 7) % height;
        let covered = result
            .ids
            .iter()
            .enumerate()
            .skip((i as usize * 7_368_787) % result.ids.len())
            .find(|(_, id)| **id != 0)
            .map(|(k, _)| (k as u32 % width, k as u32 / width));
        for (px, py) in [(x, y)].into_iter().chain(covered) {
            if renderer.pick(&targets, px, py)? != result.pick(px, py) {
                disagree += 1;
            }
        }
    }
    checks.push(Check::new(
        "Renderer::pick agrees with the full ID readback",
        disagree == 0,
        format!("{disagree} of {} sampled pixels disagree", samples * 2),
    ));

    Ok(GpuRun {
        scene,
        upload,
        frame,
        render,
        result,
        checks,
    })
}

fn color_target(renderer: &Renderer, width: u32, height: u32) -> wgpu::TextureView {
    renderer
        .device()
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("torture colour"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn time_frames(
    renderer: &Renderer,
    scene: &Scene,
    camera: &Camera,
    width: u32,
    height: u32,
) -> DynResult<Duration> {
    let targets = renderer.create_targets(width, height)?;
    let view = color_target(renderer, width, height);
    let frame = || -> DynResult<Duration> {
        let started = Instant::now();
        renderer.draw(scene, camera, &view, &targets)?;
        renderer.finish()?;
        Ok(started.elapsed())
    };
    for _ in 0..3 {
        frame()?;
    }
    let mut times = (0..30).map(|_| frame()).collect::<DynResult<Vec<_>>>()?;
    times.sort_unstable();
    Ok(times[times.len() / 2])
}

/// The outcome of one case: its report lines and its row of the demo image.
pub struct CaseOutcome {
    /// Every check, in order.
    pub checks: Vec<Check>,
    /// The case's row: the lit view with stats, and a column of technical views.
    pub panel: Image,
}

impl CaseOutcome {
    /// True when every check passed.
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }
}

/// Runs `work` with wgpu's validation and out-of-memory errors captured instead of
/// panicking in the default handler, and returns the first one as an error.
pub fn guarded<T>(renderer: &Renderer, work: impl FnOnce() -> DynResult<T>) -> DynResult<T> {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let device = renderer.device();
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = work();
    let mut errors = Vec::new();
    for scope in [validation, memory] {
        // Native wgpu resolves scopes immediately.
        let mut future = std::pin::pin!(scope.pop());
        if let Poll::Ready(Some(e)) = future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            errors.push(e.to_string());
        }
    }
    match (result, errors.is_empty()) {
        (Ok(value), true) => Ok(value),
        (Err(e), true) => Err(e),
        (_, false) => Err(format!("wgpu error: {}", errors.join("; ")).into()),
    }
}

/// Size of the main, lit view of a case. Checks run at this size.
pub const MAIN: (u32, u32) = (960, 600);
/// Size of each technical view beside it: four stack to the main view's height.
pub const INSET: (u32, u32) = (240, 150);

/// A technical view beside the main one.
#[derive(Clone, Copy)]
pub enum Inset {
    /// The scene in a view mode, from the case camera.
    View(ViewMode),
    /// The scene in a view mode from another camera, with a label.
    Zoom(ViewMode, Camera, &'static str),
    /// As `Zoom`, with every triangle outlined over the view.
    Wired(ViewMode, Camera, &'static str),
    /// A path-traced reference from the case camera, on the CPU: needs `Shot::trace`.
    PathTraced,
    /// Where the GPU disagrees with the double-precision reference.
    Errors,
    /// The picture rebuilt from picking alone: each pixel coloured by the face it picks.
    Picks,
}

/// One case to run.
pub struct Shot<'a> {
    /// Demo name, for the stats overlay.
    pub demo: &'a str,
    /// What the case is.
    pub title: &'a str,
    /// The mesh.
    pub mesh: &'a Mesh,
    /// Face colours, as for [`Renderer::upload_with_colors`].
    pub colors: &'a [[u8; 3]],
    /// The camera, including the clip planes under test.
    pub camera: &'a Camera,
    /// Faces that must never reach the ID buffer.
    pub forbidden: &'a [u32],
    /// The view mode of the main view, normally [`ViewMode::Lit`].
    pub main: ViewMode,
    /// Light baked per vertex, if any (see [`Renderer::upload_baked`]).
    pub light: Option<&'a [f32]>,
    /// Whether the lit view draws face boundary edges.
    pub edges: bool,
    /// The ray tracer and sky, for [`Inset::PathTraced`].
    pub trace: Option<(&'a cad_render::trace::Tracer, cad_render::trace::Sky)>,
    /// Three technical views; the error map always follows them.
    pub insets: [Inset; 3],
}

/// Runs one case end to end and prints its report. A mesh the GPU cannot take is a failed
/// check, not an error, so the remaining cases still run.
///
/// # Errors
///
/// Reference errors for meshes that refer to missing vertices, and render errors in the
/// technical views.
pub fn run_case(renderer: &Renderer, shot: &Shot) -> DynResult<CaseOutcome> {
    let (width, height) = MAIN;
    let mesh = shot.mesh;
    let faces = {
        let mut f: Vec<u32> = mesh.triangle_faces.iter().map(|f| f.0).collect();
        f.sort_unstable();
        f.dedup();
        f.len()
    };
    println!(
        "    {} triangles, {} faces, {width} x {height} px, near {} mm, far {} mm",
        mesh.triangles.len(),
        faces,
        shot.camera.near,
        shot.camera.far
    );
    let gpu = match guarded(renderer, || {
        renderer.set_edges(shot.edges);
        run_gpu(
            renderer,
            mesh,
            shot.colors,
            shot.light,
            shot.camera,
            MAIN,
            shot.main,
        )
    }) {
        Ok(gpu) => gpu,
        Err(e) => {
            let checks = vec![Check::new(
                "the mesh uploads and draws",
                false,
                e.to_string(),
            )];
            print_checks(&checks);
            let mut panel = frame_for(shot, false);
            crate::text::label(&mut panel, 20, 20, &format!("FAILED: {e}"));
            return Ok(CaseOutcome { checks, panel });
        }
    };
    let started = Instant::now();
    let reference = reference(mesh, shot.camera, width, height)?;
    let reference_time = started.elapsed();
    let s = score(&gpu.result, &reference);

    println!(
        "    {} edge segments; upload {:.1} ms   frame median {:.2} ms   render + readback {:.1} ms   f64 reference {:.0} ms",
        gpu.scene.edge_count(),
        ms(gpu.upload),
        ms(gpu.frame),
        ms(gpu.render),
        ms(reference_time)
    );
    println!(
        "    covered: GPU {} px, reference {} px; tied {} px; {} px differ only within {JITTER_PX} px of an edge",
        s.covered_gpu, s.covered_ref, s.ties, s.subpixel
    );
    let mut checks = picture_checks(&s);
    if !shot.forbidden.is_empty() {
        let shown = gpu
            .result
            .ids
            .iter()
            .filter(|id| {
                id.checked_sub(1)
                    .is_some_and(|f| shot.forbidden.contains(&f))
            })
            .count();
        checks.push(Check::new(
            "zero-area faces never reach the ID buffer",
            shown == 0,
            format!("{shown} pixels pick a face with no area"),
        ));
    }
    checks.extend(
        gpu.checks
            .iter()
            .map(|c| Check::new(&c.name, c.passed, &c.detail)),
    );
    print_checks(&checks);

    let passed = checks.iter().all(|c| c.passed);
    let failed = checks.iter().filter(|c| !c.passed).count();
    let mut panel = frame_for(shot, passed);
    let mut main = gpu.result.image.clone();
    let wrong = s.interior_wrong + s.boundary_wrong + s.speckle;
    let lines = [
        format!("{} / {}", shot.demo, shot.title),
        format!(
            "TRIANGLES {}   FACES {}",
            crate::text::thousands(mesh.triangles.len() as u64),
            crate::text::thousands(faces as u64)
        ),
        format!(
            "FRAME {:.1} MS   UPLOAD {}",
            ms(gpu.frame),
            if gpu.upload.as_secs_f64() >= 1.0 {
                format!("{:.1} S", gpu.upload.as_secs_f64())
            } else {
                format!("{:.0} MS", ms(gpu.upload))
            }
        ),
        format!(
            "PICKING {} WRONG OF {} PX",
            crate::text::thousands(wrong as u64),
            crate::text::thousands(s.covered_ref as u64)
        ),
    ];
    stats(&mut main, &lines, passed, failed);
    blit(&mut panel, &main, MARGIN, MARGIN);

    let insets = shot.insets.iter().copied().chain([Inset::Errors]);
    for (i, inset) in insets.enumerate() {
        let (image, name) = inset_image(renderer, &gpu, shot, &s, inset)?;
        let y = MARGIN + i as u32 * INSET.1;
        let x = MARGIN + width + MARGIN;
        let mut image = image;
        crate::text::label(&mut image, 6, 6, name);
        blit(&mut panel, &image, x, y);
        // Hairline between the stacked views.
        if i > 0 {
            fill(&mut panel, x, y, INSET.0, 1, FRAME);
        }
    }
    renderer.set_view_mode(ViewMode::Lit);
    renderer.set_edges(true);
    Ok(CaseOutcome { checks, panel })
}

fn inset_image(
    renderer: &Renderer,
    gpu: &GpuRun,
    shot: &Shot,
    s: &Score,
    inset: Inset,
) -> DynResult<(Image, &'static str)> {
    let (w, h) = INSET;
    Ok(match inset {
        Inset::View(mode) => {
            renderer.set_view_mode(mode);
            let frame = renderer.render(&gpu.scene, shot.camera, w, h)?;
            (frame.image, label_of(mode))
        }
        Inset::Zoom(mode, camera, name) => {
            renderer.set_view_mode(mode);
            (renderer.render(&gpu.scene, &camera, w, h)?.image, name)
        }
        Inset::Wired(mode, camera, name) => {
            renderer.set_view_mode(mode);
            renderer.set_wire_overlay(true);
            let image = renderer.render(&gpu.scene, &camera, w, h)?.image;
            renderer.set_wire_overlay(false);
            (image, name)
        }
        Inset::PathTraced => {
            let (bvh, sky) = shot
                .trace
                .ok_or("a path-traced view needs a ray tracer in the shot")?;
            let started = Instant::now();
            let image = cad_render::trace::path_trace(
                shot.mesh,
                shot.colors,
                bvh,
                shot.camera,
                (w, h),
                sky,
                48,
                2,
            );
            println!(
                "    path traced {w} x {h} at 48 samples per pixel in {:.0} ms",
                ms(started.elapsed())
            );
            (image, "PATH TRACED (CPU)")
        }
        Inset::Errors => (error_map(&gpu.result.image, s), "ERRORS VS F64 REF"),
        Inset::Picks => {
            let source = &gpu.result;
            let mut rebuilt = blank(source.image.width, source.image.height, [2, 2, 3, 255]);
            for (px, id) in rebuilt.rgba.chunks_exact_mut(4).zip(&source.ids) {
                if let Some(face) = id.checked_sub(1) {
                    let [r, g, b] = shot
                        .colors
                        .get(face as usize)
                        .copied()
                        .unwrap_or(cad_render::DEFAULT_FACE_COLOR);
                    px.copy_from_slice(&[r, g, b, 255]);
                }
            }
            (downscale(&rebuilt, w, h), "REBUILT FROM PICKS")
        }
    })
}

fn label_of(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Lit => "LIT",
        ViewMode::Unlit => "UNLIT",
        ViewMode::Wireframe => "WIREFRAME",
        ViewMode::Triangles => "TRIANGLES",
        ViewMode::Faces => "FACES (ID BUFFER)",
        ViewMode::Normals => "WORLD NORMAL",
        ViewMode::Lighting => "LIGHTING ONLY",
        ViewMode::Depth => "SCENE DEPTH",
        ViewMode::Overdraw => "OVERDRAW",
    }
}

/// Stats in the corner of the main view, in the manner of an engine's `stat unit` overlay.
fn stats(image: &mut Image, lines: &[String], passed: bool, failed: usize) {
    let scale = 2;
    let line_h = crate::text::height(scale) + 6;
    let widest = lines
        .iter()
        .map(|l| crate::text::width(l, scale))
        .max()
        .unwrap_or(0)
        .min(image.width - 24);
    let box_h = (lines.len() as u32 + 1) * line_h + 12;
    crate::text::shade(image, 12, 12, widest + 20, box_h, 0.72);
    for (i, line) in lines.iter().enumerate() {
        let color = if i == 0 {
            [255, 255, 255, 255]
        } else {
            [140, 235, 140, 255]
        };
        let fits: String = line
            .chars()
            .take(((image.width - 44) / (6 * scale)) as usize)
            .collect();
        crate::text::draw(image, 22, 20 + i as u32 * line_h, scale, color, &fits);
    }
    let (verdict, color) = if passed {
        ("PASS".to_string(), [90, 230, 110, 255])
    } else {
        (format!("FAIL: {failed} CHECKS"), [255, 90, 80, 255])
    };
    crate::text::draw(
        image,
        22,
        20 + lines.len() as u32 * line_h,
        scale,
        color,
        &verdict,
    );
}

/// The lit picture washed out, with wrong pixels painted: red away from face boundaries,
/// orange at boundaries, magenta where coincident faces speckle. Scaled to an inset, any
/// wrong pixel in a block marks the block.
fn error_map(image: &Image, s: &Score) -> Image {
    let (w, h) = INSET;
    let mut out = downscale(image, w, h);
    for px in out.rgba.chunks_exact_mut(4) {
        for c in &mut px[..3] {
            *c = (u16::from(*c) / 4 + 20) as u8;
        }
    }
    let (sx, sy) = (image.width / w, image.height / h);
    for y in 0..h {
        for x in 0..w {
            let mut worst = 0u8;
            for dy in 0..sy {
                for dx in 0..sx {
                    let k = ((y * sy + dy) * image.width + x * sx + dx) as usize;
                    worst = worst.max(s.marks.get(k).copied().unwrap_or(0));
                }
            }
            let color = match worst {
                1 => [255, 40, 40, 255],
                2 => [255, 150, 0, 255],
                3 => [230, 60, 255, 255],
                _ => continue,
            };
            let k = ((y * w + x) * 4) as usize;
            out.rgba[k..k + 4].copy_from_slice(&color);
        }
    }
    out
}

/// Box-filters an image down by a whole factor in each direction.
pub fn downscale(image: &Image, w: u32, h: u32) -> Image {
    let (sx, sy) = ((image.width / w).max(1), (image.height / h).max(1));
    let mut out = blank(w, h, [0, 0, 0, 255]);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            for dy in 0..sy {
                for dx in 0..sx {
                    if let Some(p) = image.pixel(x * sx + dx, y * sy + dy) {
                        for (total, channel) in sum.iter_mut().zip(p) {
                            *total += u32::from(channel);
                        }
                    }
                }
            }
            let n = sx * sy;
            let k = ((y * w + x) * 4) as usize;
            for (slot, total) in out.rgba[k..k + 3].iter_mut().zip(sum) {
                *slot = (total / n) as u8;
            }
        }
    }
    out
}

/// Prints checks as PASS or FAIL lines.
pub fn print_checks(checks: &[Check]) {
    for c in checks {
        let tag = if c.passed { "PASS" } else { "FAIL" };
        println!("    {tag}  {}: {}", c.name, c.detail);
    }
}

/// Space around and between views.
pub const MARGIN: u32 = 6;
/// Editor-style dark frame around views.
pub const FRAME: [u8; 4] = [24, 25, 28, 255];
const PASS_BAR: [u8; 4] = [60, 200, 90, 255];
const FAIL_BAR: [u8; 4] = [230, 60, 50, 255];

/// The dark frame for one case row, with a status stripe down its left side.
fn frame_for(shot: &Shot, passed: bool) -> Image {
    let _ = shot;
    let (w, h) = MAIN;
    let mut out = blank(MARGIN * 3 + w + INSET.0, MARGIN * 2 + h, FRAME);
    fill(
        &mut out,
        0,
        0,
        3,
        u32::MAX,
        if passed { PASS_BAR } else { FAIL_BAR },
    );
    out
}

/// An image of one colour.
pub fn blank(width: u32, height: u32, color: [u8; 4]) -> Image {
    Image {
        width,
        height,
        rgba: color
            .iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 4)
            .collect(),
    }
}

/// Fills a rectangle, clipped to the image.
pub fn fill(out: &mut Image, x0: u32, y0: u32, w: u32, h: u32, colour: [u8; 4]) {
    for y in y0..y0.saturating_add(h).min(out.height) {
        for x in x0..x0.saturating_add(w).min(out.width) {
            let j = (y as usize * out.width as usize + x as usize) * 4;
            out.rgba[j..j + 4].copy_from_slice(&colour);
        }
    }
}

/// Copies `src` into `out` with its top left corner at `(x0, y0)`, clipped to `out`.
pub fn blit(out: &mut Image, src: &Image, x0: u32, y0: u32) {
    for y in 0..src.height {
        let ty = y0 + y;
        if ty >= out.height {
            break;
        }
        let cols = src.width.min(out.width.saturating_sub(x0)) as usize;
        let from = y as usize * src.width as usize * 4;
        let to = (ty as usize * out.width as usize + x0 as usize) * 4;
        out.rgba[to..to + cols * 4].copy_from_slice(&src.rgba[from..from + cols * 4]);
    }
}

/// Stacks images vertically on the dark frame colour.
pub fn stack(panels: &[Image]) -> Image {
    let width = panels.iter().map(|p| p.width).max().unwrap_or(1);
    let height = panels.iter().map(|p| p.height).sum::<u32>().max(1);
    let mut out = blank(width, height, FRAME);
    let mut y = 0;
    for p in panels {
        blit(&mut out, p, 0, y);
        y += p.height;
    }
    out
}

/// A camera from an eye and target, with Z up.
pub fn look(eye: Vec3, target: Vec3, near: f64, far: f64) -> Camera {
    Camera {
        eye,
        target,
        up: Vec3::Z,
        fov_y: Camera::DEFAULT_FOV_Y,
        near,
        far,
    }
}
