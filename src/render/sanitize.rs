//! Cleans blocks that contain raw HTML from the document.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use ammonia::{Builder, UrlRelative, UrlRelativeEvaluate};

use super::urls;

const CACHE_LIMIT: usize = 4096;

pub struct Sanitizer {
    builder: Builder<'static>,
    base: Option<PathBuf>,
    cache: HashMap<u64, String>,
}

impl std::fmt::Debug for Sanitizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sanitizer").field("base", &self.base).field("cached", &self.cache.len()).finish()
    }
}

impl Sanitizer {
    pub fn new(base: Option<PathBuf>) -> Self {
        Self { builder: make_builder(base.clone()), base, cache: HashMap::new() }
    }

    pub fn base(&self) -> Option<&PathBuf> {
        self.base.as_ref()
    }

    pub fn clean(&mut self, key: u64, html: &str) -> String {
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let cleaned = self.builder.clean(html).to_string();
        if self.cache.len() >= CACHE_LIMIT {
            self.cache.clear();
        }
        self.cache.insert(key, cleaned.clone());
        cleaned
    }
}

fn make_builder(base: Option<PathBuf>) -> Builder<'static> {
    let mut b = Builder::default();
    b.add_tags(["input", "button", "picture", "source", "video", "audio", "figure", "figcaption", "abbr", "u", "s", "center"]);
    b.add_generic_attributes(["class", "id", "data-l", "data-orig", "align", "dir", "title", "lang"]);
    b.add_tag_attributes("input", ["type", "checked", "disabled"]);
    b.add_tag_attributes("button", ["type"]);
    b.add_tag_attributes("img", ["loading", "srcset"]);
    b.add_tag_attributes("source", ["src", "srcset", "type", "media"]);
    b.add_tag_attributes("video", ["src", "controls", "width", "height", "poster", "loop", "muted"]);
    b.add_tag_attributes("audio", ["src", "controls", "loop", "muted"]);
    b.add_tag_attributes("details", ["open"]);
    b.add_tag_attributes("ol", ["type", "reversed"]);
    b.add_tag_attributes("td", ["width"]);
    b.add_tag_attributes("th", ["width"]);
    b.add_url_schemes(["data", "file"]);
    b.link_rel(Some("noopener noreferrer"));
    b.url_relative(UrlRelative::Custom(Box::new(LocalUrls { base })));
    b
}

struct LocalUrls {
    base: Option<PathBuf>,
}

impl UrlRelativeEvaluate<'static> for LocalUrls {
    fn evaluate<'url>(&self, url: &'url str) -> Option<Cow<'url, str>> {
        if url.starts_with('#') {
            return Some(Cow::Borrowed(url));
        }
        match urls::resolve_local(url, self.base.as_deref()) {
            Some(path) => Some(Cow::Owned(urls::file_url(&path))),
            None => Some(Cow::Borrowed(url)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_handlers() {
        let mut s = Sanitizer::new(None);
        let out = s.clean(1, r#"<p onclick="x()">hi<script>alert(1)</script></p><a href="javascript:alert(1)">x</a>"#);
        assert!(!out.contains("script"));
        assert!(!out.contains("onclick"));
        assert!(!out.contains("javascript"));
        assert!(out.contains("<p>hi</p>"));
    }

    #[test]
    fn keeps_preview_markup() {
        let mut s = Sanitizer::new(None);
        let html = r#"<div class="code" data-l="2"><button class="code-copy" type="button"></button></div><details open><summary>S</summary>x</details>"#;
        let out = s.clean(2, html);
        assert!(out.contains(r#"class="code""#));
        assert!(out.contains(r#"data-l="2""#));
        assert!(out.contains("<details open"));
        assert!(out.contains("<button"));
    }

    #[test]
    fn rewrites_relative_images() {
        let mut s = Sanitizer::new(Some(PathBuf::from("C:\\docs")));
        let out = s.clean(3, r##"<img src="pics/a.png"><a href="#top">t</a>"##);
        assert!(out.contains("https://mahimarkd.localhost/file/C:/docs/pics/a.png"), "{out}");
        assert!(out.contains(r##"href="#top""##));
    }
}
