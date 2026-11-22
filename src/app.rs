//! Glue between the Slint UI and the gradebook logic.
//!
//! This file holds the state and the public face of the app; `refresh` fills the screens from
//! the state, and `callbacks` handles what the screens ask for.

mod callbacks;
mod refresh;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use chrono::{Local, NaiveDate};
use slint::{ComponentHandle, Timer, TimerMode, VecModel};
use uuid::Uuid;

use crate::book::GradeBook;
use crate::calc;
use crate::model::{BackupData, GradeType};
use crate::prefs::{Appearance, Preferences};
use crate::store::{self, LoadOutcome, Store};
use crate::system;
use crate::ui::{Backend, MainWindow, NameItem, StudentItem, StudentRowData, Theme};
use crate::update::{self, Fetcher};
use crate::view::{Filters, DEFAULT_ATTENTION};

use refresh::{refresh_all, show_alert};

/// How long to wait after the last weight-slider change before writing to disk.
const WEIGHT_SAVE_DELAY: Duration = Duration::from_millis(400);

/// How long the bar that offers an undo stays.
const TOAST_DURATION: Duration = Duration::from_secs(8);

/// How long to wait after the last change of a preference before writing it.
const PREFERENCE_SAVE_DELAY: Duration = Duration::from_millis(400);

/// What the confirmation alert is currently asking about.
enum Pending {
    Nothing,
    Restore(BackupData),
    NewYear,
    RemoveSubject(String),
    RemoveCategory(GradeType),
}

/// What a dialog that is open was opened on. Its number is the one `Backend.dialog` has.
enum Dialog {
    None,
    EditScore(Uuid),
    EditStudent(Uuid),
    PasteList,
    Needed(Uuid),
    Report(Uuid),
    Rename(RenameTarget),
}

impl Dialog {
    fn number(&self) -> i32 {
        match self {
            Dialog::None => 0,
            Dialog::EditScore(_) => 1,
            Dialog::EditStudent(_) => 2,
            Dialog::PasteList => 3,
            Dialog::Needed(_) => 4,
            Dialog::Report(_) => 5,
            Dialog::Rename(_) => 6,
        }
    }
}

#[derive(Clone)]
enum RenameTarget {
    Subject(String),
    Category(GradeType),
}

/// What the button on the bar at the bottom does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastAction {
    Undo,
    Redo,
}

impl ToastAction {
    fn label(self) -> &'static str {
        match self {
            ToastAction::Undo => "Undo",
            ToastAction::Redo => "Redo",
        }
    }
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

/// What opens a folder, a file or a web page.
type Opener = Rc<dyn Fn(&OsStr) -> io::Result<()>>;

struct State {
    book: GradeBook,
    store: Option<Store>,
    filters: Filters,
    /// What was typed in each student's grade field, kept across filter changes.
    drafts: HashMap<Uuid, String>,
    pending: Pending,
    dialog: Dialog,
    rows: Rc<VecModel<StudentRowData>>,
    /// The Students screen's list, and the lists of what was added on the Settings screen. All
    /// of these are updated in place (see `refresh::sync`), so that a control that stays keeps
    /// what it has, the keyboard included.
    student_items: Rc<VecModel<StudentItem>>,
    category_items: Rc<VecModel<NameItem>>,
    subject_items: Rc<VecModel<NameItem>>,
    level_items: Rc<VecModel<NameItem>>,
    /// Set when one of those lists lost an entry, which can take the keyboard with it. The
    /// handlers of undo and redo look at it, and clear it first.
    entry_removed: Cell<bool>,
    save_failed_alert_shown: bool,

    /// What the pickers list, so that the position picked maps back to a value.
    levels: Vec<u32>,
    subjects: Vec<String>,
    categories: Vec<GradeType>,
    /// The grade levels that can still be added.
    addable_levels: Vec<u32>,
    /// The subject the weights on the Settings screen are for; `None` is all of them.
    weight_scope: Option<String>,
    /// The Insights screen lists the averages below this.
    attention_below: u32,

    toast_action: ToastAction,
    /// The state of the history that the bar offering an undo or a redo is about. Once the
    /// history has moved on, the bar would take back something else than it says.
    toast_version: Option<u64>,
    /// What the last look for a newer version found.
    release: Option<update::Release>,

