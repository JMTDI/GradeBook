//! The in-memory gradebook and the operations the UI performs on it, without the
//! persistence (see `store`).

use uuid::Uuid;

use crate::calc;
use crate::model::{BackupData, GradeEntry, GradeType, Student};

#[derive(Debug, Clone, Default)]
pub struct GradeBook {
    data: BackupData,
}

impl GradeBook {
    pub fn new(data: BackupData) -> Self {
        let mut book = GradeBook { data };
        book.sanitise_weights();
        book
    }

    pub fn data(&self) -> &BackupData {
        &self.data
    }

    /// Replaces everything, as Restore does.
    pub fn replace(&mut self, data: BackupData) {
        self.data = data;
        self.sanitise_weights();
    }

    pub fn students(&self) -> &[Student] {
        &self.data.students
    }

    pub fn student(&self, id: Uuid) -> Option<&Student> {
        self.data.students.iter().find(|s| s.id == id)
    }

    /// Adds a student; blank names are ignored.
    pub fn add_student(&mut self, name: &str, grade_level: u32) -> Option<Uuid> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }
        let student = Student {
            id: Uuid::new_v4(),
            name: trimmed.to_string(),
            grade_level,
        };
        let id = student.id;
        self.data.students.push(student);
        Some(id)
    }

    /// Removes a student together with all of their grades.
    pub fn delete_student(&mut self, id: Uuid) -> bool {
        let before = self.data.students.len();
        self.data.students.retain(|s| s.id != id);
        self.data.grades.retain(|g| g.student_id != id);
        self.data.students.len() != before
    }

    pub fn add_grade(
        &mut self,
        student_id: Uuid,
        subject: &str,
        period: u32,
        value: String,
        grade_type: GradeType,
    ) -> Uuid {
        let id = Uuid::new_v4();
        self.data.grades.push(GradeEntry {
            id,
            student_id,
            subject: subject.to_string(),
            period,
            value,
            grade_type,
        });
        id
    }

    pub fn delete_grade(&mut self, id: Uuid) -> bool {
        let before = self.data.grades.len();
        self.data.grades.retain(|g| g.id != id);
        self.data.grades.len() != before
    }

    pub fn weight(&self, grade_type: GradeType) -> f64 {
        self.data.weight(grade_type)
    }

    pub fn set_weight(&mut self, grade_type: GradeType, value: f64) {
        self.data.set_weight(grade_type, clamp_weight(value));
    }

    pub fn total_weight(&self) -> f64 {
        GradeType::ALL.iter().map(|t| self.weight(*t)).sum()
    }

    pub fn students_in_level(&self, level: u32) -> impl Iterator<Item = &Student> {
        self.data.students.iter().filter(move |s| s.grade_level == level)
    }

    pub fn grades_for<'a>(
        &'a self,
        student_id: Uuid,
        subject: &'a str,
        period: u32,
    ) -> impl Iterator<Item = &'a GradeEntry> {
        self.data
            .grades
            .iter()
            .filter(move |g| g.student_id == student_id && g.subject == subject && g.period == period)
    }

    pub fn student_average(&self, student_id: Uuid, subject: &str, period: u32) -> Option<f64> {
        calc::percent_average(self.grades_for(student_id, subject, period), |t| self.weight(t))
    }

    /// Mean of the averages of the students in `level` who have a usable average.
    pub fn class_average(&self, level: u32, subject: &str, period: u32) -> Option<f64> {
        calc::mean(
            self.students_in_level(level)
                .filter_map(|s| self.student_average(s.id, subject, period)),
        )
    }

    fn sanitise_weights(&mut self) {
        for grade_type in GradeType::ALL {
            let clean = clamp_weight(self.data.weight(grade_type));
            self.data.set_weight(grade_type, clean);
        }
    }
}

