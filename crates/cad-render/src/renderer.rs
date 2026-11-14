use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc;

use cad_kernel::{FaceIndex, Mesh};
use cad_math::Vec3;
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::image::{Frame, Image, decode_id};
use crate::{RenderError, edges::boundary_edges};

/// Colour format of offscreen renders.
pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Colour of faces with no colour of their own, 8-bit sRGB.
pub const DEFAULT_FACE_COLOR: [u8; 3] = [188, 203, 223];
/// Linear colours.
const EDGE_COLOR: [f32; 4] = [0.01, 0.01, 0.015, 1.0];
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.85,
    g: 0.86,
    b: 0.88,
    a: 1.0,
};
const TECH_BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.004,
    g: 0.004,
    b: 0.005,
    a: 1.0,
};

/// Words per face vertex: position, normal, face index, packed sRGB colour.
const FACE_VERTEX_WORDS: usize = 8;
const FACE_VERTEX_BYTES: u64 = 32;
const FACE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32, 3 => Unorm8x4];
const EDGE_VERTEX_BYTES: u64 = 12;
/// Bytes per pixel of both the colour and the ID format.
const PIXEL_BYTES: u32 = 4;

/// What the viewport shows, after the view modes of game engine editors.
///
/// Every mode except [`ViewMode::Overdraw`] writes the face ID buffer, so picking works the
/// same in all of them. Face boundary edges are drawn in the lit mode only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Each face in its colour with dark edges on face boundaries. Lit by the scene's baked
    /// lighting when it has some (see [`Renderer::upload_baked`]), otherwise from the eye.
    #[default]
    Lit,
    /// Each face in its colour, unlit and without edges: the base colour alone.
    Unlit,
    /// Every triangle outlined, on a dark fill.
    Wireframe,
    /// Every triangle in its own colour, to show how finely a surface is tessellated.
    Triangles,
    /// Every face in a colour made from its index: the picking buffer made visible.
    Faces,
    /// The world-space surface normal as red, green, and blue.
    Normals,
    /// The lighting alone, on white surfaces: the baked light when the scene has some,
    /// otherwise the light from the eye.
    Lighting,
    /// Distance from the eye as a colour ramp, striped where the stored depth value crosses
    /// each 1/64. Stripes bunch where depth precision is spent; where there are none, nearby
    /// surfaces share a depth value and can fight.
    Depth,
    /// Every surface drawn with additive colour and no depth test: brightness counts how many
    /// triangles cover each pixel. The ID buffer is not written.
    Overdraw,
}

impl ViewMode {
    /// Every mode, in order.
    pub const ALL: [Self; 9] = [
        Self::Lit,
        Self::Unlit,
        Self::Wireframe,
        Self::Triangles,
        Self::Faces,
        Self::Normals,
        Self::Lighting,
        Self::Depth,
        Self::Overdraw,
    ];

    /// A short name for menus and labels.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Lit => "Lit",
            Self::Unlit => "Unlit",
            Self::Wireframe => "Wireframe",
            Self::Triangles => "Triangles",
            Self::Faces => "Faces",
            Self::Normals => "World normal",
            Self::Lighting => "Lighting only",
            Self::Depth => "Scene depth",
            Self::Overdraw => "Overdraw",
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Lit => 0,
            Self::Unlit => 1,
            Self::Wireframe => 2,
            Self::Triangles => 3,
            Self::Faces => 4,
            Self::Normals => 5,
            Self::Depth => 6,
            Self::Overdraw => 7,
            Self::Lighting => 8,
        }
    }

    fn from_code(code: u8) -> Self {
        Self::ALL
            .into_iter()
            .find(|mode| mode.code() == code)
            .unwrap_or_default()
    }

    /// The lit and unlit modes show the model; the others are technical views on black.
    fn background(self) -> wgpu::Color {
        match self {
            Self::Lit | Self::Unlit => BACKGROUND,
            _ => TECH_BACKGROUND,
        }
    }

    fn draws_edges(self) -> bool {
        self == Self::Lit
    }
}

/// Draws meshes with shaded faces and edges, and reports which face is under a pixel.
///
/// Works without a window: see [`Renderer::headless`] and [`Renderer::render`]. A windowed
/// viewport creates its own device for its surface and passes it to [`Renderer::new`].
#[derive(Debug)]
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    faces: wgpu::RenderPipeline,
    overdraw: wgpu::RenderPipeline,
    edges: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    planes_layout: wgpu::BindGroupLayout,
    adapter: String,
    mode: AtomicU8,
    overlay: AtomicBool,
    show_edges: AtomicBool,
}

/// A mesh uploaded to the GPU.
#[derive(Debug)]
pub struct Scene {
    face_vertices: wgpu::Buffer,
    face_vertex_count: u32,
    /// The plane of each drawn triangle, from which faces take their depth.
    planes: wgpu::BindGroup,
    edge_vertices: Option<wgpu::Buffer>,
    edge_vertex_count: u32,
    /// Vertex positions are stored relative to this point, the centre of the mesh's bounds,
    /// so a small part far from the world origin keeps the precision of single floats.
    origin: Vec3,
    skipped: u32,
    /// Whether vertices carry baked light.
    baked: bool,
}

impl Scene {
    /// Whether the scene carries baked lighting.
    #[must_use]
    pub fn is_baked(&self) -> bool {
        self.baked
    }

    /// Number of triangles drawn.
    #[must_use]
    pub fn triangle_count(&self) -> u32 {
        self.face_vertex_count / 3
    }

    /// Number of edge segments drawn.
    #[must_use]
    pub fn edge_count(&self) -> u32 {
        self.edge_vertex_count / 2
    }

    /// Number of triangles left out because they have no area: zero area, or a height under
    /// a billionth of their longest side. They could never be seen, and a GPU could still
    /// rasterize a pixel or two of them and let them be picked.
    #[must_use]
    pub fn skipped_triangles(&self) -> u32 {
        self.skipped
    }
}

/// The depth and face ID buffers that accompany a colour target of the same size.
#[derive(Debug)]
pub struct Targets {
    ids: wgpu::Texture,
    id_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl Targets {
    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }
}

