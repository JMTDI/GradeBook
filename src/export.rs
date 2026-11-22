//! Files the app writes for other programs: CSV for spreadsheets, and a report card as a
//! web page that is printed (or saved as a PDF) from the browser.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::book::GradeBook;
use crate::calc::{self, Score};
use crate::view::ReportCard;

/// A byte order mark. Without it Excel reads a UTF-8 file as Windows-1252 and garbles the
/// "−" in "A−" and any accented names.
const BOM: char = '\u{feff}';

/// A CSV field, quoted when it has to be.
fn quote(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// A field typed by a person, such as a student's name. A spreadsheet takes text that starts
/// with `=`, `+`, `-` or `@` for a formula and runs it, so a name pasted from somewhere
/// untrusted can't start that way: a quote in front makes it plain text.
fn text(field: &str) -> String {
    if field.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        quote(&format!("'{field}"))
    } else {
        quote(field)
    }
}

/// A score as the app lets anyone type it: digits, points like `18/20`, extra credit like `+5`,
/// or `EX`. Those are plain, and a spreadsheet must see them as they are. Anything else can only
/// come from a backup file that was edited, so it is untrusted text like a name is.
fn score(value: &str) -> String {
    let typed_here = calc::classify(value) != Score::Unusable
        && value.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '.' | '+' | 'E' | 'e'));
    if typed_here {
        quote(value)
    } else {
        text(value)
    }
}

fn row(fields: &[String]) -> String {
    let mut line = fields.join(",");
    line.push_str("\r\n");
    line
}

/// A number without trailing zeros: 90, 92.5, 83.33.
fn number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Every score in the gradebook, one per line.
pub fn scores_csv(book: &GradeBook) -> String {
    let mut out = String::from(BOM);
    out.push_str(&row(&[
        "Grade level",
        "Student",
        "Subject",
        "Period",
        "Category",
        "Score",
        "Percent",
        "Letter",
        "Counts",
    ]
    .map(String::from)));
    let mut dropped: HashMap<(Uuid, String, u32), HashSet<Uuid>> = HashMap::new();
    for student in book.students() {
        for grade in book.data().grades.iter().filter(|g| g.student_id == student.id) {
            let left_out = dropped
                .entry((student.id, grade.subject.clone(), grade.period))
                .or_insert_with(|| book.dropped(student.id, &grade.subject, grade.period));
            let counts = match calc::classify(&grade.value) {
                Score::Percent(_) if left_out.contains(&grade.id) => "Dropped (lowest)",
                Score::Percent(_) => "Yes",
                Score::Bonus(_) => "Extra credit",
                Score::Excused => "Excused",
                Score::Unusable => "No",
            };
            // A dropped score keeps its percentage; only its place in the average is gone.
            let percent = calc::parse_score(&grade.value).map(number).unwrap_or_default();
            out.push_str(&row(&[
                quote(&calc::grade_level_label(student.grade_level)),
                text(&student.name),
                text(&grade.subject),
                grade.period.to_string(),
                text(grade.grade_type.label()),
                score(&grade.value),
                percent,
                calc::letter_for_text(&grade.value).to_string(),
                counts.to_string(),
            ]));
        }
    }
    out
}

fn period_headings() -> Vec<String> {
    calc::PERIODS.map(|p| format!("Period {p}")).collect()
}

/// One student's averages by subject and period, a subject per line.
pub fn report_csv(card: &ReportCard) -> String {
    let mut out = String::from(BOM);
    let mut header: Vec<String> = ["Student", "Grade level", "Subject"].map(String::from).to_vec();
    header.extend(period_headings());
    header.extend(["Average", "Letter"].map(String::from));
    out.push_str(&row(&header));

    let mut line = |subject: &str, periods: &[Option<f64>], average: Option<f64>| {
        let mut fields = vec![
            text(&card.name),
            quote(&calc::grade_level_label(card.grade_level)),
            text(subject),
        ];
        fields.extend(periods.iter().map(|p| p.map(number).unwrap_or_default()));
        fields.push(average.map(number).unwrap_or_default());
        fields.push(average.map(|a| calc::average_letter(a).to_string()).unwrap_or_default());
        out.push_str(&row(&fields));
    };
    for subject in &card.subjects {
        line(&subject.subject, &subject.periods, subject.average);
    }
    if !card.subjects.is_empty() {
        line("Overall", &card.periods, card.overall);
    }
    out
}

