<!-- When you replace an image or GIF below, bump its ?v= number on all of them. VS Code's Markdown preview keeps showing the old picture while its URL is unchanged. -->
<div align="center">

<img src="assets/icons/icon-128.png" alt="GradeBook icon" width="96" height="96">

# GradeBook

**A fast, friendly gradebook for teachers and homeschoolers.**<br>
Weighted averages, letter grades, insights, report cards and automatic backups, as a native app for Windows, macOS and Linux.

[![Build](https://github.com/JMTDI/GradeBook/actions/workflows/desktop.yml/badge.svg)](https://github.com/JMTDI/GradeBook/actions/workflows/desktop.yml)
![Platforms: Windows, macOS and Linux](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-5856D6)
![Rust 1.92 or newer](https://img.shields.io/badge/Rust-1.92%2B-DEA584?logo=rust&logoColor=white)
[![Downloads](https://img.shields.io/github/downloads/JMTDI/GradeBook/total?logo=github)](https://github.com/JMTDI/GradeBook/releases)

[![Download for Windows](https://img.shields.io/badge/Download%20for-Windows-0078D6?style=for-the-badge)](https://github.com/JMTDI/GradeBook/releases/latest)
[![Download for macOS](https://img.shields.io/badge/Download%20for-macOS-1D1D1F?style=for-the-badge)](https://github.com/JMTDI/GradeBook/releases/latest)
[![Download for Linux](https://img.shields.io/badge/Download%20for-Linux-E95420?style=for-the-badge)](https://github.com/JMTDI/GradeBook/releases/latest)

[Features](#features) · [Install](#install) · [Using the app](#using-the-app) · [Your data](#your-data) · [Build from source](#build-from-source)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/demo-dark.gif?v=4">
  <img src="docs/demo-light.gif?v=4" alt="GradeBook in action: scores are added and Enter moves on to the next student, a click shows what a student needs on the next test, a deleted score comes back with Undo, the Insights screen sums up the class, the lowest homework score is left out of the average from Settings, and the button in the corner switches between light and dark" width="760">
</picture>

</div>

## Features

- 📚 **Organised the way you teach.** Grade levels 1–10 (add Kindergarten and grades 11–20 if you need them), six periods and 20 subjects plus your own, one class at a time.
- ⚖️ **Weighted averages.** Homework, tests, quizzes and categories of your own, such as Projects, count as much as you decide (50 / 17 / 33 to begin with), in every subject or in one. The weights re-balance on their own for students who don't have every kind of score yet.
- 🧮 **Scores the way teachers write them.** Points like `18/20`, extra credit like `+5`, excused scores (`EX`), and an option to leave out the lowest score of a kind.
- 🔤 **Letter grades that add up.** From A+ to F, including fractional scores: 91.5 is a B and 99.5 is an A.
- 🎯 **"What do I need?"** Pick a letter and see the score a student needs on the next test, quiz or homework to reach it.
- 📊 **Insights.** The class average, the best and the lowest, how the letters are spread, who needs attention, and how the class average moved over the periods.
- 🧾 **Report cards.** Every subject and period of a student on one page, saved as a spreadsheet or opened as a page you print.
- ⌨️ **Quick to fill in.** Enter adds a score and moves on to the next student, and a class list pasted from a document or spreadsheet adds all its students at once.
- ↩️ **Undo and redo.** For everything you change, from the keyboard or with the buttons in the corner. A bar offers to take back what you just did.
- 🛟 **Your data is safe.** Every change is saved at once, in a way that can't leave a half-written file behind. The first start of each day keeps a copy, the latest seven are kept, and Backup and Restore move the whole gradebook as one file.
- 🗓️ **A new school year in one click.** It keeps a copy of the year that ends, clears the scores and moves the students up a grade.
- 🌗 **Light and dark.** It follows your system's setting, and a button in the corner switches it. GradeBook remembers your choice.
- ⚡ **Light on your computer.** A native Rust app built with [Slint](https://slint.dev): no browser engine and no GPU needed. On Linux the window opens in about half a second and the app idles at around 20 MB of memory.
- 🔒 **Private.** It works offline and never sends your data anywhere. The one thing that goes online is the look for a new version, which reads a version number from GitHub and sends nothing of yours. You can turn it off.
- 🔔 **Tells you about updates.** GradeBook looks on GitHub for a newer release each time it starts. When there is one, a bar with a **Download** button appears above the tabs.
- 📦 **Installers for every system.** A Windows `.exe`, a macOS universal `.dmg`, and a Linux `.deb` and AppImage.

## Install

Download the installer for your system from the [Releases page](https://github.com/JMTDI/GradeBook/releases/latest).

| System | File | How to install |
|---|---|---|
| **Windows** 10/11 (64-bit) | `GradeBook-1.0.0-windows-x64-setup.exe` | Run it. It installs for your user only, so no administrator rights are needed. |
| **macOS** 11+ (Apple Silicon and Intel) | `GradeBook-1.0.0-macos-universal.dmg` | Open it and drag GradeBook to Applications. |
| **Debian, Ubuntu** and derivatives | `gradebook_1.0.0-1_amd64.deb` | `sudo apt install ./gradebook_1.0.0-1_amd64.deb` |
| **Any other 64-bit Linux** | `GradeBook-1.0.0-x86_64.AppImage` | `chmod +x` it and run it. If it doesn't start, install `libfuse2` or run it with `--appimage-extract-and-run`. |

> [!NOTE]
> The installers aren't code-signed, so your system may warn you the first time you open GradeBook.
> - **Windows:** SmartScreen may say "Windows protected your PC". Choose **More info → Run anyway**.
> - **macOS:** the app isn't notarized, so the first time, right-click it and choose **Open** (or run `xattr -dr com.apple.quarantine /Applications/GradeBook.app`).

GradeBook tells you when a new version is out (see [Updates](#updates)). Close it, then install the
new version the way you installed the first one. Your gradebook is kept in its own folder, which
installing doesn't touch.

Backup and Restore open your system's file chooser. On Linux that is the desktop portal
(`xdg-desktop-portal`), falling back to `zenity`.

## Using the app

Four screens, with a tab bar at the bottom to switch between them. In the top-left corner, **Undo**
and **Redo** take back and bring back your changes; in the top-right corner, the moon or sun button
switches between light and dark.

### Grades

Choose a grade level, a period (1–6) and a subject. Every student in that grade level gets a row.
Choose **Homework**, **Test** or **Quiz** (or a category of your own), type a score and press
<kbd>Enter</kbd>: the score is added and the keyboard moves on to the next student's field, so a
whole class goes in without touching the mouse.

A score can be typed as:

| Type | To mean |
|---|---|
| `92.5` or `92,5` | 92.5 percent |
| `18/20` | points: 18 out of 20, which is 90 percent |
| `+5` | extra credit: five bonus points added to the total of its kind |
| `EX` | excused: shown, but not counted for or against the student |

Scores appear as chips tinted by kind (grey for homework, red for tests, blue for quizzes, and a
colour of its own for each category you add) with their letter grade. Click a chip to change its
score or kind, or to delete it; the red × beside it deletes it at once. Under the chips is the
student's weighted average with its letter grade, and the class average is at the bottom.

Two buttons sit under each student:

- **What do I need?** Pick the average you want, such as a B, and choose homework, test or quiz:
  GradeBook tells you the lowest score on the next one that gets there. If nothing can, it says so.
- **Report Card** shows every subject the student has scores in, period by period, with their
  overall average. **Save as CSV** writes it as a spreadsheet and **Printable Report** opens it as a
  page in your browser, where you can print it or save it as a PDF.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/grades-dark.png?v=4">
    <img src="docs/screenshots/grades.png?v=4" alt="The Grades screen: every student in a grade level with their scores, averages and letter grades, and a struck-through score that is left out of the average" width="640">
  </picture>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/needed-dark.png?v=4">
    <img src="docs/screenshots/needed.png?v=4" alt="What does Grace Hopper need? The score she needs on her next test to reach a C+" width="400">
  </picture>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/report-dark.png?v=4">
    <img src="docs/screenshots/report.png?v=4" alt="A report card: every subject, period by period, with the overall average" width="400">
  </picture>
</p>

### Students

Add a student with a name and a grade level, or use **Paste a Class List** to add a whole class at
once: one name per line, with numbers or bullets at the start of a line ignored. A column copied
from a spreadsheet works too, and you see how many students will be added before you confirm.

The pencil beside a student changes their name or grade level. To delete a student, hover over the
row and click the red minus (or focus the row and press <kbd>Delete</kbd>), then confirm with the
red **Delete** button. Deleting a student deletes their scores too, and **Undo** brings them back.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/students-dark.png?v=4">
    <img src="docs/screenshots/students.png?v=4" alt="The Students screen: add a student or paste a class list, and the list of students" width="640">
  </picture>
</p>

### Insights

A summary of the class you chose on the Grades screen: the class average, the highest and the lowest
average (with whose they are), and the median; how many students have each letter, A to F; the
students under an average you choose with the slider (70 to begin with, and GradeBook remembers it);
and the class average in each of the six periods, to see whether the class is going up or down.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/insights-dark.png?v=4">
    <img src="docs/screenshots/insights.png?v=4" alt="The Insights screen: the class average, the best and the lowest, how the letters are spread, who needs attention, and the class average by period" width="640">
  </picture>
</p>

### Settings

- **Assignment weights.** Set how much homework, tests, quizzes and each category of your own count
  (50 / 17 / 33 for the first three to begin with; the total is shown). They apply to every subject
  until you choose one subject in the list at the top and give it weights of its own: Spelling
  might count quizzes more than Math does. **Use the Default Weights** takes a subject back. Each
  kind also has a switch to **leave out the lowest score** of that kind in the averages, which
  shows on the Grades screen as a struck-through chip.
- **Categories.** Add kinds of scores of your own, such as Project or Participation. A new one has no
  weight until you give it one. You can rename or remove them; removing one that has scores asks first.
- **Subjects.** The 20 usual subjects are always there. Add the ones you teach that aren't.
- **Grade levels.** Grades 1 to 10 are always there. Add Kindergarten or any grade from 11 to 20.
- **Your data.** Backup, Restore, **Open Data Folder**, **Export Scores (CSV)** (every score in the
  gradebook as a spreadsheet), the automatic backups, and **Start a New School Year**. More on these
  [below](#your-data).
- **About.** The version.
- **Updates.** A switch for the look for a new version when GradeBook starts (on to begin with),
  and **Check for Updates** to look right away. More on this [below](#updates).

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/settings-dark.png?v=4">
    <img src="docs/screenshots/settings.png?v=4" alt="The Settings screen: the weights, with a switch to leave out the lowest score of each kind" width="640">
  </picture>
</p>

### Undo, redo and the keyboard

Everything you change can be undone: scores, students, weights, categories, subjects, a restore and
even a new school year. GradeBook keeps the last 100 steps until you quit. **Undo** and **Redo** in the
corner do it with the mouse, and a bar at the bottom offers to take back the change you just made.

| To | Press |
|---|---|
| Undo | <kbd>Ctrl</kbd>+<kbd>Z</kbd> (<kbd>⌘</kbd>+<kbd>Z</kbd> on a Mac) |
| Redo | <kbd>Ctrl</kbd>+<kbd>Y</kbd> or <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> (<kbd>⌘</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> on a Mac) |
| Add a score and go to the next student | <kbd>Enter</kbd> in a student's score field |
| Close a dialog without saving | <kbd>Esc</kbd> |

In a field with text in it, <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes your typing first, as it always does;
in an empty field it undoes the last change to the gradebook. While a dialog is open, nothing behind
it can be undone.

### How averages work

The scores of each kind are averaged first, then combined using the weights. The weights are
re-balanced over the kinds a student has scores for, so a student with only tests is graded on tests
alone. Extra credit adds its points to the total of its kind (`+5` on top of three scores raises that
kind's average by about 1.7), and only once the kind has a score to add it to. Excused scores are left
out. If a kind is set to leave out the lowest score, that is done when a student has at least two.

For example, Ada in the Grades screenshot above has tests of 96 and 92.5 (average 94.25), quizzes of
88 and 100 (average 94) and homework that averages 96.07 once her lowest score, the 84, is left out
(her `EX` isn't counted). This gradebook gives tests 45, quizzes 15, homework 30 and projects 10,
but Ada's only Project entry is `+5` of extra credit, with no project score to add it to, so projects
don't count for her and the weights re-balance over the other three kinds:
(45 × 94.25 + 15 × 94 + 30 × 96.07) ÷ 90 ≈ 94.8, an A−.

### Letter grades

Each letter starts at the score shown, so 91.5 is a B and 99.5 is an A. The letter next to an
average always matches the rounded number displayed.

| Letter | A+ | A | A− | B+ | B | B− | C+ | C | C− | D | E | F |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **From** | 100 | 96 | 94 | 92 | 88 | 86 | 84 | 79 | 76 | 70 | 63 | 0 |

> [!TIP]
> GradeBook follows your system's light or dark setting. To switch, click the moon or sun button in the top-right corner of any screen; GradeBook remembers your choice. To start in one or the other no matter what, set the `GRADEBOOK_THEME` environment variable to `light` or `dark`.

## Your data

GradeBook saves after every change, in `gradebook.json` in:

| System | Folder |
|---|---|
| Windows | `%APPDATA%\jmtditech\gradebook\data` |
| macOS | `~/Library/Application Support/com.jmtditech.gradebook` |
| Linux | `~/.local/share/gradebook` |

**Open Data Folder** in Settings takes you there. `gradebook --data-dir` prints the exact folder on
Linux (on macOS run `/Applications/GradeBook.app/Contents/MacOS/gradebook --data-dir`; the Windows
build has no console window, so use the folder above). Set `GRADEBOOK_DATA_DIR` to keep the data
somewhere else (for example on a USB stick). Uninstalling never deletes it.

A small `settings.json` next to `gradebook.json` remembers your light or dark choice, the line you
drew on the Insights screen and whether to look for updates. It isn't part of a Backup.

The file format is the one earlier versions used, so their files open as they are. A file only gets
new entries (your own categories, subjects and grade levels, and the grading options) once you use
those features.

### Automatic backups

The first time you start GradeBook each day, it copies your data into the `backups` folder next to
`gradebook.json`, as `gradebook-<date>.json`, and keeps the seven newest. To go back to one, choose
it with **Restore**. A damaged data file is never copied.

### Backup and Restore

In Settings, **Backup** writes a `GradeBookBackup.json` file and **Restore** reads one, so you can
keep a copy of your gradebook or move it to another computer. Restoring replaces everything, so the
app asks first and saves a copy of the data it replaces next to `gradebook.json`, as
`gradebook.before-restore-<time>.json` (one per restore, never overwritten). If that copy can't be
saved, the restore doesn't go ahead. You can also undo a restore until you quit.

### A new school year

**Start a New School Year** (Settings, under Your Data) saves a copy of everything as
`backups/school-year-<date>.json`, which is never deleted or overwritten, then clears every score and
moves each student up a grade. Students in the highest grade stay where they are, and your weights,
categories and subjects are kept. If the copy can't be saved, nothing changes. Until you quit, you
can undo it.

### Spreadsheets and reports

**Export Scores (CSV)** writes every score in the gradebook, one per line, with its student,
subject, period, kind, percentage and letter, and whether it counts. A report card has its own CSV
and a printable page; the page is saved in your computer's temporary folder and opened in your
browser, and GradeBook deletes the ones that are more than a day old when it starts.
Both CSV files open in Excel, Numbers and Google Sheets.

### Updates

Each time it starts, GradeBook looks on GitHub for a newer release. When there is one, a bar appears
above the tabs with its version number and a **Download** button, which opens the release page in
your browser, where the installers are. The cross hides the bar until GradeBook starts again.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/update-dark.png?v=4">
    <img src="docs/screenshots/update.png?v=4" alt="The bar above the tabs that says GradeBook 9.0.0 is available, with a Download button and a cross that hides it" width="640">
  </picture>
</p>

Under **Updates** in Settings, a switch turns the look at start off, and **Check for Updates** looks
right away and says what it found. That brings the bar back if you had hidden it.

The look asks GitHub for the latest release of this project, with `curl`, or with `wget` where there
is no `curl` (Windows, macOS and most Linux systems have one). It only reads the version number:
nothing about you or your gradebook is sent, although GitHub sees the request as it sees any visit
to a web page. Nothing is downloaded or installed by itself. Without a network connection GradeBook
says nothing, and **Check for Updates** tells you it couldn't look.

### If something goes wrong

If the data file can't be read (for example, it was damaged), GradeBook moves it aside as
`gradebook.unreadable-<time>.json`, tells you, and starts with an empty gradebook. It never
overwrites that file.

## Build from source

You need [Rust](https://rustup.rs) 1.92 or newer. On Linux you also need `pkg-config` and
`libfontconfig1-dev` (Debian/Ubuntu names).

```sh
cargo run --release     # build and start the app
cargo test              # logic, headless UI interaction tests and screenshots
```

The tests drive the real UI without a window: they click, type, drag sliders and read what's on
screen, and write PNG screenshots of every screen to `target/tmp/screenshots`.

`cargo run --example make_icons` regenerates the icons in `assets/icons` from `assets/AppIcon.png`.

<details>
<summary><b>Linux run-time libraries</b></summary>

At run time on Linux the app loads the system's X11 or Wayland libraries (the `.deb` depends on
them): `libx11-6`, `libx11-xcb1`, `libxcb1`, `libxcb-xkb1`, `libxcursor1`, `libxext6`,
`libxfixes3`, `libxi6`, `libxrender1`, `libxkbcommon0` and `libxkbcommon-x11-0`, plus
`libwayland-client0` and `libwayland-cursor0` on Wayland.

</details>

<details>
<summary><b>Build the installers</b></summary>

Each one is built on its own operating system; the GitHub Actions workflow
([`desktop.yml`](.github/workflows/desktop.yml)) does all three.

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

To publish a release, see *Release a new version* below.

</details>

<details>
<summary><b>Release a new version</b></summary>

A release is made by hand, and you choose its version number:

1. On GitHub, open **Actions**, pick **Desktop app** in the list on the left and click **Run workflow**.
2. Type the version: three numbers, like `1.2.0`.
3. Click the green **Run workflow** button.

The workflow builds the installers on Windows, macOS and Linux, checks that each one reports the
version you typed, then publishes a GitHub release named `v1.2.0` with the four installers, a
`SHA256SUMS.txt` file and release notes that it writes itself. If anything fails, nothing is
published. Pushes and pull requests build the installers too, but publish nothing: find them under
**Artifacts** on the run's page.

A published release reaches people by itself: every GradeBook that looks for updates (see
[Updates](#updates)) shows the bar for it the next time it starts. Drafts and pre-releases aren't
offered.

The other choices in the form:

| Choice | What it does |
|---|---|
| **Publish a GitHub release** | Untick it to only build and read the release notes in the run summary. Nothing is released. |
| **Mark the release as a pre-release** | Flags the release as a pre-release, so it isn't shown as the latest one. |
| **Create the release as a draft** | Creates the release unpublished, so you can read and edit the notes before you publish it. |

From a terminal: `gh workflow run desktop.yml -f version=1.2.0` (add `-f prerelease=true`,
`-f draft=true` or `-f publish=false` for the other choices).

**The release notes** list the commits since the previous release (the newest version tag below the
one you're releasing), under a table of download links and above a list of SHA-256 checksums. Each
commit goes under ✨ What's new, 🎨 Improvements, 🐛 Fixes, 📚 Documentation, 🏗️ Build and
packaging, ✅ Tests or 🔧 Other changes, so the first words of a commit message matter: *Add…* lands
under What's new, *Fix…* under Fixes, and *Make…* or *Update…* under Improvements. Prefixes such as
`feat:`, `fix:` and `docs:` work too. A commit that only touches the README, `docs/`, `tests/` or
the build files goes under Documentation, Tests or Build and packaging. Put `[skip notes]` in a
commit message to leave it out; merge commits are never listed.

Good to know:

- **The button appears once this workflow is on the default branch.** After that, *Use workflow
  from* chooses the branch to release, and the release is that branch's latest commit.
- **The version isn't saved in the code.** The workflow sets it while building, so `Cargo.toml`
  keeps the version used for local builds, and the `v1.2.0` tag points to a commit that doesn't say
  1.2.0. To build a version on your own computer, run `packaging/set-version.sh 1.2.0` first (it
  edits `Cargo.toml` and `Cargo.lock`).
- **A version is released once.** If its tag or release already exists, the run stops straight away.
- **The app id** is `com.jmtditech.gradebook`. It is set in `src/lib.rs` and in each installer's
  files, and `cargo test` fails if they disagree, so change them together.

</details>

<details>
<summary><b>Code signing</b></summary>

Nothing is signed or notarized, which is why Windows and macOS show a warning on first launch.
Removing it takes a certificate you own: an Authenticode certificate for Windows, and an Apple
Developer ID plus notarization for macOS. The macOS app is signed ad hoc, which Apple Silicon
needs in order to run it at all.

</details>

## Project layout

```
src/        model.rs, calc.rs, book.rs, view.rs  the gradebook logic (no UI)
            export.rs                            CSV files and the printable report
            store.rs, prefs.rs                   the data file, backups and settings.json
            update.rs, system.rs                 the update check, opening folders and pages
            app.rs, app/                         connects the UI to the logic
ui/         *.slint                              the screens, dialogs and controls
tests/      interaction.rs, screenshots.rs       headless UI tests
            packaging.rs                         checks on the installer files
packaging/  linux/, macos/, windows/             installers
            set-version.sh, release-notes.py     used by the release workflow
assets/     icons/                               the app icons
examples/   make_icons.rs                        regenerates assets/icons
docs/       screenshots/, demo-*.gif             the images in this README
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
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-light.png">
  <a href="https://slint.dev"><img src="https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-dark.png" alt="Made with Slint" width="160"></a>
</picture>

</div>
