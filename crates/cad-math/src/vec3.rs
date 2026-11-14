use std::ops::{Add, Mul, Neg, Sub};

/// A 3D vector or point in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    /// Unit vector along X.
    pub const X: Self = Self::new(1.0, 0.0, 0.0);
    /// Unit vector along Y.
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);
    /// Unit vector along Z.
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);

    /// Creates a vector from components.
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Dot product.
    #[must_use]
    pub fn dot(self, o: Self) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    /// Cross product.
    #[must_use]
    pub fn cross(self, o: Self) -> Self {
        Self::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    /// Euclidean length.
    #[must_use]
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// Unit vector in the same direction, or `None` for a zero-length vector.
    #[must_use]
    pub fn normalized(self) -> Option<Self> {
        let len = self.length();
        (len > f64::EPSILON).then(|| self * (1.0 / len))
    }

    /// Euclidean distance between two points.
    #[must_use]
    pub fn distance(self, other: Self) -> f64 {
        (other - self).length()
    }

    /// The point halfway between two points.
    #[must_use]
    pub fn midpoint(self, other: Self) -> Self {
        (self + other) * 0.5
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_of_x_and_y_is_z() {
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
    }

    #[test]
    fn length_of_3_4_0_is_5() {
        assert!((Vec3::new(3.0, 4.0, 0.0).length() - 5.0).abs() < 1e-12);
    }

    #[test]
    fn zero_vector_has_no_direction() {
        assert!(Vec3::ZERO.normalized().is_none());
    }
}
