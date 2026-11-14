//! Kernel worker process: serves an OpenCascade kernel on standard input and output.
//!
//! Started by `cad_kernel_occt::worker::WorkerKernel`. Not meant to be run by hand.
//! Diagnostics go to standard error; standard output carries only protocol frames. At
//! startup the worker moves standard output aside, so anything OCCT prints lands on
//! standard error instead of corrupting a frame.
//!
//! Usage: `occt-worker [--test-delay-ms N]`

use std::io::{BufReader, BufWriter};
use std::process::ExitCode;
use std::time::Duration;

use cad_kernel_occt::OcctKernel;
use cad_kernel_occt::worker::{ServeOptions, serve};

fn main() -> ExitCode {
    let mut options = ServeOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let delay = (arg == "--test-delay-ms").then(|| args.next()).flatten();
        match delay.and_then(|ms| ms.parse().ok()) {
            Some(ms) => options.delay = Some(Duration::from_millis(ms)),
            None => {
                eprintln!("occt-worker: bad argument `{arg}`");
                return ExitCode::from(2);
            }
        }
    }
    let protocol = match cad_kernel_occt::claim_stdout() {
        Ok(file) => file,
        Err(e) => {
            eprintln!("occt-worker: {e}");
            return ExitCode::from(2);
        }
    };
    let kernel = match OcctKernel::new() {
        Ok(kernel) => kernel,
        Err(e) => {
            eprintln!("occt-worker: {e}");
            return ExitCode::from(2);
        }
    };
    let input = BufReader::new(std::io::stdin().lock());
    let output = BufWriter::new(protocol);
    match serve(&kernel, input, output, options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("occt-worker: {e}");
            ExitCode::FAILURE
        }
    }
}
