//! Things the operating system does for the app: opening a folder, a file or a web page.

use std::ffi::OsStr;
use std::io;
use std::process::{Command, Stdio};

/// Opens a folder, a file or a web address with whatever the system uses for it. It
/// returns as soon as the program that does the opening has started.
pub fn open(target: &OsStr) -> io::Result<()> {
    // Nothing passed in here starts with a dash, and a target that did would be read as an
    // option by the opening program.
    if target.as_encoded_bytes().starts_with(b"-") {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a path or address"));
    }
    let mut command = opener(target);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = command.spawn()?;
    // Collects the program's exit, so it doesn't linger, without making the window wait.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(target_os = "windows")]
fn opener(target: &OsStr) -> Command {
    let mut command = Command::new("explorer");
    command.arg(target);
    command
}

#[cfg(target_os = "macos")]
fn opener(target: &OsStr) -> Command {
    let mut command = Command::new("open");
    command.arg(target);
    command
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn opener(target: &OsStr) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(target);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_that_looks_like_an_option_is_refused() {
        for target in ["-h", "--version", "-"] {
            let error = open(OsStr::new(target)).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{target}");
        }
    }
}
