//! What the screens ask for: the handlers of every callback of `Backend`.

use super::*;

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

use slint::{ModelRc, SharedString};

use crate::book::{self, ListError};
use crate::calc::{self, Needed};
use crate::export;
use crate::store::StoreError;
use crate::ui::ReportRowData;
use crate::update::Check;
use crate::view;

use super::refresh::{
    close_dialog, hide_alert, hide_toast, modal_open, open_dialog, refresh_all, refresh_grades, refresh_insights,
    refresh_name_lists, refresh_student, refresh_undo, refresh_weight_status, refresh_weights, save, show_alert,
    show_toast, strings,
};

/// A position as the screens pass it (a negative one is no position at all).
fn index(position: i32) -> usize {
    position.max(0) as usize
}

pub(super) fn install(window: &MainWindow, ctx: &Ctx) {
    let backend = window.global::<Backend>();
    inputs(&backend);
    grades(&backend, ctx);
    students(&backend, ctx);
    insights(&backend, ctx);
    weights(&backend, ctx);
    lists(&backend, ctx);
    data(&backend, ctx);
    about(&backend, ctx);
    history(&backend, ctx);
    dialogs(&backend, ctx);
    alerts(&backend, ctx);
    appearance(&backend, ctx);
}

/// The questions the screens ask while the user types.
fn inputs(backend: &Backend<'_>) {
    backend.on_sanitize_grade(|text| calc::sanitize_grade_input(&text).into());
    backend.on_is_blank(|text| text.trim().is_empty());
    backend.on_is_valid_score(|text| calc::parse_grade_input(&text).is_some());
    backend.on_count_names(|text| book::parse_names(&text).len() as i32);
}

/// Offers to take back the change that was just made.
fn toast_change(ctx: &Ctx, window: &MainWindow) {
    let label = ctx.state.borrow().book.undo_label().map(str::to_string);
    if let Some(label) = label {
        show_toast(ctx, window, &label, ToastAction::Undo);
    }
}

/// After a change to a list or a setting that can show up anywhere.
fn after_change(ctx: &Ctx, window: &MainWindow) {
    save(window, &ctx.state);
    refresh_all(window, &ctx.state);
}

fn grades(backend: &Backend<'_>, ctx: &Ctx) {
    // The Grade / Period / Subject pickers.
    let c = ctx.clone();
    backend.on_filters_changed(move || {
        let Some(window) = c.window() else { return };
        let backend = window.global::<Backend>();
        {
            let mut s = c.state.borrow_mut();
            let level = s
                .levels
                .get(index(backend.get_grade_level_index()))
                .copied()
                .unwrap_or(s.filters.grade_level);
            let period = calc::PERIODS.start() + index(backend.get_period_index()) as u32;
            let subject = s
                .subjects
                .get(index(backend.get_subject_index()))
                .cloned()
                .unwrap_or_else(|| calc::DEFAULT_SUBJECT.to_string());
            s.filters = Filters {
                grade_level: level,
                period,
                subject,
            };
        }
        refresh_grades(&window, &c.state);
        refresh_insights(&window, &c.state);
    });

    // Adding and removing scores.
    let c = ctx.clone();
    backend.on_add_grade(move |student_id, text| {
        let Some(window) = c.window() else { return false };
        let (Ok(student_id), Some(value)) = (Uuid::parse_str(&student_id), calc::parse_grade_input(&text)) else {
            return false;
        };
        {
            let mut s = c.state.borrow_mut();
            if s.book.student(student_id).is_none() {
                return false;
            }
            let category = s
                .categories
                .get(index(window.global::<Backend>().get_grade_type()))
                .cloned()
                .unwrap_or(GradeType::Test);
            let (subject, period) = (s.filters.subject.clone(), s.filters.period);
            s.book.add_grade(student_id, &subject, period, value, category);
            s.drafts.remove(&student_id);
        }
        save(&window, &c.state);
        refresh_student(&window, &c.state, student_id);
        true
    });

    let c = ctx.clone();
    backend.on_delete_grade(move |grade_id| {
        let Some(window) = c.window() else { return };
        let Ok(grade_id) = Uuid::parse_str(&grade_id) else {
            return;
        };
        let owner = {
            let mut s = c.state.borrow_mut();
            let owner = s.book.grade(grade_id).map(|g| g.student_id);
            s.book.delete_grade(grade_id);
            owner
        };
        let Some(owner) = owner else { return };
        save(&window, &c.state);
        refresh_student(&window, &c.state, owner);
        toast_change(&c, &window);
    });

    let c = ctx.clone();
    backend.on_draft_edited(move |student_id, text| {
        if let Ok(id) = Uuid::parse_str(&student_id) {
            let mut s = c.state.borrow_mut();
            if text.is_empty() {
                s.drafts.remove(&id);
            } else {
                s.drafts.insert(id, text.to_string());
            }
        }
    });

    let c = ctx.clone();
    backend.on_edit_grade(move |grade_id| {
        let Some(window) = c.window() else { return };
        let Ok(id) = Uuid::parse_str(&grade_id) else { return };
        {
            let s = c.state.borrow();
            let Some(grade) = s.book.grade(id) else { return };
            let who = s
                .book
                .student(grade.student_id)
                .map_or("", |student| student.name.as_str());
            let backend = window.global::<Backend>();
            backend.set_edit_title(format!("{who} · {} · Period {}", grade.subject, grade.period).into());
            backend.set_edit_value(grade.value.as_str().into());
            let kind = s.categories.iter().position(|category| *category == grade.grade_type);
            backend.set_edit_kind(kind.unwrap_or(0) as i32);
        }
        open_dialog(&window, &c.state, Dialog::EditScore(id));
    });

    let c = ctx.clone();
    backend.on_open_needed(move |student_id| {
        let Some(window) = c.window() else { return };
        let Ok(id) = Uuid::parse_str(&student_id) else { return };
        open_needed(&window, &c.state, id);
    });

    let c = ctx.clone();
    backend.on_open_report(move |student_id| {
        let Some(window) = c.window() else { return };
        let Ok(id) = Uuid::parse_str(&student_id) else { return };
        open_report(&window, &c.state, id);
    });
}

