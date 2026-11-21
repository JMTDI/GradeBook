//! Drives the real UI headlessly: locates controls by their accessibility labels,
//! clicks them through the normal pointer pipeline, types into text fields, and
//! checks both what the screen shows and what ends up on disk.
//!
//! Slint's testing backend can only be initialised once per process, so this is a
//! single test that walks through the whole app in order. Each part leaves the data the
//! next one expects: two students, Ada Lovelace and Grace Hopper, in Grade 1.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::NaiveDate;
use i_slint_backend_testing::{
    init_integration_test_with_mock_time, mock_elapsed_time, AccessibleRole, ElementHandle, ElementQuery,
};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, Model, ModelRc, SharedString};

use gradebook::app::App;
use gradebook::model::{BackupData, GradeType};
use gradebook::prefs::{Appearance, Preferences};
use gradebook::store::{self, Store};
use gradebook::ui::{Backend, StudentRowData, Theme};

const MINUS: &str = "\u{2212}";

/// Lets what is waiting run: handlers of changed properties, timers, dialogs that came up.
fn settle() {
    mock_elapsed_time(Duration::from_millis(1));
}

fn resize(app: &App, width: f32, height: f32) {
    app.window().window().set_size(slint::LogicalSize::new(width, height));
    settle();
}

fn element(app: &App, label: &str) -> ElementHandle {
    ElementHandle::find_by_accessible_label(app.window(), label)
        .next()
        .unwrap_or_else(|| panic!("no control labelled {label:?}"))
}

/// Every control with this role and label, in the order they are drawn: what is on top is last.
fn with_role(app: &App, role: AccessibleRole, label: &str) -> Vec<ElementHandle> {
    let wanted = label.to_string();
    ElementQuery::from_root(app.window())
        .match_accessible_role(role)
        .match_predicate(move |e| e.accessible_label().is_some_and(|l| l == wanted))
        .find_all()
}

/// The topmost button with this label (the buttons of an alert or a dialog sit above the
/// page's).
fn button(app: &App, label: &str) -> ElementHandle {
    with_role(app, AccessibleRole::Button, label)
        .pop()
        .unwrap_or_else(|| panic!("no button labelled {label:?}"))
}

/// The lowest button with this label: the one in the header, not the one on the bar below.
fn first_button(app: &App, label: &str) -> ElementHandle {
    with_role(app, AccessibleRole::Button, label)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no button labelled {label:?}"))
}

/// Presses a button without the pointer, then lets what it set off run.
fn invoke(element: &ElementHandle) {
    element.invoke_accessible_default_action();
    settle();
}

fn count(app: &App, label: &str) -> usize {
    ElementHandle::find_by_accessible_label(app.window(), label).count()
}

/// The horizontal position of the first control with this role and label (for the type
/// selector and the legend, that is the one belonging to the first row).
fn x_of(app: &App, role: AccessibleRole, label: &str) -> f32 {
    with_role(app, role, label)
        .first()
        .unwrap_or_else(|| panic!("no {role:?} labelled {label:?}"))
        .absolute_position()
        .x
}

fn click(app: &App, label: &str) {
    element(app, label).mock_single_click(PointerEventButton::Left);
    settle();
}

/// Clicks a tab of the first student's type selector. The legend above the list has text with
/// the same words, so the role has to be asked for.
fn click_tab(app: &App, label: &str) {
    with_role(app, AccessibleRole::Tab, label)
        .first()
        .unwrap_or_else(|| panic!("no tab labelled {label:?}"))
        .mock_single_click(PointerEventButton::Left);
    settle();
}

/// Clicks the topmost button with this label. The text inside a button carries the label of
/// its words too, so a word like "Remove" can be found in the list behind an alert as well.
fn click_button(app: &App, label: &str) {
    button(app, label).mock_single_click(PointerEventButton::Left);
    settle();
}

/// Focuses a text field with a real click and types into it key by key.
fn type_into(app: &App, label: &str, text: &str) {
    click(app, label);
    for ch in text.chars() {
        press(app, ch.to_string());
    }
}

fn press(app: &App, text: impl Into<SharedString>) {
    let text = text.into();
    let window = app.window().window();
    window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
    settle();
}

/// Presses a key with some modifiers held down, such as Control (Command on a Mac).
fn press_with(app: &App, held: &[Key], text: &str) {
    let window = app.window().window();
    for key in held {
        window.dispatch_event(WindowEvent::KeyPressed { text: (*key).into() });
    }
    press(app, text);
    for key in held.iter().rev() {
        window.dispatch_event(WindowEvent::KeyReleased { text: (*key).into() });
    }
}

fn value(app: &App, label: &str) -> String {
    element(app, label)
        .accessible_value()
        .map(|v| v.to_string())
        .unwrap_or_default()
}

fn enabled(app: &App, label: &str) -> bool {
    element(app, label).accessible_enabled().unwrap_or(true)
}

fn saved(dir: &Path) -> BackupData {
    store::read_backup_file(&dir.join("gradebook.json")).expect("data file")
}

/// The copies of the data kept next to the data file before each restore.
fn restore_copies(dir: &Path) -> Vec<BackupData> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("gradebook.before-restore-") && name.ends_with(".json"))
        })
        .map(|path| store::read_backup_file(&path).unwrap())
        .collect()
}

fn texts(model: ModelRc<SharedString>) -> Vec<String> {
    model.iter().map(|text| text.to_string()).collect()
}

fn row_names(app: &App) -> Vec<String> {
    app.window()
        .global::<Backend>()
        .get_rows()
        .iter()
        .map(|r| r.name.to_string())
        .collect()
}

fn row_of(app: &App, name: &str) -> StudentRowData {
    app.window()
        .global::<Backend>()
        .get_rows()
        .iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("{name} is not on the Grades screen"))
}

/// The scores shown for a student, as typed.
fn chips(app: &App, name: &str) -> Vec<String> {
    row_of(app, name).chips.iter().map(|c| c.value.to_string()).collect()
}

fn comboboxes(app: &App) -> Vec<ElementHandle> {
    ElementQuery::from_root(app.window())
        .match_accessible_role(AccessibleRole::Combobox)
        .find_all()
}

/// A release as the GitHub API describes it.
fn release(tag: &str) -> String {
    format!(
        r#"{{"tag_name":"{tag}","html_url":"https://github.com/JMTDI/GradeBook/releases/tag/{tag}","draft":false,"prerelease":false}}"#
    )
}

#[test]
fn the_app_works_end_to_end() {
    init_integration_test_with_mock_time();

    let dir = tempfile::tempdir().unwrap();
    let app = App::new(Some(Store::in_dir(dir.path()))).unwrap();
    app.window().show().unwrap();
    resize(&app, 1000.0, 760.0);

    // What would be opened is recorded instead of opened, and the day is a fixed one.
    let opened: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let recorder = opened.clone();
    app.set_opener(move |target| {
        recorder.borrow_mut().push(PathBuf::from(target));
        Ok(())
    });
    app.set_today(|| NaiveDate::from_ymd_opt(2031, 6, 1).unwrap());

    starting_empty(&app);
    students(&app, dir.path());
    scores(&app, dir.path());
    score_kinds(&app, dir.path());
    insights(&app, dir.path());
    weights(&app, dir.path());
    dialogs_and_keys(&app, dir.path());
    lists(&app, dir.path());
    keyboard_after_undo(&app, dir.path());
    new_school_year(&app, dir.path());
    files_and_reports(&app, dir.path(), &opened);
    updates(&app, dir.path(), &opened);
    update_choice_at_start();
    update_bar_from_the_keyboard();
    backup_and_restore(&app, dir.path());
    damaged_and_unsaved();
    data_folder_before_the_first_save();
    daily_backup(dir.path());
    light_and_dark();
}

fn starting_empty(app: &App) {
    let backend = app.window().global::<Backend>();
    assert!(!backend.get_can_undo() && !backend.get_can_redo());
    assert!(
        !enabled(app, "Undo") && !enabled(app, "Redo"),
        "nothing to take back yet"
    );
    assert!(!backend.get_toast().visible);
    assert_eq!(backend.get_rows().row_count(), 0);

    // Grades 1 to 10, six periods, the usual subjects and the three kinds of scores.
    assert_eq!(backend.get_grade_level_options().row_count(), 10);
    assert_eq!(backend.get_grade_level_options().row_data(0).unwrap(), "Grade 1");
    assert_eq!(backend.get_grade_level_options().row_data(9).unwrap(), "Grade 10");
    assert_eq!(backend.get_period_options().row_count(), 6);
    assert_eq!(backend.get_subject_options().row_count(), 20);
    assert_eq!(texts(backend.get_category_names()), ["Homework", "Test", "Quiz"]);
    assert_eq!(
        backend.get_addable_level_options().row_count(),
        11,
        "Kindergarten and grades 11 to 20 can be added"
    );
}

// ---- Students -------------------------------------------------------------------------------

