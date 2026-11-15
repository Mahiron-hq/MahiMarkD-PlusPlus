//! Plugin state and behavior. Everything here runs on the Notepad++ UI thread.
//!
//! Borrows of the `RefCell` fields are kept short: Win32 and Notepad++ calls can
//! re-enter the plugin through notifications.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL};
use windows::core::{HSTRING, w};

use crate::dialogs;
use crate::export;
use crate::npp::{self, FuncItem, NppData, ScNotification, ShortcutKey};
use crate::panel;
use crate::render::urls;
use crate::settings::{self, PageSize, Settings, Theme};
use crate::text::{colorref_is_dark, colorref_to_hex};
use crate::webview::{self, Events, Host};
use crate::worker::{Job, Worker};

pub const NAME: &str = "MahiMarkD++";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const CMD_TOGGLE: usize = 0;
pub const CMD_EXPORT_HTML: usize = 2;
pub const CMD_EXPORT_PDF: usize = 3;
pub const CMD_EXPORT_DOCX: usize = 4;
pub const CMD_SETTINGS: usize = 6;
pub const CMD_ABOUT: usize = 7;

const LIGHT_BG: u32 = 0x00ff_ffff;
const LIGHT_FG: u32 = 0x0028_231f;
const DARK_BG: u32 = 0x001e_1e1e;
const DARK_FG: u32 = 0x00d4_d4d4;
const SYNC_ECHO_WINDOW: Duration = Duration::from_millis(400);

const SHELL_OPEN_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "avif", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods",
    "odp", "rtf", "epub", "zip", "7z", "rar", "mp3", "mp4", "wav", "ogg", "flac", "mkv", "webm", "avi", "mov",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Html,
    Pdf,
    Docx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostState {
    None,
    Creating,
    Ready,
    Failed,
}

#[derive(Debug)]
pub struct App {
    pub npp: NppData,
    pub module: HINSTANCE,
    funcs: Cell<*mut FuncItem>,
    toggle_key: Cell<ShortcutKey>,
    settings: RefCell<Option<Settings>>,
    panel: Cell<Option<HWND>>,
    registered: Cell<bool>,
    visible: Cell<bool>,
    auto_shown: Cell<bool>,
    ready: Cell<bool>,
    suppressed: RefCell<HashSet<isize>>,
    host: RefCell<Option<Host>>,
    host_state: Cell<HostState>,
    host_error: RefCell<Option<String>>,
    page_ready: Cell<bool>,
    need_reset: Cell<bool>,
    shown_doc: Cell<isize>,
    worker: RefCell<Option<Worker>>,
    editor_echo: Cell<Option<(isize, Instant)>>,
    pending: RefCell<Vec<ExportKind>>,
    open_settings: Cell<bool>,
    requests: RefCell<HashMap<u64, PathBuf>>,
    next_id: Cell<u64>,
    tasks: Arc<Mutex<Vec<(bool, String)>>>,
    shutting_down: Cell<bool>,
}

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

pub fn app() -> Option<Rc<App>> {
    APP.with(|a| a.borrow().clone())
}

pub fn install(npp: NppData, module: HINSTANCE, funcs: *mut FuncItem, toggle_key: ShortcutKey) {
    let app = App {
        npp,
        module,
        funcs: Cell::new(funcs),
        toggle_key: Cell::new(toggle_key),
        settings: RefCell::new(None),
        panel: Cell::new(None),
        registered: Cell::new(false),
        visible: Cell::new(false),
        auto_shown: Cell::new(false),
        ready: Cell::new(false),
        suppressed: RefCell::new(HashSet::new()),
        host: RefCell::new(None),
        host_state: Cell::new(HostState::None),
        host_error: RefCell::new(None),
        page_ready: Cell::new(false),
        need_reset: Cell::new(true),
        shown_doc: Cell::new(0),
        worker: RefCell::new(None),
        editor_echo: Cell::new(None),
        pending: RefCell::new(Vec::new()),
        open_settings: Cell::new(false),
        requests: RefCell::new(HashMap::new()),
        next_id: Cell::new(1),
        tasks: Arc::new(Mutex::new(Vec::new())),
        shutting_down: Cell::new(false),
    };
    APP.with(|a| *a.borrow_mut() = Some(Rc::new(app)));
}

