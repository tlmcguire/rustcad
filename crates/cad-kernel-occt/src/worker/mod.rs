//! Running a kernel in a separate worker process (T004 spike, `docs/PLAN.md` section 6.4).
//!
//! * [`serve`] is the worker side: it answers kernel calls arriving on a byte stream.
//! * [`WorkerKernel`] is the client side: a [`Kernel`](cad_kernel::Kernel) that starts a
//!   worker process, sends it each call over a pipe, and survives the worker dying.
//! * The `occt-worker` binary in this crate is a worker that serves an
//!   [`OcctKernel`](crate::OcctKernel) on its standard input and output.
//!
//! Nothing here needs OCCT or `unsafe`; `serve` works with any kernel.

mod client;
mod server;
mod wire;

pub use client::{WorkerConfig, WorkerKernel};
pub use server::{ServeOptions, serve};