fn students(app: &App, dir: &Path) {
    let backend = app.window().global::<Backend>();

    // Adding one at a time.
    app.set_tab(1);
    assert!(!enabled(app, "Add"), "Add must be disabled while the name is empty");
    type_into(app, "Student Name", "   ");
    assert!(!enabled(app, "Add"), "whitespace is not a name");

    // Replace the spaces with a real name, typed key by key, then submit with Return.
    for _ in 0..3 {
        press(app, Key::Backspace);
    }
    type_into(app, "Student Name", "Ada Lovelace");
    assert_eq!(value(app, "Student Name"), "Ada Lovelace");
    assert!(enabled(app, "Add"));
    click(app, "Add");
    assert_eq!(value(app, "Student Name"), "", "the field is cleared after adding");
    assert_eq!(saved(dir).students.len(), 1);
    assert_eq!(saved(dir).students[0].grade_level, 1);
    assert!(enabled(app, "Undo"), "there is something to take back now");

    type_into(app, "Student Name", "Grace Hopper");
    press(app, Key::Return);
    assert_eq!(saved(dir).students.len(), 2, "Return adds the student too");
    assert_eq!(backend.get_students().row_count(), 2);

    // Pasting a class list: numbers and bullets are dropped, blank lines are skipped and the
    // cells of a spreadsheet row are joined.
    click(app, "Paste a Class List");
    assert_eq!(backend.get_dialog(), 3);
    assert!(!enabled(app, "Add Students"), "nothing to add yet");
    element(app, "Names, one per line")
        .set_accessible_value("1. Alan Turing\n2) Katherine Johnson\n- Linus\n\n   \nAda\tByron\n");
    settle();
    assert!(enabled(app, "Add Students"));
    click(app, "Add Students");
    assert_eq!(backend.get_dialog(), 0, "the dialog closes");
    let data = saved(dir);
    let names: Vec<&str> = data.students.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Ada Lovelace",
            "Grace Hopper",
            "Alan Turing",
            "Katherine Johnson",
            "Linus",
            "Ada Byron"
        ]
    );
    assert!(data.students.iter().all(|s| s.grade_level == 1));

    // The bar at the bottom offers to take it back; the buttons in the corner do too.
    let toast = backend.get_toast();
    assert!(toast.visible);
    assert_eq!(
        (toast.message.as_str(), toast.action.as_str()),
        ("4 students added", "Undo")
    );
    invoke(&button(app, "Undo"));
    assert_eq!(saved(dir).students.len(), 2);
    let toast = backend.get_toast();
    assert_eq!(
        (toast.message.as_str(), toast.action.as_str()),
        ("Undone: 4 students added", "Redo")
    );
    assert!(enabled(app, "Redo"));
    assert_eq!(
        first_button(app, "Redo").accessible_description().as_deref(),
        Some("4 students added"),
        "the button says what it would do again"
    );
    invoke(&first_button(app, "Redo"));
    assert_eq!(saved(dir).students.len(), 6);
    assert_eq!(
        first_button(app, "Undo").accessible_description().as_deref(),
        Some("4 students added")
    );
    invoke(&first_button(app, "Undo"));
    assert_eq!(saved(dir).students.len(), 2);

    // Editing a student: the name and the grade level.
    type_into(app, "Student Name", "Temp");
    press(app, Key::Return);
    click(app, "Edit Temp");
    assert_eq!(backend.get_dialog(), 2);
    assert_eq!(value(app, "Name"), "Temp");
    element(app, "Name").set_accessible_value("Edited Name");
    settle();
    let level = comboboxes(app).pop().expect("the grade level of the dialog");
    level.invoke_accessible_expand_action();
    press(app, Key::DownArrow);
    press(app, Key::Escape);
    click(app, "Save");
    assert_eq!(backend.get_dialog(), 0);
    let data = saved(dir);
    assert_eq!(
        (data.students[2].name.as_str(), data.students[2].grade_level),
        ("Edited Name", 2)
    );
    assert_eq!(backend.get_toast().message, "Student edited");

    // Deleting is two steps, and can be taken back.
    click(app, "Delete Edited Name");
    assert!(count(app, "Confirm delete Edited Name") > 0);
    assert_eq!(
        backend.get_students().row_count(),
        3,
        "revealing the button does not delete yet"
    );
    invoke(&element(app, "Confirm delete Edited Name"));
    assert_eq!(saved(dir).students.len(), 2);
    assert_eq!(backend.get_toast().message, "Student deleted");
}

// ---- Typing and adding scores -------------------------------------------------------------------

fn scores(app: &App, dir: &Path) {
    let backend = app.window().global::<Backend>();
    app.set_tab(0);
    assert_eq!(row_names(app), ["Ada Lovelace", "Grace Hopper"]);

    // Scores are entered as Homework, Test or Quiz, in that order, on the selector and
    // in the legend of tints above the list.
    let tab = |label| x_of(app, AccessibleRole::Tab, label);
    assert!(tab("Homework") < tab("Test") && tab("Test") < tab("Quiz"));
    let legend = |label| x_of(app, AccessibleRole::Text, label);
    assert!(legend("Homework") < legend("Tests") && legend("Tests") < legend("Quizzes"));

    // Letters are rejected while typing, a comma counts as the decimal separator.
    type_into(app, "Grade for Ada Lovelace", "9x2,5");
    assert_eq!(value(app, "Grade for Ada Lovelace"), "92.5");
    press(app, Key::Return);
    let data = saved(dir);
    assert_eq!(data.grades.len(), 1, "Return adds the score");
    assert_eq!(
        (data.grades[0].value.as_str(), &data.grades[0].grade_type),
        ("92.5", &GradeType::Test)
    );
    assert_eq!((data.grades[0].subject.as_str(), data.grades[0].period), ("Math", 1));
    assert_eq!(
        value(app, "Grade for Ada Lovelace"),
        "",
        "the field is cleared after adding"
    );

    // And the keyboard goes on to the next student's field.
    press(app, "6");
    press(app, "1");
    assert_eq!(value(app, "Grade for Grace Hopper"), "61");
    press(app, Key::Backspace);
    press(app, Key::Backspace);
    assert_eq!(value(app, "Grade for Grace Hopper"), "");

    let ada = row_of(app, "Ada Lovelace");
    assert_eq!(ada.chips.row_count(), 1);
    assert_eq!(ada.chips.row_data(0).unwrap().letter, "B+");
    assert_eq!(ada.average, "92.5 B+");
    assert_eq!(backend.get_class_average(), "92.5 B+");

    // The type selector is shared by every row, and starts on Test.
    assert_eq!(backend.get_grade_type(), 1);
    click_tab(app, "Quiz");
    assert_eq!(backend.get_grade_type(), 2);
    type_into(app, "Grade for Grace Hopper", "70");
    click(app, "Add grade for Grace Hopper");
    let data = saved(dir);
    assert_eq!(data.grades.len(), 2);
    assert_eq!(data.grades[1].grade_type, GradeType::Quiz);
    assert_eq!(
        backend.get_class_average(),
        "81.2 C",
        "mean of 92.5 and 70 is 81.25, shown as 81.2"
    );

    // Unusable input is ignored and left in the field.
    type_into(app, "Grade for Grace Hopper", ".");
    click(app, "Add grade for Grace Hopper");
    assert_eq!(saved(dir).grades.len(), 2);
    assert_eq!(value(app, "Grade for Grace Hopper"), ".");

    // A typed-but-unsent score survives adding a score for someone else.
    type_into(app, "Grade for Ada Lovelace", "55");
    click(app, "Add grade for Grace Hopper");
    assert_eq!(value(app, "Grade for Ada Lovelace"), "55");

    // Clicking a tab takes the keyboard focus off the page being hidden, so
    // keystrokes can't end up in its fields.
    click(app, "Settings");
    press(app, "7");
    click(app, "Grades");
    assert_eq!(value(app, "Grade for Ada Lovelace"), "55");

    // The picker menu works from the keyboard.
    let pickers = comboboxes(app);
    assert_eq!(
        pickers.len(),
        3,
        "Grade, Period and Subject (the other screens are hidden)"
    );
    assert_eq!(backend.get_grade_level_index(), 0);
    pickers[0].invoke_accessible_expand_action();
    press(app, Key::DownArrow);
    assert_eq!(backend.get_grade_level_index(), 1, "Down picks the next option");
    assert!(
        row_names(app).is_empty(),
        "and the list follows: grade 2 has no students"
    );
    press(app, Key::UpArrow);
    assert_eq!(backend.get_grade_level_index(), 0);
    assert_eq!(row_names(app), ["Ada Lovelace", "Grace Hopper"]);
    press(app, Key::Escape);
    press(app, Key::DownArrow);
    assert_eq!(backend.get_grade_level_index(), 0, "Escape closed the menu");

    // Changing the filters.
    backend.set_period_index(1);
    backend.invoke_filters_changed();
    assert_eq!(
        row_of(app, "Ada Lovelace").chips.row_count(),
        0,
        "period 2 has no scores"
    );
    assert_eq!(backend.get_class_average(), "");
    backend.set_period_index(0);
    backend.invoke_filters_changed();
    assert_eq!(row_of(app, "Ada Lovelace").chips.row_count(), 1);
    assert_eq!(
        value(app, "Grade for Ada Lovelace"),
        "55",
        "drafts survive a filter change"
    );

    // Removing a score, and taking that back from the keyboard. The shortcut works wherever the
    // keyboard is: on no field at all, in an empty field, and in a field with text, where Ctrl+Z
    // undoes the typing first.
    click(app, "Remove grade 92.5 for Ada Lovelace");
    assert_eq!(saved(dir).grades.len(), 1);
    assert_eq!(row_of(app, "Ada Lovelace").average, "");
    assert_eq!(backend.get_class_average(), "70.0 D");
    let toast = backend.get_toast();
    assert_eq!(
        (toast.message.as_str(), toast.action.as_str()),
        ("Score deleted", "Undo")
    );

    app.set_tab(1);
    settle();
    app.set_tab(0);
    settle();
    press_with(app, &[Key::Control], "z");
    assert_eq!(saved(dir).grades.len(), 2, "Ctrl+Z puts the score back");
    assert_eq!(row_of(app, "Ada Lovelace").average, "92.5 B+");
    press_with(app, &[Key::Control], "y");
    assert_eq!(saved(dir).grades.len(), 1, "Ctrl+Y does it again");
    press_with(app, &[Key::Control], "z");
    assert_eq!(saved(dir).grades.len(), 2);
    press_with(app, &[Key::Control, Key::Shift], "z");
    assert_eq!(saved(dir).grades.len(), 1, "so does Ctrl+Shift+Z");

    element(app, "Grade for Ada Lovelace").set_accessible_value("");
    settle();
    click(app, "Grade for Ada Lovelace");
    press_with(app, &[Key::Control], "z");
    assert_eq!(saved(dir).grades.len(), 2, "in an empty field too");
    press_with(app, &[Key::Control, Key::Shift], "z");
    assert_eq!(saved(dir).grades.len(), 1);

    press(app, "7");
    press_with(app, &[Key::Control], "z");
    assert_eq!(
        value(app, "Grade for Ada Lovelace"),
        "",
        "with text in the field it undoes the typing"
    );
    assert_eq!(saved(dir).grades.len(), 1, "and leaves the scores alone");
    press_with(app, &[Key::Control], "z");
    assert_eq!(
        saved(dir).grades.len(),
        2,
        "the next one, with nothing left to undo there, is the app's"
    );
    press_with(app, &[Key::Control], "y");
    assert_eq!(saved(dir).grades.len(), 1);
    assert_eq!(row_of(app, "Ada Lovelace").average, "");

    element(app, "Grade for Grace Hopper").set_accessible_value("");
    settle();
    assert_eq!(backend.get_class_average(), "70.0 D");

    // The bar that offers to take a removal back is for that removal. The next change takes it
    // away, so that its button can't take back a different step.
    click(app, "Remove grade 70 for Grace Hopper");
    assert_eq!(backend.get_toast().message, "Score deleted");
    type_into(app, "Grade for Ada Lovelace", "65");
    press(app, Key::Return);
    assert_eq!(chips(app, "Ada Lovelace"), ["65"]);
    assert!(!backend.get_toast().visible, "the history has moved on");
    assert_eq!(
        with_role(app, AccessibleRole::Button, "Undo").len(),
        1,
        "only the button in the header is left"
    );
    press_with(app, &[Key::Control], "z");
    press_with(app, &[Key::Control], "z");
    assert_eq!(chips(app, "Ada Lovelace"), Vec::<String>::new());
    assert_eq!(chips(app, "Grace Hopper"), ["70"]);
    assert_eq!(backend.get_class_average(), "70.0 D");
}

