//! Renders a box-shaped mesh to a PNG without a window.
//!
//! Usage: `cargo run -p cad-render --example snapshot -- out.png`

use cad_math::Vec3;
use cad_render::{Camera, Renderer};

#[path = "support/mod.rs"]
mod support;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "snapshot.png".into());
    let renderer = Renderer::headless()?;
    let scene = renderer.upload(&support::box_mesh())?;
    let camera = Camera::framing(Vec3::new(20.0, 10.0, 5.0), 23.0, Vec3::new(1.0, -1.0, 0.8))?;
    let frame = renderer.render(&scene, &camera, 800, 600)?;
    frame.image.write_png(&path)?;
    println!("adapter: {}", renderer.adapter());
    println!(
        "wrote {path}; face under the centre pixel: {:?}",
        frame.pick(400, 300)
    );
    Ok(())
}
