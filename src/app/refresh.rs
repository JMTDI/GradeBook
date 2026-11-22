//! Filling the screens from the state.

use super::*;

use std::collections::HashSet;
use std::hash::Hash;

use slint::{Model, ModelRc, SharedString};

use crate::calc;
use crate::ui::{
    AlertData, BarData, CategoryRow, ChipData, InsightsData, NameItem, StandingData, StudentItem, ToastData, TrendPoint,
};
use crate::view::{self, GradesView, RowView};

pub(super) fn strings(items: impl Iterator<Item = String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(items.map(SharedString::from).collect::<Vec<_>>()))
}

fn models<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

/// Brings `model` to `items`. An entry whose key is in both stays where it is and is only
/// updated, so the controls of a row that stays keep what was typed in them and the keyboard;
/// only what is gone is removed and what is new inserted. Replacing the whole model would
/// destroy the focused field, and the next Ctrl+Z would go nowhere.
///
/// Returns whether an entry was destroyed: a control that had the keyboard in it is gone then,
/// and the keyboard has to be given back (see `callbacks::after_history`).
fn sync<T, K>(model: &VecModel<T>, items: Vec<T>, key: impl Fn(&T) -> K) -> bool
where
    T: Clone + PartialEq + 'static,
    K: Eq + Hash,
{
    let mut destroyed = false;
    let wanted: HashSet<K> = items.iter().map(&key).collect();
    let mut position = 0;
    while let Some(row) = model.row_data(position) {
        if wanted.contains(&key(&row)) {
            position += 1;
        } else {
            model.remove(position);
            destroyed = true;
        }
    }
    let count = items.len();
    for (position, item) in items.into_iter().enumerate() {
        match model.row_data(position) {
            Some(current) if key(&current) == key(&item) => {
                if current != item {
                    model.set_row_data(position, item);
                }
            }
            _ => model.insert(position, item),
        }
    }
    while model.row_count() > count {
        model.remove(count);
        destroyed = true;
    }
    destroyed
}

/// What the legend above the scores calls a category.
fn legend_label(category: &GradeType) -> String {
    match category {
        GradeType::Homework => "Homework".to_string(),
        GradeType::Test => "Tests".to_string(),
        GradeType::Quiz => "Quizzes".to_string(),
        GradeType::Custom(name) => name.clone(),
    }
}

fn to_row(row: &RowView, draft: &str) -> StudentRowData {
    let chips: Vec<ChipData> = row
        .chips
        .iter()
        .map(|chip| ChipData {
            id: chip.id.to_string().into(),
            value: chip.value.as_str().into(),
            letter: chip.letter.into(),
            kind: chip.kind as i32,
            dropped: chip.dropped,
        })
        .collect();
    StudentRowData {
        id: row.id.to_string().into(),
        name: row.name.as_str().into(),
        draft: draft.into(),
        chips: models(chips),
        average: row.average.clone().unwrap_or_default().into(),
    }
}

/// Rebuilds the Grades screen (rows, chips, averages). A student who stays on the screen
/// keeps their row, and with it the field that may have the keyboard.
pub(super) fn refresh_grades(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    let GradesView { rows, class_average } = view::grades_view(&s.book, &s.filters);
    let data: Vec<StudentRowData> = rows
        .iter()
        .map(|row| to_row(row, s.drafts.get(&row.id).map(String::as_str).unwrap_or("")))
        .collect();
    if sync(&s.rows, data, |row| row.id.clone()) {
        s.entry_removed.set(true);
    }
    let backend = window.global::<Backend>();
    backend.set_rows(ModelRc::from(s.rows.clone()));
    backend.set_class_average(class_average.unwrap_or_default().into());
}

