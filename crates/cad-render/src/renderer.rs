use std::sync::mpsc;

use cad_kernel::{FaceIndex, Mesh};
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::image::{Frame, Image, decode_id};
use crate::{RenderError, edges::feature_edges};

/// Colour format of offscreen renders.
pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Linear colours.
const FACE_COLOR: [f32; 4] = [0.50, 0.60, 0.74, 1.0];
const EDGE_COLOR: [f32; 4] = [0.01, 0.01, 0.015, 1.0];
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.85,
    g: 0.86,
    b: 0.88,
    a: 1.0,
};

/// Words per face vertex: position, normal, face index.
const FACE_VERTEX_WORDS: usize = 7;
const FACE_VERTEX_BYTES: u64 = 28;
const EDGE_VERTEX_BYTES: u64 = 12;
/// Bytes per pixel of both the colour and the ID format.
const PIXEL_BYTES: u32 = 4;

/// Draws meshes with shaded faces and edges, and reports which face is under a pixel.
///
/// Works without a window: see [`Renderer::headless`] and [`Renderer::render`]. A windowed
/// viewport creates its own device for its surface and passes it to [`Renderer::new`].
#[derive(Debug)]
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    faces: wgpu::RenderPipeline,
    edges: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    adapter: String,
}

/// A mesh uploaded to the GPU.
#[derive(Debug)]
pub struct Scene {
    face_vertices: wgpu::Buffer,
    face_vertex_count: u32,
    edge_vertices: Option<wgpu::Buffer>,
    edge_vertex_count: u32,
}

impl Scene {
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
                        let (device, queue) = adapter
                            .request_device(&wgpu::DeviceDescriptor::default())
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

        let pipeline = |label, vertex: (&str, wgpu::VertexBufferLayout<'_>), fragment, topology| {
            let id_writes = match topology {
                wgpu::PrimitiveTopology::TriangleList => wgpu::ColorWrites::ALL,
                _ => wgpu::ColorWrites::empty(),
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
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
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[
                        Some(color_format.into()),
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
        let faces = pipeline(
            "cad-render faces",
            (
                "face_vs",
                wgpu::VertexBufferLayout {
                    array_stride: FACE_VERTEX_BYTES,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3, 1 => Float32x3, 2 => Uint32
                    ],
                },
            ),
            "face_fs",
            wgpu::PrimitiveTopology::TriangleList,
        );
        let edges = pipeline(
            "cad-render edges",
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
        );

        Self {
            device,
            queue,
            faces,
            edges,
            globals,
            bind_group,
            adapter,
        }
    }

    /// The graphics device, for callers that create their own colour targets.
    #[must_use]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The graphics adapter in use, for logs and reports.
    #[must_use]
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Uploads a mesh. Face boundaries become the drawn edges.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidMesh`] when the mesh is empty, its arrays disagree in
    /// length, or a triangle refers to a vertex that does not exist.
    pub fn upload(&self, mesh: &Mesh) -> Result<Scene, RenderError> {
        let faces = face_vertex_words(mesh)?;
        let face_vertices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cad-render face vertices"),
                contents: bytemuck::cast_slice(&faces),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let mut lines: Vec<f32> = Vec::new();
        for [a, b] in feature_edges(mesh) {
            for index in [a, b] {
                let p = position(mesh, index)?;
                lines.extend(p);
            }
        }
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
            face_vertex_count: count(faces.len(), FACE_VERTEX_WORDS)?,
            edge_vertices,
            edge_vertex_count: count(lines.len(), 3)?,
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
        let (view, view_proj) = camera.matrices(aspect)?;
        let mut globals = [0.0_f32; GLOBALS_WORDS];
        let values = view_proj
            .iter()
            .chain(&view)
            .flatten()
            .chain(&FACE_COLOR)
            .chain(&EDGE_COLOR);
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
                            load: wgpu::LoadOp::Clear(BACKGROUND),
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
                        load: wgpu::LoadOp::Clear(1.0),
                        store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_pipeline(&self.faces);
            pass.set_vertex_buffer(0, scene.face_vertices.slice(..));
            pass.draw(0..scene.face_vertex_count, 0..1);
            if let Some(edges) = &scene.edge_vertices {
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

/// Two matrices and two colours.
const GLOBALS_WORDS: usize = 40;
const GLOBALS_BYTES: u64 = 160;

fn position(mesh: &Mesh, index: u32) -> Result<[f32; 3], RenderError> {
    let p = usize::try_from(index)
        .ok()
        .and_then(|i| mesh.positions.get(i))
        .ok_or_else(|| RenderError::InvalidMesh(format!("vertex {index} does not exist")))?;
    // The GPU works in single precision.
    Ok([p.x as f32, p.y as f32, p.z as f32])
}

/// Expands a mesh to three independent vertices per triangle, seven words each:
/// position and normal as float bits, then the face index.
fn face_vertex_words(mesh: &Mesh) -> Result<Vec<u32>, RenderError> {
    let invalid = |what: &str| Err(RenderError::InvalidMesh(what.into()));
    if mesh.normals.len() != mesh.positions.len() {
        return invalid("normals and positions differ in length");
    }
    if mesh.triangle_faces.len() != mesh.triangles.len() {
        return invalid("triangle_faces and triangles differ in length");
    }
    if mesh.triangles.is_empty() {
        return invalid("mesh has no triangles");
    }
    let mut words = Vec::with_capacity(mesh.triangles.len() * 3 * FACE_VERTEX_WORDS);
    for (triangle, face) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        for &index in triangle {
            let p = position(mesh, index)?;
            // `position` succeeded, so the normal at the same index exists.
            let n = mesh
                .normals
                .get(index as usize)
                .copied()
                .unwrap_or_default();
            words.extend(p.map(f32::to_bits));
            words.extend([n.x as f32, n.y as f32, n.z as f32].map(f32::to_bits));
            words.push(face.0);
        }
    }
    Ok(words)
}
