//! Acceptance tests and benchmark for T004: the kernel in a worker process.
//!
//! These start the real `occt-worker` binary, so they need a build linked to OCCT. Without
//! it each test prints `SKIPPED` and passes, unless `OCCT_REQUIRED` is set.
//!
//! The benchmark is ignored by default:
//! `cargo test --release -p cad-kernel-occt --test worker -- --ignored --nocapture`

use std::sync::Arc;
use std::time::{Duration, Instant};

use cad_kernel::{BoolOp, KResult, Kernel, KernelError, ShapeId};
use cad_kernel_occt::OcctKernel;
use cad_kernel_occt::worker::{WorkerConfig, WorkerKernel};
use cad_math::{Tolerance, Vec3};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const WORKER: &str = env!("CARGO_BIN_EXE_occt-worker");

fn occt_present() -> bool {
    if cad_kernel_occt::available() {
        return true;
    }
    let required = std::env::var_os("OCCT_REQUIRED").is_some_and(|v| !v.is_empty());
    assert!(
        !required,
        "OCCT_REQUIRED is set, but this build has no OCCT"
    );
    eprintln!("SKIPPED: this build was made without OCCT");
    false
}

fn worker() -> WorkerKernel {
    WorkerKernel::new(WorkerConfig::new(WORKER))
}

/// A worker that sleeps before answering each call, so a call can be caught in progress.
fn slow_worker(delay_ms: u64, timeout: Duration) -> WorkerKernel {
    let mut config = WorkerConfig::new(WORKER);
    config.args = vec!["--test-delay-ms".into(), delay_ms.to_string().into()];
    config.timeout = timeout;
    WorkerKernel::new(config)
}

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn pocket_cut(k: &dyn Kernel) -> KResult<(ShapeId, ShapeId, cad_kernel::OpResult)> {
    let a = k.make_box(Vec3::ZERO, v(40.0, 20.0, 10.0))?;
    let b = k.make_box(v(10.0, 5.0, 5.0), v(20.0, 10.0, 10.0))?;
    Ok((a, b, k.boolean(BoolOp::Subtract, a, b)?))
}

#[test]
fn worker_gives_the_same_answers_as_the_in_process_kernel() -> TestResult {
    if !occt_present() {
        return Ok(());
    }
    let (direct, remote) = (OcctKernel::new()?, worker());
    assert_eq!(remote.name(), "occt-worker");
    let (da, db, dcut) = pocket_cut(&direct)?;
    let (ra, rb, rcut) = pocket_cut(&remote)?;

    // History is identical once each side's own handles are swapped for operand numbers.
    let operands = |r: &cad_kernel::OpResult, a: ShapeId, b: ShapeId| -> Vec<_> {
        let faces = r.history.faces.iter();
        faces
            .map(|e| {
                let operand = if e.source_shape == a { 0 } else { 1 };
                assert!(
                    e.source_shape == a || e.source_shape == b,
                    "foreign handle in history"
                );
                (operand, e.source_face, e.fate, e.result_faces.clone())
            })
            .collect()
    };
    assert_eq!(operands(&rcut, ra, rb), operands(&dcut, da, db));
    assert_eq!(rcut.history.generated_faces, dcut.history.generated_faces);

    assert_eq!(
        remote.face_count(rcut.shape)?,
        direct.face_count(dcut.shape)?
    );
    assert_eq!(
        remote.mass_props(rcut.shape)?,
        direct.mass_props(dcut.shape)?
    );
    let tol = Tolerance::default();
    assert_eq!(
        remote.tessellate(rcut.shape, tol)?,
        direct.tessellate(dcut.shape, tol)?
    );
    assert_eq!(remote.workers_lost(), 0);
    Ok(())
}

#[test]
fn kernel_errors_cross_the_process_boundary_with_our_handles() -> TestResult {
    if !occt_present() {
        return Ok(());
    }
    let k = worker();
    let bad = k.make_box(Vec3::ZERO, v(1.0, -1.0, 1.0));
    assert!(matches!(bad, Err(KernelError::InvalidInput(_))), "{bad:?}");

    let a = k.make_box(Vec3::ZERO, v(1.0, 1.0, 1.0))?;
    let b = k.make_box(Vec3::ZERO, v(2.0, 2.0, 2.0))?;
    k.release(a);
    k.release(a);
    assert_eq!(k.face_count(a), Err(KernelError::UnknownShape(a)));
    assert_eq!(
        k.boolean(BoolOp::Union, b, a).map(|_| ()),
        Err(KernelError::UnknownShape(a))
    );
    assert_eq!(
        k.mass_props(ShapeId(999)),
        Err(KernelError::UnknownShape(ShapeId(999)))
    );
    assert_eq!(k.face_count(b)?, 6);
    // None of that cost a worker.
    assert_eq!(k.workers_lost(), 0);
    Ok(())
}

