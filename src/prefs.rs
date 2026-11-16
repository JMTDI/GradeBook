//! Preferences that belong to the person using the app rather than to the gradebook, such as
//! light or dark. They are kept in their own small file next to the data file, so they never
//! end up in a Backup file and a Restore never replaces them.

use serde::{Deserialize, Serialize};

/// Which colour scheme the app uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    /// Follow the system's light or dark setting.
    System,
    Light,
    Dark,
}

impl Appearance {
    /// Reads `GRADEBOOK_THEME` (`light` or `dark`); anything else follows the system.
    pub fn from_env() -> Appearance {
        match std::env::var("GRADEBOOK_THEME").ok().as_deref() {
            Some("light") => Appearance::Light,
            Some("dark") => Appearance::Dark,
            _ => Appearance::System,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
    /// The choice made with the light/dark button. `None` until the button has been used,
    /// which means following the system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
}
