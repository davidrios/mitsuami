//! The real KDE services, end to end: clipboard, the global drawer menu,
//! alert dialogs and file dialogs. App tests use scripted services instead,
//! so this is where the native ones are checked. Runs on Qt's offscreen
//! platform, whose clipboard is the process's own.
//!
//! A plain `main` (harness = false): Qt has to run on the main thread.

#[cfg(target_os = "linux")]
mod checks {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use mitsuami_core::services::{Alert, Menu, MenuBar, MenuItem, OpenFile, Shortcut};
    use mitsuami_core::{Size, Ui};
    use mitsuami_kirigami::{BackendOptions, KirigamiBackend, KirigamiHandle};

    pub struct Fixture {
        pub ui: Ui,
        pub handle: KirigamiHandle,
    }

    pub fn fixture() -> Fixture {
        mitsuami_kirigami::init_for_tests();
        let backend = KirigamiBackend::new(BackendOptions::default());
        let handle = backend.handle();
        Fixture { ui: Ui::new(backend), handle }
    }

    /// Lets Qt deliver what it queued and ticks the UI, until `done`.
    fn pump_until(f: &Fixture, what: &str, done: impl Fn() -> bool) {
        let started = Instant::now();
        while !done() {
            assert!(started.elapsed() < Duration::from_secs(5), "timed out waiting for {what}");
            f.handle.pump();
            f.ui.tick();
            f.handle.show_pending_windows();
            std::thread::sleep(Duration::from_millis(5));
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
        let qml_window = f.handle.qml_window(window).unwrap();
        let drawer = qml_window.object("globalDrawer").expect("a global drawer");
        assert!(drawer.bool("isMenu"), "shown as a menu, from the toolbar");
        // One submenu per menu, then Quit.
        for title in ["File", "View", "Quit"] {
            assert!(drawer.find("text", title).is_some(), "the drawer has {title}");
        }
        let quit = drawer.find("text", "Quit").unwrap();
        assert!(!quit.str("shortcut").is_empty(), "Quit has the standard shortcut");
        let new = drawer.find("text", "New").expect("the New item");
        assert_eq!(new.str("shortcut"), "Ctrl+N");

        // Disabled items don't run.
        drawer.find("text", "Unavailable").unwrap().invoke("trigger");
        new.invoke("trigger");
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
        pump_until(f, "the alert", || !f.handle.open_dialogs().is_empty());

        let dialog = f.handle.open_dialogs()[0];
        assert_eq!(dialog.str("title"), "Proceed?");
        let overlay = f.handle.qml_window(window).unwrap().object("overlay");
        assert_eq!(dialog.object("parent"), overlay, "shown in its window");
        pump_until(f, "the dialog to open", || dialog.bool("opened"));
        dialog.find("text", "No").expect("the No button").invoke("trigger");
        pump_until(f, "the answer", || answer.borrow().is_some());

        assert_eq!(*answer.borrow(), Some(1));
        assert!(f.handle.open_dialogs().is_empty());
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
        pump_until(f, "the file dialog", || !f.handle.open_dialogs().is_empty());

        let dialog = f.handle.open_dialogs()[0];
        assert_eq!(dialog.object("parentWindow"), f.handle.qml_window(window), "attached to its window");
        dialog.invoke("reject");
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
