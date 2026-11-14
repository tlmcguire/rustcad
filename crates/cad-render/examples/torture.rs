//! Torture tests for the renderer: meshes of up to 16 million triangles, half a million
//! separate bodies, extreme depth ranges, coincident faces, slivers, thin walls, and picking.
//!
//! **These are deliberately heavy.** The largest cases need several GB of memory and keep the
//! GPU busy for seconds at a time.
//!
//! Headless (the default), every case renders offscreen and is checked pixel by pixel against
//! a CPU reference rasterizer in double precision (`support/verify.rs`). Each demo prints what
//! it measured and writes `torture-<demo>.png`: one row per case with the shaded picture, the
//! face ID buffer in false colour, and an error map. The run exits with an error if any check
//! failed.
//!
//! With `--window`, the first demo named opens in a window instead: drag to orbit, scroll to
//! zoom, click to print the face under the cursor, left and right arrows to change case.
//!
//! Usage: `cargo run --release -p cad-render --example torture -- [demo ...] [--out DIR] [--window]`
//!
//! Demos: `dense`, `bodies`, `depth`, `zfight`, `slivers`, `thin`, `picking`, or `all`
//! (the default). Images go to `target/torture` unless `--out` says otherwise.

use std::path::PathBuf;

use cad_kernel::{FaceIndex, Mesh};
use cad_math::Vec3;
use cad_render::{Camera, RenderError, Renderer};

#[path = "support/verify.rs"]
mod verify;
#[path = "support/viewer.rs"]
mod viewer;

use verify::{Check, DynResult, look, print_checks, run_case, stack};

const DEMOS: [&str; 7] = [
    "dense", "bodies", "depth", "zfight", "slivers", "thin", "picking",
];
/// Image size of one case.
const SIZE: (u32, u32) = (640, 400);

const WARNING: &str = "\
WARNING: these are torture tests for the renderer. The largest cases build meshes of up to
16 million triangles, need several GB of memory, and keep the GPU fully busy for seconds at a
time. Your machine may become sluggish while they run. Close other heavy applications first,
and on machines with less than 8 GB of memory run one demo at a time. Press Ctrl+C to stop.";

/// One scene to render. The mesh is built only when the case runs, so large cases do not
/// all sit in memory at once.
pub struct Case {
    /// What the case is.
    pub title: String,
    /// The camera, including the clip planes under test.
    pub camera: Camera,
    /// Builds the mesh.
    pub build: Box<dyn Fn() -> Mesh>,
    /// Faces that must never reach the ID buffer.
    pub forbidden: Vec<u32>,
}

impl Case {
    fn new(title: &str, camera: Camera, build: impl Fn() -> Mesh + 'static) -> Self {
        Self {
            title: title.to_string(),
            camera,
            build: Box::new(build),
            forbidden: Vec::new(),
        }
    }
}

/// Checks that need no picture.
type ExtraChecks = fn(&Renderer) -> DynResult<Vec<Check>>;

/// A named group of cases.
struct Demo {
    cases: Vec<Case>,
    /// Stop after the first failing case: the later ones are larger versions of it.
    ramp: bool,
    /// Checks that need no picture.
    extra: Option<ExtraChecks>,
}

impl Demo {
    fn of(cases: Vec<Case>) -> Self {
        Self {
            cases,
            ramp: false,
            extra: None,
        }
    }
}

fn demo(name: &str) -> DynResult<Demo> {
    match name {
        "dense" => dense(),
        "bodies" => bodies(),
        "depth" => depth(),
        "zfight" => zfight(),
        "slivers" => slivers(),
        "thin" => thin(),
        _ => picking(),
    }
}