#[derive(Clone, Copy, Debug)]
struct WebEvents;

impl Events for WebEvents {
    fn created(&self, host: windows::core::Result<Host>) {
        if let Some(a) = app() {
            a.on_host_created(host);
        }
    }

    fn message(&self, json: String) {
        if let Some(a) = app() {
            a.on_message(&json);
        }
    }

    fn external_link(&self, uri: String) {
        if let Some(a) = app()
            && matches!(urls::scheme(&uri).as_deref(), Some("http" | "https" | "mailto"))
        {
            a.shell_open(&uri);
        }
    }

    fn accelerator(&self, vkey: u32) -> bool {
        app().is_some_and(|a| a.on_accelerator(vkey))
    }

    fn zoom_changed(&self, zoom: f64) {
        if let Some(a) = app() {
            a.on_zoom_changed(zoom);
        }
    }

    fn process_failed(&self, browser_gone: bool) {
        if let Some(a) = app() {
            a.on_process_failed(browser_gone);
        }
    }
}

fn key_down(vk: u16) -> bool {
    // SAFETY: reads keyboard state of the current thread.
    unsafe { GetKeyState(vk as i32) < 0 }
}

impl App {
    fn post_to_panel(&self, msg: u32) {
        if let Some(h) = self.panel.get() {
            // SAFETY: posting to our own window.
            unsafe {
                let _ = PostMessageW(Some(h), msg, WPARAM(0), LPARAM(0));
            }
        }
    }

    fn cmd_id(&self, index: usize) -> i32 {
        let funcs = self.funcs.get();
        if funcs.is_null() {
            return 0;
        }
        // SAFETY: `funcs` points to the static menu array with more than `index` entries.
        unsafe { (*funcs.add(index)).cmd_id }
    }

    pub fn settings(&self) -> Settings {
        if let Some(s) = self.settings.borrow().as_ref() {
            return s.clone();
        }
        let loaded = self.settings_path().map(|p| Settings::load(&p)).unwrap_or_default();
        *self.settings.borrow_mut() = Some(loaded.clone());
        loaded
    }

    fn settings_path(&self) -> Option<PathBuf> {
        self.npp.plugins_config_dir().map(|d| d.join(settings::FILE_NAME))
    }

    fn store_settings(&self, s: Settings) {
        *self.settings.borrow_mut() = Some(s.clone());
        if let Some(path) = self.settings_path()
            && let Err(e) = s.save(&path)
        {
            self.toast(&format!("Could not save settings: {e}"), true);
        }
    }

    fn host(&self) -> Option<Host> {
        self.host.borrow().clone()
    }

    fn current_doc(&self) -> (isize, Option<String>) {
        let id = self.npp.current_buffer_id();
        (id, self.npp.buffer_path(id))
    }

    fn base_dir(path: Option<&str>) -> Option<PathBuf> {
        let p = Path::new(path?);
        if !p.is_absolute() {
            return None;
        }
        p.parent().map(Path::to_path_buf)
    }

    fn update_check(&self) {
        self.npp.set_menu_check(self.cmd_id(CMD_TOGGLE), self.visible.get());
    }

    fn ensure_panel(&self) -> Option<HWND> {
        if let Some(h) = self.panel.get() {
            return Some(h);
        }
        let h = panel::create(self.module, self.npp.npp)?;
        self.panel.set(Some(h));
        let raw = h.0 as isize;
        let worker = Worker::start(Box::new(move || {
            // SAFETY: the window handle stays valid until shutdown, when the worker is stopped first.
            unsafe {
                let _ = PostMessageW(Some(HWND(raw as _)), panel::WM_APP_RENDERED, WPARAM(0), LPARAM(0));
            }
        }));
        *self.worker.borrow_mut() = Some(worker);
        Some(h)
    }

