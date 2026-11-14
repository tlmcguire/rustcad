//! An interactive window for the torture cases, in the manner of an engine viewport.
//!
//! Controls:
//! - left drag: orbit; right drag, or shift and left drag: pan; scroll or pinch: zoom
//!   toward the cursor
//! - 1 to 9: view mode (lit, unlit, wireframe, triangles, faces, world normal, lighting
//!   only, scene depth, overdraw)
//! - W: wire overlay; E: face edges; space: turntable; R: reset the view
//! - `[` and `]`: turn the sun; `-` and `=`: lower and raise it. Lit cases re-bake their
//!   lighting by ray tracing on the CPU when the sun moves.
//! - left and right arrows: change case; H: hide the panel; Esc: quit; click: print the face
//!
//! A panel in the bottom left corner shows the case, the view mode, the toggles, the sun, and
//! the frame time.

use std::sync::Arc;
use std::time::Instant;

use cad_kernel::Mesh;
use cad_math::Vec3;
use cad_render::{Camera, Image, Navigation, Renderer, Scene, Targets, ViewMode, wgpu};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use crate::Case;
use crate::verify::DynResult;
use cad_render::trace::{self, Sky, Tracer};

const PANEL: (u32, u32) = (600, 196);

/// Opens the cases of a demo in a window and returns when it closes.
///
/// # Errors
///
/// Window, adapter, and render errors.
pub fn run(demo: &str, cases: Vec<Case>) -> DynResult<()> {
    println!(
        "{demo}: drag to orbit, right-drag or shift-drag to pan, scroll or pinch to zoom; 1-9 view modes, \
         W wire overlay, E edges, [ ] - = move the sun, R reset, arrows change case"
    );
    let mut app = App {
        demo: demo.to_string(),
        cases,
        index: 0,
        view: None,
        cursor: (0.0, 0.0),
        dragging: None,
        moved: 0.0,
        modifiers: ModifiersState::empty(),
        orbit: Navigation::default(),
        spinning: false,
        last_frame: Instant::now(),
        frames: 0,
        title_at: Instant::now(),
        frame_ms: 0.0,
        panel: true,
        failure: None,
    };
    EventLoop::new()?.run_app(&mut app)?;
    app.failure.map_or(Ok(()), |message| Err(message.into()))
}

/// A case built and on the GPU, with what it needs to re-bake its light.
struct Loaded {
    mesh: Mesh,
    colors: Vec<[u8; 3]>,
    /// The ray tracer and the sun, for lit cases.
    lighting: Option<(Tracer, Sky)>,
    scene: Result<Scene, String>,
}

