// Viewport shader: faces in one of the view modes, a face ID output for picking, and
// flat-coloured edges.
//
// The view modes (`ViewMode` in renderer.rs) are: 0 lit, 1 unlit, 2 wireframe, 3 triangles,
// 4 faces, 5 world normal, 6 scene depth, 7 overdraw. Overdraw has its own fragment entry
// point and pipeline. The face ID output is the same in every other mode.

struct Globals {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    // x: view mode; y, z: near and far clip planes.
    params: vec4<f32>,
    edge_color: vec4<f32>,
    // Eye position, and the fraction of its distance an edge vertex moves toward it.
    eye_pull: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct FaceInput {
    @builtin(vertex_index) vertex: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) face: u32,
    // 8-bit sRGB colour of the face.
    @location(3) color: vec4<f32>,
};

struct FaceVarying {
    @builtin(position) clip: vec4<f32>,
    @location(0) view_normal: vec3<f32>,
    @location(1) @interpolate(flat) face: u32,
    // Linear colour of the face.
    @location(2) @interpolate(flat) color: vec3<f32>,
    // Distance in front of the eye.
    @location(3) distance: f32,
    @location(4) world_normal: vec3<f32>,
    // One at its own corner of the triangle, zero along the opposite side.
    @location(5) corner: vec3<f32>,
    @location(6) @interpolate(flat) triangle: u32,
};

struct Output {
    @location(0) color: vec4<f32>,
    // Face index plus one. Zero means background.
    @location(1) id: u32,
};

@vertex
fn face_vs(in: FaceInput) -> FaceVarying {
    var out: FaceVarying;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.view_normal = (globals.view * vec4<f32>(in.normal, 0.0)).xyz;
    out.world_normal = in.normal;
    out.face = in.face;
    out.color = to_linear(in.color.rgb);
    out.distance = -(globals.view * vec4<f32>(in.position, 1.0)).z;
    // Faces are drawn without an index buffer, three vertices per triangle in order.
    let k = in.vertex % 3u;
    out.corner = vec3<f32>(f32(k == 0u), f32(k == 1u), f32(k == 2u));
    out.triangle = in.vertex / 3u;
    return out;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn hash(v: u32) -> u32 {
    var x = v * 747796405u + 2891336453u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    return (x >> 22u) ^ x;
}

// A bright, distinct colour for an index: hues spaced by the golden ratio, so neighbouring
// indices never look alike.
fn index_color(index: u32) -> vec3<f32> {
    let hue = fract(f32(index) * 0.618034 + f32(hash(index) & 1023u) / 4096.0);
    let k = vec3<f32>(0.0, 2.0 / 3.0, 1.0 / 3.0);
    let rgb = clamp(abs(fract(hue + k) * 6.0 - 3.0) - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
    let value = 0.75 + 0.25 * f32(hash(index ^ 0x9e3779b9u) & 255u) / 255.0;
    return to_linear(mix(vec3<f32>(1.0), rgb, 0.8) * value);
}

// Deep blue near, through teal and green, to yellow far.
fn ramp(t: f32) -> vec3<f32> {
    let a = vec3<f32>(0.10, 0.08, 0.40);
    let b = vec3<f32>(0.05, 0.55, 0.55);
    let c = vec3<f32>(0.98, 0.90, 0.25);
    return to_linear(select(mix(b, c, t * 2.0 - 1.0), mix(a, b, t * 2.0), t < 0.5));
}

@fragment
fn face_fs(in: FaceVarying) -> Output {
    // Headlight: brightest where the surface faces the camera. `abs` keeps
    // faces with flipped normals visible instead of black.
    let n = normalize(in.view_normal);
    let light = 0.3 + 0.7 * abs(n.z);
    let mode = u32(globals.params.x + 0.5);
    var rgb = in.color * light;
    // Derivatives must be taken in uniform control flow, before the mode switch.
    let pixels = in.corner / max(fwidth(in.corner), vec3<f32>(1e-6));
    switch mode {
        case 1u: {
            rgb = in.color;
        }
        case 2u: {
            // Within about a pixel of a side of the triangle: draw the wire.
            let wire = 1.0 - smoothstep(0.0, 1.2, min(min(pixels.x, pixels.y), pixels.z));
            let fill = vec3<f32>(0.004, 0.006, 0.01) + in.color * 0.03;
            rgb = mix(fill, to_linear(vec3<f32>(0.35, 0.85, 1.0)), wire);
        }
        case 3u: {
            rgb = index_color(in.triangle) * light;
        }
        case 4u: {
            rgb = index_color(in.face) * light;
        }
        case 5u: {
            rgb = to_linear(normalize(in.world_normal) * 0.5 + 0.5);
        }
        case 6u: {
            let near = globals.params.y;
            let far = globals.params.z;
            let t = clamp(log(max(in.distance, near) / near) / log(far / near), 0.0, 1.0);
            // The stored depth value, in 64 bands.
            let band = u32(floor(in.clip.z * 64.0)) % 2u;
            rgb = ramp(t) * select(1.0, 0.5, band == 1u);
        }
        default: {}
    }
    var out: Output;
    out.color = vec4<f32>(rgb, 1.0);
    out.id = in.face + 1u;
    return out;
}

// Additive, depth test off: each covering triangle adds a little heat.
@fragment
fn overdraw_fs(in: FaceVarying) -> Output {
    var out: Output;
    out.color = vec4<f32>(0.060, 0.022, 0.006, 1.0);
    out.id = 0u; // Masked off in the pipeline.
    return out;
}

@vertex
fn edge_vs(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    // Move edges toward the eye so they win against their own faces. Moving along the ray
    // keeps the line where it was on screen and changes only its depth.
    let eye = globals.eye_pull.xyz;
    let pulled = position + (eye - position) * globals.eye_pull.w;
    return globals.view_proj * vec4<f32>(pulled, 1.0);
}

@fragment
fn edge_fs() -> Output {
    var out: Output;
    out.color = globals.edge_color;
    out.id = 0u; // Masked off in the pipeline: edges do not write IDs.
    return out;
}
