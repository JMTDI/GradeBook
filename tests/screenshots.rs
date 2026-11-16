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
use slint::{ComponentHandle, PhysicalSize, PlatformError, Rgb8Pixel};
use uuid::Uuid;

use gradebook::app::{App, Appearance};
use gradebook::model::{BackupData, GradeEntry, GradeType, Student};
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
        grade(&grace, "Math", 1, "78", Test),
        grade(&grace, "Math", 1, "70", Quiz),
        grade(&grace, "Math", 1, "85", Homework),
        grade(&alan, "Math", 1, "100", Test),
        // Other subject/period: must not show up.
        grade(&alan, "Art", 1, "12", Test),
        grade(&alan, "Math", 2, "34", Test),
    ];

    let mut students = vec![ada, grace, alan, katherine, linus];
    students.extend(others);
    BackupData {
        students,
        grades,
        ..BackupData::default()
    }
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
    let at = slint::LogicalPosition::new(420.0, 30.0);
    screen.window.dispatch_event(WindowEvent::PointerMoved { position: at });
    screen.window.dispatch_event(WindowEvent::PointerPressed {
        position: at,
        button: PointerEventButton::Left,
    });
    screen.window.dispatch_event(WindowEvent::PointerReleased {
        position: at,
        button: PointerEventButton::Left,
    });
    screen.shot("grades-picker-open", 1000, 760, 1.0);
    screen.window.dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    screen.window.dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Escape.into(),
    });

    app.set_appearance(Appearance::Dark);
    screen.shot("grades-dark", 1000, 760, 1.0);

    // Settings tab.
    app.set_appearance(Appearance::Light);
    app.set_tab(1);
    screen.shot("settings-light", 1000, 760, 1.0);
    screen.shot("settings-narrow", 540, 700, 1.0);
    app.set_appearance(Appearance::Dark);
    screen.shot("settings-dark", 1000, 760, 1.0);

    // A grade level with no students.
    app.set_appearance(Appearance::Light);
    app.set_tab(0);
    backend.set_grade_level_index(8);
    backend.invoke_filters_changed();
    screen.shot("grades-empty", 1000, 760, 1.0);

    // An alert on top of the Settings screen.
    app.set_tab(1);
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
}
