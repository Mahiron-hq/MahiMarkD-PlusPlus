use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=resources/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is set by cargo");

    let mut parts: Vec<u16> = version.split(['.', '-', '+']).take(3).map(|p| p.parse().unwrap_or(0)).collect();
    parts.resize(3, 0);
    let numeric = format!("{},{},{},0", parts[0], parts[1], parts[2]);
    let icon = manifest_dir.join("resources").join("icon.ico");
    let icon = icon.to_string_lossy().replace('\\', "\\\\");

    let rc = format!(
        r#"#include <winver.h>

1 ICON "{icon}"

VS_VERSION_INFO VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_DLL
FILESUBTYPE VFT2_UNKNOWN
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Lev Burmistrov (Mahiron-hq)"
            VALUE "FileDescription", "MahiMarkD++ - Markdown preview for Notepad++"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "MahiMarkD++"
            VALUE "LegalCopyright", "Copyright (C) 2026 Lev Burmistrov (Mahiron-hq). GPL-3.0-only"
            VALUE "OriginalFilename", "MahiMarkD++.dll"
            VALUE "ProductName", "MahiMarkD++"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );

    let rc_path = out_dir.join("mahimarkd.rc");
    fs::write(&rc_path, rc).expect("write generated resource script");
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional().expect("compile resources");
}