#[test]
fn killing_the_worker_mid_call_returns_backend_lost_and_the_next_call_succeeds() -> TestResult {
    if !occt_present() {
        return Ok(());
    }
    // Every call takes at least 400 ms, so the kill at 100 ms lands in the middle of one.
    let k = Arc::new(slow_worker(400, Duration::from_secs(30)));
    let survivor = k.make_box(Vec3::ZERO, v(1.0, 1.0, 1.0))?;

    let caller = Arc::clone(&k);
    let call = std::thread::spawn(move || {
        let started = Instant::now();
        (
            caller.make_box(Vec3::ZERO, v(2.0, 2.0, 2.0)),
            started.elapsed(),
        )
    });
    std::thread::sleep(Duration::from_millis(100));
    k.kill_worker();
    let (result, took) = call.join().map_err(|_| "caller panicked")?;

    assert!(
        matches!(result, Err(KernelError::BackendLost(_))),
        "{result:?}"
    );
    assert!(
        took < Duration::from_millis(390),
        "the call ran to completion: {took:?}"
    );
    assert_eq!(k.workers_lost(), 1);

    // The next call starts a new worker and succeeds.
    let fresh = k.make_box(Vec3::ZERO, v(3.0, 3.0, 3.0))?;
    assert_eq!(k.face_count(fresh)?, 6);
    // Shapes from the dead worker are gone, and say so.
    assert_eq!(
        k.face_count(survivor),
        Err(KernelError::UnknownShape(survivor))
    );
    assert_eq!(k.workers_lost(), 1);
    Ok(())
}

#[test]
fn a_call_that_exceeds_the_timeout_is_abandoned_and_the_worker_replaced() -> TestResult {
    if !occt_present() {
        return Ok(());
    }
    let k = slow_worker(600, Duration::from_millis(150));
    let started = Instant::now();
    let result = k.make_box(Vec3::ZERO, v(1.0, 1.0, 1.0));
    let took = started.elapsed();
    match &result {
        Err(KernelError::BackendLost(why)) => assert!(why.contains("no answer"), "{why}"),
        other => panic!("expected a timeout, got {other:?}"),
    }
    assert!(
        took < Duration::from_millis(550),
        "waited for the worker anyway: {took:?}"
    );
    assert_eq!(k.workers_lost(), 1);
    Ok(())
}

#[test]
fn a_worker_that_cannot_start_is_reported_per_call() {
    // Needs no OCCT: the program does not exist.
    let k = WorkerKernel::new(WorkerConfig::new("/nonexistent/occt-worker"));
    for _ in 0..2 {
        let result = k.make_box(Vec3::ZERO, v(1.0, 1.0, 1.0));
        match &result {
            Err(KernelError::BackendLost(why)) => assert!(why.contains("cannot start"), "{why}"),
            other => panic!("expected BackendLost, got {other:?}"),
        }
    }
    assert_eq!(k.workers_lost(), 0, "no worker ever ran");
}

#[test]
fn calls_from_several_threads_share_one_worker() -> TestResult {
    if !occt_present() {
        return Ok(());
    }
    let k = Arc::new(worker());
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let k = Arc::clone(&k);
            std::thread::spawn(move || -> KResult<f64> {
                let (_, _, cut) = pocket_cut(k.as_ref())?;
                Ok(k.mass_props(cut.shape)?.volume)
            })
        })
        .collect();
    for thread in threads {
        let volume = thread.join().map_err(|_| "thread panicked")??;
        assert!(
            Tolerance {
                linear: 1e-3,
                ..Tolerance::default()
            }
            .eq_len(volume, 7000.0)
        );
    }
    assert_eq!(k.workers_lost(), 0);
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Benchmark
// ---------------------------------------------------------------------------------------

/// A slab with an `n` by `n` grid of square pockets: 6 + 5 n^2 faces.
fn pocketed_slab(k: &dyn Kernel, n: u32) -> KResult<ShapeId> {
    let side = 10.0 * f64::from(n) + 10.0;
    let mut slab = k.make_box(Vec3::ZERO, v(side, side, 10.0))?;
    for i in 0..n {
        for j in 0..n {
            let corner = v(7.0 + 10.0 * f64::from(i), 7.0 + 10.0 * f64::from(j), 5.0);
            let tool = k.make_box(corner, v(6.0, 6.0, 10.0))?;
            let cut = k.boolean(BoolOp::Subtract, slab, tool)?;
            k.release(slab);
            k.release(tool);
            slab = cut.shape;
        }
    }
    Ok(slab)
}

