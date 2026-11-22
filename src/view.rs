//! What the screens show, derived from the book and the current filters.
//! Kept free of any UI types so it can be unit-tested.

use uuid::Uuid;

use crate::book::GradeBook;
use crate::calc;
use crate::model::Student;

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
    /// Position of the score's category in `GradeBook::categories`.
    pub kind: usize,
    /// The lowest score of its category, left out of the average.
    pub dropped: bool,
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
    let categories = book.categories();
    let dropped = book.dropped(student.id, &filters.subject, filters.period);
    let chips = book
        .grades_for(student.id, &filters.subject, filters.period)
        .map(|grade| ChipView {
            id: grade.id,
            value: grade.value.clone(),
            letter: calc::letter_for_text(&grade.value),
            kind: categories.iter().position(|c| *c == grade.grade_type).unwrap_or(0),
            dropped: dropped.contains(&grade.id),
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

/// The letters the distribution is counted in: A+, A and A− are all an A.
pub const LETTER_FAMILIES: [&str; 6] = ["A", "B", "C", "D", "E", "F"];

/// The default for "needs attention": averages below this.
pub const DEFAULT_ATTENTION: u32 = 70;

/// A student's name and average.
#[derive(Debug, Clone, PartialEq)]
pub struct Standing {
    pub name: String,
    pub average: f64,
}

/// How a class is doing in one subject.
#[derive(Debug, Clone, PartialEq)]
pub struct Insights {
    /// Students in the grade level.
    pub students: usize,
    /// Of those, the ones with an average in this subject and period.
    pub counted: usize,
    pub class_average: Option<f64>,
    pub highest: Option<Standing>,
    pub lowest: Option<Standing>,
    pub median: Option<f64>,
    /// How many students are in each of [`LETTER_FAMILIES`].
    pub distribution: [usize; 6],
    /// Students whose average is below the threshold, lowest first.
    pub attention: Vec<Standing>,
    /// The class average of every period, from period 1.
    pub trend: Vec<Option<f64>>,
}

/// The family of an average as it is displayed ("A−" and "A+" are both an A).
fn letter_family(average: f64) -> usize {
    let letter = calc::average_letter(average);
    LETTER_FAMILIES
        .iter()
        .position(|family| letter.starts_with(family))
        .unwrap_or(LETTER_FAMILIES.len() - 1)
}

pub fn insights(book: &GradeBook, filters: &Filters, attention_below: f64) -> Insights {
    let mut standings: Vec<Standing> = book
        .students_in_level(filters.grade_level)
        .filter_map(|student| {
            book.student_average(student.id, &filters.subject, filters.period)
                .map(|average| Standing {
                    name: student.name.clone(),
                    average,
                })
        })
        .collect();

    let mut distribution = [0; 6];
    for standing in &standings {
        distribution[letter_family(standing.average)] += 1;
    }
    let class_average = calc::mean(standings.iter().map(|s| s.average));
    let median = calc::median(standings.iter().map(|s| s.average).collect());
    let counted = standings.len();

    let by_average = |a: &Standing, b: &Standing| a.average.total_cmp(&b.average).then_with(|| a.name.cmp(&b.name));
    standings.sort_by(by_average);
    let lowest = standings.first().cloned();
    // With a tie at the top, the first of them by name.
    let top = standings.last().map(|s| s.average);
    let highest = standings.iter().find(|s| Some(s.average) == top).cloned();
    // What counts is the average as it is shown, to one decimal place: 69.96 is shown as 70.0
    // and is not below 70.
    let attention = standings
        .iter()
        .filter(|s| calc::displayed(s.average) < attention_below)
        .cloned()
        .collect();

    Insights {
        students: book.students_in_level(filters.grade_level).count(),
        counted,
        class_average,
        highest,
        lowest,
        median,
        distribution,
        attention,
        trend: calc::PERIODS
            .map(|period| book.class_average(filters.grade_level, &filters.subject, period))
            .collect(),
    }
}

/// One subject of a report card.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportSubject {
    pub subject: String,
    /// The average of each period, from period 1.
    pub periods: Vec<Option<f64>>,
    /// The mean of the periods that have an average.
    pub average: Option<f64>,
}

/// Every subject a student has scores in, by period.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportCard {
    pub student_id: Uuid,
    pub name: String,
    pub grade_level: u32,
    pub subjects: Vec<ReportSubject>,
    /// The mean of the subjects' averages in each period.
    pub periods: Vec<Option<f64>>,
    /// The mean of the subjects' averages.
    pub overall: Option<f64>,
}

