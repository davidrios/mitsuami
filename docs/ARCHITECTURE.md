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

> Implementing a backend? The step-by-step guide is [BACKENDS.md](BACKENDS.md).

The core speaks **only in `NodeId`s and plain data**. Each backend keeps its own `NodeId → native handle` map, so core has no generic parameters and no `dyn Any` handles.

```rust
// As implemented (crates/mitsuami-core/src/{command,backend,services}.rs).
pub enum Command {
    Create        { id: NodeId, kind: WidgetKind, props: Vec<Prop> }, // zero frame; initial props, reactive ones too
    SetProp       { id: NodeId, prop: Prop },
    Insert        { parent: NodeId, child: NodeId, index: usize },  // a ScrollView has exactly one child
    Remove        { parent: NodeId, child: NodeId },
    Destroy       { id: NodeId },       // every native node of a removed subtree, children first
    SetFrame      { id: NodeId, frame: Rect },          // parent-relative, logical units; never for windows
    SetA11y       { id: NodeId, a11y: A11yProps },      // backends may no-op initially
    SetWindowSize { id: NodeId, size: Size },
    SetFocusOrder { window: NodeId, order: Vec<NodeId> }, // Tab order, owned by the core
    ScrollTo      { id: NodeId, offset: Point },        // already clamped; backend reports Scrolled
    Focus         { id: NodeId },
}

pub enum UiEvent {   // backend → core, through the EventSink
    Click, Changed(EventValue), Submit, FocusIn, FocusOut, Scrolled(Point),
    WindowResized(Size), WindowCloseRequested, MetricsChanged,
    Pointer(PointerEvent),  // drawn custom widgets (§6.3)
    Custom(AnyValue),       // custom widgets and native views, their own event types
}

pub trait Backend {
    fn init(&mut self, events: EventSink);
    fn metrics(&self) -> PlatformMetrics;             // fonts, spacing tokens, scale, color scheme, reduced motion…
    fn apply(&mut self, batch: &[Command]);
    fn measure(&mut self, id: NodeId, request: MeasureRequest) -> Size;
    fn services(&self) -> Box<dyn Services>;          // clipboard, dialogs, menus (replaceable: tests use a fake)

    // Testing and accessibility hooks: part of the contract from day one.
    fn perform(&mut self, id: NodeId, action: &A11yAction) -> Result<(), ActionError>;     // act on the native control
    fn synthesize(&mut self, id: NodeId, input: &SyntheticInput) -> Result<(), ActionError>; // keys, scroll wheel
    fn native_state(&self, id: NodeId) -> Option<NativeState>; // props, frame, children, focus, scroll offset
    fn capture(&mut self, id: NodeId, reply: Reply<Result<Image, CaptureError>>); // offscreen screenshot
}

pub trait Services {
    fn clipboard_text(&mut self, reply: Reply<Option<String>>);
    fn set_clipboard_text(&mut self, text: &str, reply: Reply<Result<(), ServiceError>>);
    fn alert(&mut self, parent: Option<NodeId>, alert: &Alert, reply: Reply<usize>);   // never blocks
    fn open_file(&mut self, parent: Option<NodeId>, request: &OpenFile, reply: Reply<Option<Vec<PathBuf>>>);
    fn save_file(&mut self, parent: Option<NodeId>, request: &SaveFile, reply: Reply<Option<PathBuf>>);
    fn set_menu(&mut self, menu: &MenuBarData, activate: Rc<dyn Fn(u32)>); // keeps the platform's standard menus
}
```

