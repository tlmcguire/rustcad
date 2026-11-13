//! Golden test harness, fixtures, snapshot tools.
//!
//! * [`ReferenceKernel`]: a box-only `Kernel` for tests.
//! * [`assert_golden`] and [`Golden`]: compare any kernel's results with a golden file.
//!
//! See ARCHITECTURE.md and ROADMAP.md.

pub mod golden;
mod reference_kernel;

pub use golden::{Golden, GoldenError, Mismatch, assert_golden};
pub use reference_kernel::ReferenceKernel;
