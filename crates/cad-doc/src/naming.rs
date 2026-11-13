//! Stable face IDs: the Phase 0 spike of `docs/PLAN.md`, section 8.4.
//!
//! A kernel's [`FaceIndex`] means nothing after the next operation or the next rebuild. A
//! [`StableFaceId`] is derived from what made the face, so the same modelling steps give the
//! same IDs whatever order the kernel lists faces in:
//!
//! * a face of a primitive is named by the feature and its role in it: `F1/box:+z`;
//! * a face that survives an operation, trimmed or not, keeps its ID;
//! * a face split by feature `F3` gives children `F1/box:+z/split@F3#0`, `#1`, ordered by a
//!   geometric key, never by kernel order;
//! * a face with no ancestor is named by the operation: `F3/new#0`.
//!
//! A [`FaceRef`] stores an ID and a geometric description. [`resolve`] finds the face again
//! after a rebuild and never rebinds silently: anything other than an exact ID match is
//! reported as such.

use std::cmp::Ordering;
use std::fmt;

use cad_kernel::{FaceFate, FaceIndex, HistoryMap, Kernel, KernelError, Mesh, OpResult, ShapeId};
use cad_math::{Tolerance, Vec3};

/// Permanent identifier of a feature in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FeatureId(pub u64);

/// A side of an axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoxSide {
    /// The face whose outward normal is -X.
    XMin,
    /// The face whose outward normal is +X.
    XMax,
    /// The face whose outward normal is -Y.
    YMin,
    /// The face whose outward normal is +Y.
    YMax,
    /// The face whose outward normal is -Z.
    ZMin,
    /// The face whose outward normal is +Z.
    ZMax,
}

impl BoxSide {
    const ALL: [(Self, Vec3, &'static str); 6] = [
        (Self::XMin, Vec3::new(-1.0, 0.0, 0.0), "-x"),
        (Self::XMax, Vec3::X, "+x"),
        (Self::YMin, Vec3::new(0.0, -1.0, 0.0), "-y"),
        (Self::YMax, Vec3::Y, "+y"),
        (Self::ZMin, Vec3::new(0.0, 0.0, -1.0), "-z"),
        (Self::ZMax, Vec3::Z, "+z"),
    ];
}

/// What a face is to the feature that created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaceRole {
    /// A side of a box primitive.
    Box(BoxSide),
    /// A face an operation created with no input ancestor, numbered by geometric key.
    New(u32),
}

/// One split in a face's ancestry: which feature split it and which child this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Split {
    /// The feature whose operation split the face.
    pub by: FeatureId,
    /// Position among the children, ordered by geometric key.
    pub child: u32,
}

/// An identifier for a face that survives rebuilds and kernel reordering.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StableFaceId {
    /// The feature that created the original face.
    pub feature: FeatureId,
    /// The original face's role in that feature.
    pub role: FaceRole,
    /// Splits since then, oldest first. Empty for a face that was never split.
    pub splits: Vec<Split>,
}

impl StableFaceId {
    fn child(&self, by: FeatureId, child: u32) -> Self {
        let mut splits = self.splits.clone();
        splits.push(Split { by, child });
        Self {
            feature: self.feature,
            role: self.role,
            splits,
        }
    }

    /// True when `self` is a piece of `other`: same origin, and `other`'s splits are a strict
    /// prefix of this face's splits.
    #[must_use]
    pub fn is_piece_of(&self, other: &Self) -> bool {
        self.feature == other.feature
            && self.role == other.role
            && self.splits.len() > other.splits.len()
            && self.splits.starts_with(&other.splits)
    }
}

impl fmt::Display for StableFaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "F{}/", self.feature.0)?;
        match self.role {
            FaceRole::Box(side) => {
                let label = BoxSide::ALL
                    .iter()
                    .find(|s| s.0 == side)
                    .map_or("?", |s| s.2);
                write!(f, "box:{label}")?;
            }
            FaceRole::New(n) => write!(f, "new#{n}")?,
        }
        for split in &self.splits {
            write!(f, "/split@F{}#{}", split.by.0, split.child)?;
        }
        Ok(())
    }
}