// ---- Kinds of scores ---------------------------------------------------------------------------

/// Points, extra credit and excused scores; leaving out the lowest score; what a student
/// needs on the next one. Ada has no scores when this starts, and ends with these Math ones:
/// Homework 100, 60, EX, +5, 18/20 and a Test 88.
fn score_kinds(app: &App, dir: &Path) {
    let backend = app.window().global::<Backend>();
    app.set_tab(0);
    click_tab(app, "Homework");
    assert_eq!(backend.get_grade_type(), 0);

    for score in ["100", "60"] {
        type_into(app, "Grade for Ada Lovelace", score);
        press(app, Key::Return);
    }
    assert_eq!(row_of(app, "Ada Lovelace").average, "80.0 C");

    // EX is an excused score: shown, and not counted. Typing it in lower case is fine.
    type_into(app, "Grade for Ada Lovelace", "ex");
    assert_eq!(value(app, "Grade for Ada Lovelace"), "EX");
    press(app, Key::Return);
    assert_eq!(
        row_of(app, "Ada Lovelace").average,
        "80.0 C",
        "an excused score changes nothing"
    );

    // +5 is extra credit: five points on the total of the category.
    type_into(app, "Grade for Ada Lovelace", "+5");
    press(app, Key::Return);
    assert_eq!(row_of(app, "Ada Lovelace").average, "82.5 C");

    // 18/20 is points, worth 90 percent.
    type_into(app, "Grade for Ada Lovelace", "18/20");
    assert_eq!(value(app, "Grade for Ada Lovelace"), "18/20");
    press(app, Key::Return);
    assert_eq!(chips(app, "Ada Lovelace"), ["100", "60", "EX", "+5", "18/20"]);
    let letters: Vec<String> = row_of(app, "Ada Lovelace")
        .chips
        .iter()
        .map(|c| c.letter.to_string())
        .collect();
    assert_eq!(letters, ["A+", "F", "\u{2014}", "EC", "B"]);
    assert_eq!(row_of(app, "Ada Lovelace").average, "85.0 C+");
    let data = saved(dir);
    assert!(data.grades.iter().any(|g| g.value == "18/20" && g.subject == "Math"));

    // Leaving out the lowest Homework score is a switch in the Settings.
    app.set_tab(3);
    let switch = element(app, "Drop lowest Homework");
    assert_eq!(switch.accessible_checked(), Some(false));
    invoke(&switch);
    assert_eq!(switch.accessible_checked(), Some(true));
    assert_eq!(saved(dir).options.drop_lowest, [GradeType::Homework]);
    assert_eq!(backend.get_undo_text(), "Dropped scores changed");
    app.set_tab(0);
    let dropped: Vec<bool> = row_of(app, "Ada Lovelace").chips.iter().map(|c| c.dropped).collect();
    assert_eq!(dropped, [false, true, false, false, false], "the 60 is the lowest");
    assert_eq!(row_of(app, "Ada Lovelace").average, "97.5 A");
    app.set_tab(3);
    invoke(&element(app, "Drop lowest Homework"));
    assert!(saved(dir).options.drop_lowest.is_empty());
    app.set_tab(0);
    assert_eq!(row_of(app, "Ada Lovelace").average, "85.0 C+");

    // A Test of 88, and what Ada needs on the next one.
    click_tab(app, "Test");
    type_into(app, "Grade for Ada Lovelace", "88");
    press(app, Key::Return);
    assert_eq!(row_of(app, "Ada Lovelace").average, format!("86.8 B{MINUS}"));

    click(app, "What does Ada Lovelace need?");
    assert_eq!(backend.get_dialog(), 4);
    assert_eq!(backend.get_needed_title(), "What Does Ada Lovelace Need?");
    assert_eq!(
        backend.get_needed_target(),
        4,
        "the letter just above where she is now: a B"
    );
    assert_eq!(
        backend.get_needed_result(),
        "Ada Lovelace needs at least 91.8 on the next Test to reach a B in Math."
    );
    backend.set_needed_target(1);
    backend.invoke_needed_changed();
    assert_eq!(
        backend.get_needed_result(),
        "Even a 100 on the next Test only gets Ada Lovelace to 90.4 B in Math. \
         Try a lower letter, or a category with more weight."
    );
    backend.set_needed_target(4);
    backend.set_needed_kind(0);
    backend.invoke_needed_changed();
    assert_eq!(
        backend.get_needed_result(),
        "Ada Lovelace needs at least 96.5 on the next Homework to reach a B in Math."
    );
    click(app, "Close");
    assert_eq!(backend.get_dialog(), 0);

    // Editing a score: its value and its kind. A value that isn't a score can't be saved.
    click(app, "Edit grade 88 for Ada Lovelace");
    assert_eq!(backend.get_dialog(), 1);
    assert_eq!(backend.get_edit_title(), "Ada Lovelace \u{b7} Math \u{b7} Period 1");
    assert_eq!(value(app, "Score"), "88");
    element(app, "Score").set_accessible_value("");
    settle();
    assert!(!enabled(app, "Save"), "an empty score is not a score");
    element(app, "Score").set_accessible_value("85");
    settle();
    assert!(enabled(app, "Save"));
    let kinds = with_role(app, AccessibleRole::Tab, "Quiz");
    kinds
        .last()
        .expect("the kinds in the dialog")
        .mock_single_click(PointerEventButton::Left);
    settle();
    click(app, "Save");
    assert_eq!(backend.get_dialog(), 0);
    let edited = saved(dir)
        .grades
        .into_iter()
        .find(|g| g.value == "85")
        .expect("the edited score");
    assert_eq!(edited.grade_type, GradeType::Quiz);
    assert_eq!(backend.get_toast().message, "Score edited");
    assert_eq!(chips(app, "Ada Lovelace").last().map(String::as_str), Some("85"));

    // The dialog can also delete the score, and Escape leaves without changing anything.
    click(app, "Edit grade 85 for Ada Lovelace");
    press(app, Key::Escape);
    assert_eq!(backend.get_dialog(), 0);
    assert_eq!(chips(app, "Ada Lovelace").len(), 6);
    click(app, "Edit grade 85 for Ada Lovelace");
    click(app, "Delete Score");
    assert_eq!(backend.get_dialog(), 0);
    assert_eq!(chips(app, "Ada Lovelace"), ["100", "60", "EX", "+5", "18/20"]);
    assert_eq!(backend.get_toast().message, "Score deleted");

    // Back to the Test of 88, with one more step of undo.
    press_with(app, &[Key::Control], "z");
    click(app, "Edit grade 85 for Ada Lovelace");
    element(app, "Score").set_accessible_value("88");
    settle();
    let kinds = with_role(app, AccessibleRole::Tab, "Test");
    kinds
        .last()
        .expect("the kinds in the dialog")
        .mock_single_click(PointerEventButton::Left);
    settle();
    click(app, "Save");
    assert_eq!(row_of(app, "Ada Lovelace").average, format!("86.8 B{MINUS}"));
}

// ---- Insights -------------------------------------------------------------------------------------

