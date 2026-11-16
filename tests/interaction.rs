//! Drives the real UI headlessly: locates controls by their accessibility labels,
//! clicks them through the normal pointer pipeline, types into text fields, and
//! checks both what the screen shows and what ends up on disk.
//!
//! Slint's testing backend can only be initialised once per process, so this is a
//! single test that walks through the whole app in order.

use std::path::Path;
use std::time::Duration;

use i_slint_backend_testing::{
    init_integration_test_with_mock_time, mock_elapsed_time, AccessibleRole, ElementHandle, ElementQuery,
};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, Model};

use gradebook::app::App;
use gradebook::model::{BackupData, GradeType};
use gradebook::prefs::Appearance;
use gradebook::store::{self, Store};
use gradebook::ui::{Backend, Theme};

fn element(app: &App, label: &str) -> ElementHandle {
    ElementHandle::find_by_accessible_label(app.window(), label)
        .next()
        .unwrap_or_else(|| panic!("no control labelled {label:?}"))
}

/// The topmost button with this label (text items inside a button carry the same
/// label, and the alert's buttons sit above the page's).
fn button(app: &App, label: &str) -> ElementHandle {
    let wanted = label.to_string();
    ElementQuery::from_root(app.window())
        .match_descendants()
        .match_accessible_role(AccessibleRole::Button)
        .match_predicate(move |e| e.accessible_label().is_some_and(|l| l == wanted))
        .find_all()
        .pop()
        .unwrap_or_else(|| panic!("no button labelled {label:?}"))
}

fn count(app: &App, label: &str) -> usize {
    ElementHandle::find_by_accessible_label(app.window(), label).count()
}

/// The horizontal position of the first control with this role and label (for the type
/// selector and the legend, that is the one belonging to the first row).
fn x_of(app: &App, role: AccessibleRole, label: &str) -> f32 {
    let wanted = label.to_string();
    ElementQuery::from_root(app.window())
        .match_descendants()
        .match_accessible_role(role)
        .match_predicate(move |e| e.accessible_label().is_some_and(|l| l == wanted))
        .find_all()
        .first()
        .unwrap_or_else(|| panic!("no {role:?} labelled {label:?}"))
        .absolute_position()
        .x
}

fn click(app: &App, label: &str) {
    element(app, label).mock_single_click(PointerEventButton::Left);
}

/// Focuses a text field with a real click and types into it key by key.
fn type_into(app: &App, label: &str, text: &str) {
    click(app, label);
    for ch in text.chars() {
        press(app, ch.to_string());
    }
}

