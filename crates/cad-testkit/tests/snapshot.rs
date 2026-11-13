//! Acceptance tests for T007: the snapshot harness.
//!
//! The tests that render need a graphics adapter (hardware or software) but no display.

use std::path::{Path, PathBuf};

use cad_kernel::Kernel;
use cad_math::{Tolerance, Vec3};
use cad_render::{Camera, Image, RenderError, Renderer};
use cad_testkit::snapshot::{self, compare, difference_image, failure_dir, image_path};
use cad_testkit::{
    Difference, ReferenceKernel, Snapshot, SnapshotError, SnapshotSpec, assert_snapshot,
};

const BOX_SNAPSHOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/snapshots/box_40x20x10.snapshot"
);
/// The view recorded in the snapshot file.
const BOX_VIEW: Vec3 = Vec3::new(1.0, -1.5, 0.8);

const VALID: &str = "\
# a comment

channel_tolerance = 8
max_differing_pixels = 384
source = by eye
";

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn render_box(from: Vec3) -> Result<Image, Box<dyn std::error::Error>> {
    let kernel = ReferenceKernel::new();
    let shape = kernel.make_box(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))?;
    let mesh = kernel.tessellate(shape, Tolerance::default())?;
    let renderer = Renderer::headless()?;
    let scene = renderer.upload(&mesh)?;
    let camera = Camera::framing(Vec3::new(20.0, 10.0, 5.0), 23.0, from)?;
    Ok(renderer.render(&scene, &camera, 320, 240)?.image)
}

/// A `width` x `height` image of one colour.
fn flat(width: u32, height: u32, color: [u8; 4]) -> Image {
    Image {
        width,
        height,
        rgba: (0..width * height).flat_map(|_| color).collect(),
    }
}

fn set_pixel(image: &mut Image, x: u32, y: u32, color: [u8; 4]) {
    let start = (y * image.width + x) as usize * 4;
    image.rgba[start..start + 4].copy_from_slice(&color);
}

fn snapshot_of(reference: Image, channel_tolerance: u8, max_differing_pixels: usize) -> Snapshot {
    Snapshot {
        spec: SnapshotSpec {
            channel_tolerance,
            max_differing_pixels,
            source: "test".into(),
        },
        reference,
    }
}

