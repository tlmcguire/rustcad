//! Meshes shared by the examples.
#![allow(dead_code)]

use cad_kernel::{FaceIndex, Mesh};
use cad_math::Vec3;

/// A 40 x 20 x 10 box: four vertices and two triangles per face.
pub fn box_mesh() -> Mesh {
    let size = Vec3::new(40.0, 20.0, 10.0);
    let mut mesh = Mesh::default();
    for (face, axis) in (0u32..).zip([0usize, 0, 1, 1, 2, 2]) {
        let high = face % 2 == 1;
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut normal = [0.0; 3];
        normal[axis] = if high { 1.0 } else { -1.0 };
        let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        for (s, t) in corners {
            let mut p = [0.0; 3];
            p[axis] = if high { 1.0 } else { 0.0 };
            p[u] = s;
            p[v] = t;
            mesh.positions
                .push(Vec3::new(p[0] * size.x, p[1] * size.y, p[2] * size.z));
            mesh.normals
                .push(Vec3::new(normal[0], normal[1], normal[2]));
        }
        let base = face * 4;
        mesh.triangles
            .extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
        mesh.triangle_faces.extend([FaceIndex(face); 2]);
    }
    mesh
}

/// A wavy sheet of `2 * cells * cells` triangles with shared vertices, split into square
/// patches of 32 x 32 cells that stand in for faces.
pub fn grid_mesh(cells: u32) -> Mesh {
    let mut mesh = Mesh::default();
    let step = 200.0 / f64::from(cells);
    for j in 0..=cells {
        for i in 0..=cells {
            let (x, y) = (f64::from(i) * step - 100.0, f64::from(j) * step - 100.0);
            let (kx, ky) = (x * 0.08, y * 0.08);
            mesh.positions
                .push(Vec3::new(x, y, 6.0 * kx.sin() * ky.cos()));
            let slope = Vec3::new(-0.48 * kx.cos() * ky.cos(), 0.48 * kx.sin() * ky.sin(), 1.0);
            mesh.normals.push(slope.normalized().unwrap_or(Vec3::Z));
        }
    }
    let row = cells + 1;
    for j in 0..cells {
        for i in 0..cells {
            let a = j * row + i;
            let face = FaceIndex((j / 32) * cells.div_ceil(32) + i / 32);
            mesh.triangles
                .extend([[a, a + 1, a + row + 1], [a, a + row + 1, a + row]]);
            mesh.triangle_faces.extend([face; 2]);
        }
    }
    mesh
}
