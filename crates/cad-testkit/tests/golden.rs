//! Acceptance tests for T002: the golden geometry harness.

use cad_kernel::{BoolOp, KResult, Kernel, KernelError, MassProps, Mesh, OpResult, ShapeId};
use cad_math::{Tolerance, Vec3};
use cad_testkit::{Golden, GoldenError, ReferenceKernel, assert_golden};

const BOX_GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/box_40x20x10.golden");

const VALID: &str = "\
# a comment

volume = 8000
area = 2800
bbox_min = 0 0 0
bbox_max = 40 20 10
face_count = 6
tolerance = 1e-6
source = analytic
";

/// A second backend for the harness: the reference kernel with selected results falsified.
#[derive(Default)]
struct Lying {
    inner: ReferenceKernel,
    extra_faces: usize,
    drop_mesh: bool,
}

impl Kernel for Lying {
    fn name(&self) -> &'static str {
        "lying"
    }
    fn make_box(&self, origin: Vec3, size: Vec3) -> KResult<ShapeId> {
        self.inner.make_box(origin, size)
    }
    fn boolean(&self, op: BoolOp, a: ShapeId, b: ShapeId) -> KResult<OpResult> {
        self.inner.boolean(op, a, b)
    }
    fn face_count(&self, shape: ShapeId) -> KResult<usize> {
        Ok(self.inner.face_count(shape)? + self.extra_faces)
    }
    fn tessellate(&self, shape: ShapeId, tol: Tolerance) -> KResult<Mesh> {
        let mesh = self.inner.tessellate(shape, tol)?;
        Ok(if self.drop_mesh {
            Mesh::default()
        } else {
            mesh
        })
    }
    fn mass_props(&self, shape: ShapeId) -> KResult<MassProps> {
        self.inner.mass_props(shape)
    }
    fn release(&self, shape: ShapeId) {
        self.inner.release(shape);
    }
}

fn mismatched_fields(result: Result<(), GoldenError>) -> Vec<&'static str> {
    match result {
        Err(GoldenError::Mismatch(list)) => list.iter().map(|m| m.field).collect(),
        other => panic!("expected a mismatch, got {other:?}"),
    }
}

fn parse_error(text: &str) -> GoldenError {
    match Golden::parse(text) {
        Err(e) => e,
        Ok(g) => panic!("expected a parse failure, got {g:?}"),
    }
}

#[test]
fn golden_file_records_every_required_value() -> Result<(), GoldenError> {
    let g = Golden::load(BOX_GOLDEN)?;
    let tol = Tolerance::default();
    assert!(tol.eq_len(g.volume, 8000.0));
    assert!(tol.eq_len(g.area, 2800.0));
    assert!(tol.eq_point(g.bbox_min, Vec3::ZERO));
    assert!(tol.eq_point(g.bbox_max, Vec3::new(40.0, 20.0, 10.0)));
    assert_eq!(g.face_count, 6);
    assert!(tol.eq_len(g.tolerance, 1e-6));
    assert!(g.source.starts_with("analytic"), "source: {}", g.source);
    Ok(())
}

#[test]
fn analytic_box_case_passes() -> KResult<()> {
    let k = ReferenceKernel::new();
    let shape = k.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))?;
    assert_golden(&k, shape, BOX_GOLDEN);
    Ok(())
}

#[test]
fn same_case_runs_against_any_dyn_kernel() -> KResult<()> {
    let kernels: Vec<Box<dyn Kernel>> =
        vec![Box::new(ReferenceKernel::new()), Box::new(Lying::default())];
    for k in &kernels {
        let shape = k.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))?;
        assert_golden(k.as_ref(), shape, BOX_GOLDEN);
    }
    Ok(())
}

#[test]
fn values_inside_the_tolerance_pass() -> Result<(), GoldenError> {
    let k = ReferenceKernel::new();
    let shape = k.make_box(Vec3::ZERO, Vec3::new(40.0 + 4e-7, 20.0, 10.0))?;
    Golden::load(BOX_GOLDEN)?.check(&k, shape)
}

#[test]
fn each_mismatched_field_is_named() -> Result<(), GoldenError> {
    let golden = Golden::load(BOX_GOLDEN)?;

    // One millimetre too long in X: volume, area, and the high corner change. Nothing else.
    let k = ReferenceKernel::new();
    let longer = k.make_box(Vec3::ZERO, Vec3::new(41.0, 20.0, 10.0))?;
    assert_eq!(
        mismatched_fields(golden.check(&k, longer)),
        ["volume", "area", "bbox_max"]
    );

    // Right size, wrong place: only the bounding box changes.
    let moved = k.make_box(Vec3::new(0.0, 0.0, 5.0), Vec3::new(40.0, 20.0, 10.0))?;
    assert_eq!(
        mismatched_fields(golden.check(&k, moved)),
        ["bbox_min", "bbox_max"]
    );

    // Wrong topology only.
    let k = Lying {
        extra_faces: 1,
        ..Lying::default()
    };
    let shape = k.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))?;
    assert_eq!(mismatched_fields(golden.check(&k, shape)), ["face_count"]);

    // An empty tessellation has no bounding box.
    let k = Lying {
        drop_mesh: true,
        ..Lying::default()
    };
    let shape = k.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))?;
    assert_eq!(
        mismatched_fields(golden.check(&k, shape)),
        ["bbox_min", "bbox_max"]
    );
    Ok(())
}

