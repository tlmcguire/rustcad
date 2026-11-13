//! Acceptance tests for T006: stable face IDs through real boolean history.
//!
//! These need the OpenCascade backend, the only one with booleans. Without it each test
//! prints `SKIPPED` and passes, unless `OCCT_REQUIRED` is set, in which case it fails.

use std::collections::{BTreeMap, BTreeSet};

use cad_doc::naming::{
    FeatureId, NamedFaces, ReboundReason, Resolution, name_box, name_result, resolve,
};
use cad_kernel::{BoolOp, Kernel, ShapeId};
use cad_kernel_occt::OcctKernel;
use cad_math::{Tolerance, Vec3};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Named = Result<NamedFaces, Box<dyn std::error::Error>>;

const F1: FeatureId = FeatureId(1);
const F2: FeatureId = FeatureId(2);
const F3: FeatureId = FeatureId(3);

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

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

/// Feature 1: a box at the origin. Feature 2: a tool box. Feature 3: box minus tool.
/// Returns the names of the box alone and of the cut result. Uses a fresh kernel each time,
/// as a rebuild after a restart would.
fn cut(
    size: Vec3,
    tool_origin: Vec3,
    tool_size: Vec3,
) -> Result<(NamedFaces, NamedFaces), Box<dyn std::error::Error>> {
    let k = OcctKernel::new()?;
    let a = k.make_box(Vec3::ZERO, size)?;
    let b = k.make_box(tool_origin, tool_size)?;
    let (named_a, named_b) = (name_box(&k, F1, a)?, name_box(&k, F2, b)?);
    let result = k.boolean(BoolOp::Subtract, a, b)?;
    let named = name_result(&k, F3, &result, &[(a, &named_a), (b, &named_b)])?;
    Ok((named_a, named))
}

/// A pocket in the top face: the tool is 20 x 10, 5 deep, and sticks out of the top.
fn pocket(size: Vec3) -> Named {
    Ok(cut(size, v(10.0, 5.0, 5.0), v(20.0, 10.0, size.z))?.1)
}

/// A slot right through the box, 10 wide, from x = 15 to x = 25.
fn slot(size: Vec3) -> Result<(NamedFaces, NamedFaces), Box<dyn std::error::Error>> {
    cut(
        size,
        v(15.0, -5.0, -5.0),
        v(10.0, size.y + 10.0, size.z + 10.0),
    )
}

fn id_strings(named: &NamedFaces) -> BTreeSet<String> {
    named.ids.iter().map(ToString::to_string).collect()
}

fn set(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(ToString::to_string).collect()
}

/// The face with the given printed ID.
fn face<'a>(
    named: &'a NamedFaces,
    id: &str,
) -> Result<(cad_kernel::FaceIndex, &'a cad_doc::naming::FaceGeometry), String> {
    let index = named
        .ids
        .iter()
        .position(|i| i.to_string() == id)
        .ok_or(format!("no face {id}"))?;
    let face = cad_kernel::FaceIndex(u32::try_from(index).map_err(|e| e.to_string())?);
    Ok((face, &named.geometry[index]))
}

fn close(a: Vec3, b: Vec3) -> bool {
    Tolerance::default().eq_point(a, b)
}

#[test]
fn ids_come_from_the_creating_feature_and_role() -> TestResult {
    if occt().is_none() {
        return Ok(());
    }
    let named = pocket(v(40.0, 20.0, 10.0))?;
    // All six faces of the box, and the tool's four walls and floor. The tool's top is gone.
    let expected = set(&[
        "F1/box:-x",
        "F1/box:+x",
        "F1/box:-y",
        "F1/box:+y",
        "F1/box:-z",
        "F1/box:+z",
        "F2/box:-x",
        "F2/box:+x",
        "F2/box:-y",
        "F2/box:+y",
        "F2/box:-z",
    ]);
    assert_eq!(id_strings(&named), expected);
    assert_eq!(named.ids.len(), 11, "IDs must be unique");
    assert!(named.merged.is_empty());

    // The pocket floor is the tool's bottom face. In the result it faces up.
    let (_, floor) = face(&named, "F2/box:-z")?;
    assert!(close(floor.centroid, v(20.0, 10.0, 5.0)), "{floor:?}");
    assert!(close(floor.normal, Vec3::Z), "{floor:?}");
    Ok(())
}

