# MahiMarkD++

Fast Markdown preview panel for [Notepad++](https://notepad-plus-plus.org/).

MahiMarkD++ renders the current document in a dockable panel next to the editor and keeps it in sync while you type and scroll. Rendering runs on a background thread, only changed blocks are updated, so large files stay responsive.

## Features

- GitHub Flavored Markdown: tables, task lists, strikethrough, footnotes, alerts (`> [!NOTE]`), YAML front matter, heading anchors.
- Syntax highlighting for code blocks (highlight.js) with a copy button.
- Math with KaTeX: `$inline$`, `$$display$$` and ```` ```math ```` blocks.
- Two-way scroll sync with the editor; click in the preview to move the caret to that line.
- Table of contents sidebar that follows the scroll position.
- Find in preview (Ctrl+F, F3, Shift+F3), zoom (Ctrl+wheel, Ctrl+Plus, Ctrl+Minus, Ctrl+0).
- Local images and relative links; links to other files open in Notepad++, web links open in the browser.
- Raw HTML is allowed but sanitized: scripts, event handlers and unsafe URLs are removed.
- Light and dark themes; the Auto theme follows the colors of the Notepad++ editor theme.
- Export to HTML (self-contained, with highlighting and math), PDF and Word (DOCX).
- Opens automatically for Markdown files (configurable).
- Built for large documents: incremental updates, lazy highlighting and math rendering, off-screen content is not laid out.

## Requirements

- Windows 10 or 11 (x64, x86 or ARM64)
- Notepad++ 8.4.1 or newer (tested with 8.9.8)
- Microsoft Edge WebView2 Runtime (preinstalled on Windows 11 and on up-to-date Windows 10)

## Installation

1. Download `MahiMarkD-PlusPlus-<version>-<arch>.zip` for your Notepad++ build (x64, x86 or arm64) from [Releases](https://github.com/Mahiron-hq/MahiMarkD-PlusPlus/releases).
2. Close Notepad++.
3. Extract the archive into `<Notepad++ folder>\plugins\MahiMarkD++\` so that the file is `plugins\MahiMarkD++\MahiMarkD++.dll`.
4. Start Notepad++.

## Usage

| Action | How |
|---|---|
| Show or hide the preview | Plugins → MahiMarkD++ → Markdown Preview, or Ctrl+Shift+M |
| Export | Toolbar button in the panel, or Plugins → MahiMarkD++ → Export to … |
| Settings | Gear button in the panel, or Plugins → MahiMarkD++ → Settings |
| Back to the editor | Esc in the preview |

The shortcut can be changed in Settings → Shortcut Mapper → Plugin commands.

## Settings

Settings are stored in `MahiMarkD++.json` in the Notepad++ plugin config folder (usually `%APPDATA%\Notepad++\plugins\config`). They can be changed from the settings page in the panel:

- theme (Auto, Light, Dark), font size, text and code fonts, content width;
- automatic opening and the list of Markdown file extensions;
- scroll sync, click-to-source, math, update delay while typing;
- PDF page size (A4 or Letter).

The WebView2 cache is kept in `%LOCALAPPDATA%\MahiMarkD++\WebView2`.

## Building

Requires Rust 1.95+ with the MSVC toolchain and the Visual Studio Build Tools (C++ x64/x86 and ARM64 components for all targets).

```powershell
cargo test
cargo build --release
.\scripts\package.ps1
```

`package.ps1` builds x64, x86 and ARM64, puts the archives into `dist\` and writes `SHA256SUMS.txt`.

## License

Copyright (C) 2026 Lev Burmistrov (Mahiron-hq).

MahiMarkD++ is free software under the [GNU General Public License v3.0](LICENSE). Third-party components and their licenses are listed in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
