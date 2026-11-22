//! Persistence: the app's data file, Backup/Restore files, and the preferences file.
//!
//! The data file uses the same JSON as a backup ([`BackupData`]). A file that can't be
//! read is moved aside (never overwritten) and reported to the caller, rather than
//! being silently ignored.
//!
//! Next to it, a `backups` folder holds a copy of the data from the first start of each day
//! (the last seven days are kept) and an archive of every school year that was ended.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::NaiveDate;
use serde::Serialize;

use crate::model::BackupData;
use crate::prefs::Preferences;

pub const DATA_FILE: &str = "gradebook.json";

/// The preferences (see [`Preferences`]), next to the data file.
pub const SETTINGS_FILE: &str = "settings.json";

/// The folder, next to the data file, with the automatic backups and the school-year archives.
pub const BACKUP_DIR: &str = "backups";

/// How many days of automatic backups are kept.
pub const DAILY_BACKUPS_KEPT: usize = 7;

/// A daily backup is `gradebook-<year>-<month>-<day>.json`.
const DAILY_PREFIX: &str = "gradebook-";

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "{e}"),
            StoreError::Json(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Json(e)
    }
}

/// Where the data file lives: `$GRADEBOOK_DATA_DIR` if set (handy for portable use
/// and tests), otherwise the platform's per-user application data directory, named after
/// [`crate::APP_ID`].
pub fn default_data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("GRADEBOOK_DATA_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let (qualifier, organization, application) = id_parts(crate::APP_ID)?;
    directories::ProjectDirs::from(qualifier, organization, application).map(|dirs| dirs.data_dir().to_path_buf())
}

/// Splits a reverse-DNS id like `com.jmtditech.gradebook` into the three names the
/// `directories` crate builds a folder from: `com.jmtditech.gradebook` in
/// `~/Library/Application Support` on macOS, `jmtditech\gradebook\data` in `%APPDATA%` on
/// Windows, and `gradebook` in `~/.local/share` on Linux.
fn id_parts(id: &str) -> Option<(&str, &str, &str)> {
    let mut parts = id.splitn(3, '.');
    Some((parts.next()?, parts.next()?, parts.next()?))
}

