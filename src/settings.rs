//! Plugin settings stored as JSON in the Notepad++ plugins config folder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CURRENT_VERSION: u32 = 1;
pub const FILE_NAME: &str = "MahiMarkD++.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PageSize {
    #[default]
    A4,
    Letter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub version: u32,
    pub theme: Theme,
    pub auto_open: bool,
    pub extensions: String,
    pub scroll_sync: bool,
    pub click_to_source: bool,
    pub show_toc: bool,
    pub font_size: u32,
    pub font_family: String,
    pub code_font_family: String,
    pub content_width: u32,
    pub zoom: f64,
    pub render_delay_ms: u32,
    pub math: bool,
    pub pdf_page_size: PageSize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            theme: Theme::Auto,
            auto_open: true,
            extensions: "md, markdown, mdown, mkd, mkdn, mdwn, mdtxt, mdtext, rmd".to_string(),
            scroll_sync: true,
            click_to_source: true,
            show_toc: false,
            font_size: 15,
            font_family: String::new(),
            code_font_family: String::new(),
            content_width: 900,
            zoom: 1.0,
            render_delay_ms: 150,
            math: true,
            pdf_page_size: PageSize::A4,
        }
    }
}

impl Settings {
    /// Clamps values to sane ranges and fills the version.
    pub fn normalized(mut self) -> Self {
        self.version = CURRENT_VERSION;
        self.font_size = self.font_size.clamp(8, 40);
        self.content_width = if self.content_width == 0 { 0 } else { self.content_width.clamp(400, 4000) };
        self.zoom = if self.zoom.is_finite() { self.zoom.clamp(0.25, 5.0) } else { 1.0 };
        self.render_delay_ms = self.render_delay_ms.min(2000);
        self.font_family = clean_font(&self.font_family);
        self.code_font_family = clean_font(&self.code_font_family);
        self.extensions = self.extension_list().join(", ");
        self
    }

    pub fn extension_list(&self) -> Vec<String> {
        let mut list: Vec<String> = self
            .extensions
            .split([',', ';', ' '])
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .collect();
        list.dedup();
        list
    }

    pub fn is_markdown_path(&self, path: &str) -> bool {
        let Some(ext) = Path::new(path).extension() else { return false };
        let ext = ext.to_string_lossy().to_ascii_lowercase();
        self.extension_list().contains(&ext)
    }

    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => Self::from_json(&text),
            Err(_) => Self::default(),
        }
    }

    pub fn from_json(text: &str) -> Self {
        serde_json::from_str::<Settings>(text).unwrap_or_default().normalized()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp: PathBuf = path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, path)
    }
}

fn clean_font(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, ';' | '{' | '}' | '<' | '>' | '\\' | '\n' | '\r')).take(200).collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_and_missing_fields() {
        let s = Settings::from_json(r#"{"theme":"dark","futureField":42}"#);
        assert_eq!(s.theme, Theme::Dark);
        assert!(s.auto_open);
        assert_eq!(s.version, CURRENT_VERSION);
    }

    #[test]
    fn broken_json_gives_defaults() {
        assert_eq!(Settings::from_json("{not json"), Settings::default().normalized());
    }

    #[test]
    fn clamps_values() {
        let s = Settings::from_json(r#"{"fontSize":2,"zoom":99,"renderDelayMs":99999,"fontFamily":"Arial; } body{x"}"#);
        assert_eq!(s.font_size, 8);
        assert_eq!(s.zoom, 5.0);
        assert_eq!(s.render_delay_ms, 2000);
        assert_eq!(s.font_family, "Arial  bodyx");
    }

    #[test]
    fn extensions() {
        let s = Settings { extensions: ".MD; markdown,, bad/ext txt".into(), ..Settings::default() }.normalized();
        assert_eq!(s.extensions, "md, markdown, txt");
        assert!(s.is_markdown_path("C:\\a\\README.Md"));
        assert!(!s.is_markdown_path("C:\\a\\x.rs"));
        assert!(!s.is_markdown_path("new 1"));
    }

    #[test]
    fn round_trip_file() {
        let dir = std::env::temp_dir().join(format!("mmd-test-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        let s = Settings { theme: Theme::Light, zoom: 1.25, ..Settings::default() };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        let _ = fs::remove_dir_all(dir);
    }
}
