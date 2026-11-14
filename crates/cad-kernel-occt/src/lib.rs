//! OpenCascade backend for the Kernel trait. The only crate allowed to use unsafe.
//!
//! Phase 0 spike (T003): box, boolean with face history, face count, tessellation, and mass
//! properties, through a small hand-written C++ shim (`native/shim.cpp`) that is linked to
//! OCCT's shared libraries. T008 added B-rep and STEP exchange.
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

pub mod worker;

/// For the worker process: moves standard output to standard error, so that nothing OCCT
/// prints can reach the protocol, and returns the original standard output to write
/// protocol frames to. Call it once, before any other call into this crate.
///
/// # Errors
///
/// Fails when the descriptors cannot be duplicated, or in a build without OCCT.
pub fn claim_stdout() -> std::io::Result<std::fs::File> {
    #[cfg(occt)]
    {
        ffi::claim_stdout()
    }
    #[cfg(not(occt))]
    {
        Err(std::io::Error::other("this build was made without OCCT"))
    }
}

/// True when this build is linked to OpenCascade.
#[must_use]
pub const fn available() -> bool {
    cfg!(occt)
}