#[derive(Debug)]
pub enum LoadOutcome {
    /// First run: nothing stored yet.
    Missing,
    Loaded(BackupData),
    /// The file exists but could not be used. It was moved to `moved_to` when possible.
    Unreadable {
        error: String,
        moved_to: Option<PathBuf>,
    },
}

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Store { path: path.into() }
    }

    pub fn in_dir(dir: &Path) -> Self {
        Store::new(dir.join(DATA_FILE))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> LoadOutcome {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return LoadOutcome::Missing,
            Err(e) => return self.unreadable(e.to_string()),
        };
        match serde_json::from_slice::<BackupData>(&bytes) {
            Ok(data) => LoadOutcome::Loaded(data),
            Err(e) => self.unreadable(e.to_string()),
        }
    }

    /// The file is there but can't be used. It is moved out of the way so that the next
    /// save can't overwrite it.
    fn unreadable(&self, error: String) -> LoadOutcome {
        LoadOutcome::Unreadable {
            error,
            moved_to: self.move_aside(),
        }
    }

    pub fn save(&self, data: &BackupData) -> Result<(), StoreError> {
        write_json_atomically(&self.path, data)
    }

    /// The saved preferences. A missing or unreadable file just means the defaults:
    /// preferences are never worth interrupting anyone for.
    pub fn load_preferences(&self) -> Preferences {
        fs::read(self.settings_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save_preferences(&self, preferences: &Preferences) -> Result<(), StoreError> {
        write_json_atomically(&self.settings_path(), preferences)
    }

    fn settings_path(&self) -> PathBuf {
        self.path.with_file_name(SETTINGS_FILE)
    }

    /// Copies the current data file next to itself as `gradebook.before-restore-<time>.json`
    /// and returns the copy's path, or `None` if there is no data file yet. An earlier copy
    /// is never overwritten: restoring twice must not destroy the original data.
    pub fn copy_before_restore(&self) -> Result<Option<PathBuf>, StoreError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let copy = self.unused_sibling("before-restore");
        fs::copy(&self.path, &copy)?;
        Ok(Some(copy))
    }

    /// The folder the data file is in.
    pub fn dir(&self) -> &Path {
        match self.path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        }
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.dir().join(BACKUP_DIR)
    }

    /// Copies the data file to `backups/gradebook-<day>.json`, unless that day has a copy
    /// already, and deletes the daily copies beyond the last [`DAILY_BACKUPS_KEPT`]. The
    /// file is copied as it is, so a copy never "repairs" anything. Returns the new copy's
    /// path, or `None` if there was nothing to copy or the day was covered already.
    pub fn daily_backup(&self, day: NaiveDate) -> Result<Option<PathBuf>, StoreError> {
        if !self.path.is_file() {
            return Ok(None);
        }
        let dir = self.backups_dir();
        let target = dir.join(format!("{DAILY_PREFIX}{day}.json"));
        let created = if target.exists() {
            None
        } else {
            fs::create_dir_all(&dir)?;
            let tmp = dir.join(format!("{DAILY_PREFIX}{day}.json.tmp"));
            if let Err(e) = fs::copy(&self.path, &tmp).and_then(|_| fs::rename(&tmp, &target)) {
                let _ = fs::remove_file(&tmp);
                return Err(e.into());
            }
            Some(target)
        };
        for (_, old) in self.daily_backups().into_iter().skip(DAILY_BACKUPS_KEPT) {
            let _ = fs::remove_file(old);
        }
        Ok(created)
    }

    /// The daily backups, newest first.
    pub fn daily_backups(&self) -> Vec<(NaiveDate, PathBuf)> {
        let Ok(entries) = fs::read_dir(self.backups_dir()) else {
            return Vec::new();
        };
        let mut found: Vec<(NaiveDate, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let day = name.strip_prefix(DAILY_PREFIX)?.strip_suffix(".json")?;
                let day = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
                Some((day, entry.path()))
            })
            .collect();
        found.sort_by_key(|(day, _)| std::cmp::Reverse(*day));
        found
    }

    /// Saves `data` as `backups/school-year-<day>.json`, where it stays: unlike the daily
    /// copies it is never deleted, and an earlier one is never overwritten.
    pub fn archive_school_year(&self, data: &BackupData, day: NaiveDate) -> Result<PathBuf, StoreError> {
        let dir = self.backups_dir();
        let mut target = dir.join(format!("school-year-{day}.json"));
        let mut counter = 2;
        while target.exists() {
            target = dir.join(format!("school-year-{day}-{counter}.json"));
            counter += 1;
        }
        write_json_atomically(&target, data)?;
        Ok(target)
    }

    fn move_aside(&self) -> Option<PathBuf> {
        let target = self.unused_sibling("unreadable");
        fs::rename(&self.path, &target).ok().map(|_| target)
    }

    /// A path next to the data file, named `gradebook.<label>-<unix seconds>.json`, that
    /// doesn't exist yet (a counter is added when several are made in the same second).
    fn unused_sibling(&self, label: &str) -> PathBuf {
        let stem = self
            .path
            .file_stem()
            .map_or_else(|| "gradebook".to_string(), |s| s.to_string_lossy().into_owned());
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut candidate = self.path.with_file_name(format!("{stem}.{label}-{stamp}.json"));
        let mut counter = 2;
        while candidate.exists() {
            candidate = self
                .path
                .with_file_name(format!("{stem}.{label}-{stamp}-{counter}.json"));
            counter += 1;
        }
        candidate
    }
}

pub fn write_backup_file(path: &Path, data: &BackupData) -> Result<(), StoreError> {
    write_json_atomically(path, data)
}

pub fn read_backup_file(path: &Path) -> Result<BackupData, StoreError> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir)?;
        }
    }
    let json = serde_json::to_vec_pretty(value)?;
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    let result = write_then_replace(&tmp, path, &json);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(result?)
}

