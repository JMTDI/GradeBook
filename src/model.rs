//! Data model. Its JSON shape (camelCase keys, upper-case hyphenated ids, and `Test`,
//! `Quiz` and `Homework` grade types) is the file format of the data file and of Backup
//! files, so changing it would break files that already exist.
//!
//! Everything added since the first release is an optional field that is left out of the
//! file while it isn't used: a gradebook that uses none of the newer features is written
//! exactly as before, and a file written before they existed still loads.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// Ids are written as upper-case hyphenated UUIDs, as in existing files, so exported
/// files stay byte-for-byte comparable. Reading accepts either case.
mod upper_uuid {
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S: Serializer>(id: &Uuid, serializer: S) -> Result<S::Ok, S::Error> {
        let mut buf = Uuid::encode_buffer();
        serializer.serialize_str(id.hyphenated().encode_upper(&mut buf))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Uuid, D::Error> {
        Uuid::deserialize(deserializer)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Student {
    #[serde(with = "upper_uuid")]
    pub id: Uuid,
    pub name: String,
    pub grade_level: u32,
}

/// The category a score belongs to: one of the three every gradebook has, or one the user
/// added. It is written as a plain name, so a custom category is told apart by its name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GradeType {
    Homework,
    Test,
    Quiz,
    Custom(String),
}

impl GradeType {
    /// The categories every gradebook has, in the order they are listed everywhere in the UI.
    pub const ALL: [GradeType; 3] = [GradeType::Homework, GradeType::Test, GradeType::Quiz];

    pub fn label(&self) -> &str {
        match self {
            GradeType::Homework => "Homework",
            GradeType::Test => "Test",
            GradeType::Quiz => "Quiz",
            GradeType::Custom(name) => name,
        }
    }

    /// The category with this name: a built-in one if the name is exactly theirs.
    pub fn from_name(name: &str) -> GradeType {
        match name {
            "Homework" => GradeType::Homework,
            "Test" => GradeType::Test,
            "Quiz" => GradeType::Quiz,
            other => GradeType::Custom(other.to_string()),
        }
    }

    pub fn is_built_in(&self) -> bool {
        !matches!(self, GradeType::Custom(_))
    }
}

impl Serialize for GradeType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.label())
    }
}

impl<'de> Deserialize<'de> for GradeType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(GradeType::from_name(&String::deserialize(deserializer)?))
    }
}

/// What names are compared by: without spaces at the ends and in lower case, whatever the
/// alphabet, so that "Évaluation" and "évaluation" are one name.
fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Whether two names are the same name. Everything that keeps names apart goes by this, and so
/// does the check of a file when it is loaded: the app must never save a file that it would then
/// refuse to open.
pub fn same_name(a: &str, b: &str) -> bool {
    name_key(a) == name_key(b)
}

/// Whether `name` is one of the built-in categories (in any letter case), which a custom
/// category may not be called.
pub fn is_reserved_category_name(name: &str) -> bool {
    GradeType::ALL.iter().any(|built_in| same_name(built_in.label(), name))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradeEntry {
    #[serde(with = "upper_uuid")]
    pub id: Uuid,
    #[serde(with = "upper_uuid")]
    pub student_id: Uuid,
    pub subject: String,
    pub period: u32,
    /// Kept as text, exactly as typed: `92.5`, points as `18/20`, extra credit as `+5`, or
    /// `EX` for an excused score (see `calc::classify`).
    pub value: String,
    #[serde(rename = "type")]
    pub grade_type: GradeType,
}

/// A category the user added, next to Homework, Test and Quiz.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomCategory {
    pub name: String,
    pub weight: f64,
}

/// How scores are combined, beyond the three default weights.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradingOptions {
    /// Categories whose lowest score is left out of the average (when there are at least
    /// two), in every subject and period.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drop_lowest: Vec<GradeType>,
    /// Weights for a subject that doesn't use the default ones: subject, then category name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subject_weights: BTreeMap<String, BTreeMap<String, f64>>,
}

impl GradingOptions {
    pub fn is_default(&self) -> bool {
        *self == GradingOptions::default()
    }
}

/// Everything in a gradebook except the students and their grades: the part that the
/// settings change.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub test_weight: f64,
    pub quiz_weight: f64,
    pub homework_weight: f64,
    pub categories: Vec<CustomCategory>,
    pub subjects: Vec<String>,
    pub grade_levels: Vec<u32>,
    pub options: GradingOptions,
}

