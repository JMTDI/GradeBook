//! Glue between the Slint UI and the gradebook logic.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use uuid::Uuid;

use crate::book::GradeBook;
use crate::calc;
use crate::model::{BackupData, GradeType};
use crate::store::{self, LoadOutcome, Store, StoreError};
use crate::ui::{AlertData, Backend, ChipData, MainWindow, StudentItem, StudentRowData, Theme};
use crate::view::{self, Filters, GradesView, RowView};

/// How long to wait after the last weight-slider change before writing to disk.
const WEIGHT_SAVE_DELAY: Duration = Duration::from_millis(400);

/// What the confirmation alert is currently asking about.
enum Pending {
    Nothing,
    Restore(BackupData),
}

/// Lets only one file dialog be open at a time. The system's file choosers aren't tied
/// to the app window, so without this a double click on Backup would open two.
#[derive(Clone, Default)]
struct DialogGate(Rc<Cell<bool>>);

/// Held while a file dialog is open; dropping it lets the next one open.
struct DialogPermit(Rc<Cell<bool>>);

impl DialogGate {
    fn try_enter(&self) -> Option<DialogPermit> {
        if self.0.replace(true) {
            None
        } else {
            Some(DialogPermit(self.0.clone()))
        }
    }
}

impl Drop for DialogPermit {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

struct State {
    book: GradeBook,
    store: Option<Store>,
    filters: Filters,
    /// What was typed in each student's grade field, kept across filter changes.
    drafts: HashMap<Uuid, String>,
    pending: Pending,
    rows: Rc<VecModel<StudentRowData>>,
    save_failed_alert_shown: bool,
}

pub struct App {
    window: MainWindow,
    state: Rc<RefCell<State>>,
    // Kept alive so a pending debounced save still fires.
    _weight_timer: Rc<Timer>,
    dirty: Rc<Cell<bool>>,
    dialogs: DialogGate,
}

impl App {
    /// Builds the window. `store` is where data is loaded from and saved to;
    /// `None` keeps everything in memory only.
    pub fn new(store: Option<Store>) -> Result<App, slint::PlatformError> {
        let window = MainWindow::new()?;

        let (book, load_alert) = match store.as_ref().map(|s| s.load()) {
            Some(LoadOutcome::Loaded(data)) => (GradeBook::new(data), None),
            Some(LoadOutcome::Unreadable { error, moved_to }) => {
                let moved = match moved_to {
                    Some(path) => format!("It was moved to:\n{}\n\n", path.display()),
                    None => String::new(),
                };
                let message =
                    format!("Your saved data could not be read, so GradeBook started empty.\n\n{moved}{error}");
                (GradeBook::default(), Some(("Couldn't Open Saved Data", message)))
            }
            Some(LoadOutcome::Missing) | None => (GradeBook::default(), None),
        };

        let state = Rc::new(RefCell::new(State {
            book,
            store,
            filters: Filters::default(),
            drafts: HashMap::new(),
            pending: Pending::Nothing,
            rows: Rc::new(VecModel::default()),
            save_failed_alert_shown: false,
        }));
        let weight_timer = Rc::new(Timer::default());
        let dirty = Rc::new(Cell::new(false));

        let app = App {
            window,
            state,
            _weight_timer: weight_timer,
            dirty,
            dialogs: DialogGate::default(),
        };
        app.install_options();
        app.install_callbacks();
        push_weights(&app.window, &app.state);
        refresh_all(&app.window, &app.state);
        if let Some((title, message)) = load_alert {
            show_alert(&app.window, title, &message, "", false);
        }
        Ok(app)
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
    }

    /// Writes any change that is still waiting for the debounce timer.
    pub fn flush(&self) {
        if self.dirty.replace(false) {
            save(&self.window, &self.state);
        }
    }

    pub fn run(&self) -> Result<(), slint::PlatformError> {
        self.window.run()?;
        self.flush();
        Ok(())
    }

