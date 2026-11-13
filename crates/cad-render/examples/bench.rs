//! Measures frame time for a large mesh, without a window.
//!
//! Usage: `cargo run --release -p cad-render --example bench -- [triangles] [width] [height]`

use std::time::{Duration, Instant};

use cad_math::Vec3;
use cad_render::{Camera, OFFSCREEN_FORMAT, Renderer, wgpu};

#[path = "support/mod.rs"]
mod support;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).map(|a| a.parse::<u32>());
    let wanted = args.next().transpose()?.unwrap_or(1_000_000);
    let width = args.next().transpose()?.unwrap_or(1920);
    let height = args.next().transpose()?.unwrap_or(1080);
    let cells = (f64::from(wanted) / 2.0).sqrt().ceil() as u32;

    let renderer = Renderer::headless()?;
    println!("adapter:    {}", renderer.adapter());
    let mesh = support::grid_mesh(cells);
    let started = Instant::now();
    let scene = renderer.upload(&mesh)?;
    renderer.finish()?;
    println!("triangles:  {}", scene.triangle_count());
    println!("edges:      {}", scene.edge_count());
    println!(
        "upload:     {:.1} ms (includes edge extraction)",
        ms(started.elapsed())
    );

    let camera = Camera::framing(Vec3::ZERO, 150.0, Vec3::new(1.0, -1.0, 0.9))?;
    let targets = renderer.create_targets(width, height)?;
    let color = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("bench colour"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = color.create_view(&wgpu::TextureViewDescriptor::default());

    let frame = || -> Result<Duration, cad_render::RenderError> {
        let started = Instant::now();
        renderer.draw(&scene, &camera, &view, &targets)?;
        renderer.finish()?;
        Ok(started.elapsed())
    };
    for _ in 0..10 {
        frame()?;
    }
    let mut times = (0..200).map(|_| frame()).collect::<Result<Vec<_>, _>>()?;
    times.sort_unstable();
    println!("target:     {width} x {height}");
    println!(
        "frame time: median {:.2} ms, min {:.2} ms, max {:.2} ms over {} frames",
        ms(times[times.len() / 2]),
        ms(times[0]),
        ms(times[times.len() - 1]),
        times.len()
    );
    println!("            (draw, submit, and wait for the GPU to finish; no presentation)");

    let started = Instant::now();
    let picked = renderer.pick(&targets, width / 2, height / 2)?;
    println!(
        "pick:       {:.2} ms for one pixel -> {picked:?}",
        ms(started.elapsed())
    );
    let started = Instant::now();
    let full = renderer.render(&scene, &camera, width, height)?;
    println!(
        "offscreen:  {:.1} ms for render plus full colour and ID readback",
        ms(started.elapsed())
    );
    println!(
        "            centre pixel from full readback -> {:?}",
        full.pick(width / 2, height / 2)
    );
    Ok(())
}