/// Escapes text for HTML.
fn html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const REPORT_STYLE: &str = "\
body { font: 16px/1.4 system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif; color: #1c1c1e; max-width: 52rem; margin: 2rem auto; padding: 0 1rem; }\
h1 { margin: 0; font-size: 1.8rem; }\
.meta { color: #6c6c70; margin: .2rem 0 1.5rem; }\
table { width: 100%; border-collapse: collapse; }\
th, td { padding: .5rem .6rem; border-bottom: 1px solid #d1d1d6; text-align: center; }\
th:first-child, td:first-child { text-align: left; }\
thead th { border-bottom: 2px solid #1c1c1e; font-size: .85rem; text-transform: uppercase; letter-spacing: .04em; }\
tbody th { font-weight: 600; }\
td.average, tfoot td, tfoot th { font-weight: 700; }\
tfoot th, tfoot td { border-top: 2px solid #1c1c1e; border-bottom: 0; }\
.letter { color: #6c6c70; font-weight: 400; }\
.empty { color: #8e8e93; }\
.hint { margin-top: 2rem; padding: .7rem 1rem; background: #f2f2f7; border-radius: 8px; color: #3c3c43; }\
footer { margin-top: 2rem; color: #8e8e93; font-size: .85rem; }\
@media print { body { margin: 0; max-width: none; } .hint { display: none; } }";

/// An average as a table cell: "85.3" with its letter.
fn cell(average: Option<f64>) -> String {
    match average {
        Some(average) => {
            let shown = calc::format_average(average);
            let (number, letter) = shown.split_once(' ').unwrap_or((&shown, ""));
            format!("{} <span class=\"letter\">{}</span>", html(number), html(letter))
        }
        None => "<span class=\"empty\">—</span>".to_string(),
    }
}

/// A report card as a page for printing. `date` is shown under the name.
pub fn report_html(card: &ReportCard, date: &str) -> String {
    let mut page = String::new();
    page.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    page.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    page.push_str(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">\n",
    );
    page.push_str(&format!("<title>Report card: {}</title>\n", html(&card.name)));
    page.push_str(&format!("<style>{REPORT_STYLE}</style>\n</head>\n<body>\n"));
    page.push_str(&format!(
        "<h1>{}</h1>\n<p class=\"meta\">{} · {}</p>\n",
        html(&card.name),
        html(&calc::grade_level_label(card.grade_level)),
        html(date),
    ));

    if card.subjects.is_empty() {
        page.push_str("<p class=\"empty\">No scores have been entered for this student yet.</p>\n");
    } else {
        page.push_str("<table>\n<thead><tr><th>Subject</th>");
        for heading in period_headings() {
            page.push_str(&format!("<th>{}</th>", html(&heading.replace("Period ", "P"))));
        }
        page.push_str("<th>Average</th></tr></thead>\n<tbody>\n");
        for subject in &card.subjects {
            page.push_str(&format!("<tr><th scope=\"row\">{}</th>", html(&subject.subject)));
            for average in &subject.periods {
                page.push_str(&format!("<td>{}</td>", cell(*average)));
            }
            page.push_str(&format!("<td class=\"average\">{}</td></tr>\n", cell(subject.average)));
        }
        page.push_str("</tbody>\n<tfoot><tr><th scope=\"row\">Overall</th>");
        for average in &card.periods {
            page.push_str(&format!("<td>{}</td>", cell(*average)));
        }
        page.push_str(&format!("<td>{}</td></tr></tfoot>\n</table>\n", cell(card.overall)));
    }

    page.push_str(
        "<p class=\"hint\">To print this report or save it as a PDF, use your browser's Print command \
         (Ctrl+P, or Cmd+P on a Mac).</p>\n<footer>Made with GradeBook</footer>\n</body>\n</html>\n",
    );
    page
}

