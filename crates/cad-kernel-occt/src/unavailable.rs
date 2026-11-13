use cad_kernel::{BoolOp, KResult, Kernel, KernelError, MassProps, Mesh, OpResult, ShapeId};
use cad_math::{Tolerance, Vec3};

const MISSING: &str = "the OpenCascade backend (this build was made without OCCT)";

/// Stand-in for the OpenCascade backend in builds made without OCCT. Cannot be constructed.
#[derive(Debug)]
pub struct OcctKernel {
    never: std::convert::Infallible,
}

impl OcctKernel {
    /// Always fails: this build was made without OCCT.
    ///
    /// # Errors
    ///
    /// Returns [`KernelError::Unsupported`].
    pub fn new() -> KResult<Self> {
        Err(KernelError::Unsupported(MISSING))
    }

    /// The OCCT version this backend was compiled against. Empty in a build without OCCT.
    #[must_use]
    pub fn occt_version() -> String {
        String::new()
    }
}

impl Kernel for OcctKernel {
    fn name(&self) -> &'static str {
        match self.never {}
    }
    fn make_box(&self, _: Vec3, _: Vec3) -> KResult<ShapeId> {
        match self.never {}
    }
    fn boolean(&self, _: BoolOp, _: ShapeId, _: ShapeId) -> KResult<OpResult> {
        match self.never {}
    }
    fn face_count(&self, _: ShapeId) -> KResult<usize> {
        match self.never {}
    }
    fn tessellate(&self, _: ShapeId, _: Tolerance) -> KResult<Mesh> {
        match self.never {}
    }
    fn mass_props(&self, _: ShapeId) -> KResult<MassProps> {
        match self.never {}
    }
    fn release(&self, _: ShapeId) {
        match self.never {}
    }
}
