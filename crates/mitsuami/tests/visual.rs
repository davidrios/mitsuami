//! How captures are compared with their baselines: regions left out,
//! thresholds, and the same options on stories.

use mitsuami::prelude::*;
use mitsuami_test::prelude::*;

fn clock(time: Signal<&'static str>) -> impl View {
    Column::new().padding(16).gap(8).align(Align::Start).children((
        Text::new("Last synced").text_style(TextStyle::Headline),
        Text::new(move || time.get().to_owned()).test_id("clock"),
    ))
}

#[mitsuami_test::test]
async fn ignored_nodes_are_left_out_where_they_are_at_capture_time(app: TestApp) {
    let time = signal("10:00");
    app.mount(move || clock(time));
    // No allowance: outside the ignored node, any changed pixel fails.
    let options = VisualOptions::new().max_changed(0.0).ignore(by_test_id("clock"));
    app.assert_visual_snapshot_with("clock", &options).await;

    // Only the ignored node changed: the same baseline still matches.
    time.set("23:59");
    app.settle().await;
    app.assert_visual_snapshot_with("clock", &options).await;
}

#[mitsuami_test::test]
async fn ignored_regions_are_in_window_coordinates(app: TestApp) {
    let time = signal("10:00");
    app.mount(move || clock(time));
    let frame = app.get_by_test_id("clock").frame();
    let options = VisualOptions::new().max_changed(0.0).ignore_rect(frame);
    app.assert_visual_snapshot_with("region", &options).await;

    time.set("23:59");
    app.settle().await;
    app.assert_visual_snapshot_with("region", &options).await;
}

/// A story's comparison options, as `VisualOptions` takes them.
#[mitsuami_test::story(sizes = [(240, fit)], variants = [Light], threshold = 0.2, max_changed = 0.01, ignore = [by_test_id("clock")])]
fn synced_clock() -> impl View {
    clock(signal("10:00"))
}

mitsuami_test::main!();
