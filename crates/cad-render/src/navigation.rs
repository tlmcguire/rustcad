use cad_math::Vec3;

use crate::Camera;

/// A viewer's changes to a base camera: orbit about the target, pan across the view, and
/// zoom toward the cursor. Keeping them apart from the base camera lets a view be reset, and
/// lets the same changes apply to any camera.
///
/// Zooming scales the clip planes with the distance to the target, so the depth precision of
/// the base camera is kept.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Navigation {
    /// Turn about the vertical axis through the target, in radians.
    pub yaw: f64,
    /// Turn up or down about the target, in radians.
    pub pitch: f64,
    /// Zoom steps: each moves the eye 10 percent closer to the target.
    pub zoom: f64,
    /// Movement of eye and target together, in world units.
    pub pan: Vec3,
}

/// Radians of orbit per pixel dragged.
const ORBIT_PER_PIXEL: f64 = 0.005;

impl Navigation {
    /// The base camera with these changes applied.
    #[must_use]
    pub fn apply(&self, base: &Camera) -> Camera {
        let scale = 0.9_f64.powf(self.zoom);
        let offset = base.eye - base.target;
        let turned = if self.yaw == 0.0 && self.pitch == 0.0 {
            Camera {
                eye: base.target + offset * scale,
                ..*base
            }
        } else {
            let r = offset.length();
            let azimuth = offset.y.atan2(offset.x) + self.yaw;
            let elevation = ((offset.z / r).asin() + self.pitch).clamp(-1.5, 1.5);
            let direction = Vec3::new(
                elevation.cos() * azimuth.cos(),
                elevation.cos() * azimuth.sin(),
                elevation.sin(),
            );
            Camera {
                eye: base.target + direction * (r * scale),
                up: Vec3::Z,
                ..*base
            }
        };
        Camera {
            eye: turned.eye + self.pan,
            target: turned.target + self.pan,
            near: base.near * scale,
            far: base.far * scale,
            ..turned
        }
    }

    /// Orbits by a cursor movement of `dx, dy` pixels.
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        self.yaw -= dx * ORBIT_PER_PIXEL;
        self.pitch += dy * ORBIT_PER_PIXEL;
    }

    /// Pans by a cursor movement of `dx, dy` pixels in a view `height` pixels tall: the
    /// point under the cursor at the target's depth follows the cursor.
    pub fn pan(&mut self, base: &Camera, dx: f64, dy: f64, height: u32) {
        let camera = self.apply(base);
        let Some(frame) = Frame::of(&camera) else {
            return;
        };
        let per_pixel =
            2.0 * frame.distance * (camera.fov_y / 2.0).tan() / f64::from(height.max(1));
        self.pan = self.pan - frame.side * (dx * per_pixel) + frame.up * (dy * per_pixel);
    }

    /// Zooms by `steps` (positive is closer) toward the cursor at pixel `cursor` of a view
    /// `size` pixels wide and tall: the point under the cursor at the target's depth stays
    /// under the cursor.
    pub fn zoom_at(&mut self, base: &Camera, steps: f64, cursor: (f64, f64), size: (u32, u32)) {
        let camera = self.apply(base);
        let Some(frame) = Frame::of(&camera) else {
            return;
        };
        let (w, h) = (f64::from(size.0.max(1)), f64::from(size.1.max(1)));
        let tan_y = (camera.fov_y / 2.0).tan();
        let nx = (2.0 * cursor.0 / w - 1.0) * tan_y * w / h;
        let ny = (1.0 - 2.0 * cursor.1 / h) * tan_y;
        // The point under the cursor on the plane through the target, square to the view.
        let under = (frame.side * nx + frame.up * ny) * frame.distance;
        let scale = 0.9_f64.powf(steps);
        self.pan = self.pan + under * (1.0 - scale);
        self.zoom += steps;
    }

    /// Back to the base camera.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// A camera's view directions and distance to its target.
struct Frame {
    side: Vec3,
    up: Vec3,
    distance: f64,
}

