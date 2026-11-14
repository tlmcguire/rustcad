//! Finds an OpenCascade install and compiles the C++ shim against it.
//!
//! Search order:
//!   1. `OCCT_ROOT`, an install prefix with `include/opencascade` and `lib`.
//!   2. `target/occt/install` in the workspace, where `native/build-occt.sh target/occt` puts it.
//!
//! When neither exists the crate still builds, without a backend: `OcctKernel::new` returns an
//! error and `cfg(occt)` is off. Set `OCCT_REQUIRED=1` (CI does) to make that a build failure.
//!
//! OCCT is linked dynamically (DECISIONS.md, D2).

use std::path::{Path, PathBuf};

/// Toolkits the shim calls into directly.
const LIBS: &[&str] = &[
    "TKernel",
    "TKMath",
    "TKG2d",
    "TKG3d",
    "TKGeomBase",
    "TKBRep",
    "TKGeomAlgo",
    "TKTopAlgo",
    "TKPrim",
    "TKBO",
    "TKMesh",
    "TKXSBase",
    "TKDE",
    "TKDESTEP",
];

/// True when `lib` holds a library named `name` on any platform's naming scheme.
fn has_lib(lib: &Path, name: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(lib) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let file = entry.file_name();
        let file = file.to_string_lossy();
        file.starts_with(&format!("lib{name}.")) || file.starts_with(&format!("{name}."))
    })
}

fn find_occt() -> Option<PathBuf> {
    let usable = |root: &Path| {
        root.join("include/opencascade/Standard_Version.hxx")
            .is_file()
            && root.join("lib").is_dir()
    };
    if let Some(root) = std::env::var_os("OCCT_ROOT").map(PathBuf::from) {
        if usable(&root) {
            return Some(root);
        }
        println!(
            "cargo::warning=OCCT_ROOT={} has no include/opencascade and lib; ignoring it",
            root.display()
        );
    }
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    let local = manifest.join("../../target/occt/install");
    usable(&local).then_some(local)
}

fn main() {
    println!("cargo::rustc-check-cfg=cfg(occt)");
    println!("cargo::rerun-if-env-changed=OCCT_ROOT");
    println!("cargo::rerun-if-env-changed=OCCT_REQUIRED");
    println!("cargo::rerun-if-changed=native/shim.cpp");
    println!("cargo::rerun-if-changed=../../target/occt/install/include/opencascade");

    let Some(root) = find_occt() else {
        let required = std::env::var_os("OCCT_REQUIRED").is_some_and(|v| !v.is_empty() && v != "0");
        if required {
            println!(
                "cargo::error=OCCT_REQUIRED is set but no OpenCascade install was found. Set \
                 OCCT_ROOT or run crates/cad-kernel-occt/native/build-occt.sh target/occt"
            );
        } else {
            println!(
                "cargo::warning=OpenCascade not found: cad-kernel-occt is built without a backend \
                 and its tests are skipped. Set OCCT_ROOT or run native/build-occt.sh target/occt"
            );
        }
        return;
    };
    // An absolute path keeps the link search valid whatever directory rustc runs in.
    let root = root.canonicalize().unwrap_or(root);

    let missing: Vec<&str> = LIBS
        .iter()
        .copied()
        .filter(|name| !has_lib(&root.join("lib"), name))
        .collect();
    if !missing.is_empty() {
        println!(
            "cargo::error=the OpenCascade install at {} lacks {}. It predates STEP support; \
             rebuild it with crates/cad-kernel-occt/native/build-occt.sh",
            root.display(),
            missing.join(", ")
        );
        return;
    }

    let include = root.join("include/opencascade");
    let mut shim = cc::Build::new();
    shim.cpp(true)
        .std("c++17")
        .warnings(true)
        .file("native/shim.cpp");
    if shim.get_compiler().is_like_msvc() {
        shim.include(&include);
    } else {
        // As a system include, so warnings are reported for the shim and not for OCCT's headers.
        shim.flag("-isystem").flag(&include);
    }
    shim.compile("cad_occt_shim");

    println!(
        "cargo::rustc-link-search=native={}",
        root.join("lib").display()
    );
    for lib in LIBS {
        println!("cargo::rustc-link-lib=dylib={lib}");
    }
    println!("cargo::rustc-cfg=occt");
}
