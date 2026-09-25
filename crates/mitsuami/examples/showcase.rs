//! A counter and a small form: `cargo run -p mitsuami --example showcase`.

use mitsuami::prelude::*;

fn counter() -> impl View {
    let count = signal(0);
    Column::new().gap(Spacing::Md).children((
        Text::new(move || format!("Count: {}", count.get())).text_style(TextStyle::Title),
        Row::new().gap(Spacing::Sm).children((
            Button::new("Increment").variant(ButtonVariant::Primary).on_click(move || count.update(|c| *c += 1)),
            Button::new("Reset").enabled(move || count.get() != 0).on_click(move || count.set(0)),
        )),
        Show::new(move || count.get() >= 5, || Text::new("That's a lot of clicks.").text_style(TextStyle::Caption)),
    ))
}

fn signup() -> impl View {
    let name = signal(String::new());
    let agreed = signal(false);
    let newsletter = signal(true);
    let submitted = signal(None::<String>);
    let submit = move || submitted.set(Some(name.get_untracked()));
    Column::new().gap(Spacing::Md).children((
        Text::new("Sign up").text_style(TextStyle::Headline),
        Grid::new()
            .columns([Track::MaxContent, Track::Size(1.fr())])
            .column_gap(Spacing::Md)
            .row_gap(Spacing::Sm)
            .align(Align::Center)
            .children((
                Text::new("Name"),
                TextInput::new().a11y_label("Name").placeholder("Ada Lovelace").bind(name).on_submit(submit),
                Text::new("Newsletter"),
                Switch::new("Newsletter").bind(newsletter),
            )),
        Checkbox::new("I agree to the terms").bind(agreed),
        Row::new().gap(Spacing::Sm).align(Align::Center).children((
            Button::new("Sign up").variant(ButtonVariant::Primary).enabled(agreed).on_click(submit),
            Text::new(move || match submitted.get() {
                Some(who) if who.is_empty() => "Signed up anonymously".to_string(),
                Some(who) => format!("Welcome, {who}!"),
                None => String::new(),
            }),
        )),
    ))
}

fn main() {
    App::new()
        .window("mitsuami showcase", Size::new(520.0, 420.0), || {
            Column::new().padding(Spacing::Xl).gap(Spacing::Xl).children((counter(), signup()))
        })
        .run();
}
