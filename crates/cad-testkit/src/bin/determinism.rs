//! Determinism probe (ARCHITECTURE.md rule 9).
//!
//! Builds every case in `cad_testkit::probe` with each available kernel and prints one line
//! per case: `kernel case digest`. Inside one run it also rebuilds every case in a fresh
//! kernel and on four threads at once, and fails if any digest differs. `scripts/gates.sh`
//! runs it twice and compares the two outputs, which catches differences between processes.

use std::process::ExitCode;
use std::sync::Arc;

use cad_kernel::{KResult, Kernel};
use cad_kernel_occt::OcctKernel;
use cad_testkit::ReferenceKernel;
use cad_testkit::probe::{Case, cases, digest, occt_required};

type MakeKernel = fn() -> KResult<Arc<dyn Kernel>>;

fn reference() -> KResult<Arc<dyn Kernel>> {
    Ok(Arc::new(ReferenceKernel::new()))
}

fn occt() -> KResult<Arc<dyn Kernel>> {
    Ok(Arc::new(OcctKernel::new()?))
}

fn run(kernel: &dyn Kernel, case: Case) -> KResult<u64> {
    let shape = (case.build)(kernel)?;
    let result = digest(kernel, shape);
    kernel.release(shape);
    result
}

/// Digests every case serially in one kernel, again in a fresh kernel, and again on four
/// threads sharing one kernel. Returns the serial digests, or why they disagreed.
fn probe(make: MakeKernel, cases: &[Case]) -> Result<Vec<u64>, String> {
    let first = make().map_err(|e| e.to_string())?;
    let serial: Vec<u64> = cases
        .iter()
        .map(|case| run(first.as_ref(), *case))
        .collect::<KResult<_>>()
        .map_err(|e| e.to_string())?;

    let fresh = make().map_err(|e| e.to_string())?;
    for (case, want) in cases.iter().zip(&serial) {
        let got = run(fresh.as_ref(), *case).map_err(|e| e.to_string())?;
        if got != *want {
            return Err(format!("{}: a fresh kernel gives {got:016x}", case.name));
        }
    }

    let threads: Vec<_> = (0..4)
        .map(|offset| {
            let kernel = Arc::clone(&first);
            let cases = cases.to_vec();
            std::thread::spawn(move || {
                // Each thread walks the cases from a different start, so calls interleave.
                (0..cases.len())
                    .map(|i| {
                        let case = cases[(i + offset) % cases.len()];
                        run(kernel.as_ref(), case).map(|d| (case.name, d))
                    })
                    .collect::<KResult<Vec<_>>>()
            })
        })
        .collect();
    for thread in threads {
        let results = thread
            .join()
            .map_err(|_| "a probe thread panicked".to_string())?
            .map_err(|e| e.to_string())?;
        for (name, got) in results {
            let index = cases.iter().position(|c| c.name == name).unwrap_or(0);
            if serial.get(index) != Some(&got) {
                return Err(format!("{name}: four threads give {got:016x}"));
            }
        }
    }
    Ok(serial)
}

fn main() -> ExitCode {
    let all = cases();
    let kernels: [(&str, MakeKernel, bool); 2] =
        [("reference", reference, false), ("occt", occt, true)];
    for (name, make, booleans) in kernels {
        if name == "occt" && !cad_kernel_occt::available() {
            if occt_required() {
                eprintln!("determinism: OCCT_REQUIRED is set, but this build has no OCCT");
                return ExitCode::FAILURE;
            }
            eprintln!("determinism: SKIPPED occt (this build has no OCCT)");
            continue;
        }
        let cases: Vec<Case> = all
            .iter()
            .copied()
            .filter(|c| booleans || !c.needs_boolean)
            .collect();
        match probe(make, &cases) {
            Ok(digests) => {
                for (case, d) in cases.iter().zip(digests) {
                    println!("{name} {} {d:016x}", case.name);
                }
            }
            Err(why) => {
                eprintln!("determinism: {name}: {why}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