fn clamp_weight(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book_with_two_students() -> (GradeBook, Uuid, Uuid) {
        let mut book = GradeBook::default();
        let ada = book.add_student("  Ada ", 3).unwrap();
        let bob = book.add_student("Bob", 3).unwrap();
        book.add_student("Cy", 4).unwrap();
        (book, ada, bob)
    }

    #[test]
    fn blank_names_are_ignored_and_names_trimmed() {
        let mut book = GradeBook::default();
        assert_eq!(book.add_student("   ", 1), None);
        assert_eq!(book.add_student("", 1), None);
        let id = book.add_student("  Ada Lovelace  ", 2).unwrap();
        assert_eq!(book.student(id).unwrap().name, "Ada Lovelace");
    }

    #[test]
    fn deleting_a_student_removes_their_grades_only() {
        let (mut book, ada, bob) = book_with_two_students();
        book.add_grade(ada, "Math", 1, "90".into(), GradeType::Test);
        book.add_grade(bob, "Math", 1, "80".into(), GradeType::Test);
        assert!(book.delete_student(ada));
        assert!(!book.delete_student(ada));
        assert_eq!(book.data().grades.len(), 1);
        assert_eq!(book.data().grades[0].student_id, bob);
    }

    #[test]
    fn grades_are_filtered_by_subject_and_period() {
        let (mut book, ada, _) = book_with_two_students();
        let keep = book.add_grade(ada, "Math", 1, "90".into(), GradeType::Test);
        book.add_grade(ada, "Math", 2, "70".into(), GradeType::Test);
        book.add_grade(ada, "Art", 1, "60".into(), GradeType::Test);
        let found: Vec<_> = book.grades_for(ada, "Math", 1).map(|g| g.id).collect();
        assert_eq!(found, vec![keep]);
        assert!(book.delete_grade(keep));
        assert!(!book.delete_grade(keep));
        assert_eq!(book.grades_for(ada, "Math", 1).count(), 0);
    }

    #[test]
    fn class_average_only_counts_students_in_the_level_with_grades() {
        let (mut book, ada, bob) = book_with_two_students();
        assert_eq!(book.class_average(3, "Math", 1), None);
        book.add_grade(ada, "Math", 1, "90".into(), GradeType::Test);
        book.add_grade(bob, "Math", 1, "70".into(), GradeType::Test);
        let cy = book.students_in_level(4).next().unwrap().id;
        book.add_grade(cy, "Math", 1, "10".into(), GradeType::Test);
        assert_eq!(book.class_average(3, "Math", 1), Some(80.0));
        assert_eq!(book.class_average(3, "Math", 2), None);
        assert_eq!(book.student_average(ada, "Math", 1), Some(90.0));
    }

    #[test]
    fn weights_change_the_average() {
        let (mut book, ada, _) = book_with_two_students();
        book.add_grade(ada, "Math", 1, "100".into(), GradeType::Test);
        book.add_grade(ada, "Math", 1, "50".into(), GradeType::Quiz);
        book.set_weight(GradeType::Test, 50.0);
        book.set_weight(GradeType::Quiz, 50.0);
        assert_eq!(book.student_average(ada, "Math", 1), Some(75.0));
        book.set_weight(GradeType::Quiz, 0.0);
        assert_eq!(book.student_average(ada, "Math", 1), Some(100.0));
    }

    #[test]
    fn weights_are_clamped_and_total_reported() {
        let mut book = GradeBook::default();
        assert_eq!(book.total_weight(), 100.0);
        book.set_weight(GradeType::Test, 250.0);
        book.set_weight(GradeType::Quiz, -5.0);
        assert_eq!(book.weight(GradeType::Test), 100.0);
        assert_eq!(book.weight(GradeType::Quiz), 0.0);
        let restored = GradeBook::new(BackupData {
            test_weight: 400.0,
            ..BackupData::default()
        });
        assert_eq!(restored.weight(GradeType::Test), 100.0);
    }

    #[test]
    fn replace_swaps_everything() {
        let (mut book, _, _) = book_with_two_students();
        book.replace(BackupData::default());
        assert!(book.students().is_empty() && book.data().grades.is_empty());
    }
}
