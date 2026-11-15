//! WebView2 host for the preview panel.

use std::fs;
use std::path::Path;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, ContextMenuRequestedEventHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PrintToPdfCompletedHandler, ProcessFailedEventHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler, ZoomFactorChangedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{HSTRING, Interface, PWSTR};

use crate::assets;
use crate::guard::guarded;
use crate::render::urls;

const MAX_LOCAL_FILE: u64 = 64 * 1024 * 1024;
const MENU_KEEP: &[&str] = &["copy", "selectAll", "copyLinkLocation", "copyImage", "copyImageLocation"];

#[derive(Clone, Debug)]
pub struct Host {
    pub env: ICoreWebView2Environment,
    pub controller: ICoreWebView2Controller,
    pub webview: ICoreWebView2,
}

/// Callbacks from the web view, all invoked on the UI thread.
pub trait Events: 'static {
    fn created(&self, host: windows::core::Result<Host>);
    fn message(&self, json: String);
    fn external_link(&self, uri: String);
    fn accelerator(&self, vkey: u32) -> bool;
    fn zoom_changed(&self, zoom: f64);
    fn process_failed(&self, browser_gone: bool);
}

fn rgb(c: u32) -> COREWEBVIEW2_COLOR {
    COREWEBVIEW2_COLOR { A: 255, R: (c & 0xff) as u8, G: ((c >> 8) & 0xff) as u8, B: ((c >> 16) & 0xff) as u8 }
}

/// Starts asynchronous creation. `events.created` is called when done.
pub fn create<E: Events + Clone>(parent: HWND, user_data: &Path, background: u32, zoom: f64, events: E) {
    let options: ICoreWebView2EnvironmentOptions = CoreWebView2EnvironmentOptions::default().into();
    let ev = events.clone();
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |hr, env| {
        guarded(|| {
            let env = match hr.and_then(|_| env.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))) {
                Ok(env) => env,
                Err(e) => {
                    ev.created(Err(e));
                    return;
                }
            };
            let ev2 = ev.clone();
            let env2 = env.clone();
            let ctrl_handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |hr, controller| {
                guarded(|| {
                    let result = hr
                        .and_then(|_| controller.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER)))
                        .and_then(|controller| setup(parent, env2.clone(), controller, background, zoom, ev2.clone()));
                    ev2.created(result);
                });
                Ok(())
            }));
            // SAFETY: parent is a live window owned by this thread.
            if let Err(e) = unsafe { env.CreateCoreWebView2Controller(parent, &ctrl_handler) } {
                ev.created(Err(e));
            }
        });
        Ok(())
    }));
    let folder = HSTRING::from(user_data.as_os_str());
    // SAFETY: arguments are valid for the duration of the call; completion arrives through the message loop.
    if let Err(e) = unsafe { CreateCoreWebView2EnvironmentWithOptions(None, &folder, &options, &handler) } {
        events.created(Err(e));
    }
}