fn insights(app: &App, dir: &Path) {
    let backend = app.window().global::<Backend>();
    app.set_tab(2);
    let insights = backend.get_insights();
    assert_eq!(
        insights.summary,
        "2 of 2 students in Grade 1 have an average in Math, Period 1."
    );
    assert_eq!(
        insights.class_average,
        backend.get_class_average(),
        "the same number as the Grades screen"
    );
    assert_eq!(insights.highest_name, "Ada Lovelace");
    assert_eq!(insights.lowest_name, "Grace Hopper");
    assert_eq!(insights.lowest, "70.0 D");
    let counts: Vec<i32> = insights.bars.iter().map(|b| b.count).collect();
    assert_eq!(counts, [0, 1, 0, 1, 0, 0], "A to F: one B, one D");
    assert_eq!(insights.trend.row_count(), 6);
    let first = insights.trend.row_data(0).unwrap();
    assert!(first.has_value && first.current && first.text == insights.class_average);
    assert!(!insights.trend.row_data(1).unwrap().has_value, "nothing in period 2");

    // Nobody is below 70 yet; the slider moves the line and the choice is remembered.
    assert_eq!(backend.get_attention_below(), 70.0);
    assert_eq!(insights.attention.row_count(), 0);
    let slider = element(app, "Needs attention below");
    slider.invoke_accessible_increment_action();
    settle();
    assert_eq!(backend.get_attention_below(), 71.0);
    let insights = backend.get_insights();
    assert_eq!(insights.attention.row_count(), 1);
    assert_eq!(insights.attention.row_data(0).unwrap().name, "Grace Hopper");
    assert_eq!(insights.attention.row_data(0).unwrap().average, "70.0 D");
    mock_elapsed_time(Duration::from_millis(600));
    assert_eq!(
        Store::in_dir(dir).load_preferences().attention_below,
        Some(71),
        "saved a moment after the slider stopped"
    );
    slider.invoke_accessible_decrement_action();
    mock_elapsed_time(Duration::from_millis(600));
    assert_eq!(
        Store::in_dir(dir).load_preferences().attention_below,
        None,
        "the usual 70 isn't written down"
    );

    // The screen follows the filters at the top.
    backend.set_period_index(1);
    backend.invoke_filters_changed();
    assert_eq!(
        backend.get_insights().summary,
        "None of the 2 students in Grade 1 has a score in Math, Period 2 yet."
    );
    assert_eq!(backend.get_insights().class_average, "");
    backend.set_period_index(0);
    backend.invoke_filters_changed();
}

// ---- Weights ----------------------------------------------------------------------------------

fn weights(app: &App, dir: &Path) {
    let backend = app.window().global::<Backend>();
    app.set_tab(3);
    assert_eq!(value(app, "Test weight"), "50");
    assert_eq!(backend.get_weights_total(), 100.0);
    element(app, "Test weight").invoke_accessible_increment_action();
    assert_eq!(value(app, "Test weight"), "51");
    assert_eq!(
        saved(dir).test_weight,
        50.0,
        "weights are saved after a short pause, not on every step"
    );
    mock_elapsed_time(Duration::from_millis(600));
    assert_eq!(saved(dir).test_weight, 51.0);
    element(app, "Quiz weight").invoke_accessible_decrement_action();
    assert_eq!(value(app, "Quiz weight"), "16");
    assert_eq!(backend.get_weights_total(), 100.0, "the total follows");

    // Dragging with the mouse works, and snaps to whole numbers.
    let slider = element(app, "Homework weight");
    let (pos, size) = (slider.absolute_position(), slider.size());
    slider.mock_drag(
        slint::LogicalPosition::new(pos.x + size.width * 0.75, pos.y + size.height / 2.0),
        PointerEventButton::Left,
    );
    let weight: f32 = value(app, "Homework weight").parse().unwrap();
    assert!((60.0..=80.0).contains(&weight), "dragged to {weight}");
    assert!(backend.get_weights_total() > 100.0);
    mock_elapsed_time(Duration::from_millis(600));
    assert_eq!(saved(dir).homework_weight, f64::from(weight));
    assert_eq!(backend.get_undo_text(), "Weights changed");

    // Everything done with the sliders since the first is one step of undo.
    press_with(app, &[Key::Control], "z");
    assert_eq!(
        (
            saved(dir).test_weight,
            saved(dir).quiz_weight,
            saved(dir).homework_weight
        ),
        (50.0, 17.0, 33.0)
    );
    assert_eq!(value(app, "Test weight"), "50", "the sliders show it");
    assert_eq!(backend.get_weights_total(), 100.0);

    // One subject can have weights of its own, and then the others are left alone.
    let math = texts(backend.get_weight_scope_options())
        .iter()
        .position(|option| option == "Math")
        .unwrap();
    backend.set_weight_scope_index(math as i32);
    backend.invoke_weight_scope_changed();
    settle();
    assert!(!backend.get_weight_scope_own());
    assert!(backend
        .get_weight_scope_note()
        .contains("uses the weights for all subjects"));
    assert_eq!(count(app, "Use the Default Weights"), 0);
    element(app, "Test weight").invoke_accessible_increment_action();
    assert!(backend.get_weight_scope_own());
    mock_elapsed_time(Duration::from_millis(600));
    let data = saved(dir);
    assert_eq!(data.test_weight, 50.0, "the weights for all subjects are untouched");
    assert_eq!(data.options.subject_weights["Math"]["Test"], 51.0);
    assert_eq!(data.options.subject_weights["Math"]["Quiz"], 17.0);
    click(app, "Use the Default Weights");
    assert!(!backend.get_weight_scope_own());
    assert!(saved(dir).options.subject_weights.is_empty());
    assert_eq!(value(app, "Test weight"), "50");
    backend.set_weight_scope_index(0);
    backend.invoke_weight_scope_changed();
    settle();
}

// ---- Dialogs and the keyboard --------------------------------------------------------------------

fn dialogs_and_keys(app: &App, dir: &Path) {
    let backend = app.window().global::<Backend>();
    app.set_tab(0);

    // A score to take back: the last step of the history, and what the checks below look at.
    click_tab(app, "Test");
    type_into(app, "Grade for Grace Hopper", "50");
    press(app, Key::Return);
    let before = saved(dir);
    assert_eq!(backend.get_undo_text(), "Score added");

    // While a dialog is open, nothing behind it can be taken back.
    click(app, "Edit grade 100 for Ada Lovelace");
    assert_eq!(backend.get_dialog(), 1);
    invoke(&first_button(app, "Undo"));
    press_with(app, &[Key::Control], "z");
    assert_eq!(saved(dir), before);

    // Tab stays on the dialog instead of going to the page behind it, and Escape closes it
    // without saving.
    for _ in 0..12 {
        press(app, Key::Tab);
    }
    press(app, "7");
    assert_eq!(
        value(app, "Grade for Ada Lovelace"),
        "",
        "the page behind didn't get the key"
    );
    press(app, Key::Escape);
    assert_eq!(backend.get_dialog(), 0);
    assert_eq!(saved(dir), before);

    // Once it is closed, the shortcuts work again.
    press_with(app, &[Key::Control], "z");
    assert_eq!(saved(dir).grades.len(), before.grades.len() - 1);
    press_with(app, &[Key::Control], "y");
    assert_eq!(saved(dir), before);

    // An emptied field in a dialog keeps Ctrl+Z for itself: it brings back the text that was
    // just deleted, and there is nothing behind the dialog to undo.
    click(app, "Edit grade 100 for Ada Lovelace");
    assert_eq!(value(app, "Score"), "100");
    click(app, "Score");
    press_with(app, &[Key::Control], "a");
    press(app, Key::Backspace);
    assert_eq!(value(app, "Score"), "");
    press_with(app, &[Key::Control], "z");
    assert_eq!(value(app, "Score"), "100");
    assert_eq!(saved(dir), before);
    press(app, Key::Escape);
    assert_eq!(backend.get_dialog(), 0);

    // An alert on top of a dialog gives the dialog the keyboard back when it closes.
    click(app, "Edit grade 100 for Ada Lovelace");
    backend.set_alert(gradebook::ui::AlertData {
        visible: true,
        title: "Alert".into(),
        message: "On top".into(),
        confirm_label: "".into(),
        destructive: false,
    });
    settle();
    press(app, Key::Escape);
    assert!(!backend.get_alert().visible);
    assert_eq!(backend.get_dialog(), 1, "the dialog is still there");
    press(app, Key::Escape);
    assert_eq!(backend.get_dialog(), 0);
}

// ---- Categories, subjects and grade levels ---------------------------------------------------------

