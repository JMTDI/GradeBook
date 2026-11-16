//! What the Grades screen shows, derived from the book and the current filters.
//! Kept free of any UI types so it can be unit-tested.

use uuid::Uuid;

use crate::book::GradeBook;
use crate::calc;
use crate::model::{GradeType, Student};

/// The Grade / Period / Subject pickers.
#[derive(Debug, Clone, PartialEq)]
pub struct Filters {
    pub grade_level: u32,
    pub period: u32,
    pub subject: String,
}

impl Default for Filters {
    fn default() -> Self {
        Filters {
            grade_level: 1,
            period: 1,
            subject: calc::DEFAULT_SUBJECT.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChipView {
    pub id: Uuid,
    pub value: String,
    pub letter: &'static str,
    pub kind: GradeType,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RowView {
    pub id: Uuid,
    pub name: String,
    pub chips: Vec<ChipView>,
    /// "85.3 B", if the student has a usable average.
    pub average: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GradesView {
    pub rows: Vec<RowView>,
    /// "85.3 B", if any student in the class has an average.
    pub class_average: Option<String>,
}

pub fn row_for(book: &GradeBook, student: &Student, filters: &Filters) -> RowView {
    let chips = book
        .grades_for(student.id, &filters.subject, filters.period)
        .map(|grade| ChipView {
            id: grade.id,
            value: grade.value.clone(),
            letter: calc::letter_for_text(&grade.value),
            kind: grade.grade_type,
        })
        .collect();
    RowView {
        id: student.id,
        name: student.name.clone(),
        chips,
        average: book
            .student_average(student.id, &filters.subject, filters.period)
            .map(calc::format_average),
    }
}

pub fn grades_view(book: &GradeBook, filters: &Filters) -> GradesView {
    GradesView {
        rows: book
            .students_in_level(filters.grade_level)
            .map(|s| row_for(book, s, filters))
            .collect(),
        class_average: book
            .class_average(filters.grade_level, &filters.subject, filters.period)
            .map(calc::format_average),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_follow_the_grade_level_filter_in_insertion_order() {
        let mut book = GradeBook::default();
        book.add_student("Zed", 2).unwrap();
        book.add_student("Amy", 2).unwrap();
        book.add_student("Other", 5).unwrap();
        let view = grades_view(
            &book,
            &Filters {
                grade_level: 2,
                ..Filters::default()
            },
        );
        let names: Vec<_> = view.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Zed", "Amy"]);
        assert_eq!(view.class_average, None);
    }

    #[test]
    fn chips_averages_and_class_average() {
        let mut book = GradeBook::default();
        let ada = book.add_student("Ada", 1).unwrap();
        let bob = book.add_student("Bob", 1).unwrap();
        book.add_grade(ada, "Math", 1, "92.5".into(), GradeType::Test);
        book.add_grade(ada, "Math", 1, "100".into(), GradeType::Quiz);
        book.add_grade(bob, "Math", 1, "80".into(), GradeType::Homework);
        book.add_grade(bob, "Art", 1, "10".into(), GradeType::Test); // other subject: hidden

        let view = grades_view(&book, &Filters::default());
        let ada_row = &view.rows[0];
        assert_eq!(ada_row.chips.len(), 2);
        assert_eq!(ada_row.chips[0].value, "92.5");
        assert_eq!(ada_row.chips[0].letter, "B+");
        assert_eq!(ada_row.chips[1].kind, GradeType::Quiz);
        // (92.5 * 50 + 100 * 17) / 67 = 94.40...
        assert_eq!(ada_row.average.as_deref(), Some("94.4 A−"));
        assert_eq!(view.rows[1].average.as_deref(), Some("80.0 C"));
        // (94.40... + 80) / 2 = 87.20... -> "87.2 B−"
        assert_eq!(view.class_average.as_deref(), Some("87.2 B−"));
    }

    #[test]
    fn students_without_grades_have_no_average_or_chips() {
        let mut book = GradeBook::default();
        book.add_student("Ada", 1).unwrap();
        let view = grades_view(&book, &Filters::default());
        assert!(view.rows[0].chips.is_empty());
        assert_eq!(view.rows[0].average, None);
        assert_eq!(view.class_average, None);
    }
}
