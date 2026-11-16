fn main() {
    // The Cupertino style only affects the few std-widgets used (scroll views and
    // the palette); every visible control is a custom component.
    let config = slint_build::CompilerConfiguration::new()
        .with_style("cupertino".into())
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles)
        // Element names for the headless UI tests; not needed in release builds.
        .with_debug_info(
            std::env::var("PROFILE")
                .map(|profile| profile == "debug")
                .unwrap_or(false),
        );
    slint_build::compile_with_config("ui/main.slint", config).expect("failed to compile the Slint UI");

    embed_windows_resources();
}

/// Gives gradebook.exe its icon and version information (Explorer, the taskbar and
/// Task Manager). Version and file name come from Cargo.toml.
#[cfg(windows)]
fn embed_windows_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
    let mut resources = winresource::WindowsResource::new();
    resources.set_icon("assets/icons/icon.ico");
    // Task Manager shows the file description as the app's name.
    resources.set("FileDescription", "GradeBook");
    resources.set("ProductName", "GradeBook");
    resources
        .compile()
        .expect("failed to embed the Windows icon and version information");
}

#[cfg(not(windows))]
fn embed_windows_resources() {}
