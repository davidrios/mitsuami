//! Clipboard, dialogs and menus on GTK 4.
//!
//! GNOME apps have no menu bar: the app's menus go in a menu button at the
//! end of each window's header bar (the "primary menu", F10), one section
//! per menu, plus Quit. Shortcuts work in every window. GTK's text widgets
//! bring their own Cut/Copy/Paste context menus and keybindings, so there is
//! no Edit menu to add.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use mitsuami_core::NodeId;
use mitsuami_core::services::{
    Alert, MenuBarData, MenuEntry, OpenFile, Reply, SaveFile, ServiceError, Services, Shortcut,
};

use crate::backend::{GtkHandle, WindowParts, dialog_parent, file_filters};

/// The app's menu as GTK objects, shared by every window.
pub(crate) struct MenuParts {
    model: gio::Menu,
    actions: gio::SimpleActionGroup,
    /// `(trigger, action)` pairs, e.g. `("<Control>n", "app.item-1")`.
    shortcuts: Vec<(String, String)>,
    has_app_menus: bool,
}

const GROUP: &str = "mitsuami";

impl MenuParts {
    /// Puts the menu in a window: the header bar's menu button, the actions
    /// and the keyboard shortcuts.
    pub(crate) fn install(&self, window: &WindowParts) {
        window.window.insert_action_group(GROUP, Some(&self.actions));
        window.menu_button.set_menu_model(Some(&self.model));
        window.menu_button.set_visible(self.has_app_menus);
        let controller = &window.shortcuts;
        while let Some(old) = controller.item(0).and_downcast::<gtk::Shortcut>() {
            controller.remove_shortcut(&old);
        }
        for (trigger, action) in &self.shortcuts {
            if let Some(trigger) = gtk::ShortcutTrigger::parse_string(trigger) {
                controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(gtk::NamedAction::new(action))));
            }
        }
    }
}

/// `<Control><Shift>n`, the notation of `gtk_shortcut_trigger_parse_string`.
fn trigger(shortcut: &Shortcut) -> String {
    let mut trigger = String::new();
    if shortcut.primary {
        trigger.push_str("<Control>");
    }
    if shortcut.shift {
        trigger.push_str("<Shift>");
    }
    if shortcut.alt {
        trigger.push_str("<Alt>");
    }
    trigger.push(shortcut.key);
    trigger
}

pub struct GtkServices {
    backend: GtkHandle,
    /// The last menu installed, to update enabled states in place instead
    /// of rebuilding (which would close an open menu).
    menu: Option<MenuBarData>,
}

impl GtkServices {
    pub(crate) fn new(backend: GtkHandle) -> GtkServices {
        GtkServices { backend, menu: None }
    }

    fn build_menu(&self, menu: &MenuBarData, activate: Rc<dyn Fn(u32)>) -> MenuParts {
        let model = gio::Menu::new();
        let actions = gio::SimpleActionGroup::new();
        let mut shortcuts = Vec::new();
        for app_menu in &menu.menus {
            // Separators split a menu into sections.
            let mut section_of = |entries: &[MenuEntry]| {
                let section = gio::Menu::new();
                for entry in entries {
                    if let MenuEntry::Item { id, title, shortcut, enabled } = entry {
                        let name = format!("item-{id}");
                        let action = gio::SimpleAction::new(&name, None);
                        action.set_enabled(*enabled);
                        let (activate, id) = (activate.clone(), *id);
                        action.connect_activate(move |_, _| activate(id));
                        actions.add_action(&action);
                        let item = gio::MenuItem::new(Some(title), Some(&format!("{GROUP}.{name}")));
                        if let Some(shortcut) = shortcut {
                            let trigger = trigger(shortcut);
                            item.set_attribute_value("accel", Some(&trigger.to_variant()));
                            shortcuts.push((trigger, format!("{GROUP}.{name}")));
                        }
                        section.append_item(&item);
                    }
                }
                section
            };
            let groups: Vec<&[MenuEntry]> =
                app_menu.entries.split(|e| matches!(e, MenuEntry::Separator)).filter(|g| !g.is_empty()).collect();
            for (i, group) in groups.into_iter().enumerate() {
                // The menu's title labels its first section.
                let label = (i == 0).then_some(app_menu.title.as_str());
                model.append_section(label, &section_of(group));
            }
        }
        let quit = gio::SimpleAction::new("quit", None);
        let backend = self.backend.weak();
        quit.connect_activate(move |_, _| {
            if let Some(backend) = GtkHandle::from_weak(&backend) {
                backend.request_quit();
            }
        });
        actions.add_action(&quit);
        let section = gio::Menu::new();
        let item = gio::MenuItem::new(Some("Quit"), Some(&format!("{GROUP}.quit")));
        item.set_attribute_value("accel", Some(&"<Control>q".to_variant()));
        section.append_item(&item);
        model.append_section(None, &section);
        shortcuts.push(("<Control>q".into(), format!("{GROUP}.quit")));
        MenuParts { model, actions, shortcuts, has_app_menus: !menu.menus.is_empty() }
    }
}

