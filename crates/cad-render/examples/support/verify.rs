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
use cad_render::{Camera, Frame, Image, OFFSCREEN_FORMAT, Renderer, Scene, wgpu};

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
    /// Uploading the mesh, including edge extraction.
    pub upload: Duration,
    /// Median of draw, submit, and wait.
    pub frame: Duration,
    /// One full offscreen render with colour and ID readback.
    pub render: Duration,
    /// The frame from that render.
    pub result: Frame,
    /// Triangles drawn.
    pub triangles: u32,
    /// Edge segments drawn.
    pub edges: u32,
    /// Consistency checks: determinism and single-pixel picking.
    pub checks: Vec<Check>,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Uploads a mesh, times frames, renders it offscreen twice, and picks sample pixels.
///
/// # Errors
///
/// Any render error.
pub fn run_gpu(
    renderer: &Renderer,
    mesh: &Mesh,
    camera: &Camera,
    width: u32,
    height: u32,
) -> DynResult<GpuRun> {
    let started = Instant::now();
    let scene = renderer.upload(mesh)?;
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
        upload,
        frame,
        render,
        result,
        triangles: scene.triangle_count(),
        edges: scene.edge_count(),
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
    /// Shaded render and error map side by side.
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

/// Runs one case end to end and prints its report. A mesh the GPU cannot take is a failed
/// check, not an error, so the remaining cases still run.
///
/// `forbidden` lists faces that must never reach the ID buffer.
///
/// # Errors
///
/// Reference errors for meshes that refer to missing vertices.
pub fn run_case(
    renderer: &Renderer,
    title: &str,
    mesh: &Mesh,
    camera: &Camera,
    size: (u32, u32),
    forbidden: &[u32],
) -> DynResult<CaseOutcome> {
    let (width, height) = size;
    let faces = {
        let mut f: Vec<u32> = mesh.triangle_faces.iter().map(|f| f.0).collect();
        f.sort_unstable();
        f.dedup();
        f.len()
    };
    println!("  case: {title}");
    println!(
        "    {} triangles, {} faces, {width} x {height} px, near {} mm, far {} mm",
        mesh.triangles.len(),
        faces,
        camera.near,
        camera.far
    );
    let gpu = match guarded(renderer, || run_gpu(renderer, mesh, camera, width, height)) {
        Ok(gpu) => gpu,
        Err(e) => {
            let checks = vec![Check::new(
                "the mesh uploads and draws",
                false,
                e.to_string(),
            )];
            print_checks(&checks);
            let mut panel = blank(3 * width + 2 * GUTTER, height + BAR);
            fill(&mut panel, 0, 0, u32::MAX, BAR - 2, FAIL_BAR);
            fill(&mut panel, 0, BAR, u32::MAX, height, [235, 210, 210, 255]);
            return Ok(CaseOutcome { checks, panel });
        }
    };
    let started = Instant::now();
    let reference = reference(mesh, camera, width, height)?;
    let reference_time = started.elapsed();
    let s = score(&gpu.result, &reference);

    println!(
        "    {} edge segments; upload {:.1} ms   frame median {:.2} ms   render + readback {:.1} ms   f64 reference {:.0} ms",
        gpu.edges,
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
    if !forbidden.is_empty() {
        let shown = gpu
            .result
            .ids
            .iter()
            .filter(|id| id.checked_sub(1).is_some_and(|f| forbidden.contains(&f)))
            .count();
        checks.push(Check::new(
            "zero-area faces never reach the ID buffer",
            shown == 0,
            format!("{shown} pixels pick a face with no area"),
        ));
    }
    checks.extend(gpu.checks);
    print_checks(&checks);
    let panel = panel(&gpu.result, &s, checks.iter().all(|c| c.passed));
    Ok(CaseOutcome { checks, panel })
}

/// Prints checks as PASS or FAIL lines.
pub fn print_checks(checks: &[Check]) {
    for c in checks {
        let tag = if c.passed { "PASS" } else { "FAIL" };
        println!("    {tag}  {}: {}", c.name, c.detail);
    }
}

const GUTTER: u32 = 8;
const BAR: u32 = 6;
const PAPER: [u8; 4] = [255, 255, 255, 255];

const PASS_BAR: [u8; 4] = [40, 160, 70, 255];
const FAIL_BAR: [u8; 4] = [210, 30, 30, 255];

/// Three images side by side under a status bar: the shaded picture, the ID buffer in false
/// colour (one colour per face, so coincident faces that share a shade become visible), and
/// an error map.
///
/// The error map is the picture washed out, with wrong pixels painted: red away from face
/// boundaries, orange at boundaries, magenta where coincident faces speckle. Marks are grown
/// to 3 x 3 so single pixels survive downscaling.
pub fn panel(frame: &Frame, s: &Score, passed: bool) -> Image {
    let image = &frame.image;
    let (w, h) = (image.width, image.height);
    let mut out = blank(3 * w + 2 * GUTTER, h + BAR);
    blit(&mut out, image, 0, BAR);

    let mut ids = image.clone();
    for (px, id) in ids.rgba.chunks_exact_mut(4).zip(&frame.ids) {
        px.copy_from_slice(&id_colour(*id));
    }
    blit(&mut out, &ids, w + GUTTER, BAR);

    let mut map = image.clone();
    for px in map.rgba.chunks_exact_mut(4) {
        for c in &mut px[..3] {
            *c = 255 - (255 - *c) / 3;
        }
    }
    let (wu, hu) = (w as usize, h as usize);
    for k in 0..wu * hu {
        let colour = match s.marks.get(k) {
            Some(1) => [220, 20, 20, 255],
            Some(2) => [255, 140, 0, 255],
            Some(3) => [200, 0, 200, 255],
            _ => continue,
        };
        let (x, y) = (k % wu, k / wu);
        for dy in y.saturating_sub(1)..(y + 2).min(hu) {
            for dx in x.saturating_sub(1)..(x + 2).min(wu) {
                let j = (dy * wu + dx) * 4;
                map.rgba[j..j + 4].copy_from_slice(&colour);
            }
        }
    }
    blit(&mut out, &map, 2 * (w + GUTTER), BAR);
    fill(
        &mut out,
        0,
        0,
        u32::MAX,
        BAR - 2,
        if passed { PASS_BAR } else { FAIL_BAR },
    );
    out
}

/// A stable, fairly saturated colour per face; light grey for background.
fn id_colour(id: u32) -> [u8; 4] {
    if id == 0 {
        return [236, 237, 240, 255];
    }
    let mut x = id.wrapping_mul(0x9e37_79b9);
    x ^= x >> 15;
    x = x.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    let channel = |shift: u32| 60 + ((x >> shift) & 0xff) as u8 % 170;
    [channel(0), channel(8), channel(16), 255]
}

/// An image of one colour.
pub fn blank(width: u32, height: u32) -> Image {
    Image {
        width,
        height,
        rgba: PAPER
            .iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 4)
            .collect(),
    }
}

fn fill(out: &mut Image, x0: u32, y0: u32, w: u32, h: u32, colour: [u8; 4]) {
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

/// Stacks panels vertically with a gutter between them.
pub fn stack(panels: &[Image]) -> Image {
    let width = panels.iter().map(|p| p.width).max().unwrap_or(1);
    let height = panels.iter().map(|p| p.height + GUTTER).sum::<u32>().max(1);
    let mut out = blank(width, height);
    let mut y = 0;
    for p in panels {
        blit(&mut out, p, 0, y);
        y += p.height + GUTTER;
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