/// Writes `json` to `tmp`, then renames it over `path`, so `path` always holds either
/// the old data or the complete new data.
fn write_then_replace(tmp: &Path, path: &Path, json: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(tmp)?;
    file.write_all(json)?;
    // Flush to disk before the rename, so a power cut can't leave an empty file where
    // the data used to be.
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GradeEntry, GradeType, Student};
    use uuid::Uuid;

    fn sample() -> BackupData {
        let student = Student {
            id: Uuid::new_v4(),
            name: "Ada".into(),
            grade_level: 4,
        };
        let grade = GradeEntry {
            id: Uuid::new_v4(),
            student_id: student.id,
            subject: "Math".into(),
            period: 1,
            value: "93.5".into(),
            grade_type: GradeType::Test,
        };
        BackupData {
            students: vec![student],
            grades: vec![grade],
            ..BackupData::default()
        }
    }

    #[test]
    fn the_data_folder_is_named_after_the_app_id() {
        assert_eq!(
            id_parts("com.jmtditech.gradebook"),
            Some(("com", "jmtditech", "gradebook"))
        );
        assert_eq!(id_parts("a.b.c.d"), Some(("a", "b", "c.d")), "the rest stays together");
        assert_eq!(id_parts("gradebook"), None);
        assert_eq!(id_parts("jmtditech.gradebook"), None);
        assert!(
            id_parts(crate::APP_ID).is_some(),
            "the real id must split, or there is nowhere to save"
        );
    }

    #[test]
    fn missing_file_is_a_first_run() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(Store::in_dir(dir.path()).load(), LoadOutcome::Missing));
    }

    #[test]
    fn preferences_are_kept_in_their_own_file_next_to_the_data() {
        use crate::prefs::Appearance;

        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        assert_eq!(store.load_preferences(), Preferences::default());

        let preferences = Preferences {
            appearance: Some(Appearance::Dark),
            ..Preferences::default()
        };
        store.save_preferences(&preferences).unwrap();
        assert_eq!(store.load_preferences(), preferences);
        assert_eq!(
            fs::read_to_string(dir.path().join(SETTINGS_FILE))
                .unwrap()
                .replace([' ', '\n'], ""),
            r#"{"appearance":"dark"}"#
        );
        assert!(
            matches!(store.load(), LoadOutcome::Missing),
            "saving preferences doesn't create or touch the gradebook"
        );
    }

    #[test]
    fn missing_or_damaged_preferences_load_as_the_defaults() {
        use crate::prefs::Appearance;

        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        let settings = dir.path().join(SETTINGS_FILE);

        fs::write(&settings, "{ not json").unwrap();
        assert_eq!(store.load_preferences(), Preferences::default());
        fs::write(&settings, r#"{"appearance": "sepia"}"#).unwrap();
        assert_eq!(store.load_preferences(), Preferences::default());

        // Fields added by a newer version are ignored rather than rejected.
        fs::write(&settings, r#"{"appearance": "light", "somethingNew": [1, 2]}"#).unwrap();
        assert_eq!(store.load_preferences().appearance, Some(Appearance::Light));
    }

    #[test]
    fn saves_and_loads_and_creates_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(&dir.path().join("nested").join("deeper"));
        let data = sample();
        store.save(&data).unwrap();
        match store.load() {
            LoadOutcome::Loaded(loaded) => assert_eq!(loaded, data),
            other => panic!("unexpected {other:?}"),
        }
        // The temp file used for the atomic write must not be left behind.
        assert!(!store.path().with_extension("json.tmp").exists());
    }

    #[test]
    fn unreadable_file_is_moved_aside_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        fs::write(store.path(), "{ definitely not json").unwrap();
        match store.load() {
            LoadOutcome::Unreadable {
                moved_to: Some(moved), ..
            } => {
                assert!(moved.exists());
                assert_eq!(fs::read_to_string(moved).unwrap(), "{ definitely not json");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(!store.path().exists());
        assert!(matches!(store.load(), LoadOutcome::Missing));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_moved_aside_too() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        // A folder where the file should be: reading it fails with an error other than
        // "not found", on every platform.
        fs::create_dir(store.path()).unwrap();
        match store.load() {
            LoadOutcome::Unreadable {
                moved_to: Some(moved), ..
            } => assert!(moved.exists()),
            other => panic!("unexpected {other:?}"),
        }
        assert!(!store.path().exists(), "so the next save can't overwrite it");
        store.save(&sample()).unwrap();
        assert!(matches!(store.load(), LoadOutcome::Loaded(_)));
    }

    #[test]
    fn a_failed_save_leaves_the_old_data_and_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        let before = sample();
        store.save(&before).unwrap();
        // A folder named like the temp file makes writing it fail.
        let tmp = dir.path().join(format!("{DATA_FILE}.tmp"));
        fs::create_dir(&tmp).unwrap();
        assert!(store.save(&BackupData::default()).is_err());
        assert!(matches!(store.load(), LoadOutcome::Loaded(data) if data == before));
        assert!(tmp.is_dir(), "the blocking folder is left alone");
    }

    #[test]
    fn restore_copies_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        assert_eq!(store.copy_before_restore().unwrap(), None, "nothing to copy yet");

        let first = sample();
        store.save(&first).unwrap();
        let first_copy = store.copy_before_restore().unwrap().unwrap();
        // Restoring a second time, within the same second, must not replace the first copy.
        let second = BackupData {
            test_weight: 40.0,
            ..sample()
        };
        store.save(&second).unwrap();
        let second_copy = store.copy_before_restore().unwrap().unwrap();
        store.save(&BackupData::default()).unwrap();

        assert_ne!(first_copy, second_copy);
        assert_eq!(read_backup_file(&first_copy).unwrap(), first);
        assert_eq!(read_backup_file(&second_copy).unwrap(), second);
        let name = first_copy.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("gradebook.before-restore-") && name.ends_with(".json"),
            "{name}"
        );
    }

    #[test]
    fn a_data_file_that_cannot_be_copied_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        // A folder where the data file should be can't be copied, on any platform.
        fs::create_dir(store.path()).unwrap();
        assert!(store.copy_before_restore().is_err());
    }

    #[test]
    fn unreadable_files_moved_aside_in_the_same_second_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        let mut moved = Vec::new();
        for n in 0..3 {
            fs::write(store.path(), format!("broken {n}")).unwrap();
            match store.load() {
                LoadOutcome::Unreadable {
                    moved_to: Some(path), ..
                } => moved.push(path),
                other => panic!("unexpected {other:?}"),
            }
        }
        moved.sort();
        moved.dedup();
        assert_eq!(moved.len(), 3, "each damaged file keeps its own copy");
        let mut contents: Vec<String> = moved.iter().map(|p| fs::read_to_string(p).unwrap()).collect();
        contents.sort();
        assert_eq!(contents, ["broken 0", "broken 1", "broken 2"]);
    }

    #[test]
    fn backup_files_round_trip_and_bad_files_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("GradeBookBackup.json");
        let data = sample();
        write_backup_file(&file, &data).unwrap();
        assert_eq!(read_backup_file(&file).unwrap(), data);

        fs::write(&file, r#"{"hello": "world"}"#).unwrap();
        assert!(matches!(read_backup_file(&file), Err(StoreError::Json(_))));
        assert!(matches!(
            read_backup_file(&dir.path().join("nope.json")),
            Err(StoreError::Io(_))
        ));
    }

    #[test]
    fn newer_preferences_are_only_written_when_they_are_used() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        store.save_preferences(&Preferences::default()).unwrap();
        let text = |store: &Store| {
            fs::read_to_string(store.settings_path())
                .unwrap()
                .replace([' ', '\n'], "")
        };
        assert_eq!(
            text(&store),
            "{}",
            "a version that doesn't know them can still read the file"
        );

        let preferences = Preferences {
            check_for_updates: Some(false),
            attention_below: Some(75),
            ..Preferences::default()
        };
        store.save_preferences(&preferences).unwrap();
        assert_eq!(text(&store), r#"{"check_for_updates":false,"attention_below":75}"#);
        assert_eq!(store.load_preferences(), preferences);
    }

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn the_first_start_of_a_day_makes_a_backup_and_later_ones_leave_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        assert_eq!(store.daily_backup(day("2026-11-20")).unwrap(), None, "no data yet");
        assert!(!store.backups_dir().exists(), "and no folder for nothing");

        let morning = sample();
        store.save(&morning).unwrap();
        let backup = store.daily_backup(day("2026-11-20")).unwrap().unwrap();
        assert_eq!(backup.file_name().unwrap(), "gradebook-2026-11-20.json");
        assert_eq!(backup.parent().unwrap(), store.backups_dir());
        assert_eq!(read_backup_file(&backup).unwrap(), morning);

        // Later that day the data has changed, but the day already has its copy.
        store.save(&BackupData::default()).unwrap();
        assert_eq!(store.daily_backup(day("2026-11-20")).unwrap(), None);
        assert_eq!(
            read_backup_file(&backup).unwrap(),
            morning,
            "the first copy of the day is kept"
        );
        assert!(
            !store.backups_dir().join("gradebook-2026-11-20.json.tmp").exists(),
            "no temp file is left"
        );
    }

    #[test]
    fn a_backup_is_a_byte_for_byte_copy() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        fs::write(store.path(), "  {\"students\": [], \"odd\": 1,\n \"testWeight\":50,\"quizWeight\":17,\"homeworkWeight\":33,\"grades\":[]}  ").unwrap();
        let backup = store.daily_backup(day("2026-01-02")).unwrap().unwrap();
        assert_eq!(fs::read(backup).unwrap(), fs::read(store.path()).unwrap());
    }

    #[test]
    fn only_the_last_seven_days_of_backups_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        store.save(&sample()).unwrap();
        for n in 1..=10 {
            store.daily_backup(day(&format!("2026-02-{n:02}"))).unwrap();
        }
        let kept: Vec<String> = store.daily_backups().iter().map(|(d, _)| d.to_string()).collect();
        assert_eq!(
            kept,
            [
                "2026-02-10",
                "2026-02-09",
                "2026-02-08",
                "2026-02-07",
                "2026-02-06",
                "2026-02-05",
                "2026-02-04"
            ],
            "newest first"
        );
        assert_eq!(fs::read_dir(store.backups_dir()).unwrap().count(), 7);
    }

    #[test]
    fn pruning_only_touches_daily_backups() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        store.save(&sample()).unwrap();
        let archive = store.archive_school_year(&sample(), day("2020-01-01")).unwrap();
        let note = store.backups_dir().join("notes.txt");
        let odd = store.backups_dir().join("gradebook-not-a-date.json");
        fs::write(&note, "mine").unwrap();
        fs::write(&odd, "{}").unwrap();
        for n in 1..=9 {
            store.daily_backup(day(&format!("2026-03-{n:02}"))).unwrap();
        }
        assert_eq!(store.daily_backups().len(), DAILY_BACKUPS_KEPT);
        assert!(
            archive.exists() && note.exists() && odd.exists(),
            "everything else stays"
        );
        assert!(store.path().exists(), "so does the data");
    }

    #[test]
    fn a_school_year_archive_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        let first = sample();
        let second = BackupData {
            test_weight: 10.0,
            ..sample()
        };
        let a = store.archive_school_year(&first, day("2027-06-01")).unwrap();
        let b = store.archive_school_year(&second, day("2027-06-01")).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.file_name().unwrap(), "school-year-2027-06-01.json");
        assert_eq!(b.file_name().unwrap(), "school-year-2027-06-01-2.json");
        assert_eq!(read_backup_file(&a).unwrap(), first);
        assert_eq!(read_backup_file(&b).unwrap(), second);
        assert!(store.daily_backups().is_empty(), "an archive isn't a daily backup");
    }

    #[test]
    fn an_archive_that_cannot_be_written_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::in_dir(dir.path());
        // A file where the backups folder should be.
        fs::write(store.backups_dir(), "in the way").unwrap();
        assert!(store.archive_school_year(&sample(), day("2027-06-01")).is_err());
        store.save(&sample()).unwrap();
        assert!(store.daily_backup(day("2027-06-01")).is_err());
    }

    #[test]
    fn the_data_folder_of_a_plain_file_name_is_the_current_folder() {
        assert_eq!(Store::new("gradebook.json").dir(), Path::new("."));
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Store::in_dir(dir.path()).dir(), dir.path());
    }
}
