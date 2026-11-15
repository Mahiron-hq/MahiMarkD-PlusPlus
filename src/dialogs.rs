//! Native save dialog and message boxes.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FileSaveDialog, IFileSaveDialog, IShellItem, SHCreateItemFromParsingName,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MessageBoxW};
use windows::core::{HSTRING, PCWSTR};

/// Shows a save dialog. Returns `None` if the user cancelled.
pub fn save_file(owner: HWND, title: &str, filter_name: &str, ext: &str, dir: Option<&Path>, name: &str) -> Option<PathBuf> {
    // SAFETY: COM calls on the UI thread with valid arguments; all strings outlive the calls.
    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filter_name = HSTRING::from(filter_name);
        let pattern = HSTRING::from(format!("*.{ext}"));
        let filters = [COMDLG_FILTERSPEC { pszName: PCWSTR(filter_name.as_ptr()), pszSpec: PCWSTR(pattern.as_ptr()) }];
        dialog.SetFileTypes(&filters).ok()?;
        dialog.SetDefaultExtension(&HSTRING::from(ext)).ok()?;
        dialog.SetTitle(&HSTRING::from(title)).ok()?;
        dialog.SetFileName(&HSTRING::from(name)).ok()?;
        if let Some(dir) = dir
            && let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None)
        {
            let _ = dialog.SetFolder(&item);
        }
        let opts = dialog.GetOptions().ok()?;
        dialog.SetOptions(opts | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST).ok()?;
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as _));
        path.map(PathBuf::from)
    }
}

pub fn info(owner: HWND, title: &str, text: &str) {
    // SAFETY: plain modal message box.
    unsafe {
        MessageBoxW(Some(owner), &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONINFORMATION);
    }
}

pub fn error(owner: HWND, title: &str, text: &str) {
    // SAFETY: plain modal message box.
    unsafe {
        MessageBoxW(Some(owner), &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONERROR);
    }
}
