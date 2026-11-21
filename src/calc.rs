//! Grade math: input handling, letter grades and weighted averages.

use std::collections::HashSet;

use uuid::Uuid;

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

/// The grade levels every gradebook has. More can be added (see [`can_add_level`]).
pub const GRADE_LEVELS: std::ops::RangeInclusive<u32> = 1..=10;
pub const PERIODS: std::ops::RangeInclusive<u32> = 1..=6;

/// The level used for Kindergarten.
pub const KINDERGARTEN: u32 = 0;

/// Whether `level` can be added next to grades 1 to 10: Kindergarten, or grades 11 to 20.
pub fn can_add_level(level: u32) -> bool {
    level == KINDERGARTEN || (11..=20).contains(&level)
}

pub fn grade_level_label(level: u32) -> String {
    if level == KINDERGARTEN {
        "Kindergarten".to_string()
    } else {
        format!("Grade {level}")
    }
}

/// A grade level as typed: `K`, `Kindergarten`, `11` or `Grade 11`.
pub fn parse_grade_level(text: &str) -> Option<u32> {
    let text = text.trim().to_lowercase();
    let text = text.strip_prefix("grade").map_or(text.as_str(), str::trim);
    match text {
        "k" | "kindergarten" => Some(KINDERGARTEN),
        digits if !digits.is_empty() && digits.len() <= 3 && digits.bytes().all(|b| b.is_ascii_digit()) => {
            digits.parse().ok()
        }
        _ => None,
    }
}

/// Longest a number in a grade can be ("100.25"): long enough for decimals such as 92.5.
pub const MAX_NUMBER_LEN: usize = 6;

/// Lower bound (inclusive) of each letter band, highest first.
///
/// Bands are defined by their lower bounds rather than by whole-number ranges, so a
/// fractional score such as 99.5, 95.5 or 91.5 gets the letter of the band it falls in,
/// and anything above 100 is still an A+.
pub const LETTER_BANDS: [(f64, &str); 11] = [
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

/// The lowest score that is `letter` (an A+ is 100), or `None` for a letter that doesn't exist.
pub fn letter_floor(letter: &str) -> Option<f64> {
    LETTER_BANDS
        .iter()
        .find(|(_, name)| *name == letter)
        .map(|(floor, _)| *floor)
}

/// What a typed or stored score is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Score {
    /// A percentage, typed as `92.5` or as points, `18/20`.
    Percent(f64),
    /// Extra credit, typed `+5`: bonus points added to the average of its category.
    Bonus(f64),
    /// Typed `EX`: the score doesn't count for or against the student.
    Excused,
    /// Anything else. It is shown but never counted.
    Unusable,
}

/// A number made of digits and at most one decimal point, such as `92`, `92.5` or `.5`.
fn plain_number(text: &str) -> Option<f64> {
    let digits = text.chars().filter(char::is_ascii_digit).count();
    let separators = text.chars().filter(|c| *c == '.').count();
    if digits == 0 || separators > 1 || text.len() != digits + separators {
        return None;
    }
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

pub fn classify(text: &str) -> Score {
    let text = text.trim();
    if text.eq_ignore_ascii_case("EX") {
        return Score::Excused;
    }
    if let Some(bonus) = text.strip_prefix('+') {
        return plain_number(bonus).map_or(Score::Unusable, Score::Bonus);
    }
    if let Some((points, possible)) = text.split_once('/') {
        return match (plain_number(points), plain_number(possible)) {
            (Some(points), Some(possible)) if possible > 0.0 => Score::Percent(points / possible * 100.0),
            _ => Score::Unusable,
        };
    }
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map_or(Score::Unusable, Score::Percent)
}

/// The percentage a score stands for, if it is one.
pub fn parse_score(text: &str) -> Option<f64> {
    match classify(text) {
        Score::Percent(value) => Some(value),
        _ => None,
    }
}

/// Letter for a grade as typed; "EC" for extra credit and "—" when it has none.
pub fn letter_for_text(text: &str) -> &'static str {
    match classify(text) {
        Score::Percent(value) => letter_grade(value),
        Score::Bonus(_) => "EC",
        Score::Excused | Score::Unusable => "—",
    }
}

