//! wgpu renderer, picking, offscreen rendering.
//!
//! Phase 0 spike (T005): draws a kernel [`Mesh`](cad_kernel::Mesh) with shaded faces and
//! face-boundary edges, to a window or to an image in memory, and reports the face under a
//! pixel from a GPU ID buffer.
//!
//! ```no_run
//! # fn main() -> Result<(), cad_render::RenderError> {
//! # let mesh = cad_kernel::Mesh::default();
//! use cad_math::Vec3;
//! use cad_render::{Camera, Renderer};
//!
//! let renderer = Renderer::headless()?;
//! let scene = renderer.upload(&mesh)?;
//! let camera = Camera::framing(Vec3::ZERO, 30.0, Vec3::new(1.0, -1.0, 1.0))?;
//! let frame = renderer.render(&scene, &camera, 800, 600)?;
//! frame.image.write_png("part.png")?;
//! let face = frame.pick(400, 300);
//! # Ok(()) }
//! ```

mod camera;
mod edges;
mod image;
mod renderer;

pub use camera::Camera;
pub use edges::feature_edges;
pub use image::{Frame, Image};
pub use renderer::{OFFSCREEN_FORMAT, Renderer, Scene, Targets};
/// The graphics library, re-exported for callers that own a window surface.
pub use wgpu;

/// Why a render call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// No graphics adapter is available, hardware or software.
    NoAdapter(String),
    /// The graphics device could not be opened or was lost.
    Device(String),
    /// The mesh cannot be drawn as given.
    InvalidMesh(String),
    /// The camera cannot produce a picture.
    InvalidCamera(&'static str),
    /// The requested image size is zero or larger than the adapter supports.
    InvalidSize {
        /// Requested width in pixels.
        width: u32,
        /// Requested height in pixels.
        height: u32,
        /// Largest dimension the adapter supports.
        max: u32,
    },
    /// Pixels could not be read back from the GPU.
    Readback(String),
    /// The image could not be encoded.
    Encode(String),
    /// The image file could not be written.
    Io(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdapter(m) => write!(f, "no graphics adapter: {m}"),
            Self::Device(m) => write!(f, "graphics device error: {m}"),
            Self::InvalidMesh(m) => write!(f, "invalid mesh: {m}"),
            Self::InvalidCamera(m) => write!(f, "invalid camera: {m}"),
            Self::InvalidSize { width, height, max } => {
                write!(
                    f,
                    "invalid image size {width} x {height}: each side must be 1 to {max}"
                )
            }
            Self::Readback(m) => write!(f, "cannot read pixels back: {m}"),
            Self::Encode(m) => write!(f, "cannot encode image: {m}"),
            Self::Io(m) => write!(f, "cannot write image: {m}"),
        }
    }
}

impl std::error::Error for RenderError {}
