use crate::Vec3;

/// Document-wide tolerances.
///
/// Lengths are in millimetres. Angles are in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// Two lengths closer than this are equal.
    pub linear: f64,
    /// Two angles closer than this are equal.
    pub angular: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            linear: 1e-6,
            angular: 1e-9,
        }
    }
}

impl Tolerance {
    /// True if two lengths are equal within the linear tolerance.
    #[must_use]
    pub fn eq_len(&self, a: f64, b: f64) -> bool {
        (a - b).abs() <= self.linear
    }

    /// True if two angles are equal within the angular tolerance.
    #[must_use]
    pub fn eq_angle(&self, a: f64, b: f64) -> bool {
        (a - b).abs() <= self.angular
    }

    /// True if two points coincide within the linear tolerance.
    #[must_use]
    pub fn eq_point(&self, a: Vec3, b: Vec3) -> bool {
        (a - b).length() <= self.linear
    }

    /// True if a length is zero within the linear tolerance.
    #[must_use]
    pub fn is_zero_len(&self, a: f64) -> bool {
        a.abs() <= self.linear
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_within_tolerance_are_equal() {
        let t = Tolerance::default();
        assert!(t.eq_len(10.0, 10.0 + 5e-7));
        assert!(!t.eq_len(10.0, 10.0 + 5e-6));
    }

    #[test]
    fn points_within_tolerance_coincide() {
        let t = Tolerance::default();
        let a = Vec3::new(1.0, 2.0, 3.0);
        assert!(t.eq_point(a, a + Vec3::new(1e-7, 0.0, 0.0)));
        assert!(!t.eq_point(a, a + Vec3::new(1e-3, 0.0, 0.0)));
    }
}