/// Measured properties of one face, taken from a tessellation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceGeometry {
    /// Area-weighted centre of the face.
    pub centroid: Vec3,
    /// Unit outward normal, averaged over the face.
    pub normal: Vec3,
    /// Area of the face.
    pub area: f64,
    /// True when every triangle of the face has the same normal.
    pub planar: bool,
}

/// The faces of one shape, by kernel index, with their stable IDs.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedFaces {
    /// Stable ID of each face. Index is the kernel's [`FaceIndex`].
    pub ids: Vec<StableFaceId>,
    /// Geometry of each face, same indexing.
    pub geometry: Vec<FaceGeometry>,
    /// IDs that merged into another face: (the ID that disappeared, the face that absorbed it).
    pub merged: Vec<(StableFaceId, FaceIndex)>,
}

impl NamedFaces {
    /// The face with this ID, if it exists.
    #[must_use]
    pub fn find(&self, id: &StableFaceId) -> Option<FaceIndex> {
        let index = self.ids.iter().position(|candidate| candidate == id)?;
        u32::try_from(index).ok().map(FaceIndex)
    }

    /// The geometry of a face.
    #[must_use]
    pub fn geometry_of(&self, face: FaceIndex) -> Option<&FaceGeometry> {
        self.geometry.get(face.0 as usize)
    }

    /// A reference to a face, capturing its ID and a description to fall back on.
    #[must_use]
    pub fn reference(&self, face: FaceIndex) -> Option<FaceRef> {
        let id = self.ids.get(face.0 as usize)?.clone();
        let geometry = self.geometry_of(face)?;
        Some(FaceRef {
            id,
            normal: geometry.normal,
            point: geometry.centroid,
        })
    }
}

/// Why faces could not be named.
#[derive(Debug, Clone, PartialEq)]
pub enum NamingError {
    /// The kernel failed while the shape was being measured.
    Kernel(KernelError),
    /// A face has no triangles or no area, so it has no geometry to name it by.
    DegenerateFace(FaceIndex),
    /// A primitive's face does not match any role of that primitive.
    UnrecognisedFace(FaceIndex),
    /// Two faces of a primitive claim the same role.
    DuplicateRole(StableFaceId),
    /// The history refers to a shape or face that was not supplied.
    UnknownSource {
        /// The input shape named by the history.
        shape: ShapeId,
        /// The face of that shape.
        face: FaceIndex,
    },
    /// The history refers to an output face that does not exist.
    UnknownResult(FaceIndex),
    /// Two faces that must be ordered have the same geometric key, so any order would depend
    /// on the kernel. The ID is the parent, or the operation for new faces.
    IndistinguishableFaces(String),
}

impl fmt::Display for NamingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kernel(e) => write!(f, "kernel error while naming faces: {e}"),
            Self::DegenerateFace(face) => write!(f, "face {} has no area", face.0),
            Self::UnrecognisedFace(face) => {
                write!(
                    f,
                    "face {} does not match any role of its primitive",
                    face.0
                )
            }
            Self::DuplicateRole(id) => write!(f, "two faces claim the role {id}"),
            Self::UnknownSource { shape, face } => {
                write!(
                    f,
                    "history names face {} of unknown shape {}",
                    face.0, shape.0
                )
            }
            Self::UnknownResult(face) => write!(f, "history names missing output face {}", face.0),
            Self::IndistinguishableFaces(what) => {
                write!(
                    f,
                    "faces of {what} cannot be told apart by position or area"
                )
            }
        }
    }
}

impl std::error::Error for NamingError {}

impl From<KernelError> for NamingError {
    fn from(e: KernelError) -> Self {
        Self::Kernel(e)
    }
}

