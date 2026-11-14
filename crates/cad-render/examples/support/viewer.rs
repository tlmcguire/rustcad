//! An interactive window for the torture cases.
//!
//! Drag with the left button to orbit, scroll to zoom, click to print the face under the
//! cursor, and press the left and right arrow keys to change case. The title shows the case,
//! its triangle count, and the frame time.

use std::sync::Arc;
use std::time::Instant;

use cad_math::Vec3;
use cad_render::{Camera, Renderer, Scene, Targets, wgpu};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::Case;
use crate::verify::DynResult;

/// Opens the cases of a demo in a window and returns when it closes.
///
/// # Errors
///
/// Window, adapter, and render errors.
pub fn run(demo: &str, cases: Vec<Case>) -> DynResult<()> {
    println!(
        "{demo}: drag to orbit, scroll to zoom, click to pick, left and right arrows to change case"
    );
    let mut app = App {
        demo: demo.to_string(),
        cases,
        index: 0,
        view: None,
        cursor: (0.0, 0.0),
        pressed_at: None,
        orbit: Orbit::default(),
        frames: 0,
        window_started: Instant::now(),
        frame_ms: 0.0,
        failure: None,
    };
    EventLoop::new()?.run_app(&mut app)?;
    app.failure.map_or(Ok(()), |message| Err(message.into()))
}

/// The user's changes to the case camera.
#[derive(Clone, Copy, Default)]
struct Orbit {
    yaw: f64,
    pitch: f64,
    /// Zoom steps: each one moves the eye 10 percent closer.
    zoom: i32,
}

impl Orbit {
    /// The case camera, turned and zoomed. The clip planes scale with the distance, so the
    /// depth precision under test stays the same.
    fn apply(self, base: &Camera) -> Camera {
        let scale = 0.9_f64.powi(self.zoom);
        if self.yaw == 0.0 && self.pitch == 0.0 {
            return Camera {
                eye: base.target + (base.eye - base.target) * scale,
                near: base.near * scale,
                far: base.far * scale,
                ..*base
            };
        }
        let offset = base.eye - base.target;
        let r = offset.length();
        let azimuth = offset.y.atan2(offset.x) + self.yaw;
        let elevation = ((offset.z / r).asin() + self.pitch).clamp(-1.5, 1.5);
        let direction = Vec3::new(
            elevation.cos() * azimuth.cos(),
            elevation.cos() * azimuth.sin(),
            elevation.sin(),
        );
        Camera {
            eye: base.target + direction * (r * scale),
            up: Vec3::Z,
            near: base.near * scale,
            far: base.far * scale,
            ..*base
        }
    }
}

struct View {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    queue: wgpu::Queue,
    renderer: Renderer,
    targets: Targets,
    /// The uploaded case, or why it could not be uploaded.
    scene: Result<Scene, String>,
    triangles: usize,
}

impl View {
    fn open(event_loop: &ActiveEventLoop) -> DynResult<Self> {
        let attributes = Window::default_attributes()
            .with_title("cad-render torture test")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0));
        let window = Arc::new(event_loop.create_window(attributes)?);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(event_loop.owned_display_handle()),
        ));
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        // As in `Renderer::headless`: the default limits would cap meshes at 3.2M triangles.
        let descriptor = wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&descriptor))?;
        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("the adapter cannot draw to this window")?;
        surface.configure(&device, &config);
        let info = adapter.get_info();
        let renderer = Renderer::new(
            device,
            queue.clone(),
            config.format,
            format!("{} ({:?})", info.name, info.backend),
        );
        println!("adapter: {}", renderer.adapter());
        let targets = renderer.create_targets(config.width, config.height)?;
        Ok(Self {
            window,
            surface,
            config,
            queue,
            renderer,
            targets,
            scene: Err("no case loaded".into()),
            triangles: 0,
        })
    }

    fn load(&mut self, case: &Case) {
        // Drop the previous scene first, so two large meshes are never on the GPU together.
        self.scene = Err("loading".into());
        println!("building {} ...", case.title);
        let started = Instant::now();
        let mesh = (case.build)();
        self.triangles = mesh.triangles.len();
        let built = started.elapsed();
        let started = Instant::now();
        self.scene = crate::verify::guarded(&self.renderer, || Ok(self.renderer.upload(&mesh)?))
            .map_err(|e| e.to_string());
        match &self.scene {
            Ok(_) => println!(
                "  {} triangles: built in {:.0} ms, uploaded in {:.0} ms",
                self.triangles,
                built.as_secs_f64() * 1000.0,
                started.elapsed().as_secs_f64() * 1000.0
            ),
            Err(e) => println!("  FAILED to upload: {e}"),
        }
    }

    fn resize(&mut self, width: u32, height: u32) -> DynResult<()> {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(self.renderer.device(), &self.config);
        self.targets = self
            .renderer
            .create_targets(self.config.width, self.config.height)?;
        Ok(())
    }

    /// Draws one frame. Returns whether it reached the screen.
    fn redraw(&mut self, camera: &Camera) -> DynResult<bool> {
        let Ok(scene) = &self.scene else {
            return Ok(false);
        };
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                let view = texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                self.renderer.draw(scene, camera, &view, &self.targets)?;
                self.queue.present(texture);
                Ok(true)
            }
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                Ok(false)
            }
            _ => {
                self.resize(self.config.width, self.config.height)?;
                Ok(false)
            }
        }
    }
}

