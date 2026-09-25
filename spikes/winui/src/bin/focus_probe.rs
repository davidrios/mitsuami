//! Why does a TextBox end up focused without a GotFocus reaching the root?

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
implement_decl! { impl App as pub App_Impl: [IApplicationOverrides, IXamlMetadataProvider] }
thread_local! { static PROVIDER: XamlControlsXamlMetaDataProvider = XamlControlsXamlMetaDataProvider::new().unwrap(); }
impl IApplicationOverrides_Impl for App_Impl {
    fn OnLaunched(&self, _args: Ref<LaunchActivatedEventArgs>) -> Result<()> {
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

fn pump_for(ms: u64) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(ms) {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE as u32).as_bool() {
                _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            MsgWaitForMultipleObjectsEx(0, std::ptr::null(), 10, QS_ALLINPUT as u32, MWMO_INPUTAVAILABLE as u32);
        }
    }
}

fn state(tag: &str, a: &TextBox, b: &TextBox) {
    let f = |t: &TextBox| t.cast::<IUIElement>().unwrap().FocusState().unwrap().0;
    println!("{tag:<40} A={} B={}", f(a), f(b));
}

fn main() -> Result<()> {
    let invisible = std::env::args().any(|a| a == "--invisible");
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
        .ok()?;
        let (mut c, mut n) = (std::ptr::null_mut(), PWSTR::null());
        AddPackageDependency(PCWSTR(id.0), 0, 0, &mut c, &mut n).ok()?;
        _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32).ok()?;
    }
    std::mem::forget(DispatcherQueueController::CreateOnCurrentThread()?);
    let app = Application::compose(App)?;
    std::mem::forget(WindowsXamlManager::InitializeForCurrentThread()?);
    let res: ResourceDictionary = XamlControlsResources::new()?.cast()?;
    app.cast::<IApplication>()?.Resources()?.MergedDictionaries()?.Append(&res)?;

    let _fm = FocusManager::GotFocus(|_, args| {
        let e = args.as_ref().and_then(|a| a.NewFocusedElement().ok());
        println!("  FocusManager.GotFocus: {:?}", e.map(|e| e.as_raw()));
    })?;

    let window = Window::new()?;
    let canvas = Canvas::new()?;
    let _root_got = canvas.cast::<IUIElement>()?.GotFocus(|_, args| {
        let s = args.as_ref().and_then(|a| a.OriginalSource().ok());
        println!("  root GotFocus: {:?}", s.map(|s| s.as_raw()));
    })?;
    window.SetContent(&canvas)?;
    if invisible {
        let hwnd = window.cast::<IWindow2>()?.AppWindow()?.cast::<IAppWindow>()?.Id()?.value as usize as HWND;
        unsafe {
            let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            SetWindowLongW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED);
            _ = SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA as u32);
        }
        window.cast::<IWindow2>()?.AppWindow()?.cast::<IAppWindow>()?.Move(PointInt32 { x: -32000, y: -32000 })?;
    }
    window.Activate()?;
    pump_for(500);
    println!("window live");
    let (a, b) = (TextBox::new()?, TextBox::new()?);
    let children = canvas.cast::<IPanel>()?.Children()?;
    children.Append(&a.cast::<UIElement>()?)?;
    children.Append(&b.cast::<UIElement>()?)?;
    state("inserted (no pump)", &a, &b);
    pump_for(300);
    state("inserted + pumped", &a, &b);
    println!("programmatic focus B -> {:?}", b.cast::<IUIElement>()?.Focus(FocusState::Programmatic));
    state("after Focus(B) (no pump)", &a, &b);
    pump_for(300);
    state("after Focus(B) + pumped", &a, &b);
    window.Close()?;
    pump_for(100);
    Ok(())
}