    pub fn show(&self, manual: bool) {
        let Some(h) = self.ensure_panel() else { return };
        if !self.registered.get() {
            panel::register_dock(self, h, CMD_TOGGLE as i32);
            self.registered.set(true);
        }
        npp::send(self.npp.npp, npp::NPPM_DMMSHOW, 0, h.0 as isize);
        self.visible.set(true);
        self.auto_shown.set(!manual);
        if manual {
            let (doc, _) = self.current_doc();
            self.suppressed.borrow_mut().remove(&doc);
        }
        self.update_check();
        self.ensure_host(h);
        if let Some(host) = self.host() {
            host.set_visible(true);
            host.fit(h);
        }
        self.request_render(0);
    }

    pub fn hide(&self, by_user: bool) {
        if let Some(h) = self.panel.get() {
            npp::send(self.npp.npp, npp::NPPM_DMMHIDE, 0, h.0 as isize);
        }
        self.after_hidden(by_user);
    }

    pub fn after_hidden(&self, by_user: bool) {
        self.visible.set(false);
        self.update_check();
        if let Some(host) = self.host() {
            host.set_visible(false);
        }
        if by_user {
            let (doc, _) = self.current_doc();
            self.suppressed.borrow_mut().insert(doc);
        }
    }

    pub fn toggle(&self) {
        if self.visible.get() {
            self.hide(true);
        } else {
            self.show(self.ready.get());
        }
    }

    pub fn tab_switched(&self, active: bool) {
        if let Some(host) = self.host() {
            host.set_visible(active && self.visible.get());
        }
    }

    fn ensure_host(&self, h: HWND) {
        if matches!(self.host_state.get(), HostState::Creating | HostState::Ready) {
            return;
        }
        self.host_state.set(HostState::Creating);
        *self.host_error.borrow_mut() = None;
        // SAFETY: STA init on the UI thread; an already initialized apartment is fine.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let dir = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(NAME).join("WebView2");
        let _ = fs::create_dir_all(&dir);
        let (_, bg, _, _) = self.theme();
        webview::create(h, &dir, bg, self.settings().zoom, WebEvents);
    }

    fn on_host_created(&self, result: windows::core::Result<Host>) {
        match result {
            Ok(host) => {
                if self.shutting_down.get() {
                    host.close();
                    return;
                }
                host.set_visible(self.visible.get());
                *self.host.borrow_mut() = Some(host);
                self.host_state.set(HostState::Ready);
            }
            Err(e) => {
                self.host_state.set(HostState::Failed);
                *self.host_error.borrow_mut() = Some(format!(
                    "{NAME} could not start Microsoft Edge WebView2.\n\n{}\n\nInstall the WebView2 Runtime from https://go.microsoft.com/fwlink/p/?LinkId=2124703 and reopen the panel.",
                    e.message()
                ));
            }
        }
        if let Some(h) = self.panel.get() {
            panel::invalidate(h);
        }
    }

    pub fn status_text(&self) -> Option<String> {
        self.host_error.borrow().clone()
    }

    fn on_process_failed(&self, browser_gone: bool) {
        self.page_ready.set(false);
        self.need_reset.set(true);
        if browser_gone {
            if let Some(host) = self.host.borrow_mut().take() {
                host.close();
            }
            self.host_state.set(HostState::None);
            if self.visible.get()
                && let Some(h) = self.panel.get()
            {
                self.ensure_host(h);
            }
        } else if let Some(host) = self.host() {
            host.reload();
        }
    }

    pub fn on_resize(&self) {
        if let (Some(host), Some(h)) = (self.host(), self.panel.get()) {
            host.fit(h);
        }
    }

    pub fn focus_preview(&self) {
        if let Some(host) = self.host() {
            host.focus();
        }
    }

    /// Returns (dark, background, foreground, colors follow the editor theme).
    pub fn theme(&self) -> (bool, u32, u32, bool) {
        match self.settings().theme {
            Theme::Auto => {
                let (bg, fg) = self.npp.editor_colors();
                (colorref_is_dark(bg), bg, fg, true)
            }
            Theme::Light => (false, LIGHT_BG, LIGHT_FG, false),
            Theme::Dark => (true, DARK_BG, DARK_FG, false),
        }
    }

