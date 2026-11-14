//! Procedural meshes and colours for the torture demos.
#![allow(dead_code)]

use std::f64::consts::TAU;

use cad_kernel::{FaceIndex, Mesh};
use cad_math::Vec3;

/// An 8-bit sRGB colour from hue, saturation, and value, each 0 to 1.
pub fn hsv(h: f64, s: f64, v: f64) -> [u8; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r, g, b].map(|t| ((t + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// Linear blend of two colours.
pub fn mix(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (f64::from(a[i]) * (1.0 - t) + f64::from(b[i]) * t).round() as u8)
}

/// A uniform number in [0, 1) from a 64-bit linear congruential generator.
pub fn unit(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

/// A standard normal number (Box-Muller).
pub fn gauss(seed: &mut u64) -> f64 {
    let u = unit(seed).max(1e-12);
    let v = unit(seed);
    (-2.0 * u.ln()).sqrt() * (TAU * v).cos()
}

/// Builds meshes the way kernel tessellations look: shared vertices within a face. Also
/// collects a colour per face.
#[derive(Default)]
pub struct Builder {
    /// The mesh so far.
    pub mesh: Mesh,
    /// Colour of face `i`, filled in as faces are named.
    pub colors: Vec<[u8; 3]>,
}

impl Builder {
    /// Gives face `face` a colour.
    pub fn color(&mut self, face: u32, color: [u8; 3]) {
        let i = face as usize;
        if self.colors.len() <= i {
            self.colors.resize(i + 1, cad_render::DEFAULT_FACE_COLOR);
        }
        self.colors[i] = color;
    }

    /// Adds another model's triangles, keeping its face numbers and colours.
    pub fn append(&mut self, (mesh, colors): (Mesh, Vec<[u8; 3]>)) {
        let base = self.mesh.positions.len() as u32;
        self.mesh.positions.extend(mesh.positions);
        self.mesh.normals.extend(mesh.normals);
        self.mesh
            .triangles
            .extend(mesh.triangles.iter().map(|t| t.map(|i| i + base)));
        self.mesh.triangle_faces.extend(mesh.triangle_faces);
        for (face, color) in (0u32..).zip(colors) {
            if color != cad_render::DEFAULT_FACE_COLOR {
                self.color(face, color);
            }
        }
    }

    /// The mesh and its face colours.
    pub fn finish(self) -> (Mesh, Vec<[u8; 3]>) {
        (self.mesh, self.colors)
    }

    pub fn vertex(&mut self, p: Vec3, n: Vec3) -> u32 {
        self.mesh.positions.push(p);
        self.mesh.normals.push(n);
        (self.mesh.positions.len() - 1) as u32
    }

    pub fn tri(&mut self, t: [u32; 3], face: u32) {
        self.mesh.triangles.push(t);
        self.mesh.triangle_faces.push(FaceIndex(face));
    }

    /// A quad from corners in order around it.
    pub fn quad(&mut self, corners: [Vec3; 4], n: Vec3, face: u32) {
        let [a, b, c, d] = corners.map(|p| self.vertex(p, n));
        self.tri([a, b, c], face);
        self.tri([a, c, d], face);
    }

    /// A parallelogram from `origin` along `u` and `v`, split into `cells` with the face of
    /// each cell given by `face(i, j)`. `flip` chooses the other diagonal.
    #[allow(clippy::too_many_arguments)]
    pub fn grid(
        &mut self,
        origin: Vec3,
        u: Vec3,
        v: Vec3,
        cells: (u32, u32),
        n: Vec3,
        flip: bool,
        face: impl Fn(u32, u32) -> u32,
    ) {
        let (nu, nv) = cells;
        let base = self.mesh.positions.len() as u32;
        for j in 0..=nv {
            for i in 0..=nu {
                let p = origin
                    + u * (f64::from(i) / f64::from(nu))
                    + v * (f64::from(j) / f64::from(nv));
                self.vertex(p, n);
            }
        }
        let row = nu + 1;
        for j in 0..nv {
            for i in 0..nu {
                let a = base + j * row + i;
                let (b, c, d) = (a + 1, a + row + 1, a + row);
                let f = face(i, j);
                if flip {
                    self.tri([a, b, d], f);
                    self.tri([b, c, d], f);
                } else {
                    self.tri([a, b, c], f);
                    self.tri([a, c, d], f);
                }
            }
        }
    }

    /// An axis-aligned box whose six sides are faces `first..first + 6` in the order -X, +X,
    /// -Y, +Y, -Z, +Z. With `one_face`, all six sides are face `first`, as a body picked as a
    /// whole.
    pub fn cube(&mut self, origin: Vec3, size: Vec3, first: u32, one_face: bool) {
        let at = |x: f64, y: f64, z: f64| origin + Vec3::new(x * size.x, y * size.y, z * size.z);
        let sides = [
            (
                [
                    at(0., 0., 0.),
                    at(0., 1., 0.),
                    at(0., 1., 1.),
                    at(0., 0., 1.),
                ],
                -Vec3::X,
            ),
            (
                [
                    at(1., 0., 0.),
                    at(1., 0., 1.),
                    at(1., 1., 1.),
                    at(1., 1., 0.),
                ],
                Vec3::X,
            ),
            (
                [
                    at(0., 0., 0.),
                    at(0., 0., 1.),
                    at(1., 0., 1.),
                    at(1., 0., 0.),
                ],
                -Vec3::Y,
            ),
            (
                [
                    at(0., 1., 0.),
                    at(1., 1., 0.),
                    at(1., 1., 1.),
                    at(0., 1., 1.),
                ],
                Vec3::Y,
            ),
            (
                [
                    at(0., 0., 0.),
                    at(1., 0., 0.),
                    at(1., 1., 0.),
                    at(0., 1., 0.),
                ],
                -Vec3::Z,
            ),
            (
                [
                    at(0., 0., 1.),
                    at(0., 1., 1.),
                    at(1., 1., 1.),
                    at(1., 0., 1.),
                ],
                Vec3::Z,
            ),
        ];
        for (k, (corners, n)) in (0u32..).zip(sides) {
            self.quad(corners, n, if one_face { first } else { first + k });
        }
    }

    /// An axis-aligned box that is one face, sharing its eight corners so it has no face
    /// boundaries and no drawn edges: a body picked as a whole, like a star in a cloud.
    pub fn solid_cube(&mut self, origin: Vec3, size: Vec3, face: u32) {
        let centre = origin + size * 0.5;
        let corners: Vec<u32> = (0..8)
            .map(|k| {
                let p = origin
                    + Vec3::new(
                        size.x * f64::from(k & 1),
                        size.y * f64::from((k >> 1) & 1),
                        size.z * f64::from((k >> 2) & 1),
                    );
                let n = (p - centre).normalized().unwrap_or(Vec3::Z);
                self.vertex(p, n)
            })
            .collect();
        let c = |k: usize| corners[k];
        for [a, b, d, e] in [
            [0, 2, 6, 4],
            [1, 5, 7, 3],
            [0, 4, 5, 1],
            [2, 3, 7, 6],
            [0, 1, 3, 2],
            [4, 6, 7, 5],
        ] {
            self.tri([c(a), c(b), c(d)], face);
            self.tri([c(a), c(d), c(e)], face);
        }
    }

    /// A box with every side one colour.
    pub fn colored_cube(&mut self, origin: Vec3, size: Vec3, first: u32, color: [u8; 3]) {
        self.cube(origin, size, first, false);
        for k in 0..6 {
            self.color(first + k, color);
        }
    }

    /// A vertical prism of `sides` flat sides around `centre` (base centre), from `z` to
    /// `z + height`. The sides are face `face`, the caps `face + 1` and `face + 2`. Many sides
    /// make a cylinder.
    pub fn prism(&mut self, centre: Vec3, radius: f64, height: f64, sides: u32, face: u32) {
        let smooth = sides > 12;
        let point = |i: u32, z: f64| {
            let a = TAU * f64::from(i) / f64::from(sides);
            centre + Vec3::new(radius * a.cos(), radius * a.sin(), z)
        };
        for i in 0..sides {
            let (a, b) = (point(i, 0.0), point(i + 1, 0.0));
            let (c, d) = (point(i + 1, height), point(i, height));
            if smooth {
                let n = |i: u32| {
                    let a = TAU * f64::from(i) / f64::from(sides);
                    Vec3::new(a.cos(), a.sin(), 0.0)
                };
                let v = [
                    self.vertex(a, n(i)),
                    self.vertex(b, n(i + 1)),
                    self.vertex(c, n(i + 1)),
                    self.vertex(d, n(i)),
                ];
                self.tri([v[0], v[1], v[2]], face);
                self.tri([v[0], v[2], v[3]], face);
            } else {
                let n = (b - a).cross(d - a).normalized().unwrap_or(Vec3::X);
                self.quad([a, b, c, d], n, face);
            }
        }
        for (z, n, f) in [(0.0, -Vec3::Z, face + 1), (height, Vec3::Z, face + 2)] {
            let hub = self.vertex(centre + Vec3::new(0.0, 0.0, z), n);
            let rim: Vec<u32> = (0..sides).map(|i| self.vertex(point(i, z), n)).collect();
            for i in 0..sides as usize {
                self.tri([hub, rim[i], rim[(i + 1) % sides as usize]], f);
            }
        }
    }
}

/// A (2, 3) torus knot tube of `along x around` segments. With `detail`, the tube has spiral
/// ridges and a finer ripple that coarse meshes cannot follow, so detail appears as the
/// triangle count rises. Faces are 16 x 4 patches, coloured along the knot.
pub fn torus_knot(along: u32, around: u32, detail: bool) -> (Mesh, Vec<[u8; 3]>) {
    let detail = if detail { 1.0 } else { 0.0 };
    let curve = |t: f64| {
        let r = 30.0 + 12.0 * (3.0 * t).cos();
        Vec3::new(
            r * (2.0 * t).cos(),
            r * (2.0 * t).sin(),
            14.0 * (3.0 * t).sin(),
        )
    };
    let surface = |i: u32, j: u32| {
        let t = TAU * f64::from(i % along) / f64::from(along);
        let v = TAU * f64::from(j % around) / f64::from(around);
        let h = 1e-4;
        let c = curve(t);
        let tangent = (curve(t + h) - curve(t - h))
            .normalized()
            .unwrap_or(Vec3::X);
        let bend = curve(t + h) + curve(t - h) - c * 2.0;
        let normal = (bend - tangent * bend.dot(tangent))
            .normalized()
            .unwrap_or(Vec3::Z);
        let binormal = tangent.cross(normal);
        let radius = 7.0
            * (1.0
                + detail * 0.10 * (12.0 * v + 120.0 * t).sin()
                + detail * 0.025 * (40.0 * v - 900.0 * t).sin());
        c + (normal * v.cos() + binormal * v.sin()) * radius
    };
    let mut b = Builder::default();
    let n = along as usize * around as usize;
    b.mesh.positions.reserve(n);
    b.mesh.normals.reserve(n);
    b.mesh.triangles.reserve(2 * n);
    b.mesh.triangle_faces.reserve(2 * n);
    for i in 0..along {
        for j in 0..around {
            // The mesh's own normal, from its neighbours.
            let du = surface(i + 1, j) - surface(i + along - 1, j);
            let dv = surface(i, j + 1) - surface(i, j + around - 1);
            // dv x du points out of the tube.
            let normal = dv.cross(du).normalized().unwrap_or(Vec3::Z);
            b.vertex(surface(i, j), normal);
        }
    }
    let at = |i: u32, j: u32| (i % along) * around + j % around;
    for i in 0..along {
        for j in 0..around {
            let face = (i * 16 / along) * 4 + j * 4 / around;
            b.tri([at(i, j), at(i + 1, j), at(i + 1, j + 1)], face);
            b.tri([at(i, j), at(i + 1, j + 1), at(i, j + 1)], face);
        }
    }
    for p in 0..16 {
        for q in 0..4 {
            let shade = if q % 2 == 0 { 0.92 } else { 0.78 };
            b.color(p * 4 + q, hsv(f64::from(p) / 16.0, 0.62, shade));
        }
    }
    b.finish()
}

/// Smooth value noise in [-1, 1] on a unit lattice.
pub fn noise(x: f64, y: f64, seed: u32) -> f64 {
    let lattice = |i: i64, j: i64| {
        let mut h = (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ (j as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
            ^ u64::from(seed).wrapping_mul(0x1656_67b1_9e37_79f9);
        h ^= h >> 31;
        h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h ^= h >> 29;
        (h >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    };
    let (fx, fy) = (x.floor(), y.floor());
    let (i, j) = (fx as i64, fy as i64);
    let (tx, ty) = (x - fx, y - fy);
    let s = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (sx, sy) = (s(tx), s(ty));
    let a = lattice(i, j) + (lattice(i + 1, j) - lattice(i, j)) * sx;
    let b = lattice(i, j + 1) + (lattice(i + 1, j + 1) - lattice(i, j + 1)) * sx;
    a + (b - a) * sy
}

/// Height of the canyon at `(x, y)` in a 200 mm square: ridged fractal mountains with a
/// river valley cut through them.
pub fn canyon_height(x: f64, y: f64) -> f64 {
    let (u, v) = (x / 200.0, y / 200.0);
    let mut height = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 3.0;
    let mut weight = 1.0;
    for octave in 0..9 {
        let n = 1.0 - noise(u * frequency, v * frequency, 7 + octave).abs();
        let ridge = n * n * weight;
        weight = (ridge * 1.5).clamp(0.0, 1.0);
        height += ridge * amplitude;
        amplitude *= 0.5;
        frequency *= 2.03;
    }
    let river = (v - 0.5 - 0.12 * (u * 9.0).sin() - 0.05 * (u * 23.0).cos()).abs();
    let valley = (river / 0.16).min(1.0);
    26.0 * height * (0.12 + 0.88 * valley * valley * (3.0 - 2.0 * valley))
}

/// The canyon as a height field of `n x n` cells, two triangles each. Each triangle's face
/// is its terrain class (beach, grass, rock, scree, snow, in two tones each by broad noise),
/// so faces follow the land rather than a grid.
pub fn canyon(n: u32) -> (Mesh, Vec<[u8; 3]>) {
    let mut b = Builder::default();
    let step = 200.0 / f64::from(n);
    let count = (n as usize + 1) * (n as usize + 1);
    b.mesh.positions.reserve(count);
    b.mesh.normals.reserve(count);
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (f64::from(i) * step - 100.0, f64::from(j) * step - 100.0);
            let h = |x: f64, y: f64| canyon_height(x + 100.0, y + 100.0);
            let e = step * 0.5;
            let normal = Vec3::new(
                h(x - e, y) - h(x + e, y),
                h(x, y - e) - h(x, y + e),
                2.0 * e,
            )
            .normalized()
            .unwrap_or(Vec3::Z);
            b.vertex(Vec3::new(x, y, h(x, y)), normal);
        }
    }
    let row = n + 1;
    let class = |b: &Builder, t: [u32; 3]| {
        let p = |k: usize| b.mesh.positions[t[k] as usize];
        let up = (0..3).map(|k| b.mesh.normals[t[k] as usize].z).sum::<f64>() / 3.0;
        let c = (p(0) + p(1) + p(2)) * (1.0 / 3.0);
        let tone = u32::from(noise(c.x * 0.04, c.y * 0.04, 41) > 0.0);
        let kind = if c.z < 2.2 {
            0 // beach
        } else if c.z > 19.0 + 3.0 * noise(c.x * 0.1, c.y * 0.1, 5) && up > 0.55 {
            4 // snow
        } else if up > 0.82 && c.z < 16.0 {
            1 // grass
        } else if up > 0.62 {
            3 // scree
        } else {
            2 // rock
        };
        kind * 2 + tone
    };
    for j in 0..n {
        for i in 0..n {
            let a = j * row + i;
            for t in [[a, a + 1, a + row + 1], [a, a + row + 1, a + row]] {
                let face = class(&b, t);
                b.tri(t, face);
            }
        }
    }
    let colors = [
        [190, 172, 124],
        [178, 160, 116],
        [86, 124, 60],
        [104, 134, 68],
        [116, 98, 84],
        [138, 120, 102],
        [150, 140, 126],
        [164, 154, 138],
        [236, 238, 242],
        [224, 228, 236],
    ];
    for (face, color) in (0u32..).zip(colors) {
        b.color(face, color);
    }
    b.finish()
}

/// A city of `blocks x blocks` buildings, each one face (a body picked as a whole), on a
/// gridded ground so baked shadows can fall on it.
pub fn city(blocks: u32, scale: f64, offset: Vec3, first_face: u32) -> (Mesh, Vec<[u8; 3]>) {
    let mut b = Builder::default();
    let pitch = 4.0;
    let span = f64::from(blocks) * pitch;
    let half = span / 2.0;
    let at = |x: f64, y: f64, z: f64| offset + Vec3::new(x, y, z) * scale;
    let ground = first_face;
    let cells = (span / 2.0).ceil() as u32;
    b.grid(
        at(-half - 4.0, -half - 4.0, 0.0),
        Vec3::new(span + 8.0, 0.0, 0.0) * scale,
        Vec3::new(0.0, span + 8.0, 0.0) * scale,
        (cells, cells),
        Vec3::Z,
        false,
        |_, _| ground,
    );
    b.color(ground, [118, 122, 116]);
    let mut seed = 0x51_7cc1_u64 ^ u64::from(blocks);
    let palette = [
        [214, 208, 196],
        [190, 184, 174],
        [168, 176, 186],
        [120, 150, 190],
        [200, 170, 140],
        [150, 160, 150],
    ];
    for j in 0..blocks {
        for i in 0..blocks {
            let (x, y) = (
                f64::from(i) * pitch - half + 0.6,
                f64::from(j) * pitch - half + 0.6,
            );
            let r = (x * x + y * y).sqrt() / half;
            let downtown = (-(r * r) * 6.0).exp();
            let wide = 1.8 + 1.0 * unit(&mut seed);
            let deep = 1.8 + 1.0 * unit(&mut seed);
            let tall = 1.0
                + 3.0 * (noise(x * 0.05, y * 0.05, 3) * 0.5 + 0.5)
                + 30.0 * downtown * unit(&mut seed).powi(2);
            let face = first_face + 1 + j * blocks + i;
            let floors = (tall / 1.5).ceil().clamp(1.0, 8.0) as u32;
            let o = at(x, y, 0.0);
            let (sx, sy, sz) = (
                Vec3::new(wide, 0.0, 0.0) * scale,
                Vec3::new(0.0, deep, 0.0) * scale,
                Vec3::new(0.0, 0.0, tall) * scale,
            );
            // Walls split by floor, so baked light can vary up the facade.
            b.grid(o, sz, sy, (floors, 1), -Vec3::X, false, |_, _| face);
            b.grid(o + sx, sy, sz, (1, floors), Vec3::X, false, |_, _| face);
            b.grid(o, sx, sz, (1, floors), -Vec3::Y, false, |_, _| face);
            b.grid(o + sy, sz, sx, (floors, 1), Vec3::Y, false, |_, _| face);
            b.grid(o + sz, sy, sx, (1, 1), Vec3::Z, false, |_, _| face);
            let pick = (unit(&mut seed) * palette.len() as f64) as usize % palette.len();
            b.color(face, palette[pick]);
        }
    }
    b.finish()
}

/// A sea urchin: a ball bristling with `spikes` hair-thin three-sided spikes, each its own
/// face. With `hidden`, that many zero-area triangles (face `spikes + 1`) lie among them.
pub fn urchin(spikes: u32, hidden: u32) -> (Mesh, Vec<[u8; 3]>) {
    let mut b = Builder::default();
    let radius = 20.0;
    // The ball, as latitude and longitude bands, face 0.
    let (rings, segments) = (48u32, 96u32);
    let base = b.mesh.positions.len() as u32;
    for j in 0..=rings {
        let theta = std::f64::consts::PI * f64::from(j) / f64::from(rings);
        for i in 0..=segments {
            let phi = TAU * f64::from(i) / f64::from(segments);
            let n = Vec3::new(
                theta.sin() * phi.cos(),
                theta.sin() * phi.sin(),
                theta.cos(),
            );
            b.vertex(n * radius, n);
        }
    }
    for j in 0..rings {
        for i in 0..segments {
            let a = base + j * (segments + 1) + i;
            let (c, d) = (a + segments + 1, a + 1);
            b.tri([a, c, c + 1], 0);
            b.tri([a, c + 1, d], 0);
        }
    }
    b.color(0, [96, 44, 70]);
    let golden = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    let mut seed = 99u64;
    for k in 0..spikes {
        let z = 1.0 - 2.0 * (f64::from(k) + 0.5) / f64::from(spikes);
        let r = (1.0 - z * z).sqrt();
        let phi = golden * f64::from(k);
        let n = Vec3::new(r * phi.cos(), r * phi.sin(), z);
        let t = n
            .cross(if n.z.abs() < 0.9 { Vec3::Z } else { Vec3::X })
            .normalized()
            .unwrap_or(Vec3::X);
        let s = n.cross(t);
        let length = 8.0 + 14.0 * unit(&mut seed);
        let thick = 0.03 + 0.05 * unit(&mut seed);
        let foot = n * (radius - 0.2);
        let tip = n * (radius + length);
        let corners: Vec<Vec3> = (0..3)
            .map(|c| {
                let a = TAU * f64::from(c) / 3.0;
                foot + (t * a.cos() + s * a.sin()) * thick
            })
            .collect();
        let face = 1 + k;
        for c in 0..3 {
            let (p, q) = (corners[c], corners[(c + 1) % 3]);
            let normal = (q - p).cross(tip - p).normalized().unwrap_or(n);
            let v = [
                b.vertex(p, normal),
                b.vertex(q, normal),
                b.vertex(tip, normal),
            ];
            b.tri(v, face);
        }
        b.color(
            face,
            hsv(
                0.08 + 0.1 * unit(&mut seed),
                0.55,
                0.65 + 0.35 * unit(&mut seed),
            ),
        );
    }
    let degenerate = 1 + spikes;
    for k in 0..hidden {
        let z = 1.0 - 2.0 * (f64::from(k) + 0.25) / f64::from(hidden);
        let r = (1.0 - z * z).sqrt();
        let phi = golden * f64::from(k) * 1.7;
        let n = Vec3::new(r * phi.cos(), r * phi.sin(), z);
        let a = b.vertex(n * (radius + 1.0), n);
        let c = b.vertex(n * (radius + 6.0), n);
        let mid = b.vertex(n * (radius + 3.5), n);
        b.tri([a, mid, c], degenerate);
    }
    if hidden > 0 {
        b.color(degenerate, [255, 0, 255]);
    }
    b.finish()
}

/// A finned heat sink: a base plate and 40 fins `t` thick. Each fin's two big sides are
/// separate faces, so the back side showing through the front is a wrong pick.
pub fn heatsink(t: f64) -> (Mesh, Vec<[u8; 3]>) {
    let mut b = Builder::default();
    let (fins, pitch, height, depth) = (40u32, 1.5, 30.0, 40.0);
    let width = f64::from(fins) * pitch + 2.0;
    b.colored_cube(
        Vec3::new(-1.0, 0.0, -4.0),
        Vec3::new(width, depth, 4.0),
        0,
        [150, 156, 166],
    );
    let quad = |b: &mut Builder, c: [Vec3; 4], n: Vec3, f: u32| {
        b.grid(c[0], c[1] - c[0], c[3] - c[0], (1, 12), n, false, |_, _| f)
    };
    for k in 0..fins {
        let x0 = f64::from(k) * pitch + 0.5;
        let x1 = x0 + t;
        let f = 6 + 3 * k;
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        // The two big sides, split along their height so baked light can vary.
        quad(
            &mut b,
            [
                p(x0, 0.0, 0.0),
                p(x0, 0.0, height),
                p(x0, depth, height),
                p(x0, depth, 0.0),
            ],
            -Vec3::X,
            f,
        );
        quad(
            &mut b,
            [
                p(x1, depth, 0.0),
                p(x1, depth, height),
                p(x1, 0.0, height),
                p(x1, 0.0, 0.0),
            ],
            Vec3::X,
            f + 1,
        );
        // The thin top and front edges.
        b.quad(
            [
                p(x0, 0.0, height),
                p(x1, 0.0, height),
                p(x1, depth, height),
                p(x0, depth, height),
            ],
            Vec3::Z,
            f + 2,
        );
        b.quad(
            [
                p(x0, 0.0, 0.0),
                p(x1, 0.0, 0.0),
                p(x1, 0.0, height),
                p(x0, 0.0, height),
            ],
            -Vec3::Y,
            f + 2,
        );
        let shade = 0.88 + 0.12 * f64::from(k % 2);
        b.color(f, mix([0, 0, 0], [196, 202, 212], shade));
        b.color(f + 1, mix([0, 0, 0], [176, 184, 196], shade));
        b.color(f + 2, [226, 230, 236]);
    }
    b.finish()
}