impl Settings {
    /// Every category, in the order they are listed: the built-in ones, then the custom ones.
    pub fn category_list(&self) -> Vec<GradeType> {
        GradeType::ALL
            .into_iter()
            .chain(self.categories.iter().map(|c| GradeType::Custom(c.name.clone())))
            .collect()
    }

    /// The default weight of a category.
    pub fn weight(&self, grade_type: &GradeType) -> f64 {
        match grade_type {
            GradeType::Test => self.test_weight,
            GradeType::Quiz => self.quiz_weight,
            GradeType::Homework => self.homework_weight,
            GradeType::Custom(name) => self
                .categories
                .iter()
                .find(|c| c.name == *name)
                .map_or(0.0, |c| c.weight),
        }
    }

    pub fn set_weight(&mut self, grade_type: &GradeType, value: f64) {
        match grade_type {
            GradeType::Test => self.test_weight = value,
            GradeType::Quiz => self.quiz_weight = value,
            GradeType::Homework => self.homework_weight = value,
            GradeType::Custom(name) => {
                if let Some(category) = self.categories.iter_mut().find(|c| c.name == *name) {
                    category.weight = value;
                }
            }
        }
    }
}

/// Everything the app stores. Used both for the on-disk data file and for
/// Backup/Restore files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "RawBackupData")]
pub struct BackupData {
    pub students: Vec<Student>,
    pub test_weight: f64,
    pub quiz_weight: f64,
    pub homework_weight: f64,
    pub grades: Vec<GradeEntry>,
    /// Categories added by the user (see [`GradeType::Custom`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<CustomCategory>,
    /// Subjects added by the user, next to the built-in ones.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<String>,
    /// Grade levels added by the user, next to grades 1 to 10 (0 is Kindergarten).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grade_levels: Vec<u32>,
    #[serde(skip_serializing_if = "GradingOptions::is_default")]
    pub options: GradingOptions,
}

impl Default for BackupData {
    fn default() -> Self {
        BackupData {
            students: Vec::new(),
            test_weight: 50.0,
            quiz_weight: 17.0,
            homework_weight: 33.0,
            grades: Vec::new(),
            categories: Vec::new(),
            subjects: Vec::new(),
            grade_levels: Vec::new(),
            options: GradingOptions::default(),
        }
    }
}

/// The file as it is read, before it is checked.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBackupData {
    students: Vec<Student>,
    test_weight: f64,
    quiz_weight: f64,
    homework_weight: f64,
    grades: Vec<GradeEntry>,
    #[serde(default)]
    categories: Vec<CustomCategory>,
    #[serde(default)]
    subjects: Vec<String>,
    #[serde(default)]
    grade_levels: Vec<u32>,
    #[serde(default)]
    options: GradingOptions,
}

/// Why a file that parses is still not a usable gradebook.
#[derive(Debug, PartialEq)]
pub struct InvalidData(String);

impl fmt::Display for InvalidData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<RawBackupData> for BackupData {
    type Error = InvalidData;

    fn try_from(raw: RawBackupData) -> Result<Self, InvalidData> {
        let mut seen: Vec<String> = Vec::new();
        for category in &raw.categories {
            let key = name_key(&category.name);
            if key.is_empty() {
                return Err(InvalidData("a category has no name".into()));
            }
            if is_reserved_category_name(&category.name) || seen.contains(&key) {
                return Err(InvalidData(format!(
                    "the category \"{}\" is defined more than once",
                    category.name
                )));
            }
            seen.push(key);
        }
        let defined = |name: &str| raw.categories.iter().any(|c| c.name == name);
        if let Some(grade) = raw
            .grades
            .iter()
            .find(|g| matches!(&g.grade_type, GradeType::Custom(name) if !defined(name)))
        {
            return Err(InvalidData(format!(
                "a grade has the type \"{}\", which is not a known category",
                grade.grade_type.label()
            )));
        }

        let mut options = raw.options;
        let mut drop_lowest: Vec<GradeType> = Vec::new();
        for category in std::mem::take(&mut options.drop_lowest) {
            if (category.is_built_in() || defined(category.label())) && !drop_lowest.contains(&category) {
                drop_lowest.push(category);
            }
        }
        options.drop_lowest = drop_lowest;

        let mut subjects: Vec<String> = Vec::new();
        for subject in raw.subjects {
            let subject = subject.trim().to_string();
            if !subject.is_empty() && !subjects.iter().any(|s| same_name(s, &subject)) {
                subjects.push(subject);
            }
        }
        subjects.sort_by_key(|s| s.to_lowercase());

        let mut grade_levels = raw.grade_levels;
        grade_levels.sort_unstable();
        grade_levels.dedup();
        grade_levels.retain(|level| !(1..=10).contains(level));

        Ok(BackupData {
            students: raw.students,
            test_weight: raw.test_weight,
            quiz_weight: raw.quiz_weight,
            homework_weight: raw.homework_weight,
            grades: raw.grades,
            categories: raw.categories,
            subjects,
            grade_levels,
            options,
        })
    }
}