/// Updates one student's row in place (so other rows keep what was typed in them), and
/// everything that depends on it.
pub(super) fn refresh_student(window: &MainWindow, state: &Rc<RefCell<State>>, student_id: Uuid) {
    {
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
    refresh_insights(window, state);
    refresh_name_lists(window, state);
    refresh_undo(window, state);
}

pub(super) fn refresh_students(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    let items: Vec<StudentItem> = s
        .book
        .students()
        .iter()
        .map(|student| StudentItem {
            id: student.id.to_string().into(),
            name: student.name.as_str().into(),
            grade: calc::grade_level_label(student.grade_level).into(),
        })
        .collect();
    if sync(&s.student_items, items, |item| item.id.clone()) {
        s.entry_removed.set(true);
    }
    window
        .global::<Backend>()
        .set_students(ModelRc::from(s.student_items.clone()));
}

/// The new position of the entry that was at `index` in `old`, if it is still in `new`.
fn follow<T: PartialEq>(old: &[T], new: &[T], index: i32) -> Option<usize> {
    let before = old.get(usize::try_from(index).ok()?)?;
    new.iter().position(|item| item == before)
}

/// Brings the pickers and the lists of subjects, categories and grade levels up to date after
/// any of them changed. What was picked stays picked if it still exists.
pub(super) fn sync_lists(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let mut guard = state.borrow_mut();
    let s = &mut *guard;
    let backend = window.global::<Backend>();

    // Grade levels.
    let old_levels = std::mem::replace(&mut s.levels, s.book.grade_levels());
    if !s.levels.contains(&s.filters.grade_level) {
        s.filters.grade_level = if s.levels.contains(&1) { 1 } else { s.levels[0] };
    }
    let current_level = s.levels.iter().position(|l| *l == s.filters.grade_level).unwrap_or(0);
    backend.set_grade_level_options(strings(s.levels.iter().map(|l| calc::grade_level_label(*l))));
    backend.set_grade_level_index(current_level as i32);
    let new_student = follow(&old_levels, &s.levels, backend.get_new_student_level()).unwrap_or(current_level);
    backend.set_new_student_level(new_student as i32);

    // Subjects.
    s.subjects = s.book.subjects();
    if !s.subjects.contains(&s.filters.subject) {
        s.filters.subject = calc::DEFAULT_SUBJECT.to_string();
    }
    let current_subject = s.subjects.iter().position(|x| *x == s.filters.subject).unwrap_or(0);
    backend.set_subject_options(strings(s.subjects.iter().cloned()));
    backend.set_subject_index(current_subject as i32);

    // Categories. The kind that new scores are added as follows its category to wherever it is
    // in the list now, and goes back to Test when the category is gone: a position alone would
    // point at some other category, and every new score would silently go into that one.
    let old_categories = std::mem::replace(&mut s.categories, s.book.categories());
    backend.set_category_names(strings(s.categories.iter().map(|c| c.label().to_string())));
    backend.set_legend(strings(s.categories.iter().map(legend_label)));
    let test = s.categories.iter().position(|c| *c == GradeType::Test).unwrap_or(0);
    let adding_as = follow(&old_categories, &s.categories, backend.get_grade_type()).unwrap_or(test);
    backend.set_grade_type(adding_as as i32);

    // The subject the weights are for.
    if s.weight_scope.as_ref().is_some_and(|scope| !s.subjects.contains(scope)) {
        s.weight_scope = None;
    }
    backend.set_weight_scope_options(strings(
        std::iter::once("All subjects".to_string()).chain(s.subjects.iter().cloned()),
    ));
    let scope_index = s
        .weight_scope
        .as_ref()
        .and_then(|scope| s.subjects.iter().position(|x| x == scope))
        .map_or(0, |position| position + 1);
    backend.set_weight_scope_index(scope_index as i32);

    // The grade levels that can still be added.
    s.addable_levels = std::iter::once(calc::KINDERGARTEN)
        .chain(11..=20)
        .filter(|level| !s.levels.contains(level))
        .collect();
    backend.set_addable_level_options(strings(s.addable_levels.iter().map(|l| calc::grade_level_label(*l))));
    if backend.get_addable_level_index() < 0 || backend.get_addable_level_index() as usize >= s.addable_levels.len() {
        backend.set_addable_level_index(0);
    }

    set_name_lists(&backend, s);
}

/// The lists of what was added, each entry with how much it is used. They are updated in
/// place, by name, so that the buttons of an entry that stays keep the keyboard.
fn set_name_lists(backend: &Backend<'_>, s: &State) {
    let custom_categories: Vec<NameItem> = s
        .categories
        .iter()
        .filter(|c| !c.is_built_in())
        .map(|c| NameItem {
            name: c.label().into(),
            detail: format!(
                "{} · {}",
                scores(s.book.count_grades_in_category(c)),
                weight_text(s.book.weight(c))
            )
            .into(),
        })
        .collect();
    let custom_subjects: Vec<NameItem> = s
        .book
        .custom_subjects()
        .into_iter()
        .map(|name| NameItem {
            detail: scores(s.book.count_grades_in_subject(&name)).into(),
            name: name.into(),
        })
        .collect();
    let extra_levels: Vec<NameItem> = s
        .book
        .extra_levels()
        .into_iter()
        .map(|level| {
            let students = s.book.students_in_level(level).count();
            NameItem {
                name: calc::grade_level_label(level).into(),
                detail: match students {
                    1 => "1 student".to_string(),
                    n => format!("{n} students"),
                }
                .into(),
            }
        })
        .collect();
    let mut destroyed = false;
    for (model, items) in [
        (&s.category_items, custom_categories),
        (&s.subject_items, custom_subjects),
        (&s.level_items, extra_levels),
    ] {
        destroyed |= sync(model, items, |item| item.name.clone());
    }
    if destroyed {
        s.entry_removed.set(true);
    }
    backend.set_custom_categories(ModelRc::from(s.category_items.clone()));
    backend.set_custom_subjects(ModelRc::from(s.subject_items.clone()));
    backend.set_extra_levels(ModelRc::from(s.level_items.clone()));
}

/// Brings those details up to date after scores or weights changed, which leaves the lists
/// themselves as they are.
pub(super) fn refresh_name_lists(window: &MainWindow, state: &Rc<RefCell<State>>) {
    set_name_lists(&window.global::<Backend>(), &state.borrow());
}

fn scores(count: usize) -> String {
    match count {
        1 => "1 score".to_string(),
        n => format!("{n} scores"),
    }
}

fn weight_text(weight: f64) -> String {
    format!("weight {}%", weight.round())
}

/// Rebuilds the weight sliders and the notes under them. Not while one is being dragged:
/// that would take the slider from under the pointer (see `refresh_weight_status`).
pub(super) fn refresh_weights(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    let scope = s.weight_scope.as_deref();
    let rows: Vec<CategoryRow> = s
        .categories
        .iter()
        .enumerate()
        .map(|(position, category)| CategoryRow {
            name: category.label().into(),
            kind: position as i32,
            weight: s.book.weight_in(scope, category) as f32,
            drop_lowest: s.book.drops_lowest(category),
        })
        .collect();
    window.global::<Backend>().set_weight_rows(models(rows));
    drop(s);
    refresh_weight_status(window, state);
}

/// The total and the notes under the weight sliders.
pub(super) fn refresh_weight_status(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    let scope = s.weight_scope.as_deref();
    let backend = window.global::<Backend>();
    backend.set_weights_total(s.book.total_weight(scope) as f32);
    let own = scope.is_some_and(|subject| s.book.has_own_weights(subject));
    backend.set_weight_scope_own(own);
    let note = match scope {
        None => "Every subject uses these weights, unless you give it weights of its own below.".to_string(),
        Some(subject) if own => format!("{subject} has weights of its own. Changing these doesn't change the others."),
        Some(subject) => format!("{subject} uses the weights for all subjects until you change one here."),
    };
    backend.set_weight_scope_note(note.into());
}

pub(super) fn refresh_insights(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    let insights = view::insights(&s.book, &s.filters, f64::from(s.attention_below));
    let level = calc::grade_level_label(s.filters.grade_level);
    let summary = if insights.students == 0 {
        format!("There are no students in {level} yet.")
    } else if insights.counted == 0 {
        format!(
            "None of the {} students in {level} has a score in {}, Period {} yet.",
            insights.students, s.filters.subject, s.filters.period
        )
    } else {
        format!(
            "{} of {} students in {level} have an average in {}, Period {}.",
            insights.counted, insights.students, s.filters.subject, s.filters.period
        )
    };
    let standing = |standing: &Option<view::Standing>| match standing {
        Some(standing) => (calc::format_average(standing.average), standing.name.clone()),
        None => (String::new(), String::new()),
    };
    let (highest, highest_name) = standing(&insights.highest);
    let (lowest, lowest_name) = standing(&insights.lowest);

    let longest = insights.distribution.iter().copied().max().unwrap_or(0).max(1) as f32;
    let bars: Vec<BarData> = view::LETTER_FAMILIES
        .iter()
        .zip(insights.distribution)
        .map(|(label, count)| BarData {
            label: (*label).into(),
            count: count as i32,
            share: count as f32 / longest,
        })
        .collect();
    let attention: Vec<StandingData> = insights
        .attention
        .iter()
        .map(|standing| StandingData {
            name: standing.name.as_str().into(),
            average: calc::format_average(standing.average).into(),
        })
        .collect();
    let points: Vec<TrendPoint> = insights
        .trend
        .iter()
        .enumerate()
        .map(|(position, average)| TrendPoint {
            label: format!("P{}", position + 1).into(),
            text: average.map(calc::format_average).unwrap_or_default().into(),
            x: ((position as f64 + 0.5) / insights.trend.len() as f64) as f32,
            y: (1.0 - average.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0) as f32,
            has_value: average.is_some(),
            current: position as u32 + calc::PERIODS.start() == s.filters.period,
        })
        .collect();

    window.global::<Backend>().set_insights(InsightsData {
        summary: summary.into(),
        class_average: insights
            .class_average
            .map(calc::format_average)
            .unwrap_or_default()
            .into(),
        highest: highest.into(),
        highest_name: highest_name.into(),
        lowest: lowest.into(),
        lowest_name: lowest_name.into(),
        median: insights.median.map(calc::format_average).unwrap_or_default().into(),
        bars: models(bars),
        attention: models(attention),
        trend: models(points),
    });
}

/// Where the data is, and what has been backed up.
pub(super) fn refresh_data_info(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    let backend = window.global::<Backend>();
    match &s.store {
        Some(store) => {
            backend.set_data_folder(format!("Your data is saved in {}", store.dir().display()).into());
            let backups = store.daily_backups();
            let text = match backups.first() {
                None => "Automatic backups: the first start of each day keeps a copy of your data for a week. \
                         There isn't one yet."
                    .to_string(),
                Some((latest, _)) => format!(
                    "Automatic backups: the first start of each day keeps a copy of your data for a week. \
                     {} {} kept, the latest from {}.",
                    backups.len(),
                    if backups.len() == 1 { "copy is" } else { "copies are" },
                    latest.format("%-d %B %Y")
                ),
            };
            backend.set_backups_summary(text.into());
        }
        None => {
            backend.set_data_folder("Nothing is being saved: GradeBook found no folder to keep your data in.".into());
            backend.set_backups_summary("".into());
        }
    }
}

/// Whether there is anything to undo or redo, and what. It also takes the bar away that offers
/// to undo or redo a step, once the history has moved on from it.
pub(super) fn refresh_undo(window: &MainWindow, state: &Rc<RefCell<State>>) {
    let stale_toast = {
        let s = state.borrow();
        let backend = window.global::<Backend>();
        backend.set_can_undo(s.book.can_undo());
        backend.set_can_redo(s.book.can_redo());
        backend.set_undo_text(s.book.undo_label().unwrap_or_default().into());
        backend.set_redo_text(s.book.redo_label().unwrap_or_default().into());
        s.toast_version.is_some_and(|version| version != s.book.version())
    };
    if stale_toast {
        hide_toast(window);
        state.borrow_mut().toast_version = None;
    }
}

/// Everything, after a change that could have touched any of it.
pub(super) fn refresh_all(window: &MainWindow, state: &Rc<RefCell<State>>) {
    sync_lists(window, state);
    refresh_students(window, state);
    refresh_grades(window, state);
    refresh_weights(window, state);
    refresh_insights(window, state);
    refresh_data_info(window, state);
    refresh_undo(window, state);
}

pub(super) fn save(window: &MainWindow, state: &Rc<RefCell<State>>) {
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

pub(super) fn show_alert(window: &MainWindow, title: &str, message: &str, confirm_label: &str, destructive: bool) {
    window.global::<Backend>().set_alert(AlertData {
        visible: true,
        title: title.into(),
        message: message.into(),
        confirm_label: confirm_label.into(),
        destructive,
    });
}

pub(super) fn hide_alert(window: &MainWindow) {
    let backend = window.global::<Backend>();
    let mut alert = backend.get_alert();
    alert.visible = false;
    backend.set_alert(alert);
}

/// Shows the bar with one button at the bottom, which goes away by itself after a while. The
/// bar offers an undo or a redo, which is only good for the state of the history it was made in.
pub(super) fn show_toast(ctx: &Ctx, window: &MainWindow, message: &str, action: ToastAction) {
    {
        let mut s = ctx.state.borrow_mut();
        s.toast_action = action;
        s.toast_version = Some(s.book.version());
    }
    window.global::<Backend>().set_toast(ToastData {
        visible: true,
        message: message.into(),
        action: action.label().into(),
    });
    let weak = ctx.weak.clone();
    ctx.toast_timer.start(TimerMode::SingleShot, TOAST_DURATION, move || {
        if let Some(window) = weak.upgrade() {
            hide_toast(&window);
        }
    });
}

pub(super) fn hide_toast(window: &MainWindow) {
    let backend = window.global::<Backend>();
    let mut toast = backend.get_toast();
    toast.visible = false;
    backend.set_toast(toast);
}

/// Whether a dialog or an alert is in the way of changing the data from the keyboard.
pub(super) fn modal_open(window: &MainWindow) -> bool {
    let backend = window.global::<Backend>();
    backend.get_dialog() != 0 || backend.get_alert().visible
}

pub(super) fn open_dialog(window: &MainWindow, state: &Rc<RefCell<State>>, dialog: Dialog) {
    window.global::<Backend>().set_dialog(dialog.number());
    state.borrow_mut().dialog = dialog;
}

pub(super) fn close_dialog(window: &MainWindow, state: &Rc<RefCell<State>>) {
    window.global::<Backend>().set_dialog(0);
    state.borrow_mut().dialog = Dialog::None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_brought_to_the_new_list_by_key() {
        type Item = (i32, &'static str);
        let key = |item: &Item| item.0;
        let content = |model: &VecModel<Item>| model.iter().collect::<Vec<_>>();
        let model = VecModel::from(vec![(1, "a"), (2, "b"), (3, "c")]);

        sync(&model, vec![(1, "a"), (2, "B"), (3, "c")], key);
        assert_eq!(content(&model), [(1, "a"), (2, "B"), (3, "c")], "updated in place");

        sync(&model, vec![(1, "a"), (3, "c")], key);
        assert_eq!(content(&model), [(1, "a"), (3, "c")], "one in the middle is gone");

        sync(&model, vec![(0, "z"), (1, "a"), (2, "b"), (3, "c"), (4, "d")], key);
        assert_eq!(
            content(&model),
            [(0, "z"), (1, "a"), (2, "b"), (3, "c"), (4, "d")],
            "new ones at the front, in the middle and at the end"
        );

        sync(&model, vec![(4, "d"), (3, "c")], key);
        assert_eq!(
            content(&model),
            [(4, "d"), (3, "c")],
            "a different order, and the rest gone"
        );

        sync(&model, vec![(3, "c"), (4, "d"), (5, "e")], key);
        assert_eq!(content(&model), [(3, "c"), (4, "d"), (5, "e")]);

        sync(&model, Vec::new(), key);
        assert_eq!(model.row_count(), 0);
        sync(&model, vec![(1, "a")], key);
        assert_eq!(content(&model), [(1, "a")]);
    }

    #[test]
    fn a_picked_entry_is_followed_when_the_list_changes() {
        assert_eq!(follow(&[1, 2, 3], &[0, 1, 2, 3], 1), Some(2));
        assert_eq!(follow(&[1, 2, 3], &[1, 3], 1), None, "it is gone");
        assert_eq!(follow(&[1, 2, 3], &[1, 2, 3], 7), None);
        assert_eq!(follow(&[1, 2, 3], &[1, 2, 3], -1), None);
        assert_eq!(follow::<u32>(&[], &[1], 0), None);
    }

    #[test]
    fn counts_and_weights_read_naturally() {
        assert_eq!(scores(0), "0 scores");
        assert_eq!(scores(1), "1 score");
        assert_eq!(scores(12), "12 scores");
        assert_eq!(weight_text(17.0), "weight 17%");
        assert_eq!(weight_text(33.4), "weight 33%");
    }

    #[test]
    fn the_legend_names_the_categories_the_way_the_screen_always_has() {
        let labels: Vec<String> = GradeType::ALL.iter().map(legend_label).collect();
        assert_eq!(labels, ["Homework", "Tests", "Quizzes"]);
        assert_eq!(legend_label(&GradeType::Custom("Project".into())), "Project");
    }
}
