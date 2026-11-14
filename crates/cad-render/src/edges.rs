use cad_kernel::{FaceIndex, Mesh};

/// The boundary edges of every face in a mesh, as pairs of vertex indices.
///
/// An edge is a boundary when exactly one triangle of its face uses it. Triangles are matched
/// by vertex index, so this relies on a face's triangles sharing vertices, as kernel
/// tessellations do. An edge between two faces is reported once per face.
///
/// The result is sorted, so it does not depend on triangle order.
#[must_use]
pub fn feature_edges(mesh: &Mesh) -> Vec<[u32; 2]> {
    boundary_edges(mesh.triangles.iter().zip(&mesh.triangle_faces))
}

/// [`feature_edges`] of some of a mesh's triangles, given with their faces.
pub(crate) fn boundary_edges<'a>(
    triangles: impl Iterator<Item = (&'a [u32; 3], &'a FaceIndex)>,
) -> Vec<[u32; 2]> {
    let mut sides: Vec<(u32, u32, u32)> = triangles
        .flat_map(|(&[a, b, c], face)| {
            [(a, b), (b, c), (c, a)].map(|(p, q)| (face.0, p.min(q), p.max(q)))
        })
        .collect();
    sides.sort_unstable();
    sides
        .chunk_by(|a, b| a == b)
        .filter(|run| run.len() == 1)
        .map(|run| [run[0].1, run[0].2])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cad_kernel::FaceIndex;
    use cad_math::Vec3;

    fn mesh(triangles: &[[u32; 3]], faces: &[u32]) -> Mesh {
        Mesh {
            positions: vec![Vec3::ZERO; 6],
            normals: vec![Vec3::Z; 6],
            triangles: triangles.to_vec(),
            triangle_faces: faces.iter().map(|f| FaceIndex(*f)).collect(),
        }
    }

    #[test]
    fn a_quad_has_four_edges_and_no_diagonal() {
        let quad = mesh(&[[0, 1, 2], [0, 2, 3]], &[0, 0]);
        assert_eq!(feature_edges(&quad), [[0, 1], [0, 3], [1, 2], [2, 3]]);
    }

    #[test]
    fn an_edge_between_two_faces_is_a_boundary_of_both() {
        let two = mesh(&[[0, 1, 2], [0, 2, 3]], &[0, 1]);
        let edges = feature_edges(&two);
        assert_eq!(edges.len(), 6);
        assert_eq!(edges.iter().filter(|e| **e == [0, 2]).count(), 2);
    }

    #[test]
    fn triangle_order_and_winding_do_not_matter() {
        let a = mesh(&[[0, 1, 2], [0, 2, 3]], &[0, 0]);
        let b = mesh(&[[3, 2, 0], [2, 1, 0]], &[0, 0]);
        assert_eq!(feature_edges(&a), feature_edges(&b));
    }

    #[test]
    fn an_empty_mesh_has_no_edges() {
        assert!(feature_edges(&Mesh::default()).is_empty());
    }
}
