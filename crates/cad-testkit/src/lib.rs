//! Golden test harness, fixtures, snapshot tools.
//!
//! * [`ReferenceKernel`]: a box-only `Kernel` for tests.
//! * [`assert_golden`] and [`Golden`]: compare any kernel's results with a golden file.
//! * [`assert_snapshot`] and [`Snapshot`]: compare a rendered image with a reference image.
//!
//! See ARCHITECTURE.md and ROADMAP.md.

pub mod golden;
mod reference_kernel;
pub mod snapshot;

pub use golden::{Golden, GoldenError, Mismatch, assert_golden};
pub use reference_kernel::ReferenceKernel;
pub use snapshot::{Difference, Snapshot, SnapshotError, SnapshotSpec, assert_snapshot};
