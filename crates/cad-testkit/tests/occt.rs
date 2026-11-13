//! Acceptance tests for T003: the OpenCascade backend.
//!
//! These run only in builds linked to OCCT. Without it each test prints `SKIPPED` and
//! passes, unless `OCCT_REQUIRED` is set, in which case it fails.

use std::collections::BTreeSet;

use cad_kernel::{BoolOp, FaceFate, FaceIndex, Kernel, KernelError, OpResult, ShapeId};
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

fn pocket_cut(k: &dyn Kernel) -> Result<(ShapeId, ShapeId, OpResult), KernelError> {
    let a = base_box(k)?;
    let b = k.make_box(Vec3::new(10.0, 5.0, 5.0), Vec3::new(20.0, 10.0, 10.0))?;
    Ok((a, b, k.boolean(BoolOp::Subtract, a, b)?))
}

/// Checks the invariants every history map must satisfy and returns the entries of one operand.
fn entries_of(result: &OpResult, k: &dyn Kernel, operand: ShapeId) -> Vec<(FaceFate, usize)> {
    let face_count = k.face_count(result.shape).unwrap_or(0);
    for entry in &result.history.faces {
        let expected = match entry.result_faces.len() {
            0 => FaceFate::Deleted,
            1 => FaceFate::Modified,
            _ => FaceFate::Split,
        };
        assert_eq!(
            entry.fate, expected,
            "fate disagrees with result count: {entry:?}"
        );
        for face in &entry.result_faces {
            assert!(
                (face.0 as usize) < face_count,
                "result face out of range: {entry:?}"
            );
        }
    }
    result
        .history
        .faces
        .iter()
        .filter(|e| e.source_shape == operand)
        .map(|e| (e.fate, e.result_faces.len()))
        .collect()
}

fn count(entries: &[(FaceFate, usize)], fate: FaceFate) -> usize {
    entries.iter().filter(|e| e.0 == fate).count()
}

fn covered_faces(result: &OpResult) -> BTreeSet<u32> {
    let from_inputs = result.history.faces.iter().flat_map(|e| &e.result_faces);
    from_inputs
        .chain(&result.history.generated_faces)
        .map(|f| f.0)
        .collect()
}

#[test]
fn reports_backend_and_version() {
    let Some(k) = occt() else { return };
    assert_eq!(k.name(), "occt");
    assert!(cad_kernel_occt::available());
    let version = OcctKernel::occt_version();
    assert!(
        version.starts_with("7.9"),
        "unexpected OCCT version {version}"
    );
}

#[test]
fn box_passes_the_same_golden_case_as_the_reference_kernel() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let kernels: [&dyn Kernel; 2] = [&k, &ReferenceKernel::new()];
    for kernel in kernels {
        assert_golden(kernel, base_box(kernel)?, BOX_GOLDEN);
    }
    Ok(())
}

#[test]
fn box_minus_box_passes_its_golden_case() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let (_, _, cut) = pocket_cut(&k)?;
    assert_golden(&k, cut.shape, POCKET_GOLDEN);
    Ok(())
}

#[test]
fn make_box_rejects_bad_sizes_before_reaching_occt() {
    let Some(k) = occt() else { return };
    for size in [
        Vec3::new(0.0, 1.0, 1.0),
        Vec3::new(1.0, -1.0, 1.0),
        Vec3::new(1.0, 1.0, f64::NAN),
    ] {
        let got = k.make_box(Vec3::ZERO, size);
        assert!(
            matches!(got, Err(KernelError::InvalidInput(_))),
            "{size:?}: {got:?}"
        );
    }
}