/// Keeps what can be part of a score while it is typed: digits with one decimal separator
/// per number (up to [`MAX_NUMBER_LEN`] characters each), a `/` between points earned and
/// points possible, a leading `+` for extra credit, or `EX`. A comma counts as the decimal
/// separator (and is stored as a dot), so "92,5" typed on a European keyboard isn't
/// silently turned into 925.
pub fn sanitize_grade_input(input: &str) -> String {
    let mut out = String::new();
    let mut number_len = 0;
    let mut has_digit = false;
    let mut has_separator = false;
    let mut after_slash = false;
    let mut bonus = false;
    for c in input.chars() {
        let letters = out.starts_with('E');
        match c {
            '0'..='9' if !letters && number_len < MAX_NUMBER_LEN => {
                out.push(c);
                number_len += 1;
                has_digit = true;
            }
            '.' | ',' if !letters && !has_separator && number_len < MAX_NUMBER_LEN => {
                has_separator = true;
                out.push('.');
                number_len += 1;
            }
            '/' if !after_slash && !bonus && !letters && has_digit => {
                after_slash = true;
                has_separator = false;
                has_digit = false;
                number_len = 0;
                out.push('/');
            }
            '+' if out.is_empty() => {
                bonus = true;
                out.push('+');
            }
            'e' | 'E' if out.is_empty() => out.push('E'),
            'x' | 'X' if out == "E" => out.push('X'),
            _ => {}
        }
    }
    out
}

/// The text to store for a typed grade, or `None` if it isn't a usable score. A trailing
/// decimal point ("92.") is dropped.
pub fn parse_grade_input(input: &str) -> Option<String> {
    let cleaned = sanitize_grade_input(input.trim());
    let tidy = cleaned
        .split('/')
        .map(|part| part.strip_suffix('.').unwrap_or(part))
        .collect::<Vec<_>>()
        .join("/");
    (classify(&tidy) != Score::Unusable).then_some(tidy)
}

/// How one category counts towards an average.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub category: GradeType,
    pub weight: f64,
    /// Leave out the lowest score, when there are at least two.
    pub drop_lowest: bool,
}

/// How the scores of one subject are combined: every category, in the order they are listed.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules(pub Vec<Rule>);

impl Default for Rules {
    /// The three built-in categories with their default weights.
    fn default() -> Rules {
        let rule = |category, weight| Rule {
            category,
            weight,
            drop_lowest: false,
        };
        Rules(vec![
            rule(GradeType::Homework, 33.0),
            rule(GradeType::Test, 50.0),
            rule(GradeType::Quiz, 17.0),
        ])
    }
}

/// The scores of one category that count (as `(id, percentage)`), and the extra credit.
fn category_scores(grades: &[&GradeEntry], category: &GradeType) -> (Vec<(Uuid, f64)>, f64) {
    let mut scores = Vec::new();
    let mut bonus = 0.0;
    for grade in grades.iter().filter(|g| g.grade_type == *category) {
        match classify(&grade.value) {
            Score::Percent(value) => scores.push((grade.id, value)),
            Score::Bonus(points) => bonus += points,
            Score::Excused | Score::Unusable => {}
        }
    }
    (scores, bonus)
}

/// The position of the lowest score (the first, when several are equally low).
fn lowest(scores: &[(Uuid, f64)]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, (_, value)) in scores.iter().enumerate() {
        if best.is_none_or(|b| *value < scores[b].1) {
            best = Some(i);
        }
    }
    best
}

/// The scores that `rules` leave out of an average because they are the lowest of their
/// category.
pub fn dropped_scores<'a>(grades: impl IntoIterator<Item = &'a GradeEntry>, rules: &Rules) -> HashSet<Uuid> {
    let grades: Vec<&GradeEntry> = grades.into_iter().collect();
    let mut dropped = HashSet::new();
    for rule in rules.0.iter().filter(|r| r.drop_lowest) {
        let (scores, _) = category_scores(&grades, &rule.category);
        if scores.len() >= 2 {
            if let Some(position) = lowest(&scores) {
                dropped.insert(scores[position].0);
            }
        }
    }
    dropped
}

