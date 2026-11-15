//! Notepad++ and Scintilla interface definitions and thin message helpers.

use std::ffi::c_void;
use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Globalization::{CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar};
use windows::Win32::UI::WindowsAndMessaging::{HICON, SendMessageW, WM_USER};

pub const NPPMSG: u32 = WM_USER + 1000;
pub const NPPM_GETCURRENTSCINTILLA: u32 = NPPMSG + 4;
pub const NPPM_DMMSHOW: u32 = NPPMSG + 30;
pub const NPPM_DMMHIDE: u32 = NPPMSG + 31;
pub const NPPM_DMMREGASDCKDLG: u32 = NPPMSG + 33;
pub const NPPM_SETMENUITEMCHECK: u32 = NPPMSG + 40;
pub const NPPM_GETPLUGINSCONFIGDIR: u32 = NPPMSG + 46;
pub const NPPM_GETFULLPATHFROMBUFFERID: u32 = NPPMSG + 58;
pub const NPPM_GETCURRENTBUFFERID: u32 = NPPMSG + 60;
pub const NPPM_DOOPEN: u32 = NPPMSG + 77;
pub const NPPM_GETEDITORDEFAULTFOREGROUNDCOLOR: u32 = NPPMSG + 90;
pub const NPPM_GETEDITORDEFAULTBACKGROUNDCOLOR: u32 = NPPMSG + 91;

pub const NPPN_FIRST: u32 = 1000;
pub const NPPN_READY: u32 = NPPN_FIRST + 1;
pub const NPPN_FILECLOSED: u32 = NPPN_FIRST + 5;
pub const NPPN_FILESAVED: u32 = NPPN_FIRST + 8;
pub const NPPN_SHUTDOWN: u32 = NPPN_FIRST + 9;
pub const NPPN_BUFFERACTIVATED: u32 = NPPN_FIRST + 10;
pub const NPPN_WORDSTYLESUPDATED: u32 = NPPN_FIRST + 12;
pub const NPPN_SHORTCUTREMAPPED: u32 = NPPN_FIRST + 13;
pub const NPPN_FILERENAMED: u32 = NPPN_FIRST + 23;
pub const NPPN_DARKMODECHANGED: u32 = NPPN_FIRST + 27;

pub const DMN_FIRST: u32 = 1050;
pub const DMN_CLOSE: u32 = DMN_FIRST + 1;
pub const DMN_SWITCHIN: u32 = DMN_FIRST + 4;
pub const DMN_SWITCHOFF: u32 = DMN_FIRST + 5;

pub const DWS_ICONTAB: u32 = 0x0000_0001;
pub const DWS_USEOWNDARKMODE: u32 = 0x0000_0008;
pub const DWS_DF_CONT_RIGHT: u32 = 1 << 28;

pub const SCI_GETLENGTH: u32 = 2006;
pub const SCI_GOTOPOS: u32 = 2025;
pub const SCI_GETCODEPAGE: u32 = 2137;
pub const SCI_GETFIRSTVISIBLELINE: u32 = 2152;
pub const SCI_GETLINECOUNT: u32 = 2154;
pub const SCI_POSITIONFROMLINE: u32 = 2167;
pub const SCI_VISIBLEFROMDOCLINE: u32 = 2220;
pub const SCI_DOCLINEFROMVISIBLE: u32 = 2221;
pub const SCI_WRAPCOUNT: u32 = 2235;
pub const SCI_GRABFOCUS: u32 = 2400;
pub const SCI_GETCHARACTERPOINTER: u32 = 2520;
pub const SCI_SETFIRSTVISIBLELINE: u32 = 2613;

pub const SCN_UPDATEUI: u32 = 2007;
pub const SCN_MODIFIED: u32 = 2008;
pub const SC_MOD_INSERTTEXT: i32 = 0x1;
pub const SC_MOD_DELETETEXT: i32 = 0x2;
pub const SC_UPDATE_V_SCROLL: i32 = 0x4;
pub const SC_CP_UTF8: isize = 65001;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NppData {
    pub npp: HWND,
    pub sci_main: HWND,
    pub sci_second: HWND,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShortcutKey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: u8,
}

pub const MENU_ITEM_SIZE: usize = 64;

#[repr(C)]
#[derive(Debug)]
pub struct FuncItem {
    pub name: [u16; MENU_ITEM_SIZE],
    pub func: Option<extern "C" fn()>,
    pub cmd_id: i32,
    pub init_checked: bool,
    pub shortcut: *mut ShortcutKey,
}

impl FuncItem {
    pub fn new(name: &str, func: Option<extern "C" fn()>, shortcut: *mut ShortcutKey) -> Self {
        let mut buf = [0u16; MENU_ITEM_SIZE];
        for (dst, src) in buf.iter_mut().zip(name.encode_utf16().take(MENU_ITEM_SIZE - 1)) {
            *dst = src;
        }
        Self { name: buf, func, cmd_id: 0, init_checked: false, shortcut }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct NotifyHeader {
    pub hwnd_from: *mut c_void,
    pub id_from: usize,
    pub code: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct ScNotification {
    pub hdr: NotifyHeader,
    pub position: isize,
    pub ch: i32,
    pub modifiers: i32,
    pub modification_type: i32,
    pub text: *const u8,
    pub length: isize,
    pub lines_added: isize,
    pub message: i32,
    pub wparam: usize,
    pub lparam: isize,
    pub line: isize,
    pub fold_level_now: i32,
    pub fold_level_prev: i32,
    pub margin: i32,
    pub list_type: i32,
    pub x: i32,
    pub y: i32,
    pub token: i32,
    pub annotation_lines_added: isize,
    pub updated: i32,
    pub list_completion_method: i32,
    pub character_source: i32,
}

#[repr(C)]
#[derive(Debug)]
pub struct TbData {
    pub client: HWND,
    pub name: *const u16,
    pub dlg_id: i32,
    pub mask: u32,
    pub icon_tab: HICON,
    pub add_info: *const u16,
    pub rc_float: RECT,
    pub prev_cont: i32,
    pub module_name: *const u16,
}

pub fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: plain message send; pointer arguments are validated by callers.
    let LRESULT(r) = unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))) };
    r
}

