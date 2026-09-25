# mitsuami

Native, declarative, cross-platform UI for Rust: AppKit on macOS, WinUI 3 on
Windows, GTK 4 on Linux, driven by one Vue-inspired layer with CSS-style
flexbox/grid layout.

Status: **M1, M2 and M4 done**. The AppKit and GTK 4 backends run real
apps on macOS and Linux, and the same tests pass headlessly and against
native AppKit and GTK widgets. The escape hatches work on both: `platform!`
for per-platform code, `NativeView` for any `NSView` or GTK widget, and
custom widgets that are native where the platform has the control, and
built ad hoc from the platform's widgets, drawn or composed where it doesn't. WinUI 3 comes next;
writing a backend starts with [`docs/BACKENDS.md`](docs/BACKENDS.md). The design is in
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
| `mitsuami-appkit` | AppKit backend (macOS) |
| `mitsuami-gtk` | GTK 4 backend (Linux) |
| `mitsuami-winui` | WinUI 3 backend (placeholder until M3) |

## Testing

There are no unit tests. Everything is tested through public APIs.

```sh
cargo test --workspace                       # everything, headless
cargo test -p mitsuami --test layout grid    # one suite, filtered
MITSUAMI_NATIVE=1 cargo test                 # the same tests on the native backend
MITSUAMI_SHOW_WINDOWS=1 MITSUAMI_NATIVE=1 cargo test   # …and watch them
MITSUAMI_UPDATE_SNAPSHOTS=1 cargo test       # accept snapshot / visual baseline changes
MITSUAMI_WAIT_MS=5000 cargo test             # longer wait for background work in assertions
```

Tests control time (`app.advance(..)` moves the clock that `sleep` uses)
and answer dialogs through scripted services (`app.services()`), so they
never open real dialogs or touch your clipboard.

Try the example apps with `cargo run -p mitsuami --example showcase` and
`cargo run -p mitsuami --example escape_hatches`.

On Linux, building needs the GTK 4 development files (4.10 or newer), and
native tests need `gtk4-broadwayd`, GTK's in-memory display server: tests
run on a private Broadway display (with desktop portals off), so windows get
their exact sizes and dialogs stay off your desktop. `MITSUAMI_SHOW_WINDOWS=1`
puts them on your display instead.

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
