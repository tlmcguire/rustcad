//! Golden geometry files and the harness that checks a kernel against them.
//!
//! # File format (DECISIONS.md, D11)
//!
//! One `key = value` pair per line. Blank lines and lines starting with `#` are ignored.
//! Every key below is required exactly once. Anything else is an error.
//!
//! ```text
//! volume     = 8000            # cubic millimetres
//! area       = 2800            # square millimetres
//! bbox_min   = 0 0 0           # three numbers, millimetres
//! bbox_max   = 40 20 10
//! face_count = 6
//! tolerance  = 1e-6            # linear tolerance, millimetres
//! source     = analytic: ...   # how the values were established
//! ```
//!
//! (The trailing comments above are for illustration only. A `#` is a comment only at the
//! start of a line.)
//!
//! # How values are compared
//!
//! `tolerance` is a length. A surface that is off by that length changes the volume by about
//! `tolerance * area` and the area by about `tolerance * size`, so those are the bounds used:
//!
//! * bounding box corners: each coordinate within `tolerance`
//! * volume: within `tolerance * max(area, 1)`
//! * area: within `tolerance * max(bounding box diagonal, 1)`
//! * face count: exact
//!
//! The `Kernel` trait has no bounding box query yet, so the box is taken from the vertices of
//! a tessellation made at the golden tolerance. That is exact for planar shapes and within the
//! tolerance for curved ones.

use std::fmt;
use std::path::Path;

use cad_kernel::{Kernel, KernelError, ShapeId};
use cad_math::{Tolerance, Vec3};

/// Expected properties of one shape, loaded from a golden file.
#[derive(Debug, Clone, PartialEq)]
pub struct Golden {
    /// Expected volume in cubic millimetres.
    pub volume: f64,
    /// Expected surface area in square millimetres.
    pub area: f64,
    /// Expected low corner of the bounding box.
    pub bbox_min: Vec3,
    /// Expected high corner of the bounding box.
    pub bbox_max: Vec3,
    /// Expected number of faces.
    pub face_count: usize,
    /// Linear tolerance in millimetres. See the module docs for how it is applied.
    pub tolerance: f64,
    /// How the expected values were established.
    pub source: String,
}

/// One field where a kernel result disagreed with the golden file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// The golden file key that disagreed.
    pub field: &'static str,
    /// The expected value, with its allowed deviation where one applies.
    pub expected: String,
    /// What the kernel returned.
    pub actual: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: expected {}, got {}",
            self.field, self.expected, self.actual
        )
    }
}

/// Why a golden check did not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoldenError {
    /// The golden file could not be read.
    Io {
        /// The file that was requested.
        path: String,
        /// The operating system's explanation.
        message: String,
    },
    /// A line of the golden file is malformed.
    Parse {
        /// One-based line number.
        line: usize,
        /// What is wrong with the line.
        message: String,
    },
    /// A required key is absent from the golden file.
    MissingKey(&'static str),
    /// The golden file parses but its values contradict each other or are out of range.
    Invalid(String),
    /// The kernel failed while the shape was being measured.
    Kernel(KernelError),
    /// The kernel's results differ from the golden values. Never empty.
    Mismatch(Vec<Mismatch>),
}

impl fmt::Display for GoldenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => write!(f, "cannot read golden file {path}: {message}"),
            Self::Parse { line, message } => write!(f, "golden file line {line}: {message}"),
            Self::MissingKey(key) => write!(f, "golden file is missing key `{key}`"),
            Self::Invalid(message) => write!(f, "golden file is invalid: {message}"),
            Self::Kernel(e) => write!(f, "kernel error: {e}"),
            Self::Mismatch(list) => {
                write!(f, "{} field(s) differ from the golden values:", list.len())?;
                for m in list {
                    write!(f, "\n  {m}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for GoldenError {}

impl From<KernelError> for GoldenError {
    fn from(e: KernelError) -> Self {
        Self::Kernel(e)
    }
}

/// Values collected while parsing, before all keys are known to be present.
#[derive(Default)]
struct Fields {
    volume: Option<f64>,
    area: Option<f64>,
    bbox_min: Option<Vec3>,
    bbox_max: Option<Vec3>,
    face_count: Option<usize>,
    tolerance: Option<f64>,
    source: Option<String>,
}

fn set<T>(slot: &mut Option<T>, key: &str, value: Result<T, String>) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("duplicate key `{key}`"));
    }
    *slot = Some(value?);
    Ok(())
}

fn number(text: &str) -> Result<f64, String> {
    match text.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(format!("`{text}` is not a finite number")),
    }
}

fn vector(text: &str) -> Result<Vec3, String> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    match parts.as_slice() {
        [x, y, z] => Ok(Vec3::new(number(x)?, number(y)?, number(z)?)),
        _ => Err(format!("expected three numbers, got `{text}`")),
    }
}

fn count(text: &str) -> Result<usize, String> {
    text.parse()
        .map_err(|_| format!("`{text}` is not a non-negative whole number"))
}

fn description(text: &str) -> Result<String, String> {
    if text.is_empty() {
        Err("`source` must not be empty".into())
    } else {
        Ok(text.into())
    }
}

fn show(v: Vec3) -> String {
    format!("({}, {}, {})", v.x, v.y, v.z)
}

