//! Units and dimensioned quantities (`docs/PLAN.md` section 8.3, DECISIONS.md D10).
//!
//! A [`Quantity`] stores its value in internal units: millimetres for length and radians for
//! angle. Its [`Dimension`] records the powers of length and angle, so products such as area
//! are dimensions too. Adding quantities of different dimensions is an error.

use std::fmt;
use std::ops::{Div, Mul, Neg};

use crate::Tolerance;

/// The physical dimension of a quantity, as powers of length and angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Dimension {
    /// Power of length.
    pub length: i8,
    /// Power of angle.
    pub angle: i8,
}

impl Dimension {
    /// A plain number.
    pub const DIMENSIONLESS: Self = Self::new(0, 0);
    /// Length, stored in millimetres.
    pub const LENGTH: Self = Self::new(1, 0);
    /// Area, stored in square millimetres.
    pub const AREA: Self = Self::new(2, 0);
    /// Volume, stored in cubic millimetres.
    pub const VOLUME: Self = Self::new(3, 0);
    /// Angle, stored in radians.
    pub const ANGLE: Self = Self::new(0, 1);

    /// The dimension length^`length` times angle^`angle`.
    #[must_use]
    pub const fn new(length: i8, angle: i8) -> Self {
        Self { length, angle }
    }

    /// The dimension of a product. `None` if an exponent overflows.
    #[must_use]
    pub fn checked_mul(self, other: Self) -> Option<Self> {
        Some(Self::new(
            self.length.checked_add(other.length)?,
            self.angle.checked_add(other.angle)?,
        ))
    }

    /// The dimension of a quotient. `None` if an exponent overflows.
    #[must_use]
    pub fn checked_div(self, other: Self) -> Option<Self> {
        Some(Self::new(
            self.length.checked_sub(other.length)?,
            self.angle.checked_sub(other.angle)?,
        ))
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::DIMENSIONLESS => "dimensionless",
            Self::LENGTH => "length",
            Self::AREA => "area",
            Self::VOLUME => "volume",
            Self::ANGLE => "angle",
            _ => {
                let mut parts = Vec::new();
                for (base, power) in [("length", self.length), ("angle", self.angle)] {
                    match power {
                        0 => {}
                        1 => parts.push(base.to_string()),
                        p => parts.push(format!("{base}^{p}")),
                    }
                }
                return f.write_str(&parts.join(" * "));
            }
        };
        f.write_str(name)
    }
}

/// A unit that a quantity can be written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    /// Millimetre, the internal length unit.
    Mm,
    /// Centimetre.
    Cm,
    /// Metre.
    M,
    /// Inch, exactly 25.4 mm.
    In,
    /// Foot, exactly 304.8 mm.
    Ft,
    /// Radian, the internal angle unit.
    Rad,
    /// Degree.
    Deg,
}

impl Unit {
    /// Every unit, in a fixed order.
    pub const ALL: [Self; 7] = [
        Self::Mm,
        Self::Cm,
        Self::M,
        Self::In,
        Self::Ft,
        Self::Rad,
        Self::Deg,
    ];

