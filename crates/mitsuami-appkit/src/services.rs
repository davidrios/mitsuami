//! Clipboard, dialogs and the menu bar on macOS.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use block2::RcBlock;
use mitsuami_core::NodeId;
use mitsuami_core::services::{
    Alert, AlertStyle, FileFilter, MenuBarData, MenuData, MenuEntry, OpenFile, Reply, SaveFile, Services, Shortcut,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem,
    NSModalResponse, NSModalResponseOK, NSOpenPanel, NSPasteboard, NSPasteboardTypeString, NSSavePanel, NSWindow,
};
use objc2_core_foundation::{CFRunLoop, kCFRunLoopCommonModes};
use objc2_foundation::{NSArray, NSProcessInfo, NSString, NSURL};
use objc2_uniform_type_identifiers::UTType;

use crate::backend::AppKitHandle;

pub(crate) struct MenuIvars {
    activate: Rc<dyn Fn(u32)>,
}

define_class!(
    /// Target of the app's own menu items; the item's tag is its id.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = MenuIvars]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(choose:))]
        fn choose(&self, sender: &NSMenuItem) {
            (self.ivars().activate)(sender.tag() as u32);
        }
    }

    unsafe impl NSObjectProtocol for MenuTarget {}
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker, activate: Rc<dyn Fn(u32)>) -> Retained<MenuTarget> {
        let this = MenuTarget::alloc(mtm).set_ivars(MenuIvars { activate });
        unsafe { msg_send![super(this), init] }
    }
}

pub struct AppKitServices {
    mtm: MainThreadMarker,
    backend: AppKitHandle,
    pasteboard: Retained<NSPasteboard>,
    /// Menu items only hold weak references to their target.
    menu_target: Option<Retained<MenuTarget>>,
}

fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

fn path(url: &NSURL) -> Option<PathBuf> {
    url.path().map(|p| PathBuf::from(p.to_string()))
}

fn content_types(filters: &[FileFilter]) -> Option<Retained<NSArray<UTType>>> {
    let types: Vec<Retained<UTType>> = filters
        .iter()
        .flat_map(|f| &f.extensions)
        .filter_map(|ext| UTType::typeWithFilenameExtension(&ns(ext.trim_start_matches('.'))))
        .collect();
    (!types.is_empty()).then(|| NSArray::from_retained_slice(&types))
}

/// Runs `f` on the main run loop soon, outside whatever is calling us now.
fn later(f: impl Fn() + 'static) {
    let block = RcBlock::new(f);
    if let Some(run_loop) = CFRunLoop::main() {
        unsafe { run_loop.perform_block(kCFRunLoopCommonModes.map(|m| &**m), Some(&block)) };
        run_loop.wake_up();
    }
}

/// Wraps a one-shot reply for AppKit's (reusable) completion blocks.
fn once<T>(reply: Reply<T>) -> impl Fn(T) {
    let reply = Cell::new(Some(reply));
    move |value| {
        if let Some(reply) = reply.take() {
            reply(value);
        }
    }
}

impl AppKitServices {
    pub(crate) fn new(mtm: MainThreadMarker, backend: AppKitHandle, private_clipboard: bool) -> AppKitServices {
        let pasteboard = if private_clipboard {
            NSPasteboard::pasteboardWithUniqueName()
        } else {
            NSPasteboard::generalPasteboard()
        };
        AppKitServices { mtm, backend, pasteboard, menu_target: None }
    }

    /// The window a dialog belongs to: the requested one, or the active one.
    fn parent(&self, parent: Option<NodeId>) -> Option<Retained<NSWindow>> {
        let app = NSApplication::sharedApplication(self.mtm);
        parent.and_then(|id| self.backend.ns_window(id)).or_else(|| app.keyWindow()).or_else(|| app.mainWindow())
    }
}

impl Services for AppKitServices {
    fn clipboard_text(&mut self) -> Option<String> {
        self.pasteboard.stringForType(unsafe { NSPasteboardTypeString }).map(|s| s.to_string())
    }

    fn set_clipboard_text(&mut self, text: &str) {
        self.pasteboard.clearContents();
        self.pasteboard.setString_forType(&ns(text), unsafe { NSPasteboardTypeString });
    }

    fn alert(&mut self, parent: Option<NodeId>, alert: &Alert, reply: Reply<usize>) {
        let ns_alert = NSAlert::new(self.mtm);
        ns_alert.setMessageText(&ns(&alert.title));
        if let Some(message) = &alert.message {
            ns_alert.setInformativeText(&ns(message));
        }
        ns_alert.setAlertStyle(match alert.style {
            AlertStyle::Info => NSAlertStyle::Informational,
            AlertStyle::Warning => NSAlertStyle::Warning,
            AlertStyle::Critical => NSAlertStyle::Critical,
        });
        for button in alert.effective_buttons() {
            ns_alert.addButtonWithTitle(&ns(&button));
        }
        let reply = once(reply);
        let answer = move |response: NSModalResponse| reply((response - NSAlertFirstButtonReturn).max(0) as usize);
        match self.parent(parent) {
            // A sheet on the window it belongs to.
            Some(window) => {
                let done = RcBlock::new(answer);
                ns_alert.beginSheetModalForWindow_completionHandler(&window, Some(&done));
            }
            // No window: an app-modal alert, run once the caller has returned.
            None => later(move || answer(ns_alert.runModal())),
        }
    }