fn students(backend: &Backend<'_>, ctx: &Ctx) {
    let c = ctx.clone();
    backend.on_add_student(move |name, level| {
        let Some(window) = c.window() else { return };
        let added = {
            let mut s = c.state.borrow_mut();
            let level = s.levels.get(index(level)).copied().unwrap_or(1);
            s.book.add_student(&name, level)
        };
        if added.is_some() {
            after_change(&c, &window);
        }
    });

    let c = ctx.clone();
    backend.on_delete_student(move |student_id| {
        let Some(window) = c.window() else { return };
        let Ok(id) = Uuid::parse_str(&student_id) else { return };
        let deleted = {
            let mut s = c.state.borrow_mut();
            s.drafts.remove(&id);
            s.book.delete_student(id)
        };
        if deleted {
            after_change(&c, &window);
            toast_change(&c, &window);
            // The row that had the keyboard is gone. Give it to the shortcuts, so that Ctrl+Z
            // can take the deletion back right away.
            give_keyboard_back(&window);
        }
    });

    let c = ctx.clone();
    backend.on_edit_student(move |student_id| {
        let Some(window) = c.window() else { return };
        let Ok(id) = Uuid::parse_str(&student_id) else { return };
        {
            let s = c.state.borrow();
            let Some(student) = s.book.student(id) else { return };
            let backend = window.global::<Backend>();
            backend.set_student_name(student.name.as_str().into());
            let level = s.levels.iter().position(|level| *level == student.grade_level);
            backend.set_student_level(level.unwrap_or(0) as i32);
        }
        open_dialog(&window, &c.state, Dialog::EditStudent(id));
    });

    let c = ctx.clone();
    backend.on_open_paste_list(move || {
        let Some(window) = c.window() else { return };
        let level = {
            let s = c.state.borrow();
            s.levels.iter().position(|level| *level == s.filters.grade_level)
        };
        window.global::<Backend>().set_paste_level(level.unwrap_or(0) as i32);
        open_dialog(&window, &c.state, Dialog::PasteList);
    });
}

fn insights(backend: &Backend<'_>, ctx: &Ctx) {
    let c = ctx.clone();
    backend.on_attention_changed(move || {
        let Some(window) = c.window() else { return };
        let value = window
            .global::<Backend>()
            .get_attention_below()
            .round()
            .clamp(0.0, 100.0) as u32;
        c.state.borrow_mut().attention_below = value;
        refresh_insights(&window, &c.state);
        // Saved shortly after the slider stops moving.
        let state = c.state.clone();
        c.preference_timer
            .start(TimerMode::SingleShot, PREFERENCE_SAVE_DELAY, move || {
                let value = state.borrow().attention_below;
                update_preferences(&state, |preferences| {
                    preferences.attention_below = (value != DEFAULT_ATTENTION).then_some(value);
                });
            });
    });
}

fn weights(backend: &Backend<'_>, ctx: &Ctx) {
    let c = ctx.clone();
    backend.on_weight_scope_changed(move || {
        let Some(window) = c.window() else { return };
        let position = index(window.global::<Backend>().get_weight_scope_index());
        {
            let mut s = c.state.borrow_mut();
            let scope = position.checked_sub(1).and_then(|i| s.subjects.get(i).cloned());
            s.weight_scope = scope;
        }
        refresh_weights(&window, &c.state);
    });

    // A slider moved. The sliders themselves aren't rebuilt: that would take the one that is
    // being dragged from under the pointer.
    let c = ctx.clone();
    backend.on_weight_changed(move |position, value| {
        let Some(window) = c.window() else { return };
        {
            let mut guard = c.state.borrow_mut();
            let s = &mut *guard;
            let Some(category) = s.categories.get(index(position)).cloned() else {
                return;
            };
            s.book
                .set_weight(s.weight_scope.as_deref(), &category, f64::from(value));
        }
        refresh_weight_status(&window, &c.state);
        refresh_grades(&window, &c.state);
        refresh_insights(&window, &c.state);
        refresh_name_lists(&window, &c.state);
        refresh_undo(&window, &c.state);
        // Saved shortly after the slider stops moving.
        c.dirty.set(true);
        let (weak, state, dirty) = (c.weak.clone(), c.state.clone(), c.dirty.clone());
        c.weight_timer.start(TimerMode::SingleShot, WEIGHT_SAVE_DELAY, move || {
            if let (Some(window), true) = (weak.upgrade(), dirty.replace(false)) {
                save(&window, &state);
            }
        });
    });

    let c = ctx.clone();
    backend.on_drop_lowest_changed(move |position, on| {
        let Some(window) = c.window() else { return };
        {
            let mut s = c.state.borrow_mut();
            let Some(category) = s.categories.get(index(position)).cloned() else {
                return;
            };
            s.book.set_drop_lowest(&category, on);
        }
        save(&window, &c.state);
        refresh_grades(&window, &c.state);
        refresh_insights(&window, &c.state);
        refresh_undo(&window, &c.state);
    });

    let c = ctx.clone();
    backend.on_reset_scope_weights(move || {
        let Some(window) = c.window() else { return };
        {
            let mut guard = c.state.borrow_mut();
            let s = &mut *guard;
            if let Some(subject) = s.weight_scope.as_deref() {
                s.book.reset_subject_weights(subject);
            }
        }
        after_change(&c, &window);
        // The button that was pressed is gone with the weights of the subject's own.
        give_keyboard_back(&window);
    });
}

fn open_rename(window: &MainWindow, state: &Rc<RefCell<State>>, title: &str, current: &str, target: RenameTarget) {
    let backend = window.global::<Backend>();
    backend.set_rename_title(title.into());
    backend.set_rename_text(current.into());
    open_dialog(window, state, Dialog::Rename(target));
}

fn show_list_error(window: &MainWindow, title: &str, error: &ListError) {
    show_alert(window, title, &error.to_string(), "", false);
}

