//! Golden test harness, fixtures, snapshot tools.
//!
//! Currently provides [`ReferenceKernel`], a box-only `Kernel` for tests.
//! See ARCHITECTURE.md and ROADMAP.md.

mod reference_kernel;

pub use reference_kernel::ReferenceKernel;
