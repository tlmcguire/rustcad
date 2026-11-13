//! A `Kernel` test double that understands axis-aligned boxes and nothing else.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use cad_kernel::{
    BoolOp, FaceIndex, KResult, Kernel, KernelError, MassProps, Mesh, OpResult, ShapeId,
};
use cad_math::{Tolerance, Vec3};

/// Number of faces of a box.
const BOX_FACES: usize = 6;

/// Outward normals of the box faces, in face index order: -X, +X, -Y, +Y, -Z, +Z.
const FACE_NORMALS: [Vec3; BOX_FACES] = [
    Vec3::new(-1.0, 0.0, 0.0),
    Vec3::X,
    Vec3::new(0.0, -1.0, 0.0),
    Vec3::Y,
    Vec3::new(0.0, 0.0, -1.0),
    Vec3::Z,
];

/// Corners of each face as (x, y, z) selectors, where 0 picks the low corner and 1 the high
/// corner. Corners are listed counter-clockwise when seen from outside the box.
const FACE_CORNERS: [[[u8; 3]; 4]; BOX_FACES] = [
    [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]],
    [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]],
    [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
    [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]],
    [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]],
    [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
];

/// An axis-aligned box stored as its low corner and positive extents.
#[derive(Debug, Clone, Copy)]
struct BoxShape {
    origin: Vec3,
    size: Vec3,
}

impl BoxShape {
    fn corner(&self, pick: [u8; 3]) -> Vec3 {
        let along = |low: f64, extent: f64, high: u8| if high == 0 { low } else { low + extent };
        Vec3::new(
            along(self.origin.x, self.size.x, pick[0]),
            along(self.origin.y, self.size.y, pick[1]),
            along(self.origin.z, self.size.z, pick[2]),
        )
    }
}

#[derive(Debug, Default)]
struct State {
    next_id: u64,
    shapes: BTreeMap<u64, BoxShape>,
}

/// A tiny analytic kernel for tests: axis-aligned boxes only.
///
/// Results are exact closed-form values, so higher layers and the golden harness can run
/// against it before a real backend exists. Handles are issued in increasing order and are
/// never reused.
#[derive(Debug, Default)]
pub struct ReferenceKernel {
    state: Mutex<State>,
}

impl ReferenceKernel {
    /// Creates a kernel that owns no shapes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // The state is a plain map that is valid after any interrupted call,
        // so a poisoned lock is still safe to use.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn get(&self, shape: ShapeId) -> KResult<BoxShape> {
        self.state()
            .shapes
            .get(&shape.0)
            .copied()
            .ok_or(KernelError::UnknownShape(shape))
    }
}

impl Kernel for ReferenceKernel {
    fn name(&self) -> &'static str {
        "reference"
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
        let mut state = self.state();
        let id = state.next_id;
        state.next_id += 1;
        state.shapes.insert(id, BoxShape { origin, size });
        Ok(ShapeId(id))
    }

    fn boolean(&self, _op: BoolOp, _a: ShapeId, _b: ShapeId) -> KResult<OpResult> {
        Err(KernelError::Unsupported("boolean"))
    }

    fn face_count(&self, shape: ShapeId) -> KResult<usize> {
        self.get(shape).map(|_| BOX_FACES)
    }

    fn tessellate(&self, shape: ShapeId, _tol: Tolerance) -> KResult<Mesh> {
        let b = self.get(shape)?;
        let mut mesh = Mesh::default();
        for (face, (corners, normal)) in (0u32..).zip(FACE_CORNERS.iter().zip(FACE_NORMALS)) {
            let base = face * 4;
            for pick in corners {
                mesh.positions.push(b.corner(*pick));
                mesh.normals.push(normal);
            }
            mesh.triangles.push([base, base + 1, base + 2]);
            mesh.triangles.push([base, base + 2, base + 3]);
            mesh.triangle_faces.extend([FaceIndex(face); 2]);
        }
        Ok(mesh)
    }

    fn mass_props(&self, shape: ShapeId) -> KResult<MassProps> {
        let BoxShape { origin, size } = self.get(shape)?;
        Ok(MassProps {
            volume: size.x * size.y * size.z,
            area: 2.0 * (size.x * size.y + size.y * size.z + size.z * size.x),
            center: origin + size * 0.5,
        })
    }

    fn release(&self, shape: ShapeId) {
        self.state().shapes.remove(&shape.0);
    }
}