fn lists(app: &App, dir: &Path) {
    let backend = app.window().global::<Backend>();
    // The whole settings screen on one page, so that everything can be clicked.
    resize(app, 1000.0, 2800.0);
    app.set_tab(3);

    // A category of your own.
    assert!(!enabled(app, "Add category"));
    type_into(app, "Category Name", "Project");
    assert!(enabled(app, "Add category"));
    press(app, Key::Return);
    assert_eq!(value(app, "Category Name"), "");
    assert_eq!(
        texts(backend.get_category_names()),
        ["Homework", "Test", "Quiz", "Project"]
    );
    let data = saved(dir);
    assert_eq!(
        (data.categories[0].name.as_str(), data.categories[0].weight),
        ("Project", 0.0)
    );
    assert_eq!(backend.get_weights_total(), 100.0, "it has no weight until it gets one");
    let slider = element(app, "Project weight");
    for _ in 0..10 {
        slider.invoke_accessible_increment_action();
    }
    mock_elapsed_time(Duration::from_millis(600));
    assert_eq!(saved(dir).categories[0].weight, 10.0);

    // A name that is taken, in any case, or one of the three, is explained and left in the field.
    for taken in ["project", "Test"] {
        element(app, "Category Name").set_accessible_value(taken);
        settle();
        click(app, "Add category");
        assert_eq!(backend.get_alert().title, "Couldn't Add the Category");
        assert!(backend.get_alert().message.contains("already in use"));
        press(app, Key::Escape);
        assert!(!backend.get_alert().visible);
        assert_eq!(value(app, "Category Name"), taken);
    }
    element(app, "Category Name").set_accessible_value("");
    settle();

    // It shows up as a choice beside every student, and a score can be put in it.
    app.set_tab(0);
    resize(app, 1000.0, 760.0);
    assert!(x_of(app, AccessibleRole::Tab, "Quiz") < x_of(app, AccessibleRole::Tab, "Project"));
    click_tab(app, "Project");
    assert_eq!(backend.get_grade_type(), 3);
    type_into(app, "Grade for Grace Hopper", "9/10");
    press(app, Key::Return);
    let grace_chips = row_of(app, "Grace Hopper").chips;
    let chip = grace_chips
        .row_data(grace_chips.row_count() - 1)
        .expect("the Project score");
    assert_eq!((chip.value.as_str(), chip.kind), ("9/10", 3));
    assert!(saved(dir)
        .grades
        .iter()
        .any(|g| g.grade_type == GradeType::Custom("Project".into())));
    assert_eq!(texts(backend.get_legend()), ["Homework", "Tests", "Quizzes", "Project"]);

    // On a narrow window the four don't fit side by side: the choice becomes a menu.
    assert_eq!(comboboxes(app).len(), 3);
    resize(app, 560.0, 760.0);
    assert!(with_role(app, AccessibleRole::Tab, "Project").is_empty(), "no tabs now");
    assert_eq!(comboboxes(app).len(), 3 + 2, "a menu beside each of the two students");
    resize(app, 1000.0, 760.0);
    assert_eq!(comboboxes(app).len(), 3);
    click_tab(app, "Test");

    // Renaming it renames its scores too; a name in use is refused.
    resize(app, 1000.0, 2800.0);
    app.set_tab(3);
    assert_eq!(
        backend.get_custom_categories().row_data(0).unwrap().detail,
        "1 score \u{b7} weight 10%"
    );
    backend.set_grade_type(3);
    click(app, "Rename Project");
    assert_eq!(backend.get_dialog(), 6);
    assert_eq!(value(app, "New name"), "Project");
    element(app, "New name").set_accessible_value("quiz");
    settle();
    click_button(app, "Rename");
    assert_eq!(backend.get_alert().title, "Couldn't Rename It");
    press(app, Key::Escape);
    assert_eq!(backend.get_dialog(), 6, "the dialog is still open to try again");
    element(app, "New name").set_accessible_value("Lab");
    settle();
    click_button(app, "Rename");
    assert_eq!(backend.get_dialog(), 0);
    assert_eq!(texts(backend.get_category_names()), ["Homework", "Test", "Quiz", "Lab"]);
    assert_eq!(
        backend.get_grade_type(),
        3,
        "the kind chosen for adding scores is still the same one under its new name"
    );
    backend.set_grade_type(1);
    let data = saved(dir);
    assert_eq!(data.categories[0].name, "Lab");
    assert!(data
        .grades
        .iter()
        .any(|g| g.grade_type == GradeType::Custom("Lab".into())));
    assert_eq!(count(app, "Lab weight"), 1);

    // Removing one that has scores asks first, and takes the scores with it. It can be undone.
    let scores_before = saved(dir).grades.len();
    click(app, "Remove Lab");
    assert_eq!(backend.get_alert().title, "Remove Category?");
    assert!(backend.get_alert().message.contains("1 score"));
    click_button(app, "Cancel");
    assert_eq!(saved(dir).categories.len(), 1);
    click(app, "Remove Lab");
    click_button(app, "Remove");
    let data = saved(dir);
    assert!(data.categories.is_empty());
    assert_eq!(data.grades.len(), scores_before - 1);
    assert_eq!(texts(backend.get_category_names()), ["Homework", "Test", "Quiz"]);
    assert_eq!(backend.get_toast().message, "Category removed");
    invoke(&button(app, "Undo"));
    assert_eq!(saved(dir).categories.len(), 1, "undone: the category is back");
    assert_eq!(saved(dir).grades.len(), scores_before, "and so is its score");
    assert_eq!(texts(backend.get_category_names()).len(), 4);
    click(app, "Remove Lab");
    click_button(app, "Remove");
    assert!(saved(dir).categories.is_empty());

    // Names are the same when only the case differs, in any alphabet.
    type_into(app, "Category Name", "\u{c9}valuation");
    press(app, Key::Return);
    element(app, "Category Name").set_accessible_value("\u{e9}valuation");
    settle();
    click(app, "Add category");
    assert_eq!(backend.get_alert().title, "Couldn't Add the Category");
    assert!(backend.get_alert().message.contains("already in use"));
    press(app, Key::Escape);
    element(app, "Category Name").set_accessible_value("");
    settle();

    // The kind chosen for adding scores stays with its category when another one is removed
    // from the list before it, and goes back to Test when its own category is removed.
    type_into(app, "Category Name", "Bonus");
    press(app, Key::Return);
    assert_eq!(
        texts(backend.get_category_names()),
        ["Homework", "Test", "Quiz", "\u{c9}valuation", "Bonus"]
    );
    backend.set_grade_type(4);
    click(app, "Remove \u{c9}valuation");
    assert_eq!(
        texts(backend.get_category_names()),
        ["Homework", "Test", "Quiz", "Bonus"]
    );
    assert_eq!(
        backend.get_grade_type(),
        3,
        "Bonus moved up, and is still the one chosen"
    );
    click(app, "Remove Bonus");
    assert_eq!(texts(backend.get_category_names()), ["Homework", "Test", "Quiz"]);
    assert_eq!(backend.get_grade_type(), 1, "its category is gone: back to Test");
    assert!(saved(dir).categories.is_empty());

    // A subject of your own.
    type_into(app, "Subject Name", "Geography");
    press(app, Key::Return);
    let subjects = texts(backend.get_subject_options());
    assert_eq!(subjects.len(), 21);
    assert!(subjects.contains(&"Geography".to_string()));
    assert_eq!(saved(dir).subjects, ["Geography"]);
    element(app, "Subject Name").set_accessible_value("math");
    settle();
    click(app, "Add subject");
    assert_eq!(backend.get_alert().title, "Couldn't Add the Subject");
    press(app, Key::Escape);
    element(app, "Subject Name").set_accessible_value("");
    settle();

    // Pick it on the Grades screen and put a score in it.
    app.set_tab(0);
    resize(app, 1000.0, 760.0);
    let position = subjects.iter().position(|s| s == "Geography").unwrap();
    backend.set_subject_index(position as i32);
    backend.invoke_filters_changed();
    assert_eq!(chips(app, "Ada Lovelace"), Vec::<String>::new());
    click_tab(app, "Homework");
    type_into(app, "Grade for Ada Lovelace", "77");
    press(app, Key::Return);
    assert!(saved(dir)
        .grades
        .iter()
        .any(|g| g.subject == "Geography" && g.value == "77"));
    click_tab(app, "Test");

    // Renaming the subject renames its scores, and removing it asks, because they go too.
    resize(app, 1000.0, 2800.0);
    app.set_tab(3);
    assert_eq!(backend.get_custom_subjects().row_data(0).unwrap().detail, "1 score");
    click(app, "Rename Geography");
    element(app, "New name").set_accessible_value("geography");
    settle();
    click_button(app, "Rename");
    assert_eq!(
        backend.get_dialog(),
        0,
        "a name can be changed to the same one in another case"
    );
    assert_eq!(saved(dir).subjects, ["geography"]);
    assert!(saved(dir)
        .grades
        .iter()
        .any(|g| g.subject == "geography" && g.value == "77"));
    assert_eq!(
        texts(backend.get_subject_options())[backend.get_subject_index() as usize],
        "geography"
    );
    click(app, "Rename geography");
    element(app, "New name").set_accessible_value("Earth Science");
    settle();
    click_button(app, "Rename");
    assert!(saved(dir)
        .grades
        .iter()
        .any(|g| g.subject == "Earth Science" && g.value == "77"));
    assert_eq!(saved(dir).subjects, ["Earth Science"]);
    assert_eq!(
        texts(backend.get_subject_options())[backend.get_subject_index() as usize],
        "Earth Science"
    );
    click(app, "Remove Earth Science");
    assert_eq!(backend.get_alert().title, "Remove Subject?");
    click_button(app, "Remove");
    assert!(saved(dir).subjects.is_empty());
    assert!(saved(dir).grades.iter().all(|g| g.subject == "Math"));
    assert_eq!(
        texts(backend.get_subject_options())[backend.get_subject_index() as usize],
        "Math",
        "the screens go back to Math when their subject is gone"
    );

    // Grade levels: 1 to 10 are always there, Kindergarten and 11 to 20 can be added.
    assert_eq!(texts(backend.get_addable_level_options())[0], "Kindergarten");
    click(app, "Add grade level");
    let levels = texts(backend.get_grade_level_options());
    assert_eq!(levels.len(), 11);
    assert_eq!(levels[0], "Kindergarten");
    assert_eq!(backend.get_grade_level_index(), 1, "the screen stays on Grade 1");
    assert_eq!(saved(dir).grade_levels, [0]);
    assert_eq!(texts(backend.get_addable_level_options())[0], "Grade 11");
    click(app, "Add grade level");
    assert_eq!(
        texts(backend.get_grade_level_options()).last().map(String::as_str),
        Some("Grade 11")
    );
    assert_eq!(saved(dir).grade_levels, [0, 11]);

    // A student can be in Kindergarten, and the level can't be removed while they are.
    app.set_tab(1);
    backend.set_new_student_level(0);
    type_into(app, "Student Name", "Kid");
    press(app, Key::Return);
    assert_eq!(saved(dir).students.last().map(|s| s.grade_level), Some(0));
    assert_eq!(backend.get_students().row_data(2).unwrap().grade, "Kindergarten");
    app.set_tab(3);
    assert_eq!(backend.get_extra_levels().row_data(0).unwrap().detail, "1 student");
    click(app, "Remove Kindergarten");
    assert_eq!(backend.get_alert().title, "Couldn't Remove the Grade Level");
    assert!(backend.get_alert().message.contains("A student is still in that grade"));
    press(app, Key::Escape);
    app.set_tab(1);
    click(app, "Delete Kid");
    invoke(&element(app, "Confirm delete Kid"));
    app.set_tab(3);
    click(app, "Remove Kindergarten");
    click(app, "Remove Grade 11");
    assert!(saved(dir).grade_levels.is_empty());
    assert_eq!(texts(backend.get_grade_level_options()).len(), 10);
    assert_eq!(backend.get_grade_level_index(), 0, "still Grade 1");

    resize(app, 1000.0, 760.0);
}

