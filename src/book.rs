//! The in-memory gradebook and the operations the UI performs on it, without the
//! persistence (see `store`).
//!
//! Every change to the data is a [`Change`] that, when applied, returns the change that
//! takes it back. That is all undo and redo need: the undo history is a list of such
//! inverses, and applying one produces the inverse for the redo list.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use uuid::Uuid;

use crate::calc::{self, Needed, Rule, Rules};
use crate::model::{
    is_reserved_category_name, same_name, BackupData, CustomCategory, GradeEntry, GradeType, Settings, Student,
};

/// The undo history keeps at most this many steps...
const MAX_UNDO_STEPS: usize = 100;
/// ...and stops before it holds on to more than this many students and grades.
const MAX_UNDO_SIZE: usize = 100_000;

const MAX_NAME_LEN: usize = 40;
const MAX_CUSTOM_SUBJECTS: usize = 100;
const MAX_CUSTOM_CATEGORIES: usize = 12;
const MAX_STUDENT_NAME_LEN: usize = 80;

/// A change to the gradebook's data. Applying one makes the change and returns the one that
/// undoes it.
#[derive(Debug, Clone)]
enum Change {
    /// Put grades in at these positions (ascending), as they were before they were removed.
    InsertGrades(Vec<(usize, GradeEntry)>),
    RemoveGrades(Vec<Uuid>),
    /// Put students back with the grades they had.
    InsertStudents {
        students: Vec<(usize, Student)>,
        grades: Vec<(usize, GradeEntry)>,
    },
    /// Remove students together with their grades.
    RemoveStudents(Vec<Uuid>),
    ReplaceGrade(GradeEntry),
    ReplaceStudent(Student),
    ReplaceSettings(Box<Settings>),
    ReplaceAll(Box<BackupData>),
    /// Several changes that are one step for the user.
    Group(Vec<Change>),
}

/// Takes the grades `wanted` accepts out of `data`, with the position each one had.
fn take_grades(data: &mut BackupData, wanted: impl Fn(&GradeEntry) -> bool) -> Vec<(usize, GradeEntry)> {
    let mut removed = Vec::new();
    let mut kept = Vec::with_capacity(data.grades.len());
    for (position, grade) in std::mem::take(&mut data.grades).into_iter().enumerate() {
        if wanted(&grade) {
            removed.push((position, grade));
        } else {
            kept.push(grade);
        }
    }
    data.grades = kept;
    removed
}

impl Change {
    fn apply(self, data: &mut BackupData) -> Change {
        match self {
            Change::InsertGrades(items) => {
                let ids = items.iter().map(|(_, grade)| grade.id).collect();
                for (position, grade) in items {
                    data.grades.insert(position.min(data.grades.len()), grade);
                }
                Change::RemoveGrades(ids)
            }
            Change::RemoveGrades(ids) => {
                let ids: HashSet<Uuid> = ids.into_iter().collect();
                Change::InsertGrades(take_grades(data, |grade| ids.contains(&grade.id)))
            }
            Change::InsertStudents { students, grades } => {
                let ids = students.iter().map(|(_, student)| student.id).collect();
                for (position, student) in students {
                    data.students.insert(position.min(data.students.len()), student);
                }
                for (position, grade) in grades {
                    data.grades.insert(position.min(data.grades.len()), grade);
                }
                Change::RemoveStudents(ids)
            }
            Change::RemoveStudents(ids) => {
                let ids: HashSet<Uuid> = ids.into_iter().collect();
                let mut students = Vec::new();
                let mut kept = Vec::with_capacity(data.students.len());
                for (position, student) in std::mem::take(&mut data.students).into_iter().enumerate() {
                    if ids.contains(&student.id) {
                        students.push((position, student));
                    } else {
                        kept.push(student);
                    }
                }
                data.students = kept;
                let grades = take_grades(data, |grade| ids.contains(&grade.student_id));
                Change::InsertStudents { students, grades }
            }
            Change::ReplaceGrade(new) => match data.grades.iter_mut().find(|grade| grade.id == new.id) {
                Some(slot) => Change::ReplaceGrade(std::mem::replace(slot, new)),
                None => Change::Group(Vec::new()),
            },
            Change::ReplaceStudent(new) => match data.students.iter_mut().find(|student| student.id == new.id) {
                Some(slot) => Change::ReplaceStudent(std::mem::replace(slot, new)),
                None => Change::Group(Vec::new()),
            },
            Change::ReplaceSettings(settings) => {
                let before = data.settings();
                data.apply_settings(*settings);
                Change::ReplaceSettings(Box::new(before))
            }
            Change::ReplaceAll(all) => Change::ReplaceAll(Box::new(std::mem::replace(data, *all))),
            Change::Group(changes) => {
                let mut inverses: Vec<Change> = changes.into_iter().map(|change| change.apply(data)).collect();
                inverses.reverse();
                Change::Group(inverses)
            }
        }
    }

    /// How many students and grades the change holds on to: what keeping it costs.
    fn size(&self) -> usize {
        match self {
            Change::InsertGrades(items) => items.len(),
            Change::RemoveGrades(ids) | Change::RemoveStudents(ids) => ids.len(),
            Change::InsertStudents { students, grades } => students.len() + grades.len(),
            Change::ReplaceGrade(_) | Change::ReplaceStudent(_) | Change::ReplaceSettings(_) => 1,
            Change::ReplaceAll(all) => all.students.len() + all.grades.len() + 1,
            Change::Group(changes) => changes.iter().map(Change::size).sum::<usize>() + 1,
        }
    }
}

/// One step of the undo or redo history.
#[derive(Debug, Clone)]
struct Step {
    label: String,
    /// What undoes (or, in the redo list, redoes) the step.
    change: Change,
    /// Steps with the same key merge into one, so a slider dragged through many values is
    /// a single step.
    merge: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GradeBook {
    data: BackupData,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Counted up by every change, undo and redo. It never goes back, even when the history
    /// is cut short, so "nothing has changed since" can be told from it.
    version: u64,
}

/// Why a subject, category or grade level couldn't be added, renamed or removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListError {
    Blank,
    TooLong,
    /// The name is in use already (a category also can't be called Homework, Test or Quiz).
    Taken,
    TooMany,
    BuiltIn,
    NotFound,
    /// Only Kindergarten and grades 11 to 20 can be added.
    OutOfRange,
    /// This many students are still in the grade level.
    InUse(usize),
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListError::Blank => f.write_str("Type a name first."),
            ListError::TooLong => write!(f, "Names can have up to {MAX_NAME_LEN} letters."),
            ListError::Taken => f.write_str("That name is already in use."),
            ListError::TooMany => f.write_str("There are too many of these already."),
            ListError::BuiltIn => f.write_str("The built-in ones can't be changed."),
            ListError::NotFound => f.write_str("That isn't in the list any more."),
            ListError::OutOfRange => f.write_str("Only Kindergarten and grades 11 to 20 can be added."),
            ListError::InUse(1) => f.write_str("A student is still in that grade. Move or delete them first."),
            ListError::InUse(n) => write!(f, "{n} students are still in that grade. Move or delete them first."),
        }
    }
}

impl std::error::Error for ListError {}

