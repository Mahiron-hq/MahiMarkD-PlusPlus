//! Small string helpers shared across modules.

/// UTF-16 with a trailing NUL, for Win32 calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Escapes text for HTML element content and attribute values.
pub fn escape_html(out: &mut String, s: &str) {
    let mut last = 0;
    for (i, b) in s.bytes().enumerate() {
        let rep = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            b'\'' => "&#39;",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + 1;
    }
    out.push_str(&s[last..]);
}

pub fn html_escaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    escape_html(&mut out, s);
    out
}

/// COLORREF (0x00BBGGRR) to `#rrggbb`.
pub fn colorref_to_hex(c: u32) -> String {
    let r = c & 0xff;
    let g = (c >> 8) & 0xff;
    let b = (c >> 16) & 0xff;
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub fn colorref_is_dark(c: u32) -> bool {
    let r = (c & 0xff) as f64;
    let g = ((c >> 8) & 0xff) as f64;
    let b = ((c >> 16) & 0xff) as f64;
    0.2126 * r + 0.7152 * g + 0.0722 * b < 128.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_all_special_chars() {
        assert_eq!(html_escaped(r#"<a href="x">&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;");
        assert_eq!(html_escaped("plain текст"), "plain текст");
    }

    #[test]
    fn colorref_conversion() {
        assert_eq!(colorref_to_hex(0x00332211), "#112233");
        assert!(colorref_is_dark(0x002b2b2b));
        assert!(!colorref_is_dark(0x00ffffff));
    }
}
