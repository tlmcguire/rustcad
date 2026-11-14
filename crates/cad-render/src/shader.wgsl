// Viewport shader: shaded faces with a face ID output, and flat-coloured edges.

struct Globals {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    face_color: vec4<f32>,
    edge_color: vec4<f32>,
    // Eye position, and the fraction of its distance an edge vertex moves toward it.
    eye_pull: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct FaceInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) face: u32,
};

struct FaceVarying {
    @builtin(position) clip: vec4<f32>,
    @location(0) view_normal: vec3<f32>,
    @location(1) @interpolate(flat) face: u32,
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
    out.face = in.face;
    return out;
}

@fragment
fn face_fs(in: FaceVarying) -> Output {
    // Headlight: brightest where the surface faces the camera. `abs` keeps
    // faces with flipped normals visible instead of black.
    let n = normalize(in.view_normal);
    let light = 0.3 + 0.7 * abs(n.z);
    var out: Output;
    out.color = vec4<f32>(globals.face_color.rgb * light, 1.0);
    out.id = in.face + 1u;
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
