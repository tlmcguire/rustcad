//! Tests for the determinism and performance probes that `scripts/gates.sh` runs.
//!
//! The probes are binaries, so these tests run them as child processes, the way the gates do.

use std::path::PathBuf;
use std::process::{Command, Output};

use cad_kernel::Kernel;
use cad_testkit::ReferenceKernel;
use cad_testkit::probe::{Fnv, cases, digest};

const DETERMINISM: &str = env!("CARGO_BIN_EXE_determinism");
const PERF: &str = env!("CARGO_BIN_EXE_perf");

type TestResult = Result<(), Box<dyn std::error::Error>>;

const KEYS: [&str; 6] = [
    "pocket_build_ms",
    "union_chain_build_ms",
    "pocket_tessellate_ms",
    "pocket_mass_props_ms",
    "pocket_brep_round_trip_ms",
    "pocket_step_round_trip_ms",
];

fn budget_file(name: &str, text: &str) -> Result<PathBuf, std::io::Error> {
    let dir = std::env::temp_dir().join("cad-testkit-probes");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, text)?;
    Ok(path)
}

fn budgets(ms: &str) -> String {
    KEYS.iter().map(|k| format!("{k} = {ms}\n")).collect()
}

fn run(program: &str, args: &[&std::path::Path]) -> Result<Output, std::io::Error> {
    Command::new(program).args(args).output()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn fnv_matches_the_reference_values() {
    // FNV-1a 64 of "" and "a", from the published test vectors.
    assert_eq!(Fnv::default().finish(), 0xcbf2_9ce4_8422_2325);
    let mut h = Fnv::default();
    h.bytes(b"a");
    assert_eq!(h.finish(), 0xaf63_dc4c_8601_ec8c);
}

#[test]
fn digests_see_every_bit_of_a_float() {
    let mut a = Fnv::default();
    let mut b = Fnv::default();
    a.f64(1.0);
    b.f64(f64::from_bits(1.0_f64.to_bits() + 1));
    assert_ne!(a.finish(), b.finish());
    let mut c = Fnv::default();
    c.u64(1.0_f64.to_bits());
    let mut d = Fnv::default();
    d.f64(1.0);
    assert_eq!(c.finish(), d.finish(), "a float is hashed by its bits");
}

#[test]
fn case_names_are_unique_and_the_box_needs_no_boolean() {
    let all = cases();
    let mut names: Vec<_> = all.iter().map(|c| c.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), all.len());
    let boxes = all.iter().find(|c| c.name == "box");
    assert!(boxes.is_some_and(|c| !c.needs_boolean));
    assert!(all.iter().filter(|c| c.needs_boolean).count() >= 3);
}

#[test]
fn a_digest_is_the_same_in_a_fresh_kernel_and_differs_between_shapes() -> TestResult {
    let (first, second) = (ReferenceKernel::new(), ReferenceKernel::new());
    let build =
        |k: &dyn Kernel| k.make_box(cad_math::Vec3::ZERO, cad_math::Vec3::new(2.0, 3.0, 4.0));
    let a = digest(&first, build(&first)?)?;
    assert_eq!(a, digest(&second, build(&second)?)?);
    let other = first.make_box(cad_math::Vec3::ZERO, cad_math::Vec3::new(2.0, 3.0, 5.0))?;
    assert_ne!(a, digest(&first, other)?);
    Ok(())
}

#[test]
fn the_determinism_probe_prints_the_same_digests_twice() -> TestResult {
    let first = run(DETERMINISM, &[])?;
    let second = run(DETERMINISM, &[])?;
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert!(second.status.success(), "{}", text(&second.stderr));
    assert_eq!(first.stdout, second.stdout);
    let out = text(&first.stdout);
    assert!(
        out.lines().any(|l| l.starts_with("reference box ")),
        "{out}"
    );
    for line in out.lines() {
        let digest = line.split_whitespace().nth(2).unwrap_or("");
        assert_eq!(digest.len(), 16, "{line}");
    }
    if cad_kernel_occt::available() {
        assert!(
            out.lines().any(|l| l.starts_with("occt union_chain ")),
            "{out}"
        );
    }
    Ok(())
}

#[test]
fn the_perf_probe_passes_within_its_budgets_and_fails_over_them() -> TestResult {
    if !cad_kernel_occt::available() {
        eprintln!("SKIPPED: the perf probe needs OCCT");
        return Ok(());
    }
    let roomy = budget_file("roomy.txt", &budgets("100000"))?;
    let out = run(PERF, &[&roomy])?;
    assert!(
        out.status.success(),
        "{}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    let report = text(&out.stdout);
    for key in KEYS {
        assert!(report.contains(key), "{report}");
    }
    assert!(!report.contains("OVER"), "{report}");

    let tight = budget_file("tight.txt", &budgets("0.000001"))?;
    let out = run(PERF, &[&tight])?;
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stdout).contains("OVER"));
    Ok(())
}

#[test]
fn the_perf_probe_rejects_a_malformed_budget_file() -> TestResult {
    let mut missing = budgets("10");
    missing = missing.replace("pocket_mass_props_ms = 10\n", "");
    let cases = [
        ("missing.txt", missing, "missing key `pocket_mass_props_ms`"),
        (
            "unknown.txt",
            budgets("10") + "warp_ms = 1\n",
            "unknown key `warp_ms`",
        ),
        (
            "duplicate.txt",
            budgets("10") + "pocket_build_ms = 2\n",
            "duplicate key",
        ),
        ("negative.txt", budgets("-1"), "not a positive number"),
        (
            "no_equals.txt",
            "pocket_build_ms 10\n".to_string(),
            "expected `key = value`",
        ),
    ];
    for (name, contents, message) in cases {
        let path = budget_file(name, &contents)?;
        let out = run(PERF, &[&path])?;
        assert_eq!(out.status.code(), Some(2), "{name}");
        assert!(
            text(&out.stderr).contains(message),
            "{name}: {}",
            text(&out.stderr)
        );
    }
    let out = Command::new(PERF).output()?;
    assert_eq!(out.status.code(), Some(2), "no argument");
    let out = run(PERF, &[std::path::Path::new("/nonexistent/budgets.txt")])?;
    assert_eq!(out.status.code(), Some(2), "unreadable file");
    Ok(())
}
