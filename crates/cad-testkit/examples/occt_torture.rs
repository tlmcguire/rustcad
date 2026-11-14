//! Torture test of the kernel-to-renderer path: long boolean chains in OpenCascade, through
//! the `Kernel` trait, tessellated and rendered offscreen, then checked against the CPU
//! reference rasterizer of the `cad-render` torture example.
//!
//! **This is deliberately heavy:** hundreds of booleans on a growing shape take a while.
//!
//! It lives here rather than in `cad-render` because the renderer may not depend on the
//! OpenCascade backend (`scripts/layers.txt`). Needs the backend (see the README).
//!
//! Usage: `cargo run --release -p cad-testkit --example occt_torture -- [--out DIR] [--grid N]`
//!
//! `--grid N` sets the number of cuts and boxes per side (default 16, so 256 of each).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cad_kernel::{BoolOp, Kernel, ShapeId};
use cad_kernel_occt::OcctKernel;
use cad_math::{Tolerance, Vec3};
use cad_render::{Camera, Renderer, ViewMode};

#[path = "../../cad-render/examples/support/text.rs"]
mod text;
#[path = "../../cad-render/examples/support/verify.rs"]
mod verify;

use verify::{Check, DynResult, Inset, Shot, print_checks, run_case, stack};

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Applies `tools` to `start` one boolean at a time, timing each, and checks the history of
/// every step.
fn chain(
    kernel: &OcctKernel,
    start: ShapeId,
    op: BoolOp,
    tools: &[ShapeId],
) -> DynResult<(ShapeId, Vec<Duration>, usize)> {
    let mut shape = start;
    let mut times = Vec::with_capacity(tools.len());
    let mut untracked = 0;
    for (i, &tool) in tools.iter().enumerate() {
        let started = Instant::now();
        let result = kernel
            .boolean(op, shape, tool)
            .map_err(|e| format!("{op:?} {} of {}: {e}", i + 1, tools.len()))?;
        times.push(started.elapsed());
        let faces = kernel.face_count(result.shape)?;
        let h = &result.history;
        let named = h
            .faces
            .iter()
            .flat_map(|f| &f.result_faces)
            .chain(&h.generated_faces)
            .count();
        if named < faces {
            untracked += faces - named;
        }
        kernel.release(shape);
        kernel.release(tool);
        shape = result.shape;
    }
    Ok((shape, times, untracked))
}

fn report_chain(name: &str, times: &[Duration]) {
    let total: Duration = times.iter().sum();
    let slowest = times.iter().max().copied().unwrap_or_default();
    println!(
        "    {} {name}s: total {:.0} ms, first {:.1} ms, last {:.1} ms, slowest {:.1} ms",
        times.len(),
        ms(total),
        times.first().map_or(0.0, |d| ms(*d)),
        times.last().map_or(0.0, |d| ms(*d)),
        ms(slowest)
    );
}

fn volume_check(kernel: &OcctKernel, shape: ShapeId, expected: f64) -> DynResult<Check> {
    let got = kernel.mass_props(shape)?.volume;
    let error = (got - expected).abs() / expected;
    Ok(Check::new(
        "volume matches the exact value",
        error < 1e-9,
        format!("{got:.3} mm^3, expected {expected:.3} (relative error {error:.1e})"),
    ))
}

/// Renders and checks a kernel mesh in the default colour; the Faces view tells faces apart.
fn show(
    renderer: &Renderer,
    title: &str,
    mesh: &cad_kernel::Mesh,
    camera: &Camera,
) -> DynResult<verify::CaseOutcome> {
    println!("  case: {title}");
    run_case(
        renderer,
        &Shot {
            demo: "OCCT",
            title: &title.to_uppercase(),
            mesh,
            colors: &[],
            camera,
            forbidden: &[],
            main: ViewMode::Lit,
            light: None,
            edges: true,
            trace: None,
            insets: [
                Inset::View(ViewMode::Faces),
                Inset::View(ViewMode::Wireframe),
                Inset::View(ViewMode::Normals),
            ],
        },
    )
}

