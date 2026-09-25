//! Platform services: clipboard, dialogs and the menu bar.
//!
//! [`Services`] is the contract each platform implements, separately from
//! the widget [`Backend`](crate::Backend). Tests install a scripted fake in
//! its place, so they never open real dialogs or touch the real clipboard.
//!
//! App code uses the async functions here (`alert(...).await`, …) or the
//! same methods on [`Ui`].

use std::cell::RefCell;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::task::{Poll, Waker};

use mitsuami_reactive::{IntoValue, Value};

use crate::ui::Ui;
use crate::widget::NodeId;

/// Delivers the user's answer. Called once, possibly long after the request.
pub type Reply<T> = Box<dyn FnOnce(T)>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlertStyle {
    #[default]
    Info,
    Warning,
    /// Destructive or irreversible consequences.
    Critical,
}

/// A message with buttons. The reply is the index of the chosen button.
#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub title: String,
    pub message: Option<String>,
    /// In order of importance: the first is the default (Return) button.
    pub buttons: Vec<String>,
    pub style: AlertStyle,
}

impl Alert {
    pub fn new(title: impl Into<String>) -> Alert {
        Alert { title: title.into(), message: None, buttons: Vec::new(), style: AlertStyle::Info }
    }

    pub fn message(mut self, message: impl Into<String>) -> Alert {
        self.message = Some(message.into());
        self
    }

    pub fn button(mut self, title: impl Into<String>) -> Alert {
        self.buttons.push(title.into());
        self
    }

    pub fn style(mut self, style: AlertStyle) -> Alert {
        self.style = style;
        self
    }

    /// The buttons to show: "OK" when none were given.
    pub fn effective_buttons(&self) -> Vec<String> {
        if self.buttons.is_empty() { vec!["OK".to_string()] } else { self.buttons.clone() }
    }
}

/// Files to offer in a file dialog, e.g. `FileFilter::new("Images", ["png", "jpg"])`.
#[derive(Clone, Debug, PartialEq)]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