#[test]
fn released_and_unknown_handles_are_reported() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let a = base_box(&k)?;
    let b = base_box(&k)?;
    k.release(a);
    k.release(a);
    assert_eq!(k.face_count(a), Err(KernelError::UnknownShape(a)));
    assert_eq!(k.mass_props(a), Err(KernelError::UnknownShape(a)));
    assert_eq!(
        k.tessellate(a, Tolerance::default()),
        Err(KernelError::UnknownShape(a))
    );
    assert!(matches!(
        k.boolean(BoolOp::Union, a, b),
        Err(KernelError::UnknownShape(_))
    ));
    assert!(matches!(
        k.boolean(BoolOp::Union, b, ShapeId(99)),
        Err(KernelError::UnknownShape(_))
    ));
    assert_eq!(k.face_count(b)?, 6);
    Ok(())
}

#[test]
fn box_tessellation_is_closed_tagged_and_wound_outward() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let tol = Tolerance::default();
    let mesh = k.tessellate(base_box(&k)?, tol)?;
    assert_eq!(mesh.triangles.len(), 12);
    assert_eq!(mesh.triangle_faces.len(), 12);
    assert_eq!(mesh.normals.len(), mesh.positions.len());
    for face in 0..6 {
        let n = mesh
            .triangle_faces
            .iter()
            .filter(|f| **f == FaceIndex(face))
            .count();
        assert_eq!(n, 2, "face {face} has {n} triangles");
    }

    let center = Vec3::new(20.0, 10.0, 5.0);
    let (mut area, mut volume) = (0.0, 0.0);
    for tri in &mesh.triangles {
        let [a, b, c] = tri.map(|i| mesh.positions[i as usize]);
        let cross = (b - a).cross(c - a);
        area += cross.length() * 0.5;
        let signed = (a - center).dot((b - center).cross(c - center)) / 6.0;
        assert!(signed > 0.0, "triangle {tri:?} is wound inward");
        volume += signed;
        for i in tri {
            let normal = mesh.normals[*i as usize];
            assert!(
                tol.eq_len(normal.length(), 1.0),
                "normal {normal:?} is not unit length"
            );
            assert!(
                normal.dot(cross) > 0.0,
                "normal disagrees with winding at vertex {i}"
            );
        }
    }
    assert!(tol.eq_len(area, 2800.0), "mesh area {area}");
    assert!(tol.eq_len(volume, 8000.0), "mesh volume {volume}");
    Ok(())
}

#[test]
fn pocket_cut_history_accounts_for_every_face() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let (a, b, cut) = pocket_cut(&k)?;
    assert_eq!(k.face_count(cut.shape)?, 11);

    // The box keeps all six faces: five untouched, the top trimmed by the pocket.
    let from_box = entries_of(&cut, &k, a);
    assert_eq!(from_box.len(), 6);
    assert_eq!(count(&from_box, FaceFate::Modified), 6, "{from_box:?}");

    // The tool's top is outside the box and vanishes. Its floor and four walls line the pocket.
    let from_tool = entries_of(&cut, &k, b);
    assert_eq!(from_tool.len(), 6);
    assert_eq!(count(&from_tool, FaceFate::Modified), 5, "{from_tool:?}");
    assert_eq!(count(&from_tool, FaceFate::Deleted), 1, "{from_tool:?}");

    // Every output face has exactly one ancestor, and none appeared from nowhere.
    let all: Vec<u32> = cut
        .history
        .faces
        .iter()
        .flat_map(|e| &e.result_faces)
        .map(|f| f.0)
        .collect();
    assert_eq!(all.len(), 11, "an output face has two ancestors: {all:?}");
    assert_eq!(covered_faces(&cut), (0..11).collect());
    assert!(
        cut.history.generated_faces.is_empty(),
        "{:?}",
        cut.history.generated_faces
    );
    Ok(())
}

