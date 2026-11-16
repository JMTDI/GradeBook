//! Grade math: input handling, letter grades and weighted averages.

use crate::model::{GradeEntry, GradeType};

pub const SUBJECTS: [&str; 20] = [
    "Algebra",
    "Art",
    "Bible",
    "English",
    "Health",
    "History",
    "Literature",
    "Math",
    "Memory",
    "Music",
    "Penmanship",
    "Phonics",
    "Reading",
    "Recordkeeping",
    "Science",
    "Social Studies",
    "Spanish",
    "Spelling",
    "Typing",
    "Writing",
];

pub const DEFAULT_SUBJECT: &str = "Math";
pub const GRADE_LEVELS: std::ops::RangeInclusive<u32> = 1..=10;
pub const PERIODS: std::ops::RangeInclusive<u32> = 1..=6;

/// Longest grade a user can type ("100.25"): long enough for decimals such as 92.5.
pub const MAX_GRADE_INPUT_LEN: usize = 6;

/// Lower bound (inclusive) of each letter band, highest first.
///
/// Bands are defined by their lower bounds rather than by whole-number ranges, so a
/// fractional score such as 99.5, 95.5 or 91.5 gets the letter of the band it falls in,
/// and anything above 100 is still an A+.
const LETTER_BANDS: [(f64, &str); 11] = [
    (100.0, "A+"),
    (96.0, "A"),
    (94.0, "A−"),
    (92.0, "B+"),
    (88.0, "B"),
    (86.0, "B−"),
    (84.0, "C+"),
    (79.0, "C"),
    (76.0, "C−"),
    (70.0, "D"),
    (63.0, "E"),
];

pub fn letter_grade(value: f64) -> &'static str {
    LETTER_BANDS
        .iter()
        .find(|(min, _)| value >= *min)
        .map(|(_, letter)| *letter)
        .unwrap_or("F")
}

/// Letter for a grade as typed; "—" when it isn't a number.
pub fn letter_for_text(text: &str) -> &'static str {
    parse_score(text).map(letter_grade).unwrap_or("—")
}

/// A finite number, or `None`.
pub fn parse_score(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Keeps only digits and the first decimal separator, up to [`MAX_GRADE_INPUT_LEN`]
/// characters. A comma counts as the decimal separator (and is stored as a dot), so
/// "92,5" typed on a European keyboard isn't silently turned into 925.
pub fn sanitize_grade_input(input: &str) -> String {
    let mut seen_separator = false;
    input
        .chars()
        .filter_map(|c| match c {
            '0'..='9' => Some(c),
            '.' | ',' if !seen_separator => {
                seen_separator = true;
                Some('.')
            }
            _ => None,
        })
        .take(MAX_GRADE_INPUT_LEN)
        .collect()
}

/// The text to store for a typed grade, or `None` if it isn't a usable number.
/// A trailing decimal point ("92.") is dropped.
pub fn parse_grade_input(input: &str) -> Option<String> {
    let cleaned = sanitize_grade_input(input.trim());
    let cleaned = cleaned.strip_suffix('.').unwrap_or(&cleaned).to_string();
    parse_score(&cleaned).map(|_| cleaned)
}

/// Weighted average of the given grades, as a percentage.
///
/// Scores are averaged per grade type, then combined using each type's weight.
/// Weights are re-normalised over the types that actually have scores, so a
/// student with only tests is graded on tests alone.
pub fn percent_average<'a>(
    grades: impl IntoIterator<Item = &'a GradeEntry>,
    weight_of: impl Fn(GradeType) -> f64,
) -> Option<f64> {
    let grades: Vec<&GradeEntry> = grades.into_iter().collect();
    if grades.is_empty() {
        return None;
    }

    let mut used_weight = 0.0;
    let mut per_type: Vec<(f64, f64)> = Vec::new(); // (type average, type weight)
    for grade_type in GradeType::ALL {
        let scores: Vec<f64> = grades
            .iter()
            .filter(|g| g.grade_type == grade_type)
            .filter_map(|g| parse_score(&g.value))
            .collect();
        if scores.is_empty() {
            continue;
        }
        let weight = weight_of(grade_type);
        used_weight += weight;
        per_type.push((scores.iter().sum::<f64>() / scores.len() as f64, weight));
    }

    if used_weight <= 0.0 {
        return None;
    }
    Some(per_type.iter().map(|(avg, weight)| avg * (weight / used_weight)).sum())
}

pub fn mean(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values.into_iter().fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    (count > 0).then(|| sum / count as f64)
}