/// Subjects, categories and grade levels that were added.
fn lists(backend: &Backend<'_>, ctx: &Ctx) {
    // Categories.
    let c = ctx.clone();
    backend.on_add_category(move |name| {
        let Some(window) = c.window() else { return false };
        let added = c.state.borrow_mut().book.add_category(&name);
        match added {
            Ok(_) => {
                after_change(&c, &window);
                true
            }
            Err(e) => {
                show_list_error(&window, "Couldn't Add the Category", &e);
                false
            }
        }
    });

    let c = ctx.clone();
    backend.on_rename_category(move |position| {
        let Some(window) = c.window() else { return };
        let category = {
            let s = c.state.borrow();
            s.categories
                .iter()
                .filter(|category| !category.is_built_in())
                .nth(index(position))
                .cloned()
        };
        if let Some(category) = category {
            let name = category.label().to_string();
            open_rename(
                &window,
                &c.state,
                "Rename Category",
                &name,
                RenameTarget::Category(category),
            );
        }
    });

    let c = ctx.clone();
    backend.on_remove_category(move |position| {
        let Some(window) = c.window() else { return };
        let category = {
            let s = c.state.borrow();
            s.categories
                .iter()
                .filter(|category| !category.is_built_in())
                .nth(index(position))
                .cloned()
        };
        let Some(category) = category else { return };
        let count = c.state.borrow().book.count_grades_in_category(&category);
        if count == 0 {
            remove_category(&c, &window, &category);
        } else {
            c.state.borrow_mut().pending = Pending::RemoveCategory(category.clone());
            show_alert(
                &window,
                "Remove Category?",
                &format!(
                    "\"{}\" has {count} {}. They are deleted with it. You can undo this right after.",
                    category.label(),
                    if count == 1 { "score" } else { "scores" }
                ),
                "Remove",
                true,
            );
        }
    });

    // Subjects.
    let c = ctx.clone();
    backend.on_add_subject(move |name| {
        let Some(window) = c.window() else { return false };
        let added = c.state.borrow_mut().book.add_subject(&name);
        match added {
            Ok(_) => {
                after_change(&c, &window);
                true
            }
            Err(e) => {
                show_list_error(&window, "Couldn't Add the Subject", &e);
                false
            }
        }
    });

    let c = ctx.clone();
    backend.on_rename_subject(move |position| {
        let Some(window) = c.window() else { return };
        let subject = c.state.borrow().book.custom_subjects().get(index(position)).cloned();
        if let Some(subject) = subject {
            open_rename(
                &window,
                &c.state,
                "Rename Subject",
                &subject,
                RenameTarget::Subject(subject.clone()),
            );
        }
    });

    let c = ctx.clone();
    backend.on_remove_subject(move |position| {
        let Some(window) = c.window() else { return };
        let subject = c.state.borrow().book.custom_subjects().get(index(position)).cloned();
        let Some(subject) = subject else { return };
        let count = c.state.borrow().book.count_grades_in_subject(&subject);
        if count == 0 {
            remove_subject(&c, &window, &subject);
        } else {
            c.state.borrow_mut().pending = Pending::RemoveSubject(subject.clone());
            show_alert(
                &window,
                "Remove Subject?",
                &format!(
                    "\"{subject}\" has {count} {}. They are deleted with it. You can undo this right after.",
                    if count == 1 { "score" } else { "scores" }
                ),
                "Remove",
                true,
            );
        }
    });

    // Grade levels.
    let c = ctx.clone();
    backend.on_add_level(move || {
        let Some(window) = c.window() else { return };
        let result = {
            let mut guard = c.state.borrow_mut();
            let s = &mut *guard;
            let position = index(window.global::<Backend>().get_addable_level_index());
            match s.addable_levels.get(position) {
                Some(level) => s.book.add_grade_level(*level),
                None => Err(ListError::NotFound),
            }
        };
        match result {
            Ok(()) => after_change(&c, &window),
            Err(e) => show_list_error(&window, "Couldn't Add the Grade Level", &e),
        }
    });

    let c = ctx.clone();
    backend.on_remove_level(move |position| {
        let Some(window) = c.window() else { return };
        let result = {
            let mut s = c.state.borrow_mut();
            match s.book.extra_levels().get(index(position)).copied() {
                Some(level) => s.book.remove_grade_level(level),
                None => Err(ListError::NotFound),
            }
        };
        match result {
            Ok(()) => {
                after_change(&c, &window);
                toast_change(&c, &window);
                give_keyboard_back(&window);
            }
            Err(e) => show_list_error(&window, "Couldn't Remove the Grade Level", &e),
        }
    });
}

fn remove_category(ctx: &Ctx, window: &MainWindow, category: &GradeType) {
    let removed = ctx.state.borrow_mut().book.remove_category(category);
    match removed {
        Ok(_) => {
            after_change(ctx, window);
            toast_change(ctx, window);
            give_keyboard_back(window);
        }
        Err(e) => show_list_error(window, "Couldn't Remove the Category", &e),
    }
}

fn remove_subject(ctx: &Ctx, window: &MainWindow, subject: &str) {
    let removed = ctx.state.borrow_mut().book.remove_subject(subject);
    match removed {
        Ok(_) => {
            after_change(ctx, window);
            toast_change(ctx, window);
            give_keyboard_back(window);
        }
        Err(e) => show_list_error(window, "Couldn't Remove the Subject", &e),
    }
}