    fn post(&self, value: &Value) {
        if !self.page_ready.get() {
            return;
        }
        if let Some(host) = self.host() {
            host.post(&value.to_string());
        }
    }

    fn toast(&self, text: &str, error: bool) {
        if self.page_ready.get() && self.visible.get() {
            self.post(&json!({ "t": "toast", "text": text, "error": error }));
        } else if error {
            dialogs::error(self.npp.npp, NAME, text);
        } else {
            dialogs::info(self.npp.npp, NAME, text);
        }
    }

    pub fn send_config(&self) {
        let s = self.settings();
        let (dark, bg, fg, editor) = self.theme();
        if let Some(host) = self.host() {
            host.set_background(bg);
        }
        if let Some(h) = self.panel.get() {
            panel::invalidate(h);
        }
        self.post(&json!({
            "t": "config",
            "name": NAME,
            "version": VERSION,
            "settings": s,
            "theme": { "dark": dark, "bg": colorref_to_hex(bg), "fg": colorref_to_hex(fg), "editor": editor },
        }));
    }

    pub fn request_render(&self, delay_ms: u32) {
        if !self.visible.get() {
            return;
        }
        if let Some(h) = self.panel.get() {
            panel::set_render_timer(h, delay_ms.max(1));
        }
    }

    pub fn render_now(&self) {
        if !self.visible.get() || !self.page_ready.get() {
            return;
        }
        let (doc, path) = self.current_doc();
        let sci = self.npp.current_scintilla();
        let text = npp::scintilla_text(sci);
        let reset = self.need_reset.replace(false) || doc != self.shown_doc.get();
        self.shown_doc.set(doc);
        let job = Job { doc, reset, text, base: Self::base_dir(path.as_deref()), math: self.settings().math };
        if let Some(w) = self.worker.borrow().as_ref() {
            w.submit(job);
        }
    }

    pub fn on_rendered(&self) {
        let outputs = match self.worker.borrow().as_ref() {
            Some(w) => w.take_outputs(),
            None => return,
        };
        if outputs.is_empty() {
            return;
        }
        let mut applied = false;
        for out in outputs {
            if !self.page_ready.get() || out.doc != self.shown_doc.get() {
                self.need_reset.set(true);
                continue;
            }
            if let (Some(msg), Some(host)) = (out.message.as_deref(), self.host()) {
                host.post(msg);
            }
            applied = true;
        }
        if applied {
            self.send_editor_scroll();
            self.run_pending();
        }
    }

    fn run_pending(&self) {
        let (doc, _) = self.current_doc();
        if doc != self.shown_doc.get() {
            return;
        }
        let kinds: Vec<ExportKind> = self.pending.borrow_mut().drain(..).collect();
        for kind in kinds {
            self.export(kind);
        }
        if self.open_settings.replace(false) {
            self.post(&json!({ "t": "openSettings" }));
        }
    }

    fn send_editor_scroll(&self) {
        if !self.settings().scroll_sync {
            return;
        }
        let line = npp::first_visible_line(self.npp.current_scintilla());
        self.post(&json!({ "t": "scroll", "line": line }));
    }

    pub fn on_editor_scroll(&self, sci: HWND) {
        if !self.visible.get() || !self.page_ready.get() || sci != self.npp.current_scintilla() {
            return;
        }
        if let Some((line, at)) = self.editor_echo.get() {
            if at.elapsed() < SYNC_ECHO_WINDOW && npp::send(sci, npp::SCI_GETFIRSTVISIBLELINE, 0, 0) == line {
                return;
            }
            self.editor_echo.set(None);
        }
        self.send_editor_scroll();
    }