    fn open_file(&mut self, parent: Option<NodeId>, request: &OpenFile, reply: Reply<Option<Vec<PathBuf>>>) {
        let panel = NSOpenPanel::openPanel(self.mtm);
        panel.setCanChooseFiles(!request.directories);
        panel.setCanChooseDirectories(request.directories);
        panel.setAllowsMultipleSelection(request.multiple);
        if let Some(title) = &request.title {
            panel.setMessage(Some(&ns(title)));
        }
        if let Some(types) = content_types(&request.filters) {
            panel.setAllowedContentTypes(&types);
        }
        let reply = once(reply);
        let chosen = panel.clone();
        let done = RcBlock::new(move |response: NSModalResponse| {
            reply((response == NSModalResponseOK).then(|| chosen.URLs().iter().filter_map(|u| path(&u)).collect()));
        });
        match self.parent(parent) {
            Some(window) => panel.beginSheetModalForWindow_completionHandler(&window, &done),
            None => panel.beginWithCompletionHandler(&done),
        }
    }

    fn save_file(&mut self, parent: Option<NodeId>, request: &SaveFile, reply: Reply<Option<PathBuf>>) {
        let panel = NSSavePanel::savePanel(self.mtm);
        if let Some(title) = &request.title {
            panel.setMessage(Some(&ns(title)));
        }
        if let Some(name) = &request.default_name {
            panel.setNameFieldStringValue(&ns(name));
        }
        if let Some(types) = content_types(&request.filters) {
            panel.setAllowedContentTypes(&types);
        }
        let reply = once(reply);
        let chosen = panel.clone();
        let done = RcBlock::new(move |response: NSModalResponse| {
            reply(if response == NSModalResponseOK { chosen.URL().and_then(|u| path(&u)) } else { None });
        });
        match self.parent(parent) {
            Some(window) => panel.beginSheetModalForWindow_completionHandler(&window, &done),
            None => panel.beginWithCompletionHandler(&done),
        }
    }

    fn set_menu(&mut self, menu: &MenuBarData, activate: Rc<dyn Fn(u32)>) {
        let target = MenuTarget::new(self.mtm, activate);
        let bar = menu_bar(self.mtm, menu, &target);
        NSApplication::sharedApplication(self.mtm).setMainMenu(Some(&bar));
        self.menu_target = Some(target);
    }
}

fn item(mtm: MainThreadMarker, title: &str, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
    unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &ns(title), action, &ns(key)) }
}

/// `auto_enable`: let AppKit enable items by responder chain (standard
/// menus); otherwise the app's `enabled` state decides.
fn submenu(mtm: MainThreadMarker, bar: &NSMenu, title: &str, items: Vec<Retained<NSMenuItem>>, auto_enable: bool) {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns(title));
    menu.setAutoenablesItems(auto_enable);
    for item in items {
        menu.addItem(&item);
    }
    let holder = item(mtm, title, None, "");
    holder.setSubmenu(Some(&menu));
    bar.addItem(&holder);
}

fn app_items(mtm: MainThreadMarker, menu: &MenuData, target: &MenuTarget) -> Vec<Retained<NSMenuItem>> {
    menu.entries
        .iter()
        .map(|entry| match entry {
            MenuEntry::Separator => NSMenuItem::separatorItem(mtm),
            MenuEntry::Item { id, title, shortcut, enabled } => {
                let key = shortcut.map(|s| s.key.to_string()).unwrap_or_default();
                let item = item(mtm, title, Some(sel!(choose:)), &key);
                unsafe { item.setTarget(Some(target as &AnyObject)) };
                item.setTag(*id as isize);
                item.setEnabled(*enabled);
                if let Some(Shortcut { primary, shift, alt, .. }) = shortcut {
                    let mut mask = NSEventModifierFlags::empty();
                    if *primary {
                        mask |= NSEventModifierFlags::Command;
                    }
                    if *shift {
                        mask |= NSEventModifierFlags::Shift;
                    }
                    if *alt {
                        mask |= NSEventModifierFlags::Option;
                    }
                    item.setKeyEquivalentModifierMask(mask);
                }
                item
            }
        })
        .collect()
}

/// The app menu, the app's File menu (if any), Edit (what makes ⌘C/⌘V/⌘Z
/// work in text fields), then the app's other menus.
fn menu_bar(mtm: MainThreadMarker, menus: &MenuBarData, target: &MenuTarget) -> Retained<NSMenu> {
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
        true,
    );
    let (file, others): (Vec<&MenuData>, Vec<&MenuData>) = menus.menus.iter().partition(|m| m.title == "File");
    for menu in file {
        submenu(mtm, &bar, &menu.title, app_items(mtm, menu, target), false);
    }
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
        true,
    );
    for menu in others {
        submenu(mtm, &bar, &menu.title, app_items(mtm, menu, target), false);
    }
    bar
}