    /// The unit's symbol, as accepted by [`Unit::parse`].
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Mm => "mm",
            Self::Cm => "cm",
            Self::M => "m",
            Self::In => "in",
            Self::Ft => "ft",
            Self::Rad => "rad",
            Self::Deg => "deg",
        }
    }

    /// The unit with this symbol. Symbols are case-sensitive.
    ///
    /// # Errors
    ///
    /// [`UnitError::UnknownUnit`] carrying the text.
    pub fn parse(symbol: &str) -> Result<Self, UnitError> {
        Self::ALL
            .into_iter()
            .find(|unit| unit.symbol() == symbol)
            .ok_or_else(|| UnitError::UnknownUnit(symbol.to_string()))
    }

    /// The dimension this unit measures.
    #[must_use]
    pub const fn dimension(self) -> Dimension {
        match self {
            Self::Mm | Self::Cm | Self::M | Self::In | Self::Ft => Dimension::LENGTH,
            Self::Rad | Self::Deg => Dimension::ANGLE,
        }
    }

    /// How many internal units (mm or rad) one of this unit is.
    #[must_use]
    pub const fn factor(self) -> f64 {
        match self {
            Self::Mm | Self::Rad => 1.0,
            Self::Cm => 10.0,
            Self::M => 1000.0,
            Self::In => 25.4,
            Self::Ft => 304.8,
            Self::Deg => std::f64::consts::PI / 180.0,
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// Why a unit operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitError {
    /// Two dimensions that had to agree did not.
    Mismatch {
        /// The dimension on the left, or the one that was expected.
        left: Dimension,
        /// The dimension on the right, or the one that was found.
        right: Dimension,
    },
    /// A unit symbol that is not one of [`Unit::ALL`].
    UnknownUnit(String),
    /// Text that does not start with a finite number.
    InvalidNumber(String),
    /// A product or quotient whose dimension exponent does not fit.
    DimensionOverflow,
}

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch { left, right } => write!(f, "unit mismatch: {left} and {right}"),
            Self::UnknownUnit(text) => write!(f, "unknown unit `{text}`"),
            Self::InvalidNumber(text) => write!(f, "not a number: `{text}`"),
            Self::DimensionOverflow => f.write_str("dimension exponent out of range"),
        }
    }
}

impl std::error::Error for UnitError {}

/// A value with a dimension, stored in internal units (mm and rad).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    /// The value in internal units.
    pub value: f64,
    /// What the value measures.
    pub dim: Dimension,
}

impl Quantity {
    /// A quantity of `value` in `unit`, converted to internal units.
    #[must_use]
    pub fn new(value: f64, unit: Unit) -> Self {
        Self {
            value: value * unit.factor(),
            dim: unit.dimension(),
        }
    }

    /// A plain number.
    #[must_use]
    pub const fn number(value: f64) -> Self {
        Self {
            value,
            dim: Dimension::DIMENSIONLESS,
        }
    }

    /// A length in millimetres.
    #[must_use]
    pub const fn mm(value: f64) -> Self {
        Self {
            value,
            dim: Dimension::LENGTH,
        }
    }

    /// An angle in radians.
    #[must_use]
    pub const fn rad(value: f64) -> Self {
        Self {
            value,
            dim: Dimension::ANGLE,
        }
    }

    /// Reads a number with an optional unit, such as `40 mm`, `40mm`, `-1.5e2 deg`, or `3`.
    /// A number with no unit is dimensionless. Whitespace around and between the parts is
    /// allowed.
    ///
    /// # Errors
    ///
    /// [`UnitError::InvalidNumber`] when the text does not start with a finite number, and
    /// [`UnitError::UnknownUnit`] with the unit text when the rest is not a unit symbol.
    pub fn parse(text: &str) -> Result<Self, UnitError> {
        let text = text.trim();
        let split = number_prefix_len(text);
        let (number, rest) = text.split_at(split);
        let value: f64 = number
            .parse()
            .ok()
            .filter(|v: &f64| v.is_finite())
            .ok_or_else(|| UnitError::InvalidNumber(text.to_string()))?;
        let rest = rest.trim_start();
        if rest.is_empty() {
            return Ok(Self::number(value));
        }
        Ok(Self::new(value, Unit::parse(rest)?))
    }

    /// The value expressed in `unit`.
    ///
    /// # Errors
    ///
    /// [`UnitError::Mismatch`] when `unit` measures a different dimension; `left` is the
    /// unit's dimension and `right` is this quantity's.
    pub fn to(self, unit: Unit) -> Result<f64, UnitError> {
        self.expect(unit.dimension())?;
        Ok(self.value / unit.factor())
    }

