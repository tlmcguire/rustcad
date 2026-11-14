use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use cad_kernel::{
    BoolOp, FaceFate, FaceHistory, FaceIndex, HistoryMap, KResult, Kernel, KernelError, MassProps,
    Mesh, OpResult, ShapeId,
};
use cad_math::{Tolerance, Vec3};

use crate::ffi;

/// Angular deflection for tessellation, in radians. This is OCCT's own default.
///
/// `Tolerance::angular` is a comparison tolerance, far too small to mesh with.
const ANGULAR_DEFLECTION: f64 = 0.5;

#[derive(Default)]
struct State {
    next_id: u64,
    shapes: BTreeMap<u64, ffi::Shape>,
}

/// The OpenCascade backend. Runs in-process and serialises all calls into OCCT.
pub struct OcctKernel {
    state: Mutex<State>,
}

impl std::fmt::Debug for OcctKernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcctKernel")
            .field("shapes", &self.state().shapes.len())
            .finish()
    }
}

impl OcctKernel {
    /// Creates a kernel that owns no shapes.
    ///
    /// # Errors
    ///
    /// Never fails when the crate was built with OCCT. The signature matches the build
    /// without it, where this returns [`KernelError::Unsupported`].
    pub fn new() -> KResult<Self> {
        Ok(Self {
            state: Mutex::new(State::default()),
        })
    }

    /// The OCCT version this backend was compiled against, for example `7.9.3`.
    #[must_use]
    pub fn occt_version() -> String {
        ffi::version()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // The map is valid after any interrupted call, so a poisoned lock is still usable.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn insert(state: &mut State, shape: ffi::Shape) -> ShapeId {
        let id = state.next_id;
        state.next_id += 1;
        state.shapes.insert(id, shape);
        ShapeId(id)
    }
}

fn get(state: &State, id: ShapeId) -> KResult<&ffi::Shape> {
    state.shapes.get(&id.0).ok_or(KernelError::UnknownShape(id))
}

fn triple(values: &[f64]) -> impl Iterator<Item = Vec3> + '_ {
    values
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| Vec3::new(c[0], c[1], c[2]))
}

impl Kernel for OcctKernel {
    fn name(&self) -> &'static str {
        "occt"
    }

    fn make_box(&self, origin: Vec3, size: Vec3) -> KResult<ShapeId> {
        let tol = Tolerance::default();
        let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
        if !finite(origin) || !finite(size) {
            return Err(KernelError::InvalidInput(
                "box origin and size must be finite".into(),
            ));
        }
        for (axis, extent) in [("x", size.x), ("y", size.y), ("z", size.z)] {
            if extent < 0.0 || tol.is_zero_len(extent) {
                return Err(KernelError::InvalidInput(format!(
                    "box size along {axis} must be positive, got {extent}"
                )));
            }
        }
        // Lock order everywhere: kernel state first, then OCCT.
        let mut state = self.state();
        let occt = ffi::lock();
        let shape = ffi::make_box(
            &occt,
            [origin.x, origin.y, origin.z],
            [size.x, size.y, size.z],
        )
        .map_err(KernelError::OperationFailed)?;
        Ok(Self::insert(&mut state, shape))
    }

    fn boolean(&self, op: BoolOp, a: ShapeId, b: ShapeId) -> KResult<OpResult> {
        let code = match op {
            BoolOp::Union => 0,
            BoolOp::Subtract => 1,
            BoolOp::Intersect => 2,
        };
        let mut state = self.state();
        let occt = ffi::lock();
        let (shape, entries, generated) =
            ffi::boolean(&occt, code, get(&state, a)?, get(&state, b)?)
                .map_err(KernelError::OperationFailed)?;

        let faces = entries
            .into_iter()
            .map(|(operand, face, fate, results)| FaceHistory {
                source_shape: if operand == 0 { a } else { b },
                source_face: FaceIndex(face),
                fate: match fate {
                    0 => FaceFate::Modified,
                    1 => FaceFate::Split,
                    _ => FaceFate::Deleted,
                },
                result_faces: results.into_iter().map(FaceIndex).collect(),
            })
            .collect();
        let history = HistoryMap {
            faces,
            generated_faces: generated.into_iter().map(FaceIndex).collect(),
        };
        Ok(OpResult {
            shape: Self::insert(&mut state, shape),
            history,
        })
    }

    fn face_count(&self, shape: ShapeId) -> KResult<usize> {
        let state = self.state();
        let occt = ffi::lock();
        Ok(ffi::face_count(&occt, get(&state, shape)?))
    }

    fn tessellate(&self, shape: ShapeId, tol: Tolerance) -> KResult<Mesh> {
        if !(tol.linear.is_finite() && tol.linear > 0.0) {
            return Err(KernelError::InvalidInput(
                "linear tolerance must be positive".into(),
            ));
        }
        let state = self.state();
        let occt = ffi::lock();
        let raw = ffi::tessellate(&occt, get(&state, shape)?, tol.linear, ANGULAR_DEFLECTION)
            .map_err(KernelError::OperationFailed)?;
        Ok(Mesh {
            positions: triple(&raw.positions).collect(),
            normals: triple(&raw.normals).collect(),
            triangles: raw.triangles.as_chunks::<3>().0.to_vec(),
            triangle_faces: raw.triangle_faces.into_iter().map(FaceIndex).collect(),
        })
    }

    fn mass_props(&self, shape: ShapeId) -> KResult<MassProps> {
        let state = self.state();
        let occt = ffi::lock();
        let [volume, area, x, y, z] =
            ffi::mass_props(&occt, get(&state, shape)?).map_err(KernelError::OperationFailed)?;
        Ok(MassProps {
            volume,
            area,
            center: Vec3::new(x, y, z),
        })
    }

    fn release(&self, shape: ShapeId) {
        let mut state = self.state();
        let _occt = ffi::lock();
        state.shapes.remove(&shape.0);
    }

    fn to_brep_bytes(&self, shape: ShapeId) -> KResult<Vec<u8>> {
        let state = self.state();
        let occt = ffi::lock();
        ffi::to_brep(&occt, get(&state, shape)?).map_err(KernelError::OperationFailed)
    }

    fn from_brep_bytes(&self, bytes: &[u8]) -> KResult<ShapeId> {
        if bytes.is_empty() {
            return Err(KernelError::InvalidInput("B-rep data is empty".into()));
        }
        let mut state = self.state();
        let occt = ffi::lock();
        let shape = ffi::from_brep(&occt, bytes).map_err(KernelError::OperationFailed)?;
        Ok(Self::insert(&mut state, shape))
    }

    fn export_step(&self, shape: ShapeId) -> KResult<Vec<u8>> {
        let state = self.state();
        let occt = ffi::lock();
        ffi::export_step(&occt, get(&state, shape)?).map_err(KernelError::OperationFailed)
    }

    fn import_step(&self, bytes: &[u8]) -> KResult<ShapeId> {
        if bytes.is_empty() {
            return Err(KernelError::InvalidInput("STEP data is empty".into()));
        }
        let mut state = self.state();
        let occt = ffi::lock();
        let shape = ffi::import_step(&occt, bytes).map_err(KernelError::OperationFailed)?;
        Ok(Self::insert(&mut state, shape))
    }
}

impl Drop for OcctKernel {
    fn drop(&mut self) {
        // Free the shapes while holding the OCCT lock, like every other call into OCCT.
        let mut state = self.state();
        let _occt = ffi::lock();
        state.shapes.clear();
    }
}
