//! Escape hatches: `platform!`, custom widgets (native, drawn, composed) and
//! native views. The views under test are the `escape_hatches` example's.

#[path = "../examples/escape_hatches/rating/mod.rs"]
mod rating;
#[path = "../examples/escape_hatches/review/mod.rs"]
mod review;
#[path = "../examples/escape_hatches/store.rs"]
mod store;

use mitsuami::core::draw::DrawOp;
use mitsuami::core::{Color, Prop, WidgetKind};
use mitsuami::prelude::*;
use mitsuami_test::prelude::*;

use rating::{Rating, RatingEvent, RatingProps};
use store::Review;

/// Mounts `view` with a fresh store and returns the store.
fn mount_with_review<V: View>(app: &TestApp, view: impl FnOnce() -> V) -> Review {
    let store = Review::new();
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
async fn native_renders_are_measured(app: TestApp) {
    let stars = signal(2);
    app.mount(move || Row::new().child(rating(stars, "Your rating")));
    // Natively, the level indicator's own size; headless, the drawn one.
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
async fn the_three_renders_stay_in_sync(app: TestApp) {
    let review = mount_with_review(&app, review::renders);
    app.get_by_role(Role::Button, "4 stars").click().await;
    assert_eq!(review.stars.get_untracked(), 4);
    app.expect(by_role(Role::Slider, "Native rating")).to_have_value("4 of 5").await;
    app.expect(by_role(Role::Slider, "Drawn rating")).to_have_value("4 of 5").await;
    assert_eq!(filled_stars(&app, by_role(Role::Slider, "Drawn rating")), 4);

    app.get_by_role(Role::Slider, "Native rating").decrement().await;
    let shows = |name: &str, star: &str| {
        app.get_by_role(Role::Button, name).native_state().props.contains(&Prop::Label(star.into()))
    };
    assert!(shows("3 stars", "★") && shows("4 stars", "☆"));
    app.assert_wireframe_snapshot("three renders");
    app.assert_visual_snapshot("three renders").await;
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

// ------------------------------------------------------------------ screens

#[mitsuami_test::test]
async fn the_shared_screen_drives_the_store(app: TestApp) {
    let review = mount_with_review(&app, review::shared::review_screen);
    app.expect(by_role(Role::Button, "Submit")).to_be_disabled().await;
    app.get_by_role(Role::Slider, "Your rating").fill("4").await;
    app.get_by_label("Comment").type_text("Nice").await;
    app.get_by_role(Role::Button, "Submit").click().await;
    assert_eq!(review.stars.get_untracked(), 4);
    app.expect(by_text("Thanks for the 4 stars: “Nice”")).to_be_visible().await;
}

/// The screen `platform!` picks: on macOS, the Mac one.
#[mitsuami_test::test]
async fn the_platform_screen_drives_the_same_store(app: TestApp) {
    let review = mount_with_review(&app, review::review_screen);
    app.get_by_role(Role::Slider, "Your rating").increment().await;
    app.get_by_role(Role::Button, "Submit").click().await;
    assert_eq!(review.stars.get_untracked(), 1);
    app.expect(by_text("Thanks for the 1 stars!")).to_be_visible().await;
    app.assert_visual_snapshot("submitted").await;
}

// ------------------------------------------------------------- native views

#[cfg(target_os = "macos")]
mod native_views {
    use std::cell::RefCell;
    use std::rc::Rc;

    use mitsuami::appkit::NativeView;
    use mitsuami::appkit::objc2::rc::Retained;
    use mitsuami::appkit::objc2_app_kit::NSButton;
    use mitsuami::appkit::objc2_foundation::NSString;
    use mitsuami::core::{Prop, WidgetKind};
    use mitsuami::prelude::*;
    use mitsuami_test::prelude::*;

    use super::{mount_with_review, review};

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
        let review = mount_with_review(&app, review::macos::stars_stepper);
        if app.is_headless() {
            return;
        }
        // Accessibility increments go to the NSStepper itself; its action
        // comes back as an event the view handles.
        app.get_by_label("Stars").increment().await;
        app.get_by_label("Stars").increment().await;
        assert_eq!(review.stars.get_untracked(), 2);
        app.get_by_label("Stars").decrement().await;
        assert_eq!(review.stars.get_untracked(), 1);
    }
}

mitsuami_test::main!();