fn median_micros(runs: usize, mut call: impl FnMut() -> KResult<Duration>) -> KResult<f64> {
    let mut times = (0..runs).map(|_| call()).collect::<KResult<Vec<_>>>()?;
    times.sort_unstable();
    Ok(times[times.len() / 2].as_secs_f64() * 1e6)
}

fn timed<T>(call: impl FnOnce() -> KResult<T>) -> KResult<(T, Duration)> {
    let started = Instant::now();
    let value = call()?;
    Ok((value, started.elapsed()))
}

#[test]
#[ignore = "benchmark; run with --release -- --ignored --nocapture"]
fn bench_in_process_versus_worker() -> TestResult {
    if !occt_present() {
        return Ok(());
    }
    let direct = OcctKernel::new()?;
    let remote = worker();
    let kernels: [(&str, &dyn Kernel); 2] = [("in-process", &direct), ("worker", &remote)];
    let tol = Tolerance::default();
    let unit = v(40.0, 20.0, 10.0);
    println!(
        "\nOCCT {}, worker binary {WORKER}",
        OcctKernel::occt_version()
    );

    let (_, spawn) = timed(|| remote.make_box(Vec3::ZERO, unit))?;
    println!(
        "first call to a new worker (process start included): {:.1} ms",
        spawn.as_secs_f64() * 1e3
    );

    println!(
        "\n{:<34} {:>14} {:>14} {:>12}",
        "call (median)", "in-process", "worker", "added"
    );
    let row = |label: &str, micros: [f64; 2]| {
        println!(
            "{label:<34} {:>11.1} us {:>11.1} us {:>9.1} us",
            micros[0],
            micros[1],
            micros[1] - micros[0]
        );
    };

    let mut results = [0.0; 2];
    for (slot, (_, k)) in results.iter_mut().zip(kernels) {
        *slot = median_micros(300, || {
            let (shape, took) = timed(|| k.make_box(Vec3::ZERO, unit))?;
            k.release(shape);
            Ok(took)
        })?;
    }
    row("make_box", results);

    for (slot, (_, k)) in results.iter_mut().zip(kernels) {
        let shape = k.make_box(Vec3::ZERO, unit)?;
        *slot = median_micros(300, || Ok(timed(|| k.mass_props(shape))?.1))?;
    }
    row("mass_props (box)", results);

    for (slot, (_, k)) in results.iter_mut().zip(kernels) {
        let shape = k.make_box(Vec3::ZERO, unit)?;
        *slot = median_micros(300, || Ok(timed(|| k.face_count(shape))?.1))?;
    }
    row("face_count", results);

    for (slot, (_, k)) in results.iter_mut().zip(kernels) {
        let a = k.make_box(Vec3::ZERO, unit)?;
        let b = k.make_box(v(10.0, 5.0, 5.0), v(20.0, 10.0, 10.0))?;
        *slot = median_micros(100, || {
            let (cut, took) = timed(|| k.boolean(BoolOp::Subtract, a, b))?;
            k.release(cut.shape);
            Ok(took)
        })?;
    }
    row("boolean (box minus pocket)", results);

    for n in [1u32, 5, 16] {
        let mut cold = [0.0; 2];
        let mut triangles = 0;
        let mut build = [0.0; 2];
        for (index, (_, k)) in kernels.iter().enumerate() {
            let (shape, took) = timed(|| pocketed_slab(*k, n))?;
            build[index] = took.as_secs_f64() * 1e3;
            // First tessellation computes the mesh; later ones reuse it and only copy it out.
            let (mesh, first) = timed(|| k.tessellate(shape, tol))?;
            cold[index] = first.as_secs_f64() * 1e6;
            triangles = mesh.triangles.len();
            results[index] = median_micros(60, || Ok(timed(|| k.tessellate(shape, tol))?.1))?;
        }
        let bytes = {
            let (_, k) = kernels[0];
            let shape = pocketed_slab(k, n)?;
            let mesh = k.tessellate(shape, tol)?;
            mesh.positions.len() * 48 + mesh.triangles.len() * 16 + 16
        };
        row(&format!("tessellate, {triangles} triangles"), results);
        row("  first call (mesh computed)", cold);
        println!(
            "  {} booleans to build: {:.0} ms in-process, {:.0} ms worker; mesh is {:.1} kB on the wire",
            n * n,
            build[0],
            build[1],
            bytes as f64 / 1e3
        );
    }
    Ok(())
}