/// Weighted average of the given grades, as a percentage.
///
/// Scores are averaged per category, then combined using each category's weight. Weights
/// are re-normalised over the categories that actually have scores, so a student with only
/// tests is graded on tests alone. Within a category, the lowest score is left out if the
/// rules say so, and extra credit adds its points divided by the number of scores.
pub fn percent_average<'a>(grades: impl IntoIterator<Item = &'a GradeEntry>, rules: &Rules) -> Option<f64> {
    let grades: Vec<&GradeEntry> = grades.into_iter().collect();
    if grades.is_empty() {
        return None;
    }

    let mut used_weight = 0.0;
    let mut per_category: Vec<(f64, f64)> = Vec::new(); // (category average, category weight)
    for rule in &rules.0 {
        let (mut scores, bonus) = category_scores(&grades, &rule.category);
        if rule.drop_lowest && scores.len() >= 2 {
            if let Some(position) = lowest(&scores) {
                scores.remove(position);
            }
        }
        if scores.is_empty() {
            continue;
        }
        used_weight += rule.weight;
        let mean = scores.iter().map(|(_, v)| v).sum::<f64>() / scores.len() as f64;
        per_category.push((mean + bonus / scores.len() as f64, rule.weight));
    }

    if used_weight <= 0.0 {
        return None;
    }
    Some(
        per_category
            .iter()
            .map(|(avg, weight)| avg * (weight / used_weight))
            .sum(),
    )
}

/// What a student needs on the next score to reach a target average.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Needed {
    /// Any score keeps the average at or above the target.
    Already,
    /// This score, rounded up to one decimal place, reaches it.
    Score(f64),
    /// Even a 100 isn't enough; it would make the average `best`.
    OutOfReach { best: f64 },
    /// The category has no weight, so a score in it changes nothing.
    NoEffect,
}

/// An average as it is displayed, rounded to one decimal place.
pub fn displayed(average: f64) -> f64 {
    format!("{average:.1}").parse().unwrap_or(average)
}

/// The lowest score in `category`, added to `grades`, that gets the average to `target`.
///
/// What counts is the average as it is displayed (95.96 shows as 96.0, so it reaches 96).
/// The average only ever rises with the new score, even with a lowest score left out, so
/// the answer is found by narrowing down on the real average, with whatever rules apply.
pub fn score_needed(grades: &[GradeEntry], rules: &Rules, category: &GradeType, target: f64) -> Needed {
    if !rules.0.iter().any(|r| r.category == *category && r.weight > 0.0) {
        return Needed::NoEffect;
    }
    let average_with = |score: f64| -> f64 {
        let extra = GradeEntry {
            id: Uuid::nil(),
            student_id: Uuid::nil(),
            subject: String::new(),
            period: 0,
            value: format!("{score}"),
            grade_type: category.clone(),
        };
        percent_average(grades.iter().chain(std::iter::once(&extra)), rules).unwrap_or(0.0)
    };
    let reaches = |score: f64| displayed(average_with(score)) >= target - 1e-9;

    if reaches(0.0) {
        return Needed::Already;
    }
    if !reaches(100.0) {
        return Needed::OutOfReach {
            best: average_with(100.0),
        };
    }
    let (mut low, mut high) = (0.0, 100.0);
    for _ in 0..60 {
        let middle = (low + high) / 2.0;
        if reaches(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    let mut needed = (high * 10.0).ceil() / 10.0;
    if !reaches(needed) {
        needed += 0.1;
    }
    Needed::Score(needed.min(100.0))
}

pub fn mean(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values.into_iter().fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    (count > 0).then(|| sum / count as f64)
}

pub fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 1 {
        values[middle]
    } else {
        (values[middle - 1] + values[middle]) / 2.0
    })
}

/// The letter for an average as it is displayed, with one decimal place: 95.96 shows as
/// 96.0, so it is an A, not an A−.
pub fn average_letter(avg: f64) -> &'static str {
    letter_grade(format!("{avg:.1}").parse().unwrap_or(avg))
}

