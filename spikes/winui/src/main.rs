//! M0.5 WinUI spike: a window, a Canvas, a Button and a TextBlock, created,
//! positioned, measured and clicked imperatively, the way mitsuami's command
//! protocol would drive them. No windows-reactor at runtime.
//!
//! `cargo run` runs the scripted checks, prints a report and exits.
//! `cargo run -- --interactive` leaves the window open for clicking.

#[allow(dead_code, non_camel_case_types, non_snake_case, non_upper_case_globals, unused_qualifications, clippy::all)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

use bindings::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use windows_core::*;

fn main() -> Result<()> {
    bootstrap()?;
    unsafe {
        _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32).ok()?;
    }
    let interactive = std::env::args().any(|a| a == "--interactive");
    Application::Start(&ApplicationInitializationCallback::new(move |_| {
        let app = Application::compose(SpikeApp { provider: RefCell::new(None), interactive });
        // XAML keeps the application alive; we just must not release it early.
        std::mem::forget(app);
    }))
}

// --- Bootstrap: framework-dependent, like windows-reactor's bootstrap.rs ---

fn bootstrap() -> Result<()> {
    let mut id = PWSTR::null();
    unsafe {
        TryCreatePackageDependency(
            std::ptr::null_mut(),
            w!("Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe"),
            PACKAGE_VERSION {
                Anonymous: PACKAGE_VERSION_0 {
                    Version: 0x0002_0004_0000_0000, // 2.4.0.0
                },
            },
            PackageDependencyProcessorArchitectures_X64 | PackageDependencyProcessorArchitectures_Neutral,
            0, // lifetime: process
            PCWSTR::null(),
            0,
            &mut id,
        )
        .ok()?;
        let mut context = std::ptr::null_mut();
        let mut full_name = PWSTR::null();
        AddPackageDependency(PCWSTR(id.0), 0, 0, &mut context, &mut full_name).ok()?;
        report("bootstrap", full_name.to_string().unwrap_or_default());
        _ = HeapFree(GetProcessHeap(), 0, id.0.cast());
        _ = HeapFree(GetProcessHeap(), 0, full_name.0.cast());
    }
    Ok(())
}

// --- Application: composed from Rust, no XAML subclass codegen ---

pub struct SpikeApp {
    provider: RefCell<Option<XamlControlsXamlMetaDataProvider>>,
    interactive: bool,
}

implement_decl! {
    impl SpikeApp as pub SpikeApp_Impl: [IApplicationOverrides, IXamlMetadataProvider]
}

impl SpikeApp {
    fn provider(&self) -> Result<XamlControlsXamlMetaDataProvider> {
        let mut provider = self.provider.borrow_mut();
        if provider.is_none() {
            *provider = Some(XamlControlsXamlMetaDataProvider::new()?);
        }
        Ok(provider.clone().unwrap())
    }
}

impl IApplicationOverrides_Impl for SpikeApp_Impl {
    fn OnLaunched(&self, _args: Ref<LaunchActivatedEventArgs>) -> Result<()> {
        // Control templates, Button's among them, live in XamlControlsResources.
        let resources: ResourceDictionary = XamlControlsResources::new()?.cast()?;
        Application::Current()?.cast::<IApplication>()?.Resources()?.MergedDictionaries()?.Append(&resources)?;
        launched(self.interactive).inspect_err(|e| report("error", e))
    }
}