#[test]
fn references_survive_changes_to_the_box_size() -> TestResult {
    if occt().is_none() {
        return Ok(());
    }
    let tol = Tolerance::default();
    let before = pocket(v(40.0, 20.0, 10.0))?;
    let references: Vec<_> = (0u32..)
        .zip(&before.ids)
        .filter_map(|(i, _)| before.reference(cad_kernel::FaceIndex(i)))
        .collect();
    assert_eq!(references.len(), 11);

    for size in [
        v(55.0, 26.0, 10.0),
        v(40.0, 20.0, 14.0),
        v(100.0, 20.0, 10.0),
        v(41.0, 21.0, 11.0),
    ] {
        let after = pocket(size)?;
        assert_eq!(id_strings(&after), id_strings(&before), "size {size:?}");
        for reference in &references {
            let Resolution::Exact(found) = resolve(reference, &after) else {
                panic!("{} did not resolve exactly at size {size:?}", reference.id);
            };
            // It is the same face in the sense that matters: same ID, facing the same way.
            assert_eq!(after.ids[found.0 as usize], reference.id);
            let geometry = &after.geometry[found.0 as usize];
            assert!(
                close(geometry.normal, reference.normal),
                "{} turned around",
                reference.id
            );
        }

        // Spot checks that the IDs landed on the faces a person would mean.
        let (_, far_side) = face(&after, "F1/box:+x")?;
        assert!(tol.eq_len(far_side.centroid.x, size.x), "{far_side:?}");
        let (_, top) = face(&after, "F1/box:+z")?;
        assert!(tol.eq_len(top.centroid.z, size.z), "{top:?}");
        assert!(
            tol.eq_len(top.area, size.x * size.y - 200.0),
            "top area {}",
            top.area
        );
        let (_, floor) = face(&after, "F2/box:-z")?;
        assert!(close(floor.centroid, v(20.0, 10.0, 5.0)), "{floor:?}");
        assert!(tol.eq_len(floor.area, 200.0));
    }
    Ok(())
}

#[test]
fn a_split_face_gets_ordered_children_and_its_parent_reference_is_not_rebound() -> TestResult {
    if occt().is_none() {
        return Ok(());
    }
    let tol = Tolerance::default();
    let (whole_box, named) = slot(v(40.0, 20.0, 10.0))?;
    let expected = set(&[
        "F1/box:-x",
        "F1/box:+x",
        "F1/box:-y/split@F3#0",
        "F1/box:-y/split@F3#1",
        "F1/box:+y/split@F3#0",
        "F1/box:+y/split@F3#1",
        "F1/box:-z/split@F3#0",
        "F1/box:-z/split@F3#1",
        "F1/box:+z/split@F3#0",
        "F1/box:+z/split@F3#1",
        "F2/box:-x",
        "F2/box:+x",
    ]);
    assert_eq!(id_strings(&named), expected);

    // Children are numbered along X: #0 is the piece from 0 to 15, #1 the piece from 25 to 40.
    let (left_face, left) = face(&named, "F1/box:+z/split@F3#0")?;
    let (right_face, right) = face(&named, "F1/box:+z/split@F3#1")?;
    assert!(tol.eq_len(left.centroid.x, 7.5), "{left:?}");
    assert!(tol.eq_len(right.centroid.x, 32.5), "{right:?}");

    // A reference made to the top face before the cut existed.
    let (top_face, _) = face(&whole_box, "F1/box:+z")?;
    let reference = whole_box.reference(top_face).ok_or("no reference")?;
    match resolve(&reference, &named) {
        Resolution::Ambiguous(candidates) => {
            let offered: BTreeSet<_> = candidates.iter().map(|c| c.face.0).collect();
            assert_eq!(offered, BTreeSet::from([left_face.0, right_face.0]));
            assert!(candidates.iter().all(|c| c.id.is_piece_of(&reference.id)));
        }
        other => panic!("a reference to a split face must report candidates, got {other:?}"),
    }
    Ok(())
}

#[test]
fn child_ids_are_the_same_on_every_rebuild_and_follow_the_geometry() -> TestResult {
    if occt().is_none() {
        return Ok(());
    }
    let tol = Tolerance::default();
    let where_is = |named: &NamedFaces| -> BTreeMap<String, [i64; 3]> {
        let rounded = |x: f64| (x * 1e6).round() as i64;
        named
            .ids
            .iter()
            .zip(&named.geometry)
            .map(|(id, g)| {
                (
                    id.to_string(),
                    [g.centroid.x, g.centroid.y, g.centroid.z].map(rounded),
                )
            })
            .collect()
    };
    let first = slot(v(40.0, 20.0, 10.0))?.1;
    for _ in 0..3 {
        assert_eq!(where_is(&slot(v(40.0, 20.0, 10.0))?.1), where_is(&first));
    }

    // Lengthen the box. References to the pieces still resolve, and #1 is still the far piece.
    let longer = slot(v(70.0, 20.0, 10.0))?.1;
    for (index, id) in (0u32..).zip(&first.ids) {
        let reference = first
            .reference(cad_kernel::FaceIndex(index))
            .ok_or("no reference")?;
        assert!(
            matches!(resolve(&reference, &longer), Resolution::Exact(_)),
            "{id} was lost"
        );
    }
    let (_, right) = face(&longer, "F1/box:+z/split@F3#1")?;
    assert!(tol.eq_len(right.centroid.x, 47.5), "{right:?}");
    Ok(())
}