impl Renderer {
    /// Creates a renderer with no window or display, for the CLI, MCP, and tests.
    ///
    /// Tries a hardware adapter first and a software one second.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::NoAdapter`] when the machine has no usable graphics backend, or
    /// [`RenderError::Device`] when the adapter refuses to open.
    pub fn headless() -> Result<Self, RenderError> {
        pollster::block_on(async {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let mut failures = Vec::new();
            for force_fallback_adapter in [false, true] {
                let options = wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter,
                    ..Default::default()
                };
                match instance.request_adapter(&options).await {
                    Ok(adapter) => {
                        // The default limits cap one buffer at 256 MiB, about 3.2 million
                        // triangles. Ask for what the adapter can do instead.
                        let descriptor = wgpu::DeviceDescriptor {
                            required_limits: adapter.limits(),
                            ..Default::default()
                        };
                        let (device, queue) = adapter
                            .request_device(&descriptor)
                            .await
                            .map_err(|e| RenderError::Device(e.to_string()))?;
                        let info = adapter.get_info();
                        let name = format!("{} ({:?})", info.name, info.backend);
                        return Ok(Self::new(device, queue, OFFSCREEN_FORMAT, name));
                    }
                    Err(e) => failures.push(e.to_string()),
                }
            }
            Err(RenderError::NoAdapter(failures.join("; ")))
        })
    }

    /// Creates a renderer on an existing device, drawing to colour targets of `color_format`.
    /// `adapter` is a description of the graphics adapter for logs and reports.
    #[must_use]
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        color_format: wgpu::TextureFormat,
        adapter: String,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cad-render viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cad-render globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cad-render globals"),
            size: GLOBALS_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cad-render globals"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cad-render"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        // Faces also read the plane of their triangle, to compute depth exactly.
        let planes_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cad-render planes"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let face_layout_with_planes =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("cad-render faces"),
                bind_group_layouts: &[Some(&bind_layout), Some(&planes_layout)],
                immediate_size: 0,
            });

        // `additive`: the overdraw pipeline, which adds colour, ignores depth, and writes no IDs.
        let pipeline = |label,
                        layout: &wgpu::PipelineLayout,
                        vertex: (&str, wgpu::VertexBufferLayout<'_>),
                        fragment,
                        topology,
                        additive: bool| {
            let id_writes = match topology {
                wgpu::PrimitiveTopology::TriangleList if !additive => wgpu::ColorWrites::ALL,
                _ => wgpu::ColorWrites::empty(),
            };
            let add = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            };
            let color_target = wgpu::ColorTargetState {
                format: color_format,
                blend: additive.then_some(wgpu::BlendState {
                    color: add,
                    alpha: add,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex.0),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(vertex.1)],
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(!additive),
                    depth_compare: Some(if additive {
                        wgpu::CompareFunction::Always
                    } else {
                        // Reversed depth: 1 at the near plane, 0 at the far plane.
                        wgpu::CompareFunction::GreaterEqual
                    }),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[
                        Some(color_target),
                        Some(wgpu::ColorTargetState {
                            format: ID_FORMAT,
                            blend: None,
                            write_mask: id_writes,
                        }),
                    ],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let face_layout = || wgpu::VertexBufferLayout {
            array_stride: FACE_VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &FACE_ATTRIBUTES,
        };
        let faces = pipeline(
            "cad-render faces",
            &face_layout_with_planes,
            ("face_vs", face_layout()),
            "face_fs",
            wgpu::PrimitiveTopology::TriangleList,
            false,
        );
        let overdraw = pipeline(
            "cad-render overdraw",
            &layout,
            ("face_vs", face_layout()),
            "overdraw_fs",
            wgpu::PrimitiveTopology::TriangleList,
            true,
        );
        let edges = pipeline(
            "cad-render edges",
            &layout,
            (
                "edge_vs",
                wgpu::VertexBufferLayout {
                    array_stride: EDGE_VERTEX_BYTES,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                },
            ),
            "edge_fs",
            wgpu::PrimitiveTopology::LineList,
            false,
        );

        Self {
            device,
            queue,
            faces,
            overdraw,
            edges,
            globals,
            bind_group,
            planes_layout,
            adapter,
            mode: AtomicU8::new(ViewMode::default().code()),
            overlay: AtomicBool::new(false),
            show_edges: AtomicBool::new(true),
        }
    }

    /// The graphics device, for callers that create their own colour targets.
    #[must_use]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Sets what later draws show.
    pub fn set_view_mode(&self, mode: ViewMode) {
        self.mode.store(mode.code(), Ordering::Relaxed);
    }

    /// What draws show.
    #[must_use]
    pub fn view_mode(&self) -> ViewMode {
        ViewMode::from_code(self.mode.load(Ordering::Relaxed))
    }

    /// Sets whether later draws outline every triangle in dark lines over the view mode, as
    /// engines overlay a wireframe on a shaded view. Overdraw is never overlaid.
    pub fn set_wire_overlay(&self, on: bool) {
        self.overlay.store(on, Ordering::Relaxed);
    }

    /// Whether draws outline every triangle over the view mode.
    #[must_use]
    pub fn wire_overlay(&self) -> bool {
        self.overlay.load(Ordering::Relaxed)
    }

    /// Sets whether the lit view draws face boundary edges. On by default; a scene that is
    /// not a part, such as terrain split into patches, may look better without them.
    pub fn set_edges(&self, on: bool) {
        self.show_edges.store(on, Ordering::Relaxed);
    }

    /// Whether the lit view draws face boundary edges.
    #[must_use]
    pub fn edges(&self) -> bool {
        self.show_edges.load(Ordering::Relaxed)
    }

    /// The graphics adapter in use, for logs and reports.
    #[must_use]
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Uploads a mesh with every face in [`DEFAULT_FACE_COLOR`]. Face boundaries become the
    /// drawn edges.
    ///
    /// # Errors
    ///
    /// As [`Renderer::upload_with_colors`].
    pub fn upload(&self, mesh: &Mesh) -> Result<Scene, RenderError> {
        self.upload_with_colors(mesh, &[])
    }

    /// Uploads a mesh with a colour per face: `colors[i]` is the 8-bit sRGB colour of face
    /// `i`. Faces past the end of `colors` get [`DEFAULT_FACE_COLOR`]. Face boundaries become
    /// the drawn edges.
    ///
    /// Triangles without area are left out (see [`Scene::skipped_triangles`]), and a
    /// zero-length or non-finite vertex normal is replaced by its triangle's own normal.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidMesh`] when the mesh is empty or has no triangle with
    /// area, its arrays disagree in length, a vertex position is not finite, a triangle refers
    /// to a vertex that does not exist, or the mesh needs a larger vertex buffer than the
    /// device allows.
    pub fn upload_with_colors(
        &self,
        mesh: &Mesh,
        colors: &[[u8; 3]],
    ) -> Result<Scene, RenderError> {
        self.upload_inner(mesh, colors, None)
    }

    /// Uploads a mesh with a colour per face, as [`Renderer::upload_with_colors`], and light
    /// baked per vertex: `light[i]` is the light at `mesh.positions[i]`, from 0 for none to 1
    /// for full, computed offline (for example by ray tracing shadows and ambient occlusion).
    /// The lit view shows faces in that light instead of the light from the eye. Values are
    /// clamped to 0 to 1 and stored in eight bits.
    ///
    /// # Errors
    ///
    /// As [`Renderer::upload_with_colors`], and [`RenderError::InvalidMesh`] when `light` and
    /// the positions differ in length.
    pub fn upload_baked(
        &self,
        mesh: &Mesh,
        colors: &[[u8; 3]],
        light: &[f32],
    ) -> Result<Scene, RenderError> {
        self.upload_inner(mesh, colors, Some(light))
    }

    fn upload_inner(
        &self,
        mesh: &Mesh,
        colors: &[[u8; 3]],
        light: Option<&[f32]>,
    ) -> Result<Scene, RenderError> {
        let limits = self.device.limits();
        let fits = |bytes, what: &str| within(bytes, limits.max_buffer_size, what);
        let triangles = u64::try_from(mesh.triangles.len()).unwrap_or(u64::MAX);
        fits(
            triangles.saturating_mul(3 * FACE_VERTEX_BYTES),
            &format!("{triangles} triangles"),
        )?;
        let plane_bytes = triangles.saturating_mul(PLANE_BYTES);
        let planes_of = format!("the planes of {triangles} triangles");
        fits(plane_bytes, &planes_of)?;
        within(
            plane_bytes,
            limits.max_storage_buffer_binding_size,
            &planes_of,
        )?;
        let Prepared {
            words: faces,
            planes,
            kept,
            origin,
        } = prepare(mesh, colors, light)?;
        let plane_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cad-render planes"),
                contents: bytemuck::cast_slice(&planes),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let planes = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cad-render planes"),
            layout: &self.planes_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: plane_buffer.as_entire_binding(),
            }],
        });
        let face_vertices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cad-render face vertices"),
                contents: bytemuck::cast_slice(&faces),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let mut lines: Vec<f32> = Vec::new();
        let drawn = kept
            .iter()
            .map(|&k| (&mesh.triangles[k], &mesh.triangle_faces[k]));
        for [a, b] in boundary_edges(drawn) {
            for index in [a, b] {
                lines.extend(local(position(mesh, index)?, origin));
            }
        }
        fits(
            u64::try_from(lines.len())
                .unwrap_or(u64::MAX)
                .saturating_mul(4),
            "the edges",
        )?;
        let edge_vertices = (!lines.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cad-render edge vertices"),
                    contents: bytemuck::cast_slice(&lines),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });

        let count = |items: usize, per_vertex: usize| {
            u32::try_from(items / per_vertex)
                .map_err(|_| RenderError::InvalidMesh("too many vertices to draw".into()))
        };
        Ok(Scene {
            face_vertices,
            planes,
            face_vertex_count: count(faces.len(), FACE_VERTEX_WORDS)?,
            edge_vertices,
            edge_vertex_count: count(lines.len(), 3)?,
            origin,
            skipped: count(mesh.triangles.len() - kept.len(), 1)?,
            baked: light.is_some(),
        })
    }

    /// Creates the depth and ID buffers for a colour target of the given size.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidSize`] when either dimension is zero or exceeds what the
    /// adapter supports.
    pub fn create_targets(&self, width: u32, height: u32) -> Result<Targets, RenderError> {
        let ids = self.create_texture("cad-render face ids", ID_FORMAT, width, height)?;
        let depth = self.create_texture("cad-render depth", DEPTH_FORMAT, width, height)?;
        Ok(Targets {
            id_view: ids.create_view(&wgpu::TextureViewDescriptor::default()),
            depth_view: depth.create_view(&wgpu::TextureViewDescriptor::default()),
            ids,
            width,
            height,
        })
    }

    /// Draws a scene into `color`, and its face IDs and depth into `targets`.
    ///
    /// `color` must have the format this renderer was created with and the size of `targets`.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidCamera`] when the camera cannot produce a picture.
    pub fn draw(
        &self,
        scene: &Scene,
        camera: &Camera,
        color: &wgpu::TextureView,
        targets: &Targets,
    ) -> Result<(), RenderError> {
        let aspect = f64::from(targets.width) / f64::from(targets.height);
        // The scene's vertices are relative to its origin; move the camera the same way, in
        // double precision, so the matrices never carry large coordinates.
        let camera = &Camera {
            eye: camera.eye - scene.origin,
            target: camera.target - scene.origin,
            ..*camera
        };
        let (view, view_proj) = camera.matrices(aspect)?;
        let mode = self.view_mode();
        let mut globals = [0.0_f32; GLOBALS_WORDS];
        // Edges move toward the eye by the depth of EDGE_PULL_PX pixels at their distance.
        #[allow(clippy::cast_possible_truncation)] // The GPU works in single precision.
        let flags = u8::from(scene.baked) | (u8::from(self.wire_overlay()) << 1);
        let params = [
            f32::from(mode.code()),
            camera.near as f32,
            camera.far as f32,
            f32::from(flags),
        ];
        let pull = EDGE_PULL_PX * 2.0 * (camera.fov_y / 2.0).tan() / f64::from(targets.height);
        #[allow(clippy::cast_possible_truncation)] // The GPU works in single precision.
        let eye_pull = [
            camera.eye.x as f32,
            camera.eye.y as f32,
            camera.eye.z as f32,
            pull as f32,
        ];
        // For rebuilding each pixel's view ray in the fragment shader.
        #[allow(clippy::cast_possible_truncation)]
        let viewport = [
            targets.width as f32,
            targets.height as f32,
            (camera.fov_y / 2.0).tan() as f32,
            aspect as f32,
        ];
        let values = view_proj
            .iter()
            .chain(&view)
            .flatten()
            .chain(&params)
            .chain(&EDGE_COLOR)
            .chain(&eye_pull)
            .chain(&viewport);
        for (slot, value) in globals.iter_mut().zip(values) {
            *slot = *value;
        }
        self.queue
            .write_buffer(&self.globals, 0, bytemuck::cast_slice(&globals));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cad-render draw"),
            });
        {
            let store = wgpu::StoreOp::Store;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cad-render viewport"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: color,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(mode.background()),
                            store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &targets.id_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_bind_group(1, &scene.planes, &[]);
            pass.set_pipeline(if mode == ViewMode::Overdraw {
                &self.overdraw
            } else {
                &self.faces
            });
            pass.set_vertex_buffer(0, scene.face_vertices.slice(..));
            pass.draw(0..scene.face_vertex_count, 0..1);
            if let (Some(edges), true) = (&scene.edge_vertices, mode.draws_edges() && self.edges())
            {
                pass.set_pipeline(&self.edges);
                pass.set_vertex_buffer(0, edges.slice(..));
                pass.draw(0..scene.edge_vertex_count, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// Blocks until the GPU has finished everything submitted so far.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Device`] when the device is lost while waiting.
    pub fn finish(&self) -> Result<(), RenderError> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|e| RenderError::Device(e.to_string()))
    }

    /// The face under a pixel of the last picture drawn into `targets`, or `None` for
    /// background and for pixels outside the target. The origin is the top left corner.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Readback`] when the ID buffer cannot be read.
    pub fn pick(
        &self,
        targets: &Targets,
        x: u32,
        y: u32,
    ) -> Result<Option<FaceIndex>, RenderError> {
        if x >= targets.width || y >= targets.height {
            return Ok(None);
        }
        let bytes = self.read_texture(&targets.ids, [x, y], [1, 1])?;
        let id = bytes
            .first_chunk::<4>()
            .ok_or_else(|| RenderError::Readback("ID buffer returned no data".into()))?;
        Ok(decode_id(u32::from_le_bytes(*id)))
    }

    /// Renders a scene to an image in memory, with no window, and returns it together with
    /// the face under every pixel.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidSize`], [`RenderError::InvalidCamera`], or
    /// [`RenderError::Readback`].
    pub fn render(
        &self,
        scene: &Scene,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<Frame, RenderError> {
        let targets = self.create_targets(width, height)?;
        let color = self.create_texture("cad-render colour", OFFSCREEN_FORMAT, width, height)?;
        self.draw(
            scene,
            camera,
            &color.create_view(&wgpu::TextureViewDescriptor::default()),
            &targets,
        )?;
        let rgba = self.read_texture(&color, [0, 0], [width, height])?;
        let ids = self
            .read_texture(&targets.ids, [0, 0], [width, height])?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        Ok(Frame {
            image: Image {
                width,
                height,
                rgba,
            },
            ids,
        })
    }

    fn create_texture(
        &self,
        label: &str,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Result<wgpu::Texture, RenderError> {
        let max = self.device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > max || height > max {
            return Err(RenderError::InvalidSize { width, height, max });
        }
        Ok(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }))
    }

    /// Reads a rectangle of a four-byte-per-pixel texture as tightly packed rows.
    fn read_texture(
        &self,
        texture: &wgpu::Texture,
        origin: [u32; 2],
        size: [u32; 2],
    ) -> Result<Vec<u8>, RenderError> {
        let failed = |what: String| RenderError::Readback(what);
        let [width, height] = size;
        let row_bytes = width * PIXEL_BYTES;
        let padded_row_bytes = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cad-render readback"),
            size: u64::from(padded_row_bytes) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cad-render readback"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        let (sender, receiver) = mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            // The receiver only goes away if this call has already failed.
            let _ = sender.send(result);
        });
        self.finish()?;
        receiver
            .recv()
            .map_err(|e| failed(e.to_string()))?
            .map_err(|e| failed(e.to_string()))?;

        let mapped = buffer
            .get_mapped_range(..)
            .map_err(|e| failed(e.to_string()))?;
        let (row, padded) = (row_bytes as usize, padded_row_bytes as usize);
        let mut out = Vec::with_capacity(row * height as usize);
        for line in mapped.chunks(padded) {
            out.extend_from_slice(line.get(..row).ok_or_else(|| failed("short row".into()))?);
        }
        drop(mapped);
        buffer.unmap();
        Ok(out)
    }
}