impl BackupData {
    /// Every category, in the order they are listed: the built-in ones, then the custom ones.
    pub fn category_list(&self) -> Vec<GradeType> {
        GradeType::ALL
            .into_iter()
            .chain(self.categories.iter().map(|c| GradeType::Custom(c.name.clone())))
            .collect()
    }

    /// The default weight of a category (a subject may use others, see
    /// `GradeBook::weight_in`).
    pub fn weight(&self, grade_type: &GradeType) -> f64 {
        match grade_type {
            GradeType::Test => self.test_weight,
            GradeType::Quiz => self.quiz_weight,
            GradeType::Homework => self.homework_weight,
            GradeType::Custom(name) => self
                .categories
                .iter()
                .find(|c| c.name == *name)
                .map_or(0.0, |c| c.weight),
        }
    }

    pub fn set_weight(&mut self, grade_type: &GradeType, value: f64) {
        match grade_type {
            GradeType::Test => self.test_weight = value,
            GradeType::Quiz => self.quiz_weight = value,
            GradeType::Homework => self.homework_weight = value,
            GradeType::Custom(name) => {
                if let Some(category) = self.categories.iter_mut().find(|c| c.name == *name) {
                    category.weight = value;
                }
            }
        }
    }

    pub fn settings(&self) -> Settings {
        Settings {
            test_weight: self.test_weight,
            quiz_weight: self.quiz_weight,
            homework_weight: self.homework_weight,
            categories: self.categories.clone(),
            subjects: self.subjects.clone(),
            grade_levels: self.grade_levels.clone(),
            options: self.options.clone(),
        }
    }

    pub fn apply_settings(&mut self, settings: Settings) {
        self.test_weight = settings.test_weight;
        self.quiz_weight = settings.quiz_weight;
        self.homework_weight = settings.homework_weight;
        self.categories = settings.categories;
        self.subjects = settings.subjects;
        self.grade_levels = settings.grade_levels;
        self.options = settings.options;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backup file in the file format: camelCase keys and upper-case ids.
    const BACKUP: &str = r#"{
        "students": [
            {"id": "E621E1F8-C36C-495A-93FC-0C247A3E6E5F", "name": "Ada", "gradeLevel": 3}
        ],
        "testWeight": 50,
        "quizWeight": 17,
        "homeworkWeight": 33,
        "grades": [
            {
                "id": "0A1B2C3D-0000-4000-8000-000000000001",
                "studentId": "E621E1F8-C36C-495A-93FC-0C247A3E6E5F",
                "subject": "Math",
                "period": 2,
                "value": "92",
                "type": "Quiz"
            }
        ]
    }"#;

    #[test]
    fn decodes_a_backup_file() {
        let data: BackupData = serde_json::from_str(BACKUP).unwrap();
        assert_eq!(data.students.len(), 1);
        assert_eq!(data.students[0].name, "Ada");
        assert_eq!(data.students[0].grade_level, 3);
        assert_eq!(data.test_weight, 50.0);
        assert_eq!(data.grades[0].grade_type, GradeType::Quiz);
        assert_eq!(data.grades[0].student_id, data.students[0].id);
        assert_eq!(data.grades[0].period, 2);
    }

    #[test]
    fn encodes_in_the_file_format() {
        let data: BackupData = serde_json::from_str(BACKUP).unwrap();
        let json: serde_json::Value = serde_json::to_value(&data).unwrap();
        // Upper-case UUIDs, camelCase keys and the grade type names as written in files.
        assert_eq!(json["students"][0]["id"], "E621E1F8-C36C-495A-93FC-0C247A3E6E5F");
        assert_eq!(json["students"][0]["gradeLevel"], 3);
        assert_eq!(json["grades"][0]["studentId"], "E621E1F8-C36C-495A-93FC-0C247A3E6E5F");
        assert_eq!(json["grades"][0]["type"], "Quiz");
        assert!(json.get("testWeight").is_some() && json.get("homeworkWeight").is_some());
    }