/// Measures every face of a tessellated shape.
///
/// # Errors
///
/// Returns [`NamingError::DegenerateFace`] for a face with no area.
pub fn measure_faces(mesh: &Mesh, face_count: usize) -> Result<Vec<FaceGeometry>, NamingError> {
    let tol = Tolerance::default();
    let mut weighted_normal = vec![Vec3::ZERO; face_count];
    let mut weighted_centroid = vec![Vec3::ZERO; face_count];
    let mut area = vec![0.0_f64; face_count];
    let mut unit_normals: Vec<Vec<Vec3>> = vec![Vec::new(); face_count];

    for (triangle, face) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        let slot = face.0 as usize;
        let corners = triangle.map(|i| mesh.positions.get(i as usize).copied());
        let ([Some(a), Some(b), Some(c)], true) = (corners, slot < face_count) else {
            return Err(NamingError::DegenerateFace(*face));
        };
        let cross = (b - a).cross(c - a);
        let triangle_area = cross.length() * 0.5;
        weighted_normal[slot] = weighted_normal[slot] + cross * 0.5;
        weighted_centroid[slot] = weighted_centroid[slot] + (a + b + c) * (triangle_area / 3.0);
        area[slot] += triangle_area;
        unit_normals[slot].extend(cross.normalized());
    }

    (0..face_count)
        .map(|slot| {
            let face = FaceIndex(u32::try_from(slot).unwrap_or(u32::MAX));
            let normal = weighted_normal[slot]
                .normalized()
                .ok_or(NamingError::DegenerateFace(face))?;
            if tol.is_zero_len(area[slot]) {
                return Err(NamingError::DegenerateFace(face));
            }
            Ok(FaceGeometry {
                centroid: weighted_centroid[slot] * (1.0 / area[slot]),
                normal,
                area: area[slot],
                planar: unit_normals[slot]
                    .iter()
                    .all(|n| tol.eq_len(n.dot(normal), 1.0)),
            })
        })
        .collect()
}

fn measure(kernel: &dyn Kernel, shape: ShapeId) -> Result<Vec<FaceGeometry>, NamingError> {
    let mesh = kernel.tessellate(shape, Tolerance::default())?;
    measure_faces(&mesh, kernel.face_count(shape)?)
}

/// Orders faces by position, then area. `Equal` means they cannot be told apart.
fn geometric_order(a: &FaceGeometry, b: &FaceGeometry) -> Ordering {
    let tol = Tolerance::default();
    let keys = [
        (a.centroid.x, b.centroid.x),
        (a.centroid.y, b.centroid.y),
        (a.centroid.z, b.centroid.z),
        (a.area, b.area),
    ];
    keys.into_iter()
        .find(|(p, q)| !tol.eq_len(*p, *q))
        .map_or(Ordering::Equal, |(p, q)| p.total_cmp(&q))
}

/// Sorts faces by geometric key, failing if two of them tie.
fn sorted_by_geometry(
    mut faces: Vec<FaceIndex>,
    geometry: &[FaceGeometry],
    what: &dyn Fn() -> String,
) -> Result<Vec<FaceIndex>, NamingError> {
    let of = |face: &FaceIndex| geometry.get(face.0 as usize);
    for face in &faces {
        of(face).ok_or(NamingError::UnknownResult(*face))?;
    }
    let order = |a: &FaceIndex, b: &FaceIndex| match (of(a), of(b)) {
        (Some(a), Some(b)) => geometric_order(a, b),
        _ => Ordering::Equal,
    };
    faces.sort_by(order);
    if faces
        .windows(2)
        .any(|pair| order(&pair[0], &pair[1]) == Ordering::Equal)
    {
        return Err(NamingError::IndistinguishableFaces(what()));
    }
    Ok(faces)
}

/// Names the faces of an axis-aligned box created by `feature`, by the direction each faces.
///
/// # Errors
///
/// Returns [`NamingError::UnrecognisedFace`] when a face is not planar and axis-aligned, and
/// [`NamingError::DuplicateRole`] when two faces point the same way.
pub fn name_box(
    kernel: &dyn Kernel,
    feature: FeatureId,
    shape: ShapeId,
) -> Result<NamedFaces, NamingError> {
    let tol = Tolerance::default();
    let geometry = measure(kernel, shape)?;
    let mut ids: Vec<StableFaceId> = Vec::with_capacity(geometry.len());
    for (face, measured) in (0u32..).zip(&geometry) {
        let side = BoxSide::ALL
            .iter()
            .find(|side| measured.planar && tol.eq_len(measured.normal.dot(side.1), 1.0))
            .ok_or(NamingError::UnrecognisedFace(FaceIndex(face)))?;
        let id = StableFaceId {
            feature,
            role: FaceRole::Box(side.0),
            splits: Vec::new(),
        };
        if ids.contains(&id) {
            return Err(NamingError::DuplicateRole(id));
        }
        ids.push(id);
    }
    Ok(NamedFaces {
        ids,
        geometry,
        merged: Vec::new(),
    })
}