    pub fn on_notify(&self, n: &ScNotification) {
        let from = HWND(n.hdr.hwnd_from);
        if from == self.npp.sci_main || from == self.npp.sci_second {
            match n.hdr.code {
                npp::SCN_MODIFIED if n.modification_type & (npp::SC_MOD_INSERTTEXT | npp::SC_MOD_DELETETEXT) != 0 => {
                    self.request_render(self.settings().render_delay_ms);
                }
                npp::SCN_UPDATEUI if n.updated & npp::SC_UPDATE_V_SCROLL != 0 => self.on_editor_scroll(from),
                _ => {}
            }
            return;
        }
        let buffer = n.hdr.id_from as isize;
        match n.hdr.code {
            npp::NPPN_READY => {
                self.ready.set(true);
                let (doc, _) = self.current_doc();
                self.on_buffer_activated(doc);
            }
            npp::NPPN_BUFFERACTIVATED => {
                if self.ready.get() {
                    self.on_buffer_activated(buffer);
                }
            }
            npp::NPPN_FILESAVED | npp::NPPN_FILERENAMED => {
                if self.ready.get() && buffer == self.npp.current_buffer_id() {
                    self.need_reset.set(true);
                    self.on_buffer_activated(buffer);
                }
            }
            npp::NPPN_FILECLOSED => {
                self.suppressed.borrow_mut().remove(&buffer);
            }
            npp::NPPN_DARKMODECHANGED | npp::NPPN_WORDSTYLESUPDATED => self.send_config(),
            npp::NPPN_SHORTCUTREMAPPED => {
                if n.hdr.id_from as i32 == self.cmd_id(CMD_TOGGLE) && !n.hdr.hwnd_from.is_null() {
                    // SAFETY: Notepad++ passes a valid ShortcutKey pointer with this notification.
                    let key = unsafe { *(n.hdr.hwnd_from as *const ShortcutKey) };
                    self.toggle_key.set(key);
                }
            }
            npp::NPPN_SHUTDOWN => self.shutdown(),
            _ => {}
        }
    }

    fn on_buffer_activated(&self, buffer: isize) {
        let s = self.settings();
        if s.auto_open {
            let is_md = self.npp.buffer_path(buffer).is_some_and(|p| s.is_markdown_path(&p));
            let suppressed = self.suppressed.borrow().contains(&buffer);
            if is_md && !self.visible.get() && !suppressed {
                self.show(false);
                return;
            }
            if !is_md && self.visible.get() && self.auto_shown.get() {
                self.hide(false);
                return;
            }
        }
        self.request_render(0);
    }

    fn shutdown(&self) {
        self.shutting_down.set(true);
        self.page_ready.set(false);
        if let Some(mut w) = self.worker.borrow_mut().take() {
            w.stop();
        }
        if let Some(host) = self.host.borrow_mut().take() {
            host.close();
        }
    }

    fn on_accelerator(&self, vkey: u32) -> bool {
        let key = self.toggle_key.get();
        if key.key == 0 || vkey != key.key as u32 {
            return false;
        }
        if key.ctrl != key_down(VK_CONTROL.0) || key.shift != key_down(VK_SHIFT.0) || key.alt != key_down(VK_MENU.0) {
            return false;
        }
        self.post_to_panel(panel::WM_APP_TOGGLE);
        true
    }

    fn on_zoom_changed(&self, zoom: f64) {
        let mut s = self.settings();
        if (s.zoom - zoom).abs() > 1e-6 {
            s.zoom = zoom;
            self.store_settings(s.normalized());
        }
    }