/// Asks for a place to save a file, then calls `then` with it. Only one file dialog is open
/// at a time.
fn save_dialog(
    ctx: &Ctx,
    title: &'static str,
    file_name: String,
    filter: (&'static str, &'static [&'static str]),
    then: impl FnOnce(&MainWindow, &Rc<RefCell<State>>, &Path) + 'static,
) {
    let Some(permit) = ctx.dialogs.try_enter() else { return };
    let (weak, state) = (ctx.weak.clone(), ctx.state.clone());
    let _ = slint::spawn_local(async move {
        let _permit = permit;
        let chosen = rfd::AsyncFileDialog::new()
            .set_title(title)
            .set_file_name(&file_name)
            .add_filter(filter.0, filter.1)
            .save_file()
            .await;
        let (Some(file), Some(window)) = (chosen, weak.upgrade()) else {
            return;
        };
        then(&window, &state, file.path());
    });
}

fn data(backend: &Backend<'_>, ctx: &Ctx) {
    let c = ctx.clone();
    backend.on_backup(move || {
        save_dialog(
            &c,
            "Save GradeBook Backup",
            "GradeBookBackup.json".to_string(),
            ("JSON", &["json"]),
            write_backup,
        );
    });

    let c = ctx.clone();
    backend.on_restore(move || {
        let Some(permit) = c.dialogs.try_enter() else { return };
        let (weak, state) = (c.weak.clone(), c.state.clone());
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

    let c = ctx.clone();
    backend.on_open_data_folder(move || {
        let Some(window) = c.window() else { return };
        let folder = c.state.borrow().store.as_ref().map(|store| store.dir().to_path_buf());
        match folder {
            Some(folder) => {
                // The folder is made with the first save: before that there is nothing to open,
                // and an opener has nothing to say about a folder that isn't there.
                let _ = fs::create_dir_all(&folder);
                open_target(&window, &c.state, folder.as_os_str(), "The data folder");
            }
            None => show_alert(
                &window,
                "No Data Folder",
                "GradeBook found no folder to keep your data in.",
                "",
                false,
            ),
        }
    });

    let c = ctx.clone();
    backend.on_export_scores(move || {
        save_dialog(
            &c,
            "Export Scores",
            "GradeBookScores.csv".to_string(),
            ("CSV", &["csv"]),
            export_scores,
        );
    });

    let c = ctx.clone();
    backend.on_start_new_year(move || {
        let Some(window) = c.window() else { return };
        let plan = c.state.borrow().book.year_plan();
        if plan.moving == 0 && plan.staying == 0 && plan.scores == 0 {
            show_alert(
                &window,
                "Nothing to Do",
                "There are no students and no scores yet.",
                "",
                false,
            );
            return;
        }
        let students = |count: usize| format!("{count} {}", if count == 1 { "student" } else { "students" });
        let mut message = format!(
            "A copy of everything is saved in the backups folder first. Then {} {} cleared and {} up a grade",
            plan.scores,
            if plan.scores == 1 { "score is" } else { "scores are" },
            if plan.moving == 1 {
                "1 student moves".to_string()
            } else {
                format!("{} move", students(plan.moving))
            },
        );
        if plan.staying > 0 {
            message.push_str(&format!(
                " ({} in the highest grade stay where they are)",
                students(plan.staying)
            ));
        }
        message.push_str(". You can undo this until you quit.");
        c.state.borrow_mut().pending = Pending::NewYear;
        show_alert(&window, "Start a New School Year?", &message, "Start New Year", true);
    });
}

fn start_new_year(ctx: &Ctx, window: &MainWindow) {
    // Keep a copy of the year that ends, and don't go on without it.
    let archived = {
        let s = ctx.state.borrow();
        match &s.store {
            Some(store) => store.archive_school_year(s.book.data(), (s.today)()).map(drop),
            None => Ok(()),
        }
    };
    if let Err(e) = archived {
        show_alert(
            window,
            "Couldn't Start a New School Year",
            &format!("A copy of everything couldn't be saved first, so nothing was changed.\n\n{e}"),
            "",
            false,
        );
        return;
    }
    {
        let mut s = ctx.state.borrow_mut();
        s.book.new_school_year();
        s.drafts.clear();
    }
    after_change(ctx, window);
    toast_change(ctx, window);
}

/// Replaces the data with a backup, after the safety copy of what it replaces.
fn restore(ctx: &Ctx, window: &MainWindow, data: BackupData) {
    // Keep a copy of what is about to be replaced, and don't go on without it.
    let copied = match &ctx.state.borrow().store {
        Some(store) => store.copy_before_restore().map(drop),
        None => Ok(()),
    };
    if let Err(e) = copied {
        show_alert(
            window,
            "Couldn't Restore",
            &format!("A copy of your current data couldn't be saved first, so nothing was changed.\n\n{e}"),
            "",
            false,
        );
        return;
    }
    {
        let mut s = ctx.state.borrow_mut();
        s.book.replace(data);
        s.drafts.clear();
    }
    after_change(ctx, window);
    toast_change(ctx, window);
}

pub(super) fn write_backup(window: &MainWindow, state: &Rc<RefCell<State>>, path: &Path) {
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

pub(super) fn begin_restore(window: &MainWindow, state: &Rc<RefCell<State>>, path: &Path) {
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

fn write_text_file(window: &MainWindow, path: &Path, text: &str, title: &str) {
    if let Err(e) = fs::write(path, text) {
        show_alert(
            window,
            title,
            &format!("The file could not be saved.\n\n{e}"),
            "",
            false,
        );
    }
}

pub(super) fn export_scores(window: &MainWindow, state: &Rc<RefCell<State>>, path: &Path) {
    let csv = export::scores_csv(&state.borrow().book);
    write_text_file(window, path, &csv, "Export Failed");
}

pub(super) fn export_report(window: &MainWindow, state: &Rc<RefCell<State>>, path: &Path) {
    let csv = report_card_of_open_dialog(state).map(|card| export::report_csv(&card));
    if let Some(csv) = csv {
        write_text_file(window, path, &csv, "Export Failed");
    }
}

/// The report card the open dialog shows, built from the current data.
fn report_card_of_open_dialog(state: &Rc<RefCell<State>>) -> Option<view::ReportCard> {
    let s = state.borrow();
    let Dialog::Report(id) = &s.dialog else { return None };
    view::report_card(&s.book, *id)
}

/// Writes a file that only this user can read to the temporary folder, under a name nobody
/// can guess, and never over a file that is there already.
fn write_private_temp_file(stem: &str, extension: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let unique = Uuid::new_v4().simple().to_string();
    let path = std::env::temp_dir().join(format!("{stem}-{}.{extension}", &unique[..8]));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(bytes)?;
    Ok(path)
}

/// How long a printable report stays in the temporary folder.
const REPORT_KEPT: Duration = Duration::from_secs(24 * 60 * 60);

/// Whether a file name is one of the printable reports this app writes:
/// `GradeBook-<name>-report-<8 hex digits>.html` (see `write_private_temp_file`).
fn is_a_printable_report(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix("GradeBook-")
        .and_then(|rest| rest.strip_suffix(".html"))
    else {
        return false;
    };
    rest.rsplit_once("-report-")
        .is_some_and(|(_, random)| random.len() == 8 && random.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Deletes the printable reports in `folder` that are older than `older_than`. A report holds a
/// student's name and averages, and sits in the temporary folder, which on Windows nothing
/// empties by itself. Only files named like the app's reports are touched.
fn remove_old_reports_in(folder: &Path, older_than: Duration, now: SystemTime) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_str().is_some_and(is_a_printable_report) {
            continue;
        }
        let age = entry
            .metadata()
            .ok()
            .filter(|metadata| metadata.is_file())
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok());
        if age.is_some_and(|age| age > older_than) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Removes the printable reports that earlier runs left behind. Done when the app starts.
pub(super) fn remove_old_reports() {
    remove_old_reports_in(&std::env::temp_dir(), REPORT_KEPT, SystemTime::now());
}

/// Opens a folder, a file or a web page with the system's own program for it.
fn open_target(window: &MainWindow, state: &Rc<RefCell<State>>, target: &OsStr, what: &str) {
    let opener = state.borrow().opener.clone();
    if let Err(e) = opener(target) {
        show_alert(
            window,
            "Couldn't Open It",
            &format!("{what} couldn't be opened.\n\n{e}"),
            "",
            false,
        );
    }
}

fn open_printable_report(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let Some(card) = report_card_of_open_dialog(state) else {
        return;
    };
    let date = (state.borrow().today)().format("%-d %B %Y").to_string();
    let page = export::report_html(&card, &date);
    let stem = format!("GradeBook-{}-report", export::file_stem(&card.name));
    match write_private_temp_file(&stem, "html", page.as_bytes()) {
        Ok(path) => open_target(window, state, path.as_os_str(), "The report"),
        Err(e) => show_alert(
            window,
            "Couldn't Make the Report",
            &format!("The report could not be written.\n\n{e}"),
            "",
            false,
        ),
    }
}

/// Starts looking for a newer version on a background thread. A manual look says what it
/// found either way; an automatic one only speaks up when there is something to say. The
/// answer carries which of the two it is, so that two looks in flight can't mix them up.
pub(super) fn start_update_check(window: &MainWindow, ctx: &Ctx, manual: bool) {
    let fetcher = ctx.state.borrow().fetcher.clone();
    if manual {
        window
            .global::<Backend>()
            .set_update_status("Looking for a new version…".into());
    }
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let (ok, text) = match fetcher() {
            Ok(text) => (true, text),
            Err(error) => (false, error),
        };
        let _ = weak.upgrade_in_event_loop(move |window| {
            window
                .global::<Backend>()
                .invoke_update_fetched(manual, ok, text.into());
        });
    });
}

fn about(backend: &Backend<'_>, ctx: &Ctx) {
    let c = ctx.clone();
    backend.on_check_updates_changed(move || {
        let Some(window) = c.window() else { return };
        let on = window.global::<Backend>().get_check_updates();
        update_preferences(&c.state, |preferences| preferences.check_for_updates = Some(on));
        if on {
            start_update_check(&window, &c, true);
        }
    });

    let c = ctx.clone();
    backend.on_check_now(move || {
        let Some(window) = c.window() else { return };
        start_update_check(&window, &c, true);
    });

    let c = ctx.clone();
    backend.on_open_release_page(move || {
        let Some(window) = c.window() else { return };
        open_release_page(&window, &c.state);
    });

    let c = ctx.clone();
    backend.on_update_fetched(move |manual, ok, text| {
        let Some(window) = c.window() else { return };
        let backend = window.global::<Backend>();
        let current = update::current();
        let outcome = if ok {
            update::check(current, &text)
        } else {
            Err(text.to_string())
        };
        match outcome {
            Ok(Check::Available(release)) => {
                let message = format!("GradeBook {} is available.", release.version);
                backend.set_update_status(format!("{message} You have {current}.").into());
                backend.set_update_headline(message.into());
                backend.set_update_available(true);
                // The bar that was sent away stays away for a version that was already known,
                // unless the person asked for this look.
                let known = c
                    .state
                    .borrow()
                    .release
                    .as_ref()
                    .is_some_and(|known| known.version == release.version);
                if manual || !known {
                    backend.set_update_dismissed(false);
                }
                c.state.borrow_mut().release = Some(release);
            }
            Ok(Check::UpToDate) => {
                backend.set_update_status(format!("GradeBook {current} is the newest version.").into());
                backend.set_update_available(false);
                c.state.borrow_mut().release = None;
            }
            // A look that nobody asked for and that fails is no reason to bother anyone, and it
            // leaves the screen alone: one that was asked for may be under way.
            Err(error) if manual => {
                backend.set_update_status(format!("Couldn't look for a new version. {error}").into());
            }
            Err(_) => {}
        }
    });
}

fn open_release_page(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let url = state
        .borrow()
        .release
        .as_ref()
        .map_or_else(update::releases_page, |release| release.url.clone());
    open_target(window, state, OsStr::new(&url), "The download page");
}

fn undo(ctx: &Ctx, window: &MainWindow) {
    // Not while a dialog or an alert is open: what it is about could change under it.
    if modal_open(window) {
        return;
    }
    let label = ctx.state.borrow_mut().book.undo();
    after_history(ctx, window, label, "Undone", ToastAction::Redo);
}

fn redo(ctx: &Ctx, window: &MainWindow) {
    if modal_open(window) {
        return;
    }
    let label = ctx.state.borrow_mut().book.redo();
    after_history(ctx, window, label, "Redone", ToastAction::Undo);
}

/// Gives the keyboard to the shortcuts again. A control that is replaced or removed while it
/// has the keyboard takes it with it, and then no key reaches the shortcuts (Ctrl+Z and the
/// others) until something has the keyboard again.
fn give_keyboard_back(window: &MainWindow) {
    let backend = window.global::<Backend>();
    backend.set_keyboard_ticket(backend.get_keyboard_ticket().wrapping_add(1));
}

fn after_history(ctx: &Ctx, window: &MainWindow, label: Option<String>, verb: &str, next: ToastAction) {
    let Some(label) = label else { return };
    ctx.state.borrow().entry_removed.set(false);
    after_change(ctx, window);
    // Undo and redo replace the weight sliders of the Settings screen, and can remove rows and
    // entries of the lists. A control that had the keyboard is gone then, so the keyboard goes
    // back to the shortcuts. Elsewhere it is left alone, in the field that has it.
    if window.get_tab() == 3 || ctx.state.borrow().entry_removed.replace(false) {
        give_keyboard_back(window);
    }
    show_toast(ctx, window, &format!("{verb}: {label}"), next);
}

fn history(backend: &Backend<'_>, ctx: &Ctx) {
    let c = ctx.clone();
    backend.on_undo(move || {
        if let Some(window) = c.window() {
            undo(&c, &window);
        }
    });

    let c = ctx.clone();
    backend.on_redo(move || {
        if let Some(window) = c.window() {
            redo(&c, &window);
        }
    });

    let c = ctx.clone();
    backend.on_toast_action(move || {
        let Some(window) = c.window() else { return };
        let (action, stale) = {
            let s = c.state.borrow();
            (
                s.toast_action,
                s.toast_version.is_some_and(|version| version != s.book.version()),
            )
        };
        hide_toast(&window);
        // The bar names a step, and the history has moved on from it: it must not take back
        // another one. (Any change takes the bar away, so this is a second line of defence.)
        if stale {
            return;
        }
        match action {
            ToastAction::Undo => undo(&c, &window),
            ToastAction::Redo => redo(&c, &window),
        }
    });
}

/// The letters that can be asked for, with the lowest score each starts at.
fn needed_targets() -> Vec<String> {
    calc::LETTER_BANDS
        .iter()
        .map(|(_, letter)| letter.to_string())
        .collect()
}

/// The letter just above what the student has now: the one worth asking about.
fn default_target(average: Option<f64>) -> usize {
    let Some(average) = average else { return 1 };
    let shown = calc::displayed(average);
    calc::LETTER_BANDS
        .iter()
        .rposition(|(floor, _)| *floor > shown)
        .unwrap_or(0)
}

fn article(letter: &str) -> &'static str {
    if letter.starts_with(['A', 'E', 'F']) {
        "an"
    } else {
        "a"
    }
}

fn score_text(score: f64) -> String {
    let text = format!("{score:.1}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_string)
}

/// What the "What do I need?" dialog says.
fn needed_message(name: &str, subject: &str, category: &GradeType, letter: &str, needed: Needed) -> String {
    let category = category.label();
    let a = article(letter);
    match needed {
        Needed::Score(score) => format!(
            "{name} needs at least {} on the next {category} to reach {a} {letter} in {subject}.",
            score_text(score)
        ),
        Needed::Already => format!(
            "{name} is already safe: even a 0 on the next {category} keeps {subject} at {a} {letter} or better."
        ),
        Needed::OutOfReach { best } => format!(
            "Even a 100 on the next {category} only gets {name} to {} in {subject}. \
             Try a lower letter, or a category with more weight.",
            calc::format_average(best)
        ),
        Needed::NoEffect => {
            format!("{category} has no weight in {subject}, so a score there can't change the average.")
        }
    }
}

fn open_needed(window: &MainWindow, state: &Rc<RefCell<State>>, student_id: Uuid) {
    {
        let s = state.borrow();
        let Some(student) = s.book.student(student_id) else {
            return;
        };
        let backend = window.global::<Backend>();
        let average = s.book.student_average(student_id, &s.filters.subject, s.filters.period);
        backend.set_needed_title(format!("What Does {} Need?", student.name).into());
        backend.set_needed_subtitle(
            format!(
                "{} · Period {} · {}",
                s.filters.subject,
                s.filters.period,
                average.map_or("no average yet".to_string(), |a| format!(
                    "average now {}",
                    calc::format_average(a)
                ))
            )
            .into(),
        );
        backend.set_needed_targets(strings(needed_targets().into_iter()));
        backend.set_needed_target(default_target(average) as i32);
        // Start with the category that scores are being added in.
        backend.set_needed_kind(backend.get_grade_type());
    }
    open_dialog(window, state, Dialog::Needed(student_id));
    refresh_needed(window, state);
}

fn refresh_needed(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let message = {
        let s = state.borrow();
        let Dialog::Needed(student_id) = &s.dialog else { return };
        let Some(student) = s.book.student(*student_id) else {
            return;
        };
        let backend = window.global::<Backend>();
        let Some((floor, letter)) = calc::LETTER_BANDS.get(index(backend.get_needed_target())) else {
            return;
        };
        let Some(category) = s.categories.get(index(backend.get_needed_kind())) else {
            return;
        };
        let needed = s
            .book
            .needed(*student_id, &s.filters.subject, s.filters.period, category, *floor);
        needed_message(&student.name, &s.filters.subject, category, letter, needed)
    };
    window.global::<Backend>().set_needed_result(message.into());
}

fn open_report(window: &MainWindow, state: &Rc<RefCell<State>>, student_id: Uuid) {
    let card = view::report_card(&state.borrow().book, student_id);
    let Some(card) = card else { return };
    let row = |subject: &str, periods: &[Option<f64>], average: Option<f64>| ReportRowData {
        subject: subject.into(),
        cells: ModelRc::new(VecModel::from(
            periods
                .iter()
                .map(|p| SharedString::from(view::average_text(*p)))
                .collect::<Vec<_>>(),
        )),
        average: view::average_text(average).into(),
    };
    let rows: Vec<ReportRowData> = card
        .subjects
        .iter()
        .map(|subject| row(&subject.subject, &subject.periods, subject.average))
        .collect();
    let backend = window.global::<Backend>();
    backend.set_report_title(format!("Report Card: {}", card.name).into());
    backend.set_report_subtitle(
        format!(
            "{} · overall {}",
            calc::grade_level_label(card.grade_level),
            view::average_text(card.overall)
        )
        .into(),
    );
    backend.set_report_rows(ModelRc::new(VecModel::from(rows)));
    backend.set_report_total(row("Overall", &card.periods, card.overall));
    open_dialog(window, state, Dialog::Report(student_id));
}

/// What the dialogs do when they are saved, deleted from or closed.
fn dialogs(backend: &Backend<'_>, ctx: &Ctx) {
    let c = ctx.clone();
    backend.on_close_dialog(move || {
        if let Some(window) = c.window() {
            close_dialog(&window, &c.state);
        }
    });

    let c = ctx.clone();
    backend.on_save_edit_score(move |text, kind| {
        let Some(window) = c.window() else { return };
        let Some(value) = calc::parse_grade_input(&text) else {
            return;
        };
        let (changed, owner) = {
            let mut guard = c.state.borrow_mut();
            let s = &mut *guard;
            let Dialog::EditScore(id) = &s.dialog else { return };
            let id = *id;
            let category = s.categories.get(index(kind)).cloned().unwrap_or(GradeType::Test);
            let owner = s.book.grade(id).map(|grade| grade.student_id);
            (s.book.edit_grade(id, value, category), owner)
        };
        close_dialog(&window, &c.state);
        if let (true, Some(owner)) = (changed, owner) {
            save(&window, &c.state);
            refresh_student(&window, &c.state, owner);
            toast_change(&c, &window);
        }
    });

    let c = ctx.clone();
    backend.on_delete_edit_score(move || {
        let Some(window) = c.window() else { return };
        let owner = {
            let mut s = c.state.borrow_mut();
            let Dialog::EditScore(id) = &s.dialog else { return };
            let id = *id;
            let owner = s.book.grade(id).map(|grade| grade.student_id);
            s.book.delete_grade(id);
            owner
        };
        close_dialog(&window, &c.state);
        if let Some(owner) = owner {
            save(&window, &c.state);
            refresh_student(&window, &c.state, owner);
            toast_change(&c, &window);
        }
    });

    let c = ctx.clone();
    backend.on_save_edit_student(move |name, level| {
        let Some(window) = c.window() else { return };
        let changed = {
            let mut guard = c.state.borrow_mut();
            let s = &mut *guard;
            let Dialog::EditStudent(id) = &s.dialog else { return };
            let level = s.levels.get(index(level)).copied().unwrap_or(1);
            s.book.edit_student(*id, &name, level)
        };
        close_dialog(&window, &c.state);
        if changed {
            after_change(&c, &window);
            toast_change(&c, &window);
        }
    });

    let c = ctx.clone();
    backend.on_add_pasted(move |text, level| {
        let Some(window) = c.window() else { return };
        let added = {
            let mut s = c.state.borrow_mut();
            let level = s.levels.get(index(level)).copied().unwrap_or(1);
            s.book.add_students(&book::parse_names(&text), level).len()
        };
        close_dialog(&window, &c.state);
        if added > 0 {
            after_change(&c, &window);
            toast_change(&c, &window);
        }
    });

    let c = ctx.clone();
    backend.on_needed_changed(move || {
        if let Some(window) = c.window() {
            refresh_needed(&window, &c.state);
        }
    });

    let c = ctx.clone();
    backend.on_report_save_csv(move || {
        let Some(name) = report_card_of_open_dialog(&c.state).map(|card| card.name) else {
            return;
        };
        save_dialog(
            &c,
            "Save Report Card",
            format!("{}-report.csv", export::file_stem(&name)),
            ("CSV", &["csv"]),
            export_report,
        );
    });

    let c = ctx.clone();
    backend.on_report_open_printable(move || {
        if let Some(window) = c.window() {
            open_printable_report(&window, &c.state);
        }
    });

    let c = ctx.clone();
    backend.on_save_rename(move |text| {
        let Some(window) = c.window() else { return };
        let target = match &c.state.borrow().dialog {
            Dialog::Rename(target) => target.clone(),
            _ => return,
        };
        // When the category that new scores are added as is the one being renamed, it stays so.
        let mut adding_as_renamed: Option<GradeType> = None;
        let result = {
            let mut guard = c.state.borrow_mut();
            let s = &mut *guard;
            match &target {
                RenameTarget::Subject(old) => s.book.rename_subject(old, &text).map(|new| {
                    // What is on the screens follows the subject to its new name.
                    if s.filters.subject == *old {
                        s.filters.subject = new.clone();
                    }
                    if s.weight_scope.as_deref() == Some(old.as_str()) {
                        s.weight_scope = Some(new);
                    }
                }),
                RenameTarget::Category(category) => {
                    let adding_as = window.global::<Backend>().get_grade_type();
                    let was_adding_as = s.categories.get(index(adding_as)) == Some(category);
                    s.book.rename_category(category, &text).map(|new| {
                        if was_adding_as {
                            adding_as_renamed = Some(new);
                        }
                    })
                }
            }
        };
        match result {
            Ok(()) => {
                close_dialog(&window, &c.state);
                after_change(&c, &window);
                if let Some(renamed) = adding_as_renamed {
                    let position = c.state.borrow().categories.iter().position(|x| *x == renamed);
                    if let Some(position) = position {
                        window.global::<Backend>().set_grade_type(position as i32);
                    }
                }
                toast_change(&c, &window);
            }
            Err(e) => show_list_error(&window, "Couldn't Rename It", &e),
        }
    });
}

fn alerts(backend: &Backend<'_>, ctx: &Ctx) {
    let c = ctx.clone();
    backend.on_alert_confirmed(move || {
        let Some(window) = c.window() else { return };
        let pending = std::mem::replace(&mut c.state.borrow_mut().pending, Pending::Nothing);
        hide_alert(&window);
        match pending {
            Pending::Nothing => {}
            Pending::Restore(data) => restore(&c, &window, data),
            Pending::NewYear => start_new_year(&c, &window),
            Pending::RemoveSubject(subject) => remove_subject(&c, &window, &subject),
            Pending::RemoveCategory(category) => remove_category(&c, &window, &category),
        }
    });

    let c = ctx.clone();
    backend.on_alert_dismissed(move || {
        if let Some(window) = c.window() {
            c.state.borrow_mut().pending = Pending::Nothing;
            hide_alert(&window);
        }
    });
}

/// The light/dark button: switches, and remembers the choice.
fn appearance(backend: &Backend<'_>, ctx: &Ctx) {
    let c = ctx.clone();
    backend.on_toggle_appearance(move || {
        let Some(window) = c.window() else { return };
        let next = if window.global::<Theme>().get_dark() {
            Appearance::Light
        } else {
            Appearance::Dark
        };
        apply_appearance(&window, next);
        update_preferences(&c.state, |preferences| preferences.appearance = Some(next));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_letter_worth_asking_about_is_the_one_just_above() {
        let letter = |average| calc::LETTER_BANDS[default_target(average)].1;
        assert_eq!(letter(None), "A", "with nothing yet, the top");
        assert_eq!(letter(Some(85.0)), "B−");
        assert_eq!(letter(Some(83.9)), "C+", "83.9 shows as 83.9, a C");
        assert_eq!(letter(Some(95.0)), "A");
        assert_eq!(letter(Some(100.0)), "A+", "nothing is above the top");
        assert_eq!(letter(Some(120.0)), "A+");
        assert_eq!(letter(Some(40.0)), "E", "the lowest letter that can be asked for");
        // The displayed number counts: 93.96 is shown as 94.0, which is an A−.
        assert_eq!(letter(Some(93.96)), "A");
        assert_eq!(needed_targets().len(), calc::LETTER_BANDS.len());
        assert_eq!(needed_targets()[2], "A−");
    }

    #[test]
    fn the_answer_reads_as_a_sentence() {
        let test = GradeType::Test;
        assert_eq!(
            needed_message("Ada", "Math", &test, "B", Needed::Score(89.9)),
            "Ada needs at least 89.9 on the next Test to reach a B in Math."
        );
        assert_eq!(
            needed_message("Ada", "Math", &test, "A−", Needed::Score(100.0)),
            "Ada needs at least 100 on the next Test to reach an A− in Math."
        );
        assert!(needed_message("Ada", "Art", &GradeType::Quiz, "E", Needed::Already).contains("an E or better"));
        assert!(
            needed_message("Ada", "Math", &test, "A", Needed::OutOfReach { best: 90.04 })
                .starts_with("Even a 100 on the next Test only gets Ada to 90.0 B in Math.")
        );
        let project = GradeType::Custom("Project".into());
        assert_eq!(
            needed_message("Ada", "Math", &project, "C", Needed::NoEffect),
            "Project has no weight in Math, so a score there can't change the average."
        );
        assert_eq!(article("F"), "an");
        assert_eq!(article("B+"), "a");
        assert_eq!(
            (score_text(94.0), score_text(89.9), score_text(99.6)),
            ("94".into(), "89.9".into(), "99.6".into()),
            "a whole number loses its .0"
        );
    }

    #[test]
    fn a_report_is_written_where_only_its_owner_can_read_it_and_never_over_another_file() {
        let first = write_private_temp_file("GradeBook-test-report", "html", b"<p>one</p>").unwrap();
        let second = write_private_temp_file("GradeBook-test-report", "html", b"<p>two</p>").unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), b"<p>one</p>");
        assert_eq!(fs::read(&second).unwrap(), b"<p>two</p>");
        assert!(first.extension().is_some_and(|e| e == "html"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&first).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "not readable by other users of the computer");
        }
        let _ = fs::remove_file(first);
        let _ = fs::remove_file(second);
    }

    #[test]
    fn what_the_app_writes_as_a_report_is_what_the_cleanup_recognises() {
        let path = write_private_temp_file("GradeBook-Ada_Lovelace-report", "html", b"x").unwrap();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        assert!(is_a_printable_report(&name), "{name}");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn old_printable_reports_are_deleted_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let make = |name: &str, hours_old: u64| {
            let path = dir.path().join(name);
            let file = fs::File::create(&path).unwrap();
            file.set_modified(now - Duration::from_secs(hours_old * 3600)).unwrap();
            path
        };
        let old_report = make("GradeBook-Ada_Lovelace-report-0123abcd.html", 30);
        let new_report = make("GradeBook-Ada_Lovelace-report-4567EF01.html", 2);
        let not_ours = [
            make("GradeBook-Ada-report-0123abcd.txt", 30),
            make("GradeBook-Ada-report-xyz.html", 30),
            make("GradeBook-Ada-report-0123abcdef.html", 30),
            make("MyGradeBook-Ada-report-0123abcd.html", 30),
            make("GradeBook-notes.html", 30),
            make("gradebook.json", 30),
        ];
        let folder_like_a_report = dir.path().join("GradeBook-Dir-report-89abcdef.html");
        fs::create_dir(&folder_like_a_report).unwrap();

        remove_old_reports_in(dir.path(), REPORT_KEPT, now);
        assert!(!old_report.exists(), "a day old and more: gone");
        assert!(new_report.exists(), "made a moment ago: kept");
        for path in not_ours {
            assert!(path.exists(), "{path:?} isn't one of the app's reports");
        }
        assert!(folder_like_a_report.is_dir(), "only files are touched");

        // A folder that isn't there is nothing to clean.
        remove_old_reports_in(&dir.path().join("missing"), Duration::ZERO, now);
    }
}