/// Names the faces of an operation's result from the names of its inputs.
///
/// `inputs` pairs each input shape of the operation with its named faces.
///
/// # Errors
///
/// Returns [`NamingError::UnknownSource`] or [`NamingError::UnknownResult`] when the history
/// does not match the shapes, and [`NamingError::IndistinguishableFaces`] when split children
/// or new faces cannot be ordered deterministically.
pub fn name_result(
    kernel: &dyn Kernel,
    feature: FeatureId,
    result: &OpResult,
    inputs: &[(ShapeId, &NamedFaces)],
) -> Result<NamedFaces, NamingError> {
    let geometry = measure(kernel, result.shape)?;
    propagate(feature, &result.history, inputs, geometry)
}

/// The kernel-free core of [`name_result`]: applies a history map to input names.
///
/// # Errors
///
/// As [`name_result`].
pub fn propagate(
    feature: FeatureId,
    history: &HistoryMap,
    inputs: &[(ShapeId, &NamedFaces)],
    geometry: Vec<FaceGeometry>,
) -> Result<NamedFaces, NamingError> {
    let mut ids: Vec<Option<StableFaceId>> = vec![None; geometry.len()];
    let mut merged = Vec::new();
    let mut assign = |face: FaceIndex, id: StableFaceId| -> Result<(), NamingError> {
        match ids
            .get_mut(face.0 as usize)
            .ok_or(NamingError::UnknownResult(face))?
        {
            // Two input faces became one output face. The first keeps the name.
            Some(_) => merged.push((id, face)),
            slot => *slot = Some(id),
        }
        Ok(())
    };

    for entry in &history.faces {
        let unknown = NamingError::UnknownSource {
            shape: entry.source_shape,
            face: entry.source_face,
        };
        let source = inputs
            .iter()
            .find(|(shape, _)| *shape == entry.source_shape)
            .and_then(|(_, named)| named.ids.get(entry.source_face.0 as usize))
            .ok_or(unknown)?;
        match (entry.fate, entry.result_faces.as_slice()) {
            (FaceFate::Deleted, _) | (_, []) => {}
            (FaceFate::Modified, [only]) => assign(*only, source.clone())?,
            (_, children) => {
                let ordered =
                    sorted_by_geometry(children.to_vec(), &geometry, &|| source.to_string())?;
                for (child, face) in (0u32..).zip(ordered) {
                    assign(face, source.child(feature, child))?;
                }
            }
        }
    }

    // Faces the history did not account for are new, whether or not the kernel said so.
    let unnamed: Vec<FaceIndex> = (0u32..)
        .zip(&ids)
        .filter(|(_, id)| id.is_none())
        .map(|(face, _)| FaceIndex(face))
        .collect();
    let what = || format!("F{}/new", feature.0);
    for (n, face) in (0u32..).zip(sorted_by_geometry(unnamed, &geometry, &what)?) {
        let id = StableFaceId {
            feature,
            role: FaceRole::New(n),
            splits: Vec::new(),
        };
        ids[face.0 as usize] = Some(id);
    }

    let ids = (0u32..)
        .zip(ids)
        .map(|(face, id)| id.ok_or(NamingError::UnknownResult(FaceIndex(face))))
        .collect::<Result<_, _>>()?;
    Ok(NamedFaces {
        ids,
        geometry,
        merged,
    })
}

/// A reference to a face: its stable ID, and a description to fall back on.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceRef {
    /// The face's stable ID when the reference was made.
    pub id: StableFaceId,
    /// The face's outward normal when the reference was made.
    pub normal: Vec3,
    /// A point on the face when the reference was made.
    pub point: Vec3,
}