// ---- The keyboard after a control is gone -------------------------------------------------------

/// Undo and redo replace controls of the screen. One that had the keyboard is gone and takes it
/// with it, and then no shortcut reaches the app (Ctrl+Y would go nowhere) unless the app gives
/// the keyboard back.
fn keyboard_after_undo(app: &App, dir: &Path) {
    // On the Grades screen: the student whose field has the keyboard is taken back, row and all.
    app.set_tab(1);
    type_into(app, "Student Name", "Zed");
    press(app, Key::Return);
    assert_eq!(saved(dir).students.len(), 3);
    app.set_tab(0);
    click(app, "Grade for Zed");
    press_with(app, &[Key::Control], "z");
    assert_eq!(saved(dir).students.len(), 2);
    assert_eq!(count(app, "Grade for Zed"), 0, "the field is gone");
    press_with(app, &[Key::Control], "y");
    assert_eq!(saved(dir).students.len(), 3, "the shortcuts still work");
    press_with(app, &[Key::Control], "z");
    assert_eq!(saved(dir).students.len(), 2);

    // On the Settings screen: a weight set from the keyboard is taken back, and every slider
    // is replaced, the one that has the keyboard too. Shift+Tab leads there from the field
    // below the weights.
    resize(app, 1000.0, 2800.0);
    app.set_tab(3);
    let before = value(app, "Quiz weight");
    click(app, "Category Name");
    let back = SharedString::from(Key::Tab);
    let mut at_the_slider = false;
    for _ in 0..4 {
        press_with(app, &[Key::Shift], &back);
        press(app, Key::RightArrow);
        if value(app, "Quiz weight") != before {
            at_the_slider = true;
            break;
        }
    }
    assert!(at_the_slider, "Shift+Tab goes back to the Quiz slider");
    let after = value(app, "Quiz weight");
    assert_eq!(app.window().global::<Backend>().get_undo_text(), "Weights changed");
    press_with(app, &[Key::Control], "z");
    assert_eq!(value(app, "Quiz weight"), before);
    press_with(app, &[Key::Control], "y");
    assert_eq!(value(app, "Quiz weight"), after, "the shortcuts still work");
    press_with(app, &[Key::Control], "z");
    assert_eq!(value(app, "Quiz weight"), before);
    resize(app, 1000.0, 760.0);
    app.set_tab(0);
}

// ---- A new school year ------------------------------------------------------------------------

fn new_school_year(app: &App, dir: &Path) {
    let backend = app.window().global::<Backend>();
    resize(app, 1000.0, 2800.0);
    app.set_tab(3);
    let before = saved(dir);
    assert!(!before.grades.is_empty());

    // It asks first, and says what it will do.
    click(app, "Start a New School Year");
    let alert = backend.get_alert();
    assert_eq!(alert.title, "Start a New School Year?");
    assert!(
        alert.message.contains("2 students move up a grade"),
        "{}",
        alert.message
    );
    assert!(
        alert
            .message
            .contains(&format!("{} scores are cleared", before.grades.len())),
        "{}",
        alert.message
    );
    click_button(app, "Cancel");
    assert_eq!(saved(dir), before);

    // A copy of the year is kept first, then the scores go and the students move up.
    click(app, "Start a New School Year");
    click(app, "Start New Year");
    let data = saved(dir);
    assert!(data.grades.is_empty());
    assert!(data.students.iter().all(|s| s.grade_level == 2));
    assert_eq!(data.options, before.options, "the weights and options stay");
    let archive = dir.join("backups").join("school-year-2031-06-01.json");
    assert_eq!(store::read_backup_file(&archive).unwrap(), before);
    assert_eq!(backend.get_toast().message, "New school year started");

    // It can be taken back; starting it again the same day doesn't overwrite the copy.
    invoke(&button(app, "Undo"));
    assert_eq!(saved(dir), before);
    click(app, "Start a New School Year");
    click(app, "Start New Year");
    let second = dir.join("backups").join("school-year-2031-06-01-2.json");
    assert_eq!(store::read_backup_file(&second).unwrap(), before);
    assert_eq!(store::read_backup_file(&archive).unwrap(), before);
    press_with(app, &[Key::Control], "z");
    assert_eq!(saved(dir), before);
    assert_eq!(backend.get_grade_level_index(), 0, "Grade 1 again");

    // A copy that can't be made stops it, and nothing changes. A file where the backups folder
    // should be makes every copy fail, on any system.
    let backups = dir.join("backups");
    let away = dir.join("backups-away");
    std::fs::rename(&backups, &away).unwrap();
    std::fs::write(&backups, "not a folder").unwrap();
    click(app, "Start a New School Year");
    click(app, "Start New Year");
    let alert = backend.get_alert();
    assert_eq!(alert.title, "Couldn't Start a New School Year");
    assert!(alert.message.contains("nothing was changed"), "{}", alert.message);
    assert_eq!(saved(dir), before, "the year goes on as it was");
    assert_eq!(backend.get_grade_level_index(), 0);
    press(app, Key::Escape);
    assert!(!backend.get_alert().visible);
    std::fs::remove_file(&backups).unwrap();
    std::fs::rename(&away, &backups).unwrap();
    resize(app, 1000.0, 760.0);
}

// ---- Files, reports and the folder ------------------------------------------------------------------------

fn files_and_reports(app: &App, dir: &Path, opened: &Rc<RefCell<Vec<PathBuf>>>) {
    let backend = app.window().global::<Backend>();

    // The data folder.
    resize(app, 1000.0, 2800.0);
    app.set_tab(3);
    assert!(backend.get_data_folder().contains(&dir.display().to_string()));
    click(app, "Open Data Folder");
    assert_eq!(opened.borrow().last().map(PathBuf::as_path), Some(dir));
    resize(app, 1000.0, 760.0);

    // All scores as a spreadsheet.
    let csv = dir.join("scores.csv");
    app.export_scores_to(&csv);
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.starts_with('\u{feff}'), "a spreadsheet reads it as UTF-8");
    assert!(text.contains("Grade level,Student,Subject,Period,Category,Score,Percent,Letter,Counts"));
    assert!(text.contains("Ada Lovelace"));
    assert!(text.contains("Grace Hopper"));
    assert!(text.contains("\"18/20\"") || text.contains("18/20"));

    // A report card: a line for each subject, with the six periods.
    app.set_tab(0);
    click(app, "Report card for Ada Lovelace");
    assert_eq!(backend.get_dialog(), 5);
    assert_eq!(backend.get_report_title(), "Report Card: Ada Lovelace");
    assert!(backend.get_report_subtitle().starts_with("Grade 1 \u{b7} overall 86.8"));
    assert_eq!(backend.get_report_rows().row_count(), 1);
    let math = backend.get_report_rows().row_data(0).unwrap();
    assert_eq!(math.subject, "Math");
    assert_eq!(math.cells.row_data(0).unwrap(), format!("86.8 B{MINUS}"));
    assert_eq!(math.cells.row_data(1).unwrap(), "\u{2014}");
    assert_eq!(backend.get_report_total().subject, "Overall");

    let report = dir.join("report.csv");
    app.export_report_to(&report);
    let text = std::fs::read_to_string(&report).unwrap();
    assert!(text.contains("Student,Grade level,Subject,Period 1"));
    assert!(text.contains("Ada Lovelace"));
    assert!(text.contains("Math"));
    assert!(!text.contains("Grace Hopper"));

    // The printable report is a page in the temporary folder, opened in the browser.
    let before = opened.borrow().len();
    click(app, "Printable Report");
    assert_eq!(opened.borrow().len(), before + 1);
    let page = opened.borrow().last().unwrap().clone();
    assert!(page.starts_with(std::env::temp_dir()));
    assert!(page.extension().is_some_and(|e| e == "html"));
    let html = std::fs::read_to_string(&page).unwrap();
    assert!(html.contains("Ada Lovelace") && html.contains("Math"));
    std::fs::remove_file(&page).unwrap();
    click(app, "Close");
    assert_eq!(backend.get_dialog(), 0);

    // A student with nothing in the period shown still has a report: it covers all periods.
    click(app, "Report card for Grace Hopper");
    assert_eq!(backend.get_report_rows().row_count(), 1);
    click(app, "Close");
}

// ---- Looking for a new version ---------------------------------------------------------------------------

