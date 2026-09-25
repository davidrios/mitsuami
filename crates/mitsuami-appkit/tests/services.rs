//! The real AppKit services, end to end: clipboard (on a private
//! pasteboard), the menu bar, alert sheets and file panels. App tests use
//! scripted services instead, so this is where the native ones are checked.
//!
//! A plain `main` (harness = false): AppKit has to run on the main thread.

#[cfg(target_os = "macos")]
mod checks {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use mitsuami_appkit::{AppKitBackend, AppKitHandle, BackendOptions};
    use mitsuami_core::services::{Alert, Menu, MenuBar, MenuItem, OpenFile, Shortcut};
    use mitsuami_core::{Size, Ui};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSApplication, NSButton, NSEventModifierFlags, NSSavePanel, NSView, NSWindow};
    use objc2_foundation::{NSDate, NSRunLoop};

    pub struct Fixture {
        pub ui: Ui,
        pub handle: AppKitHandle,
    }

    pub fn fixture() -> Fixture {
        let mtm = mitsuami_appkit::init_for_tests();
        let options = BackendOptions { show_windows: false, private_clipboard: true, ..BackendOptions::default() };
        let backend = AppKitBackend::new(mtm, options);
        let handle = backend.handle();
        Fixture { ui: Ui::new(backend), handle }
    }

    /// Lets AppKit deliver completion handlers, then ticks the UI.
    fn pump(ui: &Ui) {
        for _ in 0..10 {
            let until = NSDate::dateWithTimeIntervalSinceNow(0.02);
            NSRunLoop::currentRunLoop().runUntilDate(&until);
            ui.tick();
        }
    }

    fn find_button(view: &NSView, title: &str) -> Option<Retained<NSButton>> {
        for sub in view.subviews().iter() {
            if let Some(button) = sub.downcast_ref::<NSButton>()
                && button.title().to_string() == title
            {
                return Some(button.retain());
            }
            if let Some(found) = find_button(&sub, title) {
                return Some(found);
            }
        }
        None
    }

    fn sheet_of(window: &NSWindow) -> Retained<NSWindow> {
        window.attachedSheet().expect("a sheet is attached to the window")
    }

    use objc2::Message;

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
        f.ui.tick();
        assert_eq!(*written.borrow(), Some(Ok(())));
        assert_eq!(read.borrow().as_deref(), Some("mitsuami ✓"));
    }

    pub fn menus_are_installed_and_activate(f: &Fixture) {
        let chosen = Rc::new(Cell::new(0));
        let c = chosen.clone();
        f.ui.set_menu(
            MenuBar::new()
                .menu(
                    Menu::new("File")
                        .item(MenuItem::new("New", move || c.set(c.get() + 1)).shortcut(Shortcut::primary('n')))
                        .item(MenuItem::new("Unavailable", || {}).enabled(false)),
                )
                .menu(Menu::new("View").item(MenuItem::new("Zoom", || {}))),
        );
        let mtm = objc2::MainThreadMarker::new().unwrap();
        let bar = NSApplication::sharedApplication(mtm).mainMenu().expect("a main menu");
        let titles: Vec<String> = bar.itemArray().iter().map(|i| i.title().to_string()).collect();
        assert_eq!(&titles[1..], ["File", "Edit", "View"], "app menu, File, Edit, then the rest");

        let file = bar.itemAtIndex(1).and_then(|i| i.submenu()).expect("File submenu");
        let new = file.itemAtIndex(0).unwrap();
        assert_eq!(new.keyEquivalent().to_string(), "n");
        assert!(new.keyEquivalentModifierMask().contains(NSEventModifierFlags::Command));
        assert!(new.isEnabled());
        assert!(!file.itemAtIndex(1).unwrap().isEnabled());

        file.performActionForItemAtIndex(0);
        f.ui.tick();
        assert_eq!(chosen.get(), 1, "the handler ran");
    }

    pub fn alerts_are_answered_through_their_sheet(f: &Fixture) {
        let window = f.ui.create_window("alert host", Size::new(400.0, 300.0));
        f.ui.tick();
        let answer = Rc::new(RefCell::new(None));
        let a = answer.clone();
        let reply = f.ui.alert(Some(window), Alert::new("Proceed?").button("Yes").button("No"));
        f.ui.spawn_local(async move { *a.borrow_mut() = Some(reply.await) });
        pump(&f.ui);

        let ns_window = f.handle.ns_window(window).unwrap();
        let sheet = sheet_of(&ns_window);
        let no = find_button(&sheet.contentView().unwrap(), "No").expect("the No button");
        unsafe { no.performClick(None::<&AnyObject>) };
        pump(&f.ui);

        assert_eq!(*answer.borrow(), Some(1));
        f.ui.destroy(window);
        f.ui.tick();
    }

    pub fn open_panels_report_cancellation(f: &Fixture) {
        let window = f.ui.create_window("panel host", Size::new(600.0, 400.0));
        f.ui.tick();
        let answer = Rc::new(RefCell::new(Some(Some(vec![]))));
        let a = answer.clone();
        let reply = f.ui.open_file(Some(window), OpenFile::new().multiple());
        f.ui.spawn_local(async move { *a.borrow_mut() = Some(reply.await) });
        pump(&f.ui);

        let ns_window = f.handle.ns_window(window).unwrap();
        let sheet = sheet_of(&ns_window);
        let panel = sheet.downcast::<NSSavePanel>().expect("an open panel");
        unsafe { panel.cancel(None) };
        pump(&f.ui);

        assert_eq!(*answer.borrow(), Some(None), "cancelled");
        f.ui.destroy(window);
        f.ui.tick();
    }
}

#[cfg(target_os = "macos")]
fn main() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    type Check = (&'static str, fn(&checks::Fixture));
    let checks: [Check; 4] = [
        ("clipboard_round_trips", checks::clipboard_round_trips),
        ("menus_are_installed_and_activate", checks::menus_are_installed_and_activate),
        ("alerts_are_answered_through_their_sheet", checks::alerts_are_answered_through_their_sheet),
        ("open_panels_report_cancellation", checks::open_panels_report_cancellation),
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

#[cfg(not(target_os = "macos"))]
fn main() {}