/// "85.3 B": one decimal place plus the letter, as shown under each student.
/// The letter is derived from the rounded number that is displayed.
pub fn format_average(avg: f64) -> String {
    let shown = format!("{avg:.1}");
    let letter = letter_grade(shown.parse().unwrap_or(avg));
    format!("{shown} {letter}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn entry(value: &str, grade_type: GradeType) -> GradeEntry {
        GradeEntry {
            id: Uuid::new_v4(),
            student_id: Uuid::nil(),
            subject: "Math".into(),
            period: 1,
            value: value.into(),
            grade_type,
        }
    }

    fn default_weight(t: GradeType) -> f64 {
        match t {
            GradeType::Test => 50.0,
            GradeType::Quiz => 17.0,
            GradeType::Homework => 33.0,
        }
    }

    #[test]
    fn letter_grades_for_whole_numbers() {
        let expected = [
            (100.0, "A+"),
            (99.0, "A"),
            (96.0, "A"),
            (95.0, "A−"),
            (94.0, "A−"),
            (93.0, "B+"),
            (92.0, "B+"),
            (91.0, "B"),
            (88.0, "B"),
            (87.0, "B−"),
            (86.0, "B−"),
            (85.0, "C+"),
            (84.0, "C+"),
            (83.0, "C"),
            (79.0, "C"),
            (78.0, "C−"),
            (76.0, "C−"),
            (75.0, "D"),
            (70.0, "D"),
            (69.0, "E"),
            (63.0, "E"),
            (62.0, "F"),
            (0.0, "F"),
        ];
        for (score, letter) in expected {
            assert_eq!(letter_grade(score), letter, "score {score}");
        }
    }

    #[test]
    fn letter_grades_cover_fractional_scores_between_whole_number_bands() {
        // Scores that sit between two whole-number ranges, or above 100.
        assert_eq!(letter_grade(99.5), "A");
        assert_eq!(letter_grade(95.5), "A−");
        assert_eq!(letter_grade(91.5), "B");
        assert_eq!(letter_grade(69.5), "E");
        assert_eq!(letter_grade(105.0), "A+");
    }

    #[test]
    fn letter_for_text_handles_non_numbers() {
        assert_eq!(letter_for_text("92.5"), "B+");
        assert_eq!(letter_for_text("abc"), "—");
        assert_eq!(letter_for_text("NaN"), "—");
        assert_eq!(letter_for_text(""), "—");
    }

    #[test]
    fn weighted_average_uses_all_three_types() {
        let grades = [
            entry("90", GradeType::Test),
            entry("80", GradeType::Quiz),
            entry("100", GradeType::Homework),
        ];
        let avg = percent_average(&grades, default_weight).unwrap();
        assert!((avg - 91.6).abs() < 1e-9, "{avg}");
    }

    #[test]
    fn weights_renormalise_over_the_types_present() {
        let only_tests = [entry("90", GradeType::Test)];
        assert!((percent_average(&only_tests, default_weight).unwrap() - 90.0).abs() < 1e-9);

        // Quiz (17) and homework (33) only: weights become 17/50 and 33/50.
        let two = [entry("80", GradeType::Quiz), entry("100", GradeType::Homework)];
        let expected = 80.0 * 17.0 / 50.0 + 100.0 * 33.0 / 50.0;
        assert!((percent_average(&two, default_weight).unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn scores_of_one_type_are_averaged_first() {
        let grades = [
            entry("100", GradeType::Test),
            entry("80", GradeType::Test),
            entry("50", GradeType::Quiz),
        ];
        // tests average 90, quiz 50; weights 50 and 17 -> (90*50 + 50*17) / 67
        let expected = (90.0 * 50.0 + 50.0 * 17.0) / 67.0;
        assert!((percent_average(&grades, default_weight).unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn no_average_without_usable_scores_or_weight() {
        let none: [GradeEntry; 0] = [];
        assert_eq!(percent_average(&none, default_weight), None);
        assert_eq!(percent_average(&[entry("abc", GradeType::Test)], default_weight), None);
        // The only graded type has weight 0.
        assert_eq!(percent_average(&[entry("90", GradeType::Test)], |_| 0.0), None);
    }

    #[test]
    fn mean_of_nothing_is_none() {
        assert_eq!(mean(std::iter::empty()), None);
        assert_eq!(mean([80.0, 100.0]), Some(90.0));
    }

    #[test]
    fn average_text_uses_the_rounded_number_for_the_letter() {
        assert_eq!(format_average(85.34), "85.3 C+");
        // 95.96 displays as 96.0, so it must be an A, not an A−.
        assert_eq!(format_average(95.96), "96.0 A");
        assert_eq!(format_average(100.0), "100.0 A+");
    }

    #[test]
    fn grade_input_is_sanitised_and_validated() {
        assert_eq!(sanitize_grade_input("92.5"), "92.5");
        assert_eq!(sanitize_grade_input("9a2"), "92");
        assert_eq!(sanitize_grade_input("1.2.3"), "1.23");
        assert_eq!(sanitize_grade_input("1234567"), "123456");
        assert_eq!(parse_grade_input("  92.5 "), Some("92.5".to_string()));
        assert_eq!(parse_grade_input("."), None);
        assert_eq!(parse_grade_input("abc"), None);
        assert_eq!(parse_grade_input(""), None);
    }

    #[test]
    fn a_comma_is_a_decimal_separator_and_a_trailing_point_is_dropped() {
        assert_eq!(sanitize_grade_input("92,5"), "92.5");
        assert_eq!(sanitize_grade_input("92,5,1"), "92.51");
        assert_eq!(sanitize_grade_input("92.5,1"), "92.51");
        assert_eq!(parse_grade_input("92,5"), Some("92.5".to_string()));
        assert_eq!(parse_grade_input("92."), Some("92".to_string()));
        assert_eq!(parse_grade_input(",5"), Some(".5".to_string()));
        assert_eq!(parse_grade_input(","), None);
    }
}