    fn on_message(&self, raw: &str) {
        let Ok(v) = serde_json::from_str::<Value>(raw) else { return };
        let num = |k: &str| v.get(k).and_then(Value::as_f64);
        match v.get("t").and_then(Value::as_str).unwrap_or("") {
            "ready" => {
                self.page_ready.set(true);
                self.need_reset.set(true);
                self.send_config();
                self.render_now();
            }
            "scroll" => {
                if let Some(line) = num("line")
                    && self.settings().scroll_sync
                    && line.is_finite()
                    && line >= 0.0
                {
                    let sci = self.npp.current_scintilla();
                    let visible = npp::set_first_visible_line(sci, line);
                    self.editor_echo.set(Some((visible, Instant::now())));
                }
            }
            "goto" => {
                if let Some(line) = num("line")
                    && self.settings().click_to_source
                    && line.is_finite()
                    && line >= 0.0
                {
                    npp::goto_line(self.npp.current_scintilla(), line as usize);
                }
            }
            "focusEditor" => {
                npp::send(self.npp.current_scintilla(), npp::SCI_GRABFOCUS, 0, 0);
            }
            "link" => {
                if let Some(href) = v.get("href").and_then(Value::as_str) {
                    self.open_link(href);
                }
            }
            "settings" => {
                if let Some(value) = v.get("value")
                    && let Ok(s) = serde_json::from_value::<Settings>(value.clone())
                {
                    self.apply_settings(s.normalized());
                }
            }
            "toc" => {
                let mut s = self.settings();
                s.show_toc = v.get("show").and_then(Value::as_bool).unwrap_or(false);
                self.store_settings(s);
            }
            "zoom" => {
                let z = self.settings().zoom;
                let next = match v.get("op").and_then(Value::as_str) {
                    Some("in") => z * 1.1,
                    Some("out") => z / 1.1,
                    _ => 1.0,
                };
                if let Some(host) = self.host() {
                    host.set_zoom(next.clamp(0.25, 5.0));
                }
            }
            "export" => {
                let kind = match v.get("kind").and_then(Value::as_str) {
                    Some("html") => ExportKind::Html,
                    Some("pdf") => ExportKind::Pdf,
                    Some("docx") => ExportKind::Docx,
                    _ => return,
                };
                self.export(kind);
            }
            "exportHtml" => {
                let id = v.get("id").and_then(Value::as_u64).unwrap_or(0);
                let html = v.get("html").and_then(Value::as_str).unwrap_or("");
                if let Some(path) = self.requests.borrow_mut().remove(&id) {
                    match fs::write(&path, html) {
                        Ok(()) => self.toast(&format!("Exported to {}", path.display()), false),
                        Err(e) => self.toast(&format!("Export failed: {e}"), true),
                    }
                }
            }
            "printReady" => {
                let id = v.get("id").and_then(Value::as_u64).unwrap_or(0);
                if let Some(path) = self.requests.borrow_mut().remove(&id) {
                    self.print_pdf(path);
                }
            }
            _ => {}
        }
    }

    fn apply_settings(&self, s: Settings) {
        let old = self.settings();
        self.store_settings(s.clone());
        if old.math != s.math {
            self.need_reset.set(true);
            self.request_render(0);
        }
        if old.zoom != s.zoom
            && let Some(host) = self.host()
        {
            host.set_zoom(s.zoom);
        }
        self.send_config();
    }

    fn print_pdf(&self, path: PathBuf) {
        let Some(host) = self.host() else { return };
        let letter = self.settings().pdf_page_size == PageSize::Letter;
        let shown = path.display().to_string();
        let result = host.print_pdf(&path, letter, move |ok| {
            if let Some(a) = app() {
                a.post(&json!({ "t": "printDone" }));
                if ok {
                    a.toast(&format!("Exported to {shown}"), false);
                } else {
                    a.toast("PDF export failed.", true);
                }
            }
        });
        if let Err(e) = result {
            self.post(&json!({ "t": "printDone" }));
            self.toast(&format!("PDF export failed: {}", e.message()), true);
        }
    }

    pub fn export(&self, kind: ExportKind) {
        let (doc, path) = self.current_doc();
        let base = Self::base_dir(path.as_deref());
        let stem = path
            .as_deref()
            .map(Path::new)
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document".to_string());

        if kind != ExportKind::Docx && (!self.visible.get() || !self.page_ready.get() || doc != self.shown_doc.get()) {
            self.pending.borrow_mut().push(kind);
            if self.visible.get() {
                self.request_render(0);
            } else {
                self.show(true);
            }
            return;
        }

        let (title, filter, ext) = match kind {
            ExportKind::Html => ("Export to HTML", "HTML document", "html"),
            ExportKind::Pdf => ("Export to PDF", "PDF document", "pdf"),
            ExportKind::Docx => ("Export to Word", "Word document", "docx"),
        };
        let Some(target) = dialogs::save_file(self.npp.npp, title, filter, ext, base.as_deref(), &format!("{stem}.{ext}")) else {
            return;
        };

        match kind {
            ExportKind::Docx => self.export_docx(target, base, stem),
            ExportKind::Html | ExportKind::Pdf => {
                let id = self.next_id.get();
                self.next_id.set(id + 1);
                let same_dir = target.parent().map(Path::to_path_buf) == base;
                self.requests.borrow_mut().insert(id, target);
                if kind == ExportKind::Html {
                    self.post(&json!({ "t": "exportHtml", "id": id, "sameDir": same_dir, "title": stem }));
                } else {
                    self.post(&json!({ "t": "preparePrint", "id": id }));
                }
            }
        }
    }