    // Where the outside world comes in, so tests can stand in for it.
    opener: Opener,
    today: Rc<dyn Fn() -> NaiveDate>,
    fetcher: Fetcher,
}

/// What every handler needs: the window, the state, and the timers that delay things.
#[derive(Clone)]
struct Ctx {
    weak: slint::Weak<MainWindow>,
    state: Rc<RefCell<State>>,
    // Kept alive so a pending debounced save still fires.
    weight_timer: Rc<Timer>,
    preference_timer: Rc<Timer>,
    toast_timer: Rc<Timer>,
    dirty: Rc<Cell<bool>>,
    dialogs: DialogGate,
}

impl Ctx {
    fn window(&self) -> Option<MainWindow> {
        self.weak.upgrade()
    }
}

pub struct App {
    window: MainWindow,
    ctx: Ctx,
}

impl App {
    /// Builds the window. `store` is where data is loaded from and saved to;
    /// `None` keeps everything in memory only.
    pub fn new(store: Option<Store>) -> Result<App, slint::PlatformError> {
        let window = MainWindow::new()?;
        let today: Rc<dyn Fn() -> NaiveDate> = Rc::new(|| Local::now().date_naive());

        let preferences = store.as_ref().map(Store::load_preferences).unwrap_or_default();
        let (book, load_alert) = match store.as_ref().map(|s| s.load()) {
            Some(LoadOutcome::Loaded(data)) => {
                // The first start of a day keeps a copy of the data as it was.
                if let Some(Err(e)) = store.as_ref().map(|s| s.daily_backup(today())) {
                    eprintln!("GradeBook: couldn't make the daily backup: {e}");
                }
                (GradeBook::new(data), None)
            }
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
            dialog: Dialog::None,
            rows: Rc::new(VecModel::default()),
            student_items: Rc::new(VecModel::default()),
            category_items: Rc::new(VecModel::default()),
            subject_items: Rc::new(VecModel::default()),
            level_items: Rc::new(VecModel::default()),
            entry_removed: Cell::new(false),
            save_failed_alert_shown: false,
            levels: Vec::new(),
            subjects: Vec::new(),
            categories: Vec::new(),
            addable_levels: Vec::new(),
            weight_scope: None,
            attention_below: preferences.attention_below.unwrap_or(DEFAULT_ATTENTION).min(100),
            toast_action: ToastAction::Undo,
            toast_version: None,
            release: None,
            opener: Rc::new(system::open),
            today,
            fetcher: update::github_fetcher(),
        }));
        let ctx = Ctx {
            weak: window.as_weak(),
            state,
            weight_timer: Rc::new(Timer::default()),
            preference_timer: Rc::new(Timer::default()),
            toast_timer: Rc::new(Timer::default()),
            dirty: Rc::new(Cell::new(false)),
            dialogs: DialogGate::default(),
        };

        callbacks::install(&window, &ctx);
        callbacks::remove_old_reports();
        let backend = window.global::<Backend>();
        backend.set_version(env!("CARGO_PKG_VERSION").into());
        backend.set_period_options(refresh::strings(calc::PERIODS.map(|n| format!("Period {n}"))));
        backend.set_check_updates(preferences.checks_for_updates());
        backend.set_attention_below(ctx.state.borrow().attention_below as f32);
        let start = GradeType::ALL.iter().position(|c| *c == GradeType::Test).unwrap_or(0);
        backend.set_grade_type(start as i32);
        if let Some(appearance) = preferences.appearance {
            apply_appearance(&window, appearance);
        }
        refresh_all(&window, &ctx.state);
        if let Some((title, message)) = load_alert {
            show_alert(&window, title, &message, "", false);
        }

        Ok(App { window, ctx })
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
    }

    /// Looks for a newer version on GitHub, as GradeBook does each time it starts, unless that
    /// was turned off in Settings. It is not part of `new`, so that a window made by a test
    /// never goes online.
    pub fn look_for_update_at_start(&self) {
        if self.window.global::<Backend>().get_check_updates() {
            callbacks::start_update_check(&self.window, &self.ctx, false);
        }
    }

    /// Writes any change that is still waiting for the debounce timer.
    pub fn flush(&self) {
        if self.ctx.dirty.replace(false) {
            refresh::save(&self.window, &self.ctx.state);
        }
    }

    pub fn run(&self) -> Result<(), slint::PlatformError> {
        self.window.run()?;
        self.flush();
        Ok(())
    }

    /// Switches between "Grades" (0), "Students" (1), "Insights" (2) and "Settings" (3).
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
        callbacks::write_backup(&self.window, &self.ctx.state, path);
    }

    /// Reads a backup file and asks the user to confirm replacing the current data
    /// (what the Restore button does after the user picks a file).
    pub fn restore_from(&self, path: &Path) {
        callbacks::begin_restore(&self.window, &self.ctx.state, path);
    }

    /// Writes every score as a CSV file (what Export Scores does after the user picks a file).
    pub fn export_scores_to(&self, path: &Path) {
        callbacks::export_scores(&self.window, &self.ctx.state, path);
    }

    /// Writes the report card that is open as a CSV file (what Save as CSV does).
    pub fn export_report_to(&self, path: &Path) {
        callbacks::export_report(&self.window, &self.ctx.state, path);
    }

    /// Forces the light or dark appearance instead of following the system.
    pub fn set_appearance(&self, appearance: Appearance) {
        apply_appearance(&self.window, appearance);
    }

    /// Replaces what opens folders, files and web pages (so a test can see what was opened).
    pub fn set_opener(&self, opener: impl Fn(&OsStr) -> io::Result<()> + 'static) {
        self.ctx.state.borrow_mut().opener = Rc::new(opener);
    }

    /// Replaces the day that counts as today (it names the backups and dates the reports).
    pub fn set_today(&self, today: impl Fn() -> NaiveDate + 'static) {
        self.ctx.state.borrow_mut().today = Rc::new(today);
    }

    /// Replaces the way the latest release is looked up.
    pub fn set_update_fetcher(&self, fetcher: Fetcher) {
        self.ctx.state.borrow_mut().fetcher = fetcher;
    }

    /// Delivers the answer of a look for a newer version, as the background thread does when it
    /// has one. `manual` says whether the look was asked for with the button.
    pub fn receive_update_answer(&self, manual: bool, answer: Result<String, String>) {
        let (ok, text) = match answer {
            Ok(text) => (true, text),
            Err(error) => (false, error),
        };
        self.window
            .global::<Backend>()
            .invoke_update_fetched(manual, ok, text.into());
    }
}

