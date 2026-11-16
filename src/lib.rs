//! GradeBook for Windows, macOS and Linux: a native Rust (Slint) app that keeps track of
//! students' grades, with weighted averages and letter grades.

pub mod app;
pub mod book;
pub mod calc;
pub mod cli;
pub mod model;
pub mod prefs;
pub mod store;
pub mod ui;
pub mod view;

/// The app's reverse-DNS id, which everything that identifies GradeBook to the system is
/// named after: the Linux desktop file, icon and window class, the macOS bundle identifier,
/// the Windows uninstall entry and, split at the dots, the folder the data is kept in.
/// `tests/packaging.rs` checks that the packaging files use the same id.
pub const APP_ID: &str = "com.jmtditech.gradebook";
