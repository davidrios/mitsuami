# Writing a mitsuami backend

This guide is for writing a platform backend: GTK 4 (`mitsuami-gtk`), WinUI 3 (`mitsuami-winui`), or any other.

**Working reference:** the AppKit backend (`crates/mitsuami-appkit`). Every rule here is enforced by the test suites, and many were learned the hard way.

**Background:** read [ARCHITECTURE.md](ARCHITECTURE.md) §2–§5 first.

## 1. The model in one paragraph

The core owns the widget tree, the layout and every decision. A backend is a **mirror**:

- It creates native widgets when told to, and places them where it's told.
- It reports what the user did.
- It answers questions: how big is this, what does it show, what does it look like.

The backend **never lays anything out** and **never calls back into the `Ui`** from native callbacks; it queues events instead. All coordinates are **logical units** (points / effective pixels / GTK logical px), **parent-relative**, with the **origin at the top-left**.

## 2. What you implement

| Piece | Where | Reference |
|---|---|---|
| `Backend` | `mitsuami_core::Backend` | `mitsuami-appkit/src/backend.rs` |
| `Services` (clipboard, dialogs, menus) | `mitsuami_core::services::Services` | `mitsuami-appkit/src/services.rs` |
| `TestHooks` on your shareable handle | `mitsuami_core::TestHooks` | `impl TestHooks for AppKitHandle` |
| `run(setup)`: the app's run loop | your crate | `mitsuami-appkit/src/app.rs` |
| `init_for_tests()` | your crate | same file |

You also touch three places outside your crate, each behind a `cfg(target_os = …)`:

1. **`crates/mitsuami/Cargo.toml`, `crates/mitsuami/src/app.rs`:** `App::run` calls your `run`.
2. **`crates/mitsuami-test/Cargo.toml`, `crates/mitsuami-test/src/driver.rs`:**
   - Add a `native()` that builds your backend for tests: offscreen unless `MITSUAMI_SHOW_WINDOWS=1`, recording commands, a fixed light appearance, and a private clipboard if the platform allows it.
   - Add your OS to `native_available()`.
3. **`crates/mitsuami-<name>/Cargo.toml`:** put native dependencies under `[target.'cfg(target_os = "…")'.dependencies]`, so the workspace still builds everywhere.

Keep a shareable handle (`Rc<RefCell<State>>` inside the backend, with a cloneable handle outside). `Ui::new` takes ownership of the backend, but tests, services and the run loop still need access to it.

## 3. Commands

`Backend::apply(&mut self, batch: &[Command])` receives batches. **Order within a batch matters**, and the core guarantees:

- A node is created before it is inserted, and a subtree's root is removed before it is destroyed.
- Frames arrive in a second `apply` call per commit, after structure and props, because layout measures widgets that must already exist.

Validate as you go. Panic on protocol violations such as an unknown node, a double insert, or a second child in a ScrollView. The headless backend does this too; it's how contract bugs surface.

| Command | What to do |
|---|---|
| `Create { id, kind, props }` | Create the native widget and apply `props`. **Give it a zero frame**: the core only sends frames that differ from the last one it sent, so a widget born with its own frame stays wrong (AppKit labels do this). |
| `SetProp { id, prop }` | Apply the prop (tables below). For `Value`, skip the update if the widget already shows it, so the caret and IME composition survive. |
| `Insert { parent, child, index }` | Attach at `index` among the parent's native children. **ScrollView**: its single child is the scrolled content (AppKit: `documentView`). |
| `Remove { parent, child }` | Detach only. |
| `Destroy { id }` | Free the widget. It comes for every native node of a removed subtree, children first; the root has already been removed. Drop observers, signal handlers and targets. |
| `SetFrame { id, frame }` | Place the widget, relative to its native parent's top-left. Never sent for windows. A ScrollView's content frame is in content coordinates. |
| `SetA11y { id, a11y }` | Set the accessible label, description and hidden state. |
| `SetWindowSize { id, size }` | Set the window's **content area** size (excluding title bar and menu bar). |
| `SetFocusOrder { window, order }` | Make Tab visit `order` in sequence, wrapping around. It is window-wide, across nested containers. The platform still decides *which* controls can take focus (disabled controls, macOS keyboard navigation settings). See §9. |
| `ScrollTo { id, offset }` | Scroll the ScrollView so `offset` is at its top-left. Already clamped. The platform then **reports `Scrolled`**, as for a user scroll. |
| `Focus { id }` | Give the control keyboard focus. The focus change is reported through your focus tracking (§4), not by this command. |

### Widget kinds

