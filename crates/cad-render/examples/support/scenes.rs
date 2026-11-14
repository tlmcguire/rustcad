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