struct View {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    queue: wgpu::Queue,
    renderer: Renderer,
    targets: Targets,
    /// The panel texture, when the surface can be copied into.
    panel: Option<wgpu::Texture>,
    loaded: Option<Loaded>,
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
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("the adapter cannot draw to this window")?;
        let copyable = surface
            .get_capabilities(&adapter)
            .usages
            .contains(wgpu::TextureUsages::COPY_DST);
        if copyable {
            config.usage |= wgpu::TextureUsages::COPY_DST;
        }
        surface.configure(&device, &config);
        let panel = copyable.then(|| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("torture panel"),
                size: wgpu::Extent3d {
                    width: PANEL.0,
                    height: PANEL.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: config.format,
                usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        });
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
            panel,
            loaded: None,
        })
    }

    fn load(&mut self, case: &Case) {
        // Drop the previous scene first, so two large meshes are never on the GPU together.
        self.loaded = None;
        println!("building {} ...", case.title);
        let started = Instant::now();
        let (mesh, colors) = (case.build)();
        println!(
            "  {} triangles built in {:.0} ms",
            mesh.triangles.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        let lighting = case.sky.map(|sky| (Tracer::new(&mesh), sky));
        self.renderer.set_edges(case.edges);
        let mut loaded = Loaded {
            mesh,
            colors,
            lighting,
            scene: Err("not uploaded".into()),
        };
        self.upload(&mut loaded);
        self.loaded = Some(loaded);
    }

    /// Bakes (for lit cases) and uploads.
    fn upload(&self, loaded: &mut Loaded) {
        let started = Instant::now();
        let light = loaded.lighting.as_ref().map(|(bvh, sky)| {
            let light = trace::bake(&loaded.mesh, bvh, *sky, 12, 0.05);
            println!(
                "  baked light by ray tracing in {:.0} ms",
                started.elapsed().as_secs_f64() * 1000.0
            );
            light
        });
        let started = Instant::now();
        loaded.scene = crate::verify::guarded(&self.renderer, || {
            Ok(match &light {
                Some(light) => self
                    .renderer
                    .upload_baked(&loaded.mesh, &loaded.colors, light)?,
                None => self
                    .renderer
                    .upload_with_colors(&loaded.mesh, &loaded.colors)?,
            })
        })
        .map_err(|e| e.to_string());
        match &loaded.scene {
            Ok(_) => println!(
                "  uploaded in {:.0} ms",
                started.elapsed().as_secs_f64() * 1000.0
            ),
            Err(e) => println!("  FAILED to upload: {e}"),
        }
    }

    /// Moves the sun of a lit case and re-bakes.
    fn move_sun(&mut self, turn: f64, raise: f64) {
        let Some(mut loaded) = self.loaded.take() else {
            return;
        };
        if let Some((_, sky)) = loaded.lighting.as_mut() {
            let s = sky.sun;
            let azimuth = s.y.atan2(s.x) + turn;
            let elevation = ((s.z / s.length()).asin() + raise).clamp(0.05, 1.5);
            sky.sun = Vec3::new(
                elevation.cos() * azimuth.cos(),
                elevation.cos() * azimuth.sin(),
                elevation.sin(),
            );
            self.upload(&mut loaded);
        }
        self.loaded = Some(loaded);
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

    /// Draws one frame, with the panel if given. Returns whether it reached the screen.
    fn redraw(&mut self, camera: &Camera, panel: Option<&Image>) -> DynResult<bool> {
        let Some(Ok(scene)) = self.loaded.as_ref().map(|l| &l.scene) else {
            return Ok(false);
        };
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                let view = texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                self.renderer.draw(scene, camera, &view, &self.targets)?;
                if let (Some(image), Some(panel)) = (panel, &self.panel) {
                    self.blit_panel(image, panel, &texture.texture);
                }
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

    /// Copies the panel image into the bottom left corner of the frame.
    fn blit_panel(&self, image: &Image, panel: &wgpu::Texture, frame: &wgpu::Texture) {
        let (w, h) = PANEL;
        if self.config.width < w + 24 || self.config.height < h + 24 {
            return;
        }
        let bgra = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut bytes = image.rgba.clone();
        if bgra {
            for px in bytes.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        let extent = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: panel,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            extent,
        );
        let mut encoder =
            self.renderer
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("torture panel"),
                });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: panel,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: frame,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 12,
                    y: self.config.height - h - 12,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            extent,
        );
        self.queue.submit([encoder.finish()]);
    }
}

struct App {
    demo: String,
    cases: Vec<Case>,
    index: usize,
    view: Option<View>,
    cursor: (f64, f64),
    /// The button held down, if any.
    dragging: Option<MouseButton>,
    /// How far the cursor moved while the button was down, to tell a click from a drag.
    moved: f64,
    modifiers: ModifiersState,
    orbit: Navigation,
    spinning: bool,
    last_frame: Instant,
    frames: u32,
    title_at: Instant,
    frame_ms: f64,
    panel: bool,
    failure: Option<String>,
}

impl App {
    fn camera(&self) -> Camera {
        self.orbit.apply(&self.cases[self.index].camera)
    }

