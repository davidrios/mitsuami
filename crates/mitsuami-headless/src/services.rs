//! Scripted platform services for tests: records every request and lets
//! the test answer it.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

use mitsuami_core::NodeId;
use mitsuami_core::services::{Alert, MenuBarData, MenuEntry, OpenFile, Reply, SaveFile, ServiceError, Services};

/// A request waiting for the test to answer it.
pub struct Pending<Request, Answer> {
    pub request: Request,
    pub parent: Option<NodeId>,
    reply: Reply<Answer>,
}

impl<Request, Answer> Pending<Request, Answer> {
    /// Answers as the user would. Call `settle` afterwards to let waiting
    /// tasks continue.
    pub fn respond(self, answer: Answer) {
        (self.reply)(answer);
    }
}

pub type PendingAlert = Pending<Alert, usize>;
pub type PendingOpen = Pending<OpenFile, Option<Vec<PathBuf>>>;
pub type PendingSave = Pending<SaveFile, Option<PathBuf>>;

#[derive(Default)]
struct State {
    clipboard: Option<String>,
    alerts: VecDeque<PendingAlert>,
    opens: VecDeque<PendingOpen>,
    saves: VecDeque<PendingSave>,
    menu: Option<MenuBarData>,
    activate: Option<Rc<dyn Fn(u32)>>,
}

/// Install with [`Ui::set_services`](mitsuami_core::Ui::set_services).
#[derive(Default)]
pub struct FakeServices {
    state: Rc<RefCell<State>>,
}

/// The test's side of [`FakeServices`].
#[derive(Clone)]
pub struct FakeServicesHandle {
    state: Rc<RefCell<State>>,
}

impl FakeServices {
    pub fn new() -> (FakeServices, FakeServicesHandle) {
        let services = FakeServices::default();
        let handle = FakeServicesHandle { state: services.state.clone() };
        (services, handle)
    }
}

impl FakeServicesHandle {
    pub fn clipboard(&self) -> Option<String> {
        self.state.borrow().clipboard.clone()
    }

    /// Puts text on the fake clipboard, as if copied from another app.
    pub fn set_clipboard(&self, text: &str) {
        self.state.borrow_mut().clipboard = Some(text.to_owned());
    }

    /// The oldest alert still waiting for an answer.
    pub fn take_alert(&self) -> Option<PendingAlert> {
        self.state.borrow_mut().alerts.pop_front()
    }

    pub fn take_open_file(&self) -> Option<PendingOpen> {
        self.state.borrow_mut().opens.pop_front()
    }

    pub fn take_save_file(&self) -> Option<PendingSave> {
        self.state.borrow_mut().saves.pop_front()
    }

    /// Requests of any kind still waiting for an answer.
    pub fn pending_requests(&self) -> usize {
        let state = self.state.borrow();
        state.alerts.len() + state.opens.len() + state.saves.len()
    }

    /// The menus the app installed.
    pub fn menu(&self) -> Option<MenuBarData> {
        self.state.borrow().menu.clone()
    }

    /// Chooses `menu › item`, like a click or its shortcut would. Returns
    /// `false` if there is no such item or it is disabled.
    pub fn choose_menu_item(&self, menu: &str, item: &str) -> bool {
        let (id, activate) = {
            let state = self.state.borrow();
            let Some(bar) = &state.menu else { return false };
            let found = bar.menus.iter().filter(|m| m.title == menu).flat_map(|m| &m.entries).find_map(|e| match e {
                MenuEntry::Item { id, title, enabled: true, .. } if title == item => Some(*id),
                _ => None,
            });
            match (found, &state.activate) {
                (Some(id), Some(activate)) => (id, activate.clone()),
                _ => return false,
            }
        };
        activate(id);
        true
    }
}

impl Services for FakeServices {
    fn clipboard_text(&mut self, reply: Reply<Option<String>>) {
        let text = self.state.borrow().clipboard.clone();
        reply(text);
    }

    fn set_clipboard_text(&mut self, text: &str, reply: Reply<Result<(), ServiceError>>) {
        self.state.borrow_mut().clipboard = Some(text.to_owned());
        reply(Ok(()));
    }

    fn alert(&mut self, parent: Option<NodeId>, alert: &Alert, reply: Reply<usize>) {
        self.state.borrow_mut().alerts.push_back(Pending { request: alert.clone(), parent, reply });
    }

    fn open_file(&mut self, parent: Option<NodeId>, request: &OpenFile, reply: Reply<Option<Vec<PathBuf>>>) {
        self.state.borrow_mut().opens.push_back(Pending { request: request.clone(), parent, reply });
    }

    fn save_file(&mut self, parent: Option<NodeId>, request: &SaveFile, reply: Reply<Option<PathBuf>>) {
        self.state.borrow_mut().saves.push_back(Pending { request: request.clone(), parent, reply });
    }

    fn set_menu(&mut self, menu: &MenuBarData, activate: Rc<dyn Fn(u32)>) {
        let mut state = self.state.borrow_mut();
        state.menu = Some(menu.clone());
        state.activate = Some(activate);
    }
}
