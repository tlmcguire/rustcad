use cad_math::Vec3;

use crate::RenderError;

/// Column-major 4x4 matrix, as the GPU expects it.
pub(crate) type Mat4 = [[f32; 4]; 4];

/// A perspective camera. Lengths are millimetres, angles are radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Where the camera is.
    pub eye: Vec3,
    /// The point it looks at.
    pub target: Vec3,
    /// Which way is up on screen. Must not be parallel to the view direction.
    pub up: Vec3,
    /// Vertical field of view.
    pub fov_y: f64,
    /// Distance to the near clipping plane. Must be positive.
    pub near: f64,
    /// Distance to the far clipping plane. Must exceed `near`.
    pub far: f64,
}

impl Camera {
    /// Default vertical field of view: 35 degrees.
    pub const DEFAULT_FOV_Y: f64 = 35.0 * std::f64::consts::PI / 180.0;

    /// A camera that frames a sphere, looking at its centre from the side `toward_eye`
    /// points to. Z is up unless the view is along Z, in which case Y is up.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidCamera`] when `radius` is not positive or `toward_eye`
    /// has no direction.
    pub fn framing(center: Vec3, radius: f64, toward_eye: Vec3) -> Result<Self, RenderError> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err(RenderError::InvalidCamera(
                "framing radius must be positive",
            ));
        }
        let direction = toward_eye
            .normalized()
            .ok_or(RenderError::InvalidCamera("view direction has zero length"))?;
        let fov_y = Self::DEFAULT_FOV_Y;
        let distance = 1.1 * radius / (fov_y / 2.0).sin();
        let up = if direction.cross(Vec3::Z).normalized().is_some() {
            Vec3::Z
        } else {
            Vec3::Y
        };
        Ok(Self {
            eye: center + direction * distance,
            target: center,
            up,
            fov_y,
            near: distance - 1.05 * radius,
            far: distance + 1.05 * radius,
        })
    }

    /// View matrix and combined view-projection matrix for a target of the given aspect ratio.
    pub(crate) fn matrices(&self, aspect: f64) -> Result<(Mat4, Mat4), RenderError> {
        let bad = RenderError::InvalidCamera;
        let forward = (self.target - self.eye)
            .normalized()
            .ok_or(bad("eye and target coincide"))?;
        let side = forward
            .cross(self.up)
            .normalized()
            .ok_or(bad("up is parallel to the view"))?;
        let up = side.cross(forward);
        let half = self.fov_y / 2.0;
        let lens_ok = self.fov_y.is_finite() && half > 0.0 && half < std::f64::consts::FRAC_PI_2;
        let range_ok = self.near.is_finite() && self.far.is_finite();
        if !lens_ok {
            return Err(bad("field of view must be between 0 and 180 degrees"));
        }
        if !(range_ok && self.near > 0.0 && self.far > self.near) {
            return Err(bad("clip planes must satisfy 0 < near < far"));
        }
        if !(aspect.is_finite() && aspect > 0.0) {
            return Err(bad("aspect ratio must be positive"));
        }

        let view = [
            [side.x, up.x, -forward.x, 0.0],
            [side.y, up.y, -forward.y, 0.0],
            [side.z, up.z, -forward.z, 0.0],
            [
                -side.dot(self.eye),
                -up.dot(self.eye),
                forward.dot(self.eye),
                1.0,
            ],
        ];
        // Right-handed view space, with reversed depth: 1 at the near plane and 0 at the far
        // plane. Floats are densest near zero, so reversing spends their precision on distant
        // surfaces, where the perspective divide leaves the least of it.
        let focal = 1.0 / half.tan();
        let depth = self.near / (self.far - self.near);
        let projection = [
            [focal / aspect, 0.0, 0.0, 0.0],
            [0.0, focal, 0.0, 0.0],
            [0.0, 0.0, depth, -1.0],
            [0.0, 0.0, self.far * depth, 0.0],
        ];
        Ok((narrow(view), narrow(multiply(projection, view))))
    }
}