    fn install_options(&self) {
        let backend = self.window.global::<Backend>();
        backend.set_grade_level_options(strings(calc::GRADE_LEVELS.map(|n| format!("Grade {n}"))));
        backend.set_period_options(strings(calc::PERIODS.map(|n| format!("Period {n}"))));
        backend.set_subject_options(strings(calc::SUBJECTS.iter().map(|s| s.to_string())));
        let math = calc::SUBJECTS
            .iter()
            .position(|s| *s == calc::DEFAULT_SUBJECT)
            .unwrap_or(0);
        backend.set_subject_index(math as i32);
    }

    fn install_callbacks(&self) {
        let backend = self.window.global::<Backend>();

        backend.on_sanitize_grade(|text| calc::sanitize_grade_input(&text).into());
        backend.on_is_blank(|text| text.trim().is_empty());

        // Grade / Period / Subject pickers.
        {
            let (weak, state) = (self.window.as_weak(), self.state.clone());
            backend.on_filters_changed(move || {
                let Some(window) = weak.upgrade() else { return };
                let backend = window.global::<Backend>();
                {
                    let mut s = state.borrow_mut();
                    s.filters = Filters {
                        grade_level: (backend.get_grade_level_index().max(0) as u32) + *calc::GRADE_LEVELS.start(),
                        period: (backend.get_period_index().max(0) as u32) + *calc::PERIODS.start(),
                        subject: calc::SUBJECTS
                            .get(backend.get_subject_index().max(0) as usize)
                            .unwrap_or(&calc::DEFAULT_SUBJECT)
                            .to_string(),
                    };
                }
                refresh_grades(&window, &state);
            });
        }

        // Adding and removing grades.
        {
            let (weak, state) = (self.window.as_weak(), self.state.clone());
            backend.on_add_grade(move |student_id, text| {
                let Some(window) = weak.upgrade() else { return false };
                let (Ok(student_id), Some(value)) = (Uuid::parse_str(&student_id), calc::parse_grade_input(&text))
                else {
                    return false;
                };
                {
                    let mut s = state.borrow_mut();
                    let grade_type = GradeType::from_index(window.global::<Backend>().get_grade_type().max(0) as usize)
                        .unwrap_or(GradeType::Test);
                    let (subject, period) = (s.filters.subject.clone(), s.filters.period);
                    s.book.add_grade(student_id, &subject, period, value, grade_type);
                    s.drafts.remove(&student_id);
                }
                save(&window, &state);
                refresh_student(&window, &state, student_id);
                true
            });
        }
        {
            let (weak, state) = (self.window.as_weak(), self.state.clone());
            backend.on_delete_grade(move |grade_id| {
                let Some(window) = weak.upgrade() else { return };
                let Ok(grade_id) = Uuid::parse_str(&grade_id) else {
                    return;
                };
                let owner = {
                    let mut s = state.borrow_mut();
                    let owner = s
                        .book
                        .data()
                        .grades
                        .iter()
                        .find(|g| g.id == grade_id)
                        .map(|g| g.student_id);
                    s.book.delete_grade(grade_id);
                    owner
                };
                save(&window, &state);
                if let Some(owner) = owner {
                    refresh_student(&window, &state, owner);
                }
            });
        }
        {
            let state = self.state.clone();
            backend.on_draft_edited(move |student_id, text| {
                if let Ok(id) = Uuid::parse_str(&student_id) {
                    let mut s = state.borrow_mut();
                    if text.is_empty() {
                        s.drafts.remove(&id);
                    } else {
                        s.drafts.insert(id, text.to_string());
                    }
                }
            });
        }

        // Settings: students.
        {
            let (weak, state) = (self.window.as_weak(), self.state.clone());
            backend.on_add_student(move |name, level| {
                let Some(window) = weak.upgrade() else { return };
                let added = state.borrow_mut().book.add_student(&name, level.max(1) as u32);
                if added.is_some() {
                    save(&window, &state);
                    refresh_all(&window, &state);
                }
            });
        }
        {
            let (weak, state) = (self.window.as_weak(), self.state.clone());
            backend.on_delete_student(move |student_id| {
                let Some(window) = weak.upgrade() else { return };
                let Ok(id) = Uuid::parse_str(&student_id) else { return };
                {
                    let mut s = state.borrow_mut();
                    s.book.delete_student(id);
                    s.drafts.remove(&id);
                }
                save(&window, &state);
                refresh_all(&window, &state);
            });
        }

        // Settings: weights (saved shortly after the slider stops moving).
        {
            let (weak, state) = (self.window.as_weak(), self.state.clone());
            let (timer, dirty) = (self._weight_timer.clone(), self.dirty.clone());
            backend.on_weights_changed(move || {
                let Some(window) = weak.upgrade() else { return };
                let backend = window.global::<Backend>();
                {
                    let mut s = state.borrow_mut();
                    s.book.set_weight(GradeType::Test, backend.get_test_weight() as f64);
                    s.book.set_weight(GradeType::Quiz, backend.get_quiz_weight() as f64);
                    s.book
                        .set_weight(GradeType::Homework, backend.get_homework_weight() as f64);
                }
                refresh_grades(&window, &state);
                dirty.set(true);
                let (weak, state, dirty) = (weak.clone(), state.clone(), dirty.clone());
                timer.start(TimerMode::SingleShot, WEIGHT_SAVE_DELAY, move || {
                    if let (Some(window), true) = (weak.upgrade(), dirty.replace(false)) {
                        save(&window, &state);
                    }
                });
            });
        }

        // Settings: backup and restore.
        {
            let (weak, state, gate) = (self.window.as_weak(), self.state.clone(), self.dialogs.clone());
            backend.on_backup(move || {
                let Some(permit) = gate.try_enter() else { return };
                let (weak, state) = (weak.clone(), state.clone());
                let _ = slint::spawn_local(async move {
                    let _permit = permit;
                    let chosen = rfd::AsyncFileDialog::new()
                        .set_title("Save GradeBook Backup")
                        .set_file_name("GradeBookBackup.json")
                        .add_filter("JSON", &["json"])
                        .save_file()
                        .await;
                    let (Some(file), Some(window)) = (chosen, weak.upgrade()) else {
                        return;
                    };
                    write_backup(&window, &state, file.path());
                });
            });
        }
        {
            let (weak, state, gate) = (self.window.as_weak(), self.state.clone(), self.dialogs.clone());
            backend.on_restore(move || {
                let Some(permit) = gate.try_enter() else { return };
                let (weak, state) = (weak.clone(), state.clone());
                let _ = slint::spawn_local(async move {
                    let _permit = permit;
                    let chosen = rfd::AsyncFileDialog::new()
                        .set_title("Restore GradeBook Backup")
                        .add_filter("JSON", &["json"])
                        .pick_file()
                        .await;
                    let (Some(file), Some(window)) = (chosen, weak.upgrade()) else {
                        return;
                    };
                    begin_restore(&window, &state, file.path());
                });
            });
        }

        // Alerts.
        {
            let (weak, state) = (self.window.as_weak(), self.state.clone());
            backend.on_alert_confirmed(move || {
                let Some(window) = weak.upgrade() else { return };
                let pending = std::mem::replace(&mut state.borrow_mut().pending, Pending::Nothing);
                hide_alert(&window);
                let Pending::Restore(data) = pending else { return };

                // Keep a copy of what is about to be replaced, and don't go on without it.
                let copied = match &state.borrow().store {
                    Some(store) => store.copy_before_restore().map(drop),
                    None => Ok(()),
                };
                if let Err(e) = copied {
                    show_alert(
                        &window,
                        "Couldn't Restore",
                        &format!("A copy of your current data couldn't be saved first, so nothing was changed.\n\n{e}"),
                        "",
                        false,
                    );
                    return;
                }

                {
                    let mut s = state.borrow_mut();
                    s.book.replace(data);
                    s.drafts.clear();
                }
                push_weights(&window, &state);
                save(&window, &state);
                refresh_all(&window, &state);
            });
        }
        {
            let (weak, state) = (self.window.as_weak(), self.state.clone());
            backend.on_alert_dismissed(move || {
                if let Some(window) = weak.upgrade() {
                    state.borrow_mut().pending = Pending::Nothing;
                    hide_alert(&window);
                }
            });
        }
    }

