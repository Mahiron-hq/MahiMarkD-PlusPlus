//! MahiMarkD++: Markdown preview panel for Notepad++.

pub mod diff;
pub mod export;
pub mod render;
pub mod settings;
pub mod text;
pub mod worker;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod assets;
#[cfg(windows)]
mod dialogs;
#[cfg(windows)]
mod guard;
#[cfg(windows)]
mod npp;
#[cfg(windows)]
mod panel;
#[cfg(windows)]
mod webview;

#[cfg(windows)]
mod exports {
    use std::sync::OnceLock;

    use windows::Win32::Foundation::{HINSTANCE, HMODULE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleHandleExW,
    };
    use windows::core::{BOOL, PCWSTR};

    use crate::app::{self, CMD_ABOUT, CMD_EXPORT_DOCX, CMD_EXPORT_HTML, CMD_EXPORT_PDF, CMD_SETTINGS, CMD_TOGGLE, ExportKind, NAME};
    use crate::guard::guarded;
    use crate::npp::{FuncItem, NppData, ScNotification, ShortcutKey};

    const MENU_LEN: usize = 8;
    const VK_M: u8 = 0x4D;

    struct Menu(*mut FuncItem);
    // SAFETY: the array is leaked and only touched on the Notepad++ UI thread.
    unsafe impl Send for Menu {}
    // SAFETY: see above.
    unsafe impl Sync for Menu {}

    fn default_key() -> ShortcutKey {
        ShortcutKey { ctrl: true, alt: false, shift: true, key: VK_M }
    }

    fn menu() -> *mut FuncItem {
        static MENU: OnceLock<Menu> = OnceLock::new();
        MENU.get_or_init(|| {
            let key = Box::leak(Box::new(default_key())) as *mut ShortcutKey;
            let none = std::ptr::null_mut();
            let mut items: Vec<FuncItem> = (0..MENU_LEN).map(|_| FuncItem::new("", None, none)).collect();
            items[CMD_TOGGLE] = FuncItem::new("Markdown Preview", Some(cmd_toggle), key);
            items[CMD_EXPORT_HTML] = FuncItem::new("Export to HTML...", Some(cmd_export_html), none);
            items[CMD_EXPORT_PDF] = FuncItem::new("Export to PDF...", Some(cmd_export_pdf), none);
            items[CMD_EXPORT_DOCX] = FuncItem::new("Export to Word (DOCX)...", Some(cmd_export_docx), none);
            items[CMD_SETTINGS] = FuncItem::new("Settings...", Some(cmd_settings), none);
            items[CMD_ABOUT] = FuncItem::new("About...", Some(cmd_about), none);
            Menu(Box::leak(items.into_boxed_slice()).as_mut_ptr())
        })
        .0
    }

    fn module() -> HINSTANCE {
        let mut h = HMODULE::default();
        // SAFETY: resolves the module that contains this function.
        unsafe {
            let _ = GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                PCWSTR(module as *const () as *const u16),
                &mut h,
            );
        }
        h.into()
    }

    fn with_app(f: impl FnOnce(&app::App)) {
        guarded(|| {
            if let Some(a) = app::app() {
                f(&a);
            }
        });
    }

    extern "C" fn cmd_toggle() {
        with_app(|a| a.toggle());
    }

    extern "C" fn cmd_export_html() {
        with_app(|a| a.export(ExportKind::Html));
    }

    extern "C" fn cmd_export_pdf() {
        with_app(|a| a.export(ExportKind::Pdf));
    }

    extern "C" fn cmd_export_docx() {
        with_app(|a| a.export(ExportKind::Docx));
    }

    extern "C" fn cmd_settings() {
        with_app(|a| a.open_settings());
    }

    extern "C" fn cmd_about() {
        with_app(|a| a.about());
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn isUnicode() -> BOOL {
        BOOL(1)
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn setInfo(data: NppData) {
        guarded(|| app::install(data, module(), menu(), default_key()));
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn getName() -> *const u16 {
        static NAME_W: OnceLock<Vec<u16>> = OnceLock::new();
        NAME_W.get_or_init(|| crate::text::wide(NAME)).as_ptr()
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn getFuncsArray(count: *mut i32) -> *mut FuncItem {
        if !count.is_null() {
            // SAFETY: Notepad++ passes a valid pointer to receive the item count.
            unsafe { *count = MENU_LEN as i32 };
        }
        menu()
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn beNotified(notification: *mut ScNotification) {
        if notification.is_null() {
            return;
        }
        // SAFETY: Notepad++ passes a valid notification for the duration of the call.
        let n = unsafe { &*notification };
        with_app(|a| a.on_notify(n));
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn messageProc(_msg: u32, _wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
        LRESULT(1)
    }
}