/// `a * b` for column-major matrices.
fn multiply(a: [[f64; 4]; 4], b: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for (col, out_col) in out.iter_mut().enumerate() {
        for (row, cell) in out_col.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][row] * b[col][k]).sum();
        }
    }
    out
}

// GPU matrices are single precision by design.
#[allow(clippy::cast_possible_truncation)]
fn narrow(m: [[f64; 4]; 4]) -> Mat4 {
    m.map(|col| col.map(|v| v as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(m: Mat4, p: [f32; 3]) -> [f32; 3] {
        let mut clip = [0.0_f32; 4];
        for (row, value) in clip.iter_mut().enumerate() {
            *value = m[0][row] * p[0] + m[1][row] * p[1] + m[2][row] * p[2] + m[3][row];
        }
        [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]]
    }

    #[test]
    fn target_projects_to_the_screen_centre_inside_the_depth_range() -> Result<(), RenderError> {
        let cam = Camera::framing(Vec3::new(20.0, 10.0, 5.0), 25.0, Vec3::new(1.0, -1.0, 1.0))?;
        let (_, view_proj) = cam.matrices(1.5)?;
        let ndc = project(view_proj, [20.0, 10.0, 5.0]);
        assert!(ndc[0].abs() < 1e-5 && ndc[1].abs() < 1e-5, "{ndc:?}");
        assert!(ndc[2] > 0.0 && ndc[2] < 1.0, "{ndc:?}");
        Ok(())
    }

    #[test]
    fn world_up_is_screen_up() -> Result<(), RenderError> {
        let cam = Camera::framing(Vec3::ZERO, 10.0, Vec3::X)?;
        let (_, view_proj) = cam.matrices(1.0)?;
        assert!(project(view_proj, [0.0, 0.0, 5.0])[1] > 0.1);
        // Looking from +X with Z up, +Y is to the right.
        assert!(project(view_proj, [0.0, 5.0, 0.0])[0] > 0.1);
        Ok(())
    }

    #[test]
    fn looking_along_z_still_has_an_up_direction() -> Result<(), RenderError> {
        let cam = Camera::framing(Vec3::ZERO, 10.0, Vec3::Z)?;
        cam.matrices(1.0).map(|_| ())
    }

    #[test]
    fn degenerate_cameras_are_rejected() -> Result<(), RenderError> {
        assert!(Camera::framing(Vec3::ZERO, 0.0, Vec3::X).is_err());
        assert!(Camera::framing(Vec3::ZERO, 1.0, Vec3::ZERO).is_err());
        let good = Camera::framing(Vec3::ZERO, 10.0, Vec3::X)?;
        assert!(
            Camera {
                target: good.eye,
                ..good
            }
            .matrices(1.0)
            .is_err()
        );
        assert!(
            Camera {
                up: Vec3::X,
                ..good
            }
            .matrices(1.0)
            .is_err()
        );
        assert!(Camera { near: 0.0, ..good }.matrices(1.0).is_err());
        assert!(
            Camera {
                far: good.near,
                ..good
            }
            .matrices(1.0)
            .is_err()
        );
        assert!(Camera { fov_y: 0.0, ..good }.matrices(1.0).is_err());
        assert!(good.matrices(0.0).is_err());
        Ok(())
    }

    #[test]
    fn depth_is_reversed_one_at_the_near_plane_and_zero_at_the_far_plane() -> Result<(), RenderError>
    {
        let cam = Camera {
            eye: Vec3::new(0.0, 0.0, 10.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov_y: Camera::DEFAULT_FOV_Y,
            near: 2.0,
            far: 50.0,
        };
        let (_, view_proj) = cam.matrices(1.0)?;
        let near = project(view_proj, [0.0, 0.0, 8.0])[2];
        let far = project(view_proj, [0.0, 0.0, -40.0])[2];
        let middle = project(view_proj, [0.0, 0.0, 0.0])[2];
        assert!((near - 1.0).abs() < 1e-5, "{near}");
        assert!(far.abs() < 1e-5, "{far}");
        assert!(middle > far && middle < near, "{middle}");
        Ok(())
    }
}
