//! Acceptance tests for T008 through the worker process: B-rep and STEP exchange.
//!
//! These start the real `occt-worker` binary, so they need a build linked to OCCT. Without
//! it each test prints `SKIPPED` and passes, unless `OCCT_REQUIRED` is set.
//!
//! The golden files live in `cad-testkit`, a higher layer, so the pocket's expected values
//! are repeated here from `box_40x20x10_minus_pocket.golden`.

use cad_kernel::{BoolOp, KResult, Kernel, KernelError, MassProps, ShapeId};
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

fn pocket(k: &dyn Kernel) -> KResult<ShapeId> {
    let a = k.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))?;
    let b = k.make_box(Vec3::new(10.0, 5.0, 5.0), Vec3::new(20.0, 10.0, 10.0))?;
    Ok(k.boolean(BoolOp::Subtract, a, b)?.shape)
}

/// The values of `box_40x20x10_minus_pocket.golden`, at its relative tolerance of 1e-6.
fn assert_pocket(k: &dyn Kernel, shape: ShapeId) -> TestResult {
    let MassProps {
        volume,
        area,
        center,
    } = k.mass_props(shape)?;
    let near = |got: f64, want: f64| (got - want).abs() <= 1e-6 * want;
    assert!(near(volume, 7000.0), "volume {volume}");
    assert!(near(area, 3100.0), "area {area}");
    // Centre of the box minus the pocket's share: z = (8000 * 5 - 1000 * 7.5) / 7000.
    let want = Vec3::new(20.0, 10.0, (40_000.0 - 7_500.0) / 7_000.0);
    let tol = Tolerance {
        linear: 1e-6 * 50.0,
        ..Tolerance::default()
    };
    assert!(tol.eq_point(center, want), "centre {center:?}");
    assert_eq!(k.face_count(shape)?, 11);
    Ok(())
}

#[test]
fn step_and_brep_round_trips_work_through_the_worker() -> TestResult {
    if !occt_present() {
        return Ok(());
    }
    let k = worker();
    let shape = pocket(&k)?;

    let step = k.export_step(shape)?;
    assert!(step.starts_with(b"ISO-10303-21;"), "not a STEP file");
    assert_pocket(&k, k.import_step(&step)?)?;

    let brep = k.to_brep_bytes(shape)?;
    assert_pocket(&k, k.from_brep_bytes(&brep)?)?;

    // Anything OCCT printed to standard output would have corrupted the protocol.
    assert_eq!(k.workers_lost(), 0);
    Ok(())
}

#[test]
fn the_worker_and_the_in_process_kernel_read_each_others_files() -> TestResult {
    if !occt_present() {
        return Ok(());
    }
    let (direct, remote) = (OcctKernel::new()?, worker());

    let brep = direct.to_brep_bytes(pocket(&direct)?)?;
    assert_eq!(
        remote.to_brep_bytes(pocket(&remote)?)?,
        brep,
        "same construction, same bytes"
    );
    assert_pocket(&remote, remote.from_brep_bytes(&brep)?)?;

    let step = remote.export_step(pocket(&remote)?)?;
    assert_pocket(&direct, direct.import_step(&step)?)?;
    assert_eq!(remote.workers_lost(), 0);
    Ok(())
}

#[test]
fn bad_input_through_the_worker_is_an_error_not_a_lost_worker() -> TestResult {
    if !occt_present() {
        return Ok(());
    }
    let k = worker();
    for got in [
        k.import_step(b""),
        k.import_step(b"not a shape"),
        k.from_brep_bytes(b""),
        k.from_brep_bytes(b"not a shape"),
    ] {
        assert!(
            matches!(
                got,
                Err(KernelError::InvalidInput(_) | KernelError::OperationFailed(_))
            ),
            "{got:?}"
        );
    }
    assert_eq!(k.workers_lost(), 0);
    assert_pocket(&k, pocket(&k)?)?;
    Ok(())
}

#[test]
fn exchange_on_a_lost_workers_shape_is_unknown_shape() -> TestResult {
    if !occt_present() {
        return Ok(());
    }
    let k = worker();
    let shape = pocket(&k)?;
    k.kill_worker();
    // The first call to notice the dead worker reports it; afterwards the handle is stale.
    let first = k.export_step(shape);
    assert!(
        matches!(
            first,
            Err(KernelError::BackendLost(_) | KernelError::UnknownShape(_))
        ),
        "{first:?}"
    );
    assert_eq!(
        k.to_brep_bytes(shape),
        Err(KernelError::UnknownShape(shape))
    );
    Ok(())
}