    /// Switches between "Grades" (0) and "Settings" (1).
    pub fn set_tab(&self, tab: i32) {
        self.window.set_tab(tab);
    }

    /// Shows a message the user dismisses with OK.
    pub fn notify(&self, title: &str, message: &str) {
        show_alert(&self.window, title, message, "", false);
    }

    /// Writes a backup file to `path` (what the Backup button does after the user
    /// picks a file). Failures are shown to the user in an alert.
    pub fn backup_to(&self, path: &Path) {
        write_backup(&self.window, &self.state, path);
    }

    /// Reads a backup file and asks the user to confirm replacing the current data
    /// (what the Restore button does after the user picks a file).
    pub fn restore_from(&self, path: &Path) {
        begin_restore(&self.window, &self.state, path);
    }

    /// Forces the light or dark appearance instead of following the system.
    pub fn set_appearance(&self, appearance: Appearance) {
        self.window.global::<Theme>().set_scheme(match appearance {
            Appearance::System => 0,
            Appearance::Light => 1,
            Appearance::Dark => 2,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    /// Reads `GRADEBOOK_THEME` (`light` or `dark`); anything else follows the system.
    pub fn from_env() -> Appearance {
        match std::env::var("GRADEBOOK_THEME").ok().as_deref() {
            Some("light") => Appearance::Light,
            Some("dark") => Appearance::Dark,
            _ => Appearance::System,
        }
    }
}

pub fn run() -> Result<(), slint::PlatformError> {
    let store = store::default_data_dir().map(|dir| Store::in_dir(&dir));
    let in_memory_only = store.is_none();
    let app = App::new(store)?;
    app.set_appearance(Appearance::from_env());
    if in_memory_only {
        app.notify(
            "Nowhere to Save",
            "GradeBook couldn't find a folder to keep your data in, so changes will be lost when you quit. \
             Use Backup in Settings to keep a copy.",
        );
    }
    // Lets Linux desktops tie the window to its .desktop file and icon. It needs the
    // window system to be initialised already, which creating the window did.
    if let Err(e) = slint::set_xdg_app_id(crate::APP_ID) {
        eprintln!("GradeBook: couldn't set the application id: {e}");
    }
    app.run()
}

fn strings(items: impl Iterator<Item = String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(items.map(SharedString::from).collect::<Vec<_>>()))
}

fn to_row(row: &RowView, draft: &str) -> StudentRowData {
    let chips: Vec<ChipData> = row
        .chips
        .iter()
        .map(|chip| ChipData {
            id: chip.id.to_string().into(),
            value: chip.value.as_str().into(),
            letter: chip.letter.into(),
            kind: chip.kind.index() as i32,
        })
        .collect();
    StudentRowData {
        id: row.id.to_string().into(),
        name: row.name.as_str().into(),
        draft: draft.into(),
        chips: ModelRc::new(VecModel::from(chips)),
        average: row.average.clone().unwrap_or_default().into(),
    }
}

/// Rebuilds the Grades screen (rows, chips, averages).
fn refresh_grades(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    let GradesView { rows, class_average } = view::grades_view(&s.book, &s.filters);
    let data: Vec<StudentRowData> = rows
        .iter()
        .map(|row| to_row(row, s.drafts.get(&row.id).map(String::as_str).unwrap_or("")))
        .collect();
    s.rows.set_vec(data);
    let backend = window.global::<Backend>();
    backend.set_rows(ModelRc::from(s.rows.clone()));
    backend.set_class_average(class_average.unwrap_or_default().into());
}

/// Updates one student's row in place (so other rows keep what was typed in them).
fn refresh_student(window: &MainWindow, state: &Rc<RefCell<State>>, student_id: Uuid) {
    let s = state.borrow();
    let wanted = student_id.to_string();
    let position = s
        .rows
        .iter()
        .position(|row| row.id.as_str().eq_ignore_ascii_case(&wanted));
    if let (Some(position), Some(student)) = (position, s.book.student(student_id)) {
        let row = view::row_for(&s.book, student, &s.filters);
        s.rows.set_row_data(
            position,
            to_row(&row, s.drafts.get(&student_id).map(String::as_str).unwrap_or("")),
        );
    }
    let class_average = s
        .book
        .class_average(s.filters.grade_level, &s.filters.subject, s.filters.period);
    window
        .global::<Backend>()
        .set_class_average(class_average.map(calc::format_average).unwrap_or_default().into());
}

fn refresh_students(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    let items: Vec<StudentItem> = s
        .book
        .students()
        .iter()
        .map(|student| StudentItem {
            id: student.id.to_string().into(),
            name: student.name.as_str().into(),
            grade: format!("Grade {}", student.grade_level).into(),
        })
        .collect();
    window
        .global::<Backend>()
        .set_students(ModelRc::new(VecModel::from(items)));
}

fn refresh_all(window: &MainWindow, state: &Rc<RefCell<State>>) {
    refresh_students(window, state);
    refresh_grades(window, state);
}

fn save(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let result = {
        let s = state.borrow();
        s.store.as_ref().map(|store| store.save(s.book.data()))
    };
    match result {
        Some(Err(e)) => {
            // Tell the user once, not on every keystroke-sized change.
            let first = !std::mem::replace(&mut state.borrow_mut().save_failed_alert_shown, true);
            eprintln!("GradeBook: could not save: {e}");
            if first {
                show_alert(
                    window,
                    "Couldn't Save",
                    &format!("Your latest changes could not be saved.\n\n{e}"),
                    "",
                    false,
                );
            }
        }
        Some(Ok(())) => state.borrow_mut().save_failed_alert_shown = false,
        None => {}
    }
}

/// Shows the book's weights on the Settings sliders.
fn push_weights(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    let backend = window.global::<Backend>();
    backend.set_test_weight(s.book.weight(GradeType::Test) as f32);
    backend.set_quiz_weight(s.book.weight(GradeType::Quiz) as f32);
    backend.set_homework_weight(s.book.weight(GradeType::Homework) as f32);
}

fn write_backup(window: &MainWindow, state: &Rc<RefCell<State>>, path: &Path) {
    let data = state.borrow().book.data().clone();
    if let Err(e) = store::write_backup_file(path, &data) {
        show_alert(
            window,
            "Backup Failed",
            &format!("The backup could not be saved.\n\n{e}"),
            "",
            false,
        );
    }
}

fn begin_restore(window: &MainWindow, state: &Rc<RefCell<State>>, path: &Path) {
    match store::read_backup_file(path) {
        Ok(data) => {
            let message = format!(
                "This replaces all current students, grades and weights with the backup ({} students, {} grades).{}",
                data.students.len(),
                data.grades.len(),
                if state.borrow().store.is_some() {
                    " A copy of your current data is saved next to the data file first."
                } else {
                    ""
                },
            );
            state.borrow_mut().pending = Pending::Restore(data);
            show_alert(window, "Restore Backup?", &message, "Restore", true);
        }
        Err(e) => {
            let reason = match e {
                StoreError::Json(_) => "That file isn't a GradeBook backup.",
                StoreError::Io(_) => "That file couldn't be read.",
            };
            show_alert(window, "Couldn't Restore", &format!("{reason}\n\n{e}"), "", false);
        }
    }
}

fn show_alert(window: &MainWindow, title: &str, message: &str, confirm_label: &str, destructive: bool) {
    window.global::<Backend>().set_alert(AlertData {
        visible: true,
        title: title.into(),
        message: message.into(),
        confirm_label: confirm_label.into(),
        destructive,
    });
}

fn hide_alert(window: &MainWindow) {
    let backend = window.global::<Backend>();
    let mut alert = backend.get_alert();
    alert.visible = false;
    backend.set_alert(alert);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_file_dialog_can_be_open_at_a_time() {
        let gate = DialogGate::default();
        let first = gate.try_enter();
        assert!(first.is_some());
        assert!(
            gate.try_enter().is_none(),
            "a second click while one is open is ignored"
        );
        assert!(gate.clone().try_enter().is_none(), "clones share the same gate");
        drop(first);
        assert!(gate.try_enter().is_some(), "closing the dialog lets the next one open");
    }
}
