//! Torture tests for the renderer, shown the way game engine editors show their technology:
//! a lit view with live stats beside technical view modes (triangles, wireframe, the face ID
//! buffer, depth), and an overview of every view mode.
//!
//! **These are deliberately heavy.** The largest cases build meshes of 16 million triangles,
//! need several GB of memory, and keep the GPU busy for seconds at a time.
//!
//! Headless (the default), every case renders offscreen and is checked pixel by pixel against
//! a CPU reference rasterizer in double precision (`support/verify.rs`). Each demo prints what
//! it measured and writes `torture-<demo>.png`, and the run exits with an error if any check
//! failed.
//!
//! With `--window`, the first demo named opens in a window that turns the model slowly:
//! keys 1 to 8 switch view mode, space pauses, drag orbits, scroll zooms, a click prints the
//! face under the cursor, and the left and right arrows change case.
//!
//! Usage: `cargo run --release -p cad-render --example torture -- [demo ...] [--out DIR] [--window]`
//!
//! Demos: `modes`, `dense`, `bodies`, `depth`, `zfight`, `slivers`, `thin`, `picking`, or
//! `all` (the default). Images go to `target/torture` unless `--out` says otherwise.

use std::path::PathBuf;

use cad_kernel::Mesh;
use cad_math::Vec3;
use cad_render::{Camera, Image, RenderError, Renderer, ViewMode};

#[path = "support/scenes.rs"]
mod scenes;
#[path = "support/text.rs"]
mod text;
#[path = "support/verify.rs"]
mod verify;
#[path = "support/viewer.rs"]
mod viewer;

use scenes::{Builder, gauss, hsv, mix, torus_knot, unit};
use verify::{Check, DynResult, Inset, MAIN, Shot, look, print_checks, run_case, stack};

const DEMOS: [&str; 8] = [
    "modes", "dense", "bodies", "depth", "zfight", "slivers", "thin", "picking",
];

const WARNING: &str = "\
WARNING: these are torture tests for the renderer. The largest cases build meshes of up to
16 million triangles, need several GB of memory, and keep the GPU fully busy for seconds at a
time. Your machine may become sluggish while they run. Close other heavy applications first,
and on machines with less than 8 GB of memory run one demo at a time. Press Ctrl+C to stop.";

/// A mesh with a colour per face.
pub type Model = (Mesh, Vec<[u8; 3]>);

/// One scene to render. The mesh is built only when the case runs, so large cases do not
/// all sit in memory at once.
pub struct Case {
    /// What the case is.
    pub title: String,
    /// The camera, including the clip planes under test.
    pub camera: Camera,
    /// Builds the mesh and its colours.
    pub build: Box<dyn Fn() -> Model>,
    /// Faces that must never reach the ID buffer.
    pub forbidden: Vec<u32>,
    /// The view mode of the main view.
    pub main: ViewMode,
    /// The technical views shown beside the main one.
    pub insets: [Inset; 3],
}

impl Case {
    fn new(title: &str, camera: Camera, build: impl Fn() -> Model + 'static) -> Self {
        Self {
            title: title.to_string(),
            camera,
            build: Box::new(build),
            forbidden: Vec::new(),
            main: ViewMode::Lit,
            insets: [
                Inset::View(ViewMode::Faces),
                Inset::View(ViewMode::Depth),
                Inset::Errors,
            ],
        }
    }

    fn insets(mut self, insets: [Inset; 3]) -> Self {
        self.insets = insets;
        self
    }
}

/// Checks that need no picture.
type ExtraChecks = fn(&Renderer) -> DynResult<Vec<Check>>;

/// A named group of cases.
struct Demo {
    cases: Vec<Case>,
    /// Stop after the first failing case: the later ones are larger versions of it.
    ramp: bool,
    extra: Option<ExtraChecks>,
    /// Show every view mode of each case instead of the usual row.
    overview: bool,
}

impl Demo {
    fn of(cases: Vec<Case>) -> Self {
        Self {
            cases,
            ramp: false,
            extra: None,
            overview: false,
        }
    }
}