    fn switch(&mut self, step: isize) {
        let n = self.cases.len() as isize;
        self.index = (self.index as isize + step).rem_euclid(n) as usize;
        self.orbit.reset();
        if let Some(view) = self.view.as_mut() {
            view.load(&self.cases[self.index]);
        }
    }

    /// The panel: what is shown and how to change it.
    fn panel_image(&self) -> Option<Image> {
        let view = self.view.as_ref()?;
        let loaded = view.loaded.as_ref()?;
        let case = &self.cases[self.index];
        let r = &view.renderer;
        let mut image = crate::verify::blank(PANEL.0, PANEL.1, [16, 17, 20, 255]);
        crate::verify::fill(&mut image, 0, 0, 3, PANEL.1, [90, 200, 110, 255]);
        let mode = r.view_mode();
        let number = ViewMode::ALL.iter().position(|m| *m == mode).unwrap_or(0) + 1;
        let on = |b: bool| if b { "ON" } else { "OFF" };
        let sun = loaded.lighting.as_ref().map_or_else(
            || "NO BAKED LIGHT".to_string(),
            |(_, sky)| {
                let s = sky.sun;
                format!(
                    "SUN {:.0} DEG, {:.0} UP  ([ ] - =)",
                    s.y.atan2(s.x).to_degrees().rem_euclid(360.0),
                    (s.z / s.length()).asin().to_degrees()
                )
            },
        );
        let triangles = match &loaded.scene {
            Ok(scene) => format!(
                "{} TRIANGLES  {:.1} MS/FRAME",
                crate::text::thousands(u64::from(scene.triangle_count())),
                self.frame_ms
            ),
            Err(e) => format!("FAILED: {e}"),
        };
        let lines = [
            (
                format!(
                    "{} [{}/{}]",
                    self.demo.to_uppercase(),
                    self.index + 1,
                    self.cases.len()
                ),
                [255, 255, 255, 255],
            ),
            (case.title.to_uppercase(), [200, 200, 200, 255]),
            (triangles, [140, 235, 140, 255]),
            (
                format!("VIEW {number} {} (1-9)", mode.name().to_uppercase()),
                [255, 214, 120, 255],
            ),
            (
                format!(
                    "WIRE {} (W)  EDGES {} (E)  SPIN {} (SPACE)",
                    on(r.wire_overlay()),
                    on(r.edges()),
                    on(self.spinning)
                ),
                [255, 214, 120, 255],
            ),
            (sun, [255, 214, 120, 255]),
            (
                "DRAG ORBIT  RIGHT-DRAG PAN  SCROLL ZOOM TO CURSOR".to_string(),
                [150, 150, 160, 255],
            ),
        ];
        for (i, (text, color)) in lines.iter().enumerate() {
            let fits: String = text.chars().take(48).collect();
            crate::text::draw(&mut image, 14, 10 + i as u32 * 26, 2, *color, &fits);
        }
        Some(image)
    }

    fn update_title(&self) {
        let Some(view) = self.view.as_ref() else {
            return;
        };
        view.window.set_title(&format!(
            "torture {} [{}/{}] {} | {} | {:.1} ms",
            self.demo,
            self.index + 1,
            self.cases.len(),
            self.cases[self.index].title,
            view.renderer.view_mode().name(),
            self.frame_ms
        ));
    }