struct App {
    demo: String,
    cases: Vec<Case>,
    index: usize,
    view: Option<View>,
    cursor: (f64, f64),
    /// Where the left button went down, to tell a click from a drag.
    pressed_at: Option<(f64, f64)>,
    orbit: Orbit,
    frames: u32,
    window_started: Instant,
    frame_ms: f64,
    failure: Option<String>,
}

impl App {
    fn camera(&self) -> Camera {
        self.orbit.apply(&self.cases[self.index].camera)
    }

    fn switch(&mut self, step: isize) {
        let n = self.cases.len() as isize;
        self.index = (self.index as isize + step).rem_euclid(n) as usize;
        self.orbit = Orbit::default();
        if let Some(view) = self.view.as_mut() {
            view.load(&self.cases[self.index]);
        }
        self.update_title();
    }

    fn update_title(&self) {
        let Some(view) = self.view.as_ref() else {
            return;
        };
        let case = &self.cases[self.index];
        let status = match &view.scene {
            Ok(_) => format!(
                "{} triangles, {:.1} ms per frame",
                view.triangles, self.frame_ms
            ),
            Err(e) => format!("FAILED: {e}"),
        };
        view.window.set_title(&format!(
            "torture {} [{}/{}] {} | {status}",
            self.demo,
            self.index + 1,
            self.cases.len(),
            case.title
        ));
    }

    fn handle(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) -> DynResult<()> {
        if self.view.is_none() {
            return Ok(());
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(view) = self.view.as_mut() {
                    view.resize(size.width, size.height)?;
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.logical_key {
                    Key::Named(NamedKey::ArrowRight) => self.switch(1),
                    Key::Named(NamedKey::ArrowLeft) => self.switch(-1),
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (dx, dy) = (position.x - self.cursor.0, position.y - self.cursor.1);
                self.cursor = (position.x, position.y);
                if self.pressed_at.is_some() {
                    self.orbit.yaw -= dx * 0.005;
                    self.orbit.pitch += dy * 0.005;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                };
                self.orbit.zoom += lines.round() as i32;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => self.pressed_at = Some(self.cursor),
                ElementState::Released => {
                    let start = self.pressed_at.take().unwrap_or(self.cursor);
                    let moved = (self.cursor.0 - start.0).hypot(self.cursor.1 - start.1);
                    if let (Some(view), true) = (self.view.as_ref(), moved < 3.0) {
                        let (x, y) = (self.cursor.0.max(0.0) as u32, self.cursor.1.max(0.0) as u32);
                        println!(
                            "click at ({x}, {y}): {:?}",
                            view.renderer.pick(&view.targets, x, y)?
                        );
                    }
                }
            },
            WindowEvent::RedrawRequested => {
                let camera = self.camera();
                let started = Instant::now();
                let drawn = match self.view.as_mut() {
                    Some(view) => {
                        let drawn = view.redraw(&camera)?;
                        if drawn {
                            view.renderer.finish()?;
                        }
                        drawn
                    }
                    None => false,
                };
                if drawn {
                    // Draw, submit, and wait for the GPU; presentation is not included.
                    let ms = started.elapsed().as_secs_f64() * 1000.0;
                    self.frame_ms = if self.frames == 0 {
                        ms
                    } else {
                        0.9 * self.frame_ms + 0.1 * ms
                    };
                    self.frames += 1;
                }
                if self.window_started.elapsed().as_millis() > 250 {
                    self.update_title();
                    self.window_started = Instant::now();
                }
                if let Some(view) = self.view.as_ref() {
                    view.window.request_redraw();
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: &dyn std::fmt::Display) {
        self.failure = Some(error.to_string());
        event_loop.exit();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.view.is_some() {
            return;
        }
        match View::open(event_loop) {
            Ok(mut view) => {
                view.load(&self.cases[self.index]);
                view.window.request_redraw();
                self.view = Some(view);
                self.update_title();
            }
            Err(e) => self.fail(event_loop, &e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let Err(e) = self.handle(event_loop, event) {
            self.fail(event_loop, &e);
        }
    }
}
