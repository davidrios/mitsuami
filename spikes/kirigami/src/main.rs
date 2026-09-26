//! KDE Plasma spike: a Kirigami window with Qt Quick Controls created,
//! placed, measured, driven and captured imperatively from Rust, the way
//! mitsuami's command protocol would drive them.
//!
//! `cargo run` runs the scripted checks offscreen, prints a report and exits.
//! `cargo run -- --interactive` shows the window on the desktop instead.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::time::Instant;

type Obj = *mut c_void;

unsafe extern "C" {
    fn mq_init(callback: extern "C" fn(u64, i32));
    fn mq_process_events();
    fn mq_load(qml: *const c_char, error: *mut *mut c_char) -> Obj;
    fn mq_destroy(object: Obj);
    fn mq_find_child(object: Obj, name: *const c_char) -> Obj;
    fn mq_set_parent_item(item: Obj, parent: Obj, index: i32);
    fn mq_child_count(item: Obj) -> i32;
    fn mq_child_at(item: Obj, index: i32) -> Obj;
    fn mq_set_geometry(item: Obj, x: f64, y: f64, w: f64, h: f64);
    fn mq_ensure_polished(item: Obj);
    fn mq_set_str(o: Obj, name: *const c_char, value: *const c_char);
    fn mq_get_str(o: Obj, name: *const c_char) -> *mut c_char;
    fn mq_free(s: *mut c_char);
    fn mq_set_bool(o: Obj, name: *const c_char, value: i32);
    fn mq_get_bool(o: Obj, name: *const c_char) -> i32;
    fn mq_set_real(o: Obj, name: *const c_char, value: f64);
    fn mq_get_real(o: Obj, name: *const c_char) -> f64;
    fn mq_get_object(o: Obj, name: *const c_char) -> Obj;
    fn mq_set_node(o: Obj, node: u64);
    fn mq_node_of(o: Obj) -> u64;
    fn mq_connect(o: Obj, signal: *const c_char, node: u64, event: i32) -> i32;
    fn mq_focus_item(window: Obj) -> Obj;
    fn mq_force_focus(item: Obj);
    fn mq_a11y_action(item: Obj, action: *const c_char) -> i32;
    fn mq_a11y_name(item: Obj) -> *mut c_char;
    fn mq_key(window: Obj, key: i32, text: *const c_char);
    fn mq_grab(window: Obj, rgba: *mut *mut u8, w: *mut i32, h: *mut i32, scale: *mut f64) -> i32;
    fn mq_free_pixels(rgba: *mut u8);
    fn mq_style_name() -> *mut c_char;
    fn mq_set_tab_order(window: Obj, items: *const Obj, count: i32);
    fn mq_graphics_api(window: Obj) -> *mut c_char;
}

// Event codes, as the backend would map them to UiEvents.
const CLICK: i32 = 1;
const TOGGLED: i32 = 2;
const CHECKED_CHANGED: i32 = 3;
const TEXT_EDITED: i32 = 4;
const TEXT_CHANGED: i32 = 5;
const ACCEPTED: i32 = 6;
const FOCUS: i32 = 7;
const SCROLLED: i32 = 8;
const RESIZED: i32 = 9;

// Qt::Key values.
const KEY_TAB: i32 = 0x0100_0001;
const KEY_BACKSPACE: i32 = 0x0100_0003;
const KEY_RETURN: i32 = 0x0100_0004;
const KEY_SPACE: i32 = 0x20;

