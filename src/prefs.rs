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
    /// Whether to look for a newer GradeBook when the app starts. `None` is the default, which
    /// is to look; only a choice made in Settings is written down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_for_updates: Option<bool>,
    /// The Insights screen lists the students whose average is below this. `None` is the
    /// default, [`crate::view::DEFAULT_ATTENTION`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_below: Option<u32>,
}

impl Preferences {
    /// Whether GradeBook looks for a newer version when it starts.
    pub fn checks_for_updates(&self) -> bool {
        self.check_for_updates.unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looking_for_updates_is_the_default_until_it_is_turned_off() {
        let none: Preferences = serde_json::from_str("{}").unwrap();
        assert!(none.checks_for_updates(), "a settings file without the choice");
        assert!(Preferences::default().checks_for_updates());

        let off: Preferences = serde_json::from_str(r#"{"check_for_updates": false}"#).unwrap();
        assert!(!off.checks_for_updates());
        let on: Preferences = serde_json::from_str(r#"{"check_for_updates": true}"#).unwrap();
        assert!(on.checks_for_updates());

        let written = serde_json::to_string(&Preferences {
            check_for_updates: Some(false),
            ..Preferences::default()
        })
        .unwrap();
        assert_eq!(written, r#"{"check_for_updates":false}"#);
    }
}