| Kind | AppKit (done) | GTK 4 (suggested) | WinUI 3 (suggested) |
|---|---|---|---|
| `Window` | `NSWindow` + flipped content host | `gtk::ApplicationWindow` + host as child | `Window` + `Canvas` as `Content` |
| `Container` (layout host) | flipped `NSView` subclass | `gtk::Widget` subclass that allocates children at given frames (or `gtk::Fixed`) | `Canvas` (`Canvas.Left/Top`, `Width/Height`) |
| `Text` | `NSTextField` wrapping label | `gtk::Label` (wrap on, `xalign 0`) | `TextBlock` (`TextWrapping.Wrap`) |
| `Button` | `NSButton` push | `gtk::Button` | `Button` |
| `TextInput` | `NSTextField` | `gtk::Entry` / `gtk::Text` | `TextBox` |
| `Checkbox` | `NSButton` checkbox | `gtk::CheckButton` | `CheckBox` |
| `Switch` | `NSSwitch` | `gtk::Switch` | `ToggleSwitch` |
| `ScrollView` | `NSScrollView` | `gtk::ScrolledWindow` | `ScrollViewer` |
| `Custom`, `Native` | a plain host (M4) | a plain host | a plain host |

### Props

| Prop | Applies to | Notes |
|---|---|---|
| `Title` | Window | |
| `Text` | Text | |
| `Label` | Button, Checkbox, Switch | Switches usually show no caption; use it as the accessible name. |
| `Value` | TextInput | Don't re-set a value the widget already shows. |
| `Placeholder` | TextInput | |
| `Checked` | Checkbox, Switch | Setting it programmatically **must not** emit `Changed` (§4). |
| `Enabled` | controls | |
| `TextStyle` | Text (and controls) | Map to the platform type ramp: GTK style classes (`title-1`, `heading`, `caption`, `monospace`); WinUI text styles (`TitleTextBlockStyle`, …). |
| `Variant` | Button | Primary = the default / suggested action (GTK `suggested-action`, WinUI `AccentButtonStyle`); Destructive (GTK `destructive-action`); Plain = borderless (GTK `flat`). |
| `ScrollAxes` | ScrollView | Which scrollbars / scroll directions exist. |

## 4. Events

Native callbacks **only** call `events.emit(id, event)` on the `EventSink` given to `init`. The `Ui` drains the queue at a safe time, so never call `Ui` methods from a callback: you may be inside a `Ui` borrow.

| Event | Emit when | Don't emit when |
|---|---|---|
| `Click` | a button is pressed (mouse, keyboard or accessibility) | |
| `Changed(Text)` | the user (or assistive technology) edits a text field | the core set the value. **GTK `changed` and WinUI `TextChanged` fire on programmatic sets**, so block or ignore them during `SetProp`. |
| `Changed(Bool)` | the user toggles a checkbox or switch | the core set `Checked`. **GTK `toggled`/`notify::active` and WinUI `Checked`/`Unchecked`/`Toggled` fire on programmatic sets**, so guard them. |
| `Submit` | **Return/Enter** in a text field (GTK `activate`; WinUI `KeyDown` with `Enter`) | editing ends in other ways: Tab, a click elsewhere, focus loss. AppKit's field action does fire then; that was a real bug. |
| `FocusIn` / `FocusOut` | keyboard focus moves, **from any source** (click, Tab, code): out for the old control first, then in for the new | |
| `Scrolled(offset)` | a ScrollView's offset changes, by the user **or** by `ScrollTo` | |
| `WindowResized(size)` | the window's content area changes size (report the content size, excluding any menu bar you placed in the window) | |
| `WindowCloseRequested` | the user asks to close a window. **Don't close it**: the app decides, and the core sends `Destroy`. | |
| `MetricsChanged` | text scale, scale factor, theme or contrast changes | |

Focus tracking needs one global observer, not per-widget guesses. Examples: AppKit uses KVO on `NSWindow.firstResponder`, GTK can use `notify::focus-widget` on the window, and WinUI can use `FocusManager.GotFocus`/`LostFocus`. Map the focused native object to the nearest known node by walking up its parents. Composite widgets (a text field's inner editor, a scrolled window's viewport) put focus on children you didn't create.

## 5. Measuring

`measure(id, request) -> Size` is called **synchronously during layout**, after the current batch's structure and props have been applied. It's only called for leaves; containers are never measured.

- `known_width` / `known_height`: already fixed. Measure the other axis given them, and return the known value unchanged.
- `available_width` / `available_height`: `Definite(w)` (wrap text to `w`), `MinContent` (the narrowest sensible width, e.g. the longest word) or `MaxContent` (no wrapping).
- Return logical units, rounded up (`ceil`), so text is never clipped by a fraction.
- Text inputs often have no intrinsic width; AppKit uses 200. Use something sensible and consistent.

Platform hints:
- **GTK:** `widget.measure(Orientation, for_size)` gives the minimum and natural sizes. Use natural for max-content and minimum for min-content.
- **WinUI:** `element.Measure(available)` then `DesiredSize`. Check early (the spike) whether elements must be in the live tree to measure.

