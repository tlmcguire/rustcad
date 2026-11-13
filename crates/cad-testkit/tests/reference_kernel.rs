//! Acceptance tests for T001: the box-only reference kernel.

use cad_kernel::{BoolOp, FaceIndex, KResult, Kernel, KernelError, ShapeId};
use cad_math::{Tolerance, Vec3};
use cad_testkit::ReferenceKernel;

fn box_40_20_10(k: &dyn Kernel) -> KResult<ShapeId> {
    k.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))
}

#[test]
fn make_box_rejects_non_positive_sizes() {
    let k = ReferenceKernel::new();
    let bad = [
        Vec3::new(0.0, 20.0, 10.0),
        Vec3::new(40.0, -1.0, 10.0),
        Vec3::new(40.0, 20.0, 0.0),
        Vec3::new(-40.0, -20.0, -10.0),
        Vec3::new(f64::NAN, 20.0, 10.0),
        Vec3::new(40.0, f64::INFINITY, 10.0),
    ];
    for size in bad {
        let got = k.make_box(Vec3::ZERO, size);
        assert!(
            matches!(got, Err(KernelError::InvalidInput(_))),
            "size {size:?} gave {got:?}"
        );
    }
}

#[test]
fn make_box_rejects_non_finite_origin() {
    let k = ReferenceKernel::new();
    let got = k.make_box(Vec3::new(0.0, f64::NAN, 0.0), Vec3::new(1.0, 1.0, 1.0));
    assert!(matches!(got, Err(KernelError::InvalidInput(_))), "{got:?}");
}

#[test]
fn mass_props_of_40_20_10_box() -> KResult<()> {
    let k = ReferenceKernel::new();
    let tol = Tolerance::default();
    let m = k.mass_props(box_40_20_10(&k)?)?;
    assert!(tol.eq_len(m.volume, 8000.0), "volume {}", m.volume);
    assert!(tol.eq_len(m.area, 2800.0), "area {}", m.area);
    assert!(
        tol.eq_point(m.center, Vec3::new(20.0, 10.0, 5.0)),
        "centre {:?}",
        m.center
    );
    Ok(())
}

#[test]
fn mass_props_centre_follows_origin() -> KResult<()> {
    let k = ReferenceKernel::new();
    let s = k.make_box(Vec3::new(-5.0, 100.0, 2.5), Vec3::new(2.0, 4.0, 6.0))?;
    let m = k.mass_props(s)?;
    assert!(Tolerance::default().eq_point(m.center, Vec3::new(-4.0, 102.0, 5.5)));
    Ok(())
}

#[test]
fn box_has_six_faces() -> KResult<()> {
    let k = ReferenceKernel::new();
    assert_eq!(k.face_count(box_40_20_10(&k)?)?, 6);
    Ok(())
}

#[test]
fn tessellate_gives_twelve_tagged_triangles() -> KResult<()> {
    let k = ReferenceKernel::new();
    let mesh = k.tessellate(box_40_20_10(&k)?, Tolerance::default())?;
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
    Ok(())
}

#[test]
fn tessellation_is_a_closed_outward_facing_box() -> KResult<()> {
    let k = ReferenceKernel::new();
    let tol = Tolerance::default();
    let origin = Vec3::new(1.0, 2.0, 3.0);
    let size = Vec3::new(40.0, 20.0, 10.0);
    let mesh = k.tessellate(k.make_box(origin, size)?, tol)?;
    let center = origin + size * 0.5;

    let mut area = 0.0;
    let mut volume = 0.0;
    for tri in &mesh.triangles {
        let [a, b, c] = tri.map(|i| mesh.positions[i as usize]);
        let cross = (b - a).cross(c - a);
        area += cross.length() * 0.5;
        // Signed volume of the tetrahedron from the box centre; positive when wound outward.
        let signed = (a - center).dot((b - center).cross(c - center)) / 6.0;
        assert!(signed > 0.0, "triangle {tri:?} is wound inward");
        volume += signed;
        for i in tri {
            let normal = mesh.normals[*i as usize];
            assert!(tol.eq_len(normal.length(), 1.0));
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
fn tessellation_is_deterministic() -> KResult<()> {
    let k = ReferenceKernel::new();
    let s = box_40_20_10(&k)?;
    assert_eq!(
        k.tessellate(s, Tolerance::default())?,
        k.tessellate(s, Tolerance::default())?
    );
    Ok(())
}

#[test]
fn boolean_is_unsupported() -> KResult<()> {
    let k = ReferenceKernel::new();
    let a = box_40_20_10(&k)?;
    let b = box_40_20_10(&k)?;
    for op in [BoolOp::Union, BoolOp::Subtract, BoolOp::Intersect] {
        assert!(matches!(
            k.boolean(op, a, b),
            Err(KernelError::Unsupported(_))
        ));
    }
    Ok(())
}

#[test]
fn released_handle_is_unknown() -> KResult<()> {
    let k = ReferenceKernel::new();
    let s = box_40_20_10(&k)?;
    let other = box_40_20_10(&k)?;
    k.release(s);
    assert_eq!(k.face_count(s), Err(KernelError::UnknownShape(s)));
    assert_eq!(k.mass_props(s), Err(KernelError::UnknownShape(s)));
    assert_eq!(
        k.tessellate(s, Tolerance::default()),
        Err(KernelError::UnknownShape(s))
    );
    // Releasing twice is harmless, and other shapes are untouched.
    k.release(s);
    assert_eq!(k.face_count(other)?, 6);
    Ok(())
}

#[test]
fn handles_are_not_reused_after_release() -> KResult<()> {
    let k = ReferenceKernel::new();
    let first = box_40_20_10(&k)?;
    k.release(first);
    let second = box_40_20_10(&k)?;
    assert_ne!(first, second);
    assert_eq!(k.face_count(first), Err(KernelError::UnknownShape(first)));
    Ok(())
}

#[test]
fn never_issued_handle_is_unknown() {
    let k = ReferenceKernel::new();
    assert_eq!(
        k.face_count(ShapeId(42)),
        Err(KernelError::UnknownShape(ShapeId(42)))
    );
}

#[test]
fn works_through_a_trait_object_across_threads() -> KResult<()> {
    let k: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(ReferenceKernel::new());
    assert_eq!(k.name(), "reference");
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let k = std::sync::Arc::clone(&k);
            std::thread::spawn(move || box_40_20_10(k.as_ref()))
        })
        .collect();
    let mut ids = Vec::new();
    for h in handles {
        ids.push(
            h.join()
                .map_err(|_| KernelError::OperationFailed("thread panicked".into()))??,
        );
    }
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    assert_eq!(ids.len(), 8, "handles must be unique");
    Ok(())
}