impl IXamlMetadataProvider_Impl for SpikeApp_Impl {
    fn GetXamlType(&self, r#type: &TypeName) -> Result<IXamlType> {
        self.provider()?.GetXamlType(r#type)
    }
    fn GetXamlTypeByFullName(&self, full_name: &HSTRING) -> Result<IXamlType> {
        self.provider()?.GetXamlTypeByFullName(&full_name.to_string_lossy())
    }
    fn GetXmlnsDefinitions(&self) -> Result<Array<XmlnsDefinition>> {
        self.provider()?.GetXmlnsDefinitions()
    }
}

// --- The spike proper ---

const INFINITE: Size = Size { width: f32::INFINITY, height: f32::INFINITY };

struct Spike {
    window: Window,
    canvas: Canvas,
    button: Button,
    label: TextBlock,
    clicks: Rc<Cell<u32>>,
    _revokers: Vec<EventRevoker>,
}

thread_local! {
    static SPIKE: RefCell<Option<Spike>> = const { RefCell::new(None) };
}

fn launched(interactive: bool) -> Result<()> {
    let window = Window::new()?;
    window.SetTitle("mitsuami WinUI spike")?;
    window
        .cast::<IWindow2>()?
        .AppWindow()?
        .cast::<IAppWindow2>()?
        .ResizeClient(SizeInt32 { width: 480, height: 320 })?;

    // Layout host: a Canvas that does no layout; we place children.
    let canvas = Canvas::new()?;
    let button = Button::new()?;
    button.cast::<IContentControl>()?.SetContent(&text("Click me")?)?;
    let label = TextBlock::new()?;
    label.cast::<ITextBlock>()?.SetText("The quick brown fox jumps over the lazy dog, twice.")?;
    label.cast::<ITextBlock>()?.SetTextWrapping(TextWrapping::Wrap)?;

    // 1. Measure before the element is in any tree.
    report("measure button, detached", measure(&button, INFINITE)?);
    report("measure label, detached", measure(&label, INFINITE)?);

    // Insert and place, like Command::Insert + SetFrame would.
    let children = canvas.cast::<IPanel>()?.Children()?;
    children.Append(&button.cast::<UIElement>()?)?;
    children.Append(&label.cast::<UIElement>()?)?;
    window.SetContent(&canvas)?;

    // 2. In the tree, window not yet activated.
    report("measure button, in tree, not shown", measure(&button, INFINITE)?);

    let clicks = Rc::new(Cell::new(0));
    let mut revokers = Vec::new();
    revokers.push(button.cast::<IButtonBase>()?.Click({
        let clicks = clicks.clone();
        move |_, _| {
            clicks.set(clicks.get() + 1);
            report("click handler", format!("count = {}", clicks.get()));
            if interactive {
                _ = with_spike(|s| {
                    let label = format!("Clicked {} times", s.clicks.get());
                    s.button.cast::<IContentControl>()?.SetContent(&text(&label)?)
                });
            }
        }
    })?);
    revokers.push(canvas.cast::<IFrameworkElement>()?.Loaded(move |_, _| {
        if let Err(e) = loaded(interactive) {
            report("error", e);
        }
    })?);

    SPIKE.with(|s| {
        *s.borrow_mut() = Some(Spike { window: window.clone(), canvas, button, label, clicks, _revokers: revokers })
    });
    window.Activate()
}

fn loaded(interactive: bool) -> Result<()> {
    with_spike(|s| {
        let scale = s.canvas.cast::<IUIElement>()?.XamlRoot()?.RasterizationScale()?;
        report("rasterization scale", scale);

        // 3. Live tree: the realistic case for the backend's measure().
        let button_size = measure(&s.button, INFINITE)?;
        report("measure button, live", button_size);
        let one_line = measure(&s.label, INFINITE)?;
        report("measure label, live, unconstrained", one_line);
        let wrapped = measure(&s.label, Size { width: 120.0, height: f32::INFINITY })?;
        report("measure label, live, width 120", wrapped);
        let min_content = measure(&s.label, Size { width: 0.0, height: f32::INFINITY })?;
        report("measure label, live, width 0", min_content);

        // Place with our frames, like SetFrame: Canvas.Left/Top + Width/Height.
        place(&s.button, 16.0, 16.0, button_size)?;
        place(&s.label, 16.0, 64.0, wrapped)?;

        // Content change, then an immediate re-measure with no layout pass between.
        s.button.cast::<IContentControl>()?.SetContent(&text("A much longer button label")?)?;
        s.label.cast::<ITextBlock>()?.SetText("Short")?;
        // Naively: the explicit frame Width/Height wins over the content.
        report("measure button, frame size set", measure(&s.button, INFINITE)?);
        report("measure label, frame size set", measure(&s.label, INFINITE)?);
        // With the frame size lifted to Auto (NaN) for the measure call.
        report("measure button, frame size lifted", measure_intrinsic(&s.button, INFINITE, button_size)?);
        report("measure label, frame size lifted", measure_intrinsic(&s.label, INFINITE, wrapped)?);
        s.button.cast::<IContentControl>()?.SetContent(&text("Click me")?)?;
        s.label.cast::<ITextBlock>()?.SetText("The quick brown fox jumps over the lazy dog, twice.")?;
        report("measure button, restored content", measure_intrinsic(&s.button, INFINITE, button_size)?);

        // A control created after the window is live, as a later render would.
        let late = Button::new()?;
        late.cast::<IContentControl>()?.SetContent(&text("Late")?)?;
        report("measure late button, detached", measure(&late, INFINITE)?);
        let children = s.canvas.cast::<IPanel>()?.Children()?;
        children.Append(&late.cast::<UIElement>()?)?;
        report("measure late button, just inserted", measure(&late, INFINITE)?);
        place(&late, 16.0, 140.0, measure(&late, INFINITE)?)?;

        // Does our explicit Width/Height stick through XAML's own layout pass?
        s.canvas.cast::<IUIElement>()?.UpdateLayout()?;
        let fe = s.button.cast::<IFrameworkElement>()?;
        report("button actual size after UpdateLayout", format!("{} x {}", fe.ActualWidth()?, fe.ActualHeight()?));

        // 4. Activate through UI Automation, as the test driver would.
        let before = s.clicks.get();
        let peer = FrameworkElementAutomationPeer::CreatePeerForElement(&s.button)?;
        let invoke: IInvokeProvider = peer.GetPattern(PatternInterface::Invoke)?.cast()?;
        invoke.Invoke()?;
        report("UIA Invoke ran Click synchronously", s.clicks.get() > before);
        Ok(())
    })?;

    // 5. The flush hook: work posted to the UI thread's DispatcherQueue.
    let queue = DispatcherQueue::GetForCurrentThread()?;
    queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
        let clicks = with_spike(|s| Ok(s.clicks.get())).unwrap_or_default();
        report("dispatcher queue callback, clicks seen", clicks);
        if !interactive && let Err(e) = capture() {
            report("error", e);
            finish();
        }
    }))?;
    Ok(())
}

