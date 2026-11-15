//! Markdown to preview blocks.

pub mod html;
pub mod sanitize;
pub mod slug;
pub mod urls;

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use pulldown_cmark::{Event, Options, Parser, Tag};
use serde::Serialize;

use sanitize::Sanitizer;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TocEntry {
    pub level: u8,
    pub text: String,
    pub id: String,
    pub line: u32,
}

/// One top-level Markdown element with its source line span (0-based, inclusive).
#[derive(Debug, Clone)]
pub struct Block {
    pub html: Rc<str>,
    pub hash: u64,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Default)]
pub struct Rendered {
    pub blocks: Vec<Block>,
    pub toc: Vec<TocEntry>,
    pub has_math: bool,
}

#[derive(Debug)]
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = Vec::with_capacity(text.len() / 32 + 1);
        starts.push(0);
        starts.extend(memchr_newlines(text.as_bytes()).map(|i| i + 1));
        Self { starts }
    }

    pub fn line_of(&self, offset: usize) -> u32 {
        (self.starts.partition_point(|&s| s <= offset).saturating_sub(1)) as u32
    }
}

fn memchr_newlines(bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
    bytes.iter().enumerate().filter_map(|(i, &b)| (b == b'\n').then_some(i))
}

pub fn parser_options(math: bool) -> Options {
    let mut o = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_GFM;
    if math {
        o |= Options::ENABLE_MATH;
    }
    o
}

pub fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[derive(Debug, Clone)]
struct Cached {
    html: Rc<str>,
    hash: u64,
    math: bool,
}

/// Renders documents and reuses the HTML of blocks whose source did not change.
#[derive(Debug)]
pub struct Renderer {
    sanitizer: Sanitizer,
    cache: HashMap<u64, Cached>,
}