Known gap on AppKit: min-content falls back to max-content. If your platform gives min-content cheaply (GTK does), implement it properly.

## 6. Metrics

`metrics()` returns:
- font sizes for each `TextStyle` from the platform type ramp (body 13pt on macOS, around 14–15 on WinUI and GNOME);
- the spacing tokens `xs…xl` in the platform's design language (AppKit: 4/6/8/12/20; pick yours from the GNOME HIG or Fluent);
- the scale factor, dark mode, high contrast and reduced motion.

Emit `MetricsChanged` when any of these change.

## 7. Test hooks: perform, synthesize, native_state, capture

These make one test suite run against every backend.

- **`perform(id, action)`**: do what assistive technology would.
  - `Activate`: press the button or toggle the control. Prefer the platform's accessibility press (AppKit `accessibilityPerformPress`; its return value lies for offscreen windows, so the result is ignored).
  - `SetValue(text)`: set the field's text, then emit `Changed(Text)` yourself, because an assistive technology edit is a user edit.
  - `Focus`: move keyboard focus to the control.
  - Return `ActionError::Disabled` for disabled controls and `Unsupported` for actions that don't apply.
- **`synthesize(id, input)`**: behave as close to real input as the platform allows.
  - `Key(Char | Backspace | Enter | Tab)` on text fields must go through the platform's text-editing path, so the real signals fire. AppKit drives the field editor (`insertText:`, `doCommandBySelector:`).
  - If the field wasn't focused, focus it and **put the caret at the end**: focusing selects all, and the first keystroke would replace everything.
  - Enter or Space on buttons, Space on toggles.
  - `Scroll { dx, dy }` scrolls a ScrollView like a scroll wheel would, clamped.
- **`native_state(id)`**: **read back from the widget** what it actually shows: text, title, value, placeholder, checked, enabled, frame, children (in native order), focused, and scroll offset. Only cache what the platform can't report (AppKit caches the text style and variant). After every settle, the test harness compares this with the core and fails on any difference. This check has caught every serious backend bug so far.
- **`capture(id, reply)`**: offscreen RGBA8 at backing scale, rows top to bottom. Reply when the image is ready, right away if possible. Examples:
  - AppKit: `cacheDisplayInRect:toBitmapImageRep:`, which replies immediately.
  - GTK: `gtk::WidgetPaintable` + snapshot + `render_texture`, then download.
  - WinUI: `RenderTargetBitmap.RenderAsync`, then `GetPixelsAsync`, replying from the completion.

## 8. Services

Implement `Services`. **Never block**: reply later, from the platform's completion callback.

| Service | AppKit | GTK 4 | WinUI 3 |
|---|---|---|---|
| clipboard read (async reply) | `NSPasteboard` (replies immediately) | `gdk::Clipboard::read_text_async` | `Clipboard.GetContent().GetTextAsync()` |
| clipboard write (async reply, can fail) | `NSPasteboard` (replies immediately) | `gdk::Clipboard::set_text` | `Clipboard.SetContent` (throws while another process holds the clipboard: reply `Err`) |
| alert | `NSAlert` sheet on the parent | `gtk::AlertDialog::choose` | `ContentDialog` (one at a time per window) |
| open / save | `NSOpenPanel` / `NSSavePanel` sheets with `UTType` filters | `gtk::FileDialog` (`open`/`open_multiple`/`save`) with `gtk::FileFilter` | `FileOpenPicker` / `FileSavePicker` (need the window handle: `InitializeWithWindow`) |
| menus | the global `NSMenu` bar: app menu, the app's File, Edit, the rest | `gio::Menu` on the application (`set_menubar`) or a `PopoverMenuBar` in each window | a `MenuBar` in each window |

- **`parent: None`** means the active window: AppKit uses the key window, then the main window. Only fall back to app-modal if there is no window.
- **Menus inside the window** (GTK without a global menu, WinUI): the menu bar takes space the core doesn't know about. Put it above your content host, and report the **remaining** content size in `WindowResized`.
- Keep the platform's standard menus (Quit, Edit with Cut/Copy/Paste/Undo) and leave their enabling to the platform. The app's own items follow its `enabled` state.
- `Shortcut::primary` is Ctrl on GTK and WinUI.

## 9. Tab order

`SetFocusOrder` gives the window-wide order. Platforms differ in how to impose it:

- **AppKit:** turn off `autorecalculatesKeyViewLoop` and link `nextKeyView` into a loop.
- **GTK 4:** there is no "next widget" pointer. Handle Tab and Shift+Tab in a capture-phase key controller on the window, then `grab_focus` the next widget in the order that is focusable, sensitive and visible.
- **WinUI:** set `TabIndex` to each control's position in the order. Make sure one focus scope covers the window (check `TabFocusNavigation` on the hosts).

