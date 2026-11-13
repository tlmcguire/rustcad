//! Opens a window, draws an orbiting box, and prints the face under each click.
//!
//! Usage: `cargo run -p cad-render --example viewport -- [frames]`
//! With a frame count the window closes by itself after that many frames.

use std::sync::Arc;
use std::time::Instant;

use cad_math::Vec3;
use cad_render::{Camera, Renderer, Scene, Targets, wgpu};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

#[path = "support/mod.rs"]
mod support;

type AnyError = Box<dyn std::error::Error>;

struct View {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    queue: wgpu::Queue,
    renderer: Renderer,
    scene: Scene,
    targets: Targets,
}

impl View {
    fn open(event_loop: &ActiveEventLoop) -> Result<Self, AnyError> {
        let attributes = Window::default_attributes().with_title("cad-render viewport spike");
        let window = Arc::new(event_loop.create_window(attributes)?);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(event_loop.owned_display_handle()),
        ));
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("the adapter cannot draw to this window")?;
        surface.configure(&device, &config);

        let info = adapter.get_info();
        let name = format!("{} ({:?})", info.name, info.backend);
        let renderer = Renderer::new(device, queue.clone(), config.format, name);
        let scene = renderer.upload(&support::box_mesh())?;
        let targets = renderer.create_targets(config.width, config.height)?;
        println!(
            "adapter: {}, surface format: {:?}",
            renderer.adapter(),
            config.format
        );
        Ok(Self {
            window,
            surface,
            config,
            queue,
            renderer,
            scene,
            targets,
        })
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), AnyError> {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(self.renderer.device(), &self.config);
        self.targets = self
            .renderer
            .create_targets(self.config.width, self.config.height)?;
        Ok(())
    }

    /// Draws one frame. Returns whether it reached the screen.
    fn redraw(&mut self, frame: u64) -> Result<bool, AnyError> {
        let angle = frame as f64 * 0.01;
        let from = Vec3::new(angle.cos(), angle.sin(), 0.7);
        let camera = Camera::framing(Vec3::new(20.0, 10.0, 5.0), 23.0, from)?;
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                let view = texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                self.renderer
                    .draw(&self.scene, &camera, &view, &self.targets)?;
                self.queue.present(texture);
                Ok(true)
            }
            // Hidden behind other windows or busy: nothing to draw this frame.
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                Ok(false)
            }
            other => {
                // Outdated or lost: reconfigure and try again next frame.
                println!("reconfiguring the surface: {other:?}");
                self.resize(self.config.width, self.config.height)?;
                Ok(false)
            }
        }
    }
}

struct App {
    view: Option<View>,
    cursor: (f64, f64),
    frames: u64,
    limit: Option<u64>,
    started: Instant,
    failure: Option<String>,
}

impl App {
    fn handle(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) -> Result<(), AnyError> {
        let Some(view) = self.view.as_mut() else {
            return Ok(());
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => view.resize(size.width, size.height)?,
            WindowEvent::CursorMoved { position, .. } => self.cursor = (position.x, position.y),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = (self.cursor.0.max(0.0) as u32, self.cursor.1.max(0.0) as u32);
                println!(
                    "click at ({x}, {y}): {:?}",
                    view.renderer.pick(&view.targets, x, y)?
                );
            }
            WindowEvent::RedrawRequested => {
                self.frames += u64::from(view.redraw(self.frames)?);
                if self.limit.is_some_and(|limit| self.frames >= limit) {
                    let (x, y) = (view.config.width / 2, view.config.height / 2);
                    let picked = view.renderer.pick(&view.targets, x, y)?;
                    let per_frame =
                        self.started.elapsed().as_secs_f64() * 1000.0 / self.frames as f64;
                    println!(
                        "presented {} frames at {} x {}, {per_frame:.2} ms per frame with vsync",
                        self.frames, view.config.width, view.config.height
                    );
                    println!("face under the window centre in the last frame: {picked:?}");
                    event_loop.exit();
                }
                view.window.request_redraw();
            }
            _ => {}
        }
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: &AnyError) {
        self.failure = Some(error.to_string());
        event_loop.exit();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.view.is_none() {
            match View::open(event_loop) {
                Ok(view) => {
                    view.window.request_redraw();
                    self.started = Instant::now();
                    self.view = Some(view);
                }
                Err(e) => self.fail(event_loop, &e),
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let Err(e) = self.handle(event_loop, event) {
            self.fail(event_loop, &e);
        }
    }
}

fn main() -> Result<(), AnyError> {
    let limit = std::env::args().nth(1).map(|a| a.parse()).transpose()?;
    let mut app = App {
        view: None,
        cursor: (0.0, 0.0),
        frames: 0,
        limit,
        started: Instant::now(),
        failure: None,
    };
    EventLoop::new()?.run_app(&mut app)?;
    app.failure.map_or(Ok(()), |message| Err(message.into()))
}