/// Blocks whose HTML depends on other parts of the document are never cached.
fn context_dependent(events: &[(Event<'_>, Range<usize>)]) -> bool {
    events.iter().any(|(e, _)| {
        matches!(e, Event::Start(Tag::Heading { .. }) | Event::Start(Tag::FootnoteDefinition(_)) | Event::FootnoteReference(_))
    })
}

impl Renderer {
    pub fn new(base: Option<PathBuf>) -> Self {
        Self { sanitizer: Sanitizer::new(base), cache: HashMap::new() }
    }

    pub fn base(&self) -> Option<&PathBuf> {
        self.sanitizer.base()
    }

    /// Renders the whole document into blocks. Blocks with raw HTML go through the sanitizer.
    pub fn render(&mut self, text: &str, math: bool) -> Rendered {
        let lines = LineIndex::new(text);
        let base = self.sanitizer.base().cloned();
        let mut ctx = html::Context::new(&lines, base.as_deref(), math);
        let mut blocks = Vec::new();
        let mut next_cache = HashMap::with_capacity(self.cache.len().max(64));
        let mut pending: Vec<(Event<'_>, Range<usize>)> = Vec::new();
        let mut depth = 0usize;
        let bytes = text.as_bytes();

        for (event, range) in Parser::new_ext(text, parser_options(math)).into_offset_iter() {
            match &event {
                Event::Start(_) => depth += 1,
                Event::End(_) => depth = depth.saturating_sub(1),
                _ => {}
            }
            pending.push((event, range));
            if depth != 0 {
                continue;
            }
            let span = pending[0].1.clone();
            let mut last = span.end.min(bytes.len());
            while last > span.start + 1 && bytes[last - 1].is_ascii_whitespace() {
                last -= 1;
            }
            let start = lines.line_of(span.start);
            let end = lines.line_of(last.saturating_sub(1).max(span.start)).max(start);

            let volatile = context_dependent(&pending);
            let key = {
                let mut h = DefaultHasher::new();
                math.hash(&mut h);
                text.get(span.clone()).unwrap_or("").hash(&mut h);
                h.finish()
            };
            let cached = if volatile { None } else { self.cache.get(&key).cloned() };
            let entry = match cached {
                Some(c) => c,
                None => {
                    let before = ctx.has_math;
                    ctx.has_math = false;
                    let out = html::write_block(&pending, start, &mut ctx);
                    let block_math = ctx.has_math;
                    ctx.has_math = before;
                    let html = if out.raw { self.sanitizer.clean(hash_str(&out.html), &out.html) } else { out.html };
                    Cached { hash: hash_str(&html), html: Rc::from(html), math: block_math }
                }
            };
            ctx.has_math |= entry.math;
            if !volatile {
                next_cache.insert(key, entry.clone());
            }
            blocks.push(Block { html: entry.html, hash: entry.hash, start, end });
            pending.clear();
        }
        self.cache = next_cache;
        Rendered { blocks, toc: ctx.toc, has_math: ctx.has_math }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(md: &str) -> Rendered {
        Renderer::new(None).render(md, true)
    }

    #[test]
    fn line_index() {
        let li = LineIndex::new("a\nbb\n\nc");
        assert_eq!(li.line_of(0), 0);
        assert_eq!(li.line_of(2), 1);
        assert_eq!(li.line_of(5), 2);
        assert_eq!(li.line_of(6), 3);
        assert_eq!(li.line_of(100), 3);
    }

    #[test]
    fn splits_top_level_blocks_with_lines() {
        let out = r("# Title\n\npara one\nstill one\n\n- a\n- b\n\n---\n");
        let spans: Vec<_> = out.blocks.iter().map(|b| (b.start, b.end)).collect();
        assert_eq!(spans, vec![(0, 0), (2, 3), (5, 6), (8, 8)]);
        assert!(out.blocks[0].html.starts_with("<h1 id=\"title\" data-l=\"0\">Title<a class=\"anchor\""));
        assert!(out.blocks[2].html.contains("<li data-l=\"1\">b</li>"));
        assert_eq!(out.toc, vec![TocEntry { level: 1, text: "Title".into(), id: "title".into(), line: 0 }]);
    }

    #[test]
    fn block_html_is_position_independent() {
        let a = r("para\n\n```rust\nfn x() {}\n```\n");
        let b = r("new\n\n\n\npara\n\n```rust\nfn x() {}\n```\n");
        assert_eq!(a.blocks[1].html, b.blocks[2].html);
        assert_ne!(a.blocks[1].start, b.blocks[2].start);
    }

    #[test]
    fn code_block_markup() {
        let out = r("```Rust title\nlet a = 1 < 2;\n```\n");
        let h = &out.blocks[0].html;
        assert!(h.contains("<span class=\"code-lang\">Rust</span>"));
        assert!(h.contains("<code class=\"language-rust\">let a = 1 &lt; 2;\n</code>"));
        assert!(h.contains("code-copy"));
    }

    #[test]
    fn gfm_features() {
        let out = r("> [!WARNING]\n> Careful\n\n- [x] done\n- [ ] todo\n\n| a | b |\n|:-|-:|\n| 1 | 2 |\n\n~~gone~~ and $x^2$\n");
        let all: String = out.blocks.iter().map(|b| &*b.html).collect();
        assert!(all.contains("<div class=\"alert alert-warning\""));
        assert!(all.contains("<p class=\"alert-title\">Warning</p>"));
        assert!(all.contains("<input type=\"checkbox\" disabled checked>"));
        assert!(all.contains("<th class=\"al-l\">a</th><th class=\"al-r\">b</th>"));
        assert!(all.contains("<tbody><tr data-l=\"2\"><td class=\"al-l\">1</td>"));
        assert!(all.contains("<del>gone</del>"));
        assert!(all.contains("<span class=\"math-inline\">x^2</span>"));
        assert!(out.has_math);
    }

    #[test]
    fn math_fence_and_disabled_math() {
        let md = "```math\na+b\n```\n\n$y$\n";
        assert!(r(md).blocks[0].html.starts_with("<div class=\"math-display\""));
        let off = Renderer::new(None).render(md, false);
        assert!(!off.has_math);
        assert!(off.blocks[0].html.contains("class=\"code\""));
        assert!(off.blocks[1].html.contains("$y$"));
    }

    #[test]
    fn footnotes_are_numbered() {
        let out = r("Text[^b] and[^a].\n\n[^a]: A\n[^b]: B\n");
        let all: String = out.blocks.iter().map(|b| &*b.html).collect();
        assert!(all.contains("<a href=\"#fn-b\" id=\"fnref-b\">1</a>"));
        assert!(all.contains("<a href=\"#fn-a\" id=\"fnref-a\">2</a>"));
        assert!(all.contains("id=\"fn-a\""));
    }

    #[test]
    fn raw_html_is_sanitized() {
        let out = r("<div onclick=\"x()\">ok<script>bad()</script></div>\n\ntext <b onmouseover=\"y\">b</b>\n");
        let all: String = out.blocks.iter().map(|b| &*b.html).collect();
        assert!(!all.contains("script"));
        assert!(!all.contains("onclick"));
        assert!(!all.contains("onmouseover"));
        assert!(all.contains("<b>b</b>"));
    }

    #[test]
    fn unsafe_links_lose_href() {
        let out = r("[x](javascript:alert(1)) [y](https://e.com) [z](docs/a.md)\n");
        let h = &out.blocks[0].html;
        assert!(h.contains("<a>x</a>"));
        assert!(h.contains("<a href=\"https://e.com\">y</a>"));
        assert!(h.contains("<a href=\"docs/a.md\">z</a>"));
    }

    #[test]
    fn images_resolve_against_base() {
        let out = Renderer::new(Some("C:\\docs".into())).render("![alt *text*](img/a.png \"T\")\n", true);
        let h = &out.blocks[0].html;
        assert!(h.contains("src=\"https://mahimarkd.localhost/file/C:/docs/img/a.png\""), "{h}");
        assert!(h.contains("data-orig=\"img/a.png\""));
        assert!(h.contains("alt=\"alt text\" title=\"T\" loading=\"lazy\">"));
    }

    #[test]
    fn front_matter_and_duplicate_headings() {
        let out = r("---\ntitle: x\n---\n\n## A\n\n## A\n");
        assert!(out.blocks[0].html.contains("frontmatter"));
        assert_eq!(out.toc.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["a", "a-1"]);
    }

    #[test]
    fn cached_render_matches_fresh_render() {
        let mut renderer = Renderer::new(None);
        let a = "# T\n\n$x$ para\n\n## T\n\nText[^1]\n\n[^1]: n\n";
        let first = renderer.render(a, true);
        let second = renderer.render(a, true);
        assert!(second.has_math);
        assert_eq!(first.toc, second.toc);
        let fresh = Renderer::new(None).render(a, true);
        for (x, y) in second.blocks.iter().zip(fresh.blocks.iter()) {
            assert_eq!(x.html, y.html);
            assert_eq!(x.hash, y.hash);
        }
        let edited = renderer.render("# New\n\n# T\n\n$x$ para\n\n## T\n\nText[^1]\n\n[^1]: n\n", true);
        assert_eq!(edited.toc.iter().map(|t| t.line).collect::<Vec<_>>(), vec![0, 2, 6]);
        assert_eq!(edited.toc[2].id, "t-1");
    }

    #[test]
    fn crlf_lines() {
        let out = r("# A\r\n\r\ntext\r\n");
        assert_eq!(out.blocks[1].start, 2);
    }
}
