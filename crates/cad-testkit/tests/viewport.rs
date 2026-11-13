//! Acceptance tests for T005: offscreen rendering and ID-buffer picking.
//!
//! These need a graphics adapter (hardware or software) but no window or display.

use std::collections::BTreeSet;

use cad_kernel::{FaceIndex, Kernel, Mesh};
use cad_math::{Tolerance, Vec3};
use cad_render::{Camera, Frame, RenderError, Renderer, Scene, wgpu};
use cad_testkit::ReferenceKernel;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const CENTER: Vec3 = Vec3::new(20.0, 10.0, 5.0);
/// Radius of the sphere around the 40 x 20 x 10 box, rounded up.
const RADIUS: f64 = 23.0;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn box_mesh() -> Result<Mesh, Box<dyn std::error::Error>> {
    let kernel = ReferenceKernel::new();
    let shape = kernel.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))?;
    Ok(kernel.tessellate(shape, Tolerance::default())?)
}

fn setup() -> Result<(Renderer, Scene), Box<dyn std::error::Error>> {
    let renderer = Renderer::headless()?;
    let scene = renderer.upload(&box_mesh()?)?;
    Ok((renderer, scene))
}

fn view(renderer: &Renderer, scene: &Scene, from: Vec3) -> Result<Frame, RenderError> {
    renderer.render(
        scene,
        &Camera::framing(CENTER, RADIUS, from)?,
        WIDTH,
        HEIGHT,
    )
}

fn is_edge_colored(pixel: [u8; 4]) -> bool {
    pixel[..3].iter().all(|c| *c < 60)
}

#[test]
fn box_uploads_as_twelve_triangles_and_its_face_boundaries() -> TestResult {
    let (_, scene) = setup()?;
    assert_eq!(scene.triangle_count(), 12);
    // Four boundary edges per face. Each of the 12 box edges appears once per adjacent face.
    assert_eq!(scene.edge_count(), 24);
    Ok(())
}

#[test]
fn picking_returns_the_face_that_faces_the_camera() -> TestResult {
    let (renderer, scene) = setup()?;
    // Face order of the reference kernel: -X, +X, -Y, +Y, -Z, +Z.
    let directions = [
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::X,
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::Y,
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::Z,
    ];
    for (face, direction) in (0u32..).zip(directions) {
        let frame = view(&renderer, &scene, direction)?;
        assert_eq!(
            frame.pick(WIDTH / 2 + 3, HEIGHT / 2 + 3),
            Some(FaceIndex(face)),
            "looking from {direction:?}"
        );
    }
    Ok(())
}

#[test]
fn picking_misses_outside_the_shape_and_outside_the_frame() -> TestResult {
    let (renderer, scene) = setup()?;
    let frame = view(&renderer, &scene, Vec3::Z)?;
    for (x, y) in [
        (0, 0),
        (WIDTH - 1, 0),
        (0, HEIGHT - 1),
        (WIDTH - 1, HEIGHT - 1),
    ] {
        assert_eq!(frame.pick(x, y), None, "corner ({x}, {y})");
    }
    assert_eq!(frame.pick(WIDTH, 0), None);
    assert_eq!(frame.pick(0, HEIGHT), None);
    Ok(())
}

#[test]
fn a_corner_view_shows_exactly_the_three_near_faces() -> TestResult {
    let (renderer, scene) = setup()?;
    let frame = view(&renderer, &scene, Vec3::new(1.0, -1.0, 1.0))?;
    let mut seen = BTreeSet::new();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            seen.extend(frame.pick(x, y).map(|f| f.0));
        }
    }
    // +X, -Y, +Z.
    assert_eq!(seen, BTreeSet::from([1, 2, 5]));
    Ok(())
}

#[test]
fn faces_are_shaded_and_edges_are_drawn() -> TestResult {
    let (renderer, scene) = setup()?;
    let frame = view(&renderer, &scene, Vec3::new(1.0, -1.0, 1.0))?;
    let image = &frame.image;
    let background = image.pixel(0, 0).ok_or("no corner pixel")?;
    assert!(!is_edge_colored(background), "background {background:?}");

    // Average the colour of each visible face, away from its edges.
    let mut sums = [[0u64; 4]; 6];
    let mut edge_pixels = 0;
    let mut edge_pixels_on_shape = 0;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let pixel = image.pixel(x, y).ok_or("pixel missing")?;
            let face = frame.pick(x, y);
            if is_edge_colored(pixel) {
                edge_pixels += 1;
                edge_pixels_on_shape += u32::from(face.is_some());
            } else if let Some(FaceIndex(face)) = face {
                let sum = &mut sums[face as usize];
                for (total, channel) in sum[..3].iter_mut().zip(pixel) {
                    *total += u64::from(channel);
                }
                sum[3] += 1;
            }
        }
    }
    assert!(edge_pixels > 200, "only {edge_pixels} edge pixels");
    // Edges do not write IDs, so picking through an edge still finds the face behind it.
    assert!(
        edge_pixels_on_shape > 100,
        "only {edge_pixels_on_shape} edge pixels over faces"
    );

    let brightness = |face: usize| sums[face][0] * 1000 / sums[face][3].max(1);
    for face in [1, 2, 5] {
        assert!(
            sums[face][3] > 500,
            "face {face} covers {} pixels",
            sums[face][3]
        );
        assert!(
            brightness(face) > 60_000,
            "face {face} is too dark to be shaded"
        );
    }
    // The three faces meet the view direction at the same angle here, so a headlight shades
    // them alike. From a lopsided view the face turned toward the camera is brightest.
    let lopsided = view(&renderer, &scene, Vec3::new(0.2, -0.3, 1.0))?;
    let mean = |frame: &Frame, face: u32| -> u64 {
        let (mut total, mut count) = (0u64, 0u64);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let pixel = frame.image.pixel(x, y).unwrap_or_default();
                if frame.pick(x, y) == Some(FaceIndex(face)) && !is_edge_colored(pixel) {
                    total += u64::from(pixel[0]);
                    count += 1;
                }
            }
        }
        total / count.max(1)
    };
    let (top, front) = (mean(&lopsided, 5), mean(&lopsided, 2));
    assert!(
        top > front + 20,
        "top {top} should be clearly brighter than front {front}"
    );
    Ok(())
}