    fn key(&mut self, event_loop: &ActiveEventLoop, key: Key) {
        let Some(view) = self.view.as_mut() else {
            return;
        };
        match key {
            Key::Named(NamedKey::ArrowRight) => self.switch(1),
            Key::Named(NamedKey::ArrowLeft) => self.switch(-1),
            Key::Named(NamedKey::Escape) => event_loop.exit(),
            Key::Named(NamedKey::Space) => self.spinning = !self.spinning,
            Key::Character(c) => {
                let r = &view.renderer;
                match c.as_str() {
                    "w" | "W" => r.set_wire_overlay(!r.wire_overlay()),
                    "e" | "E" => r.set_edges(!r.edges()),
                    "h" | "H" => self.panel = !self.panel,
                    "r" | "R" => self.orbit.reset(),
                    "[" => view.move_sun(-0.26, 0.0),
                    "]" => view.move_sun(0.26, 0.0),
                    "-" => view.move_sun(0.0, -0.1),
                    "=" | "+" => view.move_sun(0.0, 0.1),
                    digit => {
                        let mode = digit
                            .parse::<usize>()
                            .ok()
                            .and_then(|n| ViewMode::ALL.get(n.wrapping_sub(1)));
                        if let Some(mode) = mode {
                            r.set_view_mode(*mode);
                        }
                    }
                }
            }
            _ => {}
        }
        self.update_title();
    }

    /// Pans by a cursor movement in pixels: eye and target slide together across the view.
    fn pan(&mut self, dx: f64, dy: f64) {
        let Some(view) = self.view.as_ref() else {
            return;
        };
        let height = view.config.height;
        let base = self.cases[self.index].camera;
        self.orbit.pan(&base, dx, dy, height);
    }

    /// Zooms toward the cursor.
    fn zoom(&mut self, steps: f64) {
        let Some(view) = self.view.as_ref() else {
            return;
        };
        let size = (view.config.width, view.config.height);
        let base = self.cases[self.index].camera;
        self.orbit.zoom_at(&base, steps, self.cursor, size);
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
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.key(event_loop, event.logical_key);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (dx, dy) = (position.x - self.cursor.0, position.y - self.cursor.1);
                self.cursor = (position.x, position.y);
                self.moved += dx.abs() + dy.abs();
                match self.dragging {
                    Some(MouseButton::Right | MouseButton::Middle) => self.pan(dx, dy),
                    Some(MouseButton::Left) if self.modifiers.shift_key() => self.pan(dx, dy),
                    Some(MouseButton::Left) => self.orbit.orbit(dx, dy),
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                };
                self.zoom(lines);
            }
            WindowEvent::PinchGesture { delta, .. } if delta.is_finite() => {
                self.zoom(delta * 8.0);
            }
            WindowEvent::MouseInput { state, button, .. } => match state {
                ElementState::Pressed => {
                    self.dragging = Some(button);
                    self.moved = 0.0;
                }
                ElementState::Released => {
                    let click = button == MouseButton::Left && self.moved < 3.0;
                    self.dragging = None;
                    if let (Some(view), true) = (self.view.as_ref(), click) {
                        let (x, y) = (self.cursor.0.max(0.0) as u32, self.cursor.1.max(0.0) as u32);
                        println!(
                            "click at ({x}, {y}): {:?}",
                            view.renderer.pick(&view.targets, x, y)?
                        );
                    }
                }
            },
            WindowEvent::RedrawRequested => {
                let elapsed = self.last_frame.elapsed().as_secs_f64();
                self.last_frame = Instant::now();
                if self.spinning {
                    // One turn every 40 seconds.
                    self.orbit.yaw += elapsed * std::f64::consts::TAU / 40.0;
                }
                let camera = self.camera();
                let panel = if self.panel { self.panel_image() } else { None };
                let started = Instant::now();
                let drawn = match self.view.as_mut() {
                    Some(view) => {
                        let drawn = view.redraw(&camera, panel.as_ref())?;
                        if drawn {
                            view.renderer.finish()?;
                        }
                        drawn
                    }
                    None => false,
                };
                if drawn {
                    let ms = started.elapsed().as_secs_f64() * 1000.0;
                    self.frame_ms = if self.frames == 0 {
                        ms
                    } else {
                        0.9 * self.frame_ms + 0.1 * ms
                    };
                    self.frames += 1;
                }
                if self.title_at.elapsed().as_millis() > 250 {
                    self.update_title();
                    self.title_at = Instant::now();
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