/// What starting a new school year does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YearPlan {
    /// Students who move up a grade.
    pub moving: usize,
    /// Students already in the highest grade, who stay where they are.
    pub staying: usize,
    /// Scores that are cleared.
    pub scores: usize,
}

/// A name for a subject or category: spaces tidied, no control characters.
fn clean_name(name: &str) -> Result<String, ListError> {
    let name = name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        Err(ListError::Blank)
    } else if name.chars().count() > MAX_NAME_LEN {
        Err(ListError::TooLong)
    } else {
        Ok(name)
    }
}

fn clamp_weight(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

fn sanitise(data: &mut BackupData) {
    for grade_type in data.category_list() {
        let clean = clamp_weight(data.weight(&grade_type));
        data.set_weight(&grade_type, clean);
    }
    for weights in data.options.subject_weights.values_mut() {
        weights.values_mut().for_each(|weight| *weight = clamp_weight(*weight));
    }
}

/// The names in a pasted list: one per line, without list numbers or bullets. Cells
/// copied from a spreadsheet are joined with a space ("Ada" and "Lovelace").
pub fn parse_names(text: &str) -> Vec<String> {
    fn strip_marker(line: &str) -> &str {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix(['-', '*', '•', '–']) {
            return rest.trim_start();
        }
        let digits = line.bytes().take_while(u8::is_ascii_digit).count();
        if (1..=3).contains(&digits) {
            if let Some(rest) = line[digits..].strip_prefix(['.', ')']) {
                return rest.trim_start();
            }
        }
        line
    }

    text.lines()
        .filter_map(|line| {
            let joined = line.split('\t').map(str::trim).collect::<Vec<_>>().join(" ");
            let name = strip_marker(&joined)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(MAX_STUDENT_NAME_LEN)
                .collect::<String>();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

impl GradeBook {
    pub fn new(mut data: BackupData) -> Self {
        sanitise(&mut data);
        GradeBook {
            data,
            ..GradeBook::default()
        }
    }

    pub fn data(&self) -> &BackupData {
        &self.data
    }

    /// Replaces everything, as Restore does.
    pub fn replace(&mut self, mut data: BackupData) {
        sanitise(&mut data);
        self.commit("Backup restored", Change::ReplaceAll(Box::new(data)));
    }

    // ---- Undo and redo ---------------------------------------------------------------

    fn commit(&mut self, label: impl Into<String>, change: Change) {
        self.commit_merging(label, change, None);
    }

    fn commit_merging(&mut self, label: impl Into<String>, change: Change, merge: Option<String>) {
        let inverse = change.apply(&mut self.data);
        self.version += 1;
        self.redo.clear();
        if merge.is_some() && self.undo.last().is_some_and(|top| top.merge == merge) {
            // The step before already takes everything back to where it started.
            return;
        }
        self.undo.push(Step {
            label: label.into(),
            change: inverse,
            merge,
        });
        self.trim_history();
    }

    fn trim_history(&mut self) {
        if self.undo.len() > MAX_UNDO_STEPS {
            let excess = self.undo.len() - MAX_UNDO_STEPS;
            self.undo.drain(..excess);
        }
        let mut size: usize = self.undo.iter().map(|step| step.change.size()).sum();
        while size > MAX_UNDO_SIZE && self.undo.len() > 1 {
            size -= self.undo.remove(0).change.size();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// What undoing would take back, such as "Score deleted".
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|step| step.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|step| step.label.as_str())
    }

    /// Takes back the latest change. Returns what it was, or `None` if there was nothing.
    pub fn undo(&mut self) -> Option<String> {
        let step = self.undo.pop()?;
        let change = step.change.apply(&mut self.data);
        self.version += 1;
        self.redo.push(Step {
            label: step.label.clone(),
            change,
            merge: None,
        });
        Some(step.label)
    }

    pub fn redo(&mut self) -> Option<String> {
        let step = self.redo.pop()?;
        let change = step.change.apply(&mut self.data);
        self.version += 1;
        self.undo.push(Step {
            label: step.label.clone(),
            change,
            merge: None,
        });
        self.trim_history();
        Some(step.label)
    }

    /// Which state of the history this is: different after every change, undo and redo. A
    /// bar that offers to undo or redo a step is only good while this is what it was when
    /// the bar was shown.
    pub fn version(&self) -> u64 {
        self.version
    }

    // ---- What is in the gradebook ------------------------------------------------------

    pub fn students(&self) -> &[Student] {
        &self.data.students
    }

    pub fn student(&self, id: Uuid) -> Option<&Student> {
        self.data.students.iter().find(|s| s.id == id)
    }

    pub fn grade(&self, id: Uuid) -> Option<&GradeEntry> {
        self.data.grades.iter().find(|g| g.id == id)
    }

    /// Every category, in the order they are listed: Homework, Test, Quiz, then the custom ones.
    pub fn categories(&self) -> Vec<GradeType> {
        self.data.category_list()
    }

    /// The built-in subjects, the ones added and any a grade uses, in alphabetical order.
    pub fn subjects(&self) -> Vec<String> {
        let mut subjects: Vec<String> = calc::SUBJECTS.iter().map(|s| s.to_string()).collect();
        let used = self.data.grades.iter().map(|g| &g.subject);
        for subject in self.data.subjects.iter().chain(used) {
            if !subjects.contains(subject) {
                subjects.push(subject.clone());
            }
        }
        subjects.sort_by_key(|s| s.to_lowercase());
        subjects
    }

    /// The subjects that aren't built in.
    pub fn custom_subjects(&self) -> Vec<String> {
        self.subjects()
            .into_iter()
            .filter(|s| !calc::SUBJECTS.contains(&s.as_str()))
            .collect()
    }

    /// Grades 1 to 10, the levels added and any a student is in, lowest first.
    pub fn grade_levels(&self) -> Vec<u32> {
        let mut levels: Vec<u32> = calc::GRADE_LEVELS.collect();
        let used = self.data.students.iter().map(|s| s.grade_level);
        levels.extend(self.data.grade_levels.iter().copied().chain(used));
        levels.sort_unstable();
        levels.dedup();
        levels
    }

    /// The grade levels that aren't 1 to 10.
    pub fn extra_levels(&self) -> Vec<u32> {
        self.grade_levels()
            .into_iter()
            .filter(|level| !calc::GRADE_LEVELS.contains(level))
            .collect()
    }

    pub fn students_in_level(&self, level: u32) -> impl Iterator<Item = &Student> {
        self.data.students.iter().filter(move |s| s.grade_level == level)
    }

    pub fn grades_for<'a>(
        &'a self,
        student_id: Uuid,
        subject: &'a str,
        period: u32,
    ) -> impl Iterator<Item = &'a GradeEntry> {
        self.data
            .grades
            .iter()
            .filter(move |g| g.student_id == student_id && g.subject == subject && g.period == period)
    }

    pub fn count_grades_in_subject(&self, subject: &str) -> usize {
        self.data.grades.iter().filter(|g| g.subject == subject).count()
    }

    pub fn count_grades_in_category(&self, category: &GradeType) -> usize {
        self.data.grades.iter().filter(|g| g.grade_type == *category).count()
    }

    // ---- Averages --------------------------------------------------------------------

    /// How the scores of `subject` are combined.
    pub fn rules_for(&self, subject: &str) -> Rules {
        Rules(
            self.categories()
                .into_iter()
                .map(|category| Rule {
                    weight: self.weight_in(Some(subject), &category),
                    drop_lowest: self.data.options.drop_lowest.contains(&category),
                    category,
                })
                .collect(),
        )
    }

    pub fn student_average(&self, student_id: Uuid, subject: &str, period: u32) -> Option<f64> {
        calc::percent_average(self.grades_for(student_id, subject, period), &self.rules_for(subject))
    }

    /// Mean of the averages of the students in `level` who have a usable average.
    pub fn class_average(&self, level: u32, subject: &str, period: u32) -> Option<f64> {
        let rules = self.rules_for(subject);
        calc::mean(
            self.students_in_level(level)
                .filter_map(|s| calc::percent_average(self.grades_for(s.id, subject, period), &rules)),
        )
    }

    /// The scores the rules leave out of this student's average (the lowest of a category).
    pub fn dropped(&self, student_id: Uuid, subject: &str, period: u32) -> HashSet<Uuid> {
        calc::dropped_scores(self.grades_for(student_id, subject, period), &self.rules_for(subject))
    }

    /// What this student needs on the next score in `category` to reach `target`.
    pub fn needed(&self, student_id: Uuid, subject: &str, period: u32, category: &GradeType, target: f64) -> Needed {
        let grades: Vec<GradeEntry> = self.grades_for(student_id, subject, period).cloned().collect();
        calc::score_needed(&grades, &self.rules_for(subject), category, target)
    }

    // ---- Weights and grading options -----------------------------------------------------

    /// The default weight of a category.
    pub fn weight(&self, category: &GradeType) -> f64 {
        self.data.weight(category)
    }

    /// The weight a category has in `subject`: the subject's own if it has any, else the
    /// default. `None` is the default.
    pub fn weight_in(&self, subject: Option<&str>, category: &GradeType) -> f64 {
        subject
            .and_then(|subject| self.data.options.subject_weights.get(subject))
            .and_then(|weights| weights.get(category.label()))
            .copied()
            .unwrap_or_else(|| self.data.weight(category))
    }

    pub fn has_own_weights(&self, subject: &str) -> bool {
        self.data.options.subject_weights.contains_key(subject)
    }

    pub fn total_weight(&self, subject: Option<&str>) -> f64 {
        self.categories().iter().map(|c| self.weight_in(subject, c)).sum()
    }

    /// Sets the weight of a category, by default or just for one subject.
    pub fn set_weight(&mut self, subject: Option<&str>, category: &GradeType, value: f64) {
        if !self.categories().contains(category) {
            return;
        }
        let value = clamp_weight(value);
        let key = format!("weights:{}", subject.unwrap_or(""));
        self.change_settings("Weights changed", Some(key), |settings| match subject {
            None => settings.set_weight(category, value),
            Some(subject) => {
                let defaults: BTreeMap<String, f64> = settings
                    .category_list()
                    .iter()
                    .map(|c| (c.label().to_string(), settings.weight(c)))
                    .collect();
                settings
                    .options
                    .subject_weights
                    .entry(subject.to_string())
                    .or_insert(defaults)
                    .insert(category.label().to_string(), value);
            }
        });
    }

    /// Makes `subject` use the default weights again.
    pub fn reset_subject_weights(&mut self, subject: &str) {
        self.change_settings("Default weights restored", None, |settings| {
            settings.options.subject_weights.remove(subject);
        });
    }

    pub fn drops_lowest(&self, category: &GradeType) -> bool {
        self.data.options.drop_lowest.contains(category)
    }

    pub fn set_drop_lowest(&mut self, category: &GradeType, on: bool) {
        if !self.categories().contains(category) {
            return;
        }
        self.change_settings("Dropped scores changed", None, |settings| {
            settings.options.drop_lowest.retain(|c| c != category);
            if on {
                settings.options.drop_lowest.push(category.clone());
            }
        });
    }

    /// Changes the settings in one step. Returns whether anything changed.
    fn change_settings(&mut self, label: &str, merge: Option<String>, edit: impl FnOnce(&mut Settings)) -> bool {
        let before = self.data.settings();
        let mut settings = before.clone();
        edit(&mut settings);
        if settings == before {
            return false;
        }
        self.commit_merging(label, Change::ReplaceSettings(Box::new(settings)), merge);
        true
    }

    // ---- Students ------------------------------------------------------------------------

    /// Adds a student; blank names are ignored.
    pub fn add_student(&mut self, name: &str, grade_level: u32) -> Option<Uuid> {
        self.add_students(&[name.to_string()], grade_level).pop()
    }

    /// Adds several students to one grade level, as one step. Blank names are ignored.
    pub fn add_students(&mut self, names: &[String], grade_level: u32) -> Vec<Uuid> {
        let start = self.data.students.len();
        let students: Vec<(usize, Student)> = names
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .enumerate()
            .map(|(offset, name)| {
                let student = Student {
                    id: Uuid::new_v4(),
                    name: name.to_string(),
                    grade_level,
                };
                (start + offset, student)
            })
            .collect();
        let ids: Vec<Uuid> = students.iter().map(|(_, s)| s.id).collect();
        match ids.len() {
            0 => {}
            1 => self.commit(
                "Student added",
                Change::InsertStudents {
                    students,
                    grades: Vec::new(),
                },
            ),
            n => self.commit(
                format!("{n} students added"),
                Change::InsertStudents {
                    students,
                    grades: Vec::new(),
                },
            ),
        }
        ids
    }

    /// Changes a student's name and grade level. Returns whether anything changed.
    pub fn edit_student(&mut self, id: Uuid, name: &str, grade_level: u32) -> bool {
        let name = name.trim();
        let Some(current) = self.student(id) else { return false };
        if name.is_empty() || (current.name == name && current.grade_level == grade_level) {
            return false;
        }
        let edited = Student {
            id,
            name: name.to_string(),
            grade_level,
        };
        self.commit("Student edited", Change::ReplaceStudent(edited));
        true
    }

    /// Removes a student together with all of their grades.
    pub fn delete_student(&mut self, id: Uuid) -> bool {
        if self.student(id).is_none() {
            return false;
        }
        self.commit("Student deleted", Change::RemoveStudents(vec![id]));
        true
    }

    // ---- Grades --------------------------------------------------------------------------

    pub fn add_grade(
        &mut self,
        student_id: Uuid,
        subject: &str,
        period: u32,
        value: String,
        grade_type: GradeType,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let entry = GradeEntry {
            id,
            student_id,
            subject: subject.to_string(),
            period,
            value,
            grade_type,
        };
        let position = self.data.grades.len();
        self.commit("Score added", Change::InsertGrades(vec![(position, entry)]));
        id
    }

    /// Changes what a score says and which category it is in. Returns whether anything changed.
    pub fn edit_grade(&mut self, id: Uuid, value: String, grade_type: GradeType) -> bool {
        let Some(current) = self.grade(id) else { return false };
        if !self.categories().contains(&grade_type) || (current.value == value && current.grade_type == grade_type) {
            return false;
        }
        let edited = GradeEntry {
            value,
            grade_type,
            ..current.clone()
        };
        self.commit("Score edited", Change::ReplaceGrade(edited));
        true
    }

    pub fn delete_grade(&mut self, id: Uuid) -> bool {
        if self.grade(id).is_none() {
            return false;
        }
        self.commit("Score deleted", Change::RemoveGrades(vec![id]));
        true
    }

    // ---- Subjects ------------------------------------------------------------------------

    /// Adds a subject. Returns its tidied name.
    pub fn add_subject(&mut self, name: &str) -> Result<String, ListError> {
        let name = clean_name(name)?;
        if self.subjects().iter().any(|s| same_name(s, &name)) {
            return Err(ListError::Taken);
        }
        if self.custom_subjects().len() >= MAX_CUSTOM_SUBJECTS {
            return Err(ListError::TooMany);
        }
        self.change_settings("Subject added", None, |settings| {
            settings.subjects.push(name.clone());
            settings.subjects.sort_by_key(|s| s.to_lowercase());
        });
        Ok(name)
    }

    /// Renames a subject that was added, together with its scores.
    pub fn rename_subject(&mut self, old: &str, new: &str) -> Result<String, ListError> {
        let new = clean_name(new)?;
        if calc::SUBJECTS.contains(&old) {
            return Err(ListError::BuiltIn);
        }
        if !self.custom_subjects().iter().any(|s| s == old) {
            return Err(ListError::NotFound);
        }
        // The subject being renamed doesn't count: "coding" may become "Coding".
        if new != old && self.subjects().iter().any(|s| s != old && same_name(s, &new)) {
            return Err(ListError::Taken);
        }
        if new == old {
            return Ok(new);
        }
        let mut settings = self.data.settings();
        settings.subjects.retain(|s| s != old);
        settings.subjects.push(new.clone());
        settings.subjects.sort_by_key(|s| s.to_lowercase());
        if let Some(weights) = settings.options.subject_weights.remove(old) {
            settings.options.subject_weights.insert(new.clone(), weights);
        }
        let mut changes = vec![Change::ReplaceSettings(Box::new(settings))];
        changes.extend(self.data.grades.iter().filter(|g| g.subject == old).map(|g| {
            Change::ReplaceGrade(GradeEntry {
                subject: new.clone(),
                ..g.clone()
            })
        }));
        self.commit("Subject renamed", Change::Group(changes));
        Ok(new)
    }

    /// Removes a subject that was added, together with its scores. Returns how many scores.
    pub fn remove_subject(&mut self, name: &str) -> Result<usize, ListError> {
        if calc::SUBJECTS.contains(&name) {
            return Err(ListError::BuiltIn);
        }
        if !self.custom_subjects().iter().any(|s| s == name) {
            return Err(ListError::NotFound);
        }
        let ids: Vec<Uuid> = self
            .data
            .grades
            .iter()
            .filter(|g| g.subject == name)
            .map(|g| g.id)
            .collect();
        let removed = ids.len();
        let mut settings = self.data.settings();
        settings.subjects.retain(|s| s != name);
        settings.options.subject_weights.remove(name);
        self.commit(
            "Subject removed",
            Change::Group(vec![
                Change::RemoveGrades(ids),
                Change::ReplaceSettings(Box::new(settings)),
            ]),
        );
        Ok(removed)
    }

    // ---- Categories ----------------------------------------------------------------------

    fn check_category_name(&self, name: &str, renaming: Option<&str>) -> Result<String, ListError> {
        let name = clean_name(name)?;
        let in_use = self
            .data
            .categories
            .iter()
            .any(|c| same_name(&c.name, &name) && Some(c.name.as_str()) != renaming);
        if is_reserved_category_name(&name) || in_use {
            return Err(ListError::Taken);
        }
        Ok(name)
    }

    /// Adds a category, with no weight until it is given one.
    pub fn add_category(&mut self, name: &str) -> Result<GradeType, ListError> {
        let name = self.check_category_name(name, None)?;
        if self.data.categories.len() >= MAX_CUSTOM_CATEGORIES {
            return Err(ListError::TooMany);
        }
        self.change_settings("Category added", None, |settings| {
            settings.categories.push(CustomCategory {
                name: name.clone(),
                weight: 0.0,
            });
        });
        Ok(GradeType::Custom(name))
    }

    /// Renames a category that was added, together with its scores.
    pub fn rename_category(&mut self, category: &GradeType, new: &str) -> Result<GradeType, ListError> {
        let GradeType::Custom(old) = category else {
            return Err(ListError::BuiltIn);
        };
        if !self.data.categories.iter().any(|c| c.name == *old) {
            return Err(ListError::NotFound);
        }
        let new = self.check_category_name(new, Some(old))?;
        if new == *old {
            return Ok(category.clone());
        }
        let renamed = GradeType::Custom(new.clone());
        let mut settings = self.data.settings();
        if let Some(entry) = settings.categories.iter_mut().find(|c| c.name == *old) {
            entry.name = new.clone();
        }
        for dropped in settings.options.drop_lowest.iter_mut().filter(|c| *c == category) {
            *dropped = renamed.clone();
        }
        for weights in settings.options.subject_weights.values_mut() {
            if let Some(weight) = weights.remove(old) {
                weights.insert(new.clone(), weight);
            }
        }
        let mut changes = vec![Change::ReplaceSettings(Box::new(settings))];
        changes.extend(self.data.grades.iter().filter(|g| g.grade_type == *category).map(|g| {
            Change::ReplaceGrade(GradeEntry {
                grade_type: renamed.clone(),
                ..g.clone()
            })
        }));
        self.commit("Category renamed", Change::Group(changes));
        Ok(renamed)
    }

    /// Removes a category that was added, together with its scores. Returns how many scores.
    pub fn remove_category(&mut self, category: &GradeType) -> Result<usize, ListError> {
        let GradeType::Custom(name) = category else {
            return Err(ListError::BuiltIn);
        };
        if !self.data.categories.iter().any(|c| c.name == *name) {
            return Err(ListError::NotFound);
        }
        let ids: Vec<Uuid> = self
            .data
            .grades
            .iter()
            .filter(|g| g.grade_type == *category)
            .map(|g| g.id)
            .collect();
        let removed = ids.len();
        let mut settings = self.data.settings();
        settings.categories.retain(|c| c.name != *name);
        settings.options.drop_lowest.retain(|c| c != category);
        for weights in settings.options.subject_weights.values_mut() {
            weights.remove(name);
        }
        self.commit(
            "Category removed",
            Change::Group(vec![
                Change::RemoveGrades(ids),
                Change::ReplaceSettings(Box::new(settings)),
            ]),
        );
        Ok(removed)
    }

    // ---- Grade levels --------------------------------------------------------------------

    /// Adds Kindergarten or a grade from 11 to 20 to the levels students can be in.
    pub fn add_grade_level(&mut self, level: u32) -> Result<(), ListError> {
        if !calc::can_add_level(level) {
            return Err(ListError::OutOfRange);
        }
        if self.grade_levels().contains(&level) {
            return Err(ListError::Taken);
        }
        self.change_settings("Grade level added", None, |settings| {
            settings.grade_levels.push(level);
            settings.grade_levels.sort_unstable();
        });
        Ok(())
    }

    /// Removes a level that was added, if no student is in it.
    pub fn remove_grade_level(&mut self, level: u32) -> Result<(), ListError> {
        if calc::GRADE_LEVELS.contains(&level) {
            return Err(ListError::BuiltIn);
        }
        if !self.grade_levels().contains(&level) {
            return Err(ListError::NotFound);
        }
        let students = self.students_in_level(level).count();
        if students > 0 {
            return Err(ListError::InUse(students));
        }
        self.change_settings("Grade level removed", None, |settings| {
            settings.grade_levels.retain(|l| *l != level);
        });
        Ok(())
    }

    // ---- A new school year -----------------------------------------------------------------

    /// The grade level after `level`, if there is one.
    fn next_level(&self, level: u32) -> Option<u32> {
        self.grade_levels().into_iter().find(|l| *l > level)
    }

    /// What a new school year would do, for the confirmation.
    pub fn year_plan(&self) -> YearPlan {
        let moving = self
            .data
            .students
            .iter()
            .filter(|s| self.next_level(s.grade_level).is_some())
            .count();
        YearPlan {
            moving,
            staying: self.data.students.len() - moving,
            scores: self.data.grades.len(),
        }
    }

    /// Clears every score and moves each student up a grade level. Students in the highest
    /// level stay where they are. Weights and lists are kept.
    pub fn new_school_year(&mut self) -> YearPlan {
        let plan = self.year_plan();
        let mut changes: Vec<Change> = self
            .data
            .students
            .iter()
            .filter_map(|student| {
                self.next_level(student.grade_level).map(|level| {
                    Change::ReplaceStudent(Student {
                        grade_level: level,
                        ..student.clone()
                    })
                })
            })
            .collect();
        changes.push(Change::RemoveGrades(self.data.grades.iter().map(|g| g.id).collect()));
        self.commit("New school year started", Change::Group(changes));
        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book_with_two_students() -> (GradeBook, Uuid, Uuid) {
        let mut book = GradeBook::default();
        let ada = book.add_student("  Ada ", 3).unwrap();
        let bob = book.add_student("Bob", 3).unwrap();
        book.add_student("Cy", 4).unwrap();
        (book, ada, bob)
    }

    fn test() -> GradeType {
        GradeType::Test
    }

    fn assert_near(actual: Option<f64>, expected: f64) {
        let actual = actual.expect("an average");
        assert!((actual - expected).abs() < 1e-9, "{actual} is not {expected}");
    }

    #[test]
    fn blank_names_are_ignored_and_names_trimmed() {
        let mut book = GradeBook::default();
        assert_eq!(book.add_student("   ", 1), None);
        assert_eq!(book.add_student("", 1), None);
        let id = book.add_student("  Ada Lovelace  ", 2).unwrap();
        assert_eq!(book.student(id).unwrap().name, "Ada Lovelace");
        assert_eq!(book.undo_label(), Some("Student added"));
    }

    #[test]
    fn deleting_a_student_removes_their_grades_only() {
        let (mut book, ada, bob) = book_with_two_students();
        book.add_grade(ada, "Math", 1, "90".into(), test());
        book.add_grade(bob, "Math", 1, "80".into(), test());
        assert!(book.delete_student(ada));
        assert!(!book.delete_student(ada));
        assert_eq!(book.data().grades.len(), 1);
        assert_eq!(book.data().grades[0].student_id, bob);
    }

    #[test]
    fn grades_are_filtered_by_subject_and_period() {
        let (mut book, ada, _) = book_with_two_students();
        let keep = book.add_grade(ada, "Math", 1, "90".into(), test());
        book.add_grade(ada, "Math", 2, "70".into(), test());
        book.add_grade(ada, "Art", 1, "60".into(), test());
        let found: Vec<_> = book.grades_for(ada, "Math", 1).map(|g| g.id).collect();
        assert_eq!(found, vec![keep]);
        assert!(book.delete_grade(keep));
        assert!(!book.delete_grade(keep));
        assert_eq!(book.grades_for(ada, "Math", 1).count(), 0);
    }

    #[test]
    fn class_average_only_counts_students_in_the_level_with_grades() {
        let (mut book, ada, bob) = book_with_two_students();
        assert_eq!(book.class_average(3, "Math", 1), None);
        book.add_grade(ada, "Math", 1, "90".into(), test());
        book.add_grade(bob, "Math", 1, "70".into(), test());
        let cy = book.students_in_level(4).next().unwrap().id;
        book.add_grade(cy, "Math", 1, "10".into(), test());
        assert_eq!(book.class_average(3, "Math", 1), Some(80.0));
        assert_eq!(book.class_average(3, "Math", 2), None);
        assert_eq!(book.student_average(ada, "Math", 1), Some(90.0));
    }

    #[test]
    fn weights_change_the_average() {
        let (mut book, ada, _) = book_with_two_students();
        book.add_grade(ada, "Math", 1, "100".into(), test());
        book.add_grade(ada, "Math", 1, "50".into(), GradeType::Quiz);
        book.set_weight(None, &test(), 50.0);
        book.set_weight(None, &GradeType::Quiz, 50.0);
        assert_eq!(book.student_average(ada, "Math", 1), Some(75.0));
        book.set_weight(None, &GradeType::Quiz, 0.0);
        assert_eq!(book.student_average(ada, "Math", 1), Some(100.0));
    }

    #[test]
    fn weights_are_clamped_and_total_reported() {
        let mut book = GradeBook::default();
        assert_eq!(book.total_weight(None), 100.0);
        book.set_weight(None, &test(), 250.0);
        book.set_weight(None, &GradeType::Quiz, -5.0);
        assert_eq!(book.weight(&test()), 100.0);
        assert_eq!(book.weight(&GradeType::Quiz), 0.0);
        let restored = GradeBook::new(BackupData {
            test_weight: 400.0,
            ..BackupData::default()
        });
        assert_eq!(restored.weight(&test()), 100.0);
    }

    #[test]
    fn a_subject_can_have_weights_of_its_own() {
        let (mut book, ada, _) = book_with_two_students();
        for subject in ["Math", "Art"] {
            book.add_grade(ada, subject, 1, "100".into(), test());
            book.add_grade(ada, subject, 1, "50".into(), GradeType::Quiz);
        }
        assert!(!book.has_own_weights("Math"));
        book.set_weight(Some("Math"), &GradeType::Quiz, 50.0);
        assert!(book.has_own_weights("Math"));
        // The others of Math's weights started as the defaults: tests 50, quizzes now 50.
        assert_eq!(book.weight_in(Some("Math"), &test()), 50.0);
        assert_eq!(book.student_average(ada, "Math", 1), Some(75.0));
        assert_near(book.student_average(ada, "Art", 1), (100.0 * 50.0 + 50.0 * 17.0) / 67.0);
        assert_eq!(book.weight(&GradeType::Quiz), 17.0, "the default is untouched");
        assert_eq!(book.total_weight(Some("Math")), 33.0 + 50.0 + 50.0);

        book.reset_subject_weights("Math");
        assert!(!book.has_own_weights("Math"));
        assert_eq!(
            book.student_average(ada, "Math", 1),
            book.student_average(ada, "Art", 1)
        );
    }

    #[test]
    fn the_lowest_score_of_a_category_can_be_left_out() {
        let (mut book, ada, _) = book_with_two_students();
        let low = book.add_grade(ada, "Math", 1, "60".into(), GradeType::Quiz);
        book.add_grade(ada, "Math", 1, "100".into(), GradeType::Quiz);
        assert_eq!(book.student_average(ada, "Math", 1), Some(80.0));
        assert!(book.dropped(ada, "Math", 1).is_empty());
        book.set_drop_lowest(&GradeType::Quiz, true);
        assert!(book.drops_lowest(&GradeType::Quiz));
        assert_eq!(book.student_average(ada, "Math", 1), Some(100.0));
        assert_eq!(book.dropped(ada, "Math", 1), HashSet::from([low]));
        book.set_drop_lowest(&GradeType::Quiz, false);
        assert_eq!(book.student_average(ada, "Math", 1), Some(80.0));
    }

    #[test]
    fn what_a_student_needs_follows_the_subjects_weights() {
        let (mut book, ada, _) = book_with_two_students();
        book.add_grade(ada, "Math", 1, "80".into(), test());
        // (80 + x) / 2 is displayed as 85.0 from 84.95 up.
        assert_eq!(book.needed(ada, "Math", 1, &test(), 85.0), Needed::Score(89.9));
        book.set_weight(None, &GradeType::Quiz, 0.0);
        assert_eq!(book.needed(ada, "Math", 1, &GradeType::Quiz, 85.0), Needed::NoEffect);
    }

    #[test]
    fn replace_swaps_everything() {
        let (mut book, _, _) = book_with_two_students();
        book.replace(BackupData::default());
        assert!(book.students().is_empty() && book.data().grades.is_empty());
        book.undo();
        assert_eq!(book.students().len(), 3, "a restore can be undone");
    }

    // ---- Undo and redo ---------------------------------------------------------------------

    #[test]
    fn undo_and_redo_walk_back_and_forth_through_changes() {
        let mut book = GradeBook::default();
        let ada = book.add_student("Ada", 3).unwrap();
        let grade = book.add_grade(ada, "Math", 1, "90".into(), test());
        assert!(book.delete_grade(grade));
        assert_eq!(book.undo_label(), Some("Score deleted"));
        assert!(book.data().grades.is_empty());

        assert_eq!(book.undo().as_deref(), Some("Score deleted"));
        assert_eq!(book.data().grades.len(), 1);
        assert_eq!(book.redo_label(), Some("Score deleted"));
        assert_eq!(book.undo().as_deref(), Some("Score added"));
        assert!(book.data().grades.is_empty());
        assert_eq!(book.undo().as_deref(), Some("Student added"));
        assert!(book.students().is_empty());
        assert!(!book.can_undo());
        assert_eq!(book.undo(), None);

        assert_eq!(book.redo().as_deref(), Some("Student added"));
        assert_eq!(book.redo().as_deref(), Some("Score added"));
        assert_eq!(book.redo().as_deref(), Some("Score deleted"));
        assert!(book.data().grades.is_empty() && book.can_undo() && !book.can_redo());
    }

    #[test]
    fn a_new_change_ends_the_redo_list() {
        let mut book = GradeBook::default();
        book.add_student("Ada", 3).unwrap();
        book.undo();
        assert!(book.can_redo());
        book.add_student("Bob", 3).unwrap();
        assert!(!book.can_redo());
        assert_eq!(book.redo(), None);
    }

    #[test]
    fn undoing_a_deleted_student_brings_back_their_grades_in_the_same_places() {
        let (mut book, ada, bob) = book_with_two_students();
        book.add_grade(ada, "Math", 1, "91".into(), test());
        book.add_grade(bob, "Math", 1, "82".into(), test());
        book.add_grade(ada, "Art", 2, "73".into(), GradeType::Quiz);
        book.add_grade(bob, "Art", 2, "64".into(), GradeType::Quiz);
        let before = book.data().clone();
        assert!(book.delete_student(ada));
        assert_eq!(book.data().grades.len(), 2);
        book.undo();
        assert_eq!(book.data(), &before, "students and grades are back in their old order");
        book.redo();
        assert_eq!(book.data().grades.len(), 2);
        book.undo();
        assert_eq!(book.data(), &before);
    }

    #[test]
    fn dragging_a_weight_is_one_step() {
        let mut book = GradeBook::default();
        for value in [51.0, 52.0, 53.0, 54.0] {
            book.set_weight(None, &test(), value);
        }
        assert_eq!(book.weight(&test()), 54.0);
        assert_eq!(book.undo().as_deref(), Some("Weights changed"));
        assert_eq!(book.weight(&test()), 50.0, "all the way back");
        assert!(!book.can_undo());
        book.redo();
        assert_eq!(book.weight(&test()), 54.0);

        // Another kind of change in between starts a new step.
        book.add_student("Ada", 1);
        book.set_weight(None, &test(), 10.0);
        assert_eq!(book.undo_label(), Some("Weights changed"));
        book.undo();
        assert_eq!(book.weight(&test()), 54.0);
        // And so does another subject's weight.
        book.set_weight(Some("Math"), &test(), 20.0);
        book.set_weight(Some("Art"), &test(), 30.0);
        book.undo();
        assert_eq!(book.weight_in(Some("Math"), &test()), 20.0);
        assert!(!book.has_own_weights("Art"));
    }

    #[test]
    fn setting_a_weight_to_what_it_is_changes_nothing() {
        let mut book = GradeBook::default();
        book.set_weight(None, &test(), 50.0);
        book.set_weight(None, &GradeType::Custom("Nope".into()), 5.0);
        assert!(!book.can_undo());
    }

    #[test]
    fn the_history_version_moves_on_with_every_change_undo_and_redo_and_never_back() {
        let mut book = GradeBook::default();
        let mut seen = vec![book.version()];
        let ada = book.add_student("Ada", 3).unwrap();
        seen.push(book.version());
        book.add_grade(ada, "Math", 1, "90".into(), test());
        seen.push(book.version());
        book.undo();
        seen.push(book.version());
        book.redo();
        seen.push(book.version());
        book.set_weight(None, &test(), 51.0);
        book.set_weight(None, &test(), 52.0); // merges into the step before, and is a change
        seen.push(book.version());
        assert!(seen.windows(2).all(|pair| pair[0] < pair[1]), "{seen:?}");

        // Even when the history is cut short, the version isn't.
        for _ in 0..(MAX_UNDO_STEPS + 5) {
            book.add_grade(ada, "Math", 1, "80".into(), test());
        }
        let before = book.version();
        book.add_grade(ada, "Math", 1, "70".into(), test());
        assert!(book.version() > before);
        // Nothing to undo or change: nothing moves.
        let mut empty = GradeBook::default();
        let start = empty.version();
        assert_eq!((empty.undo(), empty.redo()), (None, None));
        assert!(!empty.delete_student(Uuid::new_v4()));
        assert_eq!(empty.version(), start);
    }

    #[test]
    fn the_history_is_bounded() {
        let mut book = GradeBook::default();
        let ada = book.add_student("Ada", 1).unwrap();
        for _ in 0..(MAX_UNDO_STEPS + 20) {
            book.add_grade(ada, "Math", 1, "90".into(), test());
        }
        let mut undone = 0;
        while book.undo().is_some() {
            undone += 1;
        }
        assert_eq!(undone, MAX_UNDO_STEPS);
        assert_eq!(book.data().grades.len(), 20, "older changes can't be taken back");
    }

    // ---- Editing -----------------------------------------------------------------------------

    #[test]
    fn a_score_can_be_edited_and_the_edit_undone() {
        let (mut book, ada, _) = book_with_two_students();
        let id = book.add_grade(ada, "Math", 1, "90".into(), test());
        assert!(!book.edit_grade(id, "90".into(), test()), "nothing changed");
        assert!(!book.edit_grade(id, "90".into(), GradeType::Custom("Nope".into())));
        assert!(!book.edit_grade(Uuid::new_v4(), "1".into(), test()));
        assert!(book.edit_grade(id, "18/20".into(), GradeType::Quiz));
        let grade = book.grade(id).unwrap();
        assert_eq!((grade.value.as_str(), &grade.grade_type), ("18/20", &GradeType::Quiz));
        book.undo();
        assert_eq!(book.grade(id).unwrap().value, "90");
        assert_eq!(book.grade(id).unwrap().grade_type, test());
    }

    #[test]
    fn a_student_can_be_edited_and_the_edit_undone() {
        let (mut book, ada, _) = book_with_two_students();
        assert!(!book.edit_student(ada, "Ada", 3), "nothing changed");
        assert!(!book.edit_student(ada, "   ", 3), "a name is needed");
        assert!(!book.edit_student(Uuid::new_v4(), "X", 3));
        assert!(book.edit_student(ada, "  Ada L. ", 5));
        assert_eq!(book.student(ada).unwrap().name, "Ada L.");
        assert_eq!(book.student(ada).unwrap().grade_level, 5);
        book.undo();
        assert_eq!(book.student(ada).unwrap().name, "Ada");
        assert_eq!(book.student(ada).unwrap().grade_level, 3);
    }

    #[test]
    fn several_students_are_added_in_one_step() {
        let mut book = GradeBook::default();
        let names: Vec<String> = ["Ann", "  ", "Ben", "Cy"].iter().map(|s| s.to_string()).collect();
        let ids = book.add_students(&names, 2);
        assert_eq!(ids.len(), 3);
        let listed: Vec<&str> = book.students().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(listed, ["Ann", "Ben", "Cy"]);
        assert_eq!(book.undo_label(), Some("3 students added"));
        book.undo();
        assert!(book.students().is_empty());
        assert!(book.add_students(&[], 1).is_empty());
        assert!(!book.can_redo() || book.redo_label() == Some("3 students added"));
    }

    #[test]
    fn pasted_lists_become_names() {
        let text =
            "1. Ada Lovelace\r\n2) Grace   Hopper\n- Alan Turing\n• Linus\n\n   \nKatherine\tJohnson\n  Dennis  \n7";
        assert_eq!(
            parse_names(text),
            [
                "Ada Lovelace",
                "Grace Hopper",
                "Alan Turing",
                "Linus",
                "Katherine Johnson",
                "Dennis",
                "7"
            ]
        );
        assert_eq!(parse_names("3rd Street Kid"), ["3rd Street Kid"]);
        assert_eq!(parse_names(&"x".repeat(200))[0].len(), MAX_STUDENT_NAME_LEN);
        assert!(parse_names("").is_empty() && parse_names("\n\n").is_empty());
    }

    // ---- Subjects ------------------------------------------------------------------------

    #[test]
    fn subjects_can_be_added_renamed_and_removed() {
        let (mut book, ada, _) = book_with_two_students();
        assert_eq!(book.subjects().len(), calc::SUBJECTS.len());
        assert_eq!(book.add_subject("  Coding   Club "), Ok("Coding Club".to_string()));
        assert_eq!(book.add_subject("coding club"), Err(ListError::Taken));
        assert_eq!(
            book.add_subject("math"),
            Err(ListError::Taken),
            "built-in ones count too"
        );
        assert_eq!(book.add_subject("  "), Err(ListError::Blank));
        assert_eq!(book.add_subject(&"x".repeat(41)), Err(ListError::TooLong));
        let subjects = book.subjects();
        let position = |name: &str| subjects.iter().position(|s| s == name).unwrap();
        assert!(position("Bible") < position("Coding Club") && position("Coding Club") < position("English"));
        assert_eq!(book.custom_subjects(), ["Coding Club"]);

        book.add_grade(ada, "Coding Club", 1, "90".into(), test());
        book.set_weight(Some("Coding Club"), &test(), 10.0);
        assert_eq!(
            book.rename_subject("Coding Club", "Robotics"),
            Ok("Robotics".to_string())
        );
        assert_eq!(book.custom_subjects(), ["Robotics"]);
        assert_eq!(book.grades_for(ada, "Robotics", 1).count(), 1);
        assert_eq!(book.weight_in(Some("Robotics"), &test()), 10.0);
        assert!(!book.has_own_weights("Coding Club"));
        assert_eq!(book.rename_subject("Math", "Maths"), Err(ListError::BuiltIn));
        assert_eq!(book.rename_subject("Nope", "X"), Err(ListError::NotFound));
        book.undo();
        assert_eq!(book.custom_subjects(), ["Coding Club"]);
        assert_eq!(book.grades_for(ada, "Coding Club", 1).count(), 1);

        assert_eq!(book.remove_subject("Math"), Err(ListError::BuiltIn));
        assert_eq!(book.remove_subject("Coding Club"), Ok(1));
        assert!(book.custom_subjects().is_empty() && book.data().grades.is_empty());
        book.undo();
        assert_eq!(book.custom_subjects(), ["Coding Club"]);
        assert_eq!(book.data().grades.len(), 1, "the scores come back with the subject");
        assert!(book.has_own_weights("Coding Club"));
    }

    #[test]
    fn a_subject_with_scores_is_listed_even_if_it_was_never_added() {
        let (mut book, ada, _) = book_with_two_students();
        book.add_grade(ada, "Chess", 1, "90".into(), test());
        assert!(book.subjects().contains(&"Chess".to_string()));
        assert_eq!(book.custom_subjects(), ["Chess"]);
        assert_eq!(book.remove_subject("Chess"), Ok(1));
        assert!(!book.subjects().contains(&"Chess".to_string()));
    }

    #[test]
    fn a_subject_can_change_only_its_letter_case() {
        let (mut book, ada, _) = book_with_two_students();
        book.add_subject("coding").unwrap();
        book.add_grade(ada, "coding", 1, "90".into(), test());
        assert_eq!(book.rename_subject("coding", "Coding"), Ok("Coding".to_string()));
        assert_eq!(book.custom_subjects(), ["Coding"]);
        assert_eq!(book.grades_for(ada, "Coding", 1).count(), 1, "its scores go along");
        book.add_subject("Chess").unwrap();
        assert_eq!(
            book.rename_subject("Chess", "CODING"),
            Err(ListError::Taken),
            "another subject's name is still taken"
        );
    }

    #[test]
    fn names_are_the_same_in_any_alphabet_and_the_file_that_is_saved_can_be_loaded() {
        let mut book = GradeBook::default();
        assert!(book.add_category("Évaluation").is_ok());
        assert_eq!(
            book.add_category("évaluation"),
            Err(ListError::Taken),
            "a letter with an accent has cases too"
        );
        assert_eq!(book.add_category("ÉVALUATION"), Err(ListError::Taken));
        assert!(book.add_subject("Ἑλληνικά").is_ok());
        assert_eq!(book.add_subject("ἑλληνικά"), Err(ListError::Taken));
        // What the book accepted is a file the load check accepts: the two use one comparison.
        let json = serde_json::to_string(book.data()).unwrap();
        let loaded: BackupData = serde_json::from_str(&json).unwrap();
        assert_eq!(&loaded, book.data());
    }

    // ---- Categories ----------------------------------------------------------------------

    #[test]
    fn categories_can_be_added_renamed_and_removed() {
        let (mut book, ada, _) = book_with_two_students();
        assert_eq!(book.add_category("Test"), Err(ListError::Taken));
        assert_eq!(book.add_category(" quiz "), Err(ListError::Taken));
        assert_eq!(book.add_category(""), Err(ListError::Blank));
        let project = book.add_category(" Project ").unwrap();
        assert_eq!(project, GradeType::Custom("Project".into()));
        assert_eq!(book.add_category("PROJECT"), Err(ListError::Taken));
        assert_eq!(book.categories().last(), Some(&project));
        assert_eq!(book.weight(&project), 0.0, "no weight until it is given one");

        book.set_weight(None, &project, 50.0);
        book.set_weight(Some("Math"), &project, 25.0);
        book.set_drop_lowest(&project, true);
        book.add_grade(ada, "Math", 1, "100".into(), project.clone());
        book.add_grade(ada, "Math", 1, "80".into(), test());
        assert_near(
            book.student_average(ada, "Math", 1),
            (100.0 * 25.0 + 80.0 * 50.0) / 75.0,
        );
        assert_eq!(book.student_average(ada, "Art", 1), None);

        let lab = book.rename_category(&project, "Lab").unwrap();
        assert_eq!(lab, GradeType::Custom("Lab".into()));
        assert_eq!(book.weight(&lab), 50.0);
        assert_eq!(book.weight_in(Some("Math"), &lab), 25.0);
        assert!(book.drops_lowest(&lab));
        assert_eq!(book.count_grades_in_category(&lab), 1);
        assert_eq!(book.count_grades_in_category(&project), 0);
        assert_eq!(book.rename_category(&test(), "X"), Err(ListError::BuiltIn));
        assert_eq!(book.rename_category(&lab, "Homework"), Err(ListError::Taken));
        assert_eq!(
            book.rename_category(&lab, "Lab"),
            Ok(lab.clone()),
            "same name: nothing to do"
        );
        book.undo();
        assert_eq!(book.count_grades_in_category(&project), 1);
        assert!(book.drops_lowest(&project) && !book.drops_lowest(&lab));
        book.redo();

        assert_eq!(book.remove_category(&test()), Err(ListError::BuiltIn));
        assert_eq!(book.remove_category(&lab), Ok(1));
        assert_eq!(book.categories(), GradeType::ALL);
        assert_eq!(book.data().grades.len(), 1);
        assert!(!book.has_own_weights("Math") || book.weight_in(Some("Math"), &lab) == 0.0);
        book.undo();
        assert_eq!(book.count_grades_in_category(&lab), 1);
        assert_eq!(book.weight(&lab), 50.0);
    }

    #[test]
    fn there_is_a_limit_to_custom_categories() {
        let mut book = GradeBook::default();
        for n in 0..MAX_CUSTOM_CATEGORIES {
            book.add_category(&format!("Extra {n}")).unwrap();
        }
        assert_eq!(book.add_category("One more"), Err(ListError::TooMany));
    }

    // ---- Grade levels --------------------------------------------------------------------

    #[test]
    fn grade_levels_can_be_added_and_removed() {
        let mut book = GradeBook::default();
        assert_eq!(book.grade_levels(), (1..=10).collect::<Vec<_>>());
        assert_eq!(book.add_grade_level(12), Ok(()));
        assert_eq!(book.add_grade_level(0), Ok(()));
        assert_eq!(book.add_grade_level(12), Err(ListError::Taken));
        assert_eq!(book.add_grade_level(5), Err(ListError::OutOfRange));
        assert_eq!(book.add_grade_level(21), Err(ListError::OutOfRange));
        assert_eq!(book.grade_levels().first(), Some(&0));
        assert_eq!(book.grade_levels().last(), Some(&12));
        assert_eq!(book.extra_levels(), [0, 12]);

        let ada = book.add_student("Ada", 12).unwrap();
        assert_eq!(book.remove_grade_level(12), Err(ListError::InUse(1)));
        assert_eq!(book.remove_grade_level(7), Err(ListError::BuiltIn));
        assert_eq!(book.remove_grade_level(15), Err(ListError::NotFound));
        book.delete_student(ada);
        assert_eq!(book.remove_grade_level(12), Ok(()));
        assert_eq!(book.extra_levels(), [0]);
        book.undo();
        assert_eq!(book.extra_levels(), [0, 12]);
    }

    #[test]
    fn a_student_in_a_level_that_isnt_listed_keeps_that_level_available() {
        let mut book = GradeBook::default();
        book.add_student("Ada", 12).unwrap();
        assert!(book.grade_levels().contains(&12));
        assert_eq!(book.extra_levels(), [12]);
    }

    // ---- A new school year ---------------------------------------------------------------

    #[test]
    fn a_new_school_year_clears_scores_and_moves_students_up() {
        let mut book = GradeBook::default();
        let kid = book.add_student("Kid", 3).unwrap();
        let senior = book.add_student("Senior", 10).unwrap();
        book.add_grade(kid, "Math", 1, "90".into(), test());
        book.add_grade(senior, "Art", 2, "80".into(), GradeType::Quiz);
        book.set_weight(None, &test(), 40.0);
        book.add_subject("Chess").unwrap();
        let before = book.data().clone();

        assert_eq!(
            book.year_plan(),
            YearPlan {
                moving: 1,
                staying: 1,
                scores: 2
            }
        );
        book.new_school_year();
        assert_eq!(book.student(kid).unwrap().grade_level, 4);
        assert_eq!(book.student(senior).unwrap().grade_level, 10, "the highest grade stays");
        assert!(book.data().grades.is_empty());
        assert_eq!(book.weight(&test()), 40.0, "settings are kept");
        assert_eq!(book.custom_subjects(), ["Chess"]);

        assert_eq!(book.undo().as_deref(), Some("New school year started"));
        assert_eq!(book.data(), &before, "one undo brings everything back");
    }

    #[test]
    fn a_new_school_year_skips_levels_that_dont_exist() {
        let mut book = GradeBook::default();
        book.add_grade_level(12).unwrap();
        let senior = book.add_student("Senior", 10).unwrap();
        let top = book.add_student("Top", 12).unwrap();
        book.new_school_year();
        assert_eq!(
            book.student(senior).unwrap().grade_level,
            12,
            "11 isn't one of the levels"
        );
        assert_eq!(book.student(top).unwrap().grade_level, 12);

        let mut book = GradeBook::default();
        book.add_grade_level(0).unwrap();
        let little = book.add_student("Little", 0).unwrap();
        book.new_school_year();
        assert_eq!(
            book.student(little).unwrap().grade_level,
            1,
            "Kindergarten comes before grade 1"
        );
    }
}