#[test]
fn a_reference_to_a_piece_is_flagged_when_the_split_stops_happening() -> TestResult {
    if occt().is_none() {
        return Ok(());
    }
    let split = slot(v(40.0, 20.0, 10.0))?.1;
    let (piece, _) = face(&split, "F1/box:+z/split@F3#1")?;
    let reference = split.reference(piece).ok_or("no reference")?;

    // The same features, but the tool no longer reaches the edges: a pocket, not a slot.
    let unsplit = cut(v(40.0, 20.0, 10.0), v(15.0, 5.0, 5.0), v(10.0, 10.0, 10.0))?.1;
    let (top, _) = face(&unsplit, "F1/box:+z")?;
    assert_eq!(
        resolve(&reference, &unsplit),
        Resolution::Rebound {
            face: top,
            reason: ReboundReason::SplitUndone
        }
    );
    Ok(())
}

#[test]
fn a_deleted_face_is_not_rebound_to_a_lookalike_elsewhere() -> TestResult {
    if occt().is_none() {
        return Ok(());
    }
    let before = pocket(v(40.0, 20.0, 10.0))?;
    // The pocket wall at x = 30. In the result it faces -X, into the pocket.
    let (wall, geometry) = face(&before, "F2/box:+x")?;
    assert!(close(geometry.normal, v(-1.0, 0.0, 0.0)), "{geometry:?}");
    let reference = before.reference(wall).ok_or("no reference")?;

    // Widen the tool until it breaks out through the +X side. That wall no longer exists.
    let after = cut(v(40.0, 20.0, 10.0), v(10.0, 5.0, 5.0), v(35.0, 10.0, 10.0))?.1;
    assert!(!id_strings(&after).contains("F2/box:+x"));

    // The only other face pointing -X is the far end of the box, 30 mm away. It is offered
    // as a candidate. It must not be chosen.
    match resolve(&reference, &after) {
        Resolution::Ambiguous(candidates) => {
            let offered: Vec<String> = candidates.iter().map(|c| c.id.to_string()).collect();
            assert_eq!(offered, ["F1/box:-x"]);
        }
        other => panic!("expected candidates only, got {other:?}"),
    }
    Ok(())
}

#[test]
fn pieces_with_the_same_centre_are_ordered_by_area_through_two_operations() -> TestResult {
    let Some(k) = occt() else { return Ok(()) };
    let tol = Tolerance::default();
    let (f4, f5) = (FeatureId(4), FeatureId(5));
    let named_box =
        |feature, origin, size| -> Result<(ShapeId, NamedFaces), Box<dyn std::error::Error>> {
            let shape = k.make_box(origin, size)?;
            Ok((shape, name_box(&k, feature, shape)?))
        };
    let (block, block_names) = named_box(F1, Vec3::ZERO, v(40.0, 20.0, 10.0))?;
    let (outer, outer_names) = named_box(F2, v(10.0, 5.0, 8.0), v(20.0, 10.0, 5.0))?;
    let (inner, inner_names) = named_box(F3, v(14.0, 8.0, 7.0), v(12.0, 4.0, 7.0))?;

    // Feature 4: a rectangular frame. Feature 5: cut it into the top as a groove.
    let frame = k.boolean(BoolOp::Subtract, outer, inner)?;
    let frame_names = name_result(
        &k,
        f4,
        &frame,
        &[(outer, &outer_names), (inner, &inner_names)],
    )?;
    let grooved = k.boolean(BoolOp::Subtract, block, frame.shape)?;
    let named = name_result(
        &k,
        f5,
        &grooved,
        &[(block, &block_names), (frame.shape, &frame_names)],
    )?;

    // The top is now an island inside a ring. Both are centred on the same point, so position
    // cannot order them and area does: the island is smaller.
    let (_, island) = face(&named, "F1/box:+z/split@F5#0")?;
    let (_, ring) = face(&named, "F1/box:+z/split@F5#1")?;
    assert!(close(island.centroid, ring.centroid));
    assert!(tol.eq_len(island.area, 48.0), "island area {}", island.area);
    assert!(tol.eq_len(ring.area, 600.0), "ring area {}", ring.area);

    // The groove is lined by faces that keep the IDs of the two tools, through both cuts.
    let ids = id_strings(&named);
    for expected in [
        "F2/box:-x",
        "F2/box:+x",
        "F2/box:-y",
        "F2/box:+y",
        "F2/box:-z",
        "F3/box:-x",
        "F3/box:+x",
        "F3/box:-y",
        "F3/box:+y",
    ] {
        assert!(ids.contains(expected), "missing {expected} in {ids:?}");
    }
    assert_eq!(named.ids.len(), 16);
    assert_eq!(ids.len(), 16, "IDs must be unique");
    Ok(())
}

