//! Renders the real UI headlessly with Slint's software renderer (the renderer the
//! shipped app uses) and writes PNG screenshots to `target/tmp/screenshots`.
//!
//! Besides producing images to look at (they are also uploaded by CI), it checks
//! that every scenario actually drew something.

use std::cell::Cell;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, Model, PhysicalSize, PlatformError, Rgb8Pixel, SharedString};
use uuid::Uuid;

use gradebook::app::App;
use gradebook::model::{BackupData, CustomCategory, GradeEntry, GradeType, GradingOptions, Student};
use gradebook::prefs::Appearance;
use gradebook::store::Store;
use gradebook::ui::{AlertData, Backend};

struct TestPlatform {
    window: Rc<MinimalSoftwareWindow>,
    clock: Rc<Cell<Duration>>,
}

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        self.clock.get()
    }
}

struct Screen {
    window: Rc<MinimalSoftwareWindow>,
    clock: Rc<Cell<Duration>>,
    out_dir: PathBuf,
}

impl Screen {
    /// Lets animations finish, lays out at `width` x `height` logical pixels (at
    /// `scale`) and returns the rendered RGB image.
    fn render(&self, width: u32, height: u32, scale: f32) -> Vec<Rgb8Pixel> {
        self.window
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
        let (w, h) = ((width as f32 * scale) as u32, (height as f32 * scale) as u32);
        self.window.set_size(PhysicalSize::new(w, h));
        // Move time forward so any running animation (e.g. the segmented control) completes.
        self.clock.set(self.clock.get() + Duration::from_secs(5));
        slint::platform::update_timers_and_animations();
        let mut buffer = vec![Rgb8Pixel::default(); (w * h) as usize];
        let drew = self.window.draw_if_needed(|renderer| {
            renderer.render(&mut buffer, w as usize);
        });
        assert!(drew, "nothing was drawn");
        buffer
    }

    fn shot(&self, name: &str, width: u32, height: u32, scale: f32) -> Vec<Rgb8Pixel> {
        let buffer = self.render(width, height, scale);
        let (w, h) = ((width as f32 * scale) as u32, (height as f32 * scale) as u32);

        // Guard against a blank or single-colour render (e.g. missing fonts).
        let mut distinct = std::collections::HashSet::new();
        for p in buffer.iter().step_by(7) {
            distinct.insert((p.r, p.g, p.b));
            if distinct.len() > 40 {
                break;
            }
        }
        assert!(
            distinct.len() > 8,
            "{name}: render looks blank ({} colours)",
            distinct.len()
        );

        std::fs::create_dir_all(&self.out_dir).unwrap();
        let path = self.out_dir.join(format!("{name}.png"));
        let mut encoder = png::Encoder::new(BufWriter::new(File::create(&path).unwrap()), w, h);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let bytes: Vec<u8> = buffer.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        writer.write_image_data(&bytes).unwrap();
        println!("wrote {}", path.display());
        buffer
    }
}

fn student(name: &str, level: u32) -> Student {
    Student {
        id: Uuid::new_v4(),
        name: name.into(),
        grade_level: level,
    }
}

fn grade(student: &Student, subject: &str, period: u32, value: &str, grade_type: GradeType) -> GradeEntry {
    GradeEntry {
        id: Uuid::new_v4(),
        student_id: student.id,
        subject: subject.into(),
        period,
        value: value.into(),
        grade_type,
    }
}