fn press(app: &App, text: impl Into<slint::SharedString>) {
    let text = text.into();
    let window = app.window().window();
    window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
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

fn row_names(app: &App) -> Vec<String> {
    app.window()
        .global::<Backend>()
        .get_rows()
        .iter()
        .map(|r| r.name.to_string())
        .collect()
}

#[test]
fn the_app_works_end_to_end() {
    init_integration_test_with_mock_time();

    let dir = tempfile::tempdir().unwrap();
    let app = App::new(Some(Store::in_dir(dir.path()))).unwrap();
    app.window().show().unwrap();
    app.window().window().set_size(slint::LogicalSize::new(1000.0, 760.0));
    let backend = app.window().global::<Backend>();

    // ---- Settings: adding students -------------------------------------------------
    app.set_tab(1);
    assert!(!enabled(&app, "Add"), "Add must be disabled while the name is empty");
    type_into(&app, "Student Name", "   ");
    assert!(!enabled(&app, "Add"), "whitespace is not a name");

    // Replace the spaces with a real name, typed key by key, then submit with Return.
    for _ in 0..3 {
        press(&app, Key::Backspace);
    }
    type_into(&app, "Student Name", "Ada Lovelace");
    assert_eq!(value(&app, "Student Name"), "Ada Lovelace");
    assert!(enabled(&app, "Add"));
    click(&app, "Add");
    assert_eq!(value(&app, "Student Name"), "", "the field is cleared after adding");
    assert_eq!(saved(dir.path()).students.len(), 1);
    assert_eq!(saved(dir.path()).students[0].grade_level, 1);

    type_into(&app, "Student Name", "Grace Hopper");
    press(&app, Key::Return);
    assert_eq!(saved(dir.path()).students.len(), 2, "Return adds the student too");
    assert_eq!(backend.get_students().row_count(), 2);

    // ---- Grades: typing and adding a grade ----------------------------------------
    app.set_tab(0);
    assert_eq!(row_names(&app), ["Ada Lovelace", "Grace Hopper"]);

    // Grades are entered as Homework, Test or Quiz, in that order, on the selector and
    // in the legend of tints above the list.
    let tab = |label| x_of(&app, AccessibleRole::Tab, label);
    assert!(tab("Homework") < tab("Test") && tab("Test") < tab("Quiz"));
    let legend = |label| x_of(&app, AccessibleRole::Text, label);
    assert!(legend("Homework") < legend("Tests") && legend("Tests") < legend("Quizzes"));

    // Letters are rejected while typing, a comma counts as the decimal separator.
    type_into(&app, "Grade for Ada Lovelace", "9x2,5");
    assert_eq!(value(&app, "Grade for Ada Lovelace"), "92.5");
    press(&app, Key::Return);
    let data = saved(dir.path());
    assert_eq!(data.grades.len(), 1, "Return adds the grade");
    assert_eq!(
        (data.grades[0].value.as_str(), data.grades[0].grade_type),
        ("92.5", GradeType::Test)
    );
    assert_eq!((data.grades[0].subject.as_str(), data.grades[0].period), ("Math", 1));
    assert_eq!(
        value(&app, "Grade for Ada Lovelace"),
        "",
        "the field is cleared after adding"
    );

    let rows = backend.get_rows();
    let ada = rows.row_data(0).unwrap();
    assert_eq!(ada.chips.row_count(), 1);
    assert_eq!(ada.chips.row_data(0).unwrap().letter, "B+");
    assert_eq!(ada.average, "92.5 B+");
    assert_eq!(backend.get_class_average(), "92.5 B+");

    // The type selector is shared by every row, and starts on Test.
    assert_eq!(backend.get_grade_type(), GradeType::Test.index() as i32);
    click(&app, "Quiz");
    assert_eq!(backend.get_grade_type(), GradeType::Quiz.index() as i32);
    type_into(&app, "Grade for Grace Hopper", "70");
    click(&app, "Add grade for Grace Hopper");
    let data = saved(dir.path());
    assert_eq!(data.grades.len(), 2);
    assert_eq!(data.grades[1].grade_type, GradeType::Quiz);
    assert_eq!(
        backend.get_class_average(),
        "81.2 C",
        "mean of 92.5 and 70 is 81.25, shown as 81.2"
    );

    // Unusable input is ignored and left in the field.
    type_into(&app, "Grade for Grace Hopper", ".");
    click(&app, "Add grade for Grace Hopper");
    assert_eq!(saved(dir.path()).grades.len(), 2);
    assert_eq!(value(&app, "Grade for Grace Hopper"), ".");

    // A typed-but-unsent grade survives adding a grade for someone else.
    type_into(&app, "Grade for Ada Lovelace", "55");
    click(&app, "Add grade for Grace Hopper");
    assert_eq!(value(&app, "Grade for Ada Lovelace"), "55");

    // Clicking a tab takes the keyboard focus off the page being hidden, so
    // keystrokes can't end up in its fields.
    click(&app, "Settings");
    press(&app, "7");
    click(&app, "Grades");
    assert_eq!(value(&app, "Grade for Ada Lovelace"), "55");

    // ---- Grades: the picker menu works from the keyboard ----------------------------
    let pickers = ElementQuery::from_root(app.window())
        .match_accessible_role(AccessibleRole::Combobox)
        .find_all();
    assert_eq!(pickers.len(), 3, "Grade, Period and Subject (Settings is hidden)");
    assert_eq!(backend.get_grade_level_index(), 0);
    pickers[0].invoke_accessible_expand_action();
    press(&app, Key::DownArrow);
    assert_eq!(backend.get_grade_level_index(), 1, "Down picks the next option");
    assert!(
        row_names(&app).is_empty(),
        "and the list follows: grade 2 has no students"
    );
    press(&app, Key::UpArrow);
    assert_eq!(backend.get_grade_level_index(), 0);
    assert_eq!(row_names(&app), ["Ada Lovelace", "Grace Hopper"]);
    press(&app, Key::Escape);
    press(&app, Key::DownArrow);
    assert_eq!(backend.get_grade_level_index(), 0, "Escape closed the menu");

    // ---- Grades: changing filters ----------------------------------------------------
    backend.set_period_index(1);
    backend.invoke_filters_changed();
    assert_eq!(
        backend.get_rows().row_data(0).unwrap().chips.row_count(),
        0,
        "period 2 has no grades"
    );
    assert_eq!(backend.get_class_average(), "");
    backend.set_period_index(0);
    backend.invoke_filters_changed();
    assert_eq!(backend.get_rows().row_data(0).unwrap().chips.row_count(), 1);
    assert_eq!(
        value(&app, "Grade for Ada Lovelace"),
        "55",
        "drafts survive a filter change"
    );

    // ---- Grades: removing a grade -----------------------------------------------------
    click(&app, "Remove grade 92.5 for Ada Lovelace");
    assert_eq!(saved(dir.path()).grades.len(), 1);
    assert_eq!(backend.get_rows().row_data(0).unwrap().average, "");
    assert_eq!(backend.get_class_average(), "70.0 D");

    // ---- Settings: weights ------------------------------------------------------------
    app.set_tab(1);
    let sliders = ElementQuery::from_root(app.window())
        .match_accessible_role(AccessibleRole::Slider)
        .find_all();
    assert_eq!(sliders.len(), 3);
    sliders[0].invoke_accessible_increment_action();
    assert_eq!(backend.get_test_weight(), 51.0);
    assert_eq!(
        saved(dir.path()).test_weight,
        50.0,
        "weights are saved after a short pause, not on every step"
    );
    mock_elapsed_time(Duration::from_millis(600));
    assert_eq!(saved(dir.path()).test_weight, 51.0);
    sliders[1].invoke_accessible_decrement_action();
    assert_eq!(backend.get_quiz_weight(), 16.0);

    // Dragging with the mouse works, and snaps to whole numbers.
    let slider = &sliders[2];
    let (pos, size) = (slider.absolute_position(), slider.size());
    slider.mock_drag(
        slint::LogicalPosition::new(pos.x + size.width * 0.75, pos.y + size.height / 2.0),
        PointerEventButton::Left,
    );
    let weight = backend.get_homework_weight();
    assert!(
        (weight - weight.round()).abs() < f32::EPSILON && (60.0..=80.0).contains(&weight),
        "dragged to {weight}"
    );

    // ---- Settings: deleting a student (two steps) ------------------------------------
    click(&app, "Delete Grace Hopper");
    assert!(count(&app, "Confirm delete Grace Hopper") > 0);
    assert_eq!(
        backend.get_students().row_count(),
        2,
        "revealing the button does not delete yet"
    );
    element(&app, "Confirm delete Grace Hopper").invoke_accessible_default_action();
    let data = saved(dir.path());
    assert_eq!(data.students.len(), 1);
    assert!(data.grades.is_empty(), "the student's grades go with them");

    // ---- Backup and restore ----------------------------------------------------------
    let backup = dir.path().join("GradeBookBackup.json");
    app.backup_to(&backup);
    let backed_up = store::read_backup_file(&backup).unwrap();
    assert_eq!(backed_up, saved(dir.path()));

    type_into(&app, "Student Name", "Temporary");
    click(&app, "Add");
    assert_eq!(saved(dir.path()).students.len(), 2);

    app.restore_from(&backup);
    assert!(backend.get_alert().visible);
    assert_eq!(backend.get_alert().title, "Restore Backup?");
    // Cancelling changes nothing.
    button(&app, "Cancel").invoke_accessible_default_action();
    assert!(!backend.get_alert().visible);
    assert_eq!(saved(dir.path()).students.len(), 2);

    app.restore_from(&backup);
    button(&app, "Restore").invoke_accessible_default_action();
    assert!(!backend.get_alert().visible);
    assert_eq!(saved(dir.path()), backed_up, "restoring brings back the backed-up data");
    assert_eq!(backend.get_students().row_count(), 1);
    let copies = restore_copies(dir.path());
    assert_eq!(
        copies.len(),
        1,
        "the replaced data is kept in a copy next to the data file"
    );
    assert_eq!(copies[0].students.len(), 2);

    // Restoring again must not overwrite that copy: it may hold the only copy of the
    // data from before the first restore.
    type_into(&app, "Student Name", "Another");
    click(&app, "Add");
    assert_eq!(saved(dir.path()).students.len(), 2);
    app.restore_from(&backup);
    button(&app, "Restore").invoke_accessible_default_action();
    assert_eq!(saved(dir.path()), backed_up);
    let copies = restore_copies(dir.path());
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
    let junk = dir.path().join("junk.json");
    std::fs::write(&junk, "{\"hello\": 1}").unwrap();
    app.restore_from(&junk);
    assert_eq!(backend.get_alert().title, "Couldn't Restore");
    assert!(backend.get_alert().message.contains("isn't a GradeBook backup"));
    press(&app, Key::Tab);
    press(&app, "z");
    assert_eq!(value(&app, "Student Name"), "");
    press(&app, Key::Escape);
    assert!(!backend.get_alert().visible);
    assert_eq!(saved(dir.path()), backed_up);

    // A file that can't be read at all says so, instead of calling it a bad backup.
    app.restore_from(&dir.path().join("does-not-exist.json"));
    assert_eq!(backend.get_alert().title, "Couldn't Restore");
    assert!(backend.get_alert().message.contains("couldn't be read"));
    press(&app, Key::Escape);

    // ---- A restore that can't keep a copy of the current data changes nothing -------------
    let blocked_dir = tempfile::tempdir().unwrap();
    let blocked = App::new(Some(Store::in_dir(blocked_dir.path()))).unwrap();
    blocked.window().show().unwrap();
    blocked
        .window()
        .window()
        .set_size(slint::LogicalSize::new(1000.0, 760.0));
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
    button(&blocked, "Restore").invoke_accessible_default_action();
    let alert = blocked_backend.get_alert();
    assert!(alert.visible && alert.title == "Couldn't Restore", "{}", alert.title);
    assert!(alert.message.contains("nothing was changed"), "{}", alert.message);
    assert_eq!(
        blocked_backend.get_students().row_count(),
        1,
        "the current data is untouched"
    );
    assert_eq!(blocked_backend.get_students().row_data(0).unwrap().name, "Keep me");

    // ---- Starting with a damaged data file ---------------------------------------------
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

    // Without anywhere to save, the user is told, and can dismiss it with OK.
    let third = App::new(None).unwrap();
    third.notify("Nowhere to Save", "Changes will be lost.");
    let alert = third.window().global::<Backend>().get_alert();
    assert!(alert.visible && alert.confirm_label.is_empty());
    assert_eq!(alert.title, "Nowhere to Save");

    // ---- Light and dark --------------------------------------------------------------
    // The button in the corner switches the appearance on either screen, and the choice
    // is remembered for the next start.
    let theme_dir = tempfile::tempdir().unwrap();
    let themed = App::new(Some(Store::in_dir(theme_dir.path()))).unwrap();
    themed.window().show().unwrap();
    themed
        .window()
        .window()
        .set_size(slint::LogicalSize::new(1000.0, 760.0));
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

    themed.set_tab(1);
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