/// Two matrices, the shading parameters, the edge colour, the eye with the edge pull, and the
/// viewport.
const GLOBALS_WORDS: usize = 48;
const GLOBALS_BYTES: u64 = 192;

/// How far edges are drawn in front of their faces, in pixels of depth at the edge's distance.
///
/// A line pixel's centre can lie up to half a pixel off the edge, where a steep or curved face
/// is already nearer than the line. A fixed offset in depth units is far too small at some
/// distances and too large at others, and edges broke into dashes. Pulling toward the eye by a
/// screen-relative amount keeps edges whole on faces up to about 70 degrees from the view,
/// and lets a hidden edge show through only when it is within this much of the front face.
const EDGE_PULL_PX: f64 = 1.5;

fn position(mesh: &Mesh, index: u32) -> Result<Vec3, RenderError> {
    usize::try_from(index)
        .ok()
        .and_then(|i| mesh.positions.get(i))
        .copied()
        .ok_or_else(|| RenderError::InvalidMesh(format!("vertex {index} does not exist")))
}

/// A position relative to `origin`, in single precision for the GPU.
#[allow(clippy::cast_possible_truncation)]
fn local(p: Vec3, origin: Vec3) -> [f32; 3] {
    let d = p - origin;
    [d.x as f32, d.y as f32, d.z as f32]
}