    fn export_docx(&self, target: PathBuf, base: Option<PathBuf>, title: String) {
        let text = npp::scintilla_text(self.npp.current_scintilla());
        let math = self.settings().math;
        let tasks = Arc::clone(&self.tasks);
        let panel = self.panel.get().or_else(|| self.ensure_panel()).map(|h| h.0 as isize);
        let spawned = thread::Builder::new().name("mahimarkd-docx".into()).spawn(move || {
            let result = export::docx::write_file(&text, base.as_deref(), math, &title, &target);
            let entry = match result {
                Ok(()) => (true, format!("Exported to {}", target.display())),
                Err(e) => (false, format!("Word export failed: {e}")),
            };
            tasks.lock().unwrap_or_else(PoisonError::into_inner).push(entry);
            if let Some(raw) = panel {
                // SAFETY: posting to our own window; a destroyed window just drops the message.
                unsafe {
                    let _ = PostMessageW(Some(HWND(raw as _)), panel::WM_APP_TASK, WPARAM(0), LPARAM(0));
                }
            }
        });
        if spawned.is_err() {
            self.toast("Word export failed: could not start a worker thread.", true);
        }
    }

    pub fn on_task_done(&self) {
        let done: Vec<(bool, String)> = self.tasks.lock().unwrap_or_else(PoisonError::into_inner).drain(..).collect();
        for (ok, text) in done {
            self.toast(&text, !ok);
        }
    }

    pub fn open_settings(&self) {
        if self.visible.get() && self.page_ready.get() {
            self.post(&json!({ "t": "openSettings" }));
        } else {
            self.open_settings.set(true);
            self.show(true);
        }
    }

    pub fn about(&self) {
        let text = format!(
            "{NAME} {VERSION}\nMarkdown preview for Notepad++\n\nCopyright (C) 2026 Lev Burmistrov (Mahiron-hq)\nLicense: GNU GPL v3.0\nhttps://github.com/Mahiron-hq/MahiMarkD-PlusPlus\n\nThird-party: pulldown-cmark, ammonia, highlight.js, KaTeX, webview2-com, windows-rs."
        );
        dialogs::info(self.npp.npp, &format!("About {NAME}"), &text);
    }

    fn shell_open(&self, target: &str) {
        // SAFETY: ShellExecute with a validated target string.
        unsafe {
            ShellExecuteW(Some(self.npp.npp), w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL);
        }
    }

    fn open_link(&self, href: &str) {
        let href = href.trim();
        if href.is_empty() || href.starts_with('#') {
            return;
        }
        if let Some(path) = urls::path_from_file_url(href) {
            self.open_local(path);
            return;
        }
        match urls::scheme(href).as_deref() {
            Some("http" | "https" | "mailto" | "ftp" | "ftps" | "tel") => self.shell_open(href),
            Some("file") | None => {
                let (_, path) = self.current_doc();
                let base = Self::base_dir(path.as_deref());
                match urls::resolve_local(href, base.as_deref()) {
                    Some(p) => self.open_local(p),
                    None => self.toast("Save the document first to open relative links.", true),
                }
            }
            Some(_) => {}
        }
    }

    fn open_local(&self, path: PathBuf) {
        if path.is_dir() {
            self.shell_open(&path.to_string_lossy());
            return;
        }
        if !path.is_file() {
            self.toast(&format!("File not found: {}", path.display()), true);
            return;
        }
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if SHELL_OPEN_EXT.contains(&ext.as_str()) {
            self.shell_open(&path.to_string_lossy());
        } else {
            self.npp.open_file(&path.to_string_lossy());
        }
    }
}
