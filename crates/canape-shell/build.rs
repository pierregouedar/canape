//! Compiles the Slint UI with its translations bundled in, and embeds
//! canape-shell.exe.manifest (package identity, asInvoker) on MSVC.

fn main() {
    // Catalogs: lang/<code>/LC_MESSAGES/canape-shell.po, without context
    // (msgctxt), so one msgid is translated once whatever the component.
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("lang")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint UI must compile");
    // A directory only changes when an entry is added or removed: watch
    // each catalog too, so an edited translation rebuilds.
    println!("cargo:rerun-if-changed=lang");
    for entry in std::fs::read_dir("lang").into_iter().flatten().flatten() {
        let po = entry.path().join("LC_MESSAGES").join("canape-shell.po");
        if po.is_file() {
            println!("cargo:rerun-if-changed={}", po.display());
        }
    }

    // The commit, shown next to the version in the settings ("" outside a
    // git checkout).
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    println!("cargo:rustc-env=CANAPE_COMMIT={commit}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");

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
