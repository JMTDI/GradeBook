//! Data model. Its JSON shape (camelCase keys, upper-case hyphenated ids, and `Test`,
//! `Quiz` and `Homework` grade types) is the file format of the data file and of Backup
//! files, so changing it would break files that already exist.

use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GradeType {
    #[serde(rename = "Homework")]
    Homework,
    #[serde(rename = "Test")]
    Test,
    #[serde(rename = "Quiz")]
    Quiz,
}

impl GradeType {
    /// The order the types are listed in everywhere in the UI.
    pub const ALL: [GradeType; 3] = [GradeType::Homework, GradeType::Test, GradeType::Quiz];

    /// Position in [`GradeType::ALL`]; also the index used by the UI.
    pub fn index(self) -> usize {
        match self {
            GradeType::Homework => 0,
            GradeType::Test => 1,
            GradeType::Quiz => 2,
        }
    }

    pub fn from_index(index: usize) -> Option<GradeType> {
        GradeType::ALL.get(index).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            GradeType::Homework => "Homework",
            GradeType::Test => "Test",
            GradeType::Quiz => "Quiz",
        }
    }
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
    /// Kept as text, exactly as typed.
    pub value: String,
    #[serde(rename = "type")]
    pub grade_type: GradeType,
}

/// Everything the app stores. Used both for the on-disk data file and for
/// Backup/Restore files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupData {
    pub students: Vec<Student>,
    pub test_weight: f64,
    pub quiz_weight: f64,
    pub homework_weight: f64,
    pub grades: Vec<GradeEntry>,
}

impl Default for BackupData {
    fn default() -> Self {
        BackupData {
            students: Vec::new(),
            test_weight: 50.0,
            quiz_weight: 17.0,
            homework_weight: 33.0,
            grades: Vec::new(),
        }
    }
}

impl BackupData {
    pub fn weight(&self, grade_type: GradeType) -> f64 {
        match grade_type {
            GradeType::Test => self.test_weight,
            GradeType::Quiz => self.quiz_weight,
            GradeType::Homework => self.homework_weight,
        }
    }

    pub fn set_weight(&mut self, grade_type: GradeType, value: f64) {
        match grade_type {
            GradeType::Test => self.test_weight = value,
            GradeType::Quiz => self.quiz_weight = value,
            GradeType::Homework => self.homework_weight = value,
        }
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
        assert!(serde_json::from_str::<BackupData>(&bad).is_err());
    }

    #[test]
    fn grade_types_are_listed_homework_test_quiz() {
        assert_eq!(GradeType::ALL.map(GradeType::label), ["Homework", "Test", "Quiz"]);
        // The UI refers to a type by its position in the list.
        for (position, grade_type) in GradeType::ALL.into_iter().enumerate() {
            assert_eq!(grade_type.index(), position);
            assert_eq!(GradeType::from_index(position), Some(grade_type));
        }
    }

    #[test]
    fn default_weights_are_50_17_33() {
        let d = BackupData::default();
        assert_eq!((d.test_weight, d.quiz_weight, d.homework_weight), (50.0, 17.0, 33.0));
    }
}
