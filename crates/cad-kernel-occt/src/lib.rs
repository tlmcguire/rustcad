//! OpenCascade backend for the Kernel trait. The only crate allowed to use unsafe.
//!
//! Phase 0 spike (T003): box, boolean with face history, face count, tessellation, and mass
//! properties, through a small hand-written C++ shim (`native/shim.cpp`) that is linked to
//! OCCT's shared libraries.
//!
//! OCCT is a native dependency that not every machine has. The build script looks for it
//! (see `build.rs`); when it is absent the crate still builds, [`available`] is false, and
//! [`OcctKernel::new`] returns an error.
//!
//! This crate uses facilities provided by the Open CASCADE Technology software.

#[cfg(occt)]
mod ffi;
#[cfg(occt)]
mod kernel;
#[cfg(occt)]
pub use kernel::OcctKernel;

#[cfg(not(occt))]
mod unavailable;
#[cfg(not(occt))]
pub use unavailable::OcctKernel;

/// True when this build is linked to OpenCascade.
#[must_use]
pub const fn available() -> bool {
    cfg!(occt)
}
