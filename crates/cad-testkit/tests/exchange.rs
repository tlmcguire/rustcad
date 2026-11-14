//! Acceptance tests for T008: B-rep and STEP exchange, checked against golden cases.
//!
//! The OCCT tests run only in builds linked to OCCT. Without it each prints `SKIPPED` and
//! passes, unless `OCCT_REQUIRED` is set, in which case it fails.

use cad_kernel::{BoolOp, Kernel, KernelError, ShapeId};
use cad_kernel_occt::OcctKernel;
use cad_math::{Tolerance, Vec3};
use cad_testkit::{ReferenceKernel, assert_golden};

const BOX_GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/box_40x20x10.golden");
const POCKET_GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/box_40x20x10_minus_pocket.golden"
);

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn occt() -> Option<OcctKernel> {
    match OcctKernel::new() {
        Ok(kernel) => Some(kernel),
        Err(e) => {
            let required = std::env::var_os("OCCT_REQUIRED").is_some_and(|v| !v.is_empty());
            assert!(!required, "OCCT_REQUIRED is set, but: {e}");
            eprintln!("SKIPPED: {e}");
            None
        }
    }
}

fn base_box(k: &dyn Kernel) -> Result<ShapeId, KernelError> {
    k.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))
}

/// The T003 box with a 20 x 10 x 5 pocket in its top face.
fn pocket(k: &dyn Kernel) -> Result<ShapeId, KernelError> {
    let a = base_box(k)?;
    let b = k.make_box(Vec3::new(10.0, 5.0, 5.0), Vec3::new(20.0, 10.0, 10.0))?;
    Ok(k.boolean(BoolOp::Subtract, a, b)?.shape)
}

/// Any failure that is reported as data, rather than a crash or a lost backend.
fn rejected<T: std::fmt::Debug>(got: &Result<T, KernelError>) -> bool {
    matches!(
        got,
        Err(KernelError::InvalidInput(_) | KernelError::OperationFailed(_))
    )
}

#[test]
fn the_reference_kernel_does_not_exchange() -> TestResult {
    let k = ReferenceKernel::new();
    let shape = base_box(&k)?;
    assert!(matches!(
        k.to_brep_bytes(shape),
        Err(KernelError::Unsupported(_))
    ));
    assert!(matches!(
        k.export_step(shape),
        Err(KernelError::Unsupported(_))
    ));
    assert!(matches!(
        k.from_brep_bytes(b"x"),
        Err(KernelError::Unsupported(_))
    ));
    assert!(matches!(
        k.import_step(b"x"),
        Err(KernelError::Unsupported(_))
    ));
    Ok(())
}

#[test]
fn a_step_round_trip_passes_the_golden_cases() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    for (shape, golden) in [(base_box(&k)?, BOX_GOLDEN), (pocket(&k)?, POCKET_GOLDEN)] {
        let step = k.export_step(shape)?;
        let text = String::from_utf8_lossy(&step);
        assert!(text.starts_with("ISO-10303-21;"), "not a STEP file");
        assert!(text.contains("MILLI"), "STEP file is not in millimetres");
        let back = k.import_step(&step)?;
        assert_golden(&k, back, golden);
    }
    Ok(())
}

#[test]
fn a_step_file_read_by_a_fresh_kernel_passes_the_golden_case() -> TestResult {
    let Some(writer) = occt() else { return Ok(()) };
    let step = writer.export_step(pocket(&writer)?)?;
    drop(writer);
    let reader = OcctKernel::new()?;
    let back = reader.import_step(&step)?;
    assert_golden(&reader, back, POCKET_GOLDEN);
    Ok(())
}

#[test]
fn a_brep_round_trip_passes_the_golden_cases() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    for (shape, golden) in [(base_box(&k)?, BOX_GOLDEN), (pocket(&k)?, POCKET_GOLDEN)] {
        let back = k.from_brep_bytes(&k.to_brep_bytes(shape)?)?;
        assert_golden(&k, back, golden);
    }
    Ok(())
}

#[test]
fn brep_bytes_are_repeatable() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let shape = pocket(&k)?;
    let first = k.to_brep_bytes(shape)?;
    assert_eq!(first, k.to_brep_bytes(shape)?, "same shape, same kernel");

    // Tessellating stores a triangulation on the shape; it must not reach the bytes.
    k.tessellate(shape, Tolerance::default())?;
    assert_eq!(first, k.to_brep_bytes(shape)?, "after tessellation");

    // The same construction in another kernel gives the same bytes.
    let other = OcctKernel::new()?;
    assert_eq!(first, other.to_brep_bytes(pocket(&other)?)?, "fresh kernel");

    // Reading the bytes back and writing them again changes nothing.
    let back = k.from_brep_bytes(&first)?;
    assert_eq!(first, k.to_brep_bytes(back)?, "after a round trip");
    Ok(())
}

#[test]
fn bad_input_is_rejected_as_data() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    assert!(matches!(
        k.import_step(b""),
        Err(KernelError::InvalidInput(_))
    ));
    assert!(matches!(
        k.from_brep_bytes(b""),
        Err(KernelError::InvalidInput(_))
    ));

    let noise: Vec<u8> = (0..4096u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let shape = pocket(&k)?;
    let step = k.export_step(shape)?;
    let brep = k.to_brep_bytes(shape)?;
    let inputs: [&[u8]; 4] = [
        b"not a shape",
        &noise,
        &step[..step.len() / 2],
        b"ISO-10303-21;",
    ];
    for input in inputs {
        let got = k.import_step(input);
        assert!(rejected(&got), "STEP {} bytes: {got:?}", input.len());
    }
    let inputs: [&[u8]; 3] = [b"not a shape", &noise, &brep[..brep.len() / 2]];
    for input in inputs {
        let got = k.from_brep_bytes(input);
        assert!(rejected(&got), "B-rep {} bytes: {got:?}", input.len());
    }

    // The kernel is still usable afterwards.
    assert_golden(&k, k.import_step(&step)?, POCKET_GOLDEN);
    Ok(())
}

#[test]
fn exchange_reports_released_handles() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let shape = base_box(&k)?;
    k.release(shape);
    assert_eq!(
        k.to_brep_bytes(shape),
        Err(KernelError::UnknownShape(shape))
    );
    assert_eq!(k.export_step(shape), Err(KernelError::UnknownShape(shape)));
    Ok(())
}
