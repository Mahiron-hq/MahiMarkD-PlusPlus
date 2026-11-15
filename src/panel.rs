//! The dockable panel window that hosts the web view.

use std::sync::OnceLock;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DT_CENTER, DT_NOPREFIX, DT_WORDBREAK, DeleteObject, DrawTextW, EndPaint, FillRect, HDC, InvalidateRect,
    PAINTSTRUCT, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::UI::Controls::NMHDR;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, GetClientRect, HICON, IDC_ARROW, IMAGE_ICON, KillTimer, LR_DEFAULTCOLOR,
    LoadCursorW, LoadImageW, RegisterClassExW, SetTimer, WINDOW_EX_STYLE, WM_APP, WM_ERASEBKGND, WM_NOTIFY, WM_PAINT, WM_SETFOCUS, WM_SIZE,
    WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::app::{self, App, NAME};
use crate::guard::guarded_or;
use crate::npp::{self, TbData};
use crate::text::wide;

pub const WM_APP_RENDERED: u32 = WM_APP + 1;
pub const WM_APP_TOGGLE: u32 = WM_APP + 2;
pub const WM_APP_TASK: u32 = WM_APP + 3;
const TIMER_RENDER: usize = 1;
const CLASS: PCWSTR = w!("MahiMarkDPlusPlusPanel");

struct Leaked(*const u16);
// SAFETY: points to immutable, never-freed UTF-16 data.
unsafe impl Send for Leaked {}
// SAFETY: see above.
unsafe impl Sync for Leaked {}

fn leak_wide(s: &str) -> *const u16 {
    Box::leak(wide(s).into_boxed_slice()).as_ptr()
}

pub fn create(module: HINSTANCE, parent: HWND) -> Option<HWND> {
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    // SAFETY: standard window class registration and creation on the UI thread.
    unsafe {
        let ok = *REGISTERED.get_or_init(|| {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                hInstance: module,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&wc) != 0
        });
        if !ok {
            return None;
        }
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            &HSTRING::from(NAME),
            WS_CHILD | WS_CLIPCHILDREN,
            0,
            0,
            300,
            300,
            Some(parent),
            None,
            Some(module),
            None,
        )
        .ok()
    }
}

fn module_file_name(module: HINSTANCE) -> String {
    let mut buf = [0u16; 1024];
    // SAFETY: buffer is valid for its length.
    let n = unsafe { GetModuleFileNameW(Some(module.into()), &mut buf) } as usize;
    let full = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
    std::path::Path::new(&full).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| format!("{NAME}.dll"))
}

fn tab_icon(module: HINSTANCE, hwnd: HWND) -> HICON {
    // SAFETY: loads icon resource 1 from this DLL.
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        let size = (16 * dpi / 96) as i32;
        LoadImageW(Some(module), PCWSTR(1 as _), IMAGE_ICON, size, size, LR_DEFAULTCOLOR).map(|h| HICON(h.0)).unwrap_or_default()
    }
}

pub fn register_dock(app: &App, hwnd: HWND, dlg_id: i32) {
    static NAME_W: OnceLock<Leaked> = OnceLock::new();
    static MODULE_W: OnceLock<Leaked> = OnceLock::new();
    let name = NAME_W.get_or_init(|| Leaked(leak_wide(NAME))).0;
    let module = MODULE_W.get_or_init(|| Leaked(leak_wide(&module_file_name(app.module)))).0;
    let mut data = TbData {
        client: hwnd,
        name,
        dlg_id,
        mask: npp::DWS_DF_CONT_RIGHT | npp::DWS_ICONTAB | npp::DWS_USEOWNDARKMODE,
        icon_tab: tab_icon(app.module, app.npp.npp),
        add_info: std::ptr::null(),
        rc_float: RECT::default(),
        prev_cont: -1,
        module_name: module,
    };
    npp::send(app.npp.npp, npp::NPPM_DMMREGASDCKDLG, 0, &mut data as *mut TbData as isize);
}

pub fn invalidate(hwnd: HWND) {
    // SAFETY: repaint request for our own window.
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

pub fn set_render_timer(hwnd: HWND, ms: u32) {
    // SAFETY: timer on our own window.
    unsafe {
        SetTimer(Some(hwnd), TIMER_RENDER, ms, None);
    }
}

fn paint_background(hwnd: HWND, hdc: HDC) {
    let Some(a) = app::app() else { return };
    let (_, bg, fg, _) = a.theme();
    // SAFETY: GDI calls with a valid DC from BeginPaint or WM_ERASEBKGND.
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(bg));
        FillRect(hdc, &rc, brush);
        let _ = DeleteObject(brush.into());
        if let Some(text) = a.status_text() {
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, windows::Win32::Foundation::COLORREF(fg));
            let mut wtext: Vec<u16> = text.encode_utf16().collect();
            rc.left += 16;
            rc.right -= 16;
            rc.top += 24;
            DrawTextW(hdc, &mut wtext, &mut rc, DT_CENTER | DT_WORDBREAK | DT_NOPREFIX);
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    guarded_or(LRESULT(0), || {
        let a = app::app();
        match (msg, a) {
            (WM_SIZE, Some(a)) => {
                a.on_resize();
                LRESULT(0)
            }
            (WM_ERASEBKGND, _) => {
                paint_background(hwnd, HDC(wparam.0 as _));
                LRESULT(1)
            }
            (WM_PAINT, _) => {
                let mut ps = PAINTSTRUCT::default();
                // SAFETY: paired BeginPaint/EndPaint on our window.
                unsafe {
                    let hdc = BeginPaint(hwnd, &mut ps);
                    paint_background(hwnd, hdc);
                    let _ = EndPaint(hwnd, &ps);
                }
                LRESULT(0)
            }
            (WM_SETFOCUS, Some(a)) => {
                a.focus_preview();
                LRESULT(0)
            }
            (WM_NOTIFY, Some(a)) if lparam.0 != 0 => {
                // SAFETY: WM_NOTIFY carries an NMHDR pointer.
                let code = unsafe { (*(lparam.0 as *const NMHDR)).code };
                match code {
                    npp::DMN_CLOSE => a.after_hidden(true),
                    npp::DMN_SWITCHIN => a.tab_switched(true),
                    npp::DMN_SWITCHOFF => a.tab_switched(false),
                    _ => {}
                }
                LRESULT(0)
            }
            (WM_TIMER, Some(a)) if wparam.0 == TIMER_RENDER => {
                // SAFETY: stop our own one-shot timer.
                unsafe {
                    let _ = KillTimer(Some(hwnd), TIMER_RENDER);
                }
                a.render_now();
                LRESULT(0)
            }
            (WM_APP_RENDERED, Some(a)) => {
                a.on_rendered();
                LRESULT(0)
            }
            (WM_APP_TOGGLE, Some(a)) => {
                a.toggle();
                LRESULT(0)
            }
            (WM_APP_TASK, Some(a)) => {
                a.on_task_done();
                LRESULT(0)
            }
            // SAFETY: default handling for everything else.
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    })
}
