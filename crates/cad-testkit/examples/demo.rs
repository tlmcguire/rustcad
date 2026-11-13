//! Early end-to-end demo: model a part with the OpenCascade backend, tessellate it, and
//! render it to a PNG with the wgpu renderer. No window.
//!
//! Usage: `cargo run -p cad-testkit --example demo -- out.png`

use cad_kernel::{BoolOp, Kernel};
use cad_kernel_occt::OcctKernel;
use cad_math::{Tolerance, Vec3};
use cad_render::{Camera, Renderer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "demo.png".into());
    let kernel = OcctKernel::new()?;

    // A 40 x 20 x 10 block with a pocket in the top, a slot through one end, and a boss.
    let block = kernel.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))?;
    let pocket = kernel.make_box(Vec3::new(6.0, 5.0, 5.0), Vec3::new(16.0, 10.0, 10.0))?;
    let slot = kernel.make_box(Vec3::new(30.0, 7.0, -1.0), Vec3::new(11.0, 6.0, 12.0))?;
    let boss = kernel.make_box(Vec3::new(24.0, 2.0, 9.0), Vec3::new(4.0, 4.0, 5.0))?;
    let mut part = block;
    for (op, tool) in [
        (BoolOp::Subtract, pocket),
        (BoolOp::Subtract, slot),
        (BoolOp::Union, boss),
    ] {
        let result = kernel.boolean(op, part, tool)?;
        let h = &result.history;
        println!(
            "{op:?}: {} input faces tracked, {} generated",
            h.faces.len(),
            h.generated_faces.len()
        );
        part = result.shape;
    }

    let props = kernel.mass_props(part)?;
    println!("kernel:  {} {}", kernel.name(), OcctKernel::occt_version());
    println!("faces:   {}", kernel.face_count(part)?);
    println!("volume:  {:.3} mm^3", props.volume);
    println!("area:    {:.3} mm^2", props.area);

    let mesh = kernel.tessellate(part, Tolerance::default())?;
    let renderer = Renderer::headless()?;
    let scene = renderer.upload(&mesh)?;
    let camera = Camera::framing(Vec3::new(20.0, 10.0, 6.0), 25.0, Vec3::new(1.0, -1.2, 0.9))?;
    let frame = renderer.render(&scene, &camera, 1200, 800)?;
    frame.image.write_png(&path)?;
    println!(
        "render:  {} triangles on {}",
        scene.triangle_count(),
        renderer.adapter()
    );
    println!(
        "picked:  face under the image centre is {:?}",
        frame.pick(600, 400)
    );
    println!("wrote    {path}");
    Ok(())
}
