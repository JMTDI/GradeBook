//! Checks on the packaging files, which the compiler never sees, so a mistake in one only shows
//! when its installer is built: the package id (`gradebook::APP_ID`) must be the same in all of
//! them, and the Windows installer script must spell its paths the way NSIS on Windows reads them.

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

#[test]
fn the_windows_installer_script_spells_its_paths_with_backslashes() {
    // NSIS on Windows finds the file name of a `File` path after the last backslash and ignores
    // "/", so `${ROOT}/target/...` (ROOT is `D:\...` there) matches no file. makensis on Linux and
    // macOS converts backslashes itself, so a slash builds fine there and only Windows fails.
    let script = read("packaging/windows/installer.nsi");
    for line in script.lines().filter(|line| !line.trim_start().starts_with(';')) {
        // The text between each pair of double quotes.
        for quoted in line.split('"').skip(1).step_by(2) {
            assert!(
                !(quoted.contains("${ROOT}") && quoted.contains('/')),
                "installer.nsi builds a path from ${{ROOT}} with a forward slash: {line}"
            );
        }
    }
}