#[test]
fn a_through_slot_splits_the_faces_it_crosses() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let a = base_box(&k)?;
    // A slab across the middle of the box, larger than it in Y and Z: cuts it in two.
    let b = k.make_box(Vec3::new(15.0, -5.0, -5.0), Vec3::new(10.0, 30.0, 20.0))?;
    let cut = k.boolean(BoolOp::Subtract, a, b)?;
    assert_eq!(k.face_count(cut.shape)?, 12);

    // The two end faces survive whole. The four long faces are each split in two.
    let from_box = entries_of(&cut, &k, a);
    assert_eq!(count(&from_box, FaceFate::Modified), 2, "{from_box:?}");
    assert_eq!(count(&from_box, FaceFate::Split), 4, "{from_box:?}");
    assert!(
        from_box.iter().all(|e| e.0 != FaceFate::Split || e.1 == 2),
        "{from_box:?}"
    );

    // Only the slab's two X faces touch the box; they become the new end faces.
    let from_tool = entries_of(&cut, &k, b);
    assert_eq!(count(&from_tool, FaceFate::Modified), 2, "{from_tool:?}");
    assert_eq!(count(&from_tool, FaceFate::Deleted), 4, "{from_tool:?}");

    assert_eq!(covered_faces(&cut), (0..12).collect());
    assert!(cut.history.generated_faces.is_empty());

    let props = k.mass_props(cut.shape)?;
    let tol = Tolerance {
        linear: 1e-6 * 2800.0,
        ..Tolerance::default()
    };
    assert!(tol.eq_len(props.volume, 6000.0), "volume {}", props.volume);
    Ok(())
}

#[test]
fn union_and_intersection_have_the_expected_volume() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let tol = Tolerance {
        linear: 1e-6 * 2800.0,
        ..Tolerance::default()
    };
    let a = base_box(&k)?;
    // Same cross-section, overlapping the last 10 mm of the box and extending 10 mm past it.
    let b = k.make_box(Vec3::new(30.0, 0.0, 0.0), Vec3::new(20.0, 20.0, 10.0))?;

    let union = k.boolean(BoolOp::Union, a, b)?;
    let props = k.mass_props(union.shape)?;
    assert!(
        tol.eq_len(props.volume, 10_000.0),
        "union volume {}",
        props.volume
    );
    assert!(Tolerance::default().eq_point(props.center, Vec3::new(25.0, 10.0, 5.0)));
    entries_of(&union, &k, a);
    assert_eq!(covered_faces(&union).len(), k.face_count(union.shape)?);

    let common = k.boolean(BoolOp::Intersect, a, b)?;
    let props = k.mass_props(common.shape)?;
    assert!(
        tol.eq_len(props.volume, 2000.0),
        "intersection volume {}",
        props.volume
    );
    assert_eq!(k.face_count(common.shape)?, 6);
    entries_of(&common, &k, a);
    assert_eq!(covered_faces(&common), (0..6).collect());
    Ok(())
}

#[test]
fn inputs_survive_a_boolean_and_results_are_repeatable() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let (a, b, first) = pocket_cut(&k)?;
    assert_golden(&k, a, BOX_GOLDEN);
    assert_eq!(k.face_count(b)?, 6);

    // Same inputs, same history, in this kernel and in a fresh one.
    let again = k.boolean(BoolOp::Subtract, a, b)?;
    assert_eq!(first.history, again.history);
    let other = OcctKernel::new()?;
    let (_, _, fresh) = pocket_cut(&other)?;
    let strip = |r: &OpResult| -> Vec<(u32, FaceFate, Vec<FaceIndex>)> {
        let faces = r.history.faces.iter();
        faces
            .map(|e| (e.source_face.0, e.fate, e.result_faces.clone()))
            .collect()
    };
    assert_eq!(strip(&first), strip(&fresh));
    Ok(())
}

#[test]
fn calls_from_several_threads_are_serialised() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let k = std::sync::Arc::new(k);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let k = std::sync::Arc::clone(&k);
            std::thread::spawn(move || -> Result<f64, KernelError> {
                let (_, _, cut) = pocket_cut(k.as_ref())?;
                Ok(k.mass_props(cut.shape)?.volume)
            })
        })
        .collect();
    for worker in workers {
        let volume = worker.join().map_err(|_| "worker panicked")??;
        assert!(
            Tolerance {
                linear: 1e-3,
                ..Tolerance::default()
            }
            .eq_len(volume, 7000.0)
        );
    }
    Ok(())
}
