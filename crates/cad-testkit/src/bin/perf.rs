//! Performance probe for the merge gates.
//!
//! Usage: `perf <budget file>`
//!
//! Times each operation in the budget file with the OCCT kernel and fails when a median is
//! over its budget. The budget file is strict `key = value` text (DECISIONS.md D11): every
//! key is an operation name with its budget in milliseconds, and every operation this probe
//! knows must have a budget. Budgets live in `scripts/`, a protected path, so a change cannot
//! loosen its own budget.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::Instant;

use cad_kernel::{KResult, Kernel, ShapeId};
use cad_kernel_occt::OcctKernel;
use cad_math::Tolerance;
use cad_testkit::probe::{cases, occt_required};

/// Runs per operation. The median of these is compared with the budget.
const RUNS: usize = 15;

type Op = fn(&dyn Kernel, ShapeId) -> KResult<()>;

fn build_case(name: &'static str) -> impl Fn(&dyn Kernel) -> KResult<ShapeId> {
    move |k| {
        let case = cases().into_iter().find(|c| c.name == name);
        match case {
            Some(case) => (case.build)(k),
            None => Err(cad_kernel::KernelError::InvalidInput(format!(
                "no case {name}"
            ))),
        }
    }
}

/// Every operation: its name, the case it starts from, and what is timed.
fn operations() -> Vec<(&'static str, &'static str, Op)> {
    vec![
        ("pocket_build_ms", "box", |k, _| {
            let shape = build_case("pocket")(k)?;
            k.release(shape);
            Ok(())
        }),
        ("union_chain_build_ms", "box", |k, _| {
            let shape = build_case("union_chain")(k)?;
            k.release(shape);
            Ok(())
        }),
        ("pocket_tessellate_ms", "pocket", |k, s| {
            k.tessellate(s, Tolerance::default()).map(drop)
        }),
        ("pocket_mass_props_ms", "pocket", |k, s| {
            k.mass_props(s).map(drop)
        }),
        ("pocket_brep_round_trip_ms", "pocket", |k, s| {
            let back = k.from_brep_bytes(&k.to_brep_bytes(s)?)?;
            k.release(back);
            Ok(())
        }),
        ("pocket_step_round_trip_ms", "pocket", |k, s| {
            let back = k.import_step(&k.export_step(s)?)?;
            k.release(back);
            Ok(())
        }),
    ]
}

fn parse_budgets(text: &str) -> Result<BTreeMap<String, f64>, String> {
    let mut budgets = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {}: expected `key = value`", n + 1))?;
        let (key, value) = (key.trim(), value.trim());
        let ms: f64 = value
            .parse()
            .ok()
            .filter(|ms: &f64| ms.is_finite() && *ms > 0.0)
            .ok_or_else(|| format!("line {}: `{value}` is not a positive number", n + 1))?;
        if budgets.insert(key.to_string(), ms).is_some() {
            return Err(format!("line {}: duplicate key `{key}`", n + 1));
        }
    }
    let known: Vec<&str> = operations().iter().map(|op| op.0).collect();
    if let Some(unknown) = budgets.keys().find(|k| !known.contains(&k.as_str())) {
        return Err(format!("unknown key `{unknown}`"));
    }
    if let Some(missing) = known.iter().find(|k| !budgets.contains_key(**k)) {
        return Err(format!("missing key `{missing}`"));
    }
    Ok(budgets)
}

fn median_ms(k: &dyn Kernel, start: &'static str, op: Op) -> KResult<f64> {
    let shape = build_case(start)(k)?;
    op(k, shape)?; // warm up
    let mut times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t = Instant::now();
        op(k, shape)?;
        times.push(t.elapsed().as_secs_f64() * 1e3);
    }
    k.release(shape);
    times.sort_by(f64::total_cmp);
    Ok(times[RUNS / 2])
}

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: perf <budget file>");
        return ExitCode::from(2);
    };
    let budgets = match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| parse_budgets(&text))
    {
        Ok(b) => b,
        Err(e) => {
            eprintln!("perf: {path}: {e}");
            return ExitCode::from(2);
        }
    };
    let kernel = match OcctKernel::new() {
        Ok(k) => k,
        Err(e) if !occt_required() => {
            eprintln!("perf: SKIPPED ({e})");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("perf: OCCT_REQUIRED is set, but: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut failed = false;
    for (name, start, op) in operations() {
        let budget = budgets.get(name).copied().unwrap_or(0.0);
        match median_ms(&kernel, start, op) {
            Ok(ms) => {
                let verdict = if ms <= budget { "ok" } else { "OVER" };
                failed |= ms > budget;
                println!("{name:<28} {ms:>9.3} ms  budget {budget:>8.1} ms  {verdict}");
            }
            Err(e) => {
                println!("{name:<28} failed: {e}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
