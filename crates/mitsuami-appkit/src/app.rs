//! Running an app: NSApplication, the menu bar and the run-loop hook.

use block2::RcBlock;
use mitsuami_core::Ui;
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSMenu, NSMenuItem};
use objc2_core_foundation::{CFRunLoop, CFRunLoopActivity, CFRunLoopObserver, kCFRunLoopCommonModes};
use objc2_foundation::{NSProcessInfo, NSString};

use crate::backend::{AppKitBackend, BackendOptions};

fn main_thread() -> MainThreadMarker {
    MainThreadMarker::new().expect("mitsuami: AppKit must be driven from the main thread")
}

/// Starts the app: `setup` creates the windows, then AppKit's run loop takes
/// over. Returns when the last window closes.
pub fn run(setup: impl FnOnce(&Ui)) {
    let mtm = main_thread();
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    app.setMainMenu(Some(&main_menu(mtm)));

    let backend = AppKitBackend::new(mtm, BackendOptions::default());
    let handle = backend.handle();
    let ui = Ui::new(backend);
    ui.set_commit_scheduler(|| {
        if let Some(run_loop) = CFRunLoop::main() {
            run_loop.wake_up();
        }
    });

    setup(&ui);
    ui.tick();
    handle.show_pending_windows();

    let tick = RcBlock::new(move |_: *mut CFRunLoopObserver, _: CFRunLoopActivity| {
        ui.tick();
        handle.show_pending_windows();
        if ui.windows().is_empty() {
            NSApplication::sharedApplication(main_thread()).stop(None);
        }
    });
    let observer =
        unsafe { CFRunLoopObserver::with_handler(None, CFRunLoopActivity::BeforeWaiting.0, true, 0, Some(&tick)) };
    let run_loop = CFRunLoop::main().expect("main run loop");
    // Common modes include event tracking, so live resizing relayouts too.
    run_loop.add_observer(observer.as_deref(), unsafe { kCFRunLoopCommonModes });

    app.activate();
    app.run();
}

/// Prepares AppKit for tests: no Dock icon, no menu bar takeover.
pub fn init_for_tests() -> MainThreadMarker {
    let mtm = main_thread();
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    mtm
}

fn item(mtm: MainThreadMarker, title: &str, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    }
}

fn submenu(mtm: MainThreadMarker, bar: &NSMenu, title: &str, items: Vec<Retained<NSMenuItem>>) {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for item in items {
        menu.addItem(&item);
    }
    let holder = item(mtm, title, None, "");
    holder.setSubmenu(Some(&menu));
    bar.addItem(&holder);
}

/// The standard app and Edit menus. Edit is what makes ⌘C/⌘V/⌘Z work in
/// text fields.
fn main_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let name = NSProcessInfo::processInfo().processName().to_string();
    let bar = NSMenu::new(mtm);
    submenu(
        mtm,
        &bar,
        &name,
        vec![
            item(mtm, &format!("Hide {name}"), Some(sel!(hide:)), "h"),
            NSMenuItem::separatorItem(mtm),
            item(mtm, &format!("Quit {name}"), Some(sel!(terminate:)), "q"),
        ],
    );
    submenu(
        mtm,
        &bar,
        "Edit",
        vec![
            item(mtm, "Undo", Some(sel!(undo:)), "z"),
            item(mtm, "Redo", Some(sel!(redo:)), "Z"),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Cut", Some(sel!(cut:)), "x"),
            item(mtm, "Copy", Some(sel!(copy:)), "c"),
            item(mtm, "Paste", Some(sel!(paste:)), "v"),
            item(mtm, "Select All", Some(sel!(selectAll:)), "a"),
        ],
    );
    bar
}
