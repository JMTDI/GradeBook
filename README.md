# GradeBook

Keep track of students' grades by grade level, period and subject, with weighted averages
(tests, quizzes and homework), letter grades, and backup and restore. A native app for Windows,
macOS and Linux, written in Rust with [Slint](https://slint.dev).

| Grades | Settings |
|---|---|
| ![Grades screen](docs/screenshots/grades.png) | ![Settings screen](docs/screenshots/settings.png) |

It follows the system light/dark setting (`docs/screenshots/grades-dark.png`).

## Install

Download the installer for your system from the
[Releases page](https://github.com/JMTDI/GradeBook/releases).

| System | File | Notes |
|---|---|---|
| Windows 10/11 (64-bit) | `GradeBook-<version>-windows-x64-setup.exe` | Installs for your user only, no administrator rights needed. The installer isn't code-signed, so SmartScreen may say "Windows protected your PC": choose **More info → Run anyway**. |
| macOS 11+ (Apple Silicon and Intel) | `GradeBook-<version>-macos-universal.dmg` | Drag GradeBook to Applications. The app isn't notarized, so the first time, **right-click → Open** (or run `xattr -dr com.apple.quarantine /Applications/GradeBook.app`). |
| Debian, Ubuntu and derivatives | `gradebook_<version>-1_amd64.deb` | `sudo apt install ./gradebook_<version>-1_amd64.deb` |
| Any other 64-bit Linux | `GradeBook-<version>-x86_64.AppImage` | `chmod +x` it and run it. If it doesn't start, install `libfuse2` or run it with `--appimage-extract-and-run`. |

Backup and Restore open the system's file chooser. On Linux that is the desktop portal
(`xdg-desktop-portal`), falling back to `zenity`.

## Your data

GradeBook saves after every change, in `gradebook.json` in:

| System | Folder |
|---|---|
| Windows | `%APPDATA%\JMTDI\GradeBook\data` |
| macOS | `~/Library/Application Support/io.github.JMTDI.GradeBook` |
| Linux | `~/.local/share/gradebook` |

`gradebook --data-dir` prints the exact folder on Linux (on macOS run
`/Applications/GradeBook.app/Contents/MacOS/gradebook --data-dir`; the Windows build has no
console window, so use the folder above). Set `GRADEBOOK_DATA_DIR` to keep the data somewhere
else (for example on a USB stick). Uninstalling never deletes it.

**Backup and Restore:** in Settings, **Backup** writes a `GradeBookBackup.json` file and
**Restore** reads one, so you can keep a copy of your gradebook or move it to another computer.
Restoring replaces everything, so the app asks first and saves a copy of the data it replaces next
to `gradebook.json`, as `gradebook.before-restore-<time>.json` (one per restore, never
overwritten). If that copy can't be saved, the restore doesn't go ahead.

If the data file can't be read (for example, it was damaged), GradeBook moves it aside as
`gradebook.unreadable-<time>.json`, tells you, and starts with an empty gradebook. It never
overwrites that file.

## Using the app

The two screens are in a tab bar at the bottom.

**Grades.** Choose a grade level (1–10), a period (1–6) and a subject (20 to choose from). Every
student in that grade level gets a row. Choose **Test**, **Quiz** or **Homework**, type a score
(decimals work: `92.5`, or `92,5`) and click **Add**. Scores appear as chips tinted by type (red
for tests, blue for quizzes, grey for homework) with their letter grade, and the red × beside a
score deletes it. Under the chips is the student's weighted average with its letter grade, and the
class average is at the bottom.

**Settings.** Add students (a name and a grade level), set the weights of tests, quizzes and
homework (50 / 17 / 33 to begin with; the total is shown), and use Backup and Restore. To delete a
student, hover over the row and click the red minus (or focus the row and press Delete), then
confirm with the red **Delete** button.

**Averages.** The scores of each type are averaged first, then combined using the weights. The
weights are re-balanced over the types a student has scores for, so a student with only tests is
graded on tests alone.

**Letter grades.** Each letter starts at the score shown, so 91.5 is a B and 99.5 is an A. The
letter next to an average always matches the rounded number displayed.

| A+ | A | A− | B+ | B | B− | C+ | C | C− | D | E | F |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 100 | 96 | 94 | 92 | 88 | 86 | 84 | 79 | 76 | 70 | 63 | below 63 |

## Build from source

You need Rust 1.92 or newer ([rustup](https://rustup.rs)). On Linux you also need
`pkg-config` and `libfontconfig1-dev` (Debian/Ubuntu names) to build.

```sh
cargo run --release     # build and start the app
cargo test              # logic, headless UI interaction tests and screenshots
```

The tests drive the real UI without a window: they click, type, drag sliders and read what's on
screen, and write PNG screenshots of every screen to `target/tmp/screenshots`.

`cargo run --example make_icons` regenerates the icons in `assets/icons` from `assets/AppIcon.png`.

At run time on Linux the app loads the system's X11 or Wayland libraries (the `.deb` depends on
them): `libx11-6`, `libx11-xcb1`, `libxcb1`, `libxcb-xkb1`, `libxcursor1`, `libxext6`,
`libxfixes3`, `libxi6`, `libxrender1`, `libxkbcommon0` and `libxkbcommon-x11-0`, plus
`libwayland-client0` and `libwayland-cursor0` on Wayland.

## Build the installers

Each one is built on its own operating system; the GitHub Actions workflow
(`.github/workflows/desktop.yml`) does all three.

```sh
# Linux .deb (needs `cargo install cargo-deb`)
cargo build --release && cargo deb --no-build

# Linux AppImage (needs appimagetool on PATH)
cargo build --release && packaging/linux/build-appimage.sh

# macOS universal .dmg (run on a Mac)
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
packaging/macos/build-dmg.sh

# Windows installer (needs NSIS; run from the project folder)
cargo build --release
mkdir dist
makensis /DVERSION=1.0.0 /DROOT=%CD% packaging\windows\installer.nsi
```

`packaging/linux/smoke-test.sh <binary>` starts a built binary under a virtual X display
(`xvfb-run`) and checks that its window appears and stays up.

To publish, bump `version` in `Cargo.toml` and push a tag named `v<version>`. The
workflow builds everything and attaches the installers to a GitHub release.

### Code signing

Nothing is signed or notarized, which is why Windows and macOS show a warning on first launch.
Removing it takes a certificate you own: an Authenticode certificate for Windows, and an Apple
Developer ID plus notarization for macOS. The macOS app is signed ad hoc, which Apple Silicon
needs in order to run it at all.

## Project layout

```
src/        model.rs, calc.rs, book.rs, view.rs  the gradebook logic (no UI)
            store.rs                             data file, Backup/Restore files
            app.rs                               connects the UI to the logic
ui/         *.slint                              the screens and controls
tests/      interaction.rs, screenshots.rs       headless UI tests
packaging/  linux/, macos/, windows/             installers
```

## Licenses

GradeBook uses the following third-party software, which is redistributed in the app:

- [Slint](https://slint.dev), under its royalty-free desktop license, which asks for the
  attribution below. Slint is also available under the GPLv3 or a commercial license.
- The [Inter](https://rsms.me/inter/) typeface, under the SIL Open Font License
  ([`ui/fonts/Inter-OFL.txt`](ui/fonts/Inter-OFL.txt), included in every package).
- Open-source Rust crates, each under its own license. `cargo tree` lists them.

<!-- The Slint royalty-free license requires this attribution on a public page where the
     binaries can be downloaded. If you switch to the GPLv3 or a commercial Slint license,
     see https://slint.dev/pricing and adjust. -->
<a href="https://slint.dev"><img src="https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png" alt="Made with Slint" width="160"></a>
