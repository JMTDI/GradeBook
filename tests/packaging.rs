//! The package id (`gradebook::APP_ID`) also names things in packaging files the compiler never
//! sees. If one is left behind, the app still builds, but the Linux icon goes missing, the macOS
//! bundle gets the wrong identifier, or the Windows uninstall entry ends up under another name.

use std::fs;
use std::path::Path;

use gradebook::APP_ID;

fn read(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn has_line(text: &str, wanted: &str) -> bool {
    text.lines().any(|line| line.trim_end() == wanted)
}

#[test]
fn the_package_id_is_the_same_in_every_packaging_file() {
    // Linux: the desktop file is named after the id and points at the icon and window class
    // of the same name.
    let desktop = read(&format!("packaging/linux/{APP_ID}.desktop"));
    assert!(has_line(&desktop, &format!("Icon={APP_ID}")), "desktop file Icon");
    assert!(
        has_line(&desktop, &format!("StartupWMClass={APP_ID}")),
        "desktop file StartupWMClass"
    );

    // The .deb installs the desktop file and the icons under that name, and the AppImage
    // script builds the same file names.
    let manifest = read("Cargo.toml");
    assert!(
        manifest.contains(&format!("packaging/linux/{APP_ID}.desktop")),
        "Cargo.toml (deb): desktop file"
    );
    assert!(
        manifest.contains(&format!("/apps/{APP_ID}.png")),
        "Cargo.toml (deb): icons"
    );
    assert!(
        has_line(&read("packaging/linux/build-appimage.sh"), &format!("id={APP_ID}")),
        "build-appimage.sh"
    );

    // macOS: the bundle identifier.
    let plist = read("packaging/macos/Info.plist");
    let after_key = plist
        .split("<key>CFBundleIdentifier</key>")
        .nth(1)
        .expect("Info.plist has a CFBundleIdentifier");
    assert!(
        after_key
            .trim_start()
            .starts_with(&format!("<string>{APP_ID}</string>")),
        "Info.plist CFBundleIdentifier"
    );

    // Windows: the installer's registry entries.
    assert!(
        has_line(
            &read("packaging/windows/installer.nsi"),
            &format!("!define APP_ID \"{APP_ID}\"")
        ),
        "installer.nsi APP_ID"
    );
}