/// "85.3 B": one decimal place plus the letter, as shown under each student.
/// The letter is derived from the rounded number that is displayed.
pub fn format_average(avg: f64) -> String {
    format!("{avg:.1} {}", average_letter(avg))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn average(grades: &[GradeEntry]) -> Option<f64> {
        percent_average(grades, &Rules::default())
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
        assert_eq!(letter_for_text("18/20"), "B", "18 of 20 is 90%");
        assert_eq!(letter_for_text("+5"), "EC");
        assert_eq!(letter_for_text("EX"), "—");
    }

    #[test]
    fn letters_have_floors() {
        assert_eq!(letter_floor("A−"), Some(94.0));
        assert_eq!(letter_floor("A+"), Some(100.0));
        assert_eq!(letter_floor("F"), None);
    }

    #[test]
    fn scores_come_in_four_kinds() {
        assert_eq!(classify("92.5"), Score::Percent(92.5));
        assert_eq!(classify(".5"), Score::Percent(0.5));
        assert_eq!(classify("18/20"), Score::Percent(90.0));
        assert_eq!(classify("7.5/10"), Score::Percent(75.0));
        assert_eq!(classify("25/20"), Score::Percent(125.0));
        assert_eq!(classify("+5"), Score::Bonus(5.0));
        assert_eq!(classify("+2.5"), Score::Bonus(2.5));
        assert_eq!(classify("EX"), Score::Excused);
        assert_eq!(classify("ex"), Score::Excused);
        for unusable in [
            "", "abc", "NaN", "inf", "1/0", "1/", "/2", "+", "+-5", "1/2/3", "5%", "1e3/5", "--",
        ] {
            assert_eq!(classify(unusable), Score::Unusable, "{unusable:?}");
        }
        assert_eq!(parse_score("18/20"), Some(90.0));
        assert_eq!(parse_score("+5"), None);
        assert_eq!(parse_score("EX"), None);
    }

    #[test]
    fn weighted_average_uses_all_three_types() {
        let grades = [
            entry("90", GradeType::Test),
            entry("80", GradeType::Quiz),
            entry("100", GradeType::Homework),
        ];
        let avg = average(&grades).unwrap();
        assert!((avg - 91.6).abs() < 1e-9, "{avg}");
    }

    #[test]
    fn weights_renormalise_over_the_types_present() {
        let only_tests = [entry("90", GradeType::Test)];
        assert!((average(&only_tests).unwrap() - 90.0).abs() < 1e-9);

        // Quiz (17) and homework (33) only: weights become 17/50 and 33/50.
        let two = [entry("80", GradeType::Quiz), entry("100", GradeType::Homework)];
        let expected = 80.0 * 17.0 / 50.0 + 100.0 * 33.0 / 50.0;
        assert!((average(&two).unwrap() - expected).abs() < 1e-9);
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
        assert!((average(&grades).unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn no_average_without_usable_scores_or_weight() {
        let none: [GradeEntry; 0] = [];
        assert_eq!(average(&none), None);
        assert_eq!(average(&[entry("abc", GradeType::Test)]), None);
        assert_eq!(
            average(&[entry("EX", GradeType::Test)]),
            None,
            "excused alone is no average"
        );
        assert_eq!(
            average(&[entry("+5", GradeType::Test)]),
            None,
            "nor is extra credit alone"
        );
        // The only graded type has weight 0.
        let mut rules = Rules::default();
        rules.0.iter_mut().for_each(|r| r.weight = 0.0);
        assert_eq!(percent_average(&[entry("90", GradeType::Test)], &rules), None);
    }

    #[test]
    fn points_count_as_the_percentage_they_make() {
        let grades = [entry("18/20", GradeType::Test), entry("45/50", GradeType::Test)];
        assert!((average(&grades).unwrap() - 90.0).abs() < 1e-9);
    }

    #[test]
    fn an_excused_score_counts_for_nothing() {
        let with = [entry("80", GradeType::Test), entry("EX", GradeType::Test)];
        let without = [entry("80", GradeType::Test)];
        assert_eq!(average(&with), average(&without));
    }

    #[test]
    fn extra_credit_adds_its_points_spread_over_the_scores_of_its_category() {
        // Two tests averaging 85, and 4 bonus points: 85 + 4 / 2 = 87.
        let grades = [
            entry("90", GradeType::Test),
            entry("80", GradeType::Test),
            entry("+4", GradeType::Test),
        ];
        assert!((average(&grades).unwrap() - 87.0).abs() < 1e-9);
        // In another category it doesn't touch the test average, only its own category's.
        let grades = [
            entry("90", GradeType::Test),
            entry("80", GradeType::Quiz),
            entry("+10", GradeType::Quiz),
        ];
        // tests 90 (weight 50), quizzes 80 + 10 = 90 (weight 17).
        assert!((average(&grades).unwrap() - 90.0).abs() < 1e-9);
    }

    #[test]
    fn the_lowest_score_of_a_category_can_be_dropped() {
        let grades = [
            entry("100", GradeType::Quiz),
            entry("60", GradeType::Quiz),
            entry("80", GradeType::Quiz),
        ];
        let mut rules = Rules::default();
        assert!((percent_average(&grades, &rules).unwrap() - 80.0).abs() < 1e-9);
        rules
            .0
            .iter_mut()
            .find(|r| r.category == GradeType::Quiz)
            .unwrap()
            .drop_lowest = true;
        assert!((percent_average(&grades, &rules).unwrap() - 90.0).abs() < 1e-9);
        let dropped = dropped_scores(&grades, &rules);
        assert_eq!(dropped, HashSet::from([grades[1].id]));

        // A single score is never dropped, and neither is an excused one.
        let single = [entry("60", GradeType::Quiz), entry("EX", GradeType::Quiz)];
        assert!(dropped_scores(&single, &rules).is_empty());
        assert!((percent_average(&single, &rules).unwrap() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn custom_categories_count_with_their_own_weight() {
        let project = GradeType::Custom("Project".into());
        let mut rules = Rules::default();
        rules.0.push(Rule {
            category: project.clone(),
            weight: 50.0,
            drop_lowest: false,
        });
        let grades = [entry("100", project), entry("80", GradeType::Test)];
        // tests 80 (weight 50), project 100 (weight 50).
        assert!((percent_average(&grades, &rules).unwrap() - 90.0).abs() < 1e-9);
        // A score in a category the rules don't know is ignored.
        let stray = [entry("70", GradeType::Custom("Gone".into()))];
        assert_eq!(percent_average(&stray, &rules), None);
    }

    #[test]
    fn mean_of_nothing_is_none() {
        assert_eq!(mean(std::iter::empty()), None);
        assert_eq!(mean([80.0, 100.0]), Some(90.0));
    }

    #[test]
    fn medians() {
        assert_eq!(median(vec![]), None);
        assert_eq!(median(vec![5.0]), Some(5.0));
        assert_eq!(median(vec![9.0, 1.0, 5.0]), Some(5.0));
        assert_eq!(median(vec![4.0, 1.0, 2.0, 3.0]), Some(2.5));
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

    #[test]
    fn points_extra_credit_and_excused_can_be_typed() {
        assert_eq!(sanitize_grade_input("18/20"), "18/20");
        assert_eq!(sanitize_grade_input("18,5/20"), "18.5/20");
        assert_eq!(sanitize_grade_input("1/2/3"), "1/23", "a second slash is ignored");
        assert_eq!(sanitize_grade_input("/5"), "5", "a slash needs points before it");
        assert_eq!(sanitize_grade_input("+5"), "+5");
        assert_eq!(sanitize_grade_input("5+5"), "55", "a plus only starts extra credit");
        assert_eq!(
            sanitize_grade_input("+5/2"),
            "+52",
            "extra credit has no points possible"
        );
        assert_eq!(sanitize_grade_input("ex"), "EX");
        assert_eq!(sanitize_grade_input("EXCUSED"), "EX");
        assert_eq!(sanitize_grade_input("5e"), "5", "letters other than EX are dropped");
        assert_eq!(sanitize_grade_input("E5"), "E");
        assert_eq!(sanitize_grade_input("1234567/1234567"), "123456/123456");

        assert_eq!(parse_grade_input("18/20"), Some("18/20".to_string()));
        assert_eq!(parse_grade_input("18./20."), Some("18/20".to_string()));
        assert_eq!(parse_grade_input("18/0"), None);
        assert_eq!(parse_grade_input("18/"), None);
        assert_eq!(parse_grade_input("+5"), Some("+5".to_string()));
        assert_eq!(parse_grade_input("+"), None);
        assert_eq!(parse_grade_input("ex"), Some("EX".to_string()));
        assert_eq!(parse_grade_input("E"), None, "EX is only complete with the X");
    }

    #[test]
    fn grade_levels_have_names_and_can_be_typed() {
        assert_eq!(grade_level_label(0), "Kindergarten");
        assert_eq!(grade_level_label(3), "Grade 3");
        assert_eq!(parse_grade_level(" k "), Some(0));
        assert_eq!(parse_grade_level("Kindergarten"), Some(0));
        assert_eq!(parse_grade_level("11"), Some(11));
        assert_eq!(parse_grade_level("Grade 12"), Some(12));
        assert_eq!(parse_grade_level("grade12"), Some(12));
        for bad in ["", "grade", "x", "1.5", "-1", "12th", "9999"] {
            assert_eq!(parse_grade_level(bad), None, "{bad:?}");
        }
        assert!(can_add_level(0) && can_add_level(11) && can_add_level(20));
        assert!(!can_add_level(5) && !can_add_level(10) && !can_add_level(21));
    }

    #[test]
    fn the_score_needed_is_found_from_the_real_average() {
        let grades = [entry("80", GradeType::Test)];
        let rules = Rules::default();
        // Only tests count so far, so one more test of x makes the average (80 + x) / 2. The
        // average is shown with one decimal, so a 90 needs 89.95 and 86 needs 85.95.
        assert_eq!(
            score_needed(&grades, &rules, &GradeType::Test, 90.0),
            Needed::Score(99.9)
        );
        assert_eq!(
            score_needed(&grades, &rules, &GradeType::Test, 86.0),
            Needed::Score(91.9)
        );
        assert_eq!(
            score_needed(&grades, &rules, &GradeType::Test, 70.0),
            Needed::Score(59.9)
        );
        // Even a zero leaves (80 + 0) / 2 = 40.
        assert_eq!(score_needed(&grades, &rules, &GradeType::Test, 40.0), Needed::Already);
        assert_eq!(score_needed(&grades, &rules, &GradeType::Test, 10.0), Needed::Already);
        // A quiz joins in at weight 17 against 50: (80 * 50 + x * 17) / 67 >= 84.95 needs x >= 99.51.
        assert_eq!(
            score_needed(&grades, &rules, &GradeType::Quiz, 85.0),
            Needed::Score(99.6)
        );

        // Whatever the target, the answer reaches it as displayed, and a tenth less does not.
        let shown = |score: f64, category: GradeType| {
            let mut all = grades.to_vec();
            all.push(entry(&format!("{score}"), category));
            displayed(percent_average(&all, &rules).unwrap())
        };
        for category in [GradeType::Test, GradeType::Quiz, GradeType::Homework] {
            for target in [55.0, 63.0, 70.0, 76.0, 79.0, 84.0, 86.0, 88.0, 92.0, 94.0, 96.0] {
                match score_needed(&grades, &rules, &category, target) {
                    Needed::Score(score) => {
                        assert!(
                            shown(score, category.clone()) >= target,
                            "{category:?} {target}: {score}"
                        );
                        assert!(
                            shown(score - 0.1, category.clone()) < target,
                            "{category:?} {target}: {score} is not the lowest"
                        );
                    }
                    Needed::Already => assert!(shown(0.0, category.clone()) >= target),
                    Needed::OutOfReach { best } => {
                        assert!(displayed(best) < target && shown(100.0, category.clone()) < target)
                    }
                    Needed::NoEffect => panic!("every category has weight"),
                }
            }
        }
        assert!(
            matches!(score_needed(&grades, &rules, &GradeType::Test, 101.0), Needed::OutOfReach { best } if (best - 90.0).abs() < 1e-9)
        );
    }

    #[test]
    fn the_score_needed_with_no_scores_yet_is_the_target_itself() {
        assert_eq!(
            score_needed(&[], &Rules::default(), &GradeType::Homework, 94.0),
            Needed::Score(94.0)
        );
    }

    #[test]
    fn a_category_without_weight_cannot_help() {
        let mut rules = Rules::default();
        rules
            .0
            .iter_mut()
            .find(|r| r.category == GradeType::Quiz)
            .unwrap()
            .weight = 0.0;
        assert_eq!(
            score_needed(&[entry("50", GradeType::Test)], &rules, &GradeType::Quiz, 90.0),
            Needed::NoEffect
        );
        assert_eq!(
            score_needed(&[], &rules, &GradeType::Custom("Nope".into()), 90.0),
            Needed::NoEffect
        );
    }

    #[test]
    fn the_score_needed_takes_dropped_scores_into_account() {
        let mut rules = Rules::default();
        rules
            .0
            .iter_mut()
            .find(|r| r.category == GradeType::Quiz)
            .unwrap()
            .drop_lowest = true;
        let grades = [entry("60", GradeType::Quiz), entry("100", GradeType::Quiz)];
        // With the lowest dropped, a new score of at least 60 takes the 60's place, so the
        // average becomes (100 + x) / 2 and a displayed 90 needs 79.9.
        assert_eq!(
            score_needed(&grades, &rules, &GradeType::Quiz, 90.0),
            Needed::Score(79.9)
        );
        // Without dropping it would be (60 + 100 + x) / 3, which no score can bring to 90.
        assert!(matches!(
            score_needed(&grades, &Rules::default(), &GradeType::Quiz, 90.0),
            Needed::OutOfReach { .. }
        ));
    }
}
