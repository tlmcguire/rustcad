//! Safe wrappers over the C++ shim in `native/shim.cpp`.
//!
//! This is the only module in the workspace that contains `unsafe`. Everything it exports is
//! safe to call, given one rule the type system does not express: OCCT is not thread safe, so
//! every function here takes an [`OcctLock`] guard as proof that the process-wide lock is held.

#![allow(unsafe_code)]

use std::ffi::{CStr, c_char, c_int};
use std::ptr::{self, NonNull};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Raw face history entry: (operand 0 or 1, source face, fate code, result faces).
pub(crate) type RawEntry = (u32, u32, u32, Vec<u32>);

/// Triangle mesh as flat arrays.
pub(crate) struct RawMesh {
    pub(crate) positions: Vec<f64>,
    pub(crate) normals: Vec<f64>,
    pub(crate) triangles: Vec<u32>,
    pub(crate) triangle_faces: Vec<u32>,
}

#[repr(C)]
struct RawShape {
    _opaque: [u8; 0],
}
#[repr(C)]
struct RawHistory {
    _opaque: [u8; 0],
}
#[repr(C)]
struct RawMeshHandle {
    _opaque: [u8; 0],
}
#[repr(C)]
struct RawBytes {
    _opaque: [u8; 0],
}

// SAFETY: these declarations match the `extern "C"` definitions in native/shim.cpp one for
// one. The shim catches every C++ exception, so none of them unwinds into Rust.
unsafe extern "C" {
    fn occt_version() -> *const c_char;
    fn occt_claim_stdout() -> isize;
    fn occt_string_free(text: *mut c_char);
    fn occt_shape_free(shape: *mut RawShape);
    fn occt_history_free(history: *mut RawHistory);
    fn occt_mesh_free(mesh: *mut RawMeshHandle);
    fn occt_bytes_free(bytes: *mut RawBytes);
    fn occt_bytes_data(bytes: *const RawBytes, data: *mut *const u8, len: *mut usize);
    fn occt_to_brep(shape: *const RawShape, error: *mut *mut c_char) -> *mut RawBytes;
    fn occt_from_brep(data: *const u8, len: usize, error: *mut *mut c_char) -> *mut RawShape;
    fn occt_export_step(shape: *const RawShape, error: *mut *mut c_char) -> *mut RawBytes;
    fn occt_import_step(data: *const u8, len: usize, error: *mut *mut c_char) -> *mut RawShape;
    fn occt_make_box(
        ox: f64,
        oy: f64,
        oz: f64,
        sx: f64,
        sy: f64,
        sz: f64,
        error: *mut *mut c_char,
    ) -> *mut RawShape;
    fn occt_boolean(
        op: c_int,
        a: *const RawShape,
        b: *const RawShape,
        history: *mut *mut RawHistory,
        error: *mut *mut c_char,
    ) -> *mut RawShape;
    fn occt_history_len(history: *const RawHistory) -> usize;
    fn occt_history_entry(
        history: *const RawHistory,
        index: usize,
        source_shape: *mut u32,
        source_face: *mut u32,
        fate: *mut u32,
        results: *mut *const u32,
        result_count: *mut usize,
    ) -> bool;
    fn occt_history_generated(
        history: *const RawHistory,
        faces: *mut *const u32,
        count: *mut usize,
    );
    fn occt_face_count(shape: *const RawShape) -> usize;
    fn occt_mass_props(shape: *const RawShape, out: *mut f64, error: *mut *mut c_char) -> bool;
    fn occt_tessellate(
        shape: *const RawShape,
        linear_deflection: f64,
        angular_deflection: f64,
        error: *mut *mut c_char,
    ) -> *mut RawMeshHandle;
    fn occt_mesh_data(
        mesh: *const RawMeshHandle,
        positions: *mut *const f64,
        normals: *mut *const f64,
        vertex_count: *mut usize,
        triangles: *mut *const u32,
        triangle_faces: *mut *const u32,
        triangle_count: *mut usize,
    );
}

static OCCT: Mutex<()> = Mutex::new(());