/// Lets the event loop run what other threads queued for it, until `done`. (The testing backend
/// only runs the queue inside `run_event_loop`, which returns once it meets the request to quit
/// that is queued behind.)
fn wait_until(what: &str, done: impl Fn() -> bool) {
    for _ in 0..500 {
        slint::invoke_from_event_loop(|| {
            let _ = slint::quit_event_loop();
        })
        .unwrap();
        slint::run_event_loop().unwrap();
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{what} never came back");
}

/// Waits for the answer of a look that was asked for with a button: the screen says that it is
/// looking until the answer is there.
fn wait_for_answer(app: &App) {
    let backend = app.window().global::<Backend>();
    wait_until("the answer", || {
        backend.get_update_status() != "Looking for a new version\u{2026}"
    });
}

fn updates(app: &App, dir: &Path, opened: &Rc<RefCell<Vec<PathBuf>>>) {
    let backend = app.window().global::<Backend>();
    app.set_tab(3);
    resize(app, 1000.0, 2800.0);

    // A release is built and tested as its own version, so what is newer than this build, and
    // what is the same, comes from the build and not from a number written here.
    let current = gradebook::update::current();
    let newer = format!("{}.0.0", current.major + 1);
    let newer_still = format!("{}.0.1", current.major + 1);

    // The bar of an earlier change goes away by itself after a few seconds.
    mock_elapsed_time(Duration::from_secs(10));
    assert!(!backend.get_toast().visible);

    // On by default, so a settings file that never said anything about it looks too. What the
    // look finds is whatever the test says it is.
    assert!(backend.get_check_updates());
    assert_eq!(Store::in_dir(dir).load_preferences().check_for_updates, None);
    assert!(!backend.get_update_available());
    assert_eq!(count(app, "Open the Download Page"), 0);
    assert!(
        with_role(app, AccessibleRole::Button, "Download").is_empty(),
        "no bar yet"
    );
    let looked = Arc::new(AtomicUsize::new(0));
    let answer: Arc<Mutex<Result<String, String>>> = Arc::new(Mutex::new(Ok(release(&format!("v{newer}")))));
    {
        let (looked, answer) = (looked.clone(), answer.clone());
        app.set_update_fetcher(Arc::new(move || {
            looked.fetch_add(1, Ordering::SeqCst);
            answer.lock().unwrap().clone()
        }));
    }
    assert_eq!(looked.load(Ordering::SeqCst), 0, "a window doesn't go online by itself");

    // When GradeBook starts it looks, on a thread of its own. The answer comes back to the
    // window through its event loop.
    app.look_for_update_at_start();
    wait_until("the answer", || backend.get_update_available());
    assert_eq!(looked.load(Ordering::SeqCst), 1);

    // What it found is told on a bar above the tabs, which stays until it is sent away, and on
    // the Settings screen. Both have a button to the page of the release.
    let release_page = |version: &str| format!("https://github.com/JMTDI/GradeBook/releases/tag/v{version}");
    let headline = format!("GradeBook {newer} is available.");
    assert_eq!(backend.get_update_headline(), headline);
    assert_eq!(backend.get_update_status(), format!("{headline} You have {current}."));
    assert!(!backend.get_update_dismissed());
    assert!(count(app, &headline) > 0, "the bar says it");
    assert!(!backend.get_toast().visible, "and it isn't the bar of a change");
    click_button(app, "Download");
    assert_eq!(
        opened.borrow().last().unwrap().to_str(),
        Some(release_page(&newer).as_str())
    );
    assert!(count(app, "Open the Download Page") > 0);
    click(app, "Open the Download Page");
    assert_eq!(
        opened.borrow().last().unwrap().to_str(),
        Some(release_page(&newer).as_str())
    );

    // The bar can be sent away. Settings goes on saying it, and having the button.
    click_button(app, "Dismiss");
    assert!(backend.get_update_dismissed());
    assert!(with_role(app, AccessibleRole::Button, "Download").is_empty());
    assert!(count(app, "Open the Download Page") > 0);

    // The same version found again, without anyone asking, leaves it away; a newer one brings
    // it back.
    app.receive_update_answer(false, Ok(release(&format!("v{newer}"))));
    settle();
    assert!(backend.get_update_dismissed());
    app.receive_update_answer(false, Ok(release(&format!("v{newer_still}"))));
    settle();
    assert!(!backend.get_update_dismissed());
    assert_eq!(
        backend.get_update_headline(),
        format!("GradeBook {newer_still} is available.")
    );
    click_button(app, "Download");
    assert_eq!(
        opened.borrow().last().unwrap().to_str(),
        Some(release_page(&newer_still).as_str())
    );

    // A look that was asked for with the button also brings it back, even for a version that
    // was sent away.
    click_button(app, "Dismiss");
    assert!(with_role(app, AccessibleRole::Button, "Download").is_empty());
    *answer.lock().unwrap() = Ok(release(&format!("v{newer_still}")));
    click(app, "Check for Updates");
    assert_eq!(backend.get_update_status(), "Looking for a new version\u{2026}");
    wait_for_answer(app);
    assert_eq!(looked.load(Ordering::SeqCst), 2);
    assert!(!backend.get_update_dismissed());
    assert_eq!(with_role(app, AccessibleRole::Button, "Download").len(), 1);

    // The newest version, and answers that aren't a release: also through the thread. With
    // nothing to say, the bar goes, sent away or not.
    *answer.lock().unwrap() = Ok(release(&format!("v{current}")));
    click(app, "Check for Updates");
    wait_for_answer(app);
    assert_eq!(
        backend.get_update_status(),
        format!("GradeBook {current} is the newest version.")
    );
    assert!(!backend.get_update_available());
    assert_eq!(count(app, "Open the Download Page"), 0);
    assert!(with_role(app, AccessibleRole::Button, "Download").is_empty());
    *answer.lock().unwrap() = Err("offline".to_string());
    click(app, "Check for Updates");
    wait_for_answer(app);
    assert_eq!(backend.get_update_status(), "Couldn't look for a new version. offline");
    assert_eq!(looked.load(Ordering::SeqCst), 4);

    // A look that fails by itself is not worth saying anything about: it leaves the screen as it
    // is, even while a look that was asked for is under way (the fetcher is held up here).
    app.receive_update_answer(false, Err("offline".to_string()));
    settle();
    assert_eq!(backend.get_update_status(), "Couldn't look for a new version. offline");
    assert!(!backend.get_update_available());
    let held = answer.lock().unwrap();
    click(app, "Check for Updates");
    assert_eq!(backend.get_update_status(), "Looking for a new version\u{2026}");
    app.receive_update_answer(false, Err("offline".to_string()));
    settle();
    assert_eq!(
        backend.get_update_status(),
        "Looking for a new version\u{2026}",
        "the one that was asked for is still being waited for"
    );
    drop(held);
    wait_for_answer(app);
    assert_eq!(backend.get_update_status(), "Couldn't look for a new version. offline");
    assert_eq!(looked.load(Ordering::SeqCst), 5);

    // Turned off, the choice is remembered and GradeBook doesn't look when it starts.
    click(app, "Look for updates at start");
    assert!(!backend.get_check_updates());
    assert_eq!(Store::in_dir(dir).load_preferences().check_for_updates, Some(false));
    app.look_for_update_at_start();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(looked.load(Ordering::SeqCst), 5, "nothing is looked up while it is off");

    // Turned on again, it looks right away, because the switch is a request too.
    *answer.lock().unwrap() = Ok(release(&format!("v{current}")));
    click(app, "Look for updates at start");
    assert!(backend.get_check_updates());
    assert_eq!(Store::in_dir(dir).load_preferences().check_for_updates, Some(true));
    assert_eq!(backend.get_update_status(), "Looking for a new version\u{2026}");
    wait_for_answer(app);
    assert_eq!(looked.load(Ordering::SeqCst), 6);
    assert_eq!(
        backend.get_update_status(),
        format!("GradeBook {current} is the newest version.")
    );
    resize(app, 1000.0, 760.0);
}

/// A gradebook that was set up before, or by someone who turned the look off, starts as it was
/// left, and one that has no settings yet looks.
fn update_choice_at_start() {
    let counting = |looked: &Arc<AtomicUsize>| -> gradebook::update::Fetcher {
        let looked = looked.clone();
        Arc::new(move || {
            looked.fetch_add(1, Ordering::SeqCst);
            Err("offline".to_string())
        })
    };

    let new_dir = tempfile::tempdir().unwrap();
    let looks = Arc::new(AtomicUsize::new(0));
    let new = App::new(Some(Store::in_dir(new_dir.path()))).unwrap();
    new.set_update_fetcher(counting(&looks));
    assert!(new.window().global::<Backend>().get_check_updates());
    new.look_for_update_at_start();
    wait_until("the look", || looks.load(Ordering::SeqCst) == 1);

    let quiet_dir = tempfile::tempdir().unwrap();
    Store::in_dir(quiet_dir.path())
        .save_preferences(&Preferences {
            check_for_updates: Some(false),
            ..Preferences::default()
        })
        .unwrap();
    let quiet = App::new(Some(Store::in_dir(quiet_dir.path()))).unwrap();
    let quiet_looks = Arc::new(AtomicUsize::new(0));
    quiet.set_update_fetcher(counting(&quiet_looks));
    assert!(!quiet.window().global::<Backend>().get_check_updates());
    quiet.look_for_update_at_start();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(quiet_looks.load(Ordering::SeqCst), 0);
}

/// The bar about a new version can be sent away from the keyboard, and the shortcuts go on
/// working: the button that had the keyboard goes with the bar, and the window takes it back.
fn update_bar_from_the_keyboard() {
    // Newer than this build, whatever version it is built as.
    let newer = format!("v{}.0.0", gradebook::update::current().major + 1);
    let mut reached = false;
    for steps in 1..=14 {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(Some(Store::in_dir(dir.path()))).unwrap();
        app.window().show().unwrap();
        resize(&app, 1000.0, 760.0);
        let backend = app.window().global::<Backend>();

        // Something to take back.
        app.set_tab(1);
        type_into(&app, "Student Name", "Zed");
        press(&app, Key::Return);
        assert_eq!(backend.get_students().row_count(), 1);
        app.set_tab(0);
        settle();
        app.receive_update_answer(false, Ok(release(&newer)));
        settle();
        assert!(!backend.get_update_dismissed());

        // Shift+Tab goes back through the controls, past the tabs to the buttons of the bar,
        // and Space presses the one it has come to. Whichever step reaches Dismiss is the one
        // that counts; the others press something else, which is harmless on a fresh window.
        let back = SharedString::from(Key::Tab);
        for _ in 0..steps {
            press_with(&app, &[Key::Shift], &back);
        }
        press(&app, " ");
        if !backend.get_update_dismissed() {
            continue;
        }
        reached = true;
        assert!(with_role(&app, AccessibleRole::Button, "Download").is_empty());
        press_with(&app, &[Key::Control], "z");
        assert_eq!(backend.get_students().row_count(), 0, "Ctrl+Z still works");
        press_with(&app, &[Key::Control], "y");
        assert_eq!(backend.get_students().row_count(), 1, "and so does Ctrl+Y");
        break;
    }
    assert!(reached, "the bar can be sent away from the keyboard");
}

// ---- Backup and restore ---------------------------------------------------------------------------

fn backup_and_restore(app: &App, dir: &Path) {
    let backend = app.window().global::<Backend>();
    app.set_tab(1);

    // Deleting a student takes their scores along.
    let grace = saved(dir)
        .students
        .iter()
        .find(|s| s.name == "Grace Hopper")
        .unwrap()
        .id;
    click(app, "Delete Grace Hopper");
    invoke(&element(app, "Confirm delete Grace Hopper"));
    let data = saved(dir);
    assert_eq!(data.students.len(), 1);
    assert!(
        data.grades.iter().all(|g| g.student_id != grace),
        "the student's scores go with them"
    );
    assert!(!data.grades.is_empty(), "the other student's stay");

    // Backup and restore.
    let backup = dir.join("GradeBookBackup.json");
    app.backup_to(&backup);
    let backed_up = store::read_backup_file(&backup).unwrap();
    assert_eq!(backed_up, saved(dir));

    type_into(app, "Student Name", "Temporary");
    click(app, "Add");
    assert_eq!(saved(dir).students.len(), 2);

    app.restore_from(&backup);
    assert!(backend.get_alert().visible);
    assert_eq!(backend.get_alert().title, "Restore Backup?");
    // Cancelling changes nothing.
    invoke(&button(app, "Cancel"));
    assert!(!backend.get_alert().visible);
    assert_eq!(saved(dir).students.len(), 2);

    app.restore_from(&backup);
    invoke(&button(app, "Restore"));
    assert!(!backend.get_alert().visible);
    assert_eq!(saved(dir), backed_up, "restoring brings back the backed-up data");
    assert_eq!(backend.get_students().row_count(), 1);
    assert_eq!(
        backend.get_toast().message,
        "Backup restored",
        "and it can be taken back"
    );
    let copies = restore_copies(dir);
    assert_eq!(
        copies.len(),
        1,
        "the replaced data is kept in a copy next to the data file"
    );
    assert_eq!(copies[0].students.len(), 2);

    // Restoring again must not overwrite that copy: it may hold the only copy of the
    // data from before the first restore.
    type_into(app, "Student Name", "Another");
    click(app, "Add");
    assert_eq!(saved(dir).students.len(), 2);
    app.restore_from(&backup);
    invoke(&button(app, "Restore"));
    assert_eq!(saved(dir), backed_up);
    let copies = restore_copies(dir);
    assert_eq!(copies.len(), 2, "every restore keeps its own copy");
    assert!(copies.iter().all(|copy| copy.students.len() == 2));
    assert!(
        copies
            .iter()
            .any(|copy| copy.students.iter().any(|s| s.name == "Temporary")),
        "the first copy, with the student that was added before the first restore, is still there"
    );

    // A file that isn't a backup is reported. Tab and typing stay on the alert
    // instead of reaching the page behind it, and Escape closes it.
    let junk = dir.join("junk.json");
    std::fs::write(&junk, "{\"hello\": 1}").unwrap();
    app.restore_from(&junk);
    assert_eq!(backend.get_alert().title, "Couldn't Restore");
    assert!(backend.get_alert().message.contains("isn't a GradeBook backup"));
    press(app, Key::Tab);
    press(app, "z");
    assert_eq!(value(app, "Student Name"), "");
    press(app, Key::Escape);
    assert!(!backend.get_alert().visible);
    assert_eq!(saved(dir), backed_up);

    // A file that can't be read at all says so, instead of calling it a bad backup.
    app.restore_from(&dir.join("does-not-exist.json"));
    assert_eq!(backend.get_alert().title, "Couldn't Restore");
    assert!(backend.get_alert().message.contains("couldn't be read"));
    press(app, Key::Escape);

    // A restore that can't keep a copy of the current data changes nothing.
    let blocked_dir = tempfile::tempdir().unwrap();
    let blocked = App::new(Some(Store::in_dir(blocked_dir.path()))).unwrap();
    blocked.window().show().unwrap();
    resize(&blocked, 1000.0, 760.0);
    let blocked_backend = blocked.window().global::<Backend>();
    blocked.set_tab(1);
    type_into(&blocked, "Student Name", "Keep me");
    click(&blocked, "Add");
    assert_eq!(blocked_backend.get_students().row_count(), 1);
    // A folder where the data file should be: it can't be copied, on any platform.
    let data_file = blocked_dir.path().join("gradebook.json");
    std::fs::remove_file(&data_file).unwrap();
    std::fs::create_dir(&data_file).unwrap();
    blocked.restore_from(&backup);
    invoke(&button(&blocked, "Restore"));
    let alert = blocked_backend.get_alert();
    assert!(alert.visible && alert.title == "Couldn't Restore", "{}", alert.title);
    assert!(alert.message.contains("nothing was changed"), "{}", alert.message);
    assert_eq!(
        blocked_backend.get_students().row_count(),
        1,
        "the current data is untouched"
    );
    assert_eq!(blocked_backend.get_students().row_data(0).unwrap().name, "Keep me");
}

fn damaged_and_unsaved() {
    // Starting with a damaged data file.
    let damaged_dir = tempfile::tempdir().unwrap();
    std::fs::write(damaged_dir.path().join("gradebook.json"), "not json at all").unwrap();
    let second = App::new(Some(Store::in_dir(damaged_dir.path()))).unwrap();
    let alert = second.window().global::<Backend>().get_alert();
    assert!(alert.visible);
    assert_eq!(alert.title, "Couldn't Open Saved Data");
    assert!(
        alert.message.contains("unreadable"),
        "tells the user where the old file went: {}",
        alert.message
    );
    assert_eq!(second.window().global::<Backend>().get_students().row_count(), 0);
    assert!(
        !damaged_dir.path().join("backups").exists(),
        "a damaged file isn't copied as a daily backup"
    );

    // Without anywhere to save, the user is told, and can dismiss it with OK.
    let third = App::new(None).unwrap();
    third.notify("Nowhere to Save", "Changes will be lost.");
    let alert = third.window().global::<Backend>().get_alert();
    assert!(alert.visible && alert.confirm_label.is_empty());
    assert_eq!(alert.title, "Nowhere to Save");
}

fn data_folder_before_the_first_save() {
    // Nothing is saved until something changes, so a first start has no folder yet. The button
    // makes it, so that there is a folder to open.
    let parent = tempfile::tempdir().unwrap();
    let folder = parent.path().join("GradeBook");
    let fresh = App::new(Some(Store::in_dir(&folder))).unwrap();
    fresh.window().show().unwrap();
    resize(&fresh, 1000.0, 2800.0);
    let opened: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let recorder = opened.clone();
    fresh.set_opener(move |target| {
        recorder.borrow_mut().push(PathBuf::from(target));
        Ok(())
    });
    assert!(!folder.exists());

    fresh.set_tab(3);
    click(&fresh, "Open Data Folder");
    assert!(folder.is_dir());
    assert_eq!(opened.borrow().as_slice(), [folder]);
}

// ---- Automatic backups --------------------------------------------------------------------------

fn daily_backup(dir: &Path) {
    // The first start of a day keeps a copy of the data as it was.
    let data = saved(dir);
    let again = App::new(Some(Store::in_dir(dir))).unwrap();
    let copies: Vec<PathBuf> = std::fs::read_dir(dir.join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("gradebook-") && name.ends_with(".json"))
        })
        .collect();
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(store::read_backup_file(&copies[0]).unwrap(), data);
    let summary = again.window().global::<Backend>().get_backups_summary();
    assert!(summary.contains("1 copy is kept"), "{summary}");
    // The school years that were kept stay where they are.
    assert!(dir.join("backups").join("school-year-2031-06-01.json").exists());
}