fn apply_appearance(window: &MainWindow, appearance: Appearance) {
    window.global::<Theme>().set_scheme(match appearance {
        Appearance::System => 0,
        Appearance::Light => 1,
        Appearance::Dark => 2,
    });
}

/// Changes the saved preferences. A failure is only noted: preferences are never worth
/// interrupting anyone for.
fn update_preferences(state: &Rc<RefCell<State>>, change: impl FnOnce(&mut Preferences)) {
    if let Some(store) = &state.borrow().store {
        let mut preferences = store.load_preferences();
        change(&mut preferences);
        if let Err(e) = store.save_preferences(&preferences) {
            eprintln!("GradeBook: couldn't save the preferences: {e}");
        }
    }
}

pub fn run() -> Result<(), slint::PlatformError> {
    let store = store::default_data_dir().map(|dir| Store::in_dir(&dir));
    let in_memory_only = store.is_none();
    let app = App::new(store)?;
    // A launch-time override beats the saved choice.
    let from_env = Appearance::from_env();
    if from_env != Appearance::System {
        app.set_appearance(from_env);
    }
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
    app.look_for_update_at_start();
    app.run()
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

    #[test]
    fn dialogs_have_the_numbers_the_screens_use() {
        let id = Uuid::nil();
        let numbers = [
            Dialog::None.number(),
            Dialog::EditScore(id).number(),
            Dialog::EditStudent(id).number(),
            Dialog::PasteList.number(),
            Dialog::Needed(id).number(),
            Dialog::Report(id).number(),
            Dialog::Rename(RenameTarget::Subject(String::new())).number(),
        ];
        assert_eq!(numbers, [0, 1, 2, 3, 4, 5, 6]);
    }
}