/// Proof that the process-wide OCCT lock is held.
pub(crate) struct OcctLock(#[allow(dead_code)] MutexGuard<'static, ()>);

/// Takes the process-wide OCCT lock.
pub(crate) fn lock() -> OcctLock {
    // The lock guards no Rust data, so a panic while it was held leaves nothing to repair.
    OcctLock(OCCT.lock().unwrap_or_else(PoisonError::into_inner))
}

/// A shape owned by OCCT. Freed on drop.
pub(crate) struct Shape(NonNull<RawShape>);

// SAFETY: a `Shape` is an owning pointer to a heap object that no other Rust value refers to.
// OCCT reference counts are atomic, and every use of the pointer other than freeing it
// requires an `OcctLock`, so moving it between threads or sharing references is sound.
unsafe impl Send for Shape {}
// SAFETY: as above; `&Shape` only allows calls that require the lock.
unsafe impl Sync for Shape {}

impl Drop for Shape {
    fn drop(&mut self) {
        // SAFETY: the pointer came from the shim, is non-null, and is freed exactly once here.
        unsafe { occt_shape_free(self.0.as_ptr()) }
    }
}

/// Takes ownership of a shim error message, if there is one.
fn take_error(error: *mut c_char) -> String {
    if error.is_null() {
        return "OCCT reported a failure without a message".into();
    }
    // SAFETY: a non-null error is a NUL-terminated string allocated by the shim, which we own
    // and free exactly once after copying it.
    unsafe {
        let message = CStr::from_ptr(error).to_string_lossy().into_owned();
        occt_string_free(error);
        message
    }
}

/// Copies `count` items from a shim array. A null or empty array gives an empty vector.
///
/// # Safety
///
/// When `data` is non-null it must point to at least `count` initialised values of `T` that
/// stay valid for the duration of the call.
unsafe fn copy<T: Copy>(data: *const T, count: usize) -> Vec<T> {
    if data.is_null() || count == 0 {
        return Vec::new();
    }
    // SAFETY: guaranteed by the caller.
    unsafe { std::slice::from_raw_parts(data, count).to_vec() }
}

/// The OCCT version the shim was compiled against.
pub(crate) fn version() -> String {
    // SAFETY: the shim returns a pointer to a static NUL-terminated string literal.
    unsafe {
        CStr::from_ptr(occt_version())
            .to_string_lossy()
            .into_owned()
    }
}

pub(crate) fn make_box(_: &OcctLock, origin: [f64; 3], size: [f64; 3]) -> Result<Shape, String> {
    let mut error = ptr::null_mut();
    // SAFETY: plain values and a valid out-pointer for the error message.
    let shape = unsafe {
        occt_make_box(
            origin[0], origin[1], origin[2], size[0], size[1], size[2], &mut error,
        )
    };
    NonNull::new(shape)
        .map(Shape)
        .ok_or_else(|| take_error(error))
}

pub(crate) fn boolean(
    _: &OcctLock,
    op: i32,
    a: &Shape,
    b: &Shape,
) -> Result<(Shape, Vec<RawEntry>, Vec<u32>), String> {
    let mut error = ptr::null_mut();
    let mut history = ptr::null_mut();
    // SAFETY: both shapes are live for the call, and the out-pointers are valid.
    let shape = unsafe { occt_boolean(op, a.0.as_ptr(), b.0.as_ptr(), &mut history, &mut error) };
    let Some(shape) = NonNull::new(shape).map(Shape) else {
        return Err(take_error(error));
    };

    let mut entries = Vec::new();
    // SAFETY: on success `history` is a live history object that we own. Every array it hands
    // out stays valid until `occt_history_free`, which runs once, after the last read.
    let generated = unsafe {
        for index in 0..occt_history_len(history) {
            let (mut source_shape, mut source_face, mut fate) = (0u32, 0u32, 0u32);
            let (mut results, mut count) = (ptr::null(), 0usize);
            let found = occt_history_entry(
                history,
                index,
                &mut source_shape,
                &mut source_face,
                &mut fate,
                &mut results,
                &mut count,
            );
            if found {
                entries.push((source_shape, source_face, fate, copy(results, count)));
            }
        }
        let (mut faces, mut count) = (ptr::null(), 0usize);
        occt_history_generated(history, &mut faces, &mut count);
        let generated = copy(faces, count);
        occt_history_free(history);
        generated
    };
    Ok((shape, entries, generated))
}

pub(crate) fn face_count(_: &OcctLock, shape: &Shape) -> usize {
    // SAFETY: the shape is live for the call.
    unsafe { occt_face_count(shape.0.as_ptr()) }
}

/// Volume, area, and the centre of mass.
pub(crate) fn mass_props(_: &OcctLock, shape: &Shape) -> Result<[f64; 5], String> {
    let mut error = ptr::null_mut();
    let mut out = [0.0_f64; 5];
    // SAFETY: the shape is live, and `out` has room for the five values the shim writes.
    let ok = unsafe { occt_mass_props(shape.0.as_ptr(), out.as_mut_ptr(), &mut error) };
    if ok { Ok(out) } else { Err(take_error(error)) }
}

pub(crate) fn tessellate(
    _: &OcctLock,
    shape: &Shape,
    linear_deflection: f64,
    angular_deflection: f64,
) -> Result<RawMesh, String> {
    let mut error = ptr::null_mut();
    // SAFETY: the shape is live for the call, and the error out-pointer is valid.
    let mesh = unsafe {
        occt_tessellate(
            shape.0.as_ptr(),
            linear_deflection,
            angular_deflection,
            &mut error,
        )
    };
    if mesh.is_null() {
        return Err(take_error(error));
    }
    let (mut positions, mut normals, mut vertex_count) = (ptr::null(), ptr::null(), 0usize);
    let (mut triangles, mut triangle_faces, mut triangle_count) = (ptr::null(), ptr::null(), 0);
    // SAFETY: `mesh` is a live mesh that we own. The shim reports arrays of three values per
    // vertex and per triangle, and one face per triangle. They stay valid until
    // `occt_mesh_free`, which runs once, after they have been copied.
    unsafe {
        occt_mesh_data(
            mesh,
            &mut positions,
            &mut normals,
            &mut vertex_count,
            &mut triangles,
            &mut triangle_faces,
            &mut triangle_count,
        );
        let raw = RawMesh {
            positions: copy(positions, vertex_count * 3),
            normals: copy(normals, vertex_count * 3),
            triangles: copy(triangles, triangle_count * 3),
            triangle_faces: copy(triangle_faces, triangle_count),
        };
        occt_mesh_free(mesh);
        Ok(raw)
    }
}

/// Copies and frees a byte buffer returned by the shim, or takes its error.
fn take_bytes(bytes: *mut RawBytes, error: *mut c_char) -> Result<Vec<u8>, String> {
    if bytes.is_null() {
        return Err(take_error(error));
    }
    let (mut data, mut len) = (ptr::null(), 0usize);
    // SAFETY: `bytes` is a live buffer that we own. Its data stays valid until
    // `occt_bytes_free`, which runs once, after the copy.
    unsafe {
        occt_bytes_data(bytes, &mut data, &mut len);
        let copied = copy(data, len);
        occt_bytes_free(bytes);
        Ok(copied)
    }
}

pub(crate) fn to_brep(_: &OcctLock, shape: &Shape) -> Result<Vec<u8>, String> {
    let mut error = ptr::null_mut();
    // SAFETY: the shape is live for the call, and the error out-pointer is valid.
    let bytes = unsafe { occt_to_brep(shape.0.as_ptr(), &mut error) };
    take_bytes(bytes, error)
}

pub(crate) fn export_step(_: &OcctLock, shape: &Shape) -> Result<Vec<u8>, String> {
    let mut error = ptr::null_mut();
    // SAFETY: the shape is live for the call, and the error out-pointer is valid.
    let bytes = unsafe { occt_export_step(shape.0.as_ptr(), &mut error) };
    take_bytes(bytes, error)
}

pub(crate) fn from_brep(_: &OcctLock, data: &[u8]) -> Result<Shape, String> {
    let mut error = ptr::null_mut();
    // SAFETY: `data` is a live slice of `data.len()` bytes that the shim only reads, and the
    // error out-pointer is valid.
    let shape = unsafe { occt_from_brep(data.as_ptr(), data.len(), &mut error) };
    NonNull::new(shape)
        .map(Shape)
        .ok_or_else(|| take_error(error))
}

pub(crate) fn import_step(_: &OcctLock, data: &[u8]) -> Result<Shape, String> {
    let mut error = ptr::null_mut();
    // SAFETY: as for `from_brep`.
    let shape = unsafe { occt_import_step(data.as_ptr(), data.len(), &mut error) };
    NonNull::new(shape)
        .map(Shape)
        .ok_or_else(|| take_error(error))
}

/// Moves standard output to standard error and returns the original standard output.
///
/// Only the worker process calls this, once, before any other OCCT call.
pub(crate) fn claim_stdout() -> std::io::Result<std::fs::File> {
    // SAFETY: the shim only duplicates and redirects descriptors; it touches no Rust state.
    let raw = unsafe { occt_claim_stdout() };
    if raw < 0 {
        return Err(std::io::Error::other(
            "cannot move standard output out of OCCT's reach",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        let fd = i32::try_from(raw).map_err(std::io::Error::other)?;
        // SAFETY: `fd` is a fresh duplicate made by the shim, open and owned by nobody else;
        // the `File` becomes its only owner.
        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{FromRawHandle, RawHandle};
        // SAFETY: `raw` is the OS handle of a fresh duplicate made by the shim, open and owned
        // by nobody else; the `File` becomes its only owner.
        Ok(unsafe { std::fs::File::from_raw_handle(raw as RawHandle) })
    }
}
