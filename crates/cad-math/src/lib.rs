//! Vectors, tolerances, units, expressions.
//!
//! Every geometric comparison in the app goes through [`Tolerance`].
//! Comparing floats with `==` anywhere else is a bug.

mod tolerance;
mod units;
mod vec3;

pub use tolerance::Tolerance;
pub use units::{Dimension, Quantity, Unit, UnitError};
pub use vec3::Vec3;