fn main() -> DynResult<()> {
    let mut out = PathBuf::from("target/torture");
    let mut chosen = Vec::new();
    let mut window = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().ok_or("--out needs a directory")?.into(),
            "--window" => window = true,
            "all" => chosen.extend(DEMOS.map(String::from)),
            name if DEMOS.contains(&name) => chosen.push(arg),
            other => {
                return Err(format!(
                    "unknown argument {other:?}; expected demos from {DEMOS:?}, all, --out DIR, or --window"
                )
                .into());
            }
        }
    }
    if chosen.is_empty() {
        chosen.extend(DEMOS.map(String::from));
    }
    eprintln!("{WARNING}\n");

    if window {
        let name = &chosen[0];
        return viewer::run(name, demo(name)?.cases);
    }

    std::fs::create_dir_all(&out)?;
    let renderer = Renderer::headless()?;
    println!("adapter: {}", renderer.adapter());
    println!(
        "largest vertex buffer the device allows: {} MiB",
        renderer.device().limits().max_buffer_size >> 20
    );
    let mut failed = Vec::new();
    for name in &chosen {
        println!("\n== {name} ==");
        let demo = demo(name)?;
        let mut checks = Vec::new();
        let mut panels = Vec::new();
        for case in &demo.cases {
            let mesh = (case.build)();
            let outcome = run_case(
                &renderer,
                &case.title,
                &mesh,
                &case.camera,
                SIZE,
                &case.forbidden,
            )?;
            drop(mesh);
            let passed = outcome.passed();
            checks.extend(outcome.checks);
            panels.push(outcome.panel);
            if demo.ramp && !passed {
                println!("  stopping the ramp: the remaining cases are larger");
                break;
            }
        }
        if let Some(extra) = demo.extra {
            println!("  case: invalid input");
            let more = extra(&renderer)?;
            print_checks(&more);
            checks.extend(more);
        }
        let path = out.join(format!("torture-{name}.png"));
        stack(&panels).write_png(&path)?;
        println!("  wrote {}", path.display());
        let n = checks.iter().filter(|c| !c.passed).count();
        if n > 0 {
            failed.push(format!("{name}: {n} failed"));
        }
    }

    println!();
    if failed.is_empty() {
        println!("all torture checks passed");
        Ok(())
    } else {
        for f in &failed {
            eprintln!("FAILED {f}");
        }
        Err(format!("torture checks failed: {}", failed.join(", ")).into())
    }
}

/// Tori of 1 to 16 million triangles, faces of 50 x 50 cells.
fn dense() -> DynResult<Demo> {
    let camera = Camera::framing(Vec3::ZERO, 85.0, Vec3::new(1.0, -1.0, 1.1))?;
    let steps = [
        (1000, 500, "1 million"),
        (2000, 500, "2 million"),
        (2000, 1000, "4 million"),
        (4000, 1000, "8 million"),
        (4000, 2000, "16 million"),
    ];
    let cases = steps
        .into_iter()
        .map(|(major, minor, label)| {
            Case::new(
                &format!("torus of {label} triangles ({major} x {minor} segments)"),
                camera,
                move || torus(60.0, 22.0, major, minor, 50),
            )
        })
        .collect();
    Ok(Demo {
        ramp: true,
        ..Demo::of(cases)
    })
}

/// Lattices of separate cubes of varied size, six faces each: up to 512,000 bodies.
fn bodies() -> DynResult<Demo> {
    let lattice = |n: u32| {
        move || {
            let mut b = Builder::default();
            let mut seed = 0x2545_f491_u64;
            let mut face = 0;
            for k in 0..n {
                for j in 0..n {
                    for i in 0..n {
                        let size = 0.6 + 1.2 * unit(&mut seed);
                        let origin = Vec3::new(f64::from(i), f64::from(j), f64::from(k)) * 2.0;
                        b.cube(origin, Vec3::new(size, size, size), face);
                        face += 6;
                    }
                }
            }
            b.mesh
        }
    };
    let frame = |n: u32| {
        let half = f64::from(n);
        Camera::framing(
            Vec3::new(half, half, half),
            half * 1.75,
            Vec3::new(1.0, -1.3, 0.9),
        )
    };
    Ok(Demo::of(vec![
        Case::new(
            "64,000 cubes: 768,000 triangles, 384,000 faces",
            frame(40)?,
            lattice(40),
        ),
        Case::new(
            "512,000 cubes: 6.1 million triangles, 3.1 million faces",
            frame(80)?,
            lattice(80),
        ),
    ]))
}