impl Golden {
    /// Parses the text of a golden file.
    ///
    /// # Errors
    ///
    /// Returns [`GoldenError::Parse`], [`GoldenError::MissingKey`], or
    /// [`GoldenError::Invalid`] when the text does not follow the format in the module docs.
    pub fn parse(text: &str) -> Result<Self, GoldenError> {
        let mut f = Fields::default();
        for (index, raw) in text.lines().enumerate() {
            let content = raw.trim();
            if content.is_empty() || content.starts_with('#') {
                continue;
            }
            let parsed = match content.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
                None => Err("expected `key = value`".to_string()),
                Some((key, value)) => match key {
                    "volume" => set(&mut f.volume, key, number(value)),
                    "area" => set(&mut f.area, key, number(value)),
                    "bbox_min" => set(&mut f.bbox_min, key, vector(value)),
                    "bbox_max" => set(&mut f.bbox_max, key, vector(value)),
                    "face_count" => set(&mut f.face_count, key, count(value)),
                    "tolerance" => set(&mut f.tolerance, key, number(value)),
                    "source" => set(&mut f.source, key, description(value)),
                    _ => Err(format!("unknown key `{key}`")),
                },
            };
            parsed.map_err(|message| GoldenError::Parse {
                line: index + 1,
                message,
            })?;
        }

        let golden = Self {
            volume: f.volume.ok_or(GoldenError::MissingKey("volume"))?,
            area: f.area.ok_or(GoldenError::MissingKey("area"))?,
            bbox_min: f.bbox_min.ok_or(GoldenError::MissingKey("bbox_min"))?,
            bbox_max: f.bbox_max.ok_or(GoldenError::MissingKey("bbox_max"))?,
            face_count: f.face_count.ok_or(GoldenError::MissingKey("face_count"))?,
            tolerance: f.tolerance.ok_or(GoldenError::MissingKey("tolerance"))?,
            source: f.source.ok_or(GoldenError::MissingKey("source"))?,
        };
        golden.validate()?;
        Ok(golden)
    }

    /// Reads and parses a golden file.
    ///
    /// # Errors
    ///
    /// Returns [`GoldenError::Io`] when the file cannot be read, otherwise as [`Golden::parse`].
    pub fn load(path: impl AsRef<Path>) -> Result<Self, GoldenError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| GoldenError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::parse(&text)
    }

    fn validate(&self) -> Result<(), GoldenError> {
        let invalid = |message: &str| Err(GoldenError::Invalid(message.into()));
        if self.tolerance <= 0.0 {
            return invalid("`tolerance` must be positive");
        }
        if self.volume < 0.0 || self.area < 0.0 {
            return invalid("`volume` and `area` must not be negative");
        }
        let (lo, hi) = (self.bbox_min, self.bbox_max);
        if lo.x > hi.x || lo.y > hi.y || lo.z > hi.z {
            return invalid("`bbox_min` must not exceed `bbox_max` on any axis");
        }
        Ok(())
    }

    /// Measures `shape` with `kernel` and compares every field with the golden values.
    ///
    /// # Errors
    ///
    /// Returns [`GoldenError::Mismatch`] listing every field that differs, or
    /// [`GoldenError::Kernel`] when the kernel cannot measure the shape.
    pub fn check(&self, kernel: &dyn Kernel, shape: ShapeId) -> Result<(), GoldenError> {
        let within = |bound: f64| Tolerance {
            linear: bound,
            ..Tolerance::default()
        };
        let length = within(self.tolerance);

        let props = kernel.mass_props(shape)?;
        let faces = kernel.face_count(shape)?;
        let mesh = kernel.tessellate(shape, length)?;

        let mut found = Vec::new();
        let mut scalar = |field, expected: f64, actual: f64, bound: f64| {
            if !within(bound).eq_len(expected, actual) {
                found.push(Mismatch {
                    field,
                    expected: format!("{expected} +/- {bound}"),
                    actual: actual.to_string(),
                });
            }
        };
        let diagonal = (self.bbox_max - self.bbox_min).length();
        scalar(
            "volume",
            self.volume,
            props.volume,
            self.tolerance * self.area.max(1.0),
        );
        scalar(
            "area",
            self.area,
            props.area,
            self.tolerance * diagonal.max(1.0),
        );

        let bounds = bounding_box(&mesh.positions);
        let mut corner = |field, expected: Vec3, actual: Option<Vec3>| {
            let same = actual.is_some_and(|a| {
                length.eq_len(expected.x, a.x)
                    && length.eq_len(expected.y, a.y)
                    && length.eq_len(expected.z, a.z)
            });
            if !same {
                found.push(Mismatch {
                    field,
                    expected: format!("{} +/- {}", show(expected), self.tolerance),
                    actual: actual.map_or_else(|| "no vertices".to_string(), show),
                });
            }
        };
        corner("bbox_min", self.bbox_min, bounds.map(|b| b.0));
        corner("bbox_max", self.bbox_max, bounds.map(|b| b.1));

        if faces != self.face_count {
            found.push(Mismatch {
                field: "face_count",
                expected: self.face_count.to_string(),
                actual: faces.to_string(),
            });
        }

        if found.is_empty() {
            Ok(())
        } else {
            Err(GoldenError::Mismatch(found))
        }
    }
}

/// Low and high corners of the box around `points`, or `None` when there are none.
fn bounding_box(points: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let (first, rest) = points.split_first()?;
    Some(rest.iter().fold((*first, *first), |(lo, hi), p| {
        (
            Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)),
            Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)),
        )
    }))
}

/// Loads the golden file at `path` and checks `shape` against it.
///
/// The check itself is [`Golden::check`], which returns the differences as data.
///
/// # Panics
///
/// Panics when the file cannot be loaded, the kernel fails, or any field differs. The message
/// names the file, the kernel, and every mismatched field.
#[track_caller]
pub fn assert_golden(kernel: &dyn Kernel, shape: ShapeId, path: impl AsRef<Path>) {
    let path = path.as_ref();
    if let Err(e) = Golden::load(path).and_then(|golden| golden.check(kernel, shape)) {
        panic!(
            "golden check failed for {} with kernel `{}`: {e}",
            path.display(),
            kernel.name()
        );
    }
}
