// Hide the console window on Windows release builds.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use std::io::Write;

fn main() -> Result<(), slint::PlatformError> {
    if let Some(text) = gradebook::cli::info(std::env::args().skip(1)) {
        // `println!` would panic if stdout is closed early (`gradebook --help | head`).
        let _ = writeln!(std::io::stdout(), "{text}");
        return Ok(());
    }
    gradebook::app::run()
}