// ---- Light and dark ------------------------------------------------------------------------------

fn light_and_dark() {
    // The button in the corner switches the appearance on any screen, and the choice
    // is remembered for the next start.
    let theme_dir = tempfile::tempdir().unwrap();
    let themed = App::new(Some(Store::in_dir(theme_dir.path()))).unwrap();
    themed.window().show().unwrap();
    resize(&themed, 1000.0, 760.0);
    themed.set_appearance(Appearance::Light);
    let theme = themed.window().global::<Theme>();
    assert!(!theme.get_dark());

    click(&themed, "Switch to dark mode");
    assert!(theme.get_dark());
    assert_eq!(
        count(&themed, "Switch to dark mode"),
        0,
        "the button now offers the other mode"
    );
    assert_eq!(
        Store::in_dir(theme_dir.path()).load_preferences().appearance,
        Some(Appearance::Dark)
    );

    themed.set_tab(3);
    click(&themed, "Switch to light mode");
    assert!(!theme.get_dark(), "the button is on the Settings screen too");
    click(&themed, "Switch to dark mode");
    assert!(theme.get_dark());

    let restarted = App::new(Some(Store::in_dir(theme_dir.path()))).unwrap();
    assert!(
        restarted.window().global::<Theme>().get_dark(),
        "a new start uses the saved choice"
    );
}
