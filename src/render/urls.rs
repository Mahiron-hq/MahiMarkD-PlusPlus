//! Link and image URL handling for the preview page.

use std::path::{Component, Path, PathBuf};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

pub const ORIGIN: &str = "https://mahimarkd.localhost/";
pub const FILE_PREFIX: &str = "https://mahimarkd.localhost/file/";

const PATH_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'/').remove(b'-').remove(b'_').remove(b'.').remove(b'~').remove(b':');

/// Lowercase URL scheme, if the string has one. Single letters are drive letters, not schemes.
pub fn scheme(url: &str) -> Option<String> {
    let colon = url.find(':')?;
    let s = &url[..colon];
    if s.len() < 2 || !s.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    Some(s.to_ascii_lowercase())
}

/// Href for a Markdown link, or `None` when the scheme is not safe to keep.
pub fn safe_href(dest: &str, is_email: bool) -> Option<String> {
    let dest = dest.trim();
    if is_email && !dest.to_ascii_lowercase().starts_with("mailto:") {
        return Some(format!("mailto:{dest}"));
    }
    match scheme(dest).as_deref() {
        None => Some(dest.to_string()),
        Some("http" | "https" | "mailto" | "ftp" | "ftps" | "file" | "tel") => Some(dest.to_string()),
        Some(_) => None,
    }
}

fn split_suffix(dest: &str) -> &str {
    let end = dest.find(['#', '?']).unwrap_or(dest.len());
    &dest[..end]
}

/// Resolves a local link or image reference to an absolute path.
pub fn resolve_local(dest: &str, base: Option<&Path>) -> Option<PathBuf> {
    let dest = dest.trim();
    if dest.is_empty() || dest.starts_with('#') {
        return None;
    }
    let raw = split_suffix(dest);
    if raw.is_empty() {
        return None;
    }
    if let Some(rest) = strip_prefix_ci(raw, "file:") {
        return file_url_to_path(rest);
    }
    if scheme(raw).is_some() {
        return None;
    }
    let decoded = percent_decode_str(raw).decode_utf8_lossy().into_owned();
    let p = decoded.replace('/', "\\");
    let path = Path::new(&p);
    let joined = if is_absolute_windows(&p) {
        path.to_path_buf()
    } else if p.starts_with('\\') {
        let root: PathBuf = base?.components().take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir)).collect();
        root.join(p.trim_start_matches('\\'))
    } else {
        base?.join(path)
    };
    Some(normalize(&joined))
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) { Some(&s[prefix.len()..]) } else { None }
}

fn file_url_to_path(rest: &str) -> Option<PathBuf> {
    let decoded = percent_decode_str(rest).decode_utf8_lossy().into_owned();
    let trimmed = decoded.trim_start_matches('/');
    let slashes = decoded.len() - trimmed.len();
    let p = trimmed.replace('/', "\\");
    if is_absolute_windows(&p) {
        Some(normalize(Path::new(&p)))
    } else if slashes == 2 && !p.is_empty() {
        Some(PathBuf::from(format!("\\\\{p}")))
    } else {
        None
    }
}

fn is_absolute_windows(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || p.starts_with("\\\\")
}

/// Removes `.` and `..` components without touching the file system.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(out.components().next_back(), Some(Component::Prefix(_) | Component::RootDir) | None) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Preview URL that serves a local file through the resource handler.
pub fn file_url(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    format!("{FILE_PREFIX}{}", utf8_percent_encode(&s, PATH_SET))
}

/// Inverse of [`file_url`].
pub fn path_from_file_url(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix(FILE_PREFIX)?;
    let rest = split_suffix(rest);
    let decoded = percent_decode_str(rest).decode_utf8_lossy().into_owned();
    let p = match decoded.strip_prefix("//") {
        Some(unc) => format!("\\\\{unc}"),
        None => decoded,
    };
    Some(PathBuf::from(p.replace('/', "\\")))
}

/// Source URL for an image. Remote and data URLs are kept, local paths go through the file handler.
pub fn image_src(dest: &str, base: Option<&Path>) -> Option<String> {
    let dest = dest.trim();
    match scheme(dest).as_deref() {
        Some("http" | "https") => Some(dest.to_string()),
        Some("data") => dest.get(..11).filter(|p| p.eq_ignore_ascii_case("data:image/")).map(|_| dest.to_string()),
        Some("file") | None => resolve_local(dest, base).map(|p| file_url(&p)),
        Some(_) => None,
    }
}

pub fn image_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes() {
        assert_eq!(scheme("https://x"), Some("https".into()));
        assert_eq!(scheme("C:\\x"), None);
        assert_eq!(scheme("docs/a.md"), None);
        assert_eq!(scheme("JavaScript:alert(1)"), Some("javascript".into()));
    }

    #[test]
    fn blocks_script_links() {
        assert_eq!(safe_href("javascript:alert(1)", false), None);
        assert_eq!(safe_href("vbscript:x", false), None);
        assert_eq!(safe_href("a.md#x", false).as_deref(), Some("a.md#x"));
        assert_eq!(safe_href("me@example.com", true).as_deref(), Some("mailto:me@example.com"));
    }

    #[test]
    fn resolves_relative_and_absolute() {
        let base = Path::new("D:\\docs\\notes");
        assert_eq!(resolve_local("img/a%20b.png", Some(base)), Some(PathBuf::from("D:\\docs\\notes\\img\\a b.png")));
        assert_eq!(resolve_local("../x.md#top", Some(base)), Some(PathBuf::from("D:\\docs\\x.md")));
        assert_eq!(resolve_local("/root.png", Some(base)), Some(PathBuf::from("D:\\root.png")));
        assert_eq!(resolve_local("C:/a/b.png", None), Some(PathBuf::from("C:\\a\\b.png")));
        assert_eq!(resolve_local("file:///C:/a/b%20c.png", None), Some(PathBuf::from("C:\\a\\b c.png")));
        assert_eq!(resolve_local("file://server/share/x.png", None), Some(PathBuf::from("\\\\server\\share\\x.png")));
        assert_eq!(resolve_local("rel.png", None), None);
        assert_eq!(resolve_local("#anchor", Some(base)), None);
    }

    #[test]
    fn file_url_round_trip() {
        for p in ["D:\\Мои документы\\a b#c.png", "\\\\server\\share\\x.png", "C:\\100%\\y.svg"] {
            let url = file_url(Path::new(p));
            assert!(url.starts_with(FILE_PREFIX));
            assert!(!url[FILE_PREFIX.len()..].contains(['#', ' ', '?'].as_slice()));
            assert_eq!(path_from_file_url(&url), Some(PathBuf::from(p)));
        }
    }

    #[test]
    fn image_sources() {
        let base = Path::new("C:\\d");
        assert_eq!(image_src("https://e.com/a.png", Some(base)).as_deref(), Some("https://e.com/a.png"));
        assert_eq!(image_src("data:image/png;base64,AAA", Some(base)).as_deref(), Some("data:image/png;base64,AAA"));
        assert_eq!(image_src("data:text/html,x", Some(base)), None);
        assert_eq!(image_src("javascript:x", Some(base)), None);
        assert_eq!(image_src("a.png", Some(base)).as_deref(), Some("https://mahimarkd.localhost/file/C:/d/a.png"));
    }
}
