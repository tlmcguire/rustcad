//! Reference images and the harness that checks a rendered image against them.
//!
//! A snapshot is two files with the same stem:
//!
//! * `name.png`: the reference image, 8-bit RGBA.
//! * `name.snapshot`: how closely a render must match it, in the `key = value` format of
//!   golden files (DECISIONS.md, D11). Every key is required exactly once.
//!
//! ```text
//! channel_tolerance    = 8      # largest difference in one colour channel that is ignored
//! max_differing_pixels = 384    # pixels allowed to exceed it
//! source               = ...    # how the reference image was established
//! ```
//!
//! (The trailing comments above are for illustration only. A `#` is a comment only at the
//! start of a line.)
//!
//! # How images are compared
//!
//! A pixel differs when any of its four channels is further than `channel_tolerance` from
//! the reference. The check passes when at most `max_differing_pixels` pixels differ. Images
//! of different sizes never match.
//!
//! The two limits absorb different things. Graphics adapters round shading slightly
//! differently, which moves many pixels by a level or two: that is `channel_tolerance`. They
//! also disagree on which pixels a thin edge covers, which changes a few pixels completely:
//! that is `max_differing_pixels`.
//!
//! # Changing a reference image
//!
//! A failing check never rewrites the reference. Run the test with `CAD_SNAPSHOT_UPDATE=1`
//! to replace the PNG with the current render, look at it, and record why in `source`.

use std::fmt;
use std::path::{Path, PathBuf};

use cad_render::{Image, RenderError};

/// Environment variable that makes [`assert_snapshot`] rewrite the reference image.
pub const UPDATE_VARIABLE: &str = "CAD_SNAPSHOT_UPDATE";

/// How closely a render must match a reference image, loaded from a `.snapshot` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotSpec {
    /// The largest difference in one colour channel that still counts as the same pixel.
    pub channel_tolerance: u8,
    /// How many pixels may differ by more than that.
    pub max_differing_pixels: usize,
    /// How the reference image was established.
    pub source: String,
}

/// A reference image with its thresholds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The thresholds from the `.snapshot` file.
    pub spec: SnapshotSpec,
    /// The reference image from the `.png` file.
    pub reference: Image,
}

/// How far an image is from a reference of the same size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Difference {
    /// Pixels where some channel is further from the reference than the tolerance.
    pub differing_pixels: usize,
    /// The largest difference in any channel of any pixel.
    pub max_channel_delta: u8,
}

/// Why a snapshot check did not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// A snapshot file could not be read or written.
    Io {
        /// The file that was requested.
        path: String,
        /// The operating system's explanation.
        message: String,
    },
    /// A line of the `.snapshot` file is malformed.
    Parse {
        /// One-based line number.
        line: usize,
        /// What is wrong with the line.
        message: String,
    },
    /// A required key is absent from the `.snapshot` file.
    MissingKey(&'static str),
    /// A PNG file could not be decoded or encoded.
    Image {
        /// The file involved.
        path: String,
        /// What the image code reported.
        error: RenderError,
    },
    /// An image's pixel data does not match its dimensions.
    InvalidImage(String),
    /// The image and the reference have different sizes.
    SizeMismatch {
        /// Width and height of the reference.
        expected: (u32, u32),
        /// Width and height of the image that was checked.
        actual: (u32, u32),
    },
    /// More pixels differ from the reference than the snapshot allows.
    Differs {
        /// What was measured.
        difference: Difference,
        /// The snapshot's `max_differing_pixels`.
        allowed: usize,
        /// The snapshot's `channel_tolerance`.
        channel_tolerance: u8,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => write!(f, "snapshot file {path}: {message}"),
            Self::Parse { line, message } => write!(f, "snapshot file line {line}: {message}"),
            Self::MissingKey(key) => write!(f, "snapshot file is missing key `{key}`"),
            Self::Image { path, error } => write!(f, "snapshot image {path}: {error}"),
            Self::InvalidImage(message) => write!(f, "invalid image: {message}"),
            Self::SizeMismatch { expected, actual } => write!(
                f,
                "image is {} x {}, the reference is {} x {}",
                actual.0, actual.1, expected.0, expected.1
            ),
            Self::Differs {
                difference,
                allowed,
                channel_tolerance,
            } => write!(
                f,
                "{} pixel(s) differ from the reference by more than {channel_tolerance} in a \
                 channel, {allowed} allowed; the largest channel difference is {}",
                difference.differing_pixels, difference.max_channel_delta
            ),
        }
    }
}

impl std::error::Error for SnapshotError {}

fn set<T>(slot: &mut Option<T>, key: &str, value: Result<T, String>) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("duplicate key `{key}`"));
    }
    *slot = Some(value?);
    Ok(())
}

