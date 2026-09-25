# Architecture — native, declarative, cross-platform UI for Rust

> Name: **mitsuami**.
> Status: design draft, pre-MVP.

## 0. Goals and non-goals

**Goals**

- Native widgets only: AppKit (macOS), WinUI 3 (Windows), GTK 4 (Linux).
- One shared declarative layer, Vue-inspired (setup-once components, `signal`/`computed`/`watch`, props/emits/slots, provide/inject).
- Layout owned by us: flexbox + grid (CSS semantics) with abstract units (`px`, `em`, `rem`, `%`, `vw`, `vh`, `fr`, platform tokens).
- HiDPI works without app code doing anything special.
- Accessibility-ready from day one: every node carries semantics even if backends ignore them at first.
- First-class escape hatches:
  1. Platform-specific **screens** that share the same logic.
  2. Raw **native views** embedded in the shared tree.
  3. **Custom widgets** with generic logic and one render per platform.
- Platform backends are detached from the core, behind a narrow, data-oriented contract.
- Testing is first class: integration and e2e tests with one API, visual regression, and a testing toolkit shipped to app authors (§12).

**Non-goals (for now)**

- Pixel-identical rendering across platforms. We want *native feel*, not sameness.
- Arbitrary CSS styling of native controls. Visual styling is semantic (variants, text styles), not pixel-level.
- Mobile targets. The architecture shouldn't rule them out, but they aren't on the roadmap.

---

## 1. Big picture

```
┌──────────────────────────────────────────────────────────────────────┐
│  App code                                                            │
│  ┌───────────────┐   ┌──────────────────────────────────────────┐    │
│  │ Domain logic  │   │ Views (components)                        │    │
│  │ plain Rust,   │◄──│ shared views + optional per-platform ones │    │
│  │ no UI deps    │   │ view! / builder API                       │    │
│  └───────────────┘   └──────────────────────────────────────────┘    │
│          ▲ stores / composables (use_xxx) bridge the two             │
├──────────┼───────────────────────────────────────────────────────────┤
│  mitsuami-core                                                         │
│  reactive runtime · component model · node tree (source of truth)   │
│  style + unit resolution · layout (Taffy) · a11y tree · focus ·     │
│  event dispatch · scheduler (batching, flush)                        │
├──────────────────────── Backend contract ────────────────────────────┤
│   Commands (data) ──►                       ◄── Events (data)        │
│   measure(id, constraints) (sync query)     run loop / services      │
├───────────────┬──────────────────────┬───────────────┬───────────────┤
│ mitsuami-appkit │ mitsuami-winui         │ mitsuami-gtk    │ mitsuami-headless│
│ objc2         │ windows-rs + WinAppSDK│ gtk4-rs       │ integration tests│
└───────────────┴──────────────────────┴───────────────┴───────────────┘
```

"Backend" means two different things here, and they stay separate:

- **Platform backend**: the AppKit, WinUI or GTK renderer. It is detached from the core by the command/event contract (§5).
- **App backend**: the application's domain logic. It is plain Rust that knows nothing about UI. Every platform's views bind to the same logic (§8).

### Crate layout

| Crate | Responsibility |
|---|---|
| `mitsuami-reactive` | Signals, computed values, effects, watchers, scopes/ownership, batching. Single-threaded, no UI knowledge. |
| `mitsuami-core` | Node tree, components, widget kinds and props, styles and units, layout (Taffy), a11y model, focus, events, scheduler, backend trait. |
| `mitsuami-widgets` | Built-in widget definitions: typed props, events and a11y defaults. Platform-free. |
| `mitsuami-macros` | `#[component]`, `view!`, `platform!`. Pure sugar over the builder API. |
| `mitsuami-appkit` / `mitsuami-winui` / `mitsuami-gtk` | Backend implementations and their `NativeRender` traits for custom widgets. |
| `mitsuami-headless` | In-memory backend with deterministic fake measurement and wireframe rendering. The default backend for integration tests. |
| `mitsuami-test` | Public testing toolkit (§12): test runner, a11y-based queries, actions, assertions, fake clock and services, snapshots, stories, visual capture and diff. |
| `cargo-mitsuami` | CLI: `visual` (run, diff, review, accept baselines), and later scaffolding and gallery. |
| `mitsuami` | Facade crate. Re-exports everything and selects the backend strictly by `cfg(target_os)`: AppKit on macOS, WinUI 3 on Windows, GTK 4 on Linux. There is no cross-toolkit override. |

---

## 2. Rendering model: fine-grained reactivity over a retained tree

This is Vue 3 **Vapor mode**-style, closer to Solid or Leptos than to a VDOM:

