//! Command-line flags that print something and exit instead of opening a window.
//! They also let packaged builds be smoke-tested on a machine without a display.

use crate::store;

const HELP: &str = "\
GradeBook: a gradebook for teachers and homeschoolers.

Usage: gradebook [OPTION]

Options:
  -V, --version   Print the version and exit
      --data-dir  Print the folder where GradeBook keeps its data and exit
  -h, --help      Print this help and exit

Environment:
  GRADEBOOK_DATA_DIR  Keep the data in this folder instead (handy for a portable copy)
  GRADEBOOK_THEME     'light' or 'dark' to override the system appearance";

/// The text to print for an informational flag in `args` (the arguments after the
/// program name), or `None` if the app should just start. Unknown arguments are
/// ignored: some platforms pass extra ones when an app is launched from the desktop.
pub fn info(args: impl IntoIterator<Item = String>) -> Option<String> {
    match args.into_iter().next()?.as_str() {
        "-V" | "--version" => Some(format!("GradeBook {}", env!("CARGO_PKG_VERSION"))),
        "-h" | "--help" => Some(HELP.to_string()),
        "--data-dir" => Some(
            store::default_data_dir()
                .map(|dir| dir.display().to_string())
                .unwrap_or_else(|| "(no data folder available)".to_string()),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Option<String> {
        info(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn version_and_help_are_printed() {
        let version = run(&["--version"]).unwrap();
        assert_eq!(version, format!("GradeBook {}", env!("CARGO_PKG_VERSION")));
        assert_eq!(run(&["-V"]), Some(version));
        assert!(run(&["--help"]).unwrap().contains("--data-dir"));
        assert_eq!(run(&["-h"]), run(&["--help"]));
        assert!(!run(&["--data-dir"]).unwrap().is_empty());
    }

    #[test]
    fn no_flags_or_unknown_flags_start_the_app() {
        assert_eq!(run(&[]), None);
        assert_eq!(run(&["-psn_0_12345"]), None);
        assert_eq!(run(&["something"]), None);
    }
}