/// Same menus, items and shortcuts; only enabled states may differ.
fn same_structure(a: &MenuBarData, b: &MenuBarData) -> bool {
    let strip = |m: &MenuBarData| {
        m.menus
            .iter()
            .map(|menu| {
                let entries: Vec<_> = menu
                    .entries
                    .iter()
                    .map(|e| match e {
                        MenuEntry::Item { id, title, shortcut, .. } => Some((*id, title.clone(), *shortcut)),
                        MenuEntry::Separator => None,
                    })
                    .collect();
                (menu.title.clone(), entries)
            })
            .collect::<Vec<_>>()
    };
    strip(a) == strip(b)
}

/// Wraps a one-shot reply for callbacks GTK types as reusable.
fn once<T>(reply: Reply<T>) -> impl Fn(T) {
    let reply = Cell::new(Some(reply));
    move |value| {
        if let Some(reply) = reply.take() {
            reply(value);
        }
    }
}

fn path(file: &gio::File) -> Option<PathBuf> {
    file.path()
}

impl Services for GtkServices {
    fn clipboard_text(&mut self, reply: Reply<Option<String>>) {
        let Some(display) = gdk::Display::default() else { return reply(None) };
        display.clipboard().read_text_async(None::<&gio::Cancellable>, move |result| {
            reply(result.ok().flatten().map(|text| text.to_string()))
        });
    }

    fn set_clipboard_text(&mut self, text: &str, reply: Reply<Result<(), ServiceError>>) {
        match gdk::Display::default() {
            Some(display) => {
                display.clipboard().set_text(text);
                reply(Ok(()));
            }
            None => reply(Err(ServiceError::Unavailable)),
        }
    }

    fn alert(&mut self, parent: Option<NodeId>, alert: &Alert, reply: Reply<usize>) {
        let buttons = alert.effective_buttons();
        let dialog = gtk::AlertDialog::builder().message(&alert.title).modal(true).default_button(0).build();
        if let Some(message) = &alert.message {
            dialog.set_detail(message);
        }
        let labels: Vec<&str> = buttons.iter().map(String::as_str).collect();
        dialog.set_buttons(&labels);
        // Escape (or closing the dialog) chooses the last button, which by
        // convention is the least committal one (Cancel).
        let cancel = buttons.len() - 1;
        dialog.set_cancel_button(cancel as i32);
        let window = dialog_parent(&self.backend, parent);
        dialog.choose(window.as_ref(), None::<&gio::Cancellable>, move |result| {
            reply(result.map_or(cancel, |i| (i.max(0) as usize).min(cancel)))
        });
    }

    fn open_file(&mut self, parent: Option<NodeId>, request: &OpenFile, reply: Reply<Option<Vec<PathBuf>>>) {
        let dialog = gtk::FileDialog::new();
        if let Some(title) = &request.title {
            dialog.set_title(title);
        }
        if let Some(filters) = file_filters(&request.filters) {
            dialog.set_filters(Some(&filters));
        }
        let window = dialog_parent(&self.backend, parent);
        let reply = Rc::new(once(reply));
        let many =
            {
                let reply = reply.clone();
                move |result: Result<gio::ListModel, glib::Error>| {
                    reply(result.ok().map(|files| {
                        files.iter::<gio::File>().filter_map(|f| f.ok()).filter_map(|f| path(&f)).collect()
                    }))
                }
            };
        let one =
            move |result: Result<gio::File, glib::Error>| reply(result.ok().and_then(|f| path(&f)).map(|p| vec![p]));
        let cancellable = None::<&gio::Cancellable>;
        match (request.directories, request.multiple) {
            (false, false) => dialog.open(window.as_ref(), cancellable, one),
            (false, true) => dialog.open_multiple(window.as_ref(), cancellable, many),
            (true, false) => dialog.select_folder(window.as_ref(), cancellable, one),
            (true, true) => dialog.select_multiple_folders(window.as_ref(), cancellable, many),
        }
    }

    fn save_file(&mut self, parent: Option<NodeId>, request: &SaveFile, reply: Reply<Option<PathBuf>>) {
        let dialog = gtk::FileDialog::new();
        if let Some(title) = &request.title {
            dialog.set_title(title);
        }
        if let Some(name) = &request.default_name {
            dialog.set_initial_name(Some(name));
        }
        if let Some(filters) = file_filters(&request.filters) {
            dialog.set_filters(Some(&filters));
        }
        let window = dialog_parent(&self.backend, parent);
        dialog
            .save(window.as_ref(), None::<&gio::Cancellable>, move |result| reply(result.ok().and_then(|f| path(&f))));
    }

    fn set_menu(&mut self, menu: &MenuBarData, activate: Rc<dyn Fn(u32)>) {
        if let Some(installed) = &self.menu
            && same_structure(installed, menu)
            && let Some(parts) = self.backend.menu_actions()
        {
            for entry in menu.menus.iter().flat_map(|m| &m.entries) {
                if let MenuEntry::Item { id, enabled, .. } = entry
                    && let Some(action) = parts.lookup_action(&format!("item-{id}")).and_downcast::<gio::SimpleAction>()
                {
                    action.set_enabled(*enabled);
                }
            }
        } else {
            let parts = self.build_menu(menu, activate);
            self.backend.set_menu(parts);
        }
        self.menu = Some(menu.clone());
    }
}

impl GtkHandle {
    /// The installed menu's actions.
    fn menu_actions(&self) -> Option<gio::SimpleActionGroup> {
        self.with_menu(|menu| menu.actions.clone())
    }
}