/// Tiny parts near the eye, huge ones far away, and a part far from the origin.
fn depth() -> DynResult<Demo> {
    let scene = || {
        let mut b = Builder::default();
        // A 1 mm cube 30 mm in front of the eye.
        b.cube(Vec3::new(-4.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 1.0), 0);
        // A 6 m wall 10 m away, with a checker of panels 10 mm in front of it.
        let s = 3000.0;
        b.quad(
            [
                Vec3::new(-s, 10_000.0, -s),
                Vec3::new(s, 10_000.0, -s),
                Vec3::new(s, 10_000.0, s),
                Vec3::new(-s, 10_000.0, s),
            ],
            -Vec3::Y,
            6,
        );
        for j in 0..8 {
            for i in 0..8 {
                if (i + j) % 2 == 0 {
                    let (x, z) = (-s + f64::from(i) * 750.0, -s + f64::from(j) * 750.0);
                    b.quad(
                        [
                            Vec3::new(x, 9_990.0, z),
                            Vec3::new(x + 750.0, 9_990.0, z),
                            Vec3::new(x + 750.0, 9_990.0, z + 750.0),
                            Vec3::new(x, 9_990.0, z + 750.0),
                        ],
                        -Vec3::Y,
                        7,
                    );
                }
            }
        }
        // A 200 m ground plane 100 mm below the eye, extending behind it.
        let g = 100_000.0;
        b.quad(
            [
                Vec3::new(-g, -g, -100.0),
                Vec3::new(g, -g, -100.0),
                Vec3::new(g, g, -100.0),
                Vec3::new(-g, g, -100.0),
            ],
            Vec3::Z,
            8,
        );
        b.mesh
    };
    let eye = Vec3::new(0.0, -30.0, 0.0);
    let target = Vec3::new(0.0, 10_000.0, -400.0);

    // A part 1 km from the origin: three 0.2 mm cubes 0.05 mm apart on a 0.1 mm plate.
    let far = Vec3::new(1.0e6, 0.0, 0.0);
    let offset = move || {
        let mut b = Builder::default();
        b.cube(
            far + Vec3::new(-1.0, -1.0, -0.1),
            Vec3::new(2.0, 2.0, 0.1),
            0,
        );
        for i in 0..3 {
            let x = -0.35 + f64::from(i) * 0.25;
            b.cube(
                far + Vec3::new(x, -0.1, 0.0),
                Vec3::new(0.2, 0.2, 0.2),
                6 + 6 * i,
            );
        }
        b.mesh
    };

    Ok(Demo::of(vec![
        Case::new(
            "near 0.01 mm, far 1,000 m: cube at 30 mm, panels 10 mm off a wall at 10 m",
            look(eye, target, 0.01, 1.0e6),
            scene,
        ),
        Case::new(
            "same scene, near 10 mm",
            look(eye, target, 10.0, 1.0e6),
            scene,
        ),
        Case::new(
            "0.2 mm cubes 1 km from the origin",
            Camera::framing(far, 1.2, Vec3::new(1.0, -1.0, 0.8))?,
            offset,
        ),
    ]))
}

/// Two 100 mm plates of different bodies, `gap` apart.
fn plates(gap: f64, flip_second: bool) -> Mesh {
    let mut b = Builder::default();
    let mut plate = |z: f64, n: u32, flip: bool, face: u32| {
        b.grid(
            Vec3::new(-50.0, -50.0, z),
            Vec3::new(100.0, 0.0, 0.0),
            Vec3::new(0.0, 100.0, 0.0),
            (n, n),
            Vec3::Z,
            flip,
            |_, _| face,
        );
    };
    plate(0.0, 20, false, 0);
    if flip_second {
        plate(gap, 7, true, 1);
    } else {
        plate(gap, 20, false, 1);
    }
    b.mesh
}

