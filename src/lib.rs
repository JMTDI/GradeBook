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

/// Reverse-DNS application id: the Linux desktop file, window class and icon name, and
/// the macOS bundle identifier (in lower case) all derive from it.
pub const APP_ID: &str = "io.github.JMTDI.GradeBook";
