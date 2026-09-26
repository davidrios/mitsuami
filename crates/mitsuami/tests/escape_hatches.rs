//! Escape hatches: `platform!`, custom widgets (native, drawn, composed) and
//! native views. The views under test are the `escape_hatches` example's.

#[path = "../examples/escape_hatches/lock/mod.rs"]
mod lock;
#[path = "../examples/escape_hatches/pips_pager/mod.rs"]
mod pips_pager;
#[path = "../examples/escape_hatches/rating/mod.rs"]
mod rating;
#[path = "../examples/escape_hatches/screen.rs"]
mod screen;
#[path = "../examples/escape_hatches/store.rs"]
mod store;

use mitsuami::core::draw::DrawOp;
use mitsuami::core::{Color, Prop, WidgetKind};
use mitsuami::prelude::*;
use mitsuami_test::prelude::*;

use lock::{Lock, LockEvent, LockProps};
use pips_pager::{PipsPager, PipsPagerEvent, PipsPagerProps};
use rating::{Rating, RatingEvent, RatingProps};
use store::Review;

/// Mounts `view` with a fresh store and returns the store.
fn mount_with_review<V: View>(app: &TestApp, view: impl FnOnce() -> V) -> Review {
    let store = Review::create();
    app.provide(store);
    app.mount(view);
    store
}

fn rating(stars: Signal<u8>, label: &'static str) -> mitsuami::core::Custom<Rating> {
    Rating::view(move || RatingProps::new(stars.get()))
        .a11y_label(label)
        .on_event(move |RatingEvent::Changed(n)| stars.set(*n))
}

// ---------------------------------------------------------------- platform!

#[mitsuami_test::test]
fn platform_picks_the_arm_for_this_platform() {
    let os = platform! {
        macos => "macos",
        windows => "windows",
        linux => "linux",
    };
    assert_eq!(os, std::env::consts::OS);

    let unix = platform! { macos | linux => true, _ => false };
    assert_eq!(unix, cfg!(any(target_os = "macos", target_os = "linux")));

    // The first matching arm wins; `_` only catches the rest.
    let first = platform! { macos => 1, macos | linux => 2, _ => 3 };
    assert_eq!(
        first,
        if cfg!(target_os = "macos") {
            1
        } else if cfg!(target_os = "linux") {
            2
        } else {
            3
        }
    );

    // Arms may have different types; only one is compiled.
    let value = platform! { windows => 1u8, _ => "not windows" };
    let _ = value;
}

#[mitsuami_test::test]
fn platform_tells_the_linux_toolkits_apart() {
    let linux = cfg!(target_os = "linux");
    let toolkit = platform! { kde => "kde", gtk => "gtk", _ => "other" };
    let expected = match (linux, cfg!(feature = "kde")) {
        (true, true) => "kde",
        (true, false) => "gtk",
        (false, _) => "other",
    };
    assert_eq!(toolkit, expected);

    // `linux` matches either toolkit; an earlier toolkit arm wins over it.
    let either = platform! { linux => true, _ => false };
    assert_eq!(either, linux);
    let first = platform! { kde | gtk => 1, linux => 2, _ => 3 };
    assert_eq!(first, if linux { 1 } else { 3 });
}

// ----------------------------------------------------------- custom widgets

#[mitsuami_test::test]
async fn custom_widgets_bring_their_semantics(app: TestApp) {
    let stars = signal(3);
    app.mount(move || rating(stars, "Your rating"));
    let slider = app.get_by_role(Role::Slider, "Your rating");
    assert_eq!(slider.value().as_deref(), Some("3 of 5"));
    assert_eq!(slider.native_state().kind, WidgetKind::Custom("Rating"));

    stars.set(1);
    app.expect(by_role(Role::Slider, "Your rating")).to_have_value("1 of 5").await;
}

#[mitsuami_test::test]
async fn accessibility_actions_become_widget_events(app: TestApp) {
    let stars = signal(3);
    app.mount(move || rating(stars, "Your rating"));
    let slider = app.get_by_role(Role::Slider, "Your rating");

    slider.increment().await;
    assert_eq!(stars.get_untracked(), 4);
    slider.decrement().await;
    slider.decrement().await;
    assert_eq!(stars.get_untracked(), 2);
    slider.fill("5").await;
    assert_eq!(stars.get_untracked(), 5);
    // The native widget shows it too (the mirror check reads it back).
    app.expect(by_role(Role::Slider, "Your rating")).to_have_value("5 of 5").await;
}