/// The plane through `point` with unit normal `n`, in single precision. Coplanar triangles of
/// different faces get the same four numbers, so the depth computed from them is the same to
/// the bit, and flush faces tie exactly instead of speckling.
#[allow(clippy::cast_possible_truncation)]
fn plane(n: Vec3, point: Vec3) -> [f32; 4] {
    // Canonical sign: the first nonzero component positive, so a plane seen from either side
    // is stored the same way.
    let flip = if n.x < 0.0 || (n.x == 0.0 && (n.y < 0.0 || (n.y == 0.0 && n.z < 0.0))) {
        -1.0
    } else {
        1.0
    };
    let n = n * flip;
    [n.x as f32, n.y as f32, n.z as f32, n.dot(point) as f32]
}

/// Bytes of plane data per drawn triangle.
const PLANE_BYTES: u64 = 16;

/// An error unless `bytes` of `what` fit in the device's limit of `max` bytes.
fn within(bytes: u64, max: u64, what: &str) -> Result<(), RenderError> {
    if bytes <= max {
        Ok(())
    } else {
        Err(RenderError::InvalidMesh(format!(
            "{what} need {} MiB; the device allows {} MiB per buffer",
            bytes >> 20,
            max >> 20
        )))
    }
}

/// A triangle whose height is under this fraction of its longest side has no area to draw.
const DEGENERATE: f64 = 1e-9;

/// What upload makes of a mesh.
struct Prepared {
    /// Eight words per vertex, three vertices per drawn triangle.
    words: Vec<u32>,
    /// Four floats per drawn triangle: its unit normal `n` and offset `c`, with `n . p = c`
    /// for points `p` relative to the origin.
    planes: Vec<f32>,
    /// Indices of the triangles drawn.
    kept: Vec<usize>,
    /// The point vertex positions are relative to.
    origin: Vec3,
}

