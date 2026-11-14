//! CPU ray tracing: a bounding volume hierarchy over a mesh, lighting baked per vertex
//! (ray-traced sun shadows and sky ambient occlusion), and a path tracer for still images.
//!
//! None of this runs on the GPU or in real time. Baking is done once per mesh and the result
//! drawn with [`Renderer::upload_baked`](crate::Renderer::upload_baked); path tracing produces
//! a reference image to compare the real-time picture against. Work is spread over every core
//! with `std` threads, and every result is the same whatever the number of threads.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let mesh = cad_kernel::Mesh::default();
//! use cad_math::Vec3;
//! use cad_render::trace::{Sky, Tracer, bake, path_trace};
//! use cad_render::{Camera, Renderer};
//!
//! let tracer = Tracer::new(&mesh);
//! let sky = Sky { sun: Vec3::new(-0.6, -0.4, 0.7), sun_share: 0.6 };
//! let light = bake(&mesh, &tracer, sky, 16, 0.05);
//! let renderer = Renderer::headless()?;
//! let scene = renderer.upload_baked(&mesh, &[], &light)?;
//! let camera = Camera::framing(Vec3::ZERO, 50.0, Vec3::new(1.0, -1.0, 1.0))?;
//! let still = path_trace(&mesh, &[], &tracer, &camera, (800, 600), sky, 64, 2);
//! still.write_png("reference.png")?;
//! # Ok(()) }
//! ```

use std::ops::{Add, Mul, Sub};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use cad_kernel::Mesh;
use cad_math::Vec3;

use crate::{Camera, Image};

/// A single-precision vector, for speed inside the tracer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct V {
    x: f32,
    y: f32,
    z: f32,
}

impl V {
    const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn of(v: Vec3) -> Self {
        Self::new(v.x as f32, v.y as f32, v.z as f32)
    }

    fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    fn cross(self, o: Self) -> Self {
        Self::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    fn len(self) -> f32 {
        self.dot(self).sqrt()
    }

    fn unit(self) -> Self {
        let l = self.len();
        if l > 0.0 {
            self * (1.0 / l)
        } else {
            Self::new(0.0, 0.0, 1.0)
        }
    }

    fn min(self, o: Self) -> Self {
        Self::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }

    fn max(self, o: Self) -> Self {
        Self::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }

    fn axis(self, a: usize) -> f32 {
        match a {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }

    fn times(self, o: Self) -> Self {
        Self::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }
}

impl Add for V {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for V {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f32> for V {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    lo: V,
    hi: V,
    /// The first triangle of a leaf, or the right child of an inner node, whose left child
    /// is the node after it.
    index: u32,
    /// Triangles in a leaf; zero for an inner node.
    count: u32,
}

/// Leaves hold at most this many triangles.
const LEAF: usize = 4;

/// A bounding volume hierarchy over a mesh's triangles, for casting rays.
#[derive(Debug)]
pub struct Tracer {
    nodes: Vec<Node>,
    /// Corners of each triangle, in hierarchy order.
    corners: Vec<[V; 3]>,
    /// The mesh triangle each entry came from.
    source: Vec<u32>,
    size: f32,
}

/// Where a ray met a triangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// Distance along the ray, in units of its direction's length.
    pub distance: f64,
    /// The index of the triangle in the mesh.
    pub triangle: usize,
    /// Barycentric weight of the triangle's second corner.
    pub u: f64,
    /// Barycentric weight of its third corner.
    pub v: f64,
}

impl Tracer {
    /// Builds the hierarchy, splitting each node at the median of its longest axis.
    /// Triangles that refer to missing vertices are left out.
    #[must_use]
    pub fn new(mesh: &Mesh) -> Self {
        let p = |i: u32| mesh.positions.get(i as usize).copied().map(V::of);
        let mut items: Vec<(u32, [V; 3], V)> = mesh
            .triangles
            .iter()
            .enumerate()
            .filter_map(|(k, t)| {
                let c = [p(t[0])?, p(t[1])?, p(t[2])?];
                let index = u32::try_from(k).ok()?;
                Some((index, c, (c[0] + c[1] + c[2]) * (1.0 / 3.0)))
            })
            .collect();
        let mut nodes = Vec::with_capacity(items.len() / 2 + 1);
        if !items.is_empty() {
            build(&mut nodes, &mut items, 0);
        }
        let size = nodes.first().map_or(1.0, |n| (n.hi - n.lo).len()).max(1e-6);
        Self {
            nodes,
            corners: items.iter().map(|i| i.1).collect(),
            source: items.iter().map(|i| i.0).collect(),
            size,
        }
    }

    /// The length of the diagonal of the mesh's bounds.
    #[must_use]
    pub fn size(&self) -> f64 {
        f64::from(self.size)
    }

    /// The nearest triangle along a ray, closer than `max`.
    #[must_use]
    pub fn closest(&self, origin: Vec3, direction: Vec3, max: f64) -> Option<Hit> {
        #[allow(clippy::cast_possible_truncation)]
        let max = max.min(f64::from(f32::MAX)) as f32;
        self.walk(V::of(origin), V::of(direction), max, false)
            .map(|(t, triangle, u, v)| Hit {
                distance: f64::from(t),
                triangle: triangle as usize,
                u: f64::from(u),
                v: f64::from(v),
            })
    }

    /// Whether any triangle blocks a ray closer than `max`.
    #[must_use]
    pub fn blocked(&self, origin: Vec3, direction: Vec3, max: f64) -> bool {
        #[allow(clippy::cast_possible_truncation)]
        let max = max.min(f64::from(f32::MAX)) as f32;
        self.hits(V::of(origin), V::of(direction), max)
    }

    fn hits(&self, o: V, d: V, max: f32) -> bool {
        self.walk(o, d, max, true).is_some()
    }

    fn walk(&self, o: V, d: V, max: f32, any: bool) -> Option<(f32, u32, f32, f32)> {
        if self.nodes.is_empty() {
            return None;
        }
        let inverse = V::new(1.0 / d.x, 1.0 / d.y, 1.0 / d.z);
        let mut best = None;
        let mut limit = max;
        let mut stack = [0u32; 64];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let at = stack[top];
            let node = self.nodes[at as usize];
            if !slab(node.lo, node.hi, o, inverse, limit) {
                continue;
            }
            if node.count > 0 {
                for k in node.index..node.index + node.count {
                    if let Some((t, u, v)) = triangle(&self.corners[k as usize], o, d, limit) {
                        limit = t;
                        best = Some((t, self.source[k as usize], u, v));
                        if any {
                            return best;
                        }
                    }
                }
            } else if top + 2 <= stack.len() {
                stack[top] = node.index;
                stack[top + 1] = at + 1;
                top += 2;
            }
        }
        best
    }
}

fn build(nodes: &mut Vec<Node>, items: &mut [(u32, [V; 3], V)], first: u32) -> u32 {
    let mut lo = V::new(f32::MAX, f32::MAX, f32::MAX);
    let mut hi = V::new(f32::MIN, f32::MIN, f32::MIN);
    let (mut centre_lo, mut centre_hi) = (lo, hi);
    for (_, corners, centre) in items.iter() {
        for p in corners {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        centre_lo = centre_lo.min(*centre);
        centre_hi = centre_hi.max(*centre);
    }
    let me = u32::try_from(nodes.len()).unwrap_or(u32::MAX);
    let count = u32::try_from(items.len()).unwrap_or(u32::MAX);
    nodes.push(Node {
        lo,
        hi,
        index: first,
        count,
    });
    if items.len() <= LEAF {
        return me;
    }
    let extent = centre_hi - centre_lo;
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    let mid = items.len() / 2;
    items.select_nth_unstable_by(mid, |a, b| a.2.axis(axis).total_cmp(&b.2.axis(axis)));
    let (left, right) = items.split_at_mut(mid);
    build(nodes, left, first);
    let right_index = build(nodes, right, first + u32::try_from(mid).unwrap_or(u32::MAX));
    nodes[me as usize].index = right_index;
    nodes[me as usize].count = 0;
    me
}

/// Whether a ray, given by its origin and reciprocal direction, meets a box before `max`.
fn slab(lo: V, hi: V, o: V, inverse: V, max: f32) -> bool {
    let (mut t0, mut t1) = (0.0f32, max);
    for a in 0..3 {
        let (oa, ia) = (o.axis(a), inverse.axis(a));
        let mut near = (lo.axis(a) - oa) * ia;
        let mut far = (hi.axis(a) - oa) * ia;
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// Moller-Trumbore: distance and barycentric weights of a hit before `max`.
fn triangle(c: &[V; 3], o: V, d: V, max: f32) -> Option<(f32, f32, f32)> {
    let e1 = c[1] - c[0];
    let e2 = c[2] - c[0];
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inverse = 1.0 / det;
    let s = o - c[0];
    let u = s.dot(p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inverse;
    (t > 0.0 && t < max).then_some((t, u, v))
}

/// How far to start a ray off a surface at `p`: enough to clear single-precision rounding of
/// the coordinates there, so tiny parts near the origin and huge ones far out both work.
fn offset(p: V) -> f32 {
    1e-4 + 4e-6 * p.x.abs().max(p.y.abs()).max(p.z.abs())
}

/// A well-spread number in [0, 1) for sample `i` of stream `seed`.
fn random(seed: u32, i: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9e37_79b9) ^ i.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    #[allow(clippy::cast_precision_loss)]
    let unit = (x >> 8) as f32 / (1u32 << 24) as f32;
    unit
}

/// A cosine-weighted direction about the unit normal `n`, from two numbers in [0, 1).
fn hemisphere(n: V, a: f32, b: f32) -> V {
    let r = a.sqrt();
    let phi = std::f32::consts::TAU * b;
    let (x, y, z) = (r * phi.cos(), r * phi.sin(), (1.0 - a).max(0.0).sqrt());
    let t = if n.x.abs() > 0.5 {
        V::new(0.0, 1.0, 0.0)
    } else {
        V::new(1.0, 0.0, 0.0)
    };
    let u = t.cross(n).unit();
    let v = n.cross(u);
    (u * x + v * y + n * z).unit()
}

/// Runs `f(i)` for `0..n` on every core and returns the results in order.
fn par_map<T: Send + Default + Clone>(n: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let mut out = vec![T::default(); n];
    let chunk = n.div_ceil(threads * 8).max(1);
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<&mut [T]>> = out.chunks_mut(chunk).map(Mutex::new).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let c = next.fetch_add(1, Ordering::Relaxed);
                    let Some(slot) = slots.get(c) else { break };
                    let Ok(mut slice) = slot.lock() else { break };
                    for (k, item) in slice.iter_mut().enumerate() {
                        *item = f(c * chunk + k);
                    }
                }
            });
        }
    });
    drop(slots);
    out
}