#[mitsuami_test::test]
async fn read_only_ratings_ignore_actions(app: TestApp) {
    app.mount(|| Rating::view(|| RatingProps { value: 2, max: 5, editable: false }).a11y_label("Average"));
    let ui = app.ui().clone();
    let id = app.get_by_role(Role::Slider, "Average").id();
    assert_eq!(ui.perform(id, &A11yAction::Increment), Err(mitsuami::core::ActionError::Unsupported));
}

#[mitsuami_test::test]
async fn ratings_are_measured(app: TestApp) {
    let stars = signal(2);
    app.mount(move || Row::new().child(rating(stars, "Your rating")));
    // On macOS, the level indicator's own size; elsewhere and headless, the
    // drawn one.
    let frame = app.get_by_role(Role::Slider, "Your rating").frame();
    assert!(frame.width() > frame.height() * 3.0 && frame.height() > 0.0, "{frame}");
}

fn filled_stars(app: &TestApp, query: Query) -> usize {
    let props = app.get(query).native_state().props;
    let drawing = props
        .iter()
        .find_map(|p| match p {
            Prop::Drawing(d) => Some(d.clone()),
            _ => None,
        })
        .expect("a drawn widget shows a drawing");
    drawing.ops().iter().filter(|op| matches!(op, DrawOp::Fill { color: Color::Accent, .. })).count()
}

#[mitsuami_test::test]
async fn drawn_widgets_draw_and_handle_clicks(app: TestApp) {
    let stars = signal(2);
    app.mount(move || Row::new().child(rating(stars, "Drawn").drawn()));
    let drawn = app.get_by_role(Role::Slider, "Drawn");
    assert_eq!(filled_stars(&app, by_role(Role::Slider, "Drawn")), 2);

    // Click the middle of the fourth star.
    let height = drawn.frame().height();
    drawn.click_at(height * 1.25 * 3.0 + height / 2.0, height / 2.0).await;
    assert_eq!(stars.get_untracked(), 4);
    assert_eq!(filled_stars(&app, by_role(Role::Slider, "Drawn")), 4);

    // Actions work the same as for the native render.
    drawn.decrement().await;
    assert_eq!(filled_stars(&app, by_role(Role::Slider, "Drawn")), 3);
}

#[mitsuami_test::test]
async fn drawings_follow_the_size(app: TestApp) {
    let height = signal(20.0);
    app.mount(move || {
        Row::new()
            .child(Rating::view(|| RatingProps::new(1)).drawn().a11y_label("Drawn").height(move || height.get().px()))
    });
    let bounds = |app: &TestApp| {
        let drawing = app
            .get_by_role(Role::Slider, "Drawn")
            .native_state()
            .props
            .into_iter()
            .find_map(|p| match p {
                Prop::Drawing(d) => Some(d),
                _ => None,
            })
            .unwrap();
        format!("{:?}", drawing.ops()[0])
    };
    let before = bounds(&app);
    height.set(40.0);
    app.settle().await;
    assert_ne!(bounds(&app), before, "a new size redraws");
}

#[mitsuami_test::test]
async fn custom_widgets_are_destroyed_with_their_subtree(app: TestApp) {
    let shown = signal(true);
    app.mount(move || {
        Column::new().child(Show::new(shown, || {
            Row::new().children((Rating::view(|| RatingProps::new(1)), Rating::view(|| RatingProps::new(1)).drawn()))
        }))
    });
    let before = app.native_node_count();
    shown.set(false);
    app.settle().await;
    assert_eq!(app.native_node_count(), before - 3);
}

// ------------------------------------------------------------------- lock

fn lock(locked: Signal<bool>) -> mitsuami::core::Custom<Lock> {
    Lock::view(move || LockProps { locked: locked.get() }).a11y_label("Lock").on_event(move |event| {
        locked.set(*event == LockEvent::LockRequested);
    })
}