/// Coplanar and nearly coplanar faces of different bodies.
fn zfight() -> DynResult<Demo> {
    let camera = Camera::framing(Vec3::ZERO, 75.0, Vec3::new(1.0, -0.8, 0.9))?;
    Ok(Demo::of(vec![
        Case::new("coincident faces, same triangulation", camera, || {
            plates(0.0, false)
        }),
        Case::new("coincident faces, different triangulation", camera, || {
            plates(0.0, true)
        }),
        Case::new("second face 0.001 mm above the first", camera, || {
            plates(1e-3, true)
        }),
        Case::new("second face 0.00001 mm above the first", camera, || {
            plates(1e-5, true)
        }),
    ]))
}

/// Slivers, needles, degenerate triangles, and invalid input.
fn slivers() -> DynResult<Demo> {
    // A disk of 200,000 fan triangles: 50 mm long, 1.6 micrometres wide at the rim.
    let fan = || {
        let mut b = Builder::default();
        let n = 200_000u32;
        let centre = b.vertex(Vec3::ZERO, Vec3::Z);
        let rim: Vec<u32> = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / f64::from(n);
                b.vertex(Vec3::new(50.0 * a.cos(), 50.0 * a.sin(), 0.0), Vec3::Z)
            })
            .collect();
        for i in 0..n {
            b.tri(
                [centre, rim[i as usize], rim[((i + 1) % n) as usize]],
                i / (n / 8),
            );
        }
        b.mesh
    };
    // A 100 x 60 mm sheet of 600,000 needles, 0.0002 mm wide: aspect ratio 500,000.
    let needles = || {
        let mut b = Builder::default();
        let strips = 300_000u32;
        let mut left = b.vertex(Vec3::new(-50.0, -30.0, 0.0), Vec3::Z);
        let mut right = b.vertex(Vec3::new(50.0, -30.0, 0.0), Vec3::Z);
        for i in 1..=strips {
            let y = -30.0 + 60.0 * f64::from(i) / f64::from(strips);
            let l = b.vertex(Vec3::new(-50.0, y, 0.0), Vec3::Z);
            let r = b.vertex(Vec3::new(50.0, y, 0.0), Vec3::Z);
            let face = (i - 1) / (strips / 6);
            b.tri([left, right, r], face);
            b.tri([left, r, l], face);
            (left, right) = (l, r);
        }
        b.mesh
    };
    // A plate behind 30,000 zero-area triangles (faces 1 and 2) that must never show.
    let degenerate = || {
        let mut b = Builder::default();
        b.grid(
            Vec3::new(-50.0, -50.0, 0.0),
            Vec3::new(100.0, 0.0, 0.0),
            Vec3::new(0.0, 100.0, 0.0),
            (4, 4),
            Vec3::Z,
            false,
            |_, _| 0,
        );
        let mut seed = 7u64;
        for i in 0..10_000u32 {
            let p = Vec3::new(
                100.0 * unit(&mut seed) - 50.0,
                100.0 * unit(&mut seed) - 50.0,
                5.0,
            );
            let d = Vec3::new(unit(&mut seed) - 0.5, unit(&mut seed) - 0.5, 0.0) * 20.0;
            let v0 = b.vertex(p, Vec3::Z);
            let v1 = b.vertex(p + d, Vec3::Z);
            let v2 = b.vertex(p + d * 2.0, Vec3::Z);
            b.tri([v0, v1, v2], 1); // collinear
            b.tri([v0, v0, v1], 1); // repeated index
            let same = b.vertex(p, Vec3::Z);
            b.tri([v0, same, same], 1 + i % 2); // three equal points
        }
        b.mesh
    };
    let camera = Camera::framing(Vec3::ZERO, 55.0, Vec3::new(0.6, -0.8, 1.2))?;
    let mut zero_area = Case::new(
        "plate behind 30,000 zero-area triangles",
        camera,
        degenerate,
    );
    zero_area.forbidden = vec![1, 2];
    Ok(Demo {
        extra: Some(invalid_input),
        ..Demo::of(vec![
            Case::new("disk of 200,000 fan slivers", camera, fan),
            Case::new("sheet of 600,000 needles", camera, needles),
            zero_area,
        ])
    })
}

