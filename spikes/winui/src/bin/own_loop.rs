//! Can XAML run without Application::Start, on our own message loop?

#[allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_qualifications,
    clippy::all
)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

use bindings::*;
use std::time::{Duration, Instant};
use windows_core::*;

pub struct App;

implement_decl! {
    impl App as pub App_Impl: [IApplicationOverrides, IXamlMetadataProvider]
}

thread_local! {
    static PROVIDER: XamlControlsXamlMetaDataProvider = XamlControlsXamlMetaDataProvider::new().unwrap();
}

impl IApplicationOverrides_Impl for App_Impl {
    fn OnLaunched(&self, _args: Ref<LaunchActivatedEventArgs>) -> Result<()> {
        println!("OnLaunched called (unexpected)");
        Ok(())
    }
}

impl IXamlMetadataProvider_Impl for App_Impl {
    fn GetXamlType(&self, r#type: &TypeName) -> Result<IXamlType> {
        PROVIDER.with(|p| p.GetXamlType(r#type))
    }
    fn GetXamlTypeByFullName(&self, full_name: &HSTRING) -> Result<IXamlType> {
        PROVIDER.with(|p| p.GetXamlTypeByFullName(&full_name.to_string_lossy()))
    }
    fn GetXmlnsDefinitions(&self) -> Result<Array<XmlnsDefinition>> {
        PROVIDER.with(|p| p.GetXmlnsDefinitions())
    }
}

fn pump() {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE as u32).as_bool() {
            _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn measure(element: &impl Interface) -> Result<Size> {
    let element: IUIElement = element.cast()?;
    element.Measure(Size { width: f32::INFINITY, height: f32::INFINITY })?;
    element.DesiredSize()
}

fn main() -> Result<()> {
    winui_bootstrap();
    unsafe {
        _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32).ok()?;
    }
    let _controller = DispatcherQueueController::CreateOnCurrentThread()?;
    println!("dispatcher queue controller: ok");
    let app = Application::compose(App)?;
    println!("application composed; Current: {:?}", Application::Current().is_ok());
    let _xaml = WindowsXamlManager::InitializeForCurrentThread()?;
    println!("windows xaml manager: ok");
    let resources: ResourceDictionary = XamlControlsResources::new()?.cast()?;
    app.cast::<IApplication>()?.Resources()?.MergedDictionaries()?.Append(&resources)?;

    let window = Window::new()?;
    println!("window created");
    let canvas = Canvas::new()?;
    let button = Button::new()?;
    button.cast::<IContentControl>()?.SetContent(&PropertyValue::CreateString("Own loop")?)?;
    canvas.cast::<IPanel>()?.Children()?.Append(&button.cast::<UIElement>()?)?;
    window.SetContent(&canvas)?;
    println!("measure before Activate: {:?}", measure(&button)?);
    window.Activate()?;
    println!("measure right after Activate: {:?}", measure(&button)?);
    let has_root = button.cast::<IUIElement>()?.XamlRoot().is_ok();
    println!("xaml root right after Activate: {has_root}");

    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let _revoker = button.cast::<IButtonBase>()?.Click({
        let clicked = clicked.clone();
        move |_, _| clicked.set(true)
    })?;
    let queued = std::rc::Rc::new(std::cell::Cell::new(false));
    DispatcherQueue::GetForCurrentThread()?.TryEnqueue(&DispatcherQueueHandler::new({
        let queued = queued.clone();
        move || queued.set(true)
    }))?;

    let start = Instant::now();
    let mut first_good = None;
    while start.elapsed() < Duration::from_millis(1500) {
        pump();
        if first_good.is_none() && measure(&button)?.width > 0.0 {
            first_good = Some(start.elapsed());
        }
        unsafe {
            MsgWaitForMultipleObjectsEx(0, std::ptr::null(), 20, QS_ALLINPUT as u32, MWMO_INPUTAVAILABLE as u32);
        }
    }
    println!("first non-zero measure after pumping: {first_good:?}");
    println!("measure after pumping: {:?}", measure(&button)?);
    println!("dispatcher queue ran from our pump: {}", queued.get());
    let peer = FrameworkElementAutomationPeer::CreatePeerForElement(&button)?;
    peer.GetPattern(PatternInterface::Invoke)?.cast::<IInvokeProvider>()?.Invoke()?;
    println!("invoke → click: {}", clicked.get());
    drop(_revoker);
    window.Close()?;
    pump();
    drop((button, canvas, window));
    pump();
    println!("closed cleanly");
    Ok(())
}

fn winui_bootstrap() {
    let mut id = PWSTR::null();
    unsafe {
        TryCreatePackageDependency(
            std::ptr::null_mut(),
            w!("Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe"),
            PACKAGE_VERSION { Anonymous: PACKAGE_VERSION_0 { Version: 0x0002_0004_0000_0000 } },
            PackageDependencyProcessorArchitectures_X64 | PackageDependencyProcessorArchitectures_Neutral,
            0,
            PCWSTR::null(),
            0,
            &mut id,
        )
        .unwrap();
        let mut context = std::ptr::null_mut();
        let mut full_name = PWSTR::null();
        AddPackageDependency(PCWSTR(id.0), 0, 0, &mut context, &mut full_name).unwrap();
    }
}