    #[test]
    fn a_gradebook_that_uses_no_newer_feature_is_written_with_the_original_keys() {
        let data: BackupData = serde_json::from_str(BACKUP).unwrap();
        let json: serde_json::Value = serde_json::to_value(&data).unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["grades", "homeworkWeight", "quizWeight", "students", "testWeight"],
            "an older version can still read it"
        );
    }

    #[test]
    fn round_trips_and_accepts_lower_case_uuids() {
        let lower = BACKUP.replace(
            "E621E1F8-C36C-495A-93FC-0C247A3E6E5F",
            "e621e1f8-c36c-495a-93fc-0c247a3e6e5f",
        );
        let data: BackupData = serde_json::from_str(&lower).unwrap();
        let again: BackupData = serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        assert_eq!(data, again);
    }

    #[test]
    fn rejects_unknown_grade_types() {
        let bad = BACKUP.replace("\"Quiz\"", "\"Exam\"");
        let error = serde_json::from_str::<BackupData>(&bad).unwrap_err().to_string();
        assert!(error.contains("Exam"), "{error}");
    }

    #[test]
    fn grade_types_are_listed_homework_test_quiz() {
        let labels: Vec<&str> = GradeType::ALL.iter().map(GradeType::label).collect();
        assert_eq!(labels, ["Homework", "Test", "Quiz"]);
        let data = BackupData::default();
        assert_eq!(data.category_list(), GradeType::ALL);
        for built_in in GradeType::ALL {
            assert_eq!(GradeType::from_name(built_in.label()), built_in);
        }
    }

    #[test]
    fn default_weights_are_50_17_33() {
        let d = BackupData::default();
        assert_eq!((d.test_weight, d.quiz_weight, d.homework_weight), (50.0, 17.0, 33.0));
    }

    const WITH_EXTRAS: &str = r#"{
        "students": [],
        "testWeight": 40, "quizWeight": 10, "homeworkWeight": 30,
        "grades": [
            {"id": "0A1B2C3D-0000-4000-8000-000000000001",
             "studentId": "E621E1F8-C36C-495A-93FC-0C247A3E6E5F",
             "subject": "Coding", "period": 1, "value": "18/20", "type": "Project"}
        ],
        "categories": [{"name": "Project", "weight": 20}],
        "subjects": ["Robotics", "coding", "Coding", " "],
        "gradeLevels": [12, 0, 12, 5],
        "options": {"dropLowest": ["Quiz", "Project", "Gone"],
                    "subjectWeights": {"Math": {"Test": 70, "Quiz": 30}}}
    }"#;

    #[test]
    fn newer_features_round_trip_and_are_tidied_on_load() {
        let data: BackupData = serde_json::from_str(WITH_EXTRAS).unwrap();
        assert_eq!(data.grades[0].grade_type, GradeType::Custom("Project".into()));
        assert_eq!(data.weight(&GradeType::Custom("Project".into())), 20.0);
        assert_eq!(data.subjects, ["coding", "Robotics"], "no blanks or repeats, sorted");
        assert_eq!(data.grade_levels, [0, 12], "no repeats, and 1 to 10 are always there");
        assert_eq!(
            data.options.drop_lowest,
            [GradeType::Quiz, GradeType::Custom("Project".into())],
            "a category that doesn't exist is dropped"
        );
        assert_eq!(data.options.subject_weights["Math"]["Test"], 70.0);

        let again: BackupData = serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        assert_eq!(data, again);
        assert_eq!(
            data.category_list().last(),
            Some(&GradeType::Custom("Project".into())),
            "custom categories come after the built-in ones"
        );
    }

    #[test]
    fn category_definitions_must_be_sound() {
        for bad in [
            r#"[{"name": "Test", "weight": 1}]"#,
            r#"[{"name": "quiz", "weight": 1}]"#,
            r#"[{"name": "Lab", "weight": 1}, {"name": "LAB", "weight": 2}]"#,
            r#"[{"name": "  ", "weight": 1}]"#,
        ] {
            let json = format!(
                r#"{{"students": [], "testWeight": 1, "quizWeight": 1, "homeworkWeight": 1, "grades": [], "categories": {bad}}}"#
            );
            assert!(serde_json::from_str::<BackupData>(&json).is_err(), "{bad}");
        }
    }

    #[test]
    fn settings_can_be_swapped_without_touching_students_and_grades() {
        let mut data: BackupData = serde_json::from_str(WITH_EXTRAS).unwrap();
        let before = data.settings();
        data.set_weight(&GradeType::Test, 99.0);
        data.set_weight(&GradeType::Custom("Project".into()), 5.0);
        data.apply_settings(before.clone());
        assert_eq!(data.settings(), before);
        assert_eq!(data.weight(&GradeType::Custom("Nope".into())), 0.0);
    }
}