Anything a platform might complete later is **reply-based** (async), even when AppKit answers immediately: `capture`, clipboard reads and writes, dialogs. `measure`, `native_state` and `metrics` stay synchronous; see [BACKENDS.md §11](BACKENDS.md#11-sync-and-async-in-the-contract).

A platform's **run loop** drives the `Ui` through three hooks:
- **`Ui::tick()`** runs ready tasks and due timers, dispatches events and commits, repeating until idle. Call it before the loop sleeps.
- **`Ui::set_commit_scheduler`** and **`Ui::set_waker`** (thread-safe) make the loop turn when something changes.
- **`Ui::time_to_next_timer()`** says when to wake up for `sleep`.

On AppKit, these are a `CFRunLoopObserver` and one re-armed `CFRunLoopTimer`, both in common modes.

Properties of this contract:

- **Serializable and inspectable.** Commands can be logged, snapshot-tested and replayed. They could even be sent to a devtools inspector later.
- **The headless backend is trivial to write.** It makes layout, trees and a11y fully testable in CI without a display, and it is the default backend for integration tests (§12).
- **Controlled inputs** (`v-model`) avoid feedback loops: the backend emits `Changed(text)`, core updates the signal, and the effect sends `SetProp` back only if the value differs from what the native widget already holds. This preserves the cursor, selection and IME composition.
- **Threading:** all UI work runs on the main thread, and the reactive runtime is `!Send`.
  - `spawn_local` runs futures on the UI thread.
  - `spawn_blocking` runs work on another thread and resumes the task on the UI thread. It uses standard `Waker`s, which call the run loop's thread-safe waker.
  - `sleep` uses the `Ui`'s clock, which tests replace with a manual one (`app.advance(...)`).
- **Scopes:** tasks and event handlers run in the reactive scope of the component that created them. `inject`, `spawn_local` and `sleep` therefore work inside them, and disposing the component cancels its tasks.

---

## 6. Escape hatches

> Working example: `crates/mitsuami/examples/escape_hatches/`, tested by `crates/mitsuami/tests/escape_hatches.rs`.

### 6.1 Platform-specific screens with shared behaviour

Logic lives in **composables** (Vue's `useXxx`) or **stores** (Pinia-like). Views are thin, so writing two views costs little.

```rust
// shared, platform-agnostic: signals and actions, provided to the screens
#[derive(Clone, Copy)]
pub struct Review { pub stars: Signal<u8>, pub comment: Signal<String>, /* … */ }
pub fn use_review() -> Review { inject::<Review>().expect("provide a Review") }

pub fn review_screen() -> impl View {
    platform! {
        macos => macos::review_screen(),   // trailing labels, NSStepper, button at the trailing edge
        _     => shared::review_screen(),  // the Windows/Linux version
    }
}
```

- `platform!` is compile-time (`cfg`), so code for other platforms is never compiled into the binary. Arms may have different types.
- Arms are `macos`, `windows`, `linux`, several joined with `|`, and a final `_`. The first matching arm wins.
- Without a `_` arm, building for a platform no arm names fails, so a missing screen can't ship by accident.
- Per-platform view files follow a convention: `review/mod.rs`, `review/macos.rs`, `review/shared.rs`. The shared screen is compiled everywhere so it can be tested everywhere.
- Capability predicates on arms (`macos if has(…)`) wait for capabilities (§11).

### 6.2 Raw native view inside the shared tree

```rust
NativeView::appkit(|cx: &mut AppKitCx| {
    let stepper = NSStepper::new(cx.mtm());
    let emitter = cx.emitter();
    cx.on_action(&*stepper, move |s| emitter.emit(s.doubleValue().round() as u8));
    stepper
})
.update(review.stars, |stepper, stars| stepper.setDoubleValue(*stars as f64))  // re-applied when stars changes
.on_event(move |stars: &u8| review.rate(*stars))
.measure(|view, request| /* optional; default intrinsicContentSize */)
.a11y_label("Stars")
```

- It takes part in layout, a11y and events like any other node. Core sees `WidgetKind::Native`.
- The factory and the current value of every `update` travel as one `Prop::Native` payload (an `Opaque`: compared by identity, printed as its label). When any value changes, the payload is sent again and every update re-applied.
- Native callbacks never touch signals directly; they `emit` events that are queued and dispatched on the next turn, like any native event.
- Accessibility actions (`Activate`, `Increment`, `Decrement`) go to the view's accessibility element, as VoiceOver's would.
- Headless tests show a native view as an empty box sized by its styles.
- `mitsuami::appkit` re-exports `objc2`, `objc2_app_kit` and `objc2_foundation` at the backend's versions.

### 6.3 Custom widgets: generic logic plus one render per platform

Custom widgets come in three tiers. Pick the lowest that works:

1. **Composition:** built from existing widgets. It runs everywhere, and it's the tier for what the canvas can't draw (text, editing). A plain component works; as a custom widget's render (`Renderer::composed`), it keeps the widget's props and events, so it can stand in for a native render.
2. **Drawn:** a shared `Drawn` implementation using a small 2D API (`Canvas`: fill and stroke rects, rounded rects, ellipses and paths) with semantic colors (`Color::Accent`, `Color::Label`, …). It runs everywhere and follows dark mode and the accent color, but isn't truly native.
3. **Native per platform:** one shared definition plus one render per platform.

```rust
// Shared definition — platform free.
pub struct Rating;
impl CustomWidget for Rating {
    const NAME: &'static str = "Rating";
    type Props = RatingProps;                 // { value: u8, max: u8, editable: bool }
    type Event = RatingEvent;                 // Changed(u8)
    fn a11y(p: &RatingProps) -> A11yProps {   // semantics are shared
        A11yProps::new(Role::Slider).label("Rating").value(format!("{} of {}", p.value, p.max))
    }
    fn action(p: &RatingProps, action: &A11yAction) -> Option<RatingEvent> { /* Increment → Changed(value + 1) … */ }
}

// Tier 2, shared too.
impl Drawn for Rating {
    fn measure(p: &RatingProps, request: &MeasureRequest, metrics: &PlatformMetrics) -> Size;
    fn draw(p: &RatingProps, canvas: &mut Canvas);
    fn pointer(p: &RatingProps, size: Size, event: &PointerEvent) -> Option<RatingEvent>;
}

// Tier 3: one impl per platform, each under cfg, e.g. rating/macos.rs.
#[cfg(target_os = "macos")]
impl NativeRender for Rating {                 // trait defined by mitsuami-appkit
    type View = NSLevelIndicator;
    fn create(p: &RatingProps, cx: &mut AppKitCx) -> Retained<NSLevelIndicator>;
    fn update(v: &NSLevelIndicator, old: &RatingProps, new: &RatingProps);
    fn measure(v: &NSLevelIndicator, p: &RatingProps, request: &MeasureRequest) -> Option<Size> { None } // None = intrinsicContentSize
    fn read(v: &NSLevelIndicator, p: &RatingProps) -> RatingProps;  // read back, for the mirror check
}

// Which render each platform uses.
impl Render for Rating {
    fn renderer() -> Renderer<Self> {
        platform! {
            macos => mitsuami::appkit::native::<Self>().with_drawn(),
            _ => Renderer::drawn(),
        }
    }
}
```

- A usage site is `Rating::view(move || RatingProps::new(stars.get())).on_event(…)`, the same on every platform. `.drawn()` and `.composed()` pick those renders where a native one exists.
- **A widget native on one platform stands in on the others.** The renderer uses the first render it has: native, then drawn, then composed. A native render is either the platform's own control (`native::<W>()`) or built **ad hoc** from the platform's widgets the way that platform's apps build it (`ad_hoc::<W>()`), so it still looks at home. `Renderer::is_native()` is true only for the platform's own control, so a screen can be honest about it.
- The `escape_hatches` example is one screen for every platform with three such widgets:
  - a lock: native on GTK (`GtkLockButton`), composed elsewhere;
  - a rating: native on macOS (`NSLevelIndicator`), ad hoc on GTK (star buttons, as GNOME Software builds it), drawn elsewhere;
  - a pips pager: WinUI's, drawn until the WinUI backend renders it.
- **A composed render** gets the props (reactive), a way to emit the widget's events, and the app's accessible label, which it puts on the control that stands for the widget. It builds as a plain container, so its built-in widgets carry the semantics.
- **Compile-time coverage:** using a widget requires `Render`, and `Render` has to name a render that exists on the platform being built: a `NativeRender` impl or a `Drawn` one. A missing render doesn't build.
- **Transport:** commands carry `WidgetKind::Custom(NAME)` and `Prop::Custom(CustomProps)`: the props as an `AnyValue` (type-erased, but still compared and printed with their own `PartialEq` and `Debug`) plus the widget's definition (semantics, action mapping, renders). Events come back as `UiEvent::Custom(AnyValue)`. Props don't need to be serializable. The native render travels with the props, so backends keep no registry.
- **Semantics** come from `CustomWidget::a11y`; app overrides (`.a11y_label(…)`) win. Accessibility actions go to the native render first; if it doesn't handle one, the core emits the event `CustomWidget::action` maps it to. Drawn and native renders behave the same for assistive technology and tests.
- **The drawn tier runs in the core.** The core measures drawn widgets itself, draws them after layout (on new props, a new size or new metrics) and sends the result as `Prop::Drawing(DisplayList)` with the frames. Backends only rasterize display lists and report `UiEvent::Pointer`, which the core turns into widget events with `Drawn::pointer`. Headless wireframes draw them too.
- **Headless** lays out natively rendered widgets with their drawn render (hence `.with_drawn()` above), or as empty boxes without one.
- **Controlled:** a render emits an event when the user changes the view; the app answers with new props, and `update` shows them. If the app ignores the event, the view shows something the core doesn't know about, and the mirror check (via `read`) reports it.

## 7. Accessibility and i18n affordances (designed in now, implemented later)

- Every node carries `A11yProps`: role, label, description, value/range, state flags (disabled, checked, expanded, selected, busy), `labelled_by`/`described_by` relations, live-region politeness, and supported actions.
- Built-in widgets derive defaults: a `Button`'s label comes from its text, and so on. Apps override with `.a11y_label("…")`, and so on.
- Most of the work comes for free because the controls are **native**: NSAccessibility, UIA and GtkAccessible already understand native controls. `SetA11y` mainly carries overrides and relations.
- Drawn and custom widgets are where real work is needed. The plan is to implement native a11y protocols per backend. [AccessKit](https://github.com/AccessKit/accesskit) is an option for drawn subtrees, because it provides the same semantic model on all three platforms.
- Core owns **focus order**. The Tab order is reading (tree) order, so it's correct in right-to-left layouts and for absolutely positioned controls. `.tab_index(n)` moves controls ahead. Backends receive it as `SetFocusOrder` and chain native focus accordingly (AppKit: `nextKeyView`). Which controls can take focus stays a platform decision; on macOS, for example, it depends on the Keyboard navigation setting.
- `PlatformMetrics` exposes reduced motion, high contrast, text scale and color scheme as signals.
- **RTL:** styles use logical edges (`padding_inline_start`, not `padding_left`). Core mirrors frames for RTL locales, since Taffy doesn't.
- Text is never baked into images. All strings go through props, so they can be localised.

---

## 8. App logic and state ("backend behaviour is the same")

- Domain logic is **plain Rust**: no mitsuami dependency, `Send` where useful, async-friendly, and testable through its own public API.
- **Stores** (Pinia-like) are the UI-thread adapter. They own signals and expose actions that call into domain logic.
- **Async** uses plain futures: `spawn_local`, `spawn_blocking` and `sleep` (built), plus `alert`, `open_file` and `save_file` for dialogs.
- **Resources and actions** (M5) will wrap these: `resource(fetch_fn)` returns a `{ loading, data, error }` signal set, and `action(fn)` gives pending-state tracking.
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
| Window | NSWindow | Window | gtk::Window + HeaderBar |
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
MITSUAMI_NATIVE=1 cargo test    # same tests on AppKit / WinUI / GTK: e2e tier
                                # (or `-- --native` for a single harness=false target)
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
| **M1 — AppKit** ✅ | Window, View hosts, Text, Button, TextInput, Checkbox, Switch; run-loop flush; measure; resize → relayout | The M0 tests pass with `--native` on macOS; conformance suite v1 passes; `Backend::capture` works and a first visual baseline exists |
| **M2 — GTK 4** ✅ | The same widget set (developed and tested on Linux, e.g. a VM or CI) | The same tests and conformance suite pass with `--native` on Linux |
| **M3 — WinUI 3** | The same widget set | The same tests and conformance suite pass with `--native` on Windows |
| **M4 — Escape hatches** ✅ (AppKit, GTK) | `platform!`, `NativeView`, `CustomWidget` + `NativeRender` (+ drawn and composed fallbacks) | Demo: one screen for every platform, with three custom widgets, each native on one platform and built ad hoc, drawn or composed on the others |
| **M5 — Ergonomics** | `#[component]`, `view!`, stores, resources | Demo rewritten with macros |
| **M6 — Visual review** | Stories, the variant matrix, perceptual diff, `cargo mitsuami visual review` HTML report, CI on three OSes | A PR that changes a widget shows up as a reviewable visual diff on all three platforms |

M4's custom widget has a native render on macOS and uses the drawn render on Windows and Linux. The GTK and WinUI counterparts of `NativeRender` and `NativeView::appkit` arrive with those backends (M2/M3); BACKENDS.md §8a says what they involve.

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

## 16. Implementation notes

Things the AppKit backend taught us, some of them now part of the contract:

- **Native views start with a zero frame.** The core only sends frames that differ from the last one it sent. AppKit controls come with frames of their own, so the backend zeroes them on creation. The mirror check caught this for a `display: none` label.
- **Presses go through `accessibilityPerformPress`**, the path VoiceOver uses. For windows that aren't on screen, it returns `NO` even after pressing, so the result is ignored.
- **Typing goes through the field editor** (`insertText:`, `deleteBackward:`, `insertNewline:`), so the delegate and action paths are the real ones. Focusing a field selects all of its text, so the backend puts the caret at the end before typing, like clicking past the end would.
- **Tests run in offscreen windows** with the light appearance forced, for comparable captures (`MITSUAMI_SHOW_WINDOWS=1` shows them). Default (Primary) buttons render grey in inactive windows. That's AppKit's behavior, and baselines reflect it.
- **Code-built windows get no Tab order.** AppKit's automatic key view loop orders controls by position on screen, which is wrong for right-to-left layouts and absolute positioning. The core now sends the order (`SetFocusOrder`), and the backend links `nextKeyView` into a loop. The conformance tests use three controls on purpose: with two, wrap-around would make any order pass. They were confirmed to fail with AppKit's position-based order.
- **Reactive styles:** every style setter takes a literal, a signal or a closure, and `style_with` edits several fields reactively. `hidden` is a flag of its own, so un-hiding restores the node's `display` (a grid stays a grid).
- **Focus:** AppKit reports focus through KVO on `NSWindow.firstResponder`. While a text field is being edited, the first responder is the window's field editor, and its delegate isn't set yet when focus moves. The backend therefore walks up from the responder through its superviews to the nearest known view. The mirror check compares native focus with the core's on every settle.
- **ScrollView:** `NSScrollView` with our content view as its document view. Scroll changes are observed through the clip view's bounds-change notifications, including programmatic scrolls, so `Scrolled` fires for both. Window-coordinate frames, visibility (clipped by enclosing scroll views) and `scroll_into_view` are computed in the core, so all backends agree. As in CSS, a scroll view's natural size is its content's, so its siblings need `.shrink(0.0)` to keep their size.
- **Run loop:** `-[NSApplication stop:]` waits for an event, so stopping from an observer posts an empty application-defined event.
- **Services:** tests always use the scripted fake, even natively, so they never show real dialogs or touch the real clipboard. The real AppKit services have their own checks (`mitsuami-appkit/tests/services.rs`): a private pasteboard, the real `NSMenu` bar, an alert sheet answered by clicking, and a cancelled open panel.
- **`cargo test -- --native` also reaches libtest harnesses**, which reject the flag, so `MITSUAMI_NATIVE=1` is the workspace-wide switch.
- **Known gap: min-content text measurement.** Min-content currently falls back to max-content, so text never shrinks below one line inside flex rows. It still wraps under a definite width (columns, fixed widths).

### M4 (escape hatches)

- **Widgets are created with their initial props, reactive ones included.** Reactive props used to arrive as `SetProp` right after `Create`. That's harmless for built-in widgets, but a custom widget or native view can't be created without its props. The core now merges props set before a node's `Create` has gone out into that `Create`. Headless rejects a custom widget or native view created without its prop; the AppKit run caught the gap first.
- **Native views' payloads are one prop.** With a factory prop plus update props sharing a key, the first update replaced the factory in `Create`. Now one reactive payload holds the factory and the current value of every update.
- **Act on the accessibility element, not the view.** An `NSStepper` isn't an accessibility element; its only accessibility child (the cell) is, and only that one increments and sends the action. The backend walks down to it, as VoiceOver does.
- **Synthesized clicks** (`SyntheticInput::Click`) are for drawn widgets only. Native controls track the mouse in a loop of their own, waiting for real events; tests drive them with accessibility actions.
- The drawn render sizes its stars from the body font, so it sits close to the native rating control. Headless wireframes draw display lists, so drawn widgets show up in reviews without pixels.

### M2 (GTK 4)

What the GTK 4 backend taught us:

- **Tests run on a private Broadway display.** The runner starts `gtk4-broadwayd` (bound to localhost) and points GDK at it, unless `MITSUAMI_SHOW_WINDOWS=1`. GTK needs a real, mapped window to capture and to track focus, and on the session display a tiling compositor would override window sizes. The display prints its address, so tests can be watched in a browser.
- **Windows get an explicit `HeaderBar`.** GTK's default size includes the titlebar. With a header bar of our own, its height is known, and the content gets exactly the size the core asks for.
- **Layout hosts allocate each child at its core frame**, and ask for exactly their own frame (window content hosts ask for nothing, so windows can shrink). Frames live in one map shared by all hosts, so a child keeps its frame when it moves to another parent (the core only resends frames that change). Leaves with an empty frame are hidden from GTK's allocation: controls can't be allocated smaller than their padding.
- **The Tab order is the window content host's `focus` vfunc.** Tab and Shift+Tab walk the core's order and wrap around; arrow keys keep GTK's geometric behaviour. The Tab conformance tests were confirmed to fail with GTK's own order.
- **Programmatic changes are muted.** GTK emits `toggled`, `notify::active` and `changed` for `set_active`/`set_text` too, so events are dropped while the backend applies commands.
- **Input is synthesized with keybinding signals** (answering the M1 open question): GTK 4 can't inject key events, so keys become the signals they are bound to, on the widgets that handle them: `insert-at-cursor`, `backspace` and `activate` on the entry's text widget, and `move-focus` on the window for Tab.
- **Button presses call `clicked` directly.** `gtk_widget_activate` would click only after the press animation, asynchronously.
- **Min-content text works here:** a wrapping `GtkLabel` reports its longest word as its minimum width.
- **Captures use the Cairo renderer**, whichever renderer the display uses, so baselines don't depend on the GPU. The content host carries the `background` style class, so captures include the window background.
- **Text styles map to GNOME's type scale** (`title-1`, `title-2`, `heading`, `caption`, `monospace`). GNOME has no callout size, so callouts use the body size.
- **Focus is tracked on the window** (`notify::focus-widget`), resolved to the nearest known node: an entry's focus sits on its inner text widget.
- **Scroll views are a `ScrolledWindow` around a `Viewport`.** Content hosts measure as their frame, so the viewport learns the content size. The adjustments are updated as soon as frames arrive, because a `ScrollTo` in the same commit needs the new range before GTK allocates.
- **Capture replies from the frame clock** (`after-paint` of the next frame), and the test executor lets the backend run while a test awaits. `TestHooks::settle` was added to the contract for this: platforms that complete work asynchronously catch up there.
- **Broadway frames stall without a browser** after a paint, until something new is drawn, so nothing may wait for two frames in a row.
- **The test display has portals off** (`GDK_DEBUG=no-portals`): otherwise file dialogs open on the real desktop, and its dark mode and fonts leak into tests.
- **Menus go in the header bar** (GNOME's primary menu button): one labelled section per app menu, then Quit (Ctrl+Q, which asks every window to close). Shortcuts are installed in every window. GTK's text widgets have their own Cut/Copy/Paste context menus, so there's no Edit menu.
- **Alerts** use `gtk::AlertDialog`: Escape chooses the last button. GTK has no alert styles, so `AlertStyle` is ignored.
- Not done yet: the `adwaita` feature, a reduced GTK 4.8 mode, and `gtk::Application` integration (single instance, app ID). `run` drives a plain GLib main loop.

### M4 on GTK

- **Custom widgets and native views on GTK** follow AppKit's shape: `mitsuami_gtk::NativeRender` (a `gtk::Widget` per render, measured by GTK unless the render measures itself), `NativeView::gtk(factory)`, and `mitsuami::gtk::gtk` for the bindings. GTK signals fire on programmatic updates, so custom events are muted while the backend applies props, like `Changed`.
- **Drawn widgets are a `DrawingArea` rasterized with Cairo.** Semantic colors come from the theme's named colors (`accent_color`, `borders`, …), with Adwaita's values as a fallback, so they follow the theme. Pointer events come from a click gesture; `SyntheticInput::Click` emits the gesture's own `pressed`/`released`, since GTK 4 can't inject pointer events.
- **Native views' accessibility actions** do what GTK's do: `activate` for Activate, and a step for spin buttons and ranges.
- **Only real platform controls count as native.** GTK has no rating control, so the example's rating is built ad hoc there, from flat buttons with `starred-symbolic` icons like GNOME Software's, and isn't labelled native. GTK's own widget in the example is `GtkLockButton`.
- **`GtkLockButton` is deprecated since GTK 4.10** and gone in GTK 5, but it's in every GTK 4. It shows a `GPermission`, and gtk4-rs can't subclass one, so the example registers a small one through GIO's C API: it reports what the props say, and acquiring or releasing just succeeds. The button's click is the request the app answers, like any controlled widget.
- **Focus requests wait for the structure.** `Ui::focus` right after building a node (a composed field focusing itself) used to reach the backend before the node was in a window, where no toolkit can focus it; headless didn't mind. Focus commands now go at the end of the batch's structure.

## 17. Open questions

- The exact Windows floor for Windows App SDK 2.4.
- Whether `windows-reactor` allows imperative element access (answered by the M0.5 spike).
- Visual baseline storage: git LFS is fine to start with. Revisit when baseline volume grows (3 platforms × variants × stories).
