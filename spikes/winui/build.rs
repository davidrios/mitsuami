//! Downloads the Windows App SDK 2.4 metadata and generates the XAML bindings
//! the spike needs, the same way `windows-reactor` generates its own.

use std::path::{Path, PathBuf};
use std::process::Command;

// The component packages behind Microsoft.WindowsAppSDK 2.4.0.
const PACKAGES: &[(&str, &str, &str)] = &[
    ("Microsoft.WindowsAppSDK.WinUI", "2.3.6", "metadata"),
    ("Microsoft.WindowsAppSDK.InteractiveExperiences", "2.1.6", "metadata/10.0.18362.0"),
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=filter.txt");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let mut bindgen = windows_bindgen::builder();
    bindgen.input_default();
    for (name, version, dir) in PACKAGES {
        bindgen.input(fetch(&out, name, version).join(dir));
    }
    bindgen
        .output(out.join("bindings.rs"))
        .implements(["Microsoft.UI.Xaml.IApplicationOverrides", "Microsoft.UI.Xaml.Markup.IXamlMetadataProvider"])
        .compose("Microsoft.UI.Xaml.Application")
        .minimal()
        .flat()
        .filter_file("filter.txt")
        .write();
}

fn fetch(out: &Path, name: &str, version: &str) -> PathBuf {
    let dir = out.join(format!("{name}.{version}"));
    if dir.exists() {
        return dir;
    }
    let nupkg = out.join(format!("{name}.{version}.zip"));
    let url = format!("https://www.nuget.org/api/v2/package/{name}/{version}");
    run(Command::new("curl").args(["-sSfL", "-o"]).arg(&nupkg).arg(url));
    std::fs::create_dir_all(&dir).unwrap();
    run(Command::new("tar").arg("-xf").arg(&nupkg).arg("-C").arg(&dir));
    dir
}

fn run(command: &mut Command) {
    let status = command.status().expect("spawn");
    assert!(status.success(), "{command:?} failed");
}
