//! The built-in widgets as stories: captured on each platform at each size,
//! in light and dark, and compared with their baselines.

use mitsuami::prelude::*;
use mitsuami_test::prelude::*;

#[mitsuami_test::story(sizes = [(340, 64)])]
fn buttons() -> impl View {
    Row::new().padding(16).gap(8).align(Align::Start).children((
        Button::new("Default"),
        Button::new("Primary").variant(ButtonVariant::Primary),
        Button::new("Disabled").enabled(false),
    ))
}

#[mitsuami_test::story(sizes = [(240, 200)])]
fn text_styles() -> impl View {
    Column::new().padding(16).gap(4).children((
        Text::new("Large title").text_style(TextStyle::LargeTitle),
        Text::new("Title").text_style(TextStyle::Title),
        Text::new("Headline").text_style(TextStyle::Headline),
        Text::new("Body"),
        Text::new("Callout").text_style(TextStyle::Callout),
        Text::new("Caption").text_style(TextStyle::Caption),
        Text::new("Monospace").text_style(TextStyle::Monospace),
    ))
}

#[mitsuami_test::story(sizes = [(200, 164)])]
fn toggles() -> impl View {
    Column::new().padding(16).gap(8).align(Align::Start).children((
        Checkbox::new("Unchecked"),
        Checkbox::new("Checked").checked(true),
        Checkbox::new("Disabled").enabled(false),
        Switch::new("Off"),
        Switch::new("On").checked(true),
    ))
}

fn signup() -> impl View {
    let agreed = signal(false);
    Column::new().padding(16).gap(8).children((
        TextInput::new().a11y_label("Name").placeholder("Your name"),
        TextInput::new().a11y_label("Email").value("ada@example.com"),
        Checkbox::new("I agree to the terms").bind(agreed),
        Row::new().justify(Justify::End).child(Button::new("Sign up").variant(ButtonVariant::Primary).enabled(agreed)),
    ))
}

/// The form at a phone's width and a window's: inputs stretch, the button
/// stays at the end.
#[mitsuami_test::story(sizes = [(280, 148), (480, 148)], play = agree)]
fn signup_ready() -> impl View {
    signup()
}

async fn agree(app: &TestApp) {
    app.get_by_role(Role::Checkbox, "I agree to the terms").click().await;
    app.expect(by_role(Role::Button, "Sign up")).to_be_enabled().await;
}

mitsuami_test::main!();