/// The native lock reports its state as its value; the composed stand-in, a
/// plain button, shows it in its caption: what a click would do.
async fn expect_locked(app: &TestApp, locked: bool) {
    if Lock::renderer().is_native() {
        let value = if locked { "Locked" } else { "Unlocked" };
        app.expect(by_role(Role::Button, "Lock")).to_have_value(value).await;
    } else {
        app.settle().await;
        let caption = if locked { "Unlock" } else { "Lock" };
        let props = app.get_by_role(Role::Button, "Lock").native_state().props;
        assert!(props.contains(&Prop::Label(caption.into())), "expected caption {caption:?} in {props:?}");
    }
}

#[mitsuami_test::test]
async fn locks_ask_and_the_app_decides(app: TestApp) {
    let locked = signal(true);
    let requests = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = requests.clone();
    app.mount(move || {
        // An app that says no to locking again.
        Row::new().child(Lock::view(move || LockProps { locked: locked.get() }).a11y_label("Lock").on_event(
            move |event| {
                log.borrow_mut().push(event.clone());
                if *event == LockEvent::UnlockRequested {
                    locked.set(false);
                }
            },
        ))
    });
    expect_locked(&app, true).await;

    app.get_by_role(Role::Button, "Lock").click().await;
    expect_locked(&app, false).await;
    app.get_by_role(Role::Button, "Lock").click().await;
    // Refused: still unlocked, natively too (the mirror check reads it back).
    expect_locked(&app, false).await;
    assert_eq!(*requests.borrow(), [LockEvent::UnlockRequested, LockEvent::LockRequested]);
}

#[mitsuami_test::test]
async fn composed_widgets_are_built_from_builtin_widgets(app: TestApp) {
    let locked = signal(true);
    app.mount(move || Row::new().child(lock(locked).composed()));

    // A plain button, named by the app's label, showing what a click does.
    let button = app.get_by_role(Role::Button, "Lock");
    assert_eq!(button.native_state().kind, mitsuami::core::WidgetKind::Button);
    assert!(button.native_state().props.contains(&Prop::Label("Unlock".into())));

    // Its events reach the widget's handlers, and the new props show.
    button.click().await;
    assert!(!locked.get_untracked());
    assert!(app.get_by_role(Role::Button, "Lock").native_state().props.contains(&Prop::Label("Lock".into())));
}

// ------------------------------------------------------------ pips pager

/// Drawn everywhere: `click_at` is for drawn widgets, and the pager is
/// native on Windows.
fn pager(page: Signal<u8>) -> mitsuami::core::Custom<PipsPager> {
    PipsPager::view(move || PipsPagerProps { count: 4, selected: page.get() })
        .drawn()
        .a11y_label("Page")
        .on_event(move |PipsPagerEvent::Selected(p)| page.set(*p))
}

#[mitsuami_test::test]
async fn pips_pagers_select_pages(app: TestApp) {
    let page = signal(0);
    app.mount(move || Row::new().child(pager(page)));
    let pips = app.get_by_role(Role::Slider, "Page");
    assert_eq!(pips.value().as_deref(), Some("Page 1 of 4"));

    // The third pip: cells are as wide as the pager is tall.
    let cell = pips.frame().height();
    pips.click_at(cell * 2.5, cell / 2.0).await;
    assert_eq!(page.get_untracked(), 2);
    app.expect(by_role(Role::Slider, "Page")).to_have_value("Page 3 of 4").await;

    pips.increment().await;
    assert_eq!(page.get_untracked(), 3);
    // Nothing past the last page.
    let id = pips.id();
    assert_eq!(app.ui().perform(id, &A11yAction::Increment), Err(mitsuami::core::ActionError::Unsupported));
    pips.fill("1").await;
    assert_eq!(page.get_untracked(), 0);
}

// ---------------------------------------------------------------- screen

#[mitsuami_test::test]
async fn each_platform_has_its_own_widget_native(app: TestApp) {
    mount_with_review(&app, screen::Screen::new);
    let native: Vec<String> = ["Lock (native)", "Rating (native)", "Page (native)"]
        .into_iter()
        .filter(|label| app.get_by_text(*label).exists())
        .map(String::from)
        .collect();
    let expected: &[&str] = platform! {
        macos => &["Rating (native)"],
        // The rating is built ad hoc there: not labelled native.
        kde => &["Lock (native)", "Page (native)"],
        linux => &["Lock (native)"],
        windows => &["Rating (native)", "Page (native)"],
        _ => &[],
    };
    assert_eq!(native, expected);
    assert!(app.get_by_text("Rating").exists() || cfg!(any(target_os = "macos", windows)));
}