thread_local! {
    static EVENTS: RefCell<Vec<(u64, i32)>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn on_event(node: u64, event: i32) {
    EVENTS.with(|e| e.borrow_mut().push((node, event)));
}

fn take_events() -> Vec<(u64, i32)> {
    EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn owned(s: *mut c_char) -> String {
    let text = unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned();
    unsafe { mq_free(s) };
    text
}

fn load(qml: &str) -> Obj {
    let mut error = std::ptr::null_mut();
    let object = unsafe { mq_load(c(qml).as_ptr(), &mut error) };
    if object.is_null() {
        panic!("QML error: {}\n{qml}", owned(error));
    }
    object
}

fn set_str(o: Obj, name: &str, value: &str) {
    unsafe { mq_set_str(o, c(name).as_ptr(), c(value).as_ptr()) }
}
fn get_str(o: Obj, name: &str) -> String {
    owned(unsafe { mq_get_str(o, c(name).as_ptr()) })
}
fn set_bool(o: Obj, name: &str, value: bool) {
    unsafe { mq_set_bool(o, c(name).as_ptr(), value as i32) }
}
fn get_bool(o: Obj, name: &str) -> bool {
    unsafe { mq_get_bool(o, c(name).as_ptr()) != 0 }
}
fn set_real(o: Obj, name: &str, value: f64) {
    unsafe { mq_set_real(o, c(name).as_ptr(), value) }
}
fn real(o: Obj, name: &str) -> f64 {
    unsafe { mq_get_real(o, c(name).as_ptr()) }
}
fn object(o: Obj, name: &str) -> Obj {
    unsafe { mq_get_object(o, c(name).as_ptr()) }
}
fn connect(o: Obj, signal: &str, node: u64, event: i32) {
    let ok = unsafe { mq_connect(o, c(signal).as_ptr(), node, event) };
    assert!(ok == 1, "no signal {signal}");
}
fn a11y(item: Obj, action: &str) -> i32 {
    unsafe { mq_a11y_action(item, c(action).as_ptr()) }
}
fn key(window: Obj, key: i32, text: &str) {
    unsafe { mq_key(window, key, c(text).as_ptr()) }
}
fn pump() {
    for _ in 0..5 {
        unsafe { mq_process_events() };
    }
}
fn implicit(item: Obj) -> (f64, f64) {
    (real(item, "implicitWidth"), real(item, "implicitHeight"))
}
fn frame(item: Obj) -> (f64, f64, f64, f64) {
    (real(item, "x"), real(item, "y"), real(item, "width"), real(item, "height"))
}

fn report(what: &str, value: impl std::fmt::Display) {
    println!("{what:<34} {value}");
}

const CONTROLS: &str = "import QtQuick\nimport QtQuick.Controls as QQC2\nimport org.kde.kirigami as Kirigami\n";

fn control(body: &str) -> Obj {
    load(&format!("{CONTROLS}{body}"))
}

const WINDOW: &str = r#"
Kirigami.ApplicationWindow {
    title: "Kirigami spike"
    pageStack.initialPage: Kirigami.Page {
        objectName: "page"
        title: "Spike"
        padding: 0
        Item { objectName: "host"; anchors.fill: parent }
    }
}
"#;

fn main() {
    let interactive = std::env::args().any(|a| a == "--interactive");
    if !interactive && std::env::var_os("QT_QPA_PLATFORM").is_none() {
        // SAFETY: single-threaded, before Qt starts.
        unsafe { std::env::set_var("QT_QPA_PLATFORM", "offscreen") };
    }
    unsafe { mq_init(on_event) };
    report("style", owned(unsafe { mq_style_name() }));
    report("platform", std::env::var("QT_QPA_PLATFORM").unwrap_or_else(|_| "(session)".into()));

    // --- Window: the content host's size, and the header above it.
    let started = Instant::now();
    let window = control(WINDOW);
    report("window created in", format!("{:?}", started.elapsed()));
    let host = unsafe { mq_find_child(window, c("host").as_ptr()) };
    assert!(!host.is_null(), "no host");
    connect(host, "widthChanged()", 1, RESIZED);
    connect(host, "heightChanged()", 1, RESIZED);
    set_real(window, "width", 400.0);
    set_real(window, "height", 300.0);
    set_bool(window, "visible", true);
    pump();
    report("window min size", format!("{} × {}", real(window, "minimumWidth"), real(window, "minimumHeight")));
    let (_, _, hw, hh) = frame(host);
    report("host at window 400 × 300", format!("{hw} × {hh}"));
    let header = 300.0 - hh;
    set_real(window, "height", 300.0 + header);
    pump();
    let (_, _, hw, hh) = frame(host);
    report("host after + header", format!("{hw} × {hh} (header {header})"));
    report("resize events", format!("{:?}", take_events()));

    // --- Metrics.
    let metrics = control(
        r#"QtObject {
            property real body: Kirigami.Theme.defaultFont.pointSize
            property real small: Kirigami.Theme.smallFont.pointSize
            property real mono: Kirigami.Theme.fixedWidthFont.pointSize
            property string family: Kirigami.Theme.defaultFont.family
            property real smallSpacing: Kirigami.Units.smallSpacing
            property real mediumSpacing: Kirigami.Units.mediumSpacing
            property real largeSpacing: Kirigami.Units.largeSpacing
            property real gridUnit: Kirigami.Units.gridUnit
            property string highlight: Kirigami.Theme.highlightColor
            property string background: Kirigami.Theme.backgroundColor
            property string text: Kirigami.Theme.textColor
        }"#,
    );
    report(
        "fonts (pt)",
        format!(
            "body {} small {} mono {} ({})",
            real(metrics, "body"),
            real(metrics, "small"),
            real(metrics, "mono"),
            get_str(metrics, "family")
        ),
    );
    report(
        "spacing",
        format!(
            "small {} medium {} large {} grid {}",
            real(metrics, "smallSpacing"),
            real(metrics, "mediumSpacing"),
            real(metrics, "largeSpacing"),
            real(metrics, "gridUnit")
        ),
    );
    report(
        "colors",
        format!(
            "highlight {} background {} text {}",
            get_str(metrics, "highlight"),
            get_str(metrics, "background"),
            get_str(metrics, "text")
        ),
    );
    for level in 1..=4 {
        let heading = control(&format!("Kirigami.Heading {{ level: {level}; text: \"Heading\" }}"));
        let (w, h) = implicit(heading);
        report(&format!("heading {level} implicit"), format!("{w} × {h}"));
        unsafe { mq_destroy(heading) };
    }

    // --- Controls: created, parented, measured before and after events.
    let kinds = [
        ("label", "QQC2.Label { wrapMode: Text.Wrap; text: \"Hello\" }"),
        ("button", "QQC2.Button { text: \"Increment\" }"),
        ("text field", "QQC2.TextField { placeholderText: \"Name\" }"),
        ("checkbox", "QQC2.CheckBox { text: \"Subscribe\" }"),
        ("switch", "QQC2.Switch { text: \"\" }"),
    ];
    let mut items = Vec::new();
    for (i, (name, qml)) in kinds.iter().enumerate() {
        let started = Instant::now();
        let item = control(qml);
        let created = started.elapsed();
        let before_parent = implicit(item);
        unsafe {
            mq_set_node(item, 10 + i as u64);
            mq_set_parent_item(item, host, i as i32);
        }
        let parented = implicit(item);
        pump();
        let after = implicit(item);
        report(
            &format!("{name} implicit"),
            format!("{before_parent:?} unparented, {parented:?} parented, {after:?} after events ({created:?})"),
        );
        items.push(item);
    }
    let [label, button, field, checkbox, switch] = items[..] else { unreachable!() };

    // Changing text: is the new implicit size there synchronously?
    set_str(button, "text", "A much longer button label");
    let sync = implicit(button);
    unsafe { mq_ensure_polished(button) };
    let polished = implicit(button);
    pump();
    report("button relabelled", format!("{sync:?} at once, {polished:?} polished, {:?} after events", implicit(button)));

    // Wrapping text: height for a width, and min-content.
    set_str(label, "text", "The quick brown fox jumps over the extraordinarily lazy dog");
    let max_content = implicit(label);
    set_real(label, "width", 120.0);
    let at_120 = (real(label, "implicitHeight"), real(label, "contentWidth"), real(label, "contentHeight"));
    set_real(label, "width", 1.0);
    let at_1 = (real(label, "contentWidth"), real(label, "contentHeight"));
    report("label max-content", format!("{max_content:?}"));
    report("label at width 120 (h, cw, ch)", format!("{at_120:?}"));
    report("label at width 1 (cw, ch)", format!("{at_1:?}"));
    set_real(label, "wrapMode", 1.0); // Text.WordWrap
    report("WordWrap at width 1 (cw, ch)", format!("{:?}", (real(label, "contentWidth"), real(label, "contentHeight"))));
    set_real(label, "wrapMode", 4.0); // Text.Wrap

    // Frames read back.
    unsafe {
        mq_set_geometry(label, 16.0, 16.0, 200.0, 40.0);
        mq_set_geometry(button, 16.0, 64.0, 220.0, 34.0);
        mq_set_geometry(field, 16.0, 110.0, 200.0, 34.0);
        mq_set_geometry(checkbox, 16.0, 156.0, 160.0, 24.0);
        mq_set_geometry(switch, 200.0, 156.0, 60.0, 24.0);
    }
    pump();
    report("button frame", format!("{:?}", frame(button)));
    let order: Vec<u64> =
        (0..unsafe { mq_child_count(host) }).map(|i| unsafe { mq_node_of(mq_child_at(host, i)) }).collect();
    report("host children (nodes)", format!("{order:?}"));
    // Insert at index 1: stacking order is children order.
    let extra = control("QQC2.Label { text: \"inserted\" }");
    unsafe {
        mq_set_node(extra, 99);
        mq_set_parent_item(extra, host, 1);
    }
    let order: Vec<u64> =
        (0..unsafe { mq_child_count(host) }).map(|i| unsafe { mq_node_of(mq_child_at(host, i)) }).collect();
    report("after insert at 1", format!("{order:?}"));
    unsafe { mq_destroy(extra) };
    report("after destroy", unsafe { mq_child_count(host) });

    // --- Events: which fire for user changes, which for programmatic ones.
    connect(button, "clicked()", 11, CLICK);
    connect(checkbox, "toggled()", 13, TOGGLED);
    connect(checkbox, "checkedChanged()", 13, CHECKED_CHANGED);
    connect(switch, "toggled()", 14, TOGGLED);
    connect(field, "textEdited()", 12, TEXT_EDITED);
    connect(field, "textChanged()", 12, TEXT_CHANGED);
    connect(field, "accepted()", 12, ACCEPTED);
    connect(window, "activeFocusItemChanged()", 1, FOCUS);

    set_bool(checkbox, "checked", true);
    set_str(field, "text", "set by code");
    report("programmatic set events", format!("{:?}", take_events()));

    for item in [button, checkbox, switch, field] {
        report("a11y", owned(unsafe { mq_a11y_name(item) }));
    }
    let r = a11y(button, "Press");
    report("a11y Press on button", format!("{r} → {:?}", take_events()));
    let r = a11y(checkbox, "Toggle");
    report("a11y Toggle on checkbox", format!("{r} → {:?} checked={}", take_events(), get_bool(checkbox, "checked")));
    let r = a11y(switch, "Toggle");
    report("a11y Toggle on switch", format!("{r} → {:?} checked={}", take_events(), get_bool(switch, "checked")));

    // --- Focus and keys.
    unsafe { mq_force_focus(field) };
    pump();
    let focus = unsafe { mq_focus_item(window) };
    report("focus after forceActiveFocus", format!("node {} events {:?}", unsafe { mq_node_of(focus) }, take_events()));
    set_real(field, "cursorPosition", get_str(field, "text").chars().count() as f64);
    for ch in "!x".chars() {
        key(window, ch as i32, &ch.to_string());
    }
    key(window, KEY_BACKSPACE, "");
    report("typed '!x' ⌫", format!("{:?} → {:?}", get_str(field, "text"), take_events()));
    key(window, KEY_RETURN, "\r");
    report("Return in field", format!("{:?}", take_events()));
    key(window, KEY_TAB, "\t");
    pump();
    let focus = unsafe { mq_focus_item(window) };
    report("after Tab", format!("node {} events {:?}", unsafe { mq_node_of(focus) }, take_events()));
    let order = [switch, button, field];
    unsafe { mq_set_tab_order(window, order.as_ptr(), 3) };
    let mut visits = Vec::new();
    for _ in 0..4 {
        key(window, KEY_TAB, "\t");
        visits.push(unsafe { mq_node_of(mq_focus_item(window)) });
    }
    take_events();
    report("Tab with order [14, 11, 12]", format!("from 13: {visits:?}"));
    unsafe { mq_force_focus(button) };
    key(window, KEY_SPACE, " ");
    pump();
    report("Space on button", format!("{:?}", take_events()));
    set_bool(button, "enabled", false);
    let r = a11y(button, "Press");
    report("a11y Press on disabled button", format!("{r} → {:?}", take_events()));
    set_bool(button, "enabled", true);

    // --- Scrolling: our content inside a ScrollView's Flickable.
    let scroll = control("QQC2.ScrollView { }");
    unsafe {
        mq_set_node(scroll, 20);
        mq_set_parent_item(scroll, host, 5);
        mq_set_geometry(scroll, 280.0, 16.0, 100.0, 120.0);
    }
    let flickable = object(scroll, "contentItem");
    let content_host = object(flickable, "contentItem");
    let content = control("Rectangle { color: \"steelblue\" }");
    unsafe {
        mq_set_node(content, 21);
        mq_set_parent_item(content, content_host, 0);
        mq_set_geometry(content, 0.0, 0.0, 100.0, 600.0);
    }
    set_real(flickable, "contentWidth", 100.0);
    set_real(flickable, "contentHeight", 600.0);
    connect(flickable, "contentYChanged()", 20, SCROLLED);
    pump();
    set_real(flickable, "contentY", 200.0);
    report("contentY set to 200", format!("{} → {:?}", real(flickable, "contentY"), take_events()));
    report("scrollview is flickable", !flickable.is_null() && !content_host.is_null());

    // --- Creation cost: many controls from the cached component.
    let started = Instant::now();
    let many: Vec<Obj> = (0..200).map(|_| control("QQC2.Button { text: \"x\" }")).collect();
    report("200 buttons created in", format!("{:?}", started.elapsed()));
    for item in many {
        unsafe { mq_destroy(item) };
    }

    // --- Capture.
    set_str(label, "text", "Count: 3");
    set_str(button, "text", "Increment");
    pump();
    let (mut pixels, mut w, mut h, mut scale) = (std::ptr::null_mut(), 0, 0, 0.0);
    let started = Instant::now();
    let ok = unsafe { mq_grab(window, &mut pixels, &mut w, &mut h, &mut scale) };
    report("graphics api", owned(unsafe { mq_graphics_api(window) }));
    report("grab", format!("ok={ok} {w} × {h} @ {scale} in {:?}", started.elapsed()));
    if ok == 1 {
        let data = unsafe { std::slice::from_raw_parts(pixels, (w * h * 4) as usize) }.to_vec();
        unsafe { mq_free_pixels(pixels) };
        let name = std::env::var("SPIKE_CAPTURE").unwrap_or_else(|_| "mitsuami-kirigami-spike".into());
        let path = std::env::temp_dir().join(format!("{name}.png"));
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(file, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&data).unwrap();
        report("capture written to", path.display());
    }

    if interactive {
        loop {
            unsafe { mq_process_events() };
            std::thread::sleep(std::time::Duration::from_millis(10));
            if !get_bool(window, "visible") {
                break;
            }
        }
    }
    unsafe { mq_destroy(window) };
}