/// A face offered as a possible meaning of a reference that no longer resolves exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// The face.
    pub face: FaceIndex,
    /// Its stable ID.
    pub id: StableFaceId,
    /// Distance from the reference's point to the face's centroid.
    pub distance: f64,
}

/// Why a reference was rebound to a face with a different ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReboundReason {
    /// The referenced face was a piece of a split face, and the split no longer happens.
    /// The whole face is back.
    SplitUndone,
    /// The referenced face was split, and exactly one piece is left.
    OnePieceLeft,
    /// The ID is gone, and exactly one face matches the description: same normal, and its
    /// plane passes through the described point.
    MatchesDescription,
}

/// The outcome of looking a reference up in a rebuilt shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// The ID exists. This is the only outcome that needs no attention.
    Exact(FaceIndex),
    /// The ID is gone and one face is a confident replacement. The caller must flag a warning.
    Rebound {
        /// The replacement face.
        face: FaceIndex,
        /// Why it was chosen.
        reason: ReboundReason,
    },
    /// The ID is gone and there is no single confident replacement. The feature that holds
    /// the reference has failed, and these are the faces to offer, nearest first.
    Ambiguous(Vec<Candidate>),
    /// The ID is gone and nothing resembles the face.
    Missing,
}

/// Looks a reference up in a shape's named faces. Never rebinds silently: every outcome other
/// than [`Resolution::Exact`] says what happened.
#[must_use]
pub fn resolve(reference: &FaceRef, named: &NamedFaces) -> Resolution {
    if let Some(face) = named.find(&reference.id) {
        return Resolution::Exact(face);
    }
    let candidate = |(face, id): (u32, &StableFaceId)| {
        let centroid = named
            .geometry
            .get(face as usize)
            .map_or(reference.point, |g| g.centroid);
        Candidate {
            face: FaceIndex(face),
            id: id.clone(),
            distance: (centroid - reference.point).length(),
        }
    };
    let nearest_first = |mut list: Vec<Candidate>| {
        list.sort_by(|a, b| a.distance.total_cmp(&b.distance).then(a.id.cmp(&b.id)));
        list
    };
    let all = || (0u32..).zip(&named.ids);

    // 1. Relatives by ancestry: pieces of the face, or the face it was a piece of.
    let pieces: Vec<Candidate> = all()
        .filter(|(_, id)| id.is_piece_of(&reference.id))
        .map(candidate)
        .collect();
    match pieces.as_slice() {
        [] => {}
        [only] => {
            return Resolution::Rebound {
                face: only.face,
                reason: ReboundReason::OnePieceLeft,
            };
        }
        _ => return Resolution::Ambiguous(nearest_first(pieces)),
    }
    let whole = all()
        .filter(|(_, id)| reference.id.is_piece_of(id))
        .max_by_key(|(_, id)| id.splits.len());
    if let Some((face, _)) = whole {
        return Resolution::Rebound {
            face: FaceIndex(face),
            reason: ReboundReason::SplitUndone,
        };
    }
    if let Some((_, face)) = named.merged.iter().find(|(id, _)| *id == reference.id) {
        return Resolution::Ambiguous(vec![candidate((face.0, &named.ids[face.0 as usize]))]);
    }

    // 2. The description: planar faces with the same normal.
    let tol = Tolerance::default();
    let alike: Vec<(u32, &StableFaceId, &FaceGeometry)> = all()
        .zip(&named.geometry)
        .map(|((face, id), geometry)| (face, id, geometry))
        .filter(|(_, _, g)| g.planar && tol.eq_len(g.normal.dot(reference.normal), 1.0))
        .collect();
    let on_plane = |g: &FaceGeometry| tol.is_zero_len(g.normal.dot(g.centroid - reference.point));
    match alike.as_slice() {
        [] => Resolution::Missing,
        [(face, _, geometry)] if on_plane(geometry) => Resolution::Rebound {
            face: FaceIndex(*face),
            reason: ReboundReason::MatchesDescription,
        },
        _ => Resolution::Ambiguous(nearest_first(
            alike
                .iter()
                .map(|(face, id, _)| candidate((*face, id)))
                .collect(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cad_kernel::FaceHistory;

    fn id(feature: u64, side: BoxSide) -> StableFaceId {
        StableFaceId {
            feature: FeatureId(feature),
            role: FaceRole::Box(side),
            splits: Vec::new(),
        }
    }

    fn flat(x: f64, z: f64) -> FaceGeometry {
        FaceGeometry {
            centroid: Vec3::new(x, 0.0, z),
            normal: Vec3::Z,
            area: 1.0,
            planar: true,
        }
    }

    fn named(ids: Vec<StableFaceId>, geometry: Vec<FaceGeometry>) -> NamedFaces {
        NamedFaces {
            ids,
            geometry,
            merged: Vec::new(),
        }
    }

    fn entry(shape: u64, face: u32, fate: FaceFate, results: &[u32]) -> FaceHistory {
        FaceHistory {
            source_shape: ShapeId(shape),
            source_face: FaceIndex(face),
            fate,
            result_faces: results.iter().map(|r| FaceIndex(*r)).collect(),
        }
    }

    #[test]
    fn ids_print_their_ancestry() {
        let top = id(1, BoxSide::ZMax);
        assert_eq!(top.to_string(), "F1/box:+z");
        let piece = top.child(FeatureId(3), 1);
        assert_eq!(piece.to_string(), "F1/box:+z/split@F3#1");
        assert!(piece.is_piece_of(&top));
        assert!(!top.is_piece_of(&piece));
        assert!(!top.is_piece_of(&top));
        assert!(!piece.is_piece_of(&id(2, BoxSide::ZMax)));
    }

    #[test]
    fn modified_faces_keep_their_id_whatever_index_they_land_on() -> Result<(), NamingError> {
        let input = named(
            vec![id(1, BoxSide::ZMax), id(1, BoxSide::ZMin)],
            vec![flat(0.0, 1.0); 2],
        );
        let history = HistoryMap {
            faces: vec![
                entry(7, 0, FaceFate::Modified, &[1]),
                entry(7, 1, FaceFate::Modified, &[0]),
            ],
            generated_faces: Vec::new(),
        };
        let out = propagate(
            FeatureId(2),
            &history,
            &[(ShapeId(7), &input)],
            vec![flat(0.0, 0.0), flat(0.0, 1.0)],
        )?;
        assert_eq!(out.ids, [id(1, BoxSide::ZMin), id(1, BoxSide::ZMax)]);
        Ok(())
    }

    #[test]
    fn split_children_are_numbered_by_position_not_kernel_order() -> Result<(), NamingError> {
        let input = named(vec![id(1, BoxSide::ZMax)], vec![flat(0.0, 1.0)]);
        let top = id(1, BoxSide::ZMax);
        for results in [[0, 1], [1, 0]] {
            let history = HistoryMap {
                faces: vec![entry(7, 0, FaceFate::Split, &results)],
                generated_faces: Vec::new(),
            };
            // Output face 0 is on the right, face 1 on the left.
            let geometry = vec![flat(30.0, 1.0), flat(5.0, 1.0)];
            let out = propagate(FeatureId(3), &history, &[(ShapeId(7), &input)], geometry)?;
            assert_eq!(
                out.ids,
                [top.child(FeatureId(3), 1), top.child(FeatureId(3), 0)]
            );
        }
        Ok(())
    }

    #[test]
    fn children_that_cannot_be_told_apart_are_an_error() {
        let input = named(vec![id(1, BoxSide::ZMax)], vec![flat(0.0, 1.0)]);
        let history = HistoryMap {
            faces: vec![entry(7, 0, FaceFate::Split, &[0, 1])],
            generated_faces: Vec::new(),
        };
        let got = propagate(
            FeatureId(3),
            &history,
            &[(ShapeId(7), &input)],
            vec![flat(5.0, 1.0), flat(5.0, 1.0)],
        );
        assert_eq!(
            got,
            Err(NamingError::IndistinguishableFaces("F1/box:+z".into()))
        );
    }

    #[test]
    fn deleted_faces_vanish_and_unaccounted_faces_are_new() -> Result<(), NamingError> {
        let input = named(
            vec![id(1, BoxSide::ZMax), id(1, BoxSide::ZMin)],
            vec![flat(0.0, 1.0); 2],
        );
        let history = HistoryMap {
            faces: vec![
                entry(7, 0, FaceFate::Deleted, &[]),
                entry(7, 1, FaceFate::Modified, &[2]),
            ],
            generated_faces: vec![FaceIndex(1)],
        };
        let geometry = vec![flat(9.0, 0.0), flat(2.0, 0.0), flat(0.0, 0.0)];
        let out = propagate(FeatureId(4), &history, &[(ShapeId(7), &input)], geometry)?;
        let new = |n| StableFaceId {
            feature: FeatureId(4),
            role: FaceRole::New(n),
            splits: vec![],
        };
        // Face 1 is left of face 0, so it is new#0. Face 0 was not in the history at all.
        assert_eq!(out.ids, [new(1), new(0), id(1, BoxSide::ZMin)]);
        assert_eq!(out.find(&id(1, BoxSide::ZMax)), None);
        Ok(())
    }

    #[test]
    fn two_inputs_merging_into_one_face_is_recorded() -> Result<(), NamingError> {
        let a = named(vec![id(1, BoxSide::ZMax)], vec![flat(0.0, 1.0)]);
        let b = named(vec![id(2, BoxSide::ZMax)], vec![flat(9.0, 1.0)]);
        let history = HistoryMap {
            faces: vec![
                entry(7, 0, FaceFate::Modified, &[0]),
                entry(8, 0, FaceFate::Modified, &[0]),
            ],
            generated_faces: Vec::new(),
        };
        let inputs = [(ShapeId(7), &a), (ShapeId(8), &b)];
        let out = propagate(FeatureId(3), &history, &inputs, vec![flat(4.0, 1.0)])?;
        assert_eq!(out.ids, [id(1, BoxSide::ZMax)]);
        assert_eq!(out.merged, [(id(2, BoxSide::ZMax), FaceIndex(0))]);

        // A reference to the absorbed face is offered the merged face, not handed it.
        let reference = FaceRef {
            id: id(2, BoxSide::ZMax),
            normal: Vec3::Z,
            point: Vec3::ZERO,
        };
        assert!(matches!(resolve(&reference, &out), Resolution::Ambiguous(c) if c.len() == 1));
        Ok(())
    }

    #[test]
    fn history_that_does_not_match_the_shapes_is_rejected() {
        let input = named(vec![id(1, BoxSide::ZMax)], vec![flat(0.0, 1.0)]);
        let run = |faces| {
            let history = HistoryMap {
                faces,
                generated_faces: Vec::new(),
            };
            propagate(
                FeatureId(2),
                &history,
                &[(ShapeId(7), &input)],
                vec![flat(0.0, 1.0)],
            )
        };
        let wrong_shape = run(vec![entry(9, 0, FaceFate::Modified, &[0])]);
        assert!(matches!(
            wrong_shape,
            Err(NamingError::UnknownSource { .. })
        ));
        let wrong_face = run(vec![entry(7, 5, FaceFate::Modified, &[0])]);
        assert!(matches!(wrong_face, Err(NamingError::UnknownSource { .. })));
        let wrong_result = run(vec![entry(7, 0, FaceFate::Modified, &[3])]);
        assert_eq!(wrong_result, Err(NamingError::UnknownResult(FaceIndex(3))));
    }

    #[test]
    fn resolution_prefers_the_id_and_reports_everything_else() {
        let top = id(1, BoxSide::ZMax);
        let reference = FaceRef {
            id: top.clone(),
            normal: Vec3::Z,
            point: Vec3::new(0.0, 0.0, 1.0),
        };

        // The ID exists: exact, even though another face matches the description better.
        let faces = named(
            vec![id(2, BoxSide::ZMax), top.clone()],
            vec![flat(0.0, 1.0), flat(50.0, 9.0)],
        );
        assert_eq!(resolve(&reference, &faces), Resolution::Exact(FaceIndex(1)));

        // Split in two: both pieces offered, nearest first, neither chosen.
        let pieces = vec![top.child(FeatureId(3), 0), top.child(FeatureId(3), 1)];
        let faces = named(pieces.clone(), vec![flat(-9.0, 1.0), flat(4.0, 1.0)]);
        match resolve(&reference, &faces) {
            Resolution::Ambiguous(c) => {
                assert_eq!(c.iter().map(|c| c.face.0).collect::<Vec<_>>(), [1, 0]);
            }
            other => panic!("expected candidates, got {other:?}"),
        }

        // One piece left: rebound, with the reason.
        let faces = named(vec![pieces[1].clone()], vec![flat(4.0, 1.0)]);
        let rebound = Resolution::Rebound {
            face: FaceIndex(0),
            reason: ReboundReason::OnePieceLeft,
        };
        assert_eq!(resolve(&reference, &faces), rebound);

        // A reference to a piece, after the split stopped happening: the whole face is back.
        let to_piece = FaceRef {
            id: pieces[0].clone(),
            ..reference.clone()
        };
        let faces = named(vec![top.clone()], vec![flat(0.0, 1.0)]);
        let rebound = Resolution::Rebound {
            face: FaceIndex(0),
            reason: ReboundReason::SplitUndone,
        };
        assert_eq!(resolve(&to_piece, &faces), rebound);
    }

    #[test]
    fn description_fallback_needs_one_face_in_the_same_plane() {
        let gone = FaceRef {
            id: id(9, BoxSide::ZMax),
            normal: Vec3::Z,
            point: Vec3::new(0.0, 0.0, 1.0),
        };
        let other = id(1, BoxSide::ZMax);
        let side = FaceGeometry {
            normal: Vec3::X,
            ..flat(0.0, 1.0)
        };

        // One face with the same normal in the same plane: rebound, flagged.
        let faces = named(
            vec![other.clone(), id(1, BoxSide::XMax)],
            vec![flat(3.0, 1.0), side],
        );
        let rebound = Resolution::Rebound {
            face: FaceIndex(0),
            reason: ReboundReason::MatchesDescription,
        };
        assert_eq!(resolve(&gone, &faces), rebound);

        // Same normal but a different plane: offered, not chosen.
        let faces = named(vec![other.clone()], vec![flat(3.0, 7.0)]);
        assert!(matches!(resolve(&gone, &faces), Resolution::Ambiguous(c) if c.len() == 1));

        // Two in the same plane: offered, not chosen.
        let faces = named(
            vec![other, id(2, BoxSide::ZMax)],
            vec![flat(3.0, 1.0), flat(8.0, 1.0)],
        );
        assert!(matches!(resolve(&gone, &faces), Resolution::Ambiguous(c) if c.len() == 2));

        // Nothing faces that way.
        let faces = named(vec![id(1, BoxSide::XMax)], vec![side]);
        assert_eq!(resolve(&gone, &faces), Resolution::Missing);
    }

    #[test]
    fn faces_are_measured_from_a_mesh() -> Result<(), NamingError> {
        // A 2 x 1 rectangle in the plane z = 3, as two triangles, facing +Z.
        let mesh = Mesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 3.0),
                Vec3::new(2.0, 0.0, 3.0),
                Vec3::new(2.0, 1.0, 3.0),
                Vec3::new(0.0, 1.0, 3.0),
            ],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            triangle_faces: vec![FaceIndex(0); 2],
        };
        let tol = Tolerance::default();
        let faces = measure_faces(&mesh, 1)?;
        assert!(tol.eq_len(faces[0].area, 2.0));
        assert!(tol.eq_point(faces[0].centroid, Vec3::new(1.0, 0.5, 3.0)));
        assert!(tol.eq_point(faces[0].normal, Vec3::Z));
        assert!(faces[0].planar);
        // A face the mesh has no triangles for cannot be named.
        assert_eq!(
            measure_faces(&mesh, 2),
            Err(NamingError::DegenerateFace(FaceIndex(1)))
        );
        Ok(())
    }
}
