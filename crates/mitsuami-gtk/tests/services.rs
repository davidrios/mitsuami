//! The real GTK services, end to end: clipboard, the header bar menu, alert
//! dialogs and file dialogs. App tests use scripted services instead, so
//! this is where the native ones are checked. Runs on the private test
//! display, whose clipboard is its own.
//!
//! A plain `main` (harness = false): GTK has to run on the main thread.

#[cfg(target_os = "linux")]
mod checks {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use gtk::glib;
    use gtk::prelude::*;
    use mitsuami_core::services::{Alert, Menu, MenuBar, MenuItem, OpenFile, Shortcut};
    use mitsuami_core::{Size, Ui};
    use mitsuami_gtk::{BackendOptions, GtkBackend, GtkHandle};

    pub struct Fixture {
        pub ui: Ui,
        pub handle: GtkHandle,
    }

    pub fn fixture() -> Fixture {
        mitsuami_gtk::init_for_tests();
        let backend = GtkBackend::new(BackendOptions::default());
        let handle = backend.handle();
        Fixture { ui: Ui::new(backend), handle }
    }

    /// Lets GTK deliver what it queued and ticks the UI, until `done`.
    fn pump_until(f: &Fixture, what: &str, done: impl Fn() -> bool) {
        let started = Instant::now();
        while !done() {
            assert!(started.elapsed() < Duration::from_secs(5), "timed out waiting for {what}");
            while glib::MainContext::default().iteration(false) {}
            f.ui.tick();
            f.handle.show_pending_windows();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn find<W: IsA<gtk::Widget>>(root: &gtk::Widget, matches: &dyn Fn(&W) -> bool) -> Option<W> {
        if let Some(w) = root.downcast_ref::<W>()
            && matches(w)
        {
            return Some(w.clone());
        }
        let mut child = root.first_child();
        while let Some(c) = child {
            if let Some(found) = find(&c, matches) {
                return Some(found);
            }
            child = c.next_sibling();
        }
        None
    }

    /// A toplevel that isn't one of ours: the dialog GTK opened.
    fn dialog_window(f: &Fixture) -> Option<gtk::Window> {
        let ours: Vec<gtk::Window> = f.handle_windows();
        gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Window>().ok())
            .find(|w| w.is_visible() && !ours.contains(w))
    }

    trait Windows {
        fn handle_windows(&self) -> Vec<gtk::Window>;
    }

    impl Windows for Fixture {
        fn handle_windows(&self) -> Vec<gtk::Window> {
            self.ui.windows().into_iter().filter_map(|id| self.handle.gtk_window(id)).collect()
        }
    }

    pub fn clipboard_round_trips(f: &Fixture) {
        let written = Rc::new(RefCell::new(None));
        let read = Rc::new(RefCell::new(None));
        let (w, r) = (written.clone(), read.clone());
        let write = f.ui.set_clipboard_text("mitsuami ✓");
        let ui = f.ui.clone();
        f.ui.spawn_local(async move {
            *w.borrow_mut() = Some(write.await);
            *r.borrow_mut() = ui.clipboard_text().await;
        });
        pump_until(f, "the clipboard", || read.borrow().is_some());
        assert_eq!(*written.borrow(), Some(Ok(())));
        assert_eq!(read.borrow().as_deref(), Some("mitsuami ✓"));
    }

    pub fn menus_are_installed_and_activate(f: &Fixture) {
        let window = f.ui.create_window("menu host", Size::new(400.0, 300.0));
        f.ui.tick();
        let chosen = Rc::new(Cell::new(0));
        let (c, unavailable) = (chosen.clone(), chosen.clone());
        f.ui.set_menu(
            MenuBar::new()
                .menu(
                    Menu::new("File")
                        .item(MenuItem::new("New", move || c.set(c.get() + 1)).shortcut(Shortcut::primary('n')))
                        .item(MenuItem::new("Unavailable", move || unavailable.set(100)).enabled(false)),
                )
                .menu(Menu::new("View").item(MenuItem::new("Zoom", || {}))),
        );
        f.ui.tick();
        let gtk_window = f.handle.gtk_window(window).unwrap();
        let header = gtk_window.titlebar().expect("a header bar");
        let button = find::<gtk::MenuButton>(&header, &|_| true).expect("the menu button");
        assert!(button.get_visible(), "the menu button shows");
        let model = button.menu_model().expect("a menu model");
        // One section per menu (labelled with its title), then Quit.
        let label = |i| model.item_attribute_value(i, "label", None).and_then(|v| v.get::<String>());
        assert_eq!(model.n_items(), 3);
        assert_eq!((label(0).as_deref(), label(1).as_deref(), label(2)), (Some("File"), Some("View"), None));
        let file = model.item_link(0, "section").expect("the File section");
        let attr = |i, name| file.item_attribute_value(i, name, None).and_then(|v| v.get::<String>());
        assert_eq!(attr(0, "label").as_deref(), Some("New"));
        assert_eq!(attr(0, "accel").as_deref(), Some("<Control>n"));
        let action = attr(0, "action").unwrap();
        let triggers = gtk_window
            .observe_controllers()
            .iter::<glib::Object>()
            .filter_map(|c| c.ok()?.downcast::<gtk::ShortcutController>().ok())
            .flat_map(|c| {
                c.iter::<glib::Object>().filter_map(|s| s.ok()?.downcast::<gtk::Shortcut>().ok()).collect::<Vec<_>>()
            })
            .filter_map(|s| s.trigger().map(|t| t.to_str().to_string()))
            .collect::<Vec<_>>();
        assert!(triggers.contains(&"<Control>n".to_string()), "the shortcut works in the window: {triggers:?}");

        // Disabled items don't run.
        let _ = gtk_window.activate_action(&attr(1, "action").unwrap(), None);
        gtk_window.activate_action(&action, None).expect("the action exists");
        f.ui.tick();
        assert_eq!(chosen.get(), 1, "the handler ran, and only the enabled one");
        f.ui.destroy(window);
        f.ui.tick();
    }

    pub fn alerts_are_answered_through_their_buttons(f: &Fixture) {
        let window = f.ui.create_window("alert host", Size::new(400.0, 300.0));
        f.ui.tick();
        let answer = Rc::new(RefCell::new(None));
        let a = answer.clone();
        let reply = f.ui.alert(Some(window), Alert::new("Proceed?").button("Yes").button("No"));
        f.ui.spawn_local(async move { *a.borrow_mut() = Some(reply.await) });
        pump_until(f, "the alert", || dialog_window(f).is_some());

        let dialog = dialog_window(f).unwrap();
        assert_eq!(dialog.transient_for(), f.handle.gtk_window(window), "attached to its window");
        let no =
            find::<gtk::Button>(dialog.upcast_ref(), &|b| b.label().as_deref() == Some("No")).expect("the No button");
        no.emit_clicked();
        pump_until(f, "the answer", || answer.borrow().is_some());

        assert_eq!(*answer.borrow(), Some(1));
        f.ui.destroy(window);
        f.ui.tick();
    }

    pub fn file_dialogs_report_cancellation(f: &Fixture) {
        let window = f.ui.create_window("dialog host", Size::new(600.0, 400.0));
        f.ui.tick();
        let answer = Rc::new(RefCell::new(Some(Some(vec![]))));
        let a = answer.clone();
        let reply = f.ui.open_file(Some(window), OpenFile::new().multiple());
        f.ui.spawn_local(async move { *a.borrow_mut() = Some(reply.await) });
        pump_until(f, "the file dialog", || dialog_window(f).is_some());

        dialog_window(f).unwrap().close();
        pump_until(f, "the answer", || *answer.borrow() == Some(None));
        f.ui.destroy(window);
        f.ui.tick();
    }
}

#[cfg(target_os = "linux")]
fn main() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    type Check = (&'static str, fn(&checks::Fixture));
    let checks: [Check; 4] = [
        ("clipboard_round_trips", checks::clipboard_round_trips),
        ("menus_are_installed_and_activate", checks::menus_are_installed_and_activate),
        ("alerts_are_answered_through_their_buttons", checks::alerts_are_answered_through_their_buttons),
        ("file_dialogs_report_cancellation", checks::file_dialogs_report_cancellation),
    ];
    let filter: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();
    let fixture = checks::fixture();
    let mut failed = 0;
    println!("\nrunning {} tests", checks.len());
    for (name, check) in checks {
        if !filter.is_empty() && !filter.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        let ok = catch_unwind(AssertUnwindSafe(|| check(&fixture))).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    }
    println!("\ntest result: {}. {failed} failed\n", if failed == 0 { "ok" } else { "FAILED" });
    std::process::exit(if failed == 0 { 0 } else { 101 });
}

#[cfg(not(target_os = "linux"))]
fn main() {}