- A component function runs **once** (Vue's `setup()`). It creates signals and returns a view description.
- Building the view creates **nodes** in a retained tree owned by `mitsuami-core`. Each node gets a stable `NodeId`.
- Every dynamic prop is an **effect** bound to exactly one `(NodeId, Prop)` pair. When a signal changes, only that prop is re-sent to the backend. There is no diffing of whole subtrees.
- Structural changes go through control-flow primitives: `Show` (v-if), `For` with keys (v-for, keyed reconciliation), `Switch` / `Match` and `Dynamic` (`<component :is>`).

Why not a VDOM? Native widgets are expensive to create and have their own internal state (focus, selection, scroll position, IME). Fine-grained updates mutate them surgically and never recreate them by accident.

### The node tree is the source of truth

```rust
struct Node {
    id: NodeId,
    kind: WidgetKind,          // Button, Text, View, …, Custom(&'static str), Native
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    style: ResolvedStyle,      // layout + semantic visual props
    layout: taffy::NodeId,
    a11y: A11yProps,           // always present, see §7
    handlers: EventHandlers,
    frame: Rect,               // last computed layout, logical units
}
```

Layout, a11y, focus order, hit testing for custom widgets and tests all read this tree. The native tree is a **mirror** of it.

### Update pipeline (one "tick")

1. Something happens: a native event, a timer, or a task completes on the UI thread.
2. Handlers run and mutate signals. Mutations are **batched**.
3. Effects flush. Each queues a `Command` (prop updates, inserts, removals) and marks layout dirty where needed.
4. Taffy computes layout for the dirty roots. It calls back into the backend for **native measurement** of leaf widgets.
5. Frame diffs become `SetFrame` commands.
6. The backend applies the whole batch in one transaction (`CATransaction`, a single dispatcher pass, or GTK's frame clock).

The backend schedules the flush through the platform run loop: a `CFRunLoopObserver` (before waiting) on macOS, a `DispatcherQueue` on Windows, and a GLib idle source or frame clock on Linux.

---

## 3. Layout and units

### Decision: we own layout; native widgets are positioned absolutely

This is the React Native / Yoga model:

- Every container (`View`, `Row`, `Column`, `Grid`, `Stack`) maps to a thin native **layout host** that does no layout of its own:
  - macOS: a flipped `NSView` subclass.
  - WinUI: a `Canvas`. A custom `Panel` would need composable-type subclassing from Rust, which is hard.
  - GTK: a `gtk::Widget` subclass whose `size_allocate` places children at our computed frames.
- Leaf widgets report their **intrinsic size** through `measure(id, known, available)`:
  - macOS: `fittingSize` / `intrinsicContentSize`, plus `preferredMaxLayoutWidth` for wrapping text.
  - WinUI: `FrameworkElement.Measure`.
  - GTK: `gtk_widget_measure`.
- [Taffy](https://github.com/DioxusLabs/taffy) computes flexbox, grid and block layout, using those sizes for leaves.

Pros: identical layout semantics on all three platforms, CSS mental model, and one layout engine to test headlessly.

Cons: we give up native auto-layout behaviours, and we must handle RTL mirroring ourselves (§7). Measurement calls cross into native code synchronously, so we cache them per `(node, constraints, content version)`.

`ScrollView` is the exception at the edges. The native scroll container scrolls, and we lay out its content.

### Units

```rust
enum Length {
    Px(f32),        // logical px = DIP = point. NOT physical pixels.
    Em(f32),        // relative to the node's inherited font size
    Rem(f32),       // relative to the platform body font size (follows user text-size settings)
    Percent(f32),   // of the containing block (Taffy-native)
    Vw(f32), Vh(f32), Vmin(f32), Vmax(f32),  // of the window's content area
    Fr(f32),        // grid tracks only
    Token(Spacing), // platform design tokens: Spacing::{Xs,Sm,Md,Lg,Xl}, ControlHeight, …
    Auto,
}
```

- **HiDPI is handled for free.** All three toolkits already work in logical units (points, effective pixels, GTK logical px × scale). We never touch physical pixels, and a scale change needs no relayout.
- `em`/`rem`/`vw`/`vh`/`Token` are resolved in core before handing styles to Taffy, which only knows length, percent and auto. Core tracks which nodes depend on the viewport or on font size, so a window resize or a system text-size change re-resolves only those nodes.
- `Token` gives platform-appropriate spacing. For example, `Spacing::Md` can be 8pt on macOS, 12 epx on WinUI and 6/12px on GNOME. The backend provides the values through `PlatformMetrics`.
- Ergonomics: `16.px()`, `1.5.em()`, `50.pct()`, `100.vw()`, `1.fr()`, `Spacing::Md`.

### Responsive / adaptive

- `use_viewport()` returns a signal of the window size, like media queries.
- `use_container_size(node_ref)` works like container queries.
- `Platform::current()` is also available at runtime, but compile-time `platform!` (§6) is preferred.

---

## 4. Styling (what can and can't be styled)

A style has two halves:

1. **Layout:** the full flex and grid property set (`direction`, `gap`, `padding`, `margin`, `align_*`, `justify_*`, `grow`, `shrink`, `basis`, `grid_template_*`, `grid_area`, `position`, `inset`, `min/max/size`, `aspect_ratio`, `overflow`). It applies to every node.
2. **Semantic visual:**
   - Text styles: `TextStyle::{LargeTitle, Title, Headline, Body, Callout, Caption, Monospace}`. These map to `NSFont.preferredFont(forTextStyle:)`, the WinUI type ramp, and GTK/libadwaita style classes.
   - Control variants: `ButtonVariant::{Default, Primary, Destructive, Plain}`. These map to `keyEquivalent "\r"` or the bezel style on macOS, `AccentButtonStyle` on WinUI, and `suggested-action` / `destructive-action` on GTK.
   - Semantic colors (`Color::Label`, `Color::SecondaryLabel`, `Color::Accent`, `Color::Separator`, …) that follow dark mode and high contrast.
   - Containers (layout hosts) can also take a background, border, corner radius and opacity, because they are plain views.

Raw RGB is allowed on containers and text. It is deliberately not exposed on native controls.

---

## 5. Backend contract

The core speaks **only in `NodeId`s and plain data**. Each backend keeps its own `NodeId → native handle` map, so core has no generic parameters and no `dyn Any` handles.

```rust
// As implemented in M0 (crates/mitsuami-core/src/{command,backend}.rs).
pub enum Command {
    Create        { id: NodeId, kind: WidgetKind, props: Vec<Prop> }, // windows are nodes too (WidgetKind::Window)
    SetProp       { id: NodeId, prop: Prop },
    Insert        { parent: NodeId, child: NodeId, index: usize },
    Remove        { parent: NodeId, child: NodeId },
    Destroy       { id: NodeId },       // every native node of a removed subtree, children first
    SetFrame      { id: NodeId, frame: Rect },          // parent-relative, logical units; never for windows
    SetA11y       { id: NodeId, a11y: A11yProps },      // backends may no-op initially
    SetWindowSize { id: NodeId, size: Size },
    Focus         { id: NodeId },
}

pub trait Backend {
    fn init(&mut self, events: EventSink);
    fn metrics(&self) -> PlatformMetrics;             // fonts, spacing tokens, scale, color scheme, reduced motion…
    fn apply(&mut self, batch: &[Command]);
    fn measure(&mut self, id: NodeId, request: MeasureRequest) -> Size;

    // Testing and accessibility hooks: part of the contract from day one.
    fn perform(&mut self, id: NodeId, action: &A11yAction) -> Result<(), ActionError>;     // act on the native control
    fn synthesize(&mut self, id: NodeId, input: &SyntheticInput) -> Result<(), ActionError>; // raw key input
    fn native_state(&self, id: NodeId) -> Option<NativeState>; // what the widget actually shows (desync checks)
    fn capture(&mut self, id: NodeId) -> Result<Image, CaptureError>; // offscreen screenshot
}
// Still to come: `services()` (dialogs, menus, clipboard, …) and the run
// loop entry point, both with M1.
```

Properties of this contract:

- **Serializable and inspectable.** Commands can be logged, snapshot-tested and replayed. They could even be sent to a devtools inspector later.
- **The headless backend is trivial to write.** It makes layout, trees and a11y fully testable in CI without a display, and it is the default backend for integration tests (§12).
- **Controlled inputs** (`v-model`) avoid feedback loops: the backend emits `Changed(text)`, core updates the signal, and the effect sends `SetProp` back only if the value differs from what the native widget already holds. This preserves the cursor, selection and IME composition.
- **Threading:** all UI work runs on the main thread, and the reactive runtime is `!Send`. Background work uses `spawn` (a thread pool or async runtime) and hands results back through a `MainThreadDispatcher` that each backend implements on its run loop. `spawn_local` runs futures on the UI thread.

---

## 6. Escape hatches

### 6.1 Platform-specific screens with shared behaviour

Logic lives in **composables** (Vue's `useXxx`) or **stores** (Pinia-like). Views are thin, so writing two views costs little.

```rust
// shared, platform-agnostic
pub fn use_preferences(store: &Settings) -> PreferencesModel { /* signals + actions */ }

#[component]
pub fn Preferences() -> impl View {
    let m = use_preferences(&use_store::<Settings>());
    platform! {
        macos => mac::PreferencesWindow(m),   // toolbar-tabbed prefs window, macOS style
        _     => PreferencesPage(m),          // shared Windows/Linux version
    }
}
```

- `platform!` is compile-time (`cfg`), so code for other platforms is never compiled into the binary.
- Arms can be `macos`, `windows`, `linux`, or groups like `desktop_unix`, plus `_`.
- Per-platform view files follow a convention: `preferences.rs`, `preferences.macos.rs`, and so on.

### 6.2 Raw native view inside the shared tree

```rust
#[cfg(target_os = "macos")]
NativeView::appkit(|cx: &mut AppKitCx| -> Retained<NSView> { /* build anything */ })
    .measure(|view, constraints| /* optional */)
    .a11y(A11yProps::group("Map"))
```

It takes part in layout, focus and a11y like any other node. Core sees `WidgetKind::Native`.

### 6.3 Custom widgets: generic logic plus one render per platform

Custom widgets come in three tiers. Pick the lowest that works:

1. **Composition:** a component built from existing widgets. It runs everywhere.
2. **Drawn:** a generic `Canvas` render using a small 2D API over CoreGraphics, Direct2D/Win2D and Cairo/GSK, with platform theme tokens. It runs everywhere and follows platform colours and metrics, but isn't truly native.
3. **Native per platform:** one shared definition plus one render per platform.

```rust
// Shared definition — platform free.
pub struct Rating;
impl CustomWidget for Rating {
    const NAME: &'static str = "Rating";
    type Props = RatingProps;                 // { value: f32, max: u8, editable: bool }
    type Event = RatingEvent;                 // Changed(f32)
    fn a11y(p: &Self::Props) -> A11yProps {   // semantics are shared
        A11yProps::slider("Rating").value(p.value).range(0.0, p.max as f32)
    }
    fn fallback() -> Option<Fallback<Self>> { Some(Fallback::Drawn(draw_stars)) }  // optional tier 1/2
}

// One impl per platform, each under cfg. They live in the same crate, e.g. rating/macos.rs.
#[cfg(target_os = "macos")]
impl NativeRender for Rating {                 // trait defined by mitsuami-appkit
    type View = NSLevelIndicator;
    fn create(p: &RatingProps, cx: &mut AppKitCx<RatingEvent>) -> Retained<Self::View>;
    fn update(v: &Self::View, old: &RatingProps, new: &RatingProps);
    fn measure(v: &Self::View, c: Constraints) -> Option<Size> { None } // None = use native
}
#[cfg(target_os = "windows")] impl NativeRender for Rating { /* RatingControl */ }
#[cfg(target_os = "linux")]   impl NativeRender for Rating { /* gtk::Box + toggles, or Drawn */ }
```

- A usage site is `Rating::view(props).on(RatingEvent::Changed, …)`. It is the same on every platform.
- **Compile-time coverage:** if a platform has neither a `NativeRender` impl nor a fallback, building for that platform fails. You can't ship a missing render by accident.
- The backend registers renders by `NAME`. Commands carry `WidgetKind::Custom(NAME)` and the props as a boxed `dyn Any`. Custom props don't need to be serializable, though they can opt in.

---

## 7. Accessibility and i18n affordances (designed in now, implemented later)

- Every node carries `A11yProps`: role, label, description, value/range, state flags (disabled, checked, expanded, selected, busy), `labelled_by`/`described_by` relations, live-region politeness, and supported actions.
- Built-in widgets derive defaults: a `Button`'s label comes from its text, and so on. Apps override with `.a11y_label("…")`, and so on.
- Most of the work comes for free because the controls are **native**: NSAccessibility, UIA and GtkAccessible already understand native controls. `SetA11y` mainly carries overrides and relations.
- Drawn and custom widgets are where real work is needed. The plan is to implement native a11y protocols per backend. [AccessKit](https://github.com/AccessKit/accesskit) is an option for drawn subtrees, because it provides the same semantic model on all three platforms.
- Core owns **focus order** (tab order follows tree order, with overrides) and keyboard navigation for layout hosts.
- `PlatformMetrics` exposes reduced motion, high contrast, text scale and color scheme as signals.
- **RTL:** styles use logical edges (`padding_inline_start`, not `padding_left`). Core mirrors frames for RTL locales, since Taffy doesn't.
- Text is never baked into images. All strings go through props, so they can be localised.

---

## 8. App logic and state ("backend behaviour is the same")

- Domain logic is **plain Rust**: no mitsuami dependency, `Send` where useful, async-friendly, and testable through its own public API.
- **Stores** (Pinia-like) are the UI-thread adapter. They own signals and expose actions that call into domain logic.
- **Resources and actions** handle async: `resource(fetch_fn)` returns a `{ loading, data, error }` signal set, and `action(fn)` gives pending-state tracking. Results come back to the UI thread through the dispatcher.
- `provide` / `inject` replaces globals, so screens can be tested with mock stores.

Every per-platform screen (§6.1) consumes the same stores and composables. That is the reuse boundary.

---

## 9. API flavour (Vue → Rust mapping)

| Vue | mitsuami |
|---|---|
| `ref(x)` / `reactive` | `signal(x)` (`ref` is a Rust keyword). `Signal<T>` is `Copy` (arena-backed) |
| `computed` | `computed(move || …)` |
| `watch` / `watchEffect` | `watch(source, cb)` / `effect(move || …)` |
| props | typed struct fields via `#[component] fn Foo(label: String, #[prop(default)] n: i32)` |
| `emit('x')` | typed callback props: `on_change: Callback<f32>` |
| `v-model` | `bind:value=signal` (two-way) |
| `v-if` / `v-show` | `<Show when=…>` / `visible=…` |
| `v-for` + `:key` | `<For each=… key=…>` |
| slots / named slots | `children` / `#[slot] header: Slot` |
| `provide` / `inject` | `provide(ctx)` / `inject::<T>()` |
| `onMounted` / `onUnmounted` | `on_mounted` / `on_cleanup` |
| template refs | `let r = node_ref(); <TextInput node_ref=r/>`, then `r.focus()` |

```rust
#[component]
fn Counter(initial: i32) -> impl View {
    let count = signal(initial);
    let doubled = computed(move || count.get() * 2);

    view! {
        <Column gap=Spacing::Md padding=2.em() align=Center>
            <Text style=TextStyle::Title>{move || format!("Count: {}", count.get())}</Text>
            <Button variant=Primary @click=move |_| count.update(|c| *c += 1)>"Increment"</Button>
            <Show when=move || doubled.get() > 10>
                <Text color=Color::SecondaryLabel>"That's a big number"</Text>
            </Show>
        </Column>
    }
}
```

The builder API is the real API. `view!` expands to it:

```rust
Column::new().gap(Spacing::Md).padding(2.em()).children((
    Text::new(move || format!("Count: {}", count.get())).text_style(TextStyle::Title),
    Button::new("Increment").variant(Primary).on_click(move || count.update(|c| *c += 1)),
))
```

---

## 10. Built-in widget set

| Widget | AppKit | WinUI 3 | GTK 4 |
|---|---|---|---|
| Window | NSWindow | Window | gtk::ApplicationWindow |
| Container / Row / Column / Grid | flipped NSView host | Canvas host | custom Widget host |
| Text | NSTextField (label) | TextBlock | gtk::Label |
| Button | NSButton | Button | gtk::Button |
| TextInput | NSTextField | TextBox | gtk::Entry |
| Checkbox | NSButton (checkbox) | CheckBox | gtk::CheckButton |
| Switch | NSSwitch | ToggleSwitch | gtk::Switch |
| Slider | NSSlider | Slider | gtk::Scale |
| Select | NSPopUpButton | ComboBox | gtk::DropDown |
| Progress | NSProgressIndicator | ProgressBar / ProgressRing | gtk::ProgressBar / Spinner |
| Image | NSImageView | Image | gtk::Picture |
| ScrollView | NSScrollView | ScrollViewer | gtk::ScrolledWindow |
| List (virtualised) | NSTableView | ListView | gtk::ListView |

**Idiomatic shell components (post-MVP).** These are where most of the "feels native" effect comes from:

- `AppShell`, `Sidebar` (source list / NavigationView / split view)
- `Toolbar` (NSToolbar / CommandBar / HeaderBar)
- `MenuBar` (the global menu on macOS; an in-window menu or hamburger elsewhere)
- `Preferences`

---

## 11. Platform versions and capabilities

We don't pick one fixed OS version per platform. Instead:

- Each backend has a **hard floor**: the oldest version the backend itself can run on.
- Everything above the floor is a **capability**. Capabilities can be queried, and built-in widgets degrade gracefully when one is missing.

### Floors

| Platform | Floor | Why |
|---|---|---|
| Windows | Whatever Windows App SDK 2.4 / WinUI 3 supports (historically 10 1809, build 17763; to be confirmed for 2.4) | Pinned to the SDK version that `windows-reactor` targets. |
| macOS | macOS 11 | The practical floor for arm64 and current Rust targets. Everything newer is a capability. |
| Linux | GTK 4.10, or GTK 4.8 in reduced mode | Chosen at build time through a cargo feature (`gtk_v4_8`, `gtk_v4_10`, `gtk_v4_12`, …). libadwaita versions work the same way under the `adwaita` feature. |

### Capability model

```rust
#[non_exhaustive]
pub enum Capability {
    // cross-platform semantics, answered by every backend
    NativeSwitch, SymbolIcons, NativeFileDialog, SystemAccentColor,
    WindowBackdropMaterial,        // Mica on Windows 11, vibrancy/glass on macOS, none on GTK
    SplitViewSidebar, SearchField, DatePicker, …
    // platform-specific ones live in each backend's own enum
}

caps().has(Capability::WindowBackdropMaterial)   // plain bool, fixed for the process lifetime
```

How capabilities are detected differs per platform, and the design follows that:

- **macOS: detected at runtime.** Objective-C is dynamic, so the backend checks the OS version, `respondsToSelector:` and class existence at startup. A single binary adapts to whatever macOS it runs on.
- **Windows: fixed at build time, plus the OS build.** The WinAppSDK version is bundled with the app, so its features are fixed. OS-dependent features, like Mica needing Windows 11, are checked at runtime from the build number.
- **Linux: fixed at build time.** gtk4-rs links newer symbols directly, so a binary built with `gtk_v4_12` won't even load on GTK 4.10. The capability set is therefore whatever the build feature enables. Distro packages pick the feature that matches their GTK. Runtime `dlsym` probing is possible later, but it isn't worth it for the MVP.

### How capabilities are used

- **Built-in widgets degrade automatically.** For example, `Switch` falls back to a checkbox-style toggle, and `SymbolIcon` falls back to a bundled icon. Each fallback is documented on the widget. App code only branches when it wants different behaviour.
- **Apps declare hard requirements:** `App::new().require(Capability::X)`. Startup then fails with a clear, native error dialog instead of breaking halfway.
- **Views can branch on capabilities:** `platform!` arms accept capability predicates, e.g. `macos if has(WindowBackdropMaterial) => …`. There is also a runtime `<Supports cap=… fallback=…>` component.
- **Custom widgets** (`NativeRender`) can query `cx.caps()` to choose an implementation.

This also settles "runtime vs compile-time platform checks": the platform is compile-time (`platform!`), and capabilities are runtime (fixed per process).

---

## 12. Testing

Testing is a first-class feature, both for mitsuami itself and for apps built with it.

### Policy

- **No unit tests.** There are no `#[cfg(test)] mod tests` blocks and no tests of private functions. Every test goes through a public API.
- There are two kinds of test:
  - **Integration tests:** public API, headless backend, in `tests/` directories. Fast and deterministic; they run on every platform, in CI and locally.
  - **End-to-end tests:** a real native backend in a real window, driven the way a user would drive it.
- **Visual regression tests** (Chromatic-style) are a third track built on the same infrastructure.
- The tooling we build for ourselves is the tooling we ship. `mitsuami-test` is a public crate for app authors, and mitsuami's own suite is its first user.

### One test API, two backends

The same test runs headless or on the real native backend. Only the runner flag changes.

```rust
use mitsuami_test::prelude::*;

#[mitsuami_test::test]
async fn increments(app: TestApp) {
    app.mount(|| Counter(0));
    app.get_by_role(Role::Button, "Increment").click().await;
    app.expect(by_text("Count: 1")).to_be_visible().await;
    app.expect(by_role(Role::Button, "Increment")).to_have_frame_within(app.window());
}
```

```
cargo test                      # headless (default): integration tier
cargo test -- --native          # same tests on AppKit / WinUI / GTK: e2e tier
cargo mitsuami visual           # visual regression tier (below)
```

Design points:

- **Queries go through the accessibility tree**, like Testing Library: `get_by_role`, `get_by_label`, `get_by_text`, `get_by_test_id` as a last resort. If a test can't find a control by role and label, a screen-reader user can't either. The a11y model does real work from day one.
- **Interactions are a11y actions:** `click` → `A11yAction::Activate`, plus `set_value`, `focus`, `scroll_into_view`, `expand`, `type_text`. The backend carries these out on the actual native control:
  - AppKit: `performClick:`, `accessibilityPerformPress`
  - WinUI: UIA patterns (Invoke, Value, Toggle)
  - GTK: `gtk_widget_activate`, `GtkAccessible`
  
  The same vocabulary later powers screen-reader support.
- **Raw input** (`app.keyboard()`, `app.pointer()`) is also available, for shortcuts, drag and drop, and custom widgets. It is synthesised at the native event level where the platform allows it.
- **No sleeps, no flakiness.** We own the scheduler, so `await` on an action or assertion means "run until the UI is settled": effects flushed, layout done, the native commit applied, and pending tasks idle. On top of that there is Playwright-style auto-retrying of assertions for async work, with a timeout.
- **Deterministic environment:**
  - a controllable fake clock (`app.clock().advance(500.ms())`)
  - a fixed locale, text scale and color scheme per test
  - animations off by default
  - `TestServices` replaces `PlatformServices`: dialogs, file pickers, clipboard and notifications are recorded, and their responses are scripted.
- **Dependency injection:** tests `provide` fake stores or services before mounting, the same `provide`/`inject` apps already use.
- **Native desync checks.** `app.native_state(node)` reads back what the native widget actually shows: text, checked state, enabled state, frame. In e2e mode, assertions compare core state with native state. This catches backend bugs that headless tests can't.
- **Snapshots:**
  - Tree, layout frames, a11y tree and command log snapshots (insta-style `.snap` files).
  - A **wireframe render**: an SVG of the headless layout (boxes, labels, roles). It is deterministic, platform-independent and diffable in review, so it's a cheap first line of visual testing.

### The main-thread problem

Native UI must run on the process main thread; on macOS this is strictly enforced. libtest runs tests on worker threads. So:

- Test targets use `harness = false`, with our runner: `mitsuami_test::main!()`, generated by `#[mitsuami_test::test]` through inventory-style registration.
- The runner owns the main thread and the native event loop, and runs test futures on it.
- In native mode, tests run serially in one process with a fresh window each (a fast, window-per-test mode). `--isolate` gives process-per-test when needed.
- CLI filtering and output stay compatible with libtest conventions, so `cargo test name_filter` and IDE runners keep working.

### Visual regression (Chromatic-style)

**Stories.** A story renders a component in a specific state. Stories double as a component gallery during development.

```rust
#[mitsuami_test::story(sizes = [(320, 200)], variants = [Light, Dark])]
fn counter_big_number() -> impl View { Counter(42) }
```

Stories can also be interaction states. `#[story(play = ...)]` runs a test script first (e.g. focus a field, type text) and then captures the result.

**Capture.** Capture happens in-process and offscreen, so no screen-recording permissions are needed and nothing depends on window placement:

- AppKit: `cacheDisplayInRect:toBitmapImageRep:`
- WinUI: `RenderTargetBitmap`
- GTK: `WidgetPaintable` → `GskRenderer::render_texture`

This is exposed as `Backend::capture`.

**Matrix.** Each capture is taken for every combination of platform, scale factor (1×, 2×), light/dark, and optionally high contrast and large text.

**Baselines.**
- Keyed by `platform / os-image / story / variant`. Native rendering differs between OS versions, so baselines are tied to a pinned CI image per platform.
- Stored in the repo under `visual/` using git LFS. A hosted store can be added later behind the same interface.

**Diffing.**
- Perceptual diff: anti-aliasing tolerant, with a per-story threshold and ignore regions (e.g. a blinking caret or a system clock).
- A **layout-only diff** from the wireframe snapshot runs first. It says *why* pixels moved: a layout change versus a native rendering change.

**Review.**
- `cargo mitsuami visual review` generates a local HTML report: side-by-side, overlay, onion-skin and diff-highlight views, and accept/reject per change. Accepting updates the baselines.
- CI runs the story matrix on macOS, Windows and Linux runners. It uploads the report as an artifact and fails the build on unapproved diffs.
- Later: a PR comment with a summary, and a small hosted review service (the real "Chromatic" part).

### What mitsuami tests about itself

- **The reactive runtime** is tested through its public API in `mitsuami-reactive/tests/`. It is exhaustively scenario-based: ownership and cleanup, batching, glitch-freedom, flush ordering.
- **Layout and units:** headless integration tests plus wireframe snapshots against a CSS reference. The expected frames come from what a browser computes for the equivalent HTML and CSS.
- **Backend conformance suite:** one shared suite that every backend must pass in `--native` mode. It covers each built-in widget's props, events, measurement sanity, a11y roles, and focus traversal. It is the executable definition of the backend contract.
- **Widget gallery stories** for every built-in widget, on every platform, form the visual baseline for the toolkit itself.

---

## 13. Risks, ranked

1. **WinUI 3 through `windows-rs` 0.100 / `windows-reactor`.**
   - Microsoft now ships `windows-reactor`, a declarative WinUI 3 framework (60+ controls, targets Windows App SDK 2.4.0). `windows-reactor-setup` stages the App Runtime for self-contained apps. Bootstrapping is therefore largely solved.
   - **The open problem is integration, not access.** Reactor has its own component/state/effect system, and we must not run two reactive systems on top of each other. We need *imperative* access to the XAML elements: create, set a property, insert into a `Canvas`, `Measure`, and events.
   - Options, in order of preference:
     - (a) Reactor exposes raw element handles or an imperative layer we can drive directly.
     - (b) Use the XAML bindings reactor itself is built on (or generate them with `windows-bindgen`) and use only `windows-reactor-setup` for bootstrap.
     - (c) Host reactor components for leaf widgets only. This is the least attractive option.
   - XAML subclassing from Rust is painful. The design avoids needing it: `Canvas` hosts, and no custom `Panel`.
   - **→ Spike this first**, before committing to details. The spike: a window, a Button inside a Canvas, a click handler and measure calls. It also answers which of (a), (b) or (c) is viable. A C++/WinRT shim remains the last-resort fallback.
2. **Measurement fidelity.** Native intrinsic sizes can be quirky: NSTextField wrapping, and WinUI's Measure needing to be in the live tree. Headless tests can't catch this. The backend conformance suite and the visual gallery (§12) cover it.
3. **The own reactive runtime is on the critical path.** Everything depends on it: effect ownership and cleanup, batching, and flush ordering relative to layout. It needs a thorough scenario-based integration suite before anything is built on top of it.

---

## 14. MVP plan

| Milestone | Scope | Done when |
|---|---|---|
| **M0 — Core + test harness** ✅ | Workspace; `mitsuami-reactive`; node tree; styles and units; Taffy integration; `Command` protocol; headless backend; `mitsuami-test` basics (custom runner, a11y queries, actions, settle, tree/layout/wireframe snapshots) | Counter and a flex/grid form pass headless integration tests written with the public test API; the reactive suite passes |
| **M0.5 — WinUI spike** (in parallel) | Throwaway, using windows-rs 0.100: a window, a Canvas, a Button, Click and Measure, driven imperatively | Integration route (a/b/c) chosen |
| **M1 — AppKit** | Window, View hosts, Text, Button, TextInput, Checkbox, Switch; run-loop flush; measure; resize → relayout | The M0 tests pass with `--native` on macOS; conformance suite v1 passes; `Backend::capture` works and a first visual baseline exists |
| **M2 — GTK 4** | The same widget set (developed and tested on Linux, e.g. a VM or CI) | The same tests and conformance suite pass with `--native` on Linux |
| **M3 — WinUI 3** | The same widget set | The same tests and conformance suite pass with `--native` on Windows |
| **M4 — Escape hatches** | `platform!`, `NativeView`, `CustomWidget` + `NativeRender` (+ drawn fallback) | Demo has a mac-specific screen sharing a store, and a custom widget with three renders |
| **M5 — Ergonomics** | `#[component]`, `view!`, stores, resources | Demo rewritten with macros |
| **M6 — Visual review** | Stories, the variant matrix, perceptual diff, `cargo mitsuami visual review` HTML report, CI on three OSes | A PR that changes a widget shows up as a reviewable visual diff on all three platforms |

Out of scope for the MVP: lists/virtualisation, menus beyond a basic app menu, dialogs beyond an alert, the a11y implementation (the model exists), animations, and a devtools inspector.

---

## 15. Decision log

| Decision | Choice |
|---|---|
| Name | **mitsuami** (三つ編み, "three-strand braid": three native backends woven into one) |
| GTK flavour | Plain gtk4 core, with an optional `adwaita` feature for shell components and style classes |
| Reactivity | Our own single-threaded runtime (`mitsuami-reactive`), not a reused one |
| `view!` syntax | JSX-like; the builder API remains the real API |
| OS support | Backend floors plus capabilities (§11); Windows floor = whatever Windows App SDK 2.4 supports |
| Layout ownership | Ours (Taffy); native widgets positioned absolutely |
| Toolkit per platform | Strictly native: AppKit / WinUI 3 / GTK 4, one per OS, no cross-toolkit dev builds |
| Windows bindings | windows-rs 0.100+ (`windows-reactor` ecosystem, WinAppSDK 2.4); MSRV 1.95, edition 2024 |
| Platform vs runtime checks | Platform is compile-time (`platform!`); capabilities are runtime |
| Testing | No unit tests. Integration (headless) + e2e (native) with one API; a11y-driven queries and actions; Chromatic-style visual regression; testing toolkit shipped to users |

## 16. Open questions

- The exact Windows floor for Windows App SDK 2.4.
- Whether `windows-reactor` allows imperative element access (answered by the M0.5 spike).
- Visual baseline storage: git LFS is fine to start with. Revisit when baseline volume grows (3 platforms × variants × stories).
- How much native input synthesis GTK 4 allows for e2e raw-input tests. GTK 4 restricts synthesised events, so we may need AT-SPI or in-process event injection.