/// A block cut by one tool made of two slabs, the first at `first_slab_x`, the second at 25.
fn two_slab_cut(first_slab_x: f64) -> Named {
    let k = OcctKernel::new()?;
    let (f4, f5) = (FeatureId(4), FeatureId(5));
    let block = k.make_box(Vec3::ZERO, v(40.0, 20.0, 10.0))?;
    let slab_a = k.make_box(v(first_slab_x, -5.0, -5.0), v(2.0, 30.0, 20.0))?;
    let slab_b = k.make_box(v(25.0, -5.0, -5.0), v(2.0, 30.0, 20.0))?;
    let (block_names, a_names, b_names) = (
        name_box(&k, F1, block)?,
        name_box(&k, F2, slab_a)?,
        name_box(&k, F3, slab_b)?,
    );
    let tool = k.boolean(BoolOp::Union, slab_a, slab_b)?;
    let tool_names = name_result(&k, f4, &tool, &[(slab_a, &a_names), (slab_b, &b_names)])?;
    let result = k.boolean(BoolOp::Subtract, block, tool.shape)?;
    Ok(name_result(
        &k,
        f5,
        &result,
        &[(block, &block_names), (tool.shape, &tool_names)],
    )?)
}

/// KNOWN LIMITATION, asserted so that fixing it is a visible change.
///
/// Numbering split children by position is only stable while the number of pieces stays the
/// same. Here one operation cuts the top into three pieces. Moving the first slab out of the
/// block leaves two, the numbers shift down, and a reference to the middle piece resolves
/// "exactly" to a different region of the part. This is a silent rebind, which section 8.4
/// rule 5 forbids. See the T006 report for the proposed fix.
#[test]
fn known_limitation_child_numbers_shift_when_the_piece_count_changes() -> TestResult {
    if occt().is_none() {
        return Ok(());
    }
    let tol = Tolerance::default();
    let three = two_slab_cut(10.0)?;
    let (middle, geometry) = face(&three, "F1/box:+z/split@F5#1")?;
    assert!(
        tol.eq_len(geometry.centroid.x, 18.5),
        "middle piece spans x 12 to 25: {geometry:?}"
    );
    let reference = three.reference(middle).ok_or("no reference")?;

    let two = two_slab_cut(-20.0)?;
    let Resolution::Exact(found) = resolve(&reference, &two) else {
        panic!("the limitation is gone: update this test and the T006 report");
    };
    // The face it found spans x 27 to 40. None of it overlaps the piece that was referenced.
    let found = &two.geometry[found.0 as usize];
    assert!(tol.eq_len(found.centroid.x, 33.5), "{found:?}");
    Ok(())
}

/// The same edit made with two cut features in a row does not rebind silently, because each
/// split is recorded against its own feature.
#[test]
fn sequential_cuts_do_not_rebind_when_an_earlier_split_disappears() -> TestResult {
    if occt().is_none() {
        return Ok(());
    }
    let build = |first_slab_x: f64| -> Named {
        let k = OcctKernel::new()?;
        let (f4, f5) = (FeatureId(4), FeatureId(5));
        let block = k.make_box(Vec3::ZERO, v(40.0, 20.0, 10.0))?;
        let slab_a = k.make_box(v(first_slab_x, -5.0, -5.0), v(2.0, 30.0, 20.0))?;
        let slab_b = k.make_box(v(25.0, -5.0, -5.0), v(2.0, 30.0, 20.0))?;
        let (block_names, a_names, b_names) = (
            name_box(&k, F1, block)?,
            name_box(&k, F2, slab_a)?,
            name_box(&k, F3, slab_b)?,
        );
        let first = k.boolean(BoolOp::Subtract, block, slab_a)?;
        let first_names =
            name_result(&k, f4, &first, &[(block, &block_names), (slab_a, &a_names)])?;
        let second = k.boolean(BoolOp::Subtract, first.shape, slab_b)?;
        Ok(name_result(
            &k,
            f5,
            &second,
            &[(first.shape, &first_names), (slab_b, &b_names)],
        )?)
    };
    let three = build(10.0)?;
    let (middle, geometry) = face(&three, "F1/box:+z/split@F4#1/split@F5#0")?;
    assert!(
        Tolerance::default().eq_len(geometry.centroid.x, 18.5),
        "{geometry:?}"
    );
    let reference = three.reference(middle).ok_or("no reference")?;

    let two = build(-20.0)?;
    match resolve(&reference, &two) {
        Resolution::Ambiguous(candidates) => assert_eq!(candidates.len(), 2),
        other => panic!("expected the two top pieces as candidates, got {other:?}"),
    }
    Ok(())
}
