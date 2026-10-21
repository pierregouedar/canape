//! Compiles the Slint UI and embeds canape-shell.exe.manifest (package
//! identity, asInvoker) on MSVC.

fn main() {
    slint_build::compile("ui/app.slint").expect("Slint UI must compile");

    let manifest = "canape-shell.exe.manifest";
    println!("cargo:rerun-if-changed={manifest}");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        let path =
            std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join(manifest);
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
            path.display()
        );
    }
}