fn setup<E: Events + Clone>(
    parent: HWND,
    env: ICoreWebView2Environment,
    controller: ICoreWebView2Controller,
    background: u32,
    zoom: f64,
    events: E,
) -> windows::core::Result<Host> {
    // SAFETY: all calls are COM calls on the UI thread with live interfaces.
    unsafe {
        let webview = controller.CoreWebView2()?;
        if let Ok(c2) = controller.cast::<ICoreWebView2Controller2>() {
            let _ = c2.SetDefaultBackgroundColor(rgb(background));
        }
        let settings = webview.Settings()?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetAreDevToolsEnabled(cfg!(debug_assertions))?;
        settings.SetAreDefaultScriptDialogsEnabled(false)?;
        settings.SetIsZoomControlEnabled(true)?;
        settings.SetIsBuiltInErrorPageEnabled(false)?;
        if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
            let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
        }
        if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
            let _ = s4.SetIsGeneralAutofillEnabled(false);
            let _ = s4.SetIsPasswordAutosaveEnabled(false);
        }
        if let Ok(s6) = settings.cast::<ICoreWebView2Settings6>() {
            let _ = s6.SetIsSwipeNavigationEnabled(false);
        }

        let mut token = 0i64;
        webview.AddWebResourceRequestedFilter(&HSTRING::from(format!("{}*", urls::ORIGIN)), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
        let env_res = env.clone();
        webview.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    guarded(|| {
                        let _ = serve(&env_res, &args);
                    });
                }
                Ok(())
            })),
            &mut token,
        )?;

        let ev = events.clone();
        webview.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut raw = PWSTR::null();
                    if args.WebMessageAsJson(&mut raw).is_ok() {
                        let json = take_pwstr(raw);
                        guarded(|| ev.message(json));
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        let ev = events.clone();
        webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut raw = PWSTR::null();
                    if args.Uri(&mut raw).is_ok() {
                        let uri = take_pwstr(raw);
                        if !uri.starts_with(urls::ORIGIN) {
                            let _ = args.SetCancel(true);
                            guarded(|| ev.external_link(uri));
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        let ev = events.clone();
        webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let _ = args.SetHandled(true);
                    let mut raw = PWSTR::null();
                    if args.Uri(&mut raw).is_ok() {
                        let uri = take_pwstr(raw);
                        guarded(|| ev.external_link(uri));
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        let ev = events.clone();
        webview.add_ProcessFailed(
            &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                    if args.ProcessFailedKind(&mut kind).is_ok() {
                        let browser_gone = kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED;
                        let renderer = kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                            || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE;
                        if browser_gone || renderer {
                            guarded(|| ev.process_failed(browser_gone));
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        if let Ok(w11) = webview.cast::<ICoreWebView2_11>() {
            w11.add_ContextMenuRequested(
                &ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        guarded(|| filter_context_menu(&args));
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }

        let ev = events.clone();
        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                    let mut vkey = 0u32;
                    if args.KeyEventKind(&mut kind).is_ok()
                        && (kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN)
                        && args.VirtualKey(&mut vkey).is_ok()
                    {
                        let mut handled = false;
                        guarded(|| handled = ev.accelerator(vkey));
                        if handled {
                            let _ = args.SetHandled(true);
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        let ev = events.clone();
        controller.add_ZoomFactorChanged(
            &ZoomFactorChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(c) = sender {
                    let mut z = 1.0f64;
                    if c.ZoomFactor(&mut z).is_ok() {
                        guarded(|| ev.zoom_changed(z));
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        let _ = controller.SetZoomFactor(zoom);
        let host = Host { env, controller, webview };
        host.fit(parent);
        host.webview.Navigate(&HSTRING::from(format!("{}index.html", urls::ORIGIN)))?;
        Ok(host)
    }
}

fn filter_context_menu(args: &ICoreWebView2ContextMenuRequestedEventArgs) {
    // SAFETY: COM calls on live interfaces from the event callback.
    unsafe {
        let Ok(items) = args.MenuItems() else { return };
        let mut count = 0u32;
        if items.Count(&mut count).is_err() {
            return;
        }
        for i in (0..count).rev() {
            let Ok(item) = items.GetValueAtIndex(i) else { continue };
            let mut raw = PWSTR::null();
            let keep = item.Name(&mut raw).is_ok() && MENU_KEEP.contains(&take_pwstr(raw).as_str());
            if !keep {
                let _ = items.RemoveValueAtIndex(i);
            }
        }
        let mut left = 0u32;
        if items.Count(&mut left).is_ok() && left == 0 {
            let _ = args.SetHandled(true);
        }
    }
}

fn serve(env: &ICoreWebView2Environment, args: &ICoreWebView2WebResourceRequestedEventArgs) -> windows::core::Result<()> {
    // SAFETY: COM calls on live interfaces from the event callback.
    unsafe {
        let request = args.Request()?;
        let mut raw = PWSTR::null();
        request.Uri(&mut raw)?;
        let uri = take_pwstr(raw);
        let (status, reason, mime, body) = resolve(&uri);
        let stream = SHCreateMemStream(Some(&body));
        let headers = format!("Content-Type: {mime}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff");
        let response = env.CreateWebResourceResponse(stream.as_ref(), status, &HSTRING::from(reason), &HSTRING::from(headers))?;
        args.SetResponse(&response)
    }
}

fn resolve(uri: &str) -> (i32, &'static str, &'static str, Vec<u8>) {
    const NOT_FOUND: (i32, &str, &str, Vec<u8>) = (404, "Not Found", "text/plain", Vec::new());
    if let Some(path) = urls::path_from_file_url(uri) {
        let Some(mime) = urls::image_mime(&path) else { return NOT_FOUND };
        return match fs::metadata(&path) {
            Ok(m) if m.is_file() && m.len() <= MAX_LOCAL_FILE => match fs::read(&path) {
                Ok(data) => (200, "OK", mime, data),
                Err(_) => NOT_FOUND,
            },
            _ => NOT_FOUND,
        };
    }
    let rel = uri.strip_prefix(urls::ORIGIN).unwrap_or("");
    let rel = rel.split(['?', '#']).next().unwrap_or("");
    match assets::get(rel) {
        Some((mime, data)) => (200, "OK", mime, data.to_vec()),
        None => NOT_FOUND,
    }
}

impl Host {
    pub fn fit(&self, parent: HWND) {
        let mut rc = RECT::default();
        // SAFETY: parent is a live window; controller is live.
        unsafe {
            if GetClientRect(parent, &mut rc).is_ok() {
                let _ = self.controller.SetBounds(rc);
            }
        }
    }

    pub fn set_visible(&self, visible: bool) {
        // SAFETY: COM call on a live controller.
        unsafe {
            let _ = self.controller.SetIsVisible(visible);
        }
    }

    pub fn focus(&self) {
        // SAFETY: COM call on a live controller.
        unsafe {
            let _ = self.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        }
    }

    pub fn post(&self, json: &str) {
        // SAFETY: COM call on a live web view; the string is copied by the callee.
        unsafe {
            let _ = self.webview.PostWebMessageAsJson(&HSTRING::from(json));
        }
    }

    pub fn set_background(&self, color: u32) {
        if let Ok(c2) = self.controller.cast::<ICoreWebView2Controller2>() {
            // SAFETY: COM call on a live controller.
            unsafe {
                let _ = c2.SetDefaultBackgroundColor(rgb(color));
            }
        }
    }

    pub fn set_zoom(&self, zoom: f64) {
        // SAFETY: COM call on a live controller.
        unsafe {
            let _ = self.controller.SetZoomFactor(zoom);
        }
    }

    pub fn reload(&self) {
        // SAFETY: COM call on a live web view.
        unsafe {
            let _ = self.webview.Reload();
        }
    }

    pub fn close(&self) {
        // SAFETY: COM call on a live controller.
        unsafe {
            let _ = self.controller.Close();
        }
    }

    /// Prints the current page to PDF. `done` gets `true` on success.
    pub fn print_pdf(&self, path: &Path, letter: bool, done: impl FnOnce(bool) + 'static) -> windows::core::Result<()> {
        // SAFETY: COM calls on live interfaces; completion arrives on the UI thread.
        unsafe {
            let wv7 = self.webview.cast::<ICoreWebView2_7>()?;
            let env6 = self.env.cast::<ICoreWebView2Environment6>()?;
            let ps = env6.CreatePrintSettings()?;
            ps.SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT)?;
            if letter {
                ps.SetPageWidth(8.5)?;
                ps.SetPageHeight(11.0)?;
            } else {
                ps.SetPageWidth(8.27)?;
                ps.SetPageHeight(11.69)?;
            }
            ps.SetMarginTop(0.6)?;
            ps.SetMarginBottom(0.6)?;
            ps.SetMarginLeft(0.6)?;
            ps.SetMarginRight(0.6)?;
            ps.SetShouldPrintBackgrounds(true)?;
            ps.SetShouldPrintHeaderAndFooter(false)?;
            let handler = PrintToPdfCompletedHandler::create(Box::new(move |hr, ok| {
                guarded(move || done(hr.is_ok() && ok));
                Ok(())
            }));
            wv7.PrintToPdf(&HSTRING::from(path.as_os_str()), &ps, &handler)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_assets_and_rejects_others() {
        assert_eq!(resolve("https://mahimarkd.localhost/index.html").0, 200);
        assert_eq!(resolve("https://mahimarkd.localhost/app.js?v=1").0, 200);
        assert_eq!(resolve("https://mahimarkd.localhost/../secret").0, 404);
        assert_eq!(resolve("https://mahimarkd.localhost/file/C:/Windows/win.ini").0, 404);
    }
}