impl Frame {
    fn of(camera: &Camera) -> Option<Self> {
        let forward = camera.target - camera.eye;
        let distance = forward.length();
        let forward = forward.normalized()?;
        let side = forward.cross(camera.up).normalized()?;
        Some(Self {
            side,
            up: side.cross(forward),
            distance,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Camera {
        Camera {
            eye: Vec3::new(30.0, -40.0, 20.0),
            target: Vec3::new(1.0, 2.0, 3.0),
            up: Vec3::Z,
            fov_y: 0.6,
            near: 10.0,
            far: 100.0,
        }
    }

    /// Where a world point lands on screen, in pixels of a `w x h` view.
    fn screen(camera: &Camera, p: Vec3, w: f64, h: f64) -> (f64, f64) {
        let Some(frame) = Frame::of(camera) else {
            return (f64::NAN, f64::NAN);
        };
        let forward = frame.up.cross(frame.side);
        let d = p - camera.eye;
        let depth = d.dot(forward);
        let tan_y = (camera.fov_y / 2.0).tan();
        let nx = d.dot(frame.side) / depth / (tan_y * w / h);
        let ny = d.dot(frame.up) / depth / tan_y;
        ((nx + 1.0) / 2.0 * w, (1.0 - ny) / 2.0 * h)
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn no_changes_give_the_base_camera_and_reset_returns_to_it() {
        let mut nav = Navigation::default();
        assert_eq!(nav.apply(&base()), base());
        nav.orbit(40.0, -10.0);
        nav.zoom = 3.0;
        assert_ne!(nav.apply(&base()), base());
        nav.reset();
        assert_eq!(nav, Navigation::default());
    }

    #[test]
    fn orbiting_keeps_the_distance_and_target() {
        let mut nav = Navigation::default();
        nav.orbit(120.0, 35.0);
        assert!((nav.yaw + 0.6).abs() < 1e-12 && (nav.pitch - 0.175).abs() < 1e-12);
        let camera = nav.apply(&base());
        let before = (base().eye - base().target).length();
        assert!(((camera.eye - camera.target).length() - before).abs() < 1e-9);
        assert!(close(camera.target, base().target));
        assert_eq!(camera.up, Vec3::Z);
        // Pitch stops short of straight up, where the view would have no up direction.
        nav.orbit(0.0, 10_000.0);
        assert!(nav.apply(&base()).eye.z < base().target.z + before);
    }

    #[test]
    fn zooming_moves_closer_and_scales_the_clip_planes() {
        let nav = Navigation {
            zoom: 2.0,
            ..Navigation::default()
        };
        let camera = nav.apply(&base());
        let before = (base().eye - base().target).length();
        assert!(((camera.eye - camera.target).length() - before * 0.81).abs() < 1e-9);
        assert!((camera.near - 8.1).abs() < 1e-9 && (camera.far - 81.0).abs() < 1e-9);
        let turned = Navigation {
            zoom: 2.0,
            yaw: 0.3,
            ..Navigation::default()
        };
        let camera = turned.apply(&base());
        assert!(((camera.eye - camera.target).length() - before * 0.81).abs() < 1e-9);
    }

    #[test]
    fn panning_moves_eye_and_target_across_the_view() {
        let mut nav = Navigation::default();
        nav.pan(&base(), 100.0, 0.0, 600);
        let camera = nav.apply(&base());
        let shift = camera.target - base().target;
        assert!(close(camera.eye - base().eye, shift));
        let forward = (base().target - base().eye).normalized().unwrap_or(Vec3::X);
        assert!(shift.dot(forward).abs() < 1e-9);
        // The target's old position now sits 100 pixels to the right of centre.
        let (x, y) = screen(&camera, base().target, 800.0, 600.0);
        assert!(
            (x - 500.0).abs() < 1e-6 && (y - 300.0).abs() < 1e-6,
            "{x}, {y}"
        );
        // Dragging down carries the scene down with the cursor.
        let mut down = Navigation::default();
        down.pan(&base(), 0.0, 50.0, 600);
        let (_, y) = screen(&down.apply(&base()), base().target, 800.0, 600.0);
        assert!((y - 350.0).abs() < 1e-6, "{y}");
    }

    #[test]
    fn zooming_at_the_cursor_keeps_the_point_under_it() {
        let (w, h) = (800u32, 600u32);
        let cursor = (620.0, 140.0);
        let mut nav = Navigation {
            yaw: 0.2,
            ..Navigation::default()
        };
        let before = nav.apply(&base());
        // The point under the cursor on the target's plane.
        let Some(frame) = Frame::of(&before) else {
            return;
        };
        let tan_y = (before.fov_y / 2.0).tan();
        let nx = (2.0 * cursor.0 / f64::from(w) - 1.0) * tan_y * f64::from(w) / f64::from(h);
        let ny = (1.0 - 2.0 * cursor.1 / f64::from(h)) * tan_y;
        let point = before.target + (frame.side * nx + frame.up * ny) * frame.distance;
        nav.zoom_at(&base(), 3.0, cursor, (w, h));
        let after = nav.apply(&base());
        let (x, y) = screen(&after, point, f64::from(w), f64::from(h));
        assert!(
            (x - cursor.0).abs() < 1e-6 && (y - cursor.1).abs() < 1e-6,
            "{x}, {y}"
        );
        assert!((after.eye - after.target).length() < (before.eye - before.target).length());
        // At the centre, zooming does not pan.
        let mut centred = Navigation::default();
        centred.zoom_at(&base(), 2.0, (400.0, 300.0), (w, h));
        assert!(centred.pan.length() < 1e-9);
        assert_eq!(centred.zoom, 2.0);
    }

    #[test]
    fn a_degenerate_camera_is_left_alone() {
        let broken = Camera {
            target: base().eye,
            ..base()
        };
        let mut nav = Navigation::default();
        nav.pan(&broken, 10.0, 10.0, 100);
        nav.zoom_at(&broken, 1.0, (1.0, 1.0), (10, 10));
        assert_eq!(nav, Navigation::default());
    }
}
