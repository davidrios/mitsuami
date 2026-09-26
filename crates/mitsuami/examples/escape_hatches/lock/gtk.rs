//! The lock's GTK render: `GtkLockButton` itself. It's deprecated since GTK
//! 4.10 (gone in GTK 5), but it's the real widget in every GTK 4.
//!
//! A lock button shows a `GPermission` and asks it to acquire or release.
//! Here the app owns the state, so the permission is a small one of our
//! own: it reports what the props say, and acquiring or releasing just
//! succeeds; the button's click is what the widget reports.

#![allow(deprecated)]

use std::sync::OnceLock;

use mitsuami::core::{A11yAction, ActionError};
use mitsuami::gtk::gtk::gio::ffi as gio_ffi;
use mitsuami::gtk::gtk::glib::gobject_ffi;
use mitsuami::gtk::gtk::glib::translate::FromGlib;
use mitsuami::gtk::gtk::prelude::*;
use mitsuami::gtk::gtk::{self, gio, glib};
use mitsuami::gtk::{Emitter, GtkCx, NativeRender};

use super::{Lock, LockEvent, LockProps};

// ------------------------------------------------ an app-owned GPermission

/// Acquiring and releasing succeed right away: the app has already decided
/// by the time the props say so.
unsafe extern "C" fn succeed_async(
    permission: *mut gio_ffi::GPermission,
    cancellable: *mut gio_ffi::GCancellable,
    callback: gio_ffi::GAsyncReadyCallback,
    data: glib::ffi::gpointer,
) {
    unsafe {
        let task = gio_ffi::g_task_new(permission as *mut _, cancellable, callback, data);
        gio_ffi::g_task_return_boolean(task, glib::ffi::GTRUE);
        gobject_ffi::g_object_unref(task as *mut _);
    }
}

unsafe extern "C" fn succeed_finish(
    _permission: *mut gio_ffi::GPermission,
    result: *mut gio_ffi::GAsyncResult,
    error: *mut *mut glib::ffi::GError,
) -> glib::ffi::gboolean {
    unsafe { gio_ffi::g_task_propagate_boolean(result as *mut gio_ffi::GTask, error) }
}

unsafe extern "C" fn succeed(
    _permission: *mut gio_ffi::GPermission,
    _cancellable: *mut gio_ffi::GCancellable,
    _error: *mut *mut glib::ffi::GError,
) -> glib::ffi::gboolean {
    glib::ffi::GTRUE
}

unsafe extern "C" fn class_init(class: glib::ffi::gpointer, _data: glib::ffi::gpointer) {
    let class = unsafe { &mut *(class as *mut gio_ffi::GPermissionClass) };
    class.acquire = Some(succeed);
    class.acquire_async = Some(succeed_async);
    class.acquire_finish = Some(succeed_finish);
    class.release = Some(succeed);
    class.release_async = Some(succeed_async);
    class.release_finish = Some(succeed_finish);
}

fn app_permission_type() -> glib::Type {
    static TYPE: OnceLock<glib::ffi::GType> = OnceLock::new();
    let raw = *TYPE.get_or_init(|| unsafe {
        gobject_ffi::g_type_register_static_simple(
            gio_ffi::g_permission_get_type(),
            c"MitsuamiAppPermission".as_ptr(),
            std::mem::size_of::<gio_ffi::GPermissionClass>() as u32,
            Some(class_init),
            std::mem::size_of::<gio_ffi::GPermission>() as u32,
            None,
            0,
        )
    });
    unsafe { glib::Type::from_glib(raw) }
}

fn app_permission() -> gio::Permission {
    let object = glib::Object::with_type(app_permission_type());
    object.downcast().expect("an app permission is a GPermission")
}

fn show(permission: &gio::Permission, locked: bool) {
    // Allowed when unlocked; the user can always ask for the other state.
    permission.impl_update(!locked, true, true);
}

fn permission_of(button: &gtk::LockButton) -> gio::Permission {
    button.permission().expect("the lock button has its permission")
}

// ---------------------------------------------------------------- render

impl NativeRender for Lock {
    type Widget = gtk::LockButton;

    fn create(props: &LockProps, cx: &mut GtkCx) -> gtk::LockButton {
        let permission = app_permission();
        show(&permission, props.locked);
        let button = gtk::LockButton::new(Some(&permission));
        // The button asked the permission (which just succeeded); this is
        // the request the app answers.
        let emitter = cx.emitter();
        button.connect_clicked(move |button| {
            let locked = !permission_of(button).is_allowed();
            emitter.emit(LockEvent::toggle(locked));
        });
        button
    }

    fn update(button: &gtk::LockButton, _old: &LockProps, new: &LockProps) {
        show(&permission_of(button), new.locked);
    }

    fn read(button: &gtk::LockButton, _props: &LockProps) -> LockProps {
        LockProps { locked: !permission_of(button).is_allowed() }
    }

    /// Activating is a click, as GTK's accessibility action does.
    fn perform(
        button: &gtk::LockButton,
        _props: &LockProps,
        action: &A11yAction,
        _: &Emitter,
    ) -> Result<(), ActionError> {
        match action {
            A11yAction::Activate => {
                button.emit_clicked();
                Ok(())
            }
            _ => Err(ActionError::Unsupported),
        }
    }
}