fn demo(name: &str) -> DynResult<Demo> {
    match name {
        "modes" => modes(),
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
            let (mesh, colors) = (case.build)();
            let label = name.to_uppercase();
            let title = case.title.to_uppercase();
            let shot = Shot {
                demo: &label,
                title: &title,
                mesh: &mesh,
                colors: &colors,
                camera: &case.camera,
                forbidden: &case.forbidden,
                main: case.main,
                insets: case.insets,
            };
            let outcome = run_case(&renderer, &shot)?;
            let passed = outcome.passed();
            if demo.overview {
                panels.push(overview(&renderer, &mesh, &colors, &case.camera)?);
            } else {
                panels.push(outcome.panel);
            }
            checks.extend(outcome.checks);
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

/// Every view mode of one scene in a labelled grid, with a title bar.
fn overview(
    renderer: &Renderer,
    mesh: &Mesh,
    colors: &[[u8; 3]],
    camera: &Camera,
) -> DynResult<Image> {
    let (tw, th) = (480, 300);
    let m = verify::MARGIN;
    let header = 64;
    let mut out = verify::blank(4 * tw + 5 * m, header + 2 * th + 3 * m, verify::FRAME);
    text::draw(
        &mut out,
        m + 6,
        14,
        3,
        [255, 255, 255, 255],
        "RUSTCAD RENDERER / VIEW MODES",
    );
    text::draw(
        &mut out,
        m + 6,
        46,
        1,
        [150, 220, 150, 255],
        &format!(
            "{} TRIANGLES, {} FACES. EVERY MODE BUT OVERDRAW WRITES THE FACE ID BUFFER USED FOR PICKING.",
            text::thousands(mesh.triangles.len() as u64),
            text::thousands(colors.len() as u64)
        ),
    );
    let scene = renderer.upload_with_colors(mesh, colors)?;
    for (i, mode) in ViewMode::ALL.into_iter().enumerate() {
        renderer.set_view_mode(mode);
        let mut tile = renderer.render(&scene, camera, tw, th)?.image;
        text::label(
            &mut tile,
            8,
            8,
            &format!("{} {}", i + 1, mode.name().to_uppercase()),
        );
        let (col, row) = (i as u32 % 4, i as u32 / 4);
        verify::blit(
            &mut out,
            &tile,
            m + col * (tw + m),
            header + m + row * (th + m),
        );
    }
    renderer.set_view_mode(ViewMode::Lit);
    Ok(out)
}

/// The scene for the view-mode overview: a ridged knot over a plate ringed with blocks.
fn showcase() -> Model {
    let (mesh, colors) = torus_knot(180, 24, false);
    let mut b = Builder { mesh, colors };
    b.colored_cube(
        Vec3::new(-62.0, -62.0, -30.0),
        Vec3::new(124.0, 124.0, 6.0),
        64,
        [150, 156, 168],
    );
    for i in 0..24u32 {
        let a = std::f64::consts::TAU * f64::from(i) / 24.0;
        let size = 4.0 + 3.0 * f64::from(i % 3);
        let centre = Vec3::new(52.0 * a.cos(), 52.0 * a.sin(), -24.0);
        b.colored_cube(
            centre - Vec3::new(size / 2.0, size / 2.0, 0.0),
            Vec3::new(size, size, size),
            70 + 6 * i,
            hsv(0.07 + 0.02 * f64::from(i % 4), 0.75, 0.95),
        );
    }
    b.finish()
}

fn modes() -> DynResult<Demo> {
    let camera = Camera::framing(Vec3::new(0.0, 0.0, -6.0), 72.0, Vec3::new(1.0, -1.25, 0.85))?;
    Ok(Demo {
        overview: true,
        ..Demo::of(vec![Case::new("knot on a plate", camera, showcase)])
    })
}

/// The ridged knot from 1 to 16 million triangles. Zoomed views show the detail appear.
fn dense() -> DynResult<Demo> {
    let camera = Camera::framing(Vec3::ZERO, 50.0, Vec3::new(0.9, -1.1, 1.0))?;
    // A telephoto view from the same eye onto the near side of the tube.
    let near_side = Vec3::new(42.0, 0.0, 0.0);
    let zoom = Camera {
        target: near_side + (camera.eye - near_side).normalized().unwrap_or(Vec3::Z) * 7.0,
        fov_y: Camera::DEFAULT_FOV_Y / 25.0,
        ..camera
    };
    let steps = [
        (2000, 250, "1 million"),
        (2000, 500, "2 million"),
        (4000, 500, "4 million"),
        (4000, 1000, "8 million"),
        (8000, 1000, "16 million"),
    ];
    let cases = steps
        .into_iter()
        .map(|(along, around, label)| {
            Case::new(
                &format!("ridged knot, {label} triangles"),
                camera,
                move || torus_knot(along, around, true),
            )
            .insets([
                Inset::Zoom(ViewMode::Triangles, zoom, "TRIANGLES, ZOOM 25X"),
                Inset::Zoom(ViewMode::Wireframe, zoom, "WIREFRAME, ZOOM 25X"),
                Inset::Errors,
            ])
        })
        .collect();
    Ok(Demo {
        ramp: true,
        ..Demo::of(cases)
    })
}

/// A spiral galaxy of `n` separate cubes, each its own body and pick target.
fn galaxy(n: u32) -> Model {
    let mut b = Builder::default();
    let mut seed = 0x2545_f491_u64 ^ u64::from(n);
    let core = [255, 176, 60];
    let arm = [50, 100, 230];
    let rim = [120, 50, 200];
    for i in 0..n {
        let (p, color) = if unit(&mut seed) < 0.18 {
            // The bulge.
            let p = Vec3::new(
                gauss(&mut seed) * 7.0,
                gauss(&mut seed) * 7.0,
                gauss(&mut seed) * 3.0,
            );
            (p, mix(core, [255, 120, 40], unit(&mut seed)))
        } else {
            // Two logarithmic spiral arms.
            let t = unit(&mut seed);
            let r = 6.0 + 54.0 * t.powf(0.8);
            let branch = f64::from(i % 2) * std::f64::consts::PI;
            let angle = branch + 2.2 * r.ln() + gauss(&mut seed) * (0.28 - 0.12 * t);
            let spread = gauss(&mut seed) * (1.5 + 2.0 * t);
            let p = Vec3::new(
                (r + spread) * angle.cos(),
                (r + spread) * angle.sin(),
                gauss(&mut seed) * (1.6 - 1.1 * t),
            );
            let color = if unit(&mut seed) < 0.04 {
                [235, 50, 130] // star-forming regions
            } else {
                mix(arm, rim, t)
            };
            (p, mix(core, color, (r / 14.0).min(1.0)))
        };
        let size = 0.25 + 0.35 * unit(&mut seed);
        b.solid_cube(
            p - Vec3::new(size / 2.0, size / 2.0, size / 2.0),
            Vec3::new(size, size, size),
            i,
        );
        b.color(i, color);
    }
    b.finish()
}

fn bodies() -> DynResult<Demo> {
    let camera = Camera::framing(Vec3::ZERO, 58.0, Vec3::new(0.35, -1.0, 0.75))?;
    Ok(Demo::of(vec![
        Case::new("spiral galaxy of 128,000 separate bodies", camera, || {
            galaxy(128_000)
        }),
        Case::new("spiral galaxy of 512,000 separate bodies", camera, || {
            galaxy(512_000)
        }),
    ]))
}

/// A bolt on a workbench 60 mm from the eye, and a factory 300 m away.
fn workshop() -> Model {
    let mut b = Builder::default();
    b.colored_cube(
        Vec3::new(-500.0, -100.0, -40.0),
        Vec3::new(1000.0, 700.0, 40.0),
        0,
        [158, 112, 70],
    );
    // An M3 bolt: shank and hex head.
    b.prism(Vec3::new(-6.0, 40.0, 0.0), 1.5, 10.0, 32, 6);
    b.prism(Vec3::new(-6.0, 40.0, 10.0), 3.2, 2.5, 6, 9);
    for f in 6..9 {
        b.color(f, [176, 182, 192]);
    }
    for f in 9..12 {
        b.color(f, [140, 146, 158]);
    }
    // A 1 mm cube beside it.
    b.colored_cube(
        Vec3::new(3.0, 36.0, 0.0),
        Vec3::new(1.0, 1.0, 1.0),
        12,
        [230, 60, 50],
    );
    // A road 5 mm proud of the floor, with lane marks 5 mm proud of the road.
    b.quad(
        [
            Vec3::new(-3500.0, 0.0, -895.0),
            Vec3::new(3500.0, 0.0, -895.0),
            Vec3::new(3500.0, 290_000.0, -895.0),
            Vec3::new(-3500.0, 290_000.0, -895.0),
        ],
        Vec3::Z,
        19,
    );
    b.color(19, [62, 62, 68]);
    for k in 0..60 {
        let y = 2000.0 + f64::from(k) * 4800.0;
        b.quad(
            [
                Vec3::new(-75.0, y, -890.0),
                Vec3::new(75.0, y, -890.0),
                Vec3::new(75.0, y + 2400.0, -890.0),
                Vec3::new(-75.0, y + 2400.0, -890.0),
            ],
            Vec3::Z,
            20,
        );
    }
    b.color(20, [240, 240, 235]);
    // The floor, 0.9 m down, 4 km across, after the road and marks on it.
    let g = 2_000_000.0;
    b.grid(
        Vec3::new(-g, -g, -900.0),
        Vec3::new(2.0 * g, 0.0, 0.0),
        Vec3::new(0.0, 2.0 * g, 0.0),
        (8, 8),
        Vec3::Z,
        false,
        |_, _| 18,
    );
    b.color(18, [96, 112, 92]);
    // The factory: 40 m wide, 12 m tall, its front 300 m away, windows 20 mm proud. The
    // windows come first in the mesh: draw order must not decide what is in front.
    for row in 0..3 {
        for col in 0..10 {
            let x = -18_500.0 + f64::from(col) * 3800.0;
            let z = 600.0 + f64::from(row) * 3600.0;
            b.quad(
                [
                    Vec3::new(x, 299_980.0, z),
                    Vec3::new(x + 2500.0, 299_980.0, z),
                    Vec3::new(x + 2500.0, 299_980.0, z + 2000.0),
                    Vec3::new(x, 299_980.0, z + 2000.0),
                ],
                -Vec3::Y,
                27,
            );
        }
    }
    b.colored_cube(
        Vec3::new(-20_000.0, 300_000.0, -900.0),
        Vec3::new(40_000.0, 20_000.0, 12_000.0),
        21,
        [204, 198, 186],
    );
    b.color(27, [40, 72, 124]);
    b.finish()
}

/// Three 0.2 mm cubes on a plate, 1 km from the origin.
fn far_part() -> Model {
    let far = Vec3::new(1.0e6, 0.0, 0.0);
    let mut b = Builder::default();
    b.colored_cube(
        far + Vec3::new(-1.0, -1.0, -0.1),
        Vec3::new(2.0, 2.0, 0.1),
        0,
        [150, 156, 168],
    );
    for i in 0..3u32 {
        let x = -0.35 + f64::from(i) * 0.25;
        b.colored_cube(
            far + Vec3::new(x, -0.1, 0.0),
            Vec3::new(0.2, 0.2, 0.2),
            6 + 6 * i,
            hsv(f64::from(i) / 3.0, 0.7, 0.95),
        );
    }
    b.finish()
}

fn depth() -> DynResult<Demo> {
    let eye = Vec3::new(0.0, -60.0, 25.0);
    let target = Vec3::new(0.0, 300_000.0, 2500.0);
    let views = [
        Inset::View(ViewMode::Depth),
        Inset::View(ViewMode::Faces),
        Inset::Errors,
    ];
    Ok(Demo::of(vec![
        Case::new(
            "bolt at 60 mm, factory at 300 m, near plane 0.01 mm",
            look(eye, target, 0.01, 4.0e6),
            workshop,
        )
        .insets(views),
        Case::new(
            "same scene, near plane 20 mm",
            look(eye, target, 20.0, 4.0e6),
            workshop,
        )
        .insets(views),
        Case::new(
            "0.2 mm cubes 1 km from the origin",
            Camera::framing(Vec3::new(1.0e6, 0.0, 0.0), 1.2, Vec3::new(1.0, -1.0, 0.8))?,
            far_part,
        )
        .insets([
            Inset::View(ViewMode::Faces),
            Inset::View(ViewMode::Wireframe),
            Inset::Errors,
        ]),
    ]))
}

/// A box whose sides are each split into an `n x n` grid, faces `first..first + 6`.
fn gridded_box(b: &mut Builder, origin: Vec3, size: Vec3, n: u32, flip: bool, first: u32) {
    let (x, y, z) = (
        Vec3::new(size.x, 0.0, 0.0),
        Vec3::new(0.0, size.y, 0.0),
        Vec3::new(0.0, 0.0, size.z),
    );
    let sides = [
        (origin, y, z, -Vec3::X),
        (origin + x, z, y, Vec3::X),
        (origin, z, x, -Vec3::Y),
        (origin + y, x, z, Vec3::Y),
        (origin, x, y, -Vec3::Z),
        (origin + z, y, x, Vec3::Z),
    ];
    for (k, (o, u, v, normal)) in (0u32..).zip(sides) {
        b.grid(o, u, v, (n, n), normal, flip, |_, _| first + k);
    }
}

/// A blue block, and on or in it a second body: a flush lid with the same triangles, an
/// overlapping orange block, or a white label `gap` above the top.
fn assembly(gap: f64, same_mesh: bool, label: bool) -> Model {
    let mut b = Builder::default();
    let blue = [70, 120, 210];
    let orange = [240, 150, 50];
    gridded_box(
        &mut b,
        Vec3::ZERO,
        Vec3::new(60.0, 40.0, 20.0),
        12,
        false,
        0,
    );
    for f in 0..6 {
        b.color(f, blue);
    }
    if label {
        b.grid(
            Vec3::new(8.0, 8.0, 20.0 + gap),
            Vec3::new(44.0, 0.0, 0.0),
            Vec3::new(0.0, 24.0, 0.0),
            (7, 5),
            Vec3::Z,
            true,
            |_, _| 6,
        );
        b.color(6, [245, 245, 240]);
    } else if same_mesh {
        b.grid(
            Vec3::new(0.0, 0.0, 20.0),
            Vec3::new(0.0, 40.0, 0.0),
            Vec3::new(60.0, 0.0, 0.0),
            (12, 12),
            Vec3::Z,
            false,
            |_, _| 6,
        );
        b.color(6, orange);
    } else {
        gridded_box(
            &mut b,
            Vec3::new(30.0, 0.0, 0.0),
            Vec3::new(50.0, 40.0, 20.0),
            7,
            true,
            6,
        );
        for f in 6..12 {
            b.color(f, orange);
        }
    }
    b.finish()
}

fn zfight() -> DynResult<Demo> {
    let camera = Camera::framing(Vec3::new(40.0, 20.0, 10.0), 52.0, Vec3::new(0.8, -1.0, 0.9))?;
    let views = [
        Inset::View(ViewMode::Faces),
        Inset::View(ViewMode::Triangles),
        Inset::Errors,
    ];
    Ok(Demo::of(vec![
        Case::new("flush faces with the same triangles", camera, || {
            assembly(0.0, true, false)
        })
        .insets(views),
        Case::new(
            "two overlapping bodies, tops and fronts flush",
            camera,
            || assembly(0.0, false, false),
        )
        .insets(views),
        Case::new("a label 0.001 mm above a face", camera, || {
            assembly(1e-3, false, true)
        })
        .insets(views),
        Case::new("a label 0.00001 mm above a face", camera, || {
            assembly(1e-5, false, true)
        })
        .insets(views),
    ]))
}

fn slivers() -> DynResult<Demo> {
    // A starburst of 200,000 fan triangles: 50 mm long, 1.6 micrometres wide at the rim.
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
            let sector = i * 24 / n;
            b.tri(
                [centre, rim[i as usize], rim[((i + 1) % n) as usize]],
                sector,
            );
        }
        for s in 0..24 {
            let value = if s % 2 == 0 { 0.95 } else { 0.8 };
            b.color(s, hsv(f64::from(s) / 24.0, 0.7, value));
        }
        b.finish()
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
            let face = (i - 1) * 12 / strips;
            b.tri([left, right, r], face);
            b.tri([left, r, l], face);
            (left, right) = (l, r);
        }
        for f in 0..12 {
            b.color(f, hsv(0.55 + f64::from(f) / 30.0, 0.65, 0.9));
        }
        b.finish()
    };
    // A plate behind 30,000 zero-area triangles (faces 1 and 2, bright red and yellow) that
    // must never show.
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
        b.color(0, [60, 66, 80]);
        b.color(1, [255, 40, 40]);
        b.color(2, [255, 230, 0]);
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
        b.finish()
    };
    let camera = Camera::framing(Vec3::ZERO, 55.0, Vec3::new(0.6, -0.8, 1.2))?;
    let centre = Camera {
        fov_y: Camera::DEFAULT_FOV_Y / 40.0,
        ..camera
    };
    let mut zero_area = Case::new(
        "plate behind 30,000 zero-area triangles",
        camera,
        degenerate,
    )
    .insets([
        Inset::View(ViewMode::Faces),
        Inset::View(ViewMode::Overdraw),
        Inset::Errors,
    ]);
    zero_area.forbidden = vec![1, 2];
    Ok(Demo {
        extra: Some(invalid_input),
        ..Demo::of(vec![
            Case::new("starburst of 200,000 slivers", camera, fan).insets([
                Inset::View(ViewMode::Triangles),
                Inset::View(ViewMode::Overdraw),
                Inset::Errors,
            ]),
            Case::new("sheet of 600,000 needles", camera, needles).insets([
                Inset::View(ViewMode::Triangles),
                Inset::Zoom(ViewMode::Wireframe, centre, "WIREFRAME, ZOOM 40X"),
                Inset::Errors,
            ]),
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

    // Zero-length normals: rejected, or still shaded rather than drawn black. The Faces view
    // lights faces by their normals and draws no edges, so every face pixel counts.
    let mut flat = quad();
    flat.normals.iter_mut().for_each(|n| *n = Vec3::ZERO);
    let camera = Camera::framing(Vec3::ZERO, 15.0, Vec3::new(0.3, -0.4, 1.0))?;
    renderer.set_view_mode(ViewMode::Faces);
    let check = match renderer.upload(&flat) {
        Err(e) => Check::new(
            "zero-length normals are rejected or shaded",
            true,
            format!("rejected: {e}"),
        ),
        Ok(scene) => {
            let frame = renderer.render(&scene, &camera, 200, 200)?;
            renderer.set_view_mode(ViewMode::Lit);
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

/// An open-topped box with walls `t` thick: blue outside, red inside, yellow along the rim.
fn enclosure(t: f64) -> Model {
    let mut b = Builder::default();
    let s = 40.0;
    let quad = |b: &mut Builder, c: [[f64; 3]; 4], n: Vec3, f: u32| {
        b.quad(c.map(|[x, y, z]| Vec3::new(x, y, z)), n, f);
    };
    let (lo, hi) = (t, s - t);
    // Outside: four walls and the bottom.
    quad(
        &mut b,
        [[0., 0., 0.], [0., s, 0.], [0., s, s], [0., 0., s]],
        -Vec3::X,
        0,
    );
    quad(
        &mut b,
        [[s, 0., 0.], [s, 0., s], [s, s, s], [s, s, 0.]],
        Vec3::X,
        1,
    );
    quad(
        &mut b,
        [[0., 0., 0.], [0., 0., s], [s, 0., s], [s, 0., 0.]],
        -Vec3::Y,
        2,
    );
    quad(
        &mut b,
        [[0., s, 0.], [s, s, 0.], [s, s, s], [0., s, s]],
        Vec3::Y,
        3,
    );
    quad(
        &mut b,
        [[0., 0., 0.], [s, 0., 0.], [s, s, 0.], [0., s, 0.]],
        -Vec3::Z,
        4,
    );
    // Inside: four walls and the floor, facing in.
    quad(
        &mut b,
        [[lo, lo, lo], [lo, lo, s], [lo, hi, s], [lo, hi, lo]],
        Vec3::X,
        5,
    );
    quad(
        &mut b,
        [[hi, lo, lo], [hi, hi, lo], [hi, hi, s], [hi, lo, s]],
        -Vec3::X,
        6,
    );
    quad(
        &mut b,
        [[lo, lo, lo], [hi, lo, lo], [hi, lo, s], [lo, lo, s]],
        Vec3::Y,
        7,
    );
    quad(
        &mut b,
        [[lo, hi, lo], [lo, hi, s], [hi, hi, s], [hi, hi, lo]],
        -Vec3::Y,
        8,
    );
    quad(
        &mut b,
        [[lo, lo, lo], [lo, hi, lo], [hi, hi, lo], [hi, lo, lo]],
        Vec3::Z,
        9,
    );
    // The rim: four strips `t` wide around the top.
    quad(
        &mut b,
        [[0., 0., s], [s, 0., s], [hi, lo, s], [lo, lo, s]],
        Vec3::Z,
        10,
    );
    quad(
        &mut b,
        [[s, 0., s], [s, s, s], [hi, hi, s], [hi, lo, s]],
        Vec3::Z,
        10,
    );
    quad(
        &mut b,
        [[s, s, s], [0., s, s], [lo, hi, s], [hi, hi, s]],
        Vec3::Z,
        10,
    );
    quad(
        &mut b,
        [[0., s, s], [0., 0., s], [lo, lo, s], [lo, hi, s]],
        Vec3::Z,
        10,
    );
    for f in 0..5 {
        b.color(f, [70, 125, 215]);
    }
    for f in 5..10 {
        b.color(f, [225, 72, 60]);
    }
    b.color(10, [245, 200, 60]);
    b.finish()
}

fn thin() -> DynResult<Demo> {
    let framed = Camera::framing(Vec3::new(20.0, 20.0, 20.0), 36.0, Vec3::new(1.0, -1.4, 1.2))?;
    let loose = Camera {
        near: 0.1,
        far: 1.0e5,
        ..framed
    };
    Ok(Demo::of(vec![
        Case::new("walls 1 mm, clip planes fitted", framed, || enclosure(1.0)),
        Case::new("walls 0.01 mm, clip planes fitted", framed, || {
            enclosure(0.01)
        }),
        Case::new("walls 0.0001 mm, clip planes fitted", framed, || {
            enclosure(1e-4)
        }),
        Case::new("walls 0.01 mm, clip planes 0.1 mm to 100 m", loose, || {
            enclosure(0.01)
        }),
    ]))
}

/// The colour of the Mandelbrot set at a point of seahorse valley.
fn mandelbrot(x: f64, y: f64) -> [u8; 3] {
    let (cx, cy) = (-0.743_643_887 + x * 0.0045, 0.131_825_904 + y * 0.0045);
    let (mut zx, mut zy) = (0.0f64, 0.0f64);
    for n in 0..400 {
        let (xx, yy) = (zx * zx, zy * zy);
        if xx + yy > 64.0 {
            let smooth = f64::from(n) + 1.0 - (xx + yy).ln().ln() / std::f64::consts::LN_2;
            let t = smooth / 400.0;
            return hsv(0.62 + 2.4 * t, 0.78, (0.35 + 6.0 * t).min(1.0));
        }
        zy = 2.0 * zx * zy + cy;
        zx = xx - yy + cx;
    }
    [10, 10, 24]
}

fn picking() -> DynResult<Demo> {
    let (w, h) = MAIN;
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
    let mosaic = move |cell_px: f64| {
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
            b.colors = (0..ny)
                .flat_map(|j| {
                    (0..nx).map(move |i| {
                        let x = (f64::from(i) + 0.5) / f64::from(nx) - 0.5;
                        let y = ((f64::from(j) + 0.5) / f64::from(ny) - 0.5) * f64::from(h)
                            / f64::from(w);
                        mandelbrot(x, y)
                    })
                })
                .collect();
            b.finish()
        }
    };
    // 2,500 cubes from 0.1 to 1.5 pixels across on a dark plate.
    let tiny = move || {
        let mut b = Builder::default();
        b.grid(
            Vec3::new(-90.0, -55.0, 0.0),
            Vec3::new(180.0, 0.0, 0.0),
            Vec3::new(0.0, 110.0, 0.0),
            (1, 1),
            Vec3::Z,
            false,
            |_, _| 0,
        );
        b.color(0, [214, 216, 222]);
        for j in 0..50u32 {
            for i in 0..50u32 {
                let size = pixel * (0.1 + 1.4 * f64::from((i + j) % 10) / 9.0);
                let origin = Vec3::new(-49.0 + f64::from(i) * 2.0, -35.0 + f64::from(j) * 1.4, 0.0);
                b.colored_cube(
                    origin,
                    Vec3::new(size, size, size),
                    1 + 6 * (j * 50 + i),
                    hsv(f64::from(i * 50 + j) * 0.618, 0.85, 0.75),
                );
            }
        }
        b.finish()
    };
    let views = [Inset::Picks, Inset::View(ViewMode::Faces), Inset::Errors];
    // Every pixel of the mosaics is a face boundary, so they are shown unlit, without edges.
    let unlit = |case: Case| Case {
        main: ViewMode::Unlit,
        ..case
    };
    Ok(Demo::of(vec![
        unlit(Case::new("576,000 faces of one pixel each", camera, mosaic(1.0)).insets(views)),
        unlit(Case::new("1.2 million faces of 0.7 pixel each", camera, mosaic(0.7)).insets(views)),
        Case::new("2,500 cubes 0.1 to 1.5 pixels across", camera, tiny).insets(views),
    ]))
}