/// A sun and a sky, lighting the bake and the path tracer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sky {
    /// Direction toward the sun. Need not be unit length.
    pub sun: Vec3,
    /// How much of the light comes from the sun, from 0 to 1; the rest comes from the sky.
    pub sun_share: f32,
}

impl Sky {
    fn sun(&self) -> V {
        V::of(self.sun).unit()
    }
}

/// Light at every vertex, from 0 to 1, for [`Renderer::upload_baked`](crate::Renderer::upload_baked).
///
/// Sky light is weighted by ambient occlusion: `rays` cosine-weighted rays over the vertex
/// normal's hemisphere, each blocked if it meets a triangle within `reach` times the mesh's
/// size. Sunlight is added where a shadow ray reaches the sun. A vertex with a zero normal
/// gets 0.5.
#[must_use]
pub fn bake(mesh: &Mesh, tracer: &Tracer, sky: Sky, rays: u32, reach: f32) -> Vec<f32> {
    let sun = sky.sun();
    let reach = tracer.size * reach;
    let share = sky.sun_share.clamp(0.0, 1.0);
    par_map(mesh.positions.len(), |i| {
        let n = mesh.normals.get(i).copied().map_or(V::default(), V::of);
        if n.len() < 0.5 {
            return 0.5;
        }
        let n = n.unit();
        let at = V::of(mesh.positions[i]);
        let o = at + n * offset(at);
        let seed = u32::try_from(i).unwrap_or(u32::MAX);
        let open = (0..rays)
            .filter(|&k| {
                let d = hemisphere(n, random(seed, 2 * k), random(seed, 2 * k + 1));
                !tracer.hits(o, d, reach)
            })
            .count();
        #[allow(clippy::cast_precision_loss)]
        let occlusion = open as f32 / rays.max(1) as f32;
        let facing = n.dot(sun).max(0.0);
        let lit = if facing > 0.0 && !tracer.hits(o, sun, f32::MAX) {
            facing
        } else {
            0.0
        };
        let sky_light = (1.0 - share) * (0.1 + 0.9 * occlusion);
        (sky_light + 1.5 * share * lit).min(1.0)
    })
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (s * 255.0).round() as u8;
    byte
}

