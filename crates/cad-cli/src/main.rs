//! Headless runner for tests, batch export, and agents.
//!
//! Stub. Prints the version so CI has a binary to build and run.

fn main() {
    println!("cad-cli {}", env!("CARGO_PKG_VERSION"));
}
