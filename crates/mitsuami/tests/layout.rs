//! Flexbox, grid, units, RTL and adaptive layout, checked against what a
//! browser computes for the equivalent CSS.

use mitsuami::core::Point;
use mitsuami::core::backend::PlatformMetrics;
use mitsuami::prelude::*;
use mitsuami_test::prelude::*;

fn boxed(id: &str, width: impl Into<Length>, height: impl Into<Length>) -> Container {
    Container::new().size(width, height).test_id(id)
}

async fn frame_of(app: &TestApp, id: &str) -> Rect {
    app.settle().await;
    app.get_by_test_id(id).frame()
}

#[mitsuami_test::test]
async fn columns_stack_and_stretch_by_default(app: TestApp) {
    app.mount(|| Column::new().children((boxed("a", Length::Auto, 20), boxed("b", Length::Auto, 30))));
    assert_eq!(frame_of(&app, "a").await, Rect::new(0.0, 0.0, 800.0, 20.0));
    assert_eq!(frame_of(&app, "b").await, Rect::new(0.0, 20.0, 800.0, 30.0));
}

#[mitsuami_test::test]
async fn rows_apply_padding_and_gap(app: TestApp) {
    app.mount(|| {
        Row::new().padding(10).gap(5).align(Align::Start).children((
            boxed("a", 50, 20),
            boxed("b", 50, 20),
            boxed("c", 50, 20),
        ))
    });
    assert_eq!(frame_of(&app, "a").await, Rect::new(10.0, 10.0, 50.0, 20.0));
    assert_eq!(frame_of(&app, "b").await, Rect::new(65.0, 10.0, 50.0, 20.0));
    assert_eq!(frame_of(&app, "c").await, Rect::new(120.0, 10.0, 50.0, 20.0));
}

#[mitsuami_test::test]
async fn units_resolve_against_font_viewport_parent_and_tokens(app: TestApp) {
    app.mount(|| {
        Column::new().align(Align::Start).children((
            boxed("em", 2.em(), 1.5.em()),
            boxed("rem", 3.rem(), 10),
            boxed("pct", 50.pct(), 10),
            boxed("viewport", 10.vw(), 10.vh()),
            boxed("vmin", 10.vmin(), 10.vmax()),
            boxed("token", Spacing::Md, Spacing::Xl),
        ))
    });
    // Font sizes and spacing tokens come from the platform; the window is 800×600.
    let metrics = app.ui().metrics();
    let body = metrics.font_sizes.body;
    let size = |w: f32, h: f32| Size::new(w.round(), h.round());
    assert_eq!(frame_of(&app, "em").await.size, size(2.0 * body, 1.5 * body));
    assert_eq!(frame_of(&app, "rem").await.size, size(3.0 * body, 10.0));
    assert_eq!(frame_of(&app, "pct").await.size, Size::new(400.0, 10.0));
    assert_eq!(frame_of(&app, "viewport").await.size, Size::new(80.0, 60.0));
    assert_eq!(frame_of(&app, "vmin").await.size, Size::new(60.0, 80.0));
    assert_eq!(frame_of(&app, "token").await.size, size(metrics.spacing.md, metrics.spacing.xl));
}

#[mitsuami_test::test]
async fn grid_distributes_fr_tracks_after_fixed_ones(app: TestApp) {
    app.mount(|| {
        Grid::new().width(400).columns([100.px(), 1.fr(), 2.fr()]).column_gap(0).children((
            boxed("a", Length::Auto, 20),
            boxed("b", Length::Auto, 20),
            boxed("c", Length::Auto, 20),
        ))
    });
    assert_eq!(frame_of(&app, "a").await, Rect::new(0.0, 0.0, 100.0, 20.0));
    assert_eq!(frame_of(&app, "b").await, Rect::new(100.0, 0.0, 100.0, 20.0));
    assert_eq!(frame_of(&app, "c").await, Rect::new(200.0, 0.0, 200.0, 20.0));
}

#[mitsuami_test::test]
async fn grid_items_can_span_and_be_placed(app: TestApp) {
    app.mount(|| {
        Grid::new().width(300).columns(repeat(3, 1.fr())).rows([20.px(), 20.px()]).children((
            boxed("header", Length::Auto, Length::Auto).grid_column(GridPlacement::span(3)),
            boxed("side", Length::Auto, Length::Auto).grid_column(GridPlacement::at(1)),
            boxed("main", Length::Auto, Length::Auto).grid_column(GridPlacement::at_span(2, 2)),
        ))
    });
    assert_eq!(frame_of(&app, "header").await, Rect::new(0.0, 0.0, 300.0, 20.0));
    assert_eq!(frame_of(&app, "side").await, Rect::new(0.0, 20.0, 100.0, 20.0));
    assert_eq!(frame_of(&app, "main").await, Rect::new(100.0, 20.0, 200.0, 20.0));
}