/// A piece of a student's name that is safe in a file name: letters, digits and dashes.
pub fn file_stem(name: &str) -> String {
    let mut stem = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            stem.push(c);
        } else if (c.is_whitespace() || c == '-' || c == '_') && !stem.ends_with('-') && !stem.is_empty() {
            stem.push('-');
        }
    }
    let stem: String = stem.trim_end_matches('-').chars().take(40).collect();
    if stem.is_empty() {
        "student".to_string()
    } else {
        stem
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GradeType;
    use crate::view;

    fn lines(csv: &str) -> Vec<&str> {
        csv.trim_start_matches(BOM).trim_end().split("\r\n").collect()
    }

    fn sample() -> GradeBook {
        let mut book = GradeBook::default();
        let ada = book.add_student("Ada, \"the Countess\"", 3).unwrap();
        let evil = book.add_student("=HYPERLINK(\"http://x\")", 3).unwrap();
        book.add_grade(ada, "Math", 1, "90".into(), GradeType::Test);
        book.add_grade(ada, "Math", 1, "18/20".into(), GradeType::Quiz);
        book.add_grade(ada, "Math", 1, "+5".into(), GradeType::Test);
        book.add_grade(ada, "Math", 1, "EX".into(), GradeType::Homework);
        book.add_grade(evil, "Art", 2, "75.5".into(), GradeType::Test);
        book
    }

    #[test]
    fn the_scores_file_has_every_score_and_opens_in_excel() {
        let csv = scores_csv(&sample());
        assert!(csv.starts_with(BOM), "a byte order mark, so Excel reads UTF-8");
        assert!(
            csv.contains("\r\n") && !csv.replace("\r\n", "").contains('\n'),
            "CRLF line ends"
        );
        let lines = lines(&csv);
        assert_eq!(
            lines[0],
            "Grade level,Student,Subject,Period,Category,Score,Percent,Letter,Counts"
        );
        assert_eq!(lines.len(), 6, "a header and five scores");
        assert_eq!(
            lines[1],
            "Grade 3,\"Ada, \"\"the Countess\"\"\",Math,1,Test,90,90,B,Yes"
        );
        assert_eq!(
            lines[2],
            "Grade 3,\"Ada, \"\"the Countess\"\"\",Math,1,Quiz,18/20,90,B,Yes"
        );
        assert_eq!(
            lines[3],
            "Grade 3,\"Ada, \"\"the Countess\"\"\",Math,1,Test,+5,,EC,Extra credit"
        );
        assert_eq!(
            lines[4],
            "Grade 3,\"Ada, \"\"the Countess\"\"\",Math,1,Homework,EX,,—,Excused"
        );
    }

    #[test]
    fn text_that_could_run_as_a_formula_is_made_plain() {
        let csv = scores_csv(&sample());
        let last = lines(&csv).pop().unwrap();
        assert!(
            last.starts_with("Grade 3,\"'=HYPERLINK(\"\"http://x\"\")\",Art,2,Test,75.5,75.5,"),
            "{last}"
        );
        for name in ["=1+1", "+1", "-1", "@SUM(A1)", "\tx"] {
            assert!(text(name).starts_with(['\'', '"']), "{name:?} -> {}", text(name));
        }
        assert_eq!(text("Ada"), "Ada");
        assert_eq!(text("a-b"), "a-b", "only a leading character matters");
    }

    #[test]
    fn a_score_that_could_run_as_a_formula_is_made_plain_but_real_scores_are_left_alone() {
        let mut book = GradeBook::default();
        let ada = book.add_student("Ada", 1).unwrap();
        // What can be typed in the app.
        for value in ["92.5", "18/20", "+5", "EX", "ex"] {
            book.add_grade(ada, "Math", 1, value.into(), GradeType::Test);
        }
        // What only a backup file that was edited can hold.
        for value in ["=1+1", "@SUM(A1)", "-5", "\t7", "+cmd", "=HYPERLINK(\"http://x\")"] {
            book.add_grade(ada, "Math", 1, value.into(), GradeType::Test);
        }
        let csv = scores_csv(&book);
        for plain in [
            ",Test,92.5,92.5,",
            ",Test,18/20,90,",
            ",Test,+5,,EC,Extra credit",
            ",Test,EX,,\u{2014},Excused",
            ",Test,ex,,\u{2014},Excused",
        ] {
            assert!(csv.contains(plain), "{plain:?} in {csv}");
        }
        for made_plain in [
            ",Test,'=1+1,",
            ",Test,'@SUM(A1),",
            ",Test,'-5,",
            ",Test,'\t7,",
            ",Test,'+cmd,",
            ",Test,\"'=HYPERLINK(\"\"http://x\"\")\",",
        ] {
            assert!(csv.contains(made_plain), "{made_plain:?} in {csv}");
        }
    }

    #[test]
    fn a_dropped_score_keeps_its_percentage_but_is_marked() {
        let mut book = GradeBook::default();
        let ada = book.add_student("Ada", 1).unwrap();
        book.add_grade(ada, "Math", 1, "60".into(), GradeType::Quiz);
        book.add_grade(ada, "Math", 1, "100".into(), GradeType::Quiz);
        book.set_drop_lowest(&GradeType::Quiz, true);
        let csv = scores_csv(&book);
        let lines = lines(&csv);
        assert_eq!(lines[1], "Grade 1,Ada,Math,1,Quiz,60,60,F,Dropped (lowest)");
        assert_eq!(lines[2], "Grade 1,Ada,Math,1,Quiz,100,100,A+,Yes");
    }

    #[test]
    fn an_empty_gradebook_exports_only_the_header() {
        assert_eq!(lines(&scores_csv(&GradeBook::default())).len(), 1);
    }

    #[test]
    fn numbers_have_no_trailing_zeros() {
        for (value, shown) in [
            (90.0, "90"),
            (92.5, "92.5"),
            (83.3333, "83.33"),
            (0.0, "0"),
            (100.0, "100"),
            (99.999, "100"),
        ] {
            assert_eq!(number(value), shown);
        }
    }

    fn card() -> ReportCard {
        let book = sample();
        let ada = book.students()[0].id;
        view::report_card(&book, ada).unwrap()
    }

    #[test]
    fn the_report_file_has_a_line_per_subject_and_an_overall_line() {
        let csv = report_csv(&card());
        let lines = lines(&csv);
        assert_eq!(
            lines[0],
            "Student,Grade level,Subject,Period 1,Period 2,Period 3,Period 4,Period 5,Period 6,Average,Letter"
        );
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[1].starts_with("\"Ada, \"\"the Countess\"\"\",Grade 3,Math,"));
        assert!(lines[1].contains(",,,,,"), "periods without scores are empty");
        assert!(lines[2].contains(",Overall,"), "{}", lines[2]);

        let none = view::report_card(&GradeBook::default(), Uuid::new_v4());
        assert!(none.is_none());
        let mut book = GradeBook::default();
        let id = book.add_student("Bo", 1).unwrap();
        assert_eq!(
            lines_count(&report_csv(&view::report_card(&book, id).unwrap())),
            1,
            "just the header"
        );
    }

    fn lines_count(csv: &str) -> usize {
        lines(csv).len()
    }

    #[test]
    fn the_printable_report_escapes_everything_and_has_no_scripts() {
        let mut book = GradeBook::default();
        let id = book.add_student("<script>alert(1)</script> & \"Co\"", 3).unwrap();
        book.add_subject("<b>Bold</b>").unwrap();
        book.add_grade(id, "<b>Bold</b>", 1, "90".into(), GradeType::Test);
        let card = view::report_card(&book, id).unwrap();
        let page = report_html(&card, "20 November 2026");

        assert!(page.starts_with("<!doctype html>"));
        assert!(
            !page.contains("<script") && !page.contains("<b>"),
            "nothing typed is left as markup"
        );
        assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;Co&quot;"));
        assert!(page.contains("&lt;b&gt;Bold&lt;/b&gt;"));
        assert!(page.contains("Content-Security-Policy"));
        assert!(page.contains("Grade 3 · 20 November 2026"));
        assert!(page.contains("90.0 <span class=\"letter\">B</span>"));
        assert!(page.contains("<th scope=\"row\">Overall</th>"));
        assert!(page.contains("<th>P1</th>") && page.contains("<th>P6</th>"));
    }

    #[test]
    fn a_report_without_scores_says_so() {
        let mut book = GradeBook::default();
        let id = book.add_student("Bo", 1).unwrap();
        let page = report_html(&view::report_card(&book, id).unwrap(), "today");
        assert!(page.contains("No scores have been entered"));
        assert!(!page.contains("<table>"));
    }

    #[test]
    fn file_names_are_made_of_safe_characters() {
        assert_eq!(file_stem("Ada Lovelace"), "Ada-Lovelace");
        assert_eq!(file_stem("  ../../etc/passwd  "), "etcpasswd");
        assert_eq!(file_stem("A  B__C"), "A-B-C");
        assert_eq!(file_stem("José Núñez"), "José-Núñez");
        assert_eq!(file_stem("***"), "student");
        assert_eq!(file_stem(""), "student");
        assert_eq!(file_stem(&"x".repeat(100)).chars().count(), 40);
        assert!(!file_stem("a/b\\c:d*e?f\"g<h>i|j").contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']));
    }
}