/// A fresh directory for one test, holding a copy of the box snapshot's spec file under
/// `stem`. Returns the path of the copy.
fn scratch_spec(stem: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("cad-testkit-t007-{}-{stem}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let spec = dir.join(format!("{stem}.snapshot"));
    std::fs::copy(BOX_SNAPSHOT, &spec)?;
    Ok(spec)
}

fn remove_scratch(spec: &Path) -> TestResult {
    if let Some(dir) = spec.parent() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

fn panic_message(result: std::thread::Result<()>) -> String {
    match result {
        Ok(()) => "did not panic".into(),
        Err(payload) => payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
            .unwrap_or_default(),
    }
}

#[test]
fn spec_file_parses() -> TestResult {
    assert_eq!(
        SnapshotSpec::parse(VALID)?,
        SnapshotSpec {
            channel_tolerance: 8,
            max_differing_pixels: 384,
            source: "by eye".into(),
        }
    );
    Ok(())
}

#[test]
fn spec_file_errors_name_the_line_or_the_key() {
    let parse_error = |text: &str| match SnapshotSpec::parse(text) {
        Err(SnapshotError::Parse { line, message }) => (line, message),
        other => (0, format!("{other:?}")),
    };
    let cases = [
        (format!("{VALID}extra = 1\n"), 6, "unknown key `extra`"),
        (
            format!("{VALID}source = again\n"),
            6,
            "duplicate key `source`",
        ),
        (VALID.replace("= 8", "= 256"), 3, "0 to 255"),
        (VALID.replace("= 8", "= -1"), 3, "0 to 255"),
        (VALID.replace("= 384", "= 1.5"), 4, "whole number"),
        (VALID.replace("= by eye", "="), 5, "must not be empty"),
        (VALID.replace("source = by eye", "source"), 5, "key = value"),
    ];
    for (text, line, expected) in cases {
        let (got_line, message) = parse_error(&text);
        assert_eq!(got_line, line, "{text}");
        assert!(message.contains(expected), "{message}");
    }

    for key in ["channel_tolerance", "max_differing_pixels", "source"] {
        let text: String = VALID
            .lines()
            .filter(|l| !l.starts_with(key))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(
            SnapshotSpec::parse(&text),
            Err(SnapshotError::MissingKey(key))
        );
    }
}

#[test]
fn identical_images_do_not_differ() -> TestResult {
    let image = flat(8, 4, [10, 20, 30, 255]);
    assert_eq!(
        compare(&image, &image, 0)?,
        Difference {
            differing_pixels: 0,
            max_channel_delta: 0,
        }
    );
    Ok(())
}

#[test]
fn channel_tolerance_is_inclusive_and_applies_to_every_channel() -> TestResult {
    let reference = flat(8, 4, [100, 100, 100, 255]);
    let mut actual = reference.clone();
    set_pixel(&mut actual, 0, 0, [108, 100, 100, 255]); // 8 off in red
    set_pixel(&mut actual, 1, 0, [100, 91, 100, 255]); // 9 off in green
    set_pixel(&mut actual, 2, 0, [100, 100, 100, 200]); // 55 off in alpha

    assert_eq!(
        compare(&actual, &reference, 8)?,
        Difference {
            differing_pixels: 2,
            max_channel_delta: 55,
        }
    );
    assert_eq!(compare(&actual, &reference, 7)?.differing_pixels, 3);
    assert_eq!(compare(&actual, &reference, 55)?.differing_pixels, 0);
    Ok(())
}

#[test]
fn check_passes_at_the_pixel_limit_and_fails_one_over() -> TestResult {
    let reference = flat(8, 4, [100, 100, 100, 255]);
    let mut actual = reference.clone();
    for x in 0..3 {
        set_pixel(&mut actual, x, 1, [0, 0, 0, 255]);
    }
    let measured = Difference {
        differing_pixels: 3,
        max_channel_delta: 100,
    };
    assert_eq!(
        snapshot_of(reference.clone(), 8, 3).check(&actual),
        Ok(measured)
    );
    assert_eq!(
        snapshot_of(reference, 8, 2).check(&actual),
        Err(SnapshotError::Differs {
            difference: measured,
            allowed: 2,
            channel_tolerance: 8,
        })
    );
    Ok(())
}

#[test]
fn size_mismatch_and_malformed_images_are_their_own_errors() {
    let reference = flat(8, 4, [0, 0, 0, 255]);
    assert_eq!(
        compare(&flat(4, 8, [0, 0, 0, 255]), &reference, 255),
        Err(SnapshotError::SizeMismatch {
            expected: (8, 4),
            actual: (4, 8),
        })
    );
    let mut short = reference.clone();
    short.rgba.pop();
    assert!(matches!(
        compare(&short, &reference, 255),
        Err(SnapshotError::InvalidImage(_))
    ));
    assert!(matches!(
        compare(&reference, &short, 255),
        Err(SnapshotError::InvalidImage(_))
    ));
}

#[test]
fn difference_image_marks_only_the_differing_pixels() -> TestResult {
    let reference = flat(8, 4, [100, 100, 100, 255]);
    let mut actual = reference.clone();
    set_pixel(&mut actual, 5, 2, [0, 0, 0, 255]);
    set_pixel(&mut actual, 6, 2, [104, 100, 100, 255]); // within tolerance

    let diff = difference_image(&actual, &reference, 8)?;
    assert_eq!((diff.width, diff.height), (8, 4));
    let red = [255, 0, 0, 255];
    for y in 0..4 {
        for x in 0..8 {
            let pixel = diff.pixel(x, y).ok_or("pixel missing")?;
            assert_eq!(pixel == red, (x, y) == (5, 2), "pixel ({x}, {y})");
        }
    }
    Ok(())
}

#[test]
fn missing_files_are_reported() -> TestResult {
    let nowhere = std::env::temp_dir().join("cad-testkit-t007-no-such.snapshot");
    assert!(matches!(
        Snapshot::load(&nowhere),
        Err(SnapshotError::Io { .. })
    ));

    // A spec file with no image beside it.
    let spec = scratch_spec("no-image")?;
    let loaded = Snapshot::load(&spec);
    remove_scratch(&spec)?;
    assert!(
        matches!(
            loaded,
            Err(SnapshotError::Image {
                error: RenderError::Io(_),
                ..
            })
        ),
        "{loaded:?}"
    );
    Ok(())
}

#[test]
fn update_writes_the_reference_image_beside_an_existing_spec() -> TestResult {
    let image = flat(8, 4, [1, 2, 3, 255]);

    let nowhere = std::env::temp_dir().join("cad-testkit-t007-no-spec.snapshot");
    assert!(matches!(
        snapshot::update(&image, &nowhere),
        Err(SnapshotError::Io { .. })
    ));
    assert!(!image_path(&nowhere).exists(), "no image without a spec");

    let spec = scratch_spec("update")?;
    snapshot::update(&image, &spec)?;
    let loaded = Snapshot::load(&spec);
    remove_scratch(&spec)?;
    assert_eq!(loaded?.reference, image);
    Ok(())
}

#[test]
fn box_render_matches_its_snapshot() -> TestResult {
    assert_snapshot(&render_box(BOX_VIEW)?, BOX_SNAPSHOT);
    Ok(())
}

#[test]
fn box_from_another_view_does_not_match() -> TestResult {
    // Not the opposite view: a box under a headlight looks the same from (-1, 1.5, -0.8).
    let other = render_box(Vec3::new(1.0, -0.4, 0.8))?;
    let snapshot = Snapshot::load(BOX_SNAPSHOT)?;
    match snapshot.check(&other) {
        Err(SnapshotError::Differs {
            difference,
            allowed,
            ..
        }) => {
            assert_eq!(allowed, snapshot.spec.max_differing_pixels);
            assert!(
                difference.differing_pixels > 10 * allowed,
                "only {} pixels differ",
                difference.differing_pixels
            );
        }
        other => return Err(format!("expected a mismatch, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn a_failed_assert_names_the_counts_saves_images_and_keeps_the_reference() -> TestResult {
    // A private copy of the snapshot, so the files this test writes have their own names.
    let spec = scratch_spec("failing")?;
    let reference = image_path(&spec);
    std::fs::copy(image_path(BOX_SNAPSHOT), &reference)?;
    let before = std::fs::read(&reference)?;

    let mut wrong = Image::read_png(&reference)?;
    let allowed = SnapshotSpec::load(&spec)?.max_differing_pixels;
    let changed = u32::try_from(allowed)? + 16;
    for i in 0..changed {
        set_pixel(&mut wrong, i % 320, 100 + i / 320, [255, 0, 255, 255]);
    }

    let message = panic_message(std::panic::catch_unwind(|| assert_snapshot(&wrong, &spec)));
    let after = std::fs::read(&reference)?;
    remove_scratch(&spec)?;

    assert!(message.contains("failing.snapshot"), "{message}");
    assert!(
        message.contains(&format!("{changed} pixel(s) differ")),
        "{message}"
    );
    assert!(message.contains(&format!("{allowed} allowed")), "{message}");
    assert_eq!(
        after, before,
        "a failing check must not touch the reference"
    );

    let saved = Image::read_png(failure_dir().join("failing.actual.png"))?;
    assert_eq!(saved, wrong);
    let diff = Image::read_png(failure_dir().join("failing.diff.png"))?;
    let red = diff
        .rgba
        .chunks_exact(4)
        .filter(|p| *p == [255, 0, 0, 255])
        .count();
    assert_eq!(red, usize::try_from(changed)?);
    Ok(())
}