impl SnapshotSpec {
    /// Parses the text of a `.snapshot` file.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Parse`] or [`SnapshotError::MissingKey`] when the text does
    /// not follow the format in the module docs.
    pub fn parse(text: &str) -> Result<Self, SnapshotError> {
        let (mut tolerance, mut pixels, mut source) = (None, None, None);
        for (index, raw) in text.lines().enumerate() {
            let content = raw.trim();
            if content.is_empty() || content.starts_with('#') {
                continue;
            }
            let parsed = match content.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
                None => Err("expected `key = value`".to_string()),
                Some((key, value)) => match key {
                    "channel_tolerance" => set(
                        &mut tolerance,
                        key,
                        value
                            .parse::<u8>()
                            .map_err(|_| format!("`{value}` is not a whole number from 0 to 255")),
                    ),
                    "max_differing_pixels" => set(
                        &mut pixels,
                        key,
                        value
                            .parse::<usize>()
                            .map_err(|_| format!("`{value}` is not a non-negative whole number")),
                    ),
                    "source" => set(
                        &mut source,
                        key,
                        if value.is_empty() {
                            Err("`source` must not be empty".to_string())
                        } else {
                            Ok(value.to_string())
                        },
                    ),
                    _ => Err(format!("unknown key `{key}`")),
                },
            };
            parsed.map_err(|message| SnapshotError::Parse {
                line: index + 1,
                message,
            })?;
        }
        Ok(Self {
            channel_tolerance: tolerance.ok_or(SnapshotError::MissingKey("channel_tolerance"))?,
            max_differing_pixels: pixels
                .ok_or(SnapshotError::MissingKey("max_differing_pixels"))?,
            source: source.ok_or(SnapshotError::MissingKey("source"))?,
        })
    }

    /// Reads and parses a `.snapshot` file.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Io`] when the file cannot be read, otherwise as
    /// [`SnapshotSpec::parse`].
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SnapshotError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| io_error(path, &e))?;
        Self::parse(&text)
    }
}

impl Snapshot {
    /// Loads the `.snapshot` file at `spec_path` and the reference image beside it.
    ///
    /// # Errors
    ///
    /// As [`SnapshotSpec::load`], or [`SnapshotError::Image`] when the reference image is
    /// missing or is not an 8-bit RGBA PNG.
    pub fn load(spec_path: impl AsRef<Path>) -> Result<Self, SnapshotError> {
        let spec = SnapshotSpec::load(&spec_path)?;
        let image = image_path(&spec_path);
        let reference = Image::read_png(&image).map_err(|error| image_error(&image, error))?;
        Ok(Self { spec, reference })
    }

    /// Compares `actual` with the reference image.
    ///
    /// Returns the measured difference when it is within the snapshot's limits.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Differs`] when too many pixels differ,
    /// [`SnapshotError::SizeMismatch`] when the sizes differ, or
    /// [`SnapshotError::InvalidImage`] when either image is malformed.
    pub fn check(&self, actual: &Image) -> Result<Difference, SnapshotError> {
        let difference = compare(actual, &self.reference, self.spec.channel_tolerance)?;
        if difference.differing_pixels > self.spec.max_differing_pixels {
            return Err(SnapshotError::Differs {
                difference,
                allowed: self.spec.max_differing_pixels,
                channel_tolerance: self.spec.channel_tolerance,
            });
        }
        Ok(difference)
    }
}

/// The reference image that belongs to the `.snapshot` file at `spec_path`.
#[must_use]
pub fn image_path(spec_path: impl AsRef<Path>) -> PathBuf {
    spec_path.as_ref().with_extension("png")
}

/// Measures how far `actual` is from `reference`.
///
/// # Errors
///
/// Returns [`SnapshotError::SizeMismatch`] when the sizes differ, or
/// [`SnapshotError::InvalidImage`] when either image's data does not match its dimensions.
pub fn compare(
    actual: &Image,
    reference: &Image,
    channel_tolerance: u8,
) -> Result<Difference, SnapshotError> {
    let mut difference = Difference {
        differing_pixels: 0,
        max_channel_delta: 0,
    };
    for delta in pixel_deltas(actual, reference)? {
        difference.max_channel_delta = difference.max_channel_delta.max(delta);
        difference.differing_pixels += usize::from(delta > channel_tolerance);
    }
    Ok(difference)
}

/// A picture of where `actual` differs from `reference`: differing pixels in red over a
/// faded copy of the reference.
///
/// # Errors
///
/// As [`compare`].
pub fn difference_image(
    actual: &Image,
    reference: &Image,
    channel_tolerance: u8,
) -> Result<Image, SnapshotError> {
    let deltas = pixel_deltas(actual, reference)?;
    let mut rgba = Vec::with_capacity(reference.rgba.len());
    for (delta, pixel) in deltas.zip(reference.rgba.as_chunks::<4>().0) {
        if delta > channel_tolerance {
            rgba.extend([255, 0, 0, 255]);
        } else {
            // Three quarters of the way to white.
            rgba.extend(pixel.iter().take(3).map(|c| 191 + c / 4));
            rgba.push(255);
        }
    }
    Ok(Image {
        width: reference.width,
        height: reference.height,
        rgba,
    })
}