/// What the renderer does with meshes that cannot be drawn correctly.
fn invalid_input(renderer: &Renderer) -> DynResult<Vec<Check>> {
    let quad = || {
        let mut b = Builder::default();
        b.quad(
            [
                Vec3::new(-10.0, -10.0, 0.0),
                Vec3::new(10.0, -10.0, 0.0),
                Vec3::new(10.0, 10.0, 0.0),
                Vec3::new(-10.0, 10.0, 0.0),
            ],
            Vec3::Z,
            0,
        );
        b.mesh
    };
    let rejected = |name: &str, mesh: Mesh| {
        let result = renderer.upload(&mesh);
        Check::new(
            name,
            matches!(result, Err(RenderError::InvalidMesh(_))),
            match result {
                Err(e) => format!("rejected: {e}"),
                Ok(_) => "accepted by upload".to_string(),
            },
        )
    };
    let mut nan = quad();
    nan.positions[2].x = f64::NAN;
    let mut inf = quad();
    inf.positions[2].z = f64::INFINITY;
    let mut out_of_range = quad();
    out_of_range.triangles[1][2] = 99;
    let mut checks = vec![
        rejected("an empty mesh is rejected", Mesh::default()),
        rejected("a triangle with a missing vertex is rejected", out_of_range),
        rejected("a NaN vertex position is rejected", nan),
        rejected("an infinite vertex position is rejected", inf),
    ];

    // Zero-length normals: rejected, or still shaded rather than drawn black.
    let mut flat = quad();
    flat.normals.iter_mut().for_each(|n| *n = Vec3::ZERO);
    let camera = Camera::framing(Vec3::ZERO, 15.0, Vec3::new(0.3, -0.4, 1.0))?;
    let check = match renderer.upload(&flat) {
        Err(e) => Check::new(
            "zero-length normals are rejected or shaded",
            true,
            format!("rejected: {e}"),
        ),
        Ok(scene) => {
            let frame = renderer.render(&scene, &camera, 200, 200)?;
            let lit: Vec<[u8; 4]> = (0..200 * 200)
                .filter(|k| frame.ids[*k as usize] != 0)
                .filter_map(|k| frame.image.pixel(k % 200, k / 200))
                .collect();
            let black = lit
                .iter()
                .filter(|p| p[0] < 40 && p[1] < 40 && p[2] < 40)
                .count();
            Check::new(
                "zero-length normals are rejected or shaded",
                black == 0 && !lit.is_empty(),
                format!(
                    "accepted; {black} of {} face pixels are near black, e.g. {:?}",
                    lit.len(),
                    lit.first()
                ),
            )
        }
    };
    checks.push(check);
    Ok(checks)
}

/// Hollow boxes with walls down to 0.1 micrometre.
fn thin() -> DynResult<Demo> {
    let hollow = |t: f64| {
        move || {
            let mut b = Builder::default();
            b.cube(Vec3::ZERO, Vec3::new(40.0, 40.0, 40.0), 0);
            let inner = 40.0 - 2.0 * t;
            b.cube(Vec3::new(t, t, t), Vec3::new(inner, inner, inner), 6);
            b.mesh
        }
    };
    let framed = Camera::framing(Vec3::new(20.0, 20.0, 20.0), 36.0, Vec3::new(1.0, -1.3, 0.9))?;
    let loose = Camera {
        near: 0.1,
        far: 1.0e5,
        ..framed
    };
    Ok(Demo::of(vec![
        Case::new("wall 1 mm, framed clip planes", framed, hollow(1.0)),
        Case::new("wall 0.01 mm, framed clip planes", framed, hollow(0.01)),
        Case::new("wall 0.0001 mm, framed clip planes", framed, hollow(1e-4)),
        Case::new("wall 0.01 mm, near 0.1 mm, far 100 m", loose, hollow(0.01)),
    ]))
}