#[test]
fn renders_to_a_png_file() -> TestResult {
    let (renderer, scene) = setup()?;
    let frame = view(&renderer, &scene, Vec3::new(1.0, -1.0, 1.0))?;
    let png = frame.image.to_png()?;
    assert_eq!(
        png.get(..8),
        Some(&b"\x89PNG\r\n\x1a\n"[..]),
        "PNG signature"
    );
    assert_eq!(png.get(12..16), Some(&b"IHDR"[..]));
    assert_eq!(png.get(16..20), Some(&WIDTH.to_be_bytes()[..]));
    assert_eq!(png.get(20..24), Some(&HEIGHT.to_be_bytes()[..]));

    let path = std::env::temp_dir().join(format!("cad-render-t005-{}.png", std::process::id()));
    frame.image.write_png(&path)?;
    let written = std::fs::read(&path)?;
    std::fs::remove_file(&path)?;
    assert_eq!(written, png);
    Ok(())
}

#[test]
fn rendering_twice_gives_identical_frames() -> TestResult {
    let (renderer, scene) = setup()?;
    let from = Vec3::new(1.0, -1.0, 1.0);
    assert_eq!(
        view(&renderer, &scene, from)?,
        view(&renderer, &scene, from)?
    );
    Ok(())
}

#[test]
fn single_pixel_pick_agrees_with_the_full_id_buffer() -> TestResult {
    let (renderer, scene) = setup()?;
    let camera = Camera::framing(CENTER, RADIUS, Vec3::new(1.0, -1.0, 1.0))?;
    let frame = renderer.render(&scene, &camera, WIDTH, HEIGHT)?;

    // The path a window uses: caller-owned colour target, renderer-owned ID and depth.
    let targets = renderer.create_targets(WIDTH, HEIGHT)?;
    let color = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: cad_render::OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    renderer.draw(&scene, &camera, &color_view, &targets)?;

    for (x, y) in [
        (0, 0),
        (WIDTH / 2, HEIGHT / 2),
        (WIDTH / 2, HEIGHT / 3),
        (WIDTH / 3, 150),
    ] {
        assert_eq!(
            renderer.pick(&targets, x, y)?,
            frame.pick(x, y),
            "pixel ({x}, {y})"
        );
    }
    assert_eq!(renderer.pick(&targets, WIDTH, HEIGHT)?, None);
    Ok(())
}

#[test]
fn bad_input_is_reported_not_panicked() -> TestResult {
    let (renderer, scene) = setup()?;
    let camera = Camera::framing(CENTER, RADIUS, Vec3::Z)?;

    assert!(matches!(
        renderer.upload(&Mesh::default()),
        Err(RenderError::InvalidMesh(_))
    ));
    let mut broken = box_mesh()?;
    broken.triangles[0] = [0, 1, 999];
    assert!(matches!(
        renderer.upload(&broken),
        Err(RenderError::InvalidMesh(_))
    ));
    let mut broken = box_mesh()?;
    broken.triangle_faces.pop();
    assert!(matches!(
        renderer.upload(&broken),
        Err(RenderError::InvalidMesh(_))
    ));
    let mut broken = box_mesh()?;
    broken.normals.pop();
    assert!(matches!(
        renderer.upload(&broken),
        Err(RenderError::InvalidMesh(_))
    ));

    for (width, height) in [(0, HEIGHT), (WIDTH, 0), (u32::MAX, HEIGHT)] {
        let got = renderer.render(&scene, &camera, width, height);
        assert!(
            matches!(got, Err(RenderError::InvalidSize { .. })),
            "{width} x {height}"
        );
    }

    let no_view = Camera {
        target: camera.eye,
        ..camera
    };
    let got = renderer.render(&scene, &no_view, WIDTH, HEIGHT);
    assert!(matches!(got, Err(RenderError::InvalidCamera(_))));
    Ok(())
}