/// Replaces the reference image of the snapshot at `spec_path` with `image`.
///
/// The `.snapshot` file must already exist and parse: thresholds and `source` are written by
/// hand, not by this function.
///
/// # Errors
///
/// As [`SnapshotSpec::load`], or [`SnapshotError::Image`] when the image cannot be written.
pub fn update(image: &Image, spec_path: impl AsRef<Path>) -> Result<(), SnapshotError> {
    SnapshotSpec::load(&spec_path)?;
    checked_len(image)?;
    let path = image_path(&spec_path);
    image
        .write_png(&path)
        .map_err(|error| image_error(&path, error))
}

/// Checks `actual` against the snapshot at `spec_path`.
///
/// The check itself is [`Snapshot::check`], which returns the difference as data. When
/// [`UPDATE_VARIABLE`] is set to `1`, the reference image is replaced with `actual` instead
/// and nothing is compared.
///
/// # Panics
///
/// Panics when the snapshot cannot be loaded or the image does not match. The message names
/// the snapshot file and the counts. On a mismatch the image and a difference image are
/// written to [`failure_dir`] and named in the message.
#[track_caller]
pub fn assert_snapshot(actual: &Image, spec_path: impl AsRef<Path>) {
    let spec_path = spec_path.as_ref();
    if std::env::var_os(UPDATE_VARIABLE).is_some_and(|value| value == "1") {
        if let Err(e) = update(actual, spec_path) {
            panic!("cannot update snapshot {}: {e}", spec_path.display());
        }
        return;
    }
    let snapshot = match Snapshot::load(spec_path) {
        Ok(snapshot) => snapshot,
        Err(e) => panic!("cannot load snapshot {}: {e}", spec_path.display()),
    };
    if let Err(e) = snapshot.check(actual) {
        let saved = save_failure(actual, &snapshot, spec_path);
        panic!(
            "snapshot check failed for {}: {e}\n  {saved}",
            spec_path.display()
        );
    }
}

/// Where [`assert_snapshot`] writes the images of a failed check.
#[must_use]
pub fn failure_dir() -> PathBuf {
    std::env::temp_dir().join("cad-snapshots")
}

/// Writes the checked image and, when the sizes match, a difference image. Returns a note
/// for the failure message.
fn save_failure(actual: &Image, snapshot: &Snapshot, spec_path: &Path) -> String {
    let dir = failure_dir();
    let stem = spec_path
        .file_stem()
        .map_or_else(|| "snapshot".into(), |s| s.to_string_lossy().into_owned());
    let actual_path = dir.join(format!("{stem}.actual.png"));
    let diff_path = dir.join(format!("{stem}.diff.png"));
    let written = std::fs::create_dir_all(&dir)
        .map_err(|e| e.to_string())
        .and_then(|()| actual.write_png(&actual_path).map_err(|e| e.to_string()));
    if let Err(message) = written {
        return format!("the image could not be saved: {message}");
    }
    let diff = difference_image(actual, &snapshot.reference, snapshot.spec.channel_tolerance)
        .map_err(|e| e.to_string())
        .and_then(|image| image.write_png(&diff_path).map_err(|e| e.to_string()));
    match diff {
        Ok(()) => format!(
            "image saved to {}, difference to {}",
            actual_path.display(),
            diff_path.display()
        ),
        Err(_) => format!("image saved to {}", actual_path.display()),
    }
}

/// The largest channel difference of each pixel, in row order.
fn pixel_deltas<'a>(
    actual: &'a Image,
    reference: &'a Image,
) -> Result<impl Iterator<Item = u8> + 'a, SnapshotError> {
    checked_len(actual)?;
    checked_len(reference)?;
    if (actual.width, actual.height) != (reference.width, reference.height) {
        return Err(SnapshotError::SizeMismatch {
            expected: (reference.width, reference.height),
            actual: (actual.width, actual.height),
        });
    }
    Ok(actual
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(reference.rgba.as_chunks::<4>().0)
        .map(|(a, b)| {
            a.iter()
                .zip(b)
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap_or(0)
        }))
}

/// Confirms that an image holds four bytes per pixel.
fn checked_len(image: &Image) -> Result<(), SnapshotError> {
    let expected = usize::try_from(image.width)
        .ok()
        .zip(usize::try_from(image.height).ok())
        .and_then(|(w, h)| w.checked_mul(h)?.checked_mul(4));
    if expected == Some(image.rgba.len()) {
        Ok(())
    } else {
        Err(SnapshotError::InvalidImage(format!(
            "{} x {} pixels need 4 bytes each, found {} bytes",
            image.width,
            image.height,
            image.rgba.len()
        )))
    }
}

fn io_error(path: &Path, error: &std::io::Error) -> SnapshotError {
    SnapshotError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

fn image_error(path: &Path, error: RenderError) -> SnapshotError {
    SnapshotError::Image {
        path: path.display().to_string(),
        error,
    }
}