impl FileFilter {
    pub fn new<S: Into<String>>(name: impl Into<String>, extensions: impl IntoIterator<Item = S>) -> FileFilter {
        FileFilter { name: name.into(), extensions: extensions.into_iter().map(Into::into).collect() }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenFile {
    pub title: Option<String>,
    pub multiple: bool,
    /// Choose folders instead of files.
    pub directories: bool,
    pub filters: Vec<FileFilter>,
}

impl OpenFile {
    pub fn new() -> OpenFile {
        OpenFile::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> OpenFile {
        self.title = Some(title.into());
        self
    }

    pub fn multiple(mut self) -> OpenFile {
        self.multiple = true;
        self
    }

    pub fn directories(mut self) -> OpenFile {
        self.directories = true;
        self
    }

    pub fn filter(mut self, filter: FileFilter) -> OpenFile {
        self.filters.push(filter);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaveFile {
    pub title: Option<String>,
    pub default_name: Option<String>,
    pub filters: Vec<FileFilter>,
}

impl SaveFile {
    pub fn new() -> SaveFile {
        SaveFile::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> SaveFile {
        self.title = Some(title.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> SaveFile {
        self.default_name = Some(name.into());
        self
    }

    pub fn filter(mut self, filter: FileFilter) -> SaveFile {
        self.filters.push(filter);
        self
    }
}

/// A keyboard shortcut. `primary` is ⌘ on macOS and Ctrl elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub key: char,
    pub primary: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Shortcut {
    /// ⌘+key on macOS, Ctrl+key elsewhere.
    pub fn primary(key: char) -> Shortcut {
        Shortcut { key: key.to_ascii_lowercase(), primary: true, shift: false, alt: false }
    }

    pub fn shift(mut self) -> Shortcut {
        self.shift = true;
        self
    }

    pub fn alt(mut self) -> Shortcut {
        self.alt = true;
        self
    }
}

/// The menu bar as data, for [`Services::set_menu`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuBarData {
    pub menus: Vec<MenuData>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuData {
    pub title: String,
    pub entries: Vec<MenuEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Item { id: u32, title: String, shortcut: Option<Shortcut>, enabled: bool },
    Separator,
}

/// What a platform provides beyond widgets.
pub trait Services {
    fn clipboard_text(&mut self) -> Option<String>;
    fn set_clipboard_text(&mut self, text: &str);

    /// Shows an alert, attached to `parent` if given (a sheet on macOS) or
    /// to the active window. Must not block: reply when the user answers.
    fn alert(&mut self, parent: Option<NodeId>, alert: &Alert, reply: Reply<usize>);
    /// Replies with the chosen paths, or `None` if cancelled.
    fn open_file(&mut self, parent: Option<NodeId>, request: &OpenFile, reply: Reply<Option<Vec<PathBuf>>>);
    fn save_file(&mut self, parent: Option<NodeId>, request: &SaveFile, reply: Reply<Option<PathBuf>>);

    /// Installs the app's menus. Platforms keep their standard menus (e.g.
    /// the macOS app and Edit menus) and call `activate` with an item's id
    /// when it is chosen.
    fn set_menu(&mut self, menu: &MenuBarData, activate: Rc<dyn Fn(u32)>);
}

// --------------------------------------------------------- menu builder

type Handler = Rc<dyn Fn()>;
/// Builds the menu data, reading reactive `enabled` states.
pub(crate) type MenuDataFn = Rc<dyn Fn() -> MenuBarData>;

/// A menu item: a title, an optional shortcut, a handler and an enabled
/// state (static or reactive).
pub struct MenuItem {
    title: String,
    shortcut: Option<Shortcut>,
    enabled: Value<bool>,
    handler: Handler,
}

impl MenuItem {
    pub fn new(title: impl Into<String>, on_select: impl Fn() + 'static) -> MenuItem {
        MenuItem { title: title.into(), shortcut: None, enabled: Value::Static(true), handler: Rc::new(on_select) }
    }

    pub fn shortcut(mut self, shortcut: Shortcut) -> MenuItem {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn enabled(mut self, enabled: impl IntoValue<bool>) -> MenuItem {
        self.enabled = enabled.into_value();
        self
    }
}

enum Entry {
    Item(MenuItem),
    Separator,
}

pub struct Menu {
    title: String,
    entries: Vec<Entry>,
}

impl Menu {
    pub fn new(title: impl Into<String>) -> Menu {
        Menu { title: title.into(), entries: Vec::new() }
    }

    pub fn item(mut self, item: MenuItem) -> Menu {
        self.entries.push(Entry::Item(item));
        self
    }

    pub fn separator(mut self) -> Menu {
        self.entries.push(Entry::Separator);
        self
    }
}

/// The app's menus. Platforms add their standard ones around them.
#[derive(Default)]
pub struct MenuBar {
    menus: Vec<Menu>,
}

impl MenuBar {
    pub fn new() -> MenuBar {
        MenuBar::default()
    }

    pub fn menu(mut self, menu: Menu) -> MenuBar {
        self.menus.push(menu);
        self
    }

    /// Splits into data (read reactively) and handlers by id.
    pub(crate) fn into_parts(self) -> (MenuDataFn, Vec<(u32, Handler)>) {
        let mut handlers = Vec::new();
        let mut next_id = 1;
        let mut menus = Vec::new();
        for menu in self.menus {
            let mut entries = Vec::new();
            for entry in menu.entries {
                match entry {
                    Entry::Item(item) => {
                        handlers.push((next_id, item.handler));
                        entries.push((next_id, item.title, item.shortcut, Some(item.enabled)));
                        next_id += 1;
                    }
                    Entry::Separator => entries.push((0, String::new(), None, None)),
                }
            }
            menus.push((menu.title, entries));
        }
        let data = Rc::new(move || MenuBarData {
            menus: menus
                .iter()
                .map(|(title, entries)| MenuData {
                    title: title.clone(),
                    entries: entries
                        .iter()
                        .map(|(id, title, shortcut, enabled)| match enabled {
                            Some(enabled) => MenuEntry::Item {
                                id: *id,
                                title: title.clone(),
                                shortcut: *shortcut,
                                enabled: enabled.get(),
                            },
                            None => MenuEntry::Separator,
                        })
                        .collect(),
                })
                .collect(),
        });
        (data, handlers)
    }
}

// ----------------------------------------------------------- async glue

struct OneShot<T> {
    value: Option<T>,
    waker: Option<Waker>,
}

/// A reply callback and the future that resolves when it is called.
pub(crate) fn reply_future<T: 'static>() -> (Reply<T>, impl Future<Output = T> + use<T>) {
    let shared = Rc::new(RefCell::new(OneShot { value: None, waker: None }));
    let sender = shared.clone();
    let reply: Reply<T> = Box::new(move |value| {
        let waker = {
            let mut shared = sender.borrow_mut();
            shared.value = Some(value);
            shared.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    });
    let future = std::future::poll_fn(move |cx| {
        let mut shared = shared.borrow_mut();
        match shared.value.take() {
            Some(value) => Poll::Ready(value),
            None => {
                shared.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    });
    (reply, future)
}

fn ui() -> Ui {
    crate::task::current_ui()
}

pub fn clipboard_text() -> Option<String> {
    ui().clipboard_text()
}

pub fn set_clipboard_text(text: &str) {
    ui().set_clipboard_text(text);
}

/// Shows an alert on the active window; resolves to the chosen button's index.
pub fn alert(alert: Alert) -> impl Future<Output = usize> {
    ui().alert(None, alert)
}

pub fn open_file(request: OpenFile) -> impl Future<Output = Option<Vec<PathBuf>>> {
    ui().open_file(None, request)
}

pub fn save_file(request: SaveFile) -> impl Future<Output = Option<PathBuf>> {
    ui().save_file(None, request)
}

/// Installs the app's menus; see [`Ui::set_menu`].
pub fn set_menu(menu: MenuBar) {
    ui().set_menu(menu);
}
