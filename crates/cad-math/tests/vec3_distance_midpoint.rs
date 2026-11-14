//! Acceptance tests for T028: `Vec3::distance` and `Vec3::midpoint`.

use cad_math::{Tolerance, Vec3};

fn tol() -> Tolerance {
    Tolerance::default()
}

fn sample_points() -> [Vec3; 5] {
    [
        Vec3::new(1.5, -2.25, 3.0),
        Vec3::new(-7.0, 0.5, 11.125),
        Vec3::new(100.0, 200.0, -300.0),
        Vec3::new(0.001, -0.002, 0.003),
        Vec3::new(-4.0, 9.0, -2.5),
    ]
}

#[test]
fn distance_from_origin_to_3_4_0_is_5() {
    let d = Vec3::ZERO.distance(Vec3::new(3.0, 4.0, 0.0));
    assert!(tol().eq_len(d, 5.0), "got {d}");
}

#[test]
fn distance_handles_all_three_axes() {
    // (1, 2, 2) has length 3; offset from a non-origin start.
    let a = Vec3::new(10.0, -5.0, 7.0);
    let b = Vec3::new(11.0, -3.0, 9.0);
    let d = a.distance(b);
    assert!(tol().eq_len(d, 3.0), "got {d}");
}

#[test]
fn distance_from_a_point_to_itself_is_zero() {
    for p in sample_points() {
        let d = p.distance(p);
        assert!(tol().is_zero_len(d), "{p:?}: got {d}");
    }
}

#[test]
fn distance_is_never_negative() {
    let pts = sample_points();
    for a in pts {
        for b in pts {
            assert!(a.distance(b) >= 0.0, "{a:?} {b:?}");
        }
    }
}

#[test]
fn distance_is_symmetric() {
    let pts = sample_points();
    for a in pts {
        for b in pts {
            let ab = a.distance(b);
            let ba = b.distance(a);
            assert!(tol().eq_len(ab, ba), "{a:?} {b:?}: {ab} vs {ba}");
        }
    }
}

#[test]
fn midpoint_of_origin_and_2_4_neg6_is_1_2_neg3() {
    let m = Vec3::ZERO.midpoint(Vec3::new(2.0, 4.0, -6.0));
    assert!(tol().eq_point(m, Vec3::new(1.0, 2.0, -3.0)), "got {m:?}");
}

#[test]
fn midpoint_of_non_origin_points() {
    let m = Vec3::new(-1.0, 3.0, 5.0).midpoint(Vec3::new(3.0, 7.0, -1.0));
    assert!(tol().eq_point(m, Vec3::new(1.0, 5.0, 2.0)), "got {m:?}");
}

#[test]
fn midpoint_of_a_point_with_itself_is_the_point() {
    for a in sample_points() {
        let m = a.midpoint(a);
        assert!(tol().eq_point(m, a), "{a:?}: got {m:?}");
    }
}

#[test]
fn midpoint_is_symmetric() {
    let pts = sample_points();
    for a in pts {
        for b in pts {
            let ab = a.midpoint(b);
            let ba = b.midpoint(a);
            assert!(tol().eq_point(ab, ba), "{a:?} {b:?}: {ab:?} vs {ba:?}");
        }
    }
}

#[test]
fn midpoint_is_equidistant_from_both_points() {
    let pts = sample_points();
    for a in pts {
        for b in pts {
            let m = a.midpoint(b);
            let da = m.distance(a);
            let db = m.distance(b);
            assert!(tol().eq_len(da, db), "{a:?} {b:?}: {da} vs {db}");
            // And it lies halfway: each half is half the whole.
            let whole = a.distance(b);
            assert!(
                tol().eq_len(da * 2.0, whole),
                "{a:?} {b:?}: {da} vs {whole}"
            );
        }
    }
}