#[mitsuami_test::test]
async fn grow_fills_the_remaining_space(app: TestApp) {
    app.mount(|| Row::new().width(300).children((boxed("fixed", 100, 20), boxed("fill", Length::Auto, 20).grow(1.0))));
    assert_eq!(frame_of(&app, "fill").await, Rect::new(100.0, 0.0, 200.0, 20.0));
}

#[mitsuami_test::test]
async fn text_wraps_to_the_available_width(app: TestApp) {
    app.mount(|| {
        Row::new()
            .align(Align::Start)
            .children((Column::new().width(100).child(Text::new("hello wonderful world")), Text::new("hello")))
    });
    let wrapped = app.get_by_text("hello wonderful world").frame();
    let one_line = app.get_by_text("hello").frame();
    assert_eq!(wrapped.width(), 100.0);
    assert!(wrapped.height() >= 2.0 * one_line.height(), "{wrapped} should span several lines of {one_line}");
}

#[mitsuami_test::test(headless)]
async fn text_wraps_word_by_word_with_headless_metrics(app: TestApp) {
    app.mount(|| Column::new().width(100).child(Text::new("hello wonderful world")));
    // 8px per character: "hello" / "wonderful" / "world" on three 20px lines.
    app.expect(by_text("hello wonderful world")).to_have_frame(Rect::new(0.0, 0.0, 100.0, 60.0)).await;
}

#[mitsuami_test::test]
async fn absolute_children_are_positioned_by_inset(app: TestApp) {
    app.mount(|| Container::new().size(200, 200).child(boxed("badge", 20, 20).absolute().top(5).end(5)));
    assert_eq!(frame_of(&app, "badge").await, Rect::new(175.0, 5.0, 20.0, 20.0));
}

#[mitsuami_test::test]
async fn right_to_left_mirrors_rows_and_logical_edges(app: TestApp) {
    app.mount(|| {
        Row::new()
            .direction(TextDirection::Rtl)
            .padding_start(10)
            .align(Align::Start)
            .children((boxed("first", 50, 20), boxed("second", 50, 20)))
    });
    assert_eq!(frame_of(&app, "first").await, Rect::new(740.0, 0.0, 50.0, 20.0));
    assert_eq!(frame_of(&app, "second").await, Rect::new(690.0, 0.0, 50.0, 20.0));
}

#[mitsuami_test::test]
async fn viewport_units_follow_window_resizes(app: TestApp) {
    app.mount(|| Column::new().align(Align::Start).child(boxed("half", 50.vw(), 10.vh())));
    assert_eq!(frame_of(&app, "half").await.size, Size::new(400.0, 60.0));

    app.resize(400.0, 300.0).await;

    assert_eq!(frame_of(&app, "half").await.size, Size::new(200.0, 30.0));
}

#[mitsuami_test::test(headless)]
async fn rem_and_text_follow_the_system_text_size(app: TestApp) {
    app.mount(|| Column::new().align(Align::Start).children((boxed("rem", 10.rem(), 10), Text::new("Hi"))));
    assert_eq!(frame_of(&app, "rem").await.size.width, 160.0);

    let mut larger: PlatformMetrics = app.ui().metrics();
    larger.font_sizes.body = 20.0;
    app.headless().set_metrics(larger);
    app.settle().await;

    assert_eq!(frame_of(&app, "rem").await.size.width, 200.0);
    // 2 characters at 10px, 25px line height.
    app.expect(by_text("Hi")).to_have_frame(Rect::new(0.0, 10.0, 20.0, 25.0)).await;
}

#[mitsuami_test::test]
async fn text_styles_change_font_size(app: TestApp) {
    app.mount(|| {
        Column::new().align(Align::Start).children((Text::new("Title").text_style(TextStyle::Title), Text::new("Body")))
    });
    let title = app.get_by_text("Title").frame();
    let body = app.get_by_text("Body").frame();
    assert_eq!(title.origin, Point::new(0.0, 0.0));
    assert_eq!(body.y(), title.max_y());
    assert!(title.height() > body.height(), "title {title} should be taller than body {body}");
}

#[mitsuami_test::test]
async fn hidden_nodes_take_no_space_and_leave_the_a11y_tree(app: TestApp) {
    app.mount(|| Column::new().children((Text::new("Secret").hidden(true), boxed("after", 10, 10))));
    assert_eq!(frame_of(&app, "after").await.origin, Point::new(0.0, 0.0));
    app.expect(by_text("Secret")).not_to_exist().await;
}

mitsuami_test::main!();