#[mitsuami_test::test]
async fn the_screen_drives_the_store(app: TestApp) {
    let review = mount_with_review(&app, screen::Screen::new);
    app.expect(by_text("Click the lock to make changes")).to_be_visible().await;
    let rating = app.ui().perform(app.get_by_role(Role::Slider, "Rating").id(), &A11yAction::Increment);
    assert!(rating.is_err(), "locked: the rating can't change");

    app.get_by_role(Role::Button, "Lock").click().await;
    app.get_by_role(Role::Slider, "Rating").fill("4").await;
    app.get_by_role(Role::Slider, "Page").increment().await;
    app.expect(by_text(screen::NOTES[1])).to_be_visible().await;

    app.get_by_role(Role::Button, "Submit").click().await;
    assert_eq!(review.stars.get_untracked(), 4);
    app.expect(by_text("Thanks for the 4 stars!")).to_be_visible().await;
    app.assert_visual_snapshot("submitted").await;
}

// ------------------------------------------------------------- native views

#[cfg(target_os = "macos")]
mod native_views {
    use std::cell::RefCell;
    use std::rc::Rc;

    use mitsuami::appkit::NativeView;
    use mitsuami::appkit::objc2::rc::Retained;
    use mitsuami::appkit::objc2_app_kit::{NSButton, NSStepper};
    use mitsuami::appkit::objc2_foundation::NSString;
    use mitsuami::core::{Prop, WidgetKind};
    use mitsuami::prelude::*;
    use mitsuami_test::prelude::*;

    #[mitsuami_test::test]
    async fn native_views_are_created_measured_and_updated(app: TestApp) {
        let title = signal("First".to_string());
        let button: Rc<RefCell<Option<Retained<NSButton>>>> = Rc::default();
        let created = button.clone();
        app.mount(move || {
            Row::new().child(
                NativeView::appkit(move |cx| {
                    let button = NSButton::new(cx.mtm());
                    *created.borrow_mut() = Some(button.clone());
                    button
                })
                .measure(|_, _| Size::new(120.0, 30.0))
                .update(title, |button, title| button.setTitle(&NSString::from_str(title)))
                .a11y_label("Raw button"),
            )
        });
        let node = app.get_by_label("Raw button");
        assert_eq!(node.native_state().kind, WidgetKind::Native);
        assert!(node.native_state().props.iter().any(|p| matches!(p, Prop::Native(_))));
        if app.is_headless() {
            // No native view to create: an empty box.
            assert!(button.borrow().is_none());
            return;
        }
        assert_eq!(node.frame().size, Size::new(120.0, 30.0));
        let button = button.borrow().clone().expect("created natively");
        assert_eq!(button.title().to_string(), "First");
        title.set("Second".into());
        app.settle().await;
        assert_eq!(button.title().to_string(), "Second");
    }

    #[mitsuami_test::test]
    async fn native_views_report_their_events(app: TestApp) {
        let stars = signal(0u8);
        app.mount(move || {
            NativeView::appkit(|cx| {
                let stepper = NSStepper::new(cx.mtm());
                stepper.setMinValue(0.0);
                stepper.setMaxValue(5.0);
                stepper.setIncrement(1.0);
                let emitter = cx.emitter();
                cx.on_action(&*stepper, move |stepper: &NSStepper| emitter.emit(stepper.doubleValue().round() as u8));
                stepper
            })
            .update(stars, |stepper, stars| stepper.setDoubleValue(*stars as f64))
            .on_event(move |value: &u8| stars.set(*value))
            .a11y_label("Stars")
        });
        if app.is_headless() {
            return;
        }
        // Accessibility increments go to the NSStepper itself; its action
        // comes back as an event the view handles.
        app.get_by_label("Stars").increment().await;
        app.get_by_label("Stars").increment().await;
        assert_eq!(stars.get_untracked(), 2);
        app.get_by_label("Stars").decrement().await;
        assert_eq!(stars.get_untracked(), 1);
    }
}

#[cfg(all(target_os = "linux", not(feature = "kde")))]
mod native_views {
    use std::cell::RefCell;
    use std::rc::Rc;

