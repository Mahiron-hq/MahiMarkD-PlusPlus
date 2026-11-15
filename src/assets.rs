//! Web assets embedded into the DLL.

macro_rules! asset {
    ($path:literal, $mime:literal) => {
        ($path, $mime, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/", $path)) as &[u8])
    };
}

const FILES: &[(&str, &str, &[u8])] = &[
    asset!("index.html", "text/html; charset=utf-8"),
    asset!("app.js", "text/javascript; charset=utf-8"),
    asset!("ui.css", "text/css; charset=utf-8"),
    asset!("doc.css", "text/css; charset=utf-8"),
    asset!("vendor/hljs/highlight.min.js", "text/javascript; charset=utf-8"),
    asset!("vendor/hljs/powershell.min.js", "text/javascript; charset=utf-8"),
    asset!("vendor/hljs/dos.min.js", "text/javascript; charset=utf-8"),
    asset!("vendor/hljs/dockerfile.min.js", "text/javascript; charset=utf-8"),
    asset!("vendor/hljs/cmake.min.js", "text/javascript; charset=utf-8"),
    asset!("vendor/katex/katex.min.js", "text/javascript; charset=utf-8"),
    asset!("vendor/katex/katex.min.css", "text/css; charset=utf-8"),
    asset!("vendor/katex/fonts/KaTeX_AMS-Regular.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Caligraphic-Bold.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Caligraphic-Regular.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Fraktur-Bold.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Fraktur-Regular.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Main-Bold.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Main-BoldItalic.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Main-Italic.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Main-Regular.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Math-BoldItalic.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Math-Italic.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_SansSerif-Bold.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_SansSerif-Italic.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_SansSerif-Regular.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Script-Regular.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Size1-Regular.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Size2-Regular.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Size3-Regular.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Size4-Regular.woff2", "font/woff2"),
    asset!("vendor/katex/fonts/KaTeX_Typewriter-Regular.woff2", "font/woff2"),
];

pub fn get(path: &str) -> Option<(&'static str, &'static [u8])> {
    FILES.iter().find(|(p, _, _)| *p == path).map(|(_, mime, data)| (*mime, *data))
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_assets() {
        assert!(super::get("index.html").is_some());
        assert!(super::get("vendor/katex/fonts/KaTeX_Main-Regular.woff2").is_some());
        assert!(super::get("../Cargo.toml").is_none());
    }
}