/// Faces about one pixel across, and parts smaller than a pixel.
fn picking() -> DynResult<Demo> {
    let (w, h) = SIZE;
    // Looking straight down from 100 mm, so a pixel has a known size on the plate.
    let d = 100.0;
    let camera = Camera {
        eye: Vec3::new(0.0, 0.0, d),
        target: Vec3::ZERO,
        up: Vec3::Y,
        fov_y: Camera::DEFAULT_FOV_Y,
        near: 50.0,
        far: 150.0,
    };
    let pixel = 2.0 * d * (camera.fov_y / 2.0).tan() / f64::from(h);
    let cells = move |cell_px: f64| {
        move || {
            let size = cell_px * pixel;
            let nx = (f64::from(w) / cell_px).ceil() as u32;
            let ny = (f64::from(h) / cell_px).ceil() as u32;
            let mut b = Builder::default();
            b.grid(
                Vec3::new(
                    -f64::from(w) / 2.0 * pixel,
                    -f64::from(h) / 2.0 * pixel,
                    0.0,
                ),
                Vec3::new(f64::from(nx) * size, 0.0, 0.0),
                Vec3::new(0.0, f64::from(ny) * size, 0.0),
                (nx, ny),
                Vec3::Z,
                false,
                move |i, j| j * nx + i,
            );
            b.mesh
        }
    };
    // 2,500 cubes from 0.1 to 1.5 pixels across on a plate.
    let tiny = move || {
        let mut b = Builder::default();
        b.grid(
            Vec3::new(-60.0, -40.0, 0.0),
            Vec3::new(120.0, 0.0, 0.0),
            Vec3::new(0.0, 80.0, 0.0),
            (1, 1),
            Vec3::Z,
            false,
            |_, _| 0,
        );
        for j in 0..50u32 {
            for i in 0..50u32 {
                let size = pixel * (0.1 + 1.4 * f64::from((i + j) % 10) / 9.0);
                let origin = Vec3::new(-50.0 + f64::from(i) * 2.0, -35.0 + f64::from(j) * 1.4, 0.0);
                b.cube(origin, Vec3::new(size, size, size), 1 + 6 * (j * 50 + i));
            }
        }
        b.mesh
    };
    Ok(Demo::of(vec![
        Case::new("256,000 faces, one pixel each", camera, cells(1.0)),
        Case::new("520,000 faces, 0.7 pixel each", camera, cells(0.7)),
        Case::new("2,500 cubes 0.1 to 1.5 pixels across", camera, tiny),
    ]))
}

/// A torus with `major x minor` segments, faces of `patch x patch` cells.
fn torus(big: f64, small: f64, major: u32, minor: u32, patch: u32) -> Mesh {
    let mut b = Builder::default();
    let quads = major as usize * minor as usize;
    b.mesh.positions.reserve(quads);
    b.mesh.normals.reserve(quads);
    b.mesh.triangles.reserve(2 * quads);
    b.mesh.triangle_faces.reserve(2 * quads);
    for i in 0..major {
        let u = std::f64::consts::TAU * f64::from(i) / f64::from(major);
        for j in 0..minor {
            let v = std::f64::consts::TAU * f64::from(j) / f64::from(minor);
            let n = Vec3::new(v.cos() * u.cos(), v.cos() * u.sin(), v.sin());
            let ring = Vec3::new(u.cos(), u.sin(), 0.0) * big;
            b.vertex(ring + n * small, n);
        }
    }
    let at = |i: u32, j: u32| (i % major) * minor + j % minor;
    let per_row = minor.div_ceil(patch);
    for i in 0..major {
        for j in 0..minor {
            let face = (i / patch) * per_row + j / patch;
            b.tri([at(i, j), at(i + 1, j), at(i + 1, j + 1)], face);
            b.tri([at(i, j), at(i + 1, j + 1), at(i, j + 1)], face);
        }
    }
    b.mesh
}

