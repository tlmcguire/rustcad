//! Kernel trait, shape handles, tessellation types.
//!
//! This crate defines the only interface the rest of the app uses to touch
//! B-rep geometry. Backends live in `cad-kernel-occt` and `cad-kernel-rs`.
//!
//! Phase 0 scope: just enough of the trait to run the kernel spike
//! (box, boolean, tessellate, mass properties). It grows per ROADMAP.md.

use cad_math::{Tolerance, Vec3};

/// Opaque handle to a shape owned by a kernel backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeId(pub u64);

/// Index of a face within one shape. Not stable across operations.
/// Stable naming is built on top of [`HistoryMap`] in `cad-doc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FaceIndex(pub u32);

/// Boolean operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolOp {
    /// A plus B.
    Union,
    /// A minus B.
    Subtract,
    /// The volume shared by A and B.
    Intersect,
}

/// What happened to an input face during an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceFate {
    /// The face exists in the output, possibly trimmed.
    Modified,
    /// The face was split into several output faces.
    Split,
    /// The face does not exist in the output.
    Deleted,
}

/// One entry of a history map: an input face and the output faces it became.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceHistory {
    /// Which input shape the face belonged to.
    pub source_shape: ShapeId,
    /// The face in the input shape.
    pub source_face: FaceIndex,
    /// What happened to it.
    pub fate: FaceFate,
    /// The corresponding faces in the output shape. Empty when deleted.
    pub result_faces: Vec<FaceIndex>,
}

/// Maps input topology to output topology for one operation.
/// Topological naming depends on every backend filling this in correctly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryMap {
    /// Input faces and what they became.
    pub faces: Vec<FaceHistory>,
    /// Output faces with no input ancestor.
    pub generated_faces: Vec<FaceIndex>,
}

/// Result of a modifying operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpResult {
    /// The new shape.
    pub shape: ShapeId,
    /// How its topology relates to the inputs.
    pub history: HistoryMap,
}

/// A triangle mesh for display and mesh export.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// Vertex positions.
    pub positions: Vec<Vec3>,
    /// Vertex normals, same length as `positions`.
    pub normals: Vec<Vec3>,
    /// Triangles as indices into `positions`.
    pub triangles: Vec<[u32; 3]>,
    /// The face each triangle came from, same length as `triangles`.
    pub triangle_faces: Vec<FaceIndex>,
}

/// Mass properties at unit density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassProps {
    /// Volume in cubic millimetres.
    pub volume: f64,
    /// Surface area in square millimetres.
    pub area: f64,
    /// Centre of mass.
    pub center: Vec3,
}

/// Why a kernel operation failed. Failures are data, not panics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelError {
    /// The arguments were invalid before any geometry was computed.
    InvalidInput(String),
    /// The handle does not refer to a live shape.
    UnknownShape(ShapeId),
    /// The geometric algorithm failed.
    OperationFailed(String),
    /// The backend crashed or timed out and was restarted.
    BackendLost(String),
    /// The backend does not implement this operation.
    Unsupported(&'static str),
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(m) => write!(f, "invalid input: {m}"),
            Self::UnknownShape(id) => write!(f, "unknown shape {}", id.0),
            Self::OperationFailed(m) => write!(f, "operation failed: {m}"),
            Self::BackendLost(m) => write!(f, "kernel backend lost: {m}"),
            Self::Unsupported(op) => write!(f, "operation not supported by this backend: {op}"),
        }
    }
}

impl std::error::Error for KernelError {}

/// Result alias for kernel calls.
pub type KResult<T> = Result<T, KernelError>;

/// A B-rep geometry backend.
pub trait Kernel: Send + Sync {
    /// Short backend name for logs and test reports.
    fn name(&self) -> &'static str;

    /// An axis-aligned box with one corner at `origin` and the given positive `size`.
    fn make_box(&self, origin: Vec3, size: Vec3) -> KResult<ShapeId>;

    /// Combines two solids.
    fn boolean(&self, op: BoolOp, a: ShapeId, b: ShapeId) -> KResult<OpResult>;

    /// Number of faces in a shape.
    fn face_count(&self, shape: ShapeId) -> KResult<usize>;

    /// Triangulates a shape for display.
    fn tessellate(&self, shape: ShapeId, tol: Tolerance) -> KResult<Mesh>;

    /// Volume, area, and centre of mass.
    fn mass_props(&self, shape: ShapeId) -> KResult<MassProps>;

    /// Releases a shape. Using the handle afterwards returns `UnknownShape`.
    fn release(&self, shape: ShapeId);
}