/// The sky's colour in a direction: a blue gradient, brighter overhead.
fn sky_color(d: V) -> V {
    let t = d.z.clamp(-0.2, 1.0) * 0.5 + 0.5;
    V::new(0.55, 0.68, 0.95) * (0.6 + 0.6 * t)
}

/// A path-traced picture of a mesh from `camera`, `width x height` pixels.
///
/// Each pixel averages `spp` paths. A path follows up to `bounces` diffuse bounces after the
/// first hit, gathering sunlight through shadow rays at every hit and sky light when it
/// escapes. Faces take their colour from `colors` (8-bit sRGB, by face index, as for
/// [`Renderer::upload_with_colors`](crate::Renderer::upload_with_colors)), and the result is
/// tone mapped to 8-bit sRGB. The same inputs give the same picture.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn path_trace(
    mesh: &Mesh,
    colors: &[[u8; 3]],
    tracer: &Tracer,
    camera: &Camera,
    (width, height): (u32, u32),
    sky: Sky,
    spp: u32,
    bounces: u32,
) -> Image {
    let forward = (camera.target - camera.eye).normalized().unwrap_or(Vec3::Y);
    let side = forward.cross(camera.up).normalized().unwrap_or(Vec3::X);
    let up = side.cross(forward);
    let (f, s, u) = (V::of(forward), V::of(side), V::of(up));
    let eye = V::of(camera.eye);
    #[allow(clippy::cast_possible_truncation)]
    let tan_y = (camera.fov_y / 2.0).tan() as f32;
    #[allow(clippy::cast_precision_loss)]
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let aspect = w / h;
    let sun = sky.sun();
    let share = sky.sun_share.clamp(0.0, 1.0);
    let albedo: Vec<V> = colors
        .iter()
        .map(|c| {
            V::new(
                srgb_to_linear(c[0]),
                srgb_to_linear(c[1]),
                srgb_to_linear(c[2]),
            )
        })
        .collect();
    let [r, g, b] = crate::DEFAULT_FACE_COLOR;
    let default = V::new(srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b));
    let sun_light = V::new(1.0, 0.95, 0.85) * (2.4 * share);
    let sky_scale = 1.4 * (1.0 - share) + 0.4;
    let normal_at = |i: u32| {
        mesh.normals
            .get(i as usize)
            .copied()
            .map_or(V::default(), V::of)
    };
    let position_at = |i: u32| {
        mesh.positions
            .get(i as usize)
            .copied()
            .map_or(V::default(), V::of)
    };
    let samples = spp.max(1);
    let pixels = par_map(width as usize * height as usize, |k| {
        let k = u32::try_from(k).unwrap_or(u32::MAX);
        let (px, py) = (k % width.max(1), k / width.max(1));
        let mut total = V::default();
        for sample in 0..samples {
            let seed = k.wrapping_mul(9781).wrapping_add(sample.wrapping_mul(6271));
            #[allow(clippy::cast_precision_loss)]
            let (fx, fy) = (px as f32 + random(seed, 0), py as f32 + random(seed, 1));
            let nx = (2.0 * fx / w - 1.0) * tan_y * aspect;
            let ny = (1.0 - 2.0 * fy / h) * tan_y;
            let mut d = (f + s * nx + u * ny).unit();
            let mut o = eye;
            let mut through = V::new(1.0, 1.0, 1.0);
            let mut radiance = V::default();
            for bounce in 0..=bounces {
                let Some((t, tri, bu, bv)) = tracer.walk(o, d, f32::MAX, false) else {
                    let light = if bounce == 0 {
                        sky_color(d)
                    } else {
                        sky_color(d) * sky_scale
                    };
                    radiance = radiance + through.times(light);
                    break;
                };
                let corners = mesh.triangles[tri as usize];
                let face = mesh.triangle_faces[tri as usize].0 as usize;
                let corner = |i: usize| position_at(corners[i]);
                let geometric = (corner(1) - corner(0)).cross(corner(2) - corner(0)).unit();
                let shade = |i: usize| normal_at(corners[i]);
                let mut n = (shade(0) * (1.0 - bu - bv) + shade(1) * bu + shade(2) * bv).unit();
                if n.dot(d) > 0.0 {
                    n = n * -1.0;
                }
                let facing_out = if geometric.dot(d) > 0.0 {
                    geometric * -1.0
                } else {
                    geometric
                };
                let surface = o + d * t;
                let p = surface + facing_out * offset(surface);
                through = through.times(albedo.get(face).copied().unwrap_or(default));
                let facing = n.dot(sun);
                if facing > 0.0 && !tracer.hits(p, sun, f32::MAX) {
                    radiance = radiance + through.times(sun_light) * facing;
                }
                d = hemisphere(
                    n,
                    random(seed, 2 + 2 * bounce),
                    random(seed, 3 + 2 * bounce),
                );
                o = p;
            }
            total = total + radiance;
        }
        #[allow(clippy::cast_precision_loss)]
        let c = total * (1.0 / samples as f32);
        // A gentle filmic curve.
        let tone = |x: f32| x / (1.0 + x) * 1.25;
        [
            linear_to_srgb(tone(c.x)),
            linear_to_srgb(tone(c.y)),
            linear_to_srgb(tone(c.z)),
            255,
        ]
    });
    Image {
        width,
        height,
        rgba: pixels.into_iter().flatten().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cad_kernel::FaceIndex;

    /// An `n x n` grid over a 10 x 10 square at height `z`, facing +Z, face `face`.
    fn plate(mesh: &mut Mesh, n: u32, z: f64, face: u32) {
        let base = u32::try_from(mesh.positions.len()).unwrap_or(0);
        for j in 0..=n {
            for i in 0..=n {
                let step = 10.0 / f64::from(n);
                mesh.positions.push(Vec3::new(
                    -5.0 + f64::from(i) * step,
                    -5.0 + f64::from(j) * step,
                    z,
                ));
                mesh.normals.push(Vec3::Z);
            }
        }
        let row = n + 1;
        for j in 0..n {
            for i in 0..n {
                let a = base + j * row + i;
                mesh.triangles
                    .extend([[a, a + 1, a + row + 1], [a, a + row + 1, a + row]]);
                mesh.triangle_faces.extend([FaceIndex(face); 2]);
            }
        }
    }

    fn brute(mesh: &Mesh, o: V, d: V) -> Option<(f32, u32)> {
        let mut best: Option<(f32, u32)> = None;
        for (k, t) in mesh.triangles.iter().enumerate() {
            let c = t.map(|i| V::of(mesh.positions[i as usize]));
            if let Some((dist, _, _)) = triangle(&c, o, d, best.map_or(f32::MAX, |b| b.0)) {
                best = Some((dist, u32::try_from(k).unwrap_or(0)));
            }
        }
        best
    }

    #[test]
    fn a_ray_down_onto_a_plate_hits_it_at_the_right_distance() {
        let mut mesh = Mesh::default();
        plate(&mut mesh, 1, 0.0, 0);
        let tracer = Tracer::new(&mesh);
        assert!((tracer.size() - (200.0f64).sqrt()).abs() < 1e-4);
        let hit = tracer.closest(Vec3::new(1.0, 2.0, 7.0), -Vec3::Z, f64::MAX);
        let hit = hit.map(|h| (h.distance, h.u + h.v <= 1.0));
        assert_eq!(hit.map(|h| h.1), Some(true));
        assert!((hit.map_or(0.0, |h| h.0) - 7.0).abs() < 1e-5);
        assert!(tracer.blocked(Vec3::new(1.0, 2.0, 7.0), -Vec3::Z, 8.0));
        assert!(!tracer.blocked(Vec3::new(1.0, 2.0, 7.0), -Vec3::Z, 6.0));
        assert!(!tracer.blocked(Vec3::new(1.0, 2.0, 7.0), Vec3::Z, f64::MAX));
        assert!(
            tracer
                .closest(Vec3::new(9.0, 2.0, 7.0), -Vec3::Z, f64::MAX)
                .is_none()
        );
    }

    #[test]
    fn the_hierarchy_agrees_with_testing_every_triangle() {
        let mut mesh = Mesh::default();
        plate(&mut mesh, 24, 0.0, 0);
        plate(&mut mesh, 7, 2.0, 1);
        // Tilt the second plate so rays meet it at many distances.
        for p in &mut mesh.positions[625..] {
            p.z += 0.2 * p.x;
        }
        let tracer = Tracer::new(&mesh);
        for k in 0..400u32 {
            let o = V::new(random(k, 0) * 12.0 - 6.0, random(k, 1) * 12.0 - 6.0, 5.0);
            let d = V::new(random(k, 2) - 0.5, random(k, 3) - 0.5, -1.0).unit();
            let fast = tracer.walk(o, d, f32::MAX, false).map(|h| (h.0, h.1));
            assert_eq!(fast, brute(&mesh, o, d), "ray {k}");
        }
    }

    #[test]
    fn an_empty_mesh_and_missing_vertices_are_never_hit() {
        let tracer = Tracer::new(&Mesh::default());
        assert!(tracer.closest(Vec3::ZERO, Vec3::X, 10.0).is_none());
        let mut mesh = Mesh::default();
        plate(&mut mesh, 1, 0.0, 0);
        mesh.triangles[0] = [0, 1, 99];
        let tracer = Tracer::new(&mesh);
        assert_eq!(tracer.source, [1]);
    }

    #[test]
    fn random_numbers_and_hemisphere_directions_stay_in_range() {
        let n = V::new(0.3, -0.4, 0.866).unit();
        for i in 0..2000 {
            let (a, b) = (random(7, 2 * i), random(7, 2 * i + 1));
            assert!((0.0..1.0).contains(&a) && (0.0..1.0).contains(&b));
            let d = hemisphere(n, a, b);
            assert!((d.len() - 1.0).abs() < 1e-4);
            assert!(d.dot(n) >= -1e-5);
        }
        let mostly_up = hemisphere(V::new(1.0, 0.0, 0.0), 0.0, 0.0);
        assert!(mostly_up.x > 0.99);
        assert!(offset(V::new(0.0, 0.0, 0.0)) < offset(V::new(1e6, 0.0, 0.0)));
    }

    #[test]
    fn work_is_spread_over_threads_and_returned_in_order() {
        let out = par_map(10_001, |i| i * 3);
        assert_eq!(out.len(), 10_001);
        assert!(out.iter().enumerate().all(|(i, v)| *v == i * 3));
        assert!(par_map(0, |i| i).is_empty());
    }

    #[test]
    fn an_open_plate_in_the_sun_bakes_to_sky_plus_sun() {
        let mut mesh = Mesh::default();
        plate(&mut mesh, 4, 0.0, 0);
        let tracer = Tracer::new(&mesh);
        // Sun 30 degrees up: it shines at half strength on the plate.
        let sky = Sky {
            sun: Vec3::new(0.866_025, 0.0, 0.5),
            sun_share: 0.5,
        };
        let light = bake(&mesh, &tracer, sky, 8, 0.2);
        for l in &light {
            assert!((l - (0.5 + 0.75 * 0.5)).abs() < 1e-3, "{l}");
        }
        let mut flat = mesh.clone();
        flat.normals[0] = Vec3::ZERO;
        assert_eq!(bake(&flat, &tracer, sky, 8, 0.2)[0], 0.5);
    }

    #[test]
    fn a_roof_shades_the_plate_beneath_it() {
        let mut mesh = Mesh::default();
        plate(&mut mesh, 10, 0.0, 0);
        let mut roof = Mesh::default();
        plate(&mut roof, 2, 1.0, 1);
        // The roof covers only x < 0, and faces down.
        let base = u32::try_from(mesh.positions.len()).unwrap_or(0);
        for (p, n) in roof.positions.iter().zip(&roof.normals) {
            mesh.positions.push(Vec3::new(p.x * 0.5 - 2.5, p.y, p.z));
            mesh.normals.push(-*n);
        }
        mesh.triangles
            .extend(roof.triangles.iter().map(|t| t.map(|i| i + base)));
        mesh.triangle_faces.extend(roof.triangle_faces);
        let tracer = Tracer::new(&mesh);
        let sky = Sky {
            sun: Vec3::Z,
            sun_share: 0.6,
        };
        let light = bake(&mesh, &tracer, sky, 32, 0.5);
        // Under the roof (x = -3) the sun is blocked; in the open (x = 3) it is not.
        let at = |x: f64| {
            mesh.positions
                .iter()
                .position(|p| (p.x - x).abs() < 1e-9 && p.y.abs() < 1e-9 && p.z == 0.0)
                .map_or(-1.0, |i| light[i])
        };
        assert!(at(3.0) > 0.99, "{}", at(3.0));
        assert!(at(-3.0) < 0.3, "{}", at(-3.0));
    }

    #[test]
    fn a_path_traced_plate_is_lit_and_the_sky_shows_around_it() {
        let mut mesh = Mesh::default();
        plate(&mut mesh, 2, 0.0, 0);
        let tracer = Tracer::new(&mesh);
        let camera = Camera {
            eye: Vec3::new(0.0, 0.0, 12.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov_y: 1.2,
            near: 1.0,
            far: 100.0,
        };
        let sky = Sky {
            sun: Vec3::Z,
            sun_share: 0.6,
        };
        let colors = [[240, 40, 40]];
        let image = path_trace(&mesh, &colors, &tracer, &camera, (32, 24), sky, 4, 1);
        assert_eq!(
            (image.width, image.height, image.rgba.len()),
            (32, 24, 32 * 24 * 4)
        );
        let centre = image.pixel(16, 12).unwrap_or_default();
        let corner = image.pixel(0, 0).unwrap_or_default();
        assert!(centre[0] > 150 && centre[0] > centre[2] + 60, "{centre:?}");
        assert!(corner[2] > corner[0], "the sky is blue: {corner:?}");
        let again = path_trace(&mesh, &colors, &tracer, &camera, (32, 24), sky, 4, 1);
        assert_eq!(again, image);
        let grey = path_trace(&mesh, &[], &tracer, &camera, (8, 6), sky, 2, 0);
        let p = grey.pixel(4, 3).unwrap_or_default();
        assert!(p[2] > p[0], "default faces are blue-grey: {p:?}");
    }

    #[test]
    fn srgb_conversions_round_trip() {
        for c in [0u8, 1, 10, 100, 200, 255] {
            assert_eq!(linear_to_srgb(srgb_to_linear(c)), c);
        }
        assert_eq!(linear_to_srgb(-1.0), 0);
        assert_eq!(linear_to_srgb(2.0), 255);
    }
}