fn sample_data() -> BackupData {
    let ada = student("Ada Lovelace", 3);
    let grace = student("Grace Hopper", 3);
    let alan = student("Alan Turing", 3);
    let katherine = student("Katherine Johnson", 3);
    let linus = student("Linus", 3);
    let others = [
        student("Margaret Hamilton", 1),
        student("Dennis Ritchie", 4),
        student("Barbara Liskov", 4),
        student("Edsger Dijkstra", 5),
        student("Donald Knuth", 7),
        student("Radia Perlman", 10),
    ];

    use GradeType::{Homework, Quiz, Test};
    let project = || GradeType::Custom("Project".into());
    let grades = vec![
        grade(&ada, "Math", 1, "96", Test),
        grade(&ada, "Math", 1, "92.5", Test),
        grade(&ada, "Math", 1, "88", Quiz),
        grade(&ada, "Math", 1, "100", Quiz),
        grade(&ada, "Math", 1, "100", Homework),
        grade(&ada, "Math", 1, "95", Homework),
        grade(&ada, "Math", 1, "99.5", Homework),
        grade(&ada, "Math", 1, "84", Homework),
        grade(&ada, "Math", 1, "91", Homework),
        grade(&ada, "Math", 1, "97", Homework),
        grade(&ada, "Math", 1, "100", Homework),
        grade(&ada, "Math", 1, "90", Homework),
        grade(&ada, "Math", 1, "+5", project()),
        grade(&ada, "Math", 1, "EX", Homework),
        grade(&grace, "Math", 1, "78", Test),
        grade(&grace, "Math", 1, "70", Quiz),
        grade(&grace, "Math", 1, "85", Homework),
        grade(&grace, "Math", 1, "18/20", project()),
        grade(&alan, "Math", 1, "100", Test),
        grade(&katherine, "Math", 1, "62", Test),
        grade(&katherine, "Math", 1, "58", Quiz),
        // Another subject: must not show up.
        grade(&alan, "Art", 1, "12", Test),
        // Another period: only the Insights screen's trend shows these.
        grade(&ada, "Math", 2, "88", Test),
        grade(&grace, "Math", 2, "74", Test),
        grade(&alan, "Math", 2, "91", Test),
        grade(&katherine, "Math", 2, "66", Test),
    ];

    let mut students = vec![ada, grace, alan, katherine, linus];
    students.extend(others);
    BackupData {
        students,
        grades,
        test_weight: 45.0,
        quiz_weight: 15.0,
        homework_weight: 30.0,
        categories: vec![CustomCategory {
            name: "Project".into(),
            weight: 10.0,
        }],
        subjects: vec!["Geography".into(), "Latin".into()],
        grade_levels: vec![11],
        options: GradingOptions {
            drop_lowest: vec![Homework],
            ..GradingOptions::default()
        },
    }
}

/// A pointer click at a position of the window.
fn click_at(screen: &Screen, x: f32, y: f32) {
    let position = slint::LogicalPosition::new(x, y);
    screen.window.dispatch_event(WindowEvent::PointerMoved { position });
    screen.window.dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    screen.window.dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn scroll(screen: &Screen, y: f32) {
    screen.window.dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(500.0, 400.0),
        delta_x: 0.0,
        delta_y: y,
    });
}

fn first_score(backend: &Backend) -> SharedString {
    backend.get_rows().row_data(0).unwrap().chips.row_data(0).unwrap().id
}

fn student_id(backend: &Backend, name: &str) -> SharedString {
    backend
        .get_rows()
        .iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("{name} is not on the screen"))
        .id
}

fn screenshots_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("screenshots")
}

