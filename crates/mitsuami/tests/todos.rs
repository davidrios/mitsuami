//! The `todos` example: its components, store, resource and action, on the
//! test clock.

#[path = "../examples/todos/screen.rs"]
mod screen;
#[path = "../examples/todos/store.rs"]
mod store;

use mitsuami::prelude::*;
use mitsuami_test::prelude::*;

use screen::{LATENCY, QUOTES, Screen};

fn todo_titles(app: &TestApp) -> Vec<String> {
    app.a11y_tree().walk().into_iter().filter(|n| n.role == Role::Checkbox).filter_map(|n| n.name.clone()).collect()
}

#[mitsuami_test::test]
async fn todos_are_added_done_hidden_and_removed(app: TestApp) {
    app.mount(Screen::new);
    assert_eq!(todo_titles(&app), ["Try view!", "Write a component", "Share a store"]);
    app.expect(by_text("3 left")).to_exist().await;

    app.get_by_label("New todo").fill("Ship M5").await;
    app.get_by_label("New todo").press(Key::Enter).await;
    app.expect(by_label("New todo")).to_have_value("").await;
    app.expect(by_text("4 left")).to_exist().await;

    app.get_by_role(Role::Checkbox, "Try view!").check().await;
    app.expect(by_text("3 left")).to_exist().await;
    app.get_by_role(Role::Switch, "Hide done").check().await;
    assert_eq!(todo_titles(&app), ["Write a component", "Share a store", "Ship M5"]);

    app.get_by_role(Role::Button, "Remove Share a store").click().await;
    app.expect(by_text("2 left")).to_exist().await;
    assert_eq!(todo_titles(&app), ["Write a component", "Ship M5"]);
}

#[mitsuami_test::test]
async fn blank_todos_cant_be_added(app: TestApp) {
    app.mount(Screen::new);
    app.expect(by_role(Role::Button, "Add")).to_be_disabled().await;
    app.get_by_label("New todo").fill("   ").await;
    app.expect(by_role(Role::Button, "Add")).to_be_disabled().await;
    app.get_by_label("New todo").press(Key::Enter).await;
    app.expect(by_text("3 left")).to_exist().await;
}

#[mitsuami_test::test]
async fn an_empty_list_says_so(app: TestApp) {
    app.mount(Screen::new);
    for title in ["Try view!", "Write a component", "Share a store"] {
        app.get_by_role(Role::Button, format!("Remove {title}")).click().await;
    }
    app.expect(by_text("Nothing to do.")).to_be_visible().await;
    app.expect(by_text("0 left")).to_exist().await;
}

#[mitsuami_test::test]
async fn the_quote_loads_and_naps_every_third_time(app: TestApp) {
    app.mount(Screen::new);
    app.expect(by_text("Loading…")).to_exist().await;
    app.advance(LATENCY).await;
    app.expect(by_text(QUOTES[0])).to_exist().await;

    app.get_by_role(Role::Button, "Another one").click().await;
    app.expect(by_role(Role::Button, "Another one")).to_be_disabled().await;
    app.advance(LATENCY).await;
    app.expect(by_text(QUOTES[1])).to_exist().await;

    // The third request fails; the last quote stays.
    app.get_by_role(Role::Button, "Another one").click().await;
    app.advance(LATENCY).await;
    app.expect(by_text("The quote server is napping.")).to_exist().await;
    app.expect(by_text(QUOTES[1])).to_exist().await;
}

#[mitsuami_test::test]
async fn syncing_is_pending_until_done(app: TestApp) {
    app.mount(Screen::new);
    app.get_by_role(Role::Button, "Sync now").click().await;
    app.expect(by_text("Syncing…")).to_exist().await;
    app.expect(by_role(Role::Button, "Sync now")).to_be_disabled().await;

    app.advance(LATENCY).await;
    app.expect(by_text("Synced 3 todos.")).to_exist().await;
    app.expect(by_role(Role::Button, "Sync now")).to_be_enabled().await;
}

#[mitsuami_test::test]
async fn screen_snapshots(app: TestApp) {
    app.mount(Screen::new);
    app.advance(LATENCY).await;
    app.assert_a11y_snapshot("loaded");
    app.assert_visual_snapshot("loaded").await;
}

mitsuami_test::main!();
