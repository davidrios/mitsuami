//! Starting an app on the native backend of the target platform.

use mitsuami_core::{AnyView, Size, Ui, UiEvent, View};
use mitsuami_reactive::{Owner, provide};

struct WindowSpec {
    title: String,
    size: Size,
    content: Box<dyn FnOnce() -> AnyView>,
}

/// An application: its windows and how to start it.
///
/// ```ignore
/// App::new().window("Counter", Size::new(360.0, 200.0), || counter(0)).run();
/// ```
#[derive(Default)]
pub struct App {
    windows: Vec<WindowSpec>,
}

impl App {
    pub fn new() -> App {
        App::default()
    }

    /// Adds a window, opened at startup. `size` is the content size.
    pub fn window<V: View>(
        mut self,
        title: impl Into<String>,
        size: Size,
        content: impl FnOnce() -> V + 'static,
    ) -> App {
        self.windows.push(WindowSpec { title: title.into(), size, content: Box::new(move || AnyView::new(content())) });
        self
    }

    /// Runs until the last window closes.
    pub fn run(self) {
        let windows = self.windows;
        let setup = move |ui: &Ui| {
            // The app scope makes the Ui available to every component
            // (`inject::<Ui>()`, `spawn_local`, `sleep`); it lives as long
            // as the app.
            let app = Owner::new_root();
            app.with(|| provide(ui.clone()));
            for spec in windows {
                open(ui, app, spec);
            }
        };
        #[cfg(target_os = "macos")]
        mitsuami_appkit::run(setup);
        #[cfg(target_os = "linux")]
        mitsuami_gtk::run(setup);
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = setup;
            panic!("mitsuami: no native backend for this platform yet (WinUI arrives in M3)");
        }
    }
}

fn open(ui: &Ui, app: Owner, spec: WindowSpec) {
    let window = ui.create_window(spec.title, spec.size);
    // Each window owns its reactive state; closing it disposes everything,
    // including the tasks it started.
    let owner = app.child();
    let root = owner.with(|| (spec.content)().build(ui));
    ui.append_child(window, root);
    let weak = ui.downgrade();
    ui.on_event(window, move |event| {
        if *event == UiEvent::WindowCloseRequested
            && let Some(ui) = weak.upgrade()
        {
            owner.dispose();
            ui.destroy(window);
        }
    });
}