    /// Checks that this quantity has dimension `dim`.
    ///
    /// # Errors
    ///
    /// [`UnitError::Mismatch`] with `left` the expected dimension and `right` this one.
    pub fn expect(self, dim: Dimension) -> Result<Self, UnitError> {
        if self.dim == dim {
            Ok(self)
        } else {
            Err(UnitError::Mismatch {
                left: dim,
                right: self.dim,
            })
        }
    }

    /// The sum of two quantities of the same dimension.
    ///
    /// # Errors
    ///
    /// [`UnitError::Mismatch`] naming both dimensions when they differ.
    pub fn checked_add(self, other: Self) -> Result<Self, UnitError> {
        self.same_dim(other)?;
        Ok(Self {
            value: self.value + other.value,
            dim: self.dim,
        })
    }

    /// The difference of two quantities of the same dimension.
    ///
    /// # Errors
    ///
    /// [`UnitError::Mismatch`] naming both dimensions when they differ.
    pub fn checked_sub(self, other: Self) -> Result<Self, UnitError> {
        self.same_dim(other)?;
        Ok(Self {
            value: self.value - other.value,
            dim: self.dim,
        })
    }

    /// The product of two quantities. Dimensions multiply.
    ///
    /// # Errors
    ///
    /// [`UnitError::DimensionOverflow`] when an exponent does not fit.
    pub fn checked_mul(self, other: Self) -> Result<Self, UnitError> {
        Ok(Self {
            value: self.value * other.value,
            dim: self
                .dim
                .checked_mul(other.dim)
                .ok_or(UnitError::DimensionOverflow)?,
        })
    }

    /// The quotient of two quantities. Dimensions divide. Division by zero is left to the
    /// caller, which knows how to report it.
    ///
    /// # Errors
    ///
    /// [`UnitError::DimensionOverflow`] when an exponent does not fit.
    pub fn checked_div(self, other: Self) -> Result<Self, UnitError> {
        Ok(Self {
            value: self.value / other.value,
            dim: self
                .dim
                .checked_div(other.dim)
                .ok_or(UnitError::DimensionOverflow)?,
        })
    }

    /// True when both quantities have the same dimension and their values agree within
    /// `tol`: the angular tolerance for angles, the linear tolerance for everything else.
    #[must_use]
    pub fn approx_eq(self, other: Self, tol: Tolerance) -> bool {
        self.dim == other.dim
            && if self.dim == Dimension::ANGLE {
                tol.eq_angle(self.value, other.value)
            } else {
                tol.eq_len(self.value, other.value)
            }
    }

    fn same_dim(self, other: Self) -> Result<(), UnitError> {
        if self.dim == other.dim {
            Ok(())
        } else {
            Err(UnitError::Mismatch {
                left: self.dim,
                right: other.dim,
            })
        }
    }
}

impl Neg for Quantity {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            value: -self.value,
            dim: self.dim,
        }
    }
}

/// Scaling by a plain number keeps the dimension.
impl Mul<f64> for Quantity {
    type Output = Self;
    fn mul(self, k: f64) -> Self {
        Self {
            value: self.value * k,
            dim: self.dim,
        }
    }
}

/// Scaling by a plain number keeps the dimension.
impl Div<f64> for Quantity {
    type Output = Self;
    fn div(self, k: f64) -> Self {
        Self {
            value: self.value / k,
            dim: self.dim,
        }
    }
}

/// Length of the longest prefix of `text` shaped like a decimal number: an optional sign,
/// digits with an optional point, and an optional exponent. Letters after the digits, such
/// as a unit, are not part of it, so `1e3mm` is `1e3` then `mm`, and `2em` is `2` then `em`.
fn number_prefix_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut i = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let whole = digits(i);
    i += whole;
    let mut fraction = 0;
    if bytes.get(i) == Some(&b'.') {
        fraction = digits(i + 1);
        i += 1 + fraction;
    }
    if whole + fraction == 0 {
        return 0;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(i + 1), Some(b'+' | b'-')));
        let exponent = digits(i + 1 + sign);
        if exponent > 0 {
            i += 1 + sign + exponent;
        }
    }
    i
}
