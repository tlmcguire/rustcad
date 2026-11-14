//! Fixed constructions and output digests for the determinism and performance gates.
//!
//! `bin/determinism.rs` prints one digest per case and kernel; `scripts/gates.sh` runs it
//! more than once and compares the output. `bin/perf.rs` times the same cases.

use cad_kernel::{BoolOp, KResult, Kernel, KernelError, ShapeId};
use cad_math::{Tolerance, Vec3};

/// A named construction that any kernel with boxes and booleans can build.
#[derive(Clone, Copy)]
pub struct Case {
    /// Stable name, printed by the gates.
    pub name: &'static str,
    /// Builds the shape.
    pub build: fn(&dyn Kernel) -> KResult<ShapeId>,
    /// True when the case needs booleans, which the reference kernel lacks.
    pub needs_boolean: bool,
}

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn base_box(k: &dyn Kernel) -> KResult<ShapeId> {
    k.make_box(Vec3::ZERO, v(40.0, 20.0, 10.0))
}

fn pocket(k: &dyn Kernel) -> KResult<ShapeId> {
    let a = base_box(k)?;
    let b = k.make_box(v(10.0, 5.0, 5.0), v(20.0, 10.0, 10.0))?;
    Ok(k.boolean(BoolOp::Subtract, a, b)?.shape)
}

fn slot(k: &dyn Kernel) -> KResult<ShapeId> {
    let a = base_box(k)?;
    let b = k.make_box(v(15.0, -5.0, -5.0), v(10.0, 30.0, 20.0))?;
    Ok(k.boolean(BoolOp::Subtract, a, b)?.shape)
}

fn union_chain(k: &dyn Kernel) -> KResult<ShapeId> {
    let mut acc = base_box(k)?;
    for i in 1..6 {
        let step = f64::from(i);
        let next = k.make_box(v(step * 5.0, step * 2.0, 0.0), v(10.0, 10.0, 10.0 + step))?;
        acc = k.boolean(BoolOp::Union, acc, next)?.shape;
    }
    Ok(acc)
}

/// Every case, in a fixed order.
#[must_use]
pub fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "box",
            build: base_box,
            needs_boolean: false,
        },
        Case {
            name: "pocket",
            build: pocket,
            needs_boolean: true,
        },
        Case {
            name: "slot",
            build: slot,
            needs_boolean: true,
        },
        Case {
            name: "union_chain",
            build: union_chain,
            needs_boolean: true,
        },
    ]
}

/// 64-bit FNV-1a, enough to tell outputs apart. Not for security.
#[derive(Debug, Clone, Copy)]
pub struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    /// Feeds bytes.
    pub fn bytes(&mut self, data: &[u8]) {
        for byte in data {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// Feeds a float by its exact bits.
    pub fn f64(&mut self, x: f64) {
        self.bytes(&x.to_bits().to_le_bytes());
    }

    /// Feeds an integer.
    pub fn u64(&mut self, x: u64) {
        self.bytes(&x.to_le_bytes());
    }

    /// The digest so far.
    #[must_use]
    pub fn finish(self) -> u64 {
        self.0
    }
}

/// A digest of everything a kernel reports about a shape: mass properties, face count, the
/// tessellation at the default tolerance, and the B-rep bytes when the kernel has them. Floats
/// are compared by their bits, so any difference at all changes the digest.
///
/// # Errors
///
/// Any kernel error other than `Unsupported` from `to_brep_bytes`.
pub fn digest(k: &dyn Kernel, shape: ShapeId) -> KResult<u64> {
    let mut h = Fnv::default();
    let props = k.mass_props(shape)?;
    for x in [
        props.volume,
        props.area,
        props.center.x,
        props.center.y,
        props.center.z,
    ] {
        h.f64(x);
    }
    h.u64(k.face_count(shape)? as u64);
    let mesh = k.tessellate(shape, Tolerance::default())?;
    for p in mesh.positions.iter().chain(&mesh.normals) {
        h.f64(p.x);
        h.f64(p.y);
        h.f64(p.z);
    }
    for t in &mesh.triangles {
        t.iter().for_each(|i| h.u64(u64::from(*i)));
    }
    mesh.triangle_faces
        .iter()
        .for_each(|f| h.u64(u64::from(f.0)));
    match k.to_brep_bytes(shape) {
        Ok(bytes) => h.bytes(&bytes),
        Err(KernelError::Unsupported(_)) => {}
        Err(e) => return Err(e),
    }
    Ok(h.finish())
}

/// Whether the OCCT tests and gates must run: `OCCT_REQUIRED` is set and not `0`.
#[must_use]
pub fn occt_required() -> bool {
    std::env::var_os("OCCT_REQUIRED").is_some_and(|v| !v.is_empty() && v != "0")
}