pub fn report_card(book: &GradeBook, student_id: Uuid) -> Option<ReportCard> {
    let student = book.student(student_id)?;
    let subjects: Vec<ReportSubject> = book
        .subjects()
        .into_iter()
        .filter(|subject| {
            book.data()
                .grades
                .iter()
                .any(|g| g.student_id == student_id && g.subject == *subject)
        })
        .map(|subject| {
            let periods: Vec<Option<f64>> = calc::PERIODS
                .map(|period| book.student_average(student_id, &subject, period))
                .collect();
            let average = calc::mean(periods.iter().flatten().copied());
            ReportSubject {
                subject,
                periods,
                average,
            }
        })
        .collect();
    let periods = (0..calc::PERIODS.count())
        .map(|i| calc::mean(subjects.iter().filter_map(|s| s.periods[i])))
        .collect();
    let overall = calc::mean(subjects.iter().filter_map(|s| s.average));
    Some(ReportCard {
        student_id,
        name: student.name.clone(),
        grade_level: student.grade_level,
        subjects,
        periods,
        overall,
    })
}

/// "85.3 B" for an average, or a dash when there isn't one.
pub fn average_text(average: Option<f64>) -> String {
    average.map_or_else(|| "—".to_string(), calc::format_average)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GradeType;

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
        // The category's position in the list: Homework, Test, Quiz.
        assert_eq!((ada_row.chips[0].kind, ada_row.chips[1].kind), (1, 2));
        assert_eq!(view.rows[1].chips[0].kind, 0);
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

    #[test]
    fn the_lowest_score_that_is_left_out_is_flagged_and_custom_categories_have_a_position() {
        let mut book = GradeBook::default();
        let ada = book.add_student("Ada", 1).unwrap();
        let project = book.add_category("Project").unwrap();
        book.set_drop_lowest(&GradeType::Quiz, true);
        book.add_grade(ada, "Math", 1, "60".into(), GradeType::Quiz);
        book.add_grade(ada, "Math", 1, "100".into(), GradeType::Quiz);
        book.add_grade(ada, "Math", 1, "+5".into(), project);
        book.add_grade(ada, "Math", 1, "EX".into(), GradeType::Test);
        let chips = &grades_view(&book, &Filters::default()).rows[0].chips;
        assert_eq!(
            chips.iter().map(|c| c.dropped).collect::<Vec<_>>(),
            [true, false, false, false]
        );
        assert_eq!(chips.iter().map(|c| c.kind).collect::<Vec<_>>(), [2, 2, 3, 1]);
        assert_eq!(
            chips.iter().map(|c| c.letter).collect::<Vec<_>>(),
            ["F", "A+", "EC", "—"]
        );
    }

    /// Four students in grade 3 with Math averages 95 (A), 85 (C+), 85 (C+) and 50 (F),
    /// and one without a score.
    fn class() -> GradeBook {
        let mut book = GradeBook::default();
        for (name, scores) in [
            ("Ann", &[(1, "95")][..]),
            ("Ben", &[(1, "85"), (2, "75")]),
            ("Cy", &[(1, "85")]),
            ("Di", &[(1, "50"), (3, "90")]),
            ("Ed", &[]),
        ] {
            let id = book.add_student(name, 3).unwrap();
            for (period, value) in scores {
                book.add_grade(id, "Math", *period, value.to_string(), GradeType::Test);
            }
        }
        book.add_student("Elsewhere", 4).unwrap();
        book
    }

    fn third_grade(period: u32) -> Filters {
        Filters {
            grade_level: 3,
            period,
            subject: "Math".into(),
        }
    }

    #[test]
    fn insights_summarise_the_class() {
        let i = insights(&class(), &third_grade(1), 70.0);
        assert_eq!((i.students, i.counted), (5, 4));
        assert_eq!(i.class_average, Some((95.0 + 85.0 + 85.0 + 50.0) / 4.0));
        assert_eq!(i.median, Some(85.0));
        let named = |s: &Option<Standing>| s.as_ref().map(|s| (s.name.clone(), s.average));
        assert_eq!(named(&i.highest), Some(("Ann".to_string(), 95.0)));
        assert_eq!(named(&i.lowest), Some(("Di".to_string(), 50.0)));
        // 95 is an A, the two 85s are C+, and 50 is an F.
        assert_eq!(i.distribution, [1, 0, 2, 0, 0, 1]);
        assert_eq!(
            i.attention,
            [Standing {
                name: "Di".into(),
                average: 50.0
            }]
        );
        assert_eq!(i.trend, [Some(78.75), Some(75.0), Some(90.0), None, None, None]);
    }

    #[test]
    fn the_attention_threshold_picks_who_is_listed() {
        let book = class();
        let names = |below: f64| -> Vec<String> {
            insights(&book, &third_grade(1), below)
                .attention
                .into_iter()
                .map(|s| s.name)
                .collect()
        };
        assert!(names(50.0).is_empty(), "below 50 is nobody: Di has exactly 50");
        assert_eq!(names(86.0), ["Di", "Ben", "Cy"], "lowest first, then by name");
        assert_eq!(names(101.0), ["Di", "Ben", "Cy", "Ann"]);
    }

    #[test]
    fn the_attention_list_goes_by_the_average_as_it_is_shown() {
        let mut book = GradeBook::default();
        for (name, score) in [("Just under", "69.94"), ("Shown as 70", "69.96"), ("At 70", "70")] {
            let id = book.add_student(name, 3).unwrap();
            book.add_grade(id, "Math", 1, score.into(), GradeType::Test);
        }
        let listed: Vec<String> = insights(&book, &third_grade(1), 70.0)
            .attention
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(
            listed,
            ["Just under"],
            "69.96 shows as 70.0, so it isn't below 70, which is what the list says"
        );
    }

    #[test]
    fn insights_of_an_empty_class_are_empty() {
        let i = insights(&GradeBook::default(), &Filters::default(), 70.0);
        assert_eq!((i.students, i.counted), (0, 0));
        assert!(i.class_average.is_none() && i.highest.is_none() && i.lowest.is_none() && i.median.is_none());
        assert_eq!(i.distribution, [0; 6]);
        assert!(i.attention.is_empty());
        assert_eq!(i.trend, vec![None; 6]);
    }

    #[test]
    fn letter_families_group_the_plus_and_minus_letters() {
        for (average, family) in [
            (100.0, 0),
            (96.0, 0),
            (94.0, 0),
            (93.9, 1),
            (88.0, 1),
            (84.0, 2),
            (76.0, 2),
            (75.9, 3),
            (70.0, 3),
            (69.9, 4),
            (63.0, 4),
            (62.9, 5),
            (0.0, 5),
        ] {
            assert_eq!(letter_family(average), family, "{average}");
        }
        // It follows the displayed number: 93.96 shows as 94.0, which is an A−.
        assert_eq!(letter_family(93.96), 0);
    }

    #[test]
    fn a_report_card_lists_the_subjects_a_student_has_scores_in() {
        let mut book = GradeBook::default();
        let ada = book.add_student("Ada", 3).unwrap();
        let bob = book.add_student("Bob", 3).unwrap();
        book.add_grade(ada, "Math", 1, "90".into(), GradeType::Test);
        book.add_grade(ada, "Math", 2, "80".into(), GradeType::Test);
        book.add_grade(ada, "Art", 2, "100".into(), GradeType::Test);
        book.add_grade(ada, "Music", 3, "EX".into(), GradeType::Test);
        book.add_grade(bob, "Science", 1, "70".into(), GradeType::Test);

        let card = report_card(&book, ada).unwrap();
        assert_eq!((card.name.as_str(), card.grade_level), ("Ada", 3));
        let names: Vec<&str> = card.subjects.iter().map(|s| s.subject.as_str()).collect();
        assert_eq!(names, ["Art", "Math", "Music"], "alphabetical, and only Ada's subjects");

        let math = &card.subjects[1];
        assert_eq!(math.periods, [Some(90.0), Some(80.0), None, None, None, None]);
        assert_eq!(math.average, Some(85.0), "the periods count equally");
        let music = &card.subjects[2];
        assert_eq!(music.average, None, "an excused score alone is no average");

        // Period 2 has Math 80 and Art 100: their mean is 90.
        assert_eq!(card.periods, [Some(90.0), Some(90.0), None, None, None, None]);
        assert_eq!(card.overall, Some((100.0 + 85.0) / 2.0));
        assert_eq!(report_card(&book, Uuid::new_v4()), None);
    }

    #[test]
    fn a_report_card_uses_each_subjects_own_weights() {
        let mut book = GradeBook::default();
        let ada = book.add_student("Ada", 3).unwrap();
        for subject in ["Math", "Art"] {
            book.add_grade(ada, subject, 1, "100".into(), GradeType::Test);
            book.add_grade(ada, subject, 1, "50".into(), GradeType::Quiz);
        }
        book.set_weight(Some("Art"), &GradeType::Quiz, 0.0);
        let card = report_card(&book, ada).unwrap();
        assert_eq!(card.subjects[0].subject, "Art");
        assert_eq!(card.subjects[0].average, Some(100.0));
        assert!(card.subjects[1].average.unwrap() < 100.0);
    }

    #[test]
    fn averages_are_shown_with_their_letter_or_a_dash() {
        assert_eq!(average_text(Some(94.04)), "94.0 A−");
        assert_eq!(average_text(None), "—");
    }
}
