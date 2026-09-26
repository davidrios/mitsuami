//! Compiles the C++ shim against the system's Qt 6, running moc on the one
//! header that declares a QObject.

use std::path::PathBuf;
use std::process::Command;

const MODULES: [&str; 5] = ["Qt6Widgets", "Qt6Quick", "Qt6Qml", "Qt6QuickControls2", "Qt6Gui"];

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut includes = Vec::new();
    for module in MODULES {
        let lib = pkg_config::Config::new()
            .atleast_version("6.5")
            .probe(module)
            .unwrap_or_else(|e| panic!("kirigami spike: Qt 6 development files not found ({module}): {e}"));
        includes.extend(lib.include_paths);
    }
    let libexec = pkg_config::get_variable("Qt6Core", "libexecdir").expect("Qt6Core has a libexecdir");
    let moc_out = out.join("moc_shim.cpp");
    let status = Command::new(PathBuf::from(libexec).join("moc"))
        .arg("src/shim.h")
        .arg("-o")
        .arg(&moc_out)
        .status()
        .expect("kirigami spike: cannot run moc");
    assert!(status.success(), "moc failed");

    let mut build = cc::Build::new();
    build.cpp(true).std("c++17").file("src/shim.cpp").file(&moc_out).include("src").flag("-fPIC");
    for include in includes {
        build.include(include);
    }
    build.warnings(false).compile("mitsuami_kirigami_shim");
    println!("cargo:rerun-if-changed=src/shim.h");
    println!("cargo:rerun-if-changed=src/shim.cpp");
}
