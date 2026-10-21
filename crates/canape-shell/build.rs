//! Compiles the Slint UI with its translations bundled in, and embeds
//! canape-shell.exe.manifest (package identity, asInvoker) on MSVC.

fn main() {
    // Catalogs: lang/<code>/LC_MESSAGES/canape-shell.po, without context
    // (msgctxt), so one msgid is translated once whatever the component.
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("lang")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint UI must compile");
    println!("cargo:rerun-if-changed=lang");

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