    use mitsuami::core::{Prop, WidgetKind};
    use mitsuami::gtk::NativeView;
    use mitsuami::gtk::gtk;
    use mitsuami::gtk::gtk::prelude::*;
    use mitsuami::prelude::*;
    use mitsuami_test::prelude::*;

    #[mitsuami_test::test]
    async fn native_views_are_created_measured_and_updated(app: TestApp) {
        let title = signal("First".to_string());
        let button: Rc<RefCell<Option<gtk::Button>>> = Rc::default();
        let created = button.clone();
        app.mount(move || {
            Row::new().child(
                NativeView::gtk(move |_| {
                    let button = gtk::Button::new();
                    *created.borrow_mut() = Some(button.clone());
                    button
                })
                .measure(|_, _| Size::new(120.0, 30.0))
                .update(title, |button, title| button.set_label(title))
                .a11y_label("Raw button"),
            )
        });
        let node = app.get_by_label("Raw button");
        assert_eq!(node.native_state().kind, WidgetKind::Native);
        assert!(node.native_state().props.iter().any(|p| matches!(p, Prop::Native(_))));
        if app.is_headless() {
            // No native view to create: an empty box.
            assert!(button.borrow().is_none());
            return;
        }
        assert_eq!(node.frame().size, Size::new(120.0, 30.0));
        let button = button.borrow().clone().expect("created natively");
        assert_eq!(button.label().as_deref(), Some("First"));
        title.set("Second".into());
        app.settle().await;
        assert_eq!(button.label().as_deref(), Some("Second"));
    }

    #[mitsuami_test::test]
    async fn native_views_report_their_events(app: TestApp) {
        let stars = signal(0u8);
        app.mount(move || {
            NativeView::gtk(|cx| {
                let spin = gtk::SpinButton::with_range(0.0, 5.0, 1.0);
                let emitter = cx.emitter();
                spin.connect_value_changed(move |spin| emitter.emit(spin.value() as u8));
                spin
            })
            // Setting the value from the app is not an event.
            .update(stars, |spin, stars| spin.set_value(*stars as f64))
            .on_event(move |value: &u8| stars.set(*value))
            .a11y_label("Stars")
        });
        if app.is_headless() {
            return;
        }
        // Accessibility increments step the spin button; its signal comes
        // back as an event the view handles.
        app.get_by_label("Stars").increment().await;
        app.get_by_label("Stars").increment().await;
        assert_eq!(stars.get_untracked(), 2);
        app.get_by_label("Stars").decrement().await;
        assert_eq!(stars.get_untracked(), 1);
    }
}

#[cfg(all(target_os = "linux", feature = "kde"))]
mod native_views {
    use std::cell::Cell;
    use std::rc::Rc;

    use mitsuami::core::{Prop, WidgetKind};
    use mitsuami::kirigami::{NativeView, QmlObject};
    use mitsuami::prelude::*;
    use mitsuami_test::prelude::*;

    #[mitsuami_test::test]
    async fn native_views_are_created_measured_and_updated(app: TestApp) {
        let title = signal("First".to_string());
        let button: Rc<Cell<Option<QmlObject>>> = Rc::default();
        let created = button.clone();
        app.mount(move || {
            Row::new().child(
                NativeView::qml(move |cx| {
                    let button = cx.load("QQC2.Button { }");
                    created.set(Some(button));
                    button
                })
                .measure(|_, _| Size::new(120.0, 30.0))
                .update(title, |button, title| button.set_str("text", title))
                .a11y_label("Raw button"),
            )
        });
        let node = app.get_by_label("Raw button");
        assert_eq!(node.native_state().kind, WidgetKind::Native);
        assert!(node.native_state().props.iter().any(|p| matches!(p, Prop::Native(_))));
        if app.is_headless() {
            // No native view to create: an empty box.
            assert!(button.get().is_none());
            return;
        }
        assert_eq!(node.frame().size, Size::new(120.0, 30.0));
        let button = button.get().expect("created natively");
        assert_eq!(button.str("text"), "First");
        title.set("Second".into());
        app.settle().await;
        assert_eq!(button.str("text"), "Second");
    }