The conformance tests use three controls arranged so that reading order and position on screen disagree (RTL rows, absolute positioning, `tab_index`). An order based on position fails them, as it should.

## 10. The run loop

`run(setup)` owns the platform's main loop (see `mitsuami-appkit/src/app.rs`):

1. Create the platform app, your backend and `Ui::new(backend)`. Install the standard menus with `ui.set_menu(MenuBar::new())`.
2. `ui.set_commit_scheduler(wake)`: called when something changed; make the loop turn soon.
3. `ui.set_waker(Arc<dyn Fn() + Send + Sync>)`: **thread-safe** wake-up, called when background work completes a task.
4. `setup(&ui)` (the app creates its windows), then `ui.tick()`, **then** show the windows, so nobody sees an unlaid-out frame.
5. Call `ui.tick()` whenever the loop is about to sleep, and again after each wake-up. After each tick, re-arm **one** timer for `ui.time_to_next_timer()`.
6. Stop when `ui.windows()` is empty.

Platform hints:

| Step | AppKit (done) | GTK 4 | WinUI 3 |
|---|---|---|---|
| tick before sleeping | `CFRunLoopObserver` (BeforeWaiting, common modes) | an idle source that is re-added when scheduled (`glib::idle_add_local_once`), guarded by a "scheduled" flag | `DispatcherQueue.TryEnqueue`, guarded the same way |
| thread-safe wake | `CFRunLoopWakeUp` | `glib::MainContext::default().invoke(...)` to schedule the tick | `DispatcherQueue.TryEnqueue` (thread-safe) |
| timer | one `CFRunLoopTimer`, re-armed | a `glib::timeout_add_local_once` replaced on re-arm | `DispatcherQueueTimer` |
| stop | `stop:` **plus an empty posted event** (otherwise it waits for the next real event) | `app.quit()` | `Application.Exit()` / close the last window |

Never call `tick()` from inside a widget callback; the loop calls it.

## 11. Sync and async in the contract

The contract is async wherever **any** platform might complete the work later. Reply-based methods take a `Reply<T>` (a `FnOnce(T)`). Call it exactly once, from a completion handler if needed, or right away when the platform is synchronous.

| Async (reply) | Why |
|---|---|
| `capture` | WinUI renders to bitmaps asynchronously |
| `clipboard_text`, `set_clipboard_text` | GTK and WinUI clipboards are async; writes can fail |
| `alert`, `open_file`, `save_file` | the user answers later |

| Sync | Why it can stay sync |
|---|---|
| `measure` | layout needs the answer now; every platform measures synchronously |
| `native_state`, `metrics` | plain property reads |
| `apply`, `set_menu` | instructions with no answer |
| `perform`, `synthesize` | the `Result` only says whether the action was accepted; its effects arrive later as events |

If your platform can only do one of the sync ones asynchronously, raise it: we change the contract rather than blocking the UI thread.

## 12. Testing your backend

```sh
MITSUAMI_NATIVE=1 cargo test                    # all app suites on your backend
MITSUAMI_NATIVE=1 cargo test -p mitsuami --test conformance   # the contract
MITSUAMI_SHOW_WINDOWS=1 MITSUAMI_NATIVE=1 cargo test          # watch it
cargo run -p mitsuami --example run_loop_smoke  # timers, background wake-up, exit (must exit by itself)
cargo run -p mitsuami --example showcase        # look at it
```

- **`tests/conformance.rs` is the contract.** Get it green first; the other suites mostly follow.
- **Mirror checks** run after every settle, comparing native and core children, props, frames, focus and scroll offsets. A failure names the node and the difference.
- **Visual baselines** are stored per backend in `tests/visual/<name>/`. The first run creates them; look at them.
- **Headless-only tests** (fake metrics, simulated system changes) are skipped in native runs.
- **Your real services** aren't exercised by app tests, which use a scripted fake. Copy `mitsuami-appkit/tests/services.rs`: a private clipboard if possible, the real menu structure plus an activation, an alert answered through its real button, and a cancelled file dialog.

## 13. Suggested order

1. Window + Container + Text, `SetFrame`, `measure` → the counter layout test passes natively.
2. Button, events, `perform`, `native_state` → the counter suite passes.
3. TextInput, Checkbox and Switch, including the "no events on programmatic set" guards and Enter-only submit → the forms suite passes.
4. Focus tracking, `SetFocusOrder`, `synthesize` → the Tab and focus conformance tests pass.
5. ScrollView → the scrolling conformance tests pass.
6. `run()` with tick, waker and timer → `run_loop_smoke` exits by itself; the showcase works.
7. Services + native services checks.
8. `capture` + visual baselines.

When something in the contract doesn't fit your platform, **change the contract rather than working around it**, and update the other backends and this guide. Capture and the clipboard became async for exactly this reason.