#[test]
fn mismatch_message_shows_expected_and_actual() -> Result<(), GoldenError> {
    let k = Lying {
        extra_faces: 2,
        ..Lying::default()
    };
    let shape = k.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))?;
    let message = match Golden::load(BOX_GOLDEN)?.check(&k, shape) {
        Err(e) => e.to_string(),
        Ok(()) => panic!("expected a mismatch"),
    };
    assert!(
        message.contains("face_count: expected 6, got 8"),
        "{message}"
    );
    Ok(())
}

#[test]
#[should_panic(expected = "volume: expected 8000")]
fn assert_golden_panics_naming_the_field() {
    let k = ReferenceKernel::new();
    match k.make_box(Vec3::ZERO, Vec3::new(41.0, 20.0, 10.0)) {
        Ok(shape) => assert_golden(&k, shape, BOX_GOLDEN),
        Err(e) => eprintln!("setup failed, test will fail for lack of a panic: {e}"),
    }
}

#[test]
#[should_panic(expected = "cannot read golden file")]
fn assert_golden_panics_on_a_missing_file() {
    let k = ReferenceKernel::new();
    assert_golden(&k, ShapeId(0), "does/not/exist.golden");
}

#[test]
fn kernel_failures_are_reported_as_kernel_errors() -> Result<(), GoldenError> {
    let k = ReferenceKernel::new();
    let got = Golden::load(BOX_GOLDEN)?.check(&k, ShapeId(7));
    assert_eq!(
        got,
        Err(GoldenError::Kernel(KernelError::UnknownShape(ShapeId(7))))
    );
    Ok(())
}

#[test]
fn valid_text_parses() -> Result<(), GoldenError> {
    let g = Golden::parse(VALID)?;
    assert_eq!(g.face_count, 6);
    assert_eq!(g.source, "analytic");
    Ok(())
}

#[test]
fn source_may_contain_equals_signs() -> Result<(), GoldenError> {
    let g = Golden::parse(&VALID.replace("source = analytic", "source = V = abc"))?;
    assert_eq!(g.source, "V = abc");
    Ok(())
}

#[test]
fn malformed_lines_are_rejected_with_their_line_number() {
    let cases = [
        ("volume = 8000", "volume = lots", 3, "not a finite number"),
        ("volume = 8000", "volume = inf", 3, "not a finite number"),
        ("volume = 8000", "volume = NaN", 3, "not a finite number"),
        ("area = 2800", "area 2800", 4, "key = value"),
        ("bbox_min = 0 0 0", "bbox_min = 0 0", 5, "three numbers"),
        (
            "bbox_max = 40 20 10",
            "bbox_max = 40 20 10 0",
            6,
            "three numbers",
        ),
        ("face_count = 6", "face_count = 6.5", 7, "whole number"),
        ("face_count = 6", "face_count = -1", 7, "whole number"),
        (
            "tolerance = 1e-6",
            "tolerence = 1e-6",
            8,
            "unknown key `tolerence`",
        ),
        ("source = analytic", "source =", 9, "must not be empty"),
        (
            "source = analytic",
            "source = analytic\nvolume = 1",
            10,
            "duplicate key `volume`",
        ),
    ];
    for (from, to, want_line, want_text) in cases {
        match parse_error(&VALID.replace(from, to)) {
            GoldenError::Parse { line, message } => {
                assert_eq!(line, want_line, "{to}: {message}");
                assert!(message.contains(want_text), "{to}: {message}");
            }
            other => panic!("{to}: expected a parse error, got {other:?}"),
        }
    }
}

#[test]
fn every_key_is_required() {
    for key in [
        "volume",
        "area",
        "bbox_min",
        "bbox_max",
        "face_count",
        "tolerance",
        "source",
    ] {
        let without: String = VALID
            .lines()
            .filter(|l| !l.starts_with(key))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(parse_error(&without), GoldenError::MissingKey(key));
    }
}

#[test]
fn contradictory_values_are_rejected() {
    let cases = [
        ("tolerance = 1e-6", "tolerance = 0"),
        ("tolerance = 1e-6", "tolerance = -1e-6"),
        ("volume = 8000", "volume = -8000"),
        ("area = 2800", "area = -1"),
        ("bbox_min = 0 0 0", "bbox_min = 0 30 0"),
    ];
    for (from, to) in cases {
        let got = parse_error(&VALID.replace(from, to));
        assert!(matches!(got, GoldenError::Invalid(_)), "{to}: {got:?}");
    }
}