/// Checks a mesh and expands it to three independent vertices per triangle, eight words each:
/// position relative to the mesh's centre and normal as float bits, the face index, and the
/// face's sRGB colour packed as RGBA bytes. Triangles without area are left out.
fn prepare(
    mesh: &Mesh,
    colors: &[[u8; 3]],
    light: Option<&[f32]>,
) -> Result<Prepared, RenderError> {
    let invalid = |what: String| Err(RenderError::InvalidMesh(what));
    if mesh.normals.len() != mesh.positions.len() {
        return invalid("normals and positions differ in length".into());
    }
    if mesh.triangle_faces.len() != mesh.triangles.len() {
        return invalid("triangle_faces and triangles differ in length".into());
    }
    if mesh.triangles.is_empty() {
        return invalid("mesh has no triangles".into());
    }
    if light.is_some_and(|l| l.len() != mesh.positions.len()) {
        return invalid("baked light and positions differ in length".into());
    }
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if let Some(i) = mesh.positions.iter().position(|p| !finite(*p)) {
        return invalid(format!("vertex {i} has a position that is not finite"));
    }
    let (mut lo, mut hi) = (mesh.positions[0], mesh.positions[0]);
    for p in &mesh.positions {
        lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    let origin = (lo + hi) * 0.5;

    let mut words = Vec::with_capacity(mesh.triangles.len() * 3 * FACE_VERTEX_WORDS);
    let mut kept = Vec::with_capacity(mesh.triangles.len());
    let mut planes = Vec::with_capacity(mesh.triangles.len() * 4);
    for (k, (triangle, face)) in mesh.triangles.iter().zip(&mesh.triangle_faces).enumerate() {
        let corners = [
            position(mesh, triangle[0])?,
            position(mesh, triangle[1])?,
            position(mesh, triangle[2])?,
        ];
        let sides = [
            corners[1] - corners[0],
            corners[2] - corners[0],
            corners[2] - corners[1],
        ];
        let longest = sides.iter().map(|s| s.dot(*s)).fold(0.0, f64::max);
        let cross = sides[0].cross(sides[1]);
        // |cross| is the longest side times the height over it.
        if cross.length() <= DEGENERATE * longest {
            continue;
        }
        let flat = cross.normalized().unwrap_or(Vec3::Z);
        let [r, g, b] = colors
            .get(face.0 as usize)
            .copied()
            .unwrap_or(DEFAULT_FACE_COLOR);
        let rgb = [r, g, b];
        for (&index, &p) in triangle.iter().zip(&corners) {
            // `position` succeeded, so the normal at the same index exists.
            let given = mesh
                .normals
                .get(index as usize)
                .copied()
                .unwrap_or_default();
            let n = if finite(given) && given.length() > 1e-12 {
                given
            } else {
                flat
            };
            words.extend(local(p, origin).map(f32::to_bits));
            words.extend(local(n, Vec3::ZERO).map(f32::to_bits));
            words.push(face.0);
            // The alpha byte carries baked light; without a bake it is full.
            let baked = light
                .and_then(|l| l.get(index as usize))
                .map_or(255, |&l| (l.clamp(0.0, 1.0) * 255.0).round() as u8);
            let [r, g, b] = rgb;
            words.push(u32::from_le_bytes([r, g, b, baked]));
        }
        planes.extend(plane(flat, corners[0] - origin));
        kept.push(k);
    }
    if kept.is_empty() {
        return invalid("mesh has no triangle with area".into());
    }
    Ok(Prepared {
        words,
        planes,
        kept,
        origin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cad_math::Vec3;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const SIZE: u32 = 64;
    /// Pixels per millimetre on the z = 0 plane for [`camera`].
    const SCALE: f64 = 10.16;

    /// Looking down -Z from 10 mm, with +X right and +Y up on screen.
    fn camera() -> Camera {
        Camera {
            eye: Vec3::new(0.0, 0.0, 10.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov_y: Camera::DEFAULT_FOV_Y,
            near: 1.0,
            far: 20.0,
        }
    }

    /// The pixel over a point of the z = 0 plane.
    fn at(x: f64, y: f64) -> (u32, u32) {
        let px = (f64::from(SIZE) / 2.0 + x * SCALE) as u32;
        let py = (f64::from(SIZE) / 2.0 - y * SCALE) as u32;
        (px, py)
    }

    /// A 2 x 2 mm quad facing +Z with its lower left corner at `(x, y, z)`, as two triangles.
    fn quad(mesh: &mut Mesh, x: f64, y: f64, z: f64, face: u32) {
        let base = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
        for (dx, dy) in [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)] {
            mesh.positions.push(Vec3::new(x + dx, y + dy, z));
            mesh.normals.push(Vec3::Z);
        }
        mesh.triangles
            .extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
        mesh.triangle_faces.extend([FaceIndex(face); 2]);
    }

    /// Face 0 on the left of the origin, face 1 on the right.
    fn two_quads() -> Mesh {
        let mut mesh = Mesh::default();
        quad(&mut mesh, -2.0, -1.0, 0.0, 0);
        quad(&mut mesh, 0.0, -1.0, 0.0, 1);
        mesh
    }

    fn render(
        renderer: &Renderer,
        mesh: &Mesh,
        colors: &[[u8; 3]],
        mode: ViewMode,
    ) -> Result<Frame, Box<dyn std::error::Error>> {
        let scene = renderer.upload_with_colors(mesh, colors)?;
        renderer.set_view_mode(mode);
        Ok(renderer.render(&scene, &camera(), SIZE, SIZE)?)
    }

    fn pixel(frame: &Frame, (x, y): (u32, u32)) -> Result<[u8; 4], String> {
        frame
            .image
            .pixel(x, y)
            .ok_or_else(|| format!("no pixel at ({x}, {y})"))
    }

    fn close(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
        a.iter().zip(&b).all(|(x, y)| x.abs_diff(*y) <= tolerance)
    }

    #[test]
    fn view_modes_have_distinct_names_and_round_trip() {
        let renderer_default = ViewMode::default();
        assert_eq!(renderer_default, ViewMode::Lit);
        let mut names: Vec<&str> = ViewMode::ALL.iter().map(|m| m.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ViewMode::ALL.len());
        for mode in ViewMode::ALL {
            assert_eq!(ViewMode::from_code(mode.code()), mode);
        }
        assert_eq!(ViewMode::from_code(200), ViewMode::Lit);
        assert!(ViewMode::Lit.draws_edges());
        for mode in &ViewMode::ALL[1..] {
            assert!(!mode.draws_edges(), "{mode:?}");
        }
    }

    #[test]
    fn the_view_mode_is_remembered() -> TestResult {
        let renderer = Renderer::headless()?;
        assert_eq!(renderer.view_mode(), ViewMode::Lit);
        renderer.set_view_mode(ViewMode::Depth);
        assert_eq!(renderer.view_mode(), ViewMode::Depth);
        Ok(())
    }

    #[test]
    fn vertex_words_carry_the_face_colour_or_the_default() -> TestResult {
        let words = prepare(&two_quads(), &[[10, 20, 30]], None)?.words;
        assert_eq!(words.len(), 4 * 3 * FACE_VERTEX_WORDS);
        let color = |vertex: usize| words[vertex * FACE_VERTEX_WORDS + 7];
        assert_eq!(color(0), u32::from_le_bytes([10, 20, 30, 255]));
        let [r, g, b] = DEFAULT_FACE_COLOR;
        assert_eq!(color(6), u32::from_le_bytes([r, g, b, 255]));
        assert_eq!(
            words[6 * FACE_VERTEX_WORDS + 6],
            1,
            "face index of vertex 6"
        );
        Ok(())
    }

    #[test]
    fn unlit_faces_show_their_colours_exactly() -> TestResult {
        let renderer = Renderer::headless()?;
        let frame = render(
            &renderer,
            &two_quads(),
            &[[200, 100, 50], [20, 160, 90]],
            ViewMode::Unlit,
        )?;
        assert!(close(
            pixel(&frame, at(-1.0, -0.5))?,
            [200, 100, 50, 255],
            2
        ));
        assert!(close(pixel(&frame, at(1.0, -0.5))?, [20, 160, 90, 255], 2));
        Ok(())
    }

    #[test]
    fn faces_without_a_colour_get_the_default() -> TestResult {
        let renderer = Renderer::headless()?;
        let frame = render(&renderer, &two_quads(), &[], ViewMode::Unlit)?;
        let [r, g, b] = DEFAULT_FACE_COLOR;
        assert!(close(pixel(&frame, at(1.0, -0.5))?, [r, g, b, 255], 2));
        Ok(())
    }

    #[test]
    fn lit_faces_facing_the_eye_are_fully_lit_with_edges_and_a_light_background() -> TestResult {
        let renderer = Renderer::headless()?;
        let frame = render(&renderer, &two_quads(), &[[200, 100, 50]], ViewMode::Lit)?;
        assert!(close(
            pixel(&frame, at(-1.0, -0.5))?,
            [200, 100, 50, 255],
            2
        ));
        assert!(pixel(&frame, (1, 1))?[0] > 200, "light background");
        // The shared edge at x = 0 is drawn dark.
        let row = at(0.0, -0.5).1;
        let darkest = (28..36)
            .filter_map(|x| frame.image.pixel(x, row))
            .map(|p| p[0])
            .min()
            .unwrap_or(255);
        assert!(darkest < 60, "darkest {darkest}");
        Ok(())
    }

    #[test]
    fn technical_views_are_on_black_without_edges_and_keep_picking() -> TestResult {
        let renderer = Renderer::headless()?;
        let mesh = two_quads();
        let lit = render(&renderer, &mesh, &[], ViewMode::Lit)?;
        for mode in [
            ViewMode::Wireframe,
            ViewMode::Triangles,
            ViewMode::Faces,
            ViewMode::Normals,
            ViewMode::Depth,
        ] {
            let frame = render(&renderer, &mesh, &[], mode)?;
            assert!(pixel(&frame, (1, 1))?[0] < 30, "{mode:?} background");
            assert_eq!(frame.ids, lit.ids, "{mode:?} IDs");
        }
        Ok(())
    }

    #[test]
    fn faces_view_colours_each_face_and_triangles_view_each_triangle() -> TestResult {
        let renderer = Renderer::headless()?;
        let mesh = two_quads();
        let faces = render(&renderer, &mesh, &[], ViewMode::Faces)?;
        // The left quad's two triangles: lower right and upper left of its diagonal.
        let (lower, upper) = (at(-0.5, -0.6), at(-1.5, 0.6));
        assert_eq!(pixel(&faces, lower)?, pixel(&faces, upper)?);
        assert_ne!(pixel(&faces, lower)?, pixel(&faces, at(1.0, 0.0))?);
        let triangles = render(&renderer, &mesh, &[], ViewMode::Triangles)?;
        assert_ne!(pixel(&triangles, lower)?, pixel(&triangles, upper)?);
        Ok(())
    }

    #[test]
    fn wireframe_draws_triangle_sides_and_leaves_the_inside_dark() -> TestResult {
        let renderer = Renderer::headless()?;
        let frame = render(&renderer, &two_quads(), &[], ViewMode::Wireframe)?;
        // (-1, 0) is on the left quad's diagonal; (-0.5, -0.5) is 0.7 mm (7 px) from it.
        let wire = pixel(&frame, at(-1.0, 0.0))?;
        let inside = pixel(&frame, at(-0.5, -0.5))?;
        assert!(wire[2] > 150, "wire {wire:?}");
        // The fill is a faint tint of the face colour.
        assert!(
            inside[2] < 80 && wire[2] > inside[2] + 100,
            "inside {inside:?}"
        );
        Ok(())
    }

    #[test]
    fn normals_view_shows_plus_z_as_blue() -> TestResult {
        let renderer = Renderer::headless()?;
        let frame = render(&renderer, &two_quads(), &[], ViewMode::Normals)?;
        assert!(close(pixel(&frame, at(1.0, 0.0))?, [128, 128, 255, 255], 2));
        Ok(())
    }

    #[test]
    fn depth_view_tells_near_from_far() -> TestResult {
        let renderer = Renderer::headless()?;
        let mut mesh = Mesh::default();
        quad(&mut mesh, -2.0, -1.0, 0.0, 0);
        quad(&mut mesh, 0.0, -1.0, 6.0, 1);
        let frame = render(&renderer, &mesh, &[], ViewMode::Depth)?;
        let near = pixel(&frame, (SIZE / 2 + 12, SIZE / 2))?;
        let far = pixel(&frame, at(-1.0, 0.0))?;
        assert!(
            far[0] > near[0] && far[1] > near[1],
            "near {near:?}, far {far:?}"
        );
        Ok(())
    }

    #[test]
    fn overdraw_adds_up_layers_and_writes_no_ids() -> TestResult {
        let renderer = Renderer::headless()?;
        let mut mesh = Mesh::default();
        quad(&mut mesh, -2.0, -1.0, 0.0, 0);
        quad(&mut mesh, 0.0, -1.0, 0.0, 1);
        quad(&mut mesh, 0.0, -1.0, -1.0, 2);
        let frame = render(&renderer, &mesh, &[], ViewMode::Overdraw)?;
        let one = pixel(&frame, at(-1.0, -0.5))?;
        let two = pixel(&frame, (SIZE / 2 + 6, at(0.0, -0.5).1))?;
        assert!(two[0] > one[0] && one[0] > 20, "one {one:?}, two {two:?}");
        assert!(frame.ids.iter().all(|id| *id == 0));
        Ok(())
    }

    /// The two quads, plus three triangles without area in front of them.
    fn with_degenerates() -> Mesh {
        let mut mesh = two_quads();
        let base = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
        for p in [
            Vec3::new(-1.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
        ] {
            mesh.positions.push(p);
            mesh.normals.push(Vec3::Z);
        }
        // Collinear, a repeated index, and three equal points.
        mesh.triangles.extend([
            [base, base + 1, base + 2],
            [base, base, base + 1],
            [base, base, base],
        ]);
        mesh.triangle_faces.extend([FaceIndex(7); 3]);
        mesh
    }

    #[test]
    fn positions_that_are_not_finite_are_rejected() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut mesh = two_quads();
            mesh.positions[3].y = bad;
            match prepare(&mesh, &[], None) {
                Err(RenderError::InvalidMesh(m)) => assert!(m.contains("vertex 3"), "{m}"),
                other => panic!(
                    "{bad}: expected InvalidMesh, got {:?}",
                    other.map(|p| p.kept)
                ),
            }
        }
    }

    #[test]
    fn triangles_without_area_are_left_out() -> TestResult {
        let prepared = prepare(&with_degenerates(), &[], None)?;
        assert_eq!(prepared.kept, [0, 1, 2, 3]);
        assert_eq!(prepared.words.len(), 4 * 3 * FACE_VERTEX_WORDS);
        Ok(())
    }

    #[test]
    fn a_thin_triangle_is_kept_and_a_flat_one_is_not() -> TestResult {
        let sliver = |height: f64| {
            let mut mesh = Mesh::default();
            for p in [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(100.0, 0.0, 0.0),
                Vec3::new(50.0, height, 0.0),
            ] {
                mesh.positions.push(p);
                mesh.normals.push(Vec3::Z);
            }
            mesh.triangles.push([0, 1, 2]);
            mesh.triangle_faces.push(FaceIndex(0));
            mesh
        };
        // A height of a millionth of the length is a sliver worth drawing.
        assert_eq!(prepare(&sliver(1e-4), &[], None)?.kept, [0]);
        // A hundred-billionth is not.
        assert!(matches!(
            prepare(&sliver(1e-9), &[], None),
            Err(RenderError::InvalidMesh(m)) if m.contains("no triangle with area")
        ));
        Ok(())
    }

    #[test]
    fn zero_and_broken_normals_are_replaced_by_the_triangle_normal() -> TestResult {
        let mut mesh = two_quads();
        mesh.normals[0] = Vec3::ZERO;
        mesh.normals[1] = Vec3::new(f64::NAN, 0.0, 0.0);
        let words = prepare(&mesh, &[], None)?.words;
        let normal = |vertex: usize| {
            let at = vertex * FACE_VERTEX_WORDS + 3;
            [0, 1, 2].map(|i| f32::from_bits(words[at + i]))
        };
        assert_eq!(normal(0), [0.0, 0.0, 1.0]);
        assert_eq!(normal(1), [0.0, 0.0, 1.0]);
        Ok(())
    }

    #[test]
    fn positions_are_stored_relative_to_the_centre_of_the_mesh() -> TestResult {
        let mut mesh = two_quads();
        let far = Vec3::new(1.0e6, -2.0e6, 3.0e5);
        for p in &mut mesh.positions {
            *p = *p + far;
        }
        let prepared = prepare(&mesh, &[], None)?;
        assert_eq!(prepared.origin, far);
        let first = [0, 1, 2].map(|i| f32::from_bits(prepared.words[i]));
        assert_eq!(first, [-2.0, -1.0, 0.0]);
        Ok(())
    }

    #[test]
    fn skipped_triangles_are_counted_and_draw_no_edges_or_pixels() -> TestResult {
        let renderer = Renderer::headless()?;
        let clean = renderer.upload(&two_quads())?;
        let scene = renderer.upload(&with_degenerates())?;
        assert_eq!(scene.triangle_count(), 4);
        assert_eq!(scene.skipped_triangles(), 3);
        assert_eq!(clean.skipped_triangles(), 0);
        assert_eq!(scene.edge_count(), clean.edge_count());
        let frame = renderer.render(&scene, &camera(), SIZE, SIZE)?;
        assert!(frame.ids.iter().all(|id| *id != 8), "face 7 was picked");
        Ok(())
    }

    #[test]
    fn reversed_depth_separates_one_millimetre_at_ten_metres_with_a_tiny_near_plane() -> TestResult
    {
        let renderer = Renderer::headless()?;
        let mut mesh = Mesh::default();
        // Face 0 is behind, drawn last; face 1 is 1 mm nearer the eye, drawn first.
        quad(&mut mesh, -1.0, -1.0, 1.0, 1);
        quad(&mut mesh, -1.0, -1.0, 0.0, 0);
        let scene = renderer.upload(&mesh)?;
        let camera = Camera {
            eye: Vec3::new(0.0, 0.0, 10_000.0),
            fov_y: Camera::DEFAULT_FOV_Y / 2000.0,
            near: 0.01,
            far: 1.0e6,
            ..camera()
        };
        let frame = renderer.render(&scene, &camera, SIZE, SIZE)?;
        assert_eq!(frame.pick(SIZE / 2, SIZE / 2), Some(FaceIndex(1)));
        Ok(())
    }

    #[test]
    fn a_part_far_from_the_origin_renders_as_it_does_at_the_origin() -> TestResult {
        let renderer = Renderer::headless()?;
        let near = renderer.render(&renderer.upload(&two_quads())?, &camera(), SIZE, SIZE)?;
        let far = Vec3::new(1.0e6, 1.0e6, 0.0);
        let mut mesh = two_quads();
        for p in &mut mesh.positions {
            *p = *p + far;
        }
        let moved = Camera {
            eye: camera().eye + far,
            target: far,
            ..camera()
        };
        let frame = renderer.render(&renderer.upload(&mesh)?, &moved, SIZE, SIZE)?;
        assert_eq!(frame.ids, near.ids);
        Ok(())
    }

    /// A 2 x 2 mm square at the origin facing +Z, split into `n x n` cells, all face `face`.
    /// `flip` picks the other diagonal and reverses the winding.
    fn square(mesh: &mut Mesh, n: u32, flip: bool, face: u32) {
        let base = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
        for j in 0..=n {
            for i in 0..=n {
                let step = 2.0 / f64::from(n);
                mesh.positions.push(Vec3::new(
                    -1.0 + f64::from(i) * step,
                    -1.0 + f64::from(j) * step,
                    0.0,
                ));
                mesh.normals.push(Vec3::Z);
            }
        }
        let row = n + 1;
        for j in 0..n {
            for i in 0..n {
                let a = base + j * row + i;
                let (b, c, d) = (a + 1, a + row + 1, a + row);
                let pair = if flip {
                    [[a, d, b], [b, d, c]]
                } else {
                    [[a, b, c], [a, c, d]]
                };
                mesh.triangles.extend(pair);
                mesh.triangle_faces.extend([FaceIndex(face); 2]);
            }
        }
    }

    #[test]
    fn plane_signs_are_canonical() {
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(plane(Vec3::Z, p), [0.0, 0.0, 1.0, 3.0]);
        assert_eq!(plane(-Vec3::Z, p), [0.0, 0.0, 1.0, 3.0]);
        assert_eq!(plane(-Vec3::Y, p), [0.0, 1.0, 0.0, 2.0]);
        assert_eq!(plane(-Vec3::X, p), [1.0, 0.0, 0.0, 1.0]);
        // The first nonzero component decides, even when a later one is negative.
        let tilted = Vec3::new(0.6, -0.8, 0.0);
        assert_eq!(plane(tilted, p), [0.6, -0.8, 0.0, -1.0]);
        let tilted = Vec3::new(0.0, 0.6, -0.8);
        assert_eq!(plane(tilted, p), [0.0, 0.6, -0.8, -1.2]);
    }

    #[test]
    fn coplanar_triangles_of_different_faces_get_identical_planes() -> TestResult {
        let mut mesh = Mesh::default();
        square(&mut mesh, 1, false, 0);
        square(&mut mesh, 3, true, 1);
        let planes = prepare(&mesh, &[], None)?.planes;
        assert_eq!(planes.len(), (2 + 18) * 4);
        for plane in planes.chunks_exact(4) {
            assert_eq!(plane, [0.0, 0.0, 1.0, 0.0]);
        }
        Ok(())
    }

    #[test]
    fn flush_faces_with_different_triangles_do_not_speckle() -> TestResult {
        let renderer = Renderer::headless()?;
        let mut mesh = Mesh::default();
        square(&mut mesh, 1, false, 0);
        square(&mut mesh, 7, true, 1);
        let scene = renderer.upload(&mesh)?;
        let camera = Camera {
            eye: Vec3::new(3.0, -4.0, 6.0),
            up: Vec3::Z,
            ..camera()
        };
        let frame = renderer.render(&scene, &camera, SIZE, SIZE)?;
        let covered: Vec<u32> = frame.ids.iter().copied().filter(|id| *id != 0).collect();
        assert!(covered.len() > 500, "{} pixels covered", covered.len());
        // Face 1 is drawn after face 0, so it wins every exact tie.
        assert!(covered.iter().all(|id| *id == 2), "face 0 showed through");
        Ok(())
    }

    #[test]
    fn data_must_fit_the_device_limit() {
        assert!(within(1 << 20, 1 << 20, "x").is_ok());
        assert!(within(0, 0, "x").is_ok());
        match within((3 << 20) + 1, 2 << 20, "the planes") {
            Err(RenderError::InvalidMesh(m)) => {
                assert_eq!(
                    m,
                    "the planes need 3 MiB; the device allows 2 MiB per buffer"
                );
            }
            other => panic!("expected InvalidMesh, got {other:?}"),
        }
    }

    /// The sRGB byte for a linear value, as the GPU writes it.
    fn srgb(linear: f64) -> u8 {
        let c = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        (c * 255.0).round() as u8
    }

    /// The linear value of an sRGB byte.
    fn linear(byte: u8) -> f64 {
        let c = f64::from(byte) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    #[test]
    fn baked_light_rides_in_the_alpha_byte_of_each_vertex() -> TestResult {
        let mesh = two_quads();
        let light: Vec<f32> = (0..mesh.positions.len())
            .map(|i| match i {
                0 => -1.0,
                1 => 2.0,
                _ => 0.5,
            })
            .collect();
        let words = prepare(&mesh, &[[10, 20, 30]], Some(&light))?.words;
        let alpha = |vertex: usize| words[vertex * FACE_VERTEX_WORDS + 7].to_le_bytes()[3];
        assert_eq!([alpha(0), alpha(1), alpha(2)], [0, 255, 128]);
        assert_eq!(words[7].to_le_bytes()[..3], [10, 20, 30]);
        let unbaked = prepare(&mesh, &[], None)?.words;
        assert_eq!(unbaked[7].to_le_bytes()[3], 255);
        Ok(())
    }

    #[test]
    fn baked_light_must_match_the_positions() {
        let result = prepare(&two_quads(), &[], Some(&[1.0; 3]));
        assert!(matches!(result, Err(RenderError::InvalidMesh(m)) if m.contains("baked light")));
    }

    #[test]
    fn lit_faces_take_baked_light_instead_of_the_light_from_the_eye() -> TestResult {
        let renderer = Renderer::headless()?;
        let mesh = two_quads();
        let light = vec![0.25; mesh.positions.len()];
        let scene = renderer.upload_baked(&mesh, &[[200, 100, 50]], &light)?;
        assert!(scene.is_baked());
        assert!(!renderer.upload(&mesh)?.is_baked());
        renderer.set_view_mode(ViewMode::Lit);
        let frame = renderer.render(&scene, &camera(), SIZE, SIZE)?;
        let expected = [200, 100, 50].map(|c| srgb(linear(c) * f64::from(64u8) / 255.0));
        let got = pixel(&frame, at(-1.0, -0.5))?;
        assert!(
            close(got, [expected[0], expected[1], expected[2], 255], 3),
            "{got:?}"
        );
        renderer.set_view_mode(ViewMode::Lighting);
        let frame = renderer.render(&scene, &camera(), SIZE, SIZE)?;
        let grey = srgb(0.85 * 64.0 / 255.0);
        assert!(close(
            pixel(&frame, at(1.0, 0.0))?,
            [grey, grey, grey, 255],
            3
        ));
        Ok(())
    }

    #[test]
    fn lighting_view_without_a_bake_shows_the_light_from_the_eye() -> TestResult {
        let renderer = Renderer::headless()?;
        let frame = render(&renderer, &two_quads(), &[], ViewMode::Lighting)?;
        // Faces square to the eye are fully lit: 0.85 of white.
        let grey = srgb(0.85);
        assert!(close(
            pixel(&frame, at(1.0, 0.0))?,
            [grey, grey, grey, 255],
            3
        ));
        Ok(())
    }

    #[test]
    fn the_wire_overlay_darkens_triangle_sides_only() -> TestResult {
        let renderer = Renderer::headless()?;
        assert!(!renderer.wire_overlay());
        let mesh = two_quads();
        let plain = render(&renderer, &mesh, &[[200, 200, 200]], ViewMode::Unlit)?;
        renderer.set_wire_overlay(true);
        assert!(renderer.wire_overlay());
        let wired = render(&renderer, &mesh, &[[200, 200, 200]], ViewMode::Unlit)?;
        renderer.set_wire_overlay(false);
        // (-1, 0) is on the left quad's diagonal; (-0.5, -0.5) is 7 px from any side.
        let side = at(-1.0, 0.0);
        assert!(pixel(&wired, side)?[0] < 120, "{:?}", pixel(&wired, side)?);
        assert!(pixel(&plain, side)?[0] > 190);
        let inside = at(-0.5, -0.5);
        assert_eq!(pixel(&wired, inside)?, pixel(&plain, inside)?);
        Ok(())
    }

    #[test]
    fn edges_can_be_turned_off_in_the_lit_view() -> TestResult {
        let renderer = Renderer::headless()?;
        assert!(renderer.edges());
        let mesh = two_quads();
        let row = at(0.0, -0.5).1;
        let darkest = |frame: &Frame| {
            (28..36)
                .filter_map(|x| frame.image.pixel(x, row))
                .map(|p| p[0])
                .min()
                .unwrap_or(0)
        };
        let with = render(&renderer, &mesh, &[[200, 100, 50]], ViewMode::Lit)?;
        renderer.set_edges(false);
        assert!(!renderer.edges());
        let without = render(&renderer, &mesh, &[[200, 100, 50]], ViewMode::Lit)?;
        renderer.set_edges(true);
        assert!(darkest(&with) < 60);
        assert!(darkest(&without) > 150, "{}", darkest(&without));
        Ok(())
    }
}