fn main() -> DynResult<()> {
    let mut out = PathBuf::from("target/torture");
    let mut n = 16u32;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().ok_or("--out needs a directory")?.into(),
            "--grid" => n = args.next().ok_or("--grid needs a number")?.parse()?,
            other => return Err(format!("unknown argument {other:?}").into()),
        }
    }
    eprintln!(
        "WARNING: this is a torture test. It runs {} OpenCascade booleans on a growing shape and\n\
         keeps a CPU core busy for a while. Press Ctrl+C to stop.\n",
        2 * n * n
    );
    std::fs::create_dir_all(&out)?;
    let kernel = OcctKernel::new()?;
    let renderer = Renderer::headless()?;
    println!(
        "kernel: {} {}; adapter: {}",
        kernel.name(),
        OcctKernel::occt_version(),
        renderer.adapter()
    );
    let mut checks = Vec::new();
    let mut panels = Vec::new();
    let nf = f64::from(n);

    // A plate riddled with n x n square pockets of growing depth, cut one at a time.
    println!("\n  chain: plate minus {} pockets", n * n);
    let pitch = 5.0;
    let side = nf * pitch + pitch;
    let plate = kernel.make_box(Vec3::ZERO, Vec3::new(side, side, 10.0))?;
    let mut tools = Vec::new();
    let mut removed = 0.0;
    for j in 0..n {
        for i in 0..n {
            let depth = 1.0 + 8.0 * f64::from((i * 7 + j * 3) % n) / nf;
            let origin = Vec3::new(
                pitch + f64::from(i) * pitch,
                pitch + f64::from(j) * pitch,
                10.0 - depth,
            );
            tools.push(kernel.make_box(origin, Vec3::new(3.0, 3.0, depth + 1.0))?);
            removed += 9.0 * depth;
        }
    }
    let (riddled, times, untracked) = chain(&kernel, plate, BoolOp::Subtract, &tools)?;
    report_chain("subtract", &times);
    let mut more = vec![
        volume_check(&kernel, riddled, side * side * 10.0 - removed)?,
        Check::new(
            "every output face has an ancestor or is reported as generated",
            untracked == 0,
            format!("{untracked} faces missing from history maps over the chain"),
        ),
    ];
    let faces = kernel.face_count(riddled)?;
    let expected_faces = 6 + 5 * (n * n) as usize;
    more.push(Check::new(
        "face count is 6 plus 5 per pocket",
        faces == expected_faces,
        format!("{faces} faces, expected {expected_faces}"),
    ));
    print_checks(&more);
    checks.extend(more);
    let started = Instant::now();
    let mesh = kernel.tessellate(riddled, Tolerance::default())?;
    println!("    tessellated in {:.0} ms", ms(started.elapsed()));
    let centre = Vec3::new(side / 2.0, side / 2.0, 5.0);
    let camera = Camera::framing(centre, side * 0.75, Vec3::new(0.7, -1.0, 1.1))?;
    let title = format!("plate minus {} pockets, cut one boolean at a time", n * n);
    let outcome = show(&renderer, &title, &mesh, &camera)?;
    checks.extend(outcome.checks);
    panels.push(outcome.panel);

    // A union of n x n overlapping boxes of varied height, joined one at a time.
    println!("\n  chain: union of {} overlapping boxes", n * n);
    let height = |i: u32, j: u32| 2.0 + f64::from((i * 5 + j * 11) % 9);
    let mut boxes = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let origin = Vec3::new(f64::from(i) * 5.0, f64::from(j) * 5.0, 0.0);
            boxes.push(kernel.make_box(origin, Vec3::new(6.0, 6.0, height(i, j)))?);
        }
    }
    let (first, rest) = boxes.split_first().ok_or("no boxes")?;
    let (city, times, untracked) = chain(&kernel, *first, BoolOp::Union, rest)?;
    report_chain("union", &times);
    // Exact volume: every coordinate is a whole millimetre, so sum the highest box over
    // each 1 mm cell.
    let extent = n * 5 + 1;
    let mut volume = 0.0;
    for y in 0..extent {
        for x in 0..extent {
            let top = (0..n)
                .flat_map(|j| (0..n).map(move |i| (i, j)))
                .filter(|&(i, j)| x >= i * 5 && x < i * 5 + 6 && y >= j * 5 && y < j * 5 + 6)
                .map(|(i, j)| height(i, j))
                .fold(0.0, f64::max);
            volume += top;
        }
    }
    let more = vec![
        volume_check(&kernel, city, volume)?,
        Check::new(
            "every output face has an ancestor or is reported as generated",
            untracked == 0,
            format!("{untracked} faces missing from history maps over the chain"),
        ),
    ];
    print_checks(&more);
    checks.extend(more);
    println!("    {} faces", kernel.face_count(city)?);
    let mesh = kernel.tessellate(city, Tolerance::default())?;
    let half = nf * 2.5;
    let camera = Camera::framing(
        Vec3::new(half, half, 5.0),
        half * 1.5,
        Vec3::new(1.0, -0.8, 0.9),
    )?;
    let title = format!(
        "union of {} overlapping boxes, one boolean at a time",
        n * n
    );
    let outcome = show(&renderer, &title, &mesh, &camera)?;
    checks.extend(outcome.checks);
    panels.push(outcome.panel);

    let path = out.join("torture-occt.png");
    stack(&panels).write_png(&path)?;
    println!("\n  wrote {}", path.display());
    let failed = checks.iter().filter(|c| !c.passed).count();
    if failed > 0 {
        return Err(format!("{failed} OCCT torture checks failed").into());
    }
    println!("all OCCT torture checks passed");
    Ok(())
}