    #[mitsuami_test::test]
    async fn native_views_report_their_events(app: TestApp) {
        let stars = signal(0u8);
        app.mount(move || {
            NativeView::qml(|cx| {
                let spin = cx.load("QQC2.SpinBox { from: 0; to: 5 }");
                let emitter = cx.emitter();
                // Not `valueModified`: Qt doesn't emit it for a screen
                // reader's steps. The backend mutes its own updates.
                spin.connect("valueChanged()", move || emitter.emit(spin.int("value") as u8));
                spin
            })
            .update(stars, |spin, stars| spin.set_int("value", *stars as i32))
            .on_event(move |value: &u8| stars.set(*value))
            .a11y_label("Stars")
        });
        if app.is_headless() {
            return;
        }
        // Accessibility increments step the spin box; its signal comes back
        // as an event the view handles.
        app.get_by_label("Stars").increment().await;
        app.get_by_label("Stars").increment().await;
        assert_eq!(stars.get_untracked(), 2);
        app.get_by_label("Stars").decrement().await;
        assert_eq!(stars.get_untracked(), 1);
    }
}

#[cfg(windows)]
mod native_views {
    use std::cell::RefCell;
    use std::rc::Rc;

    use mitsuami::core::{Prop, WidgetKind};
    use mitsuami::prelude::*;
    use mitsuami::winui::NativeView;
    use mitsuami::winui::bindings::{Button, IContentControl, IPropertyValue, IRangeBase, PropertyValue, Slider};
    use mitsuami::winui::windows_core::Interface;
    use mitsuami_test::prelude::*;

    fn label(button: &Button) -> Option<String> {
        let content = button.cast::<IContentControl>().ok()?.Content().ok()?;
        content.cast::<IPropertyValue>().ok()?.GetString().ok()
    }

    #[mitsuami_test::test]
    async fn native_views_are_created_measured_and_updated(app: TestApp) {
        let title = signal("First".to_string());
        let button: Rc<RefCell<Option<Button>>> = Rc::default();
        let created = button.clone();
        app.mount(move || {
            Row::new().child(
                NativeView::xaml(move |_| {
                    let button = Button::new()?;
                    *created.borrow_mut() = Some(button.clone());
                    Ok(button)
                })
                .measure(|_, _| Size::new(120.0, 30.0))
                .update(title, |button, title| {
                    button.cast::<IContentControl>()?.SetContent(&PropertyValue::CreateString(title)?)
                })
                .a11y_label("Raw button"),
            )
        });
        let node = app.get_by_label("Raw button");
        assert_eq!(node.native_state().kind, WidgetKind::Native);
        assert!(node.native_state().props.iter().any(|p| matches!(p, Prop::Native(_))));
        if app.is_headless() {
            // No native view to create: an empty box.
            assert!(button.borrow().is_none());
            return;
        }
        assert_eq!(node.frame().size, Size::new(120.0, 30.0));
        let button = button.borrow().clone().expect("created natively");
        assert_eq!(label(&button).as_deref(), Some("First"));
        title.set("Second".into());
        app.settle().await;
        assert_eq!(label(&button).as_deref(), Some("Second"));
    }

    #[mitsuami_test::test]
    async fn native_views_report_their_events(app: TestApp) {
        let stars = signal(0u8);
        app.mount(move || {
            NativeView::xaml(|cx| {
                let slider = Slider::new()?;
                let range: IRangeBase = slider.cast()?;
                range.SetMaximum(5.0)?;
                range.SetSmallChange(1.0)?;
                let emitter = cx.emitter();
                cx.keep(range.ValueChanged(move |sender, _| {
                    let value = sender.as_ref().and_then(|s| s.cast::<IRangeBase>().ok()?.Value().ok());
                    if let Some(value) = value {
                        emitter.emit(value.round() as u8);
                    }
                })?);
                Ok(slider)
            })
            // Setting the value from the app is not an event.
            .update(stars, |slider, stars| slider.cast::<IRangeBase>()?.SetValue(*stars as f64))
            .on_event(move |value: &u8| stars.set(*value))
            .a11y_label("Stars")
        });
        if app.is_headless() {
            return;
        }
        // Accessibility increments go to the slider's RangeValue pattern;
        // its ValueChanged comes back as an event the view handles.
        app.get_by_label("Stars").increment().await;
        app.get_by_label("Stars").increment().await;
        assert_eq!(stars.get_untracked(), 2);
        app.get_by_label("Stars").decrement().await;
        assert_eq!(stars.get_untracked(), 1);
    }
}

mitsuami_test::main!();
