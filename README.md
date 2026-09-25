# mitsuami

Native, declarative, cross-platform UI for Rust: AppKit on macOS, WinUI 3 on
Windows, GTK 4 on Linux, driven by one Vue-inspired layer with CSS-style
flexbox/grid layout.

Status: **M0 done**. Core, reactivity, layout and the test harness all work
headlessly. The native backends come next. The design is in
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

```rust
use mitsuami::prelude::*;

fn counter(initial: i32) -> impl View {
    let count = signal(initial);
    Column::new().padding(16).gap(Spacing::Md).children((
        Text::new(move || format!("Count: {}", count.get())).text_style(TextStyle::Title),
        Button::new("Increment").on_click(move || count.update(|c| *c += 1)),
    ))
}
```

## Crates

| Crate | |
|---|---|
| `mitsuami` | Facade and prelude |
| `mitsuami-reactive` | Signals, computed values, effects, ownership, context |
| `mitsuami-core` | Node tree, styles and units, Taffy layout, a11y model, `Show`/`For`, backend contract |
| `mitsuami-widgets` | Built-in widgets |
| `mitsuami-headless` | In-memory backend with deterministic metrics that validates the protocol |
| `mitsuami-test` | Test runner, a11y queries, actions, assertions, snapshots |
| `mitsuami-appkit` / `-gtk` / `-winui` | Native backends (placeholders until M1–M3) |

## Testing

There are no unit tests. Everything is tested through public APIs.

```sh
cargo test --workspace                       # everything, headless
cargo test -p mitsuami --test layout grid    # one suite, filtered
MITSUAMI_UPDATE_SNAPSHOTS=1 cargo test       # accept snapshot changes
```

UI test targets use `harness = false` and `mitsuami_test::main!()`, because
native UI has to own the main thread. See `crates/mitsuami/tests/` for
examples.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