impl NppData {
    pub fn current_scintilla(&self) -> HWND {
        let mut which: i32 = 0;
        send(self.npp, NPPM_GETCURRENTSCINTILLA, 0, &mut which as *mut i32 as isize);
        if which == 1 { self.sci_second } else { self.sci_main }
    }

    pub fn current_buffer_id(&self) -> isize {
        send(self.npp, NPPM_GETCURRENTBUFFERID, 0, 0)
    }

    pub fn buffer_path(&self, buffer_id: isize) -> Option<String> {
        let len = send(self.npp, NPPM_GETFULLPATHFROMBUFFERID, buffer_id as usize, 0);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let got = send(self.npp, NPPM_GETFULLPATHFROMBUFFERID, buffer_id as usize, buf.as_mut_ptr() as isize);
        if got <= 0 {
            return None;
        }
        buf.truncate(got as usize);
        Some(String::from_utf16_lossy(&buf))
    }

    pub fn plugins_config_dir(&self) -> Option<PathBuf> {
        let len = send(self.npp, NPPM_GETPLUGINSCONFIGDIR, 0, 0);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        if send(self.npp, NPPM_GETPLUGINSCONFIGDIR, buf.len(), buf.as_mut_ptr() as isize) == 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..end])))
    }

    pub fn open_file(&self, path: &str) -> bool {
        let wide = crate::text::wide(path);
        send(self.npp, NPPM_DOOPEN, 0, wide.as_ptr() as isize) != 0
    }

    pub fn editor_colors(&self) -> (u32, u32) {
        let bg = send(self.npp, NPPM_GETEDITORDEFAULTBACKGROUNDCOLOR, 0, 0) as u32;
        let fg = send(self.npp, NPPM_GETEDITORDEFAULTFOREGROUNDCOLOR, 0, 0) as u32;
        (bg, fg)
    }

    pub fn set_menu_check(&self, cmd_id: i32, checked: bool) {
        send(self.npp, NPPM_SETMENUITEMCHECK, cmd_id as usize, checked as isize);
    }
}

/// Reads the whole document of a Scintilla view as UTF-8.
pub fn scintilla_text(sci: HWND) -> String {
    let len = send(sci, SCI_GETLENGTH, 0, 0);
    if len <= 0 {
        return String::new();
    }
    let ptr = send(sci, SCI_GETCHARACTERPOINTER, 0, 0) as *const u8;
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: Scintilla guarantees `len` contiguous bytes at the character pointer until the next edit.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    if send(sci, SCI_GETCODEPAGE, 0, 0) == SC_CP_UTF8 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    ansi_to_string(bytes)
}

fn ansi_to_string(bytes: &[u8]) -> String {
    if bytes.is_ascii() {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    // SAFETY: sizes come from the input slice; output buffer is allocated to the reported size.
    unsafe {
        let n = MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None);
        if n <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide = vec![0u16; n as usize];
        let n = MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, Some(&mut wide));
        wide.truncate(n.max(0) as usize);
        String::from_utf16_lossy(&wide)
    }
}

/// First visible document line of a view, with the wrapped sub-line as a fraction.
pub fn first_visible_line(sci: HWND) -> f64 {
    let visible = send(sci, SCI_GETFIRSTVISIBLELINE, 0, 0);
    let doc_line = send(sci, SCI_DOCLINEFROMVISIBLE, visible as usize, 0);
    let first_sub = send(sci, SCI_VISIBLEFROMDOCLINE, doc_line as usize, 0);
    let wraps = send(sci, SCI_WRAPCOUNT, doc_line as usize, 0).max(1);
    doc_line as f64 + ((visible - first_sub).max(0) as f64 / wraps as f64)
}

/// Scrolls a view so that the given fractional document line is at the top. Returns the visible line set.
pub fn set_first_visible_line(sci: HWND, line: f64) -> isize {
    let count = send(sci, SCI_GETLINECOUNT, 0, 0).max(1);
    let doc_line = (line.floor() as isize).clamp(0, count - 1);
    let frac = (line - doc_line as f64).clamp(0.0, 1.0);
    let first_sub = send(sci, SCI_VISIBLEFROMDOCLINE, doc_line as usize, 0);
    let wraps = send(sci, SCI_WRAPCOUNT, doc_line as usize, 0).max(1);
    let visible = first_sub + ((frac * wraps as f64).floor() as isize).min(wraps - 1);
    send(sci, SCI_SETFIRSTVISIBLELINE, visible as usize, 0);
    visible
}

pub fn goto_line(sci: HWND, line: usize) {
    let count = send(sci, SCI_GETLINECOUNT, 0, 0).max(1) as usize;
    let pos = send(sci, SCI_POSITIONFROMLINE, line.min(count - 1), 0);
    if pos >= 0 {
        send(sci, SCI_GOTOPOS, pos as usize, 0);
    }
}