/// A uniform number in [0, 1) from a 64-bit linear congruential generator.
fn unit(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

/// Builds meshes the way kernel tessellations look: shared vertices within a face.
#[derive(Default)]
struct Builder {
    mesh: Mesh,
}

impl Builder {
    fn vertex(&mut self, p: Vec3, n: Vec3) -> u32 {
        self.mesh.positions.push(p);
        self.mesh.normals.push(n);
        (self.mesh.positions.len() - 1) as u32
    }

    fn tri(&mut self, t: [u32; 3], face: u32) {
        self.mesh.triangles.push(t);
        self.mesh.triangle_faces.push(FaceIndex(face));
    }

    /// A quad from corners in order around it.
    fn quad(&mut self, corners: [Vec3; 4], n: Vec3, face: u32) {
        let [a, b, c, d] = corners.map(|p| self.vertex(p, n));
        self.tri([a, b, c], face);
        self.tri([a, c, d], face);
    }

    /// A parallelogram from `origin` along `u` and `v`, split into `cells` with the face of
    /// each cell given by `face(i, j)`. `flip` chooses the other diagonal.
    #[allow(clippy::too_many_arguments)]
    fn grid(
        &mut self,
        origin: Vec3,
        u: Vec3,
        v: Vec3,
        cells: (u32, u32),
        n: Vec3,
        flip: bool,
        face: impl Fn(u32, u32) -> u32,
    ) {
        let (nu, nv) = cells;
        let base = self.mesh.positions.len() as u32;
        for j in 0..=nv {
            for i in 0..=nu {
                let p = origin
                    + u * (f64::from(i) / f64::from(nu))
                    + v * (f64::from(j) / f64::from(nv));
                self.vertex(p, n);
            }
        }
        let row = nu + 1;
        for j in 0..nv {
            for i in 0..nu {
                let a = base + j * row + i;
                let (b, c, d) = (a + 1, a + row + 1, a + row);
                let f = face(i, j);
                if flip {
                    self.tri([a, b, d], f);
                    self.tri([b, c, d], f);
                } else {
                    self.tri([a, b, c], f);
                    self.tri([a, c, d], f);
                }
            }
        }
    }

    /// An axis-aligned box with faces `first..first + 6`.
    fn cube(&mut self, origin: Vec3, size: Vec3, first: u32) {
        let at = |x: f64, y: f64, z: f64| origin + Vec3::new(x * size.x, y * size.y, z * size.z);
        let faces = [
            (
                [
                    at(0., 0., 0.),
                    at(0., 1., 0.),
                    at(0., 1., 1.),
                    at(0., 0., 1.),
                ],
                -Vec3::X,
            ),
            (
                [
                    at(1., 0., 0.),
                    at(1., 0., 1.),
                    at(1., 1., 1.),
                    at(1., 1., 0.),
                ],
                Vec3::X,
            ),
            (
                [
                    at(0., 0., 0.),
                    at(0., 0., 1.),
                    at(1., 0., 1.),
                    at(1., 0., 0.),
                ],
                -Vec3::Y,
            ),
            (
                [
                    at(0., 1., 0.),
                    at(1., 1., 0.),
                    at(1., 1., 1.),
                    at(0., 1., 1.),
                ],
                Vec3::Y,
            ),
            (
                [
                    at(0., 0., 0.),
                    at(1., 0., 0.),
                    at(1., 1., 0.),
                    at(0., 1., 0.),
                ],
                -Vec3::Z,
            ),
            (
                [
                    at(0., 0., 1.),
                    at(0., 1., 1.),
                    at(1., 1., 1.),
                    at(1., 0., 1.),
                ],
                Vec3::Z,
            ),
        ];
        for (k, (corners, n)) in (0u32..).zip(faces) {
            self.quad(corners, n, first + k);
        }
    }
}