#[test]
fn renders_every_screen() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let clock = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(TestPlatform {
        window: window.clone(),
        clock: clock.clone(),
    }))
    .unwrap();
    let screen = Screen {
        window,
        clock,
        out_dir: screenshots_dir(),
    };

    let data_dir = tempfile::tempdir().unwrap();
    let store = Store::in_dir(data_dir.path());
    store.save(&sample_data()).unwrap();
    let app = App::new(Some(store)).unwrap();
    app.window().show().unwrap();
    app.set_appearance(Appearance::Light);

    // Grade 3, Math, period 1 has the interesting data.
    let backend = app.window().global::<Backend>();
    backend.set_grade_level_index(2);
    backend.invoke_filters_changed();

    // Grades tab.
    app.set_tab(0);
    screen.shot("grades-light", 1000, 760, 1.0);
    screen.shot("grades-light-2x", 1000, 760, 2.0);
    // Top-left corner at 2x, to inspect fine details (icons, chips, text).
    screen.shot("grades-detail-2x", 540, 330, 2.0);
    screen.shot("grades-narrow", 540, 700, 1.0);

    // Open the "Grade" picker with a real click.
    screen.render(1000, 760, 1.0);
    click_at(&screen, 420.0, 30.0);
    screen.shot("grades-picker-open", 1000, 760, 1.0);
    screen.window.dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    screen.window.dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Escape.into(),
    });

    app.set_appearance(Appearance::Dark);
    screen.shot("grades-dark", 1000, 760, 1.0);
    app.set_appearance(Appearance::Light);

    // Students tab.
    app.set_tab(1);
    screen.shot("students-light", 1000, 760, 1.0);
    screen.shot("students-narrow", 540, 700, 1.0);
    app.set_appearance(Appearance::Dark);
    screen.shot("students-dark", 1000, 760, 1.0);
    app.set_appearance(Appearance::Light);

    // Insights tab. It is longer than the other screens, so it is drawn on a taller window.
    app.set_tab(2);
    screen.shot("insights-light", 1000, 1000, 1.0);
    screen.shot("insights-light-2x", 1000, 1000, 2.0);
    screen.shot("insights-narrow", 540, 1000, 1.0);
    app.set_appearance(Appearance::Dark);
    screen.shot("insights-dark", 1000, 1000, 1.0);
    app.set_appearance(Appearance::Light);

    // Settings tab, from the top and further down.
    app.set_tab(3);
    screen.shot("settings-light", 1000, 760, 1.0);
    screen.shot("settings-narrow", 540, 700, 1.0);
    app.set_appearance(Appearance::Dark);
    screen.shot("settings-dark", 1000, 760, 1.0);
    app.set_appearance(Appearance::Light);
    screen.render(1000, 760, 1.0);
    scroll(&screen, -900.0);
    screen.shot("settings-lists", 1000, 760, 1.0);
    scroll(&screen, -2000.0);
    screen.shot("settings-data", 1000, 760, 1.0);
    scroll(&screen, 5000.0);

    // The dialogs, over the screen they are opened from.
    app.set_tab(0);
    let score = first_score(&backend);
    backend.invoke_edit_grade(score);
    screen.shot("dialog-edit-score", 1000, 760, 1.0);
    backend.invoke_close_dialog();

    app.set_tab(1);
    backend.invoke_edit_student(student_id(&backend, "Ada Lovelace"));
    screen.shot("dialog-edit-student", 1000, 760, 1.0);
    backend.invoke_close_dialog();

    backend.invoke_open_paste_list();
    backend.set_paste_level(2);
    screen.shot("dialog-paste-list", 1000, 760, 1.0);
    backend.invoke_close_dialog();

    app.set_tab(0);
    backend.invoke_open_needed(student_id(&backend, "Grace Hopper"));
    screen.shot("dialog-needed", 1000, 760, 1.0);
    app.set_appearance(Appearance::Dark);
    screen.shot("dialog-needed-dark", 1000, 760, 1.0);
    app.set_appearance(Appearance::Light);
    backend.invoke_close_dialog();

    backend.invoke_open_report(student_id(&backend, "Ada Lovelace"));
    screen.shot("dialog-report", 1000, 760, 1.0);
    screen.shot("dialog-report-narrow", 540, 700, 1.0);
    app.set_appearance(Appearance::Dark);
    screen.shot("dialog-report-dark", 1000, 760, 1.0);
    app.set_appearance(Appearance::Light);
    backend.invoke_close_dialog();

    app.set_tab(3);
    backend.invoke_rename_subject(0);
    screen.shot("dialog-rename", 1000, 760, 1.0);
    backend.invoke_close_dialog();

    // A grade level with no students.
    app.set_tab(0);
    backend.set_grade_level_index(8);
    backend.invoke_filters_changed();
    screen.shot("grades-empty", 1000, 760, 1.0);
    backend.set_grade_level_index(2);
    backend.invoke_filters_changed();

    // An alert on top of the Settings screen.
    app.set_tab(3);
    backend.set_alert(AlertData {
        visible: true,
        title: "Restore Backup?".into(),
        message: "This replaces all current students, grades and weights with the backup (11 students, 16 grades). \
                  A copy of your current data is saved next to the data file first."
            .into(),
        confirm_label: "Restore".into(),
        destructive: true,
    });
    screen.shot("alert-light", 1000, 760, 1.0);
    backend.invoke_alert_dismissed();

    // The bar that offers to take a change back. One render only: it goes away by itself.
    app.set_tab(0);
    let score = first_score(&backend);
    backend.invoke_delete_grade(score);
    screen.shot("toast-light", 1000, 760, 1.0);

    // A newer version is out: the bar above the tabs stays until it is sent away, and the
    // bar of a change stands above it.
    app.receive_update_answer(
        false,
        Ok(r#"{"tag_name": "v9.0.0", "html_url": "https://github.com/JMTDI/GradeBook/releases/tag/v9.0.0"}"#.into()),
    );
    screen.shot("update-light", 1000, 760, 1.0);
    screen.shot("update-narrow", 540, 700, 1.0);
    app.set_appearance(Appearance::Dark);
    screen.shot("update-dark", 1000, 760, 1.0);
    app.set_appearance(Appearance::Light);
    let score = first_score(&backend);
    backend.invoke_delete_grade(score);
    screen.shot("update-toast", 1000, 760, 1.0);

    // And what Settings says about it, at the bottom of the screen.
    app.set_tab(3);
    screen.render(1000, 760, 1.0);
    scroll(&screen, -5000.0);
    screen.shot("settings-updates", 1000, 760, 1.0);
}