thread_local! {
    static CAPTURE: RefCell<Option<RenderTargetBitmap>> = const { RefCell::new(None) };
}

/// Step 6, `Backend::capture`: RenderTargetBitmap of the root, written as a
/// PNG. Completions arrive on the UI thread, so the non-Send XAML objects stay in a
/// thread-local instead of moving into the (Send) callbacks.
fn capture() -> Result<()> {
    let bitmap = RenderTargetBitmap::new()?;
    let render = with_spike(|s| bitmap.RenderAsync(&s.canvas.cast::<UIElement>()?))?;
    CAPTURE.with(|c| *c.borrow_mut() = Some(bitmap));
    render.when(|rendered| {
        let pixels = rendered.and_then(|()| CAPTURE.with(|c| c.borrow().as_ref().unwrap().GetPixelsAsync()));
        let result = pixels.and_then(|op| {
            op.when(|buffer| {
                if let Err(e) = buffer.and_then(write_png) {
                    report("error", e);
                }
                finish();
            })
        });
        if let Err(e) = result {
            report("error", e);
            finish();
        }
    })
}

fn write_png(buffer: IBuffer) -> Result<()> {
    let bitmap = CAPTURE.with(|c| c.borrow_mut().take()).unwrap();
    let (width, height) = (bitmap.PixelWidth()? as u32, bitmap.PixelHeight()? as u32);
    let mut bgra = vec![0; buffer.Length()? as usize];
    DataReader::FromBuffer(&buffer)?.ReadBytes(&mut bgra)?;
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2); // BGRA → RGBA (premultiplied; fine for opaque content)
    }
    let path = std::env::temp_dir().join("mitsuami-winui-spike.png");
    let file = std::fs::File::create(&path).map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&bgra))
        .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    report("capture", format!("{width}x{height} px → {}", path.display()));
    Ok(())
}

fn finish() {
    // Release our XAML references while XAML is still alive; letting the
    // thread-local drop them after Exit fails fast.
    if let Some(spike) = SPIKE.with(|s| s.borrow_mut().take()) {
        _ = spike.window.Close();
    }
    _ = Application::Current().and_then(|a| a.cast::<IApplication>()?.Exit());
}

// --- Helpers ---

fn with_spike<R>(f: impl FnOnce(&Spike) -> Result<R>) -> Result<R> {
    SPIKE.with(|s| f(s.borrow().as_ref().expect("spike not built")))
}

fn measure(element: &impl Interface, available: Size) -> Result<Size> {
    let element: IUIElement = element.cast()?;
    element.Measure(available)?;
    element.DesiredSize()
}

/// Measures ignoring the frame size we imposed: Width/Height go to Auto (NaN)
/// for the call and back to the frame afterwards.
fn measure_intrinsic(element: &impl Interface, available: Size, frame: Size) -> Result<Size> {
    let fe: IFrameworkElement = element.cast()?;
    fe.SetWidth(f64::NAN)?;
    fe.SetHeight(f64::NAN)?;
    let size = measure(element, available)?;
    fe.SetWidth(frame.width as f64)?;
    fe.SetHeight(frame.height as f64)?;
    Ok(size)
}

fn place(element: &impl Interface, x: f64, y: f64, size: Size) -> Result<()> {
    let element: UIElement = element.cast()?;
    Canvas::SetLeft(&element, x)?;
    Canvas::SetTop(&element, y)?;
    let fe: IFrameworkElement = element.cast()?;
    fe.SetWidth(size.width as f64)?;
    fe.SetHeight(size.height as f64)
}

fn text(value: &str) -> Result<IInspectable> {
    PropertyValue::CreateString(value)
}

fn report(what: &str, value: impl std::fmt::Debug) {
    println!("{what:<40} {value:?}");
}
