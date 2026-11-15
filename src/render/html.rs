//! HTML writer for one top-level Markdown block.
//!
//! Elements carry `data-l` with the line offset from the block start, so the
//! block HTML stays the same when lines above it are added or removed.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, LinkType, Tag, TagEnd};

use super::slug::{Slugger, slugify};
use super::urls;
use super::{LineIndex, TocEntry};
use crate::text::escape_html;

#[derive(Debug)]
pub struct Context<'a> {
    pub lines: &'a LineIndex,
    pub base: Option<&'a Path>,
    pub math: bool,
    pub slugger: Slugger,
    pub footnotes: HashMap<String, usize>,
    pub toc: Vec<TocEntry>,
    pub has_math: bool,
}

impl<'a> Context<'a> {
    pub fn new(lines: &'a LineIndex, base: Option<&'a Path>, math: bool) -> Self {
        Self { lines, base, math, slugger: Slugger::default(), footnotes: HashMap::new(), toc: Vec::new(), has_math: false }
    }

    fn footnote_number(&mut self, label: &str) -> usize {
        let next = self.footnotes.len() + 1;
        *self.footnotes.entry(label.to_string()).or_insert(next)
    }
}

#[derive(Debug)]
struct Heading {
    level: u8,
    id: Option<String>,
    classes: Vec<String>,
    pos: usize,
    line: u32,
    rel: u32,
    text: String,
}

struct Writer<'c, 'a> {
    ctx: &'c mut Context<'a>,
    out: String,
    block_line: u32,
    raw: bool,
    aligns: Vec<Alignment>,
    cell: usize,
    in_head: bool,
    tbody_open: bool,
    heading: Option<Heading>,
    image_depth: usize,
    image_title: String,
    in_math_block: bool,
}

/// Output of [`write_block`]: the HTML and whether it contains raw HTML from the document.
#[derive(Debug)]
pub struct BlockHtml {
    pub html: String,
    pub raw: bool,
}

pub fn write_block(events: &[(Event<'_>, Range<usize>)], block_line: u32, ctx: &mut Context<'_>) -> BlockHtml {
    let mut w = Writer {
        ctx,
        out: String::new(),
        block_line,
        raw: false,
        aligns: Vec::new(),
        cell: 0,
        in_head: false,
        tbody_open: false,
        heading: None,
        image_depth: 0,
        image_title: String::new(),
        in_math_block: false,
    };
    for (event, range) in events {
        w.event(event, range.start);
    }
    BlockHtml { html: w.out, raw: w.raw }
}

fn alert_title(kind: BlockQuoteKind) -> (&'static str, &'static str) {
    match kind {
        BlockQuoteKind::Note => ("note", "Note"),
        BlockQuoteKind::Tip => ("tip", "Tip"),
        BlockQuoteKind::Important => ("important", "Important"),
        BlockQuoteKind::Warning => ("warning", "Warning"),
        BlockQuoteKind::Caution => ("caution", "Caution"),
    }
}

fn clean_lang(info: &str) -> String {
    let first = info.split(|c: char| c.is_whitespace() || c == ',' || c == '{').next().unwrap_or("");
    first.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '#' | '.' | '-')).take(40).collect()
}

fn footnote_id(label: &str, n: usize) -> String {
    let s = slugify(label);
    if s.is_empty() { n.to_string() } else { s }
}

impl Writer<'_, '_> {
    fn rel(&self, offset: usize) -> u32 {
        self.ctx.lines.line_of(offset).saturating_sub(self.block_line)
    }

    fn open(&mut self, tag: &str, offset: usize) {
        let rel = self.rel(offset);
        self.out.push('<');
        self.out.push_str(tag);
        self.out.push_str(" data-l=\"");
        self.out.push_str(&rel.to_string());
        self.out.push_str("\">");
    }

    fn esc(&mut self, s: &str) {
        escape_html(&mut self.out, s);
    }

    fn capture_text(&mut self, s: &str) {
        if let Some(h) = self.heading.as_mut() {
            h.text.push_str(s);
        }
    }

    fn event(&mut self, event: &Event<'_>, offset: usize) {
        if self.image_depth > 0 {
            self.image_event(event);
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag, offset),
            Event::End(tag) => self.end(*tag),
            Event::Text(t) => {
                self.esc(t);
                self.capture_text(t);
            }
            Event::Code(t) => {
                self.out.push_str("<code>");
                self.esc(t);
                self.out.push_str("</code>");
                self.capture_text(t);
            }
            Event::InlineMath(t) => {
                self.ctx.has_math = true;
                self.out.push_str("<span class=\"math-inline\">");
                self.esc(t);
                self.out.push_str("</span>");
                self.capture_text(t);
            }
            Event::DisplayMath(t) => {
                self.ctx.has_math = true;
                self.out.push_str("<span class=\"math-display\">");
                self.esc(t);
                self.out.push_str("</span>");
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                self.raw = true;
                self.out.push_str(t);
            }
            Event::FootnoteReference(label) => {
                let n = self.ctx.footnote_number(label);
                let id = footnote_id(label, n);
                self.out.push_str(&format!("<sup class=\"fn-ref\"><a href=\"#fn-{id}\" id=\"fnref-{id}\">{n}</a></sup>"));
            }
            Event::SoftBreak => {
                self.out.push('\n');
                self.capture_text(" ");
            }
            Event::HardBreak => {
                self.out.push_str("<br>\n");
                self.capture_text(" ");
            }
            Event::Rule => self.open("hr", offset),
            Event::TaskListMarker(checked) => {
                self.out.push_str(if *checked {
                    "<input type=\"checkbox\" disabled checked> "
                } else {
                    "<input type=\"checkbox\" disabled> "
                });
            }
        }
    }

    fn image_event(&mut self, event: &Event<'_>) {
        match event {
            Event::Start(Tag::Image { .. }) => self.image_depth += 1,
            Event::End(TagEnd::Image) => {
                self.image_depth -= 1;
                if self.image_depth == 0 {
                    self.out.push('"');
                    if !self.image_title.is_empty() {
                        let title = std::mem::take(&mut self.image_title);
                        self.out.push_str(" title=\"");
                        self.esc(&title);
                        self.out.push('"');
                    }
                    self.out.push_str(" loading=\"lazy\">");
                }
            }
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => self.esc(t),
            Event::SoftBreak | Event::HardBreak => self.out.push(' '),
            _ => {}
        }
    }

    fn start(&mut self, tag: &Tag<'_>, offset: usize) {
        match tag {
            Tag::Paragraph => self.open("p", offset),
            Tag::Heading { level, id, classes, .. } => {
                let rel = self.rel(offset);
                self.heading = Some(Heading {
                    level: *level as u8,
                    id: id.as_ref().map(|s| s.to_string()),
                    classes: classes.iter().map(|c| c.to_string()).collect(),
                    pos: self.out.len(),
                    line: self.block_line + rel,
                    rel,
                    text: String::new(),
                });
            }
            Tag::BlockQuote(Some(kind)) => {
                let (class, title) = alert_title(*kind);
                let rel = self.rel(offset);
                self.out.push_str(&format!("<div class=\"alert alert-{class}\" data-l=\"{rel}\"><p class=\"alert-title\">{title}</p>"));
            }
            Tag::BlockQuote(None) => self.open("blockquote", offset),
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => clean_lang(info),
                    CodeBlockKind::Indented => String::new(),
                };
                let rel = self.rel(offset);
                if self.ctx.math && lang.eq_ignore_ascii_case("math") {
                    self.ctx.has_math = true;
                    self.in_math_block = true;
                    self.out.push_str(&format!("<div class=\"math-display\" data-l=\"{rel}\">"));
                } else {
                    self.out.push_str(&format!("<div class=\"code\" data-l=\"{rel}\"><div class=\"code-bar\"><span class=\"code-lang\">"));
                    self.esc(&lang);
                    self.out.push_str("</span><button class=\"code-copy\" type=\"button\" title=\"Copy code\"></button></div><pre><code");
                    if !lang.is_empty() {
                        self.out.push_str(" class=\"language-");
                        self.esc(&lang.to_ascii_lowercase());
                        self.out.push('"');
                    }
                    self.out.push('>');
                }
            }
            Tag::HtmlBlock => {}
            Tag::List(Some(start)) => {
                let rel = self.rel(offset);
                if *start == 1 {
                    self.out.push_str(&format!("<ol data-l=\"{rel}\">"));
                } else {
                    self.out.push_str(&format!("<ol start=\"{start}\" data-l=\"{rel}\">"));
                }
            }
            Tag::List(None) => self.open("ul", offset),
            Tag::Item => self.open("li", offset),
            Tag::FootnoteDefinition(label) => {
                let n = self.ctx.footnote_number(label);
                let id = footnote_id(label, n);
                let rel = self.rel(offset);
                self.out.push_str(&format!(
                    "<div class=\"footnote\" id=\"fn-{id}\" data-l=\"{rel}\"><a class=\"fn-num\" href=\"#fnref-{id}\">{n}</a><div class=\"fn-body\">"
                ));
            }
            Tag::DefinitionList => self.open("dl", offset),
            Tag::DefinitionListTitle => self.open("dt", offset),
            Tag::DefinitionListDefinition => self.open("dd", offset),
            Tag::Table(aligns) => {
                self.aligns = aligns.clone();
                self.tbody_open = false;
                let rel = self.rel(offset);
                self.out.push_str(&format!("<div class=\"table-wrap\" data-l=\"{rel}\"><table>"));
            }
            Tag::TableHead => {
                self.in_head = true;
                self.cell = 0;
                self.out.push_str("<thead>");
                self.open("tr", offset);
            }
            Tag::TableRow => {
                self.cell = 0;
                if !self.tbody_open {
                    self.tbody_open = true;
                    self.out.push_str("<tbody>");
                }
                self.open("tr", offset);
            }
            Tag::TableCell => {
                let tag = if self.in_head { "th" } else { "td" };
                let class = match self.aligns.get(self.cell) {
                    Some(Alignment::Left) => " class=\"al-l\"",
                    Some(Alignment::Center) => " class=\"al-c\"",
                    Some(Alignment::Right) => " class=\"al-r\"",
                    _ => "",
                };
                self.out.push_str(&format!("<{tag}{class}>"));
            }
            Tag::Emphasis => self.out.push_str("<em>"),
            Tag::Strong => self.out.push_str("<strong>"),
            Tag::Strikethrough => self.out.push_str("<del>"),
            Tag::Superscript => self.out.push_str("<sup>"),
            Tag::Subscript => self.out.push_str("<sub>"),
            Tag::Link { link_type, dest_url, title, .. } => {
                match urls::safe_href(dest_url, *link_type == LinkType::Email) {
                    Some(href) => {
                        self.out.push_str("<a href=\"");
                        self.esc(&href);
                        self.out.push('"');
                    }
                    None => self.out.push_str("<a"),
                }
                if !title.is_empty() {
                    self.out.push_str(" title=\"");
                    self.esc(title);
                    self.out.push('"');
                }
                self.out.push('>');
            }
            Tag::Image { dest_url, title, .. } => {
                self.out.push_str("<img");
                if let Some(src) = urls::image_src(dest_url, self.ctx.base) {
                    self.out.push_str(" src=\"");
                    self.esc(&src);
                    self.out.push('"');
                    if src.starts_with(urls::FILE_PREFIX) {
                        self.out.push_str(" data-orig=\"");
                        self.esc(dest_url);
                        self.out.push('"');
                    }
                }
                self.out.push_str(" alt=\"");
                self.image_depth = 1;
                self.image_title = title.to_string();
            }
            Tag::MetadataBlock(_) => {
                let rel = self.rel(offset);
                self.out.push_str(&format!(
                    "<div class=\"code frontmatter\" data-l=\"{rel}\"><div class=\"code-bar\"><span class=\"code-lang\">front matter</span><button class=\"code-copy\" type=\"button\" title=\"Copy code\"></button></div><pre><code class=\"language-yaml\">"
                ));
            }
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.out.push_str("</p>"),
            TagEnd::Heading(_) => self.finish_heading(),
            TagEnd::BlockQuote(Some(_)) => self.out.push_str("</div>"),
            TagEnd::BlockQuote(None) => self.out.push_str("</blockquote>"),
            TagEnd::CodeBlock => {
                if self.in_math_block {
                    self.in_math_block = false;
                    self.out.push_str("</div>");
                } else {
                    self.out.push_str("</code></pre></div>");
                }
            }
            TagEnd::HtmlBlock => {}
            TagEnd::List(true) => self.out.push_str("</ol>"),
            TagEnd::List(false) => self.out.push_str("</ul>"),
            TagEnd::Item => self.out.push_str("</li>"),
            TagEnd::FootnoteDefinition => self.out.push_str("</div></div>"),
            TagEnd::DefinitionList => self.out.push_str("</dl>"),
            TagEnd::DefinitionListTitle => self.out.push_str("</dt>"),
            TagEnd::DefinitionListDefinition => self.out.push_str("</dd>"),
            TagEnd::Table => {
                if self.tbody_open {
                    self.out.push_str("</tbody>");
                }
                self.out.push_str("</table></div>");
            }
            TagEnd::TableHead => {
                self.in_head = false;
                self.out.push_str("</tr></thead>");
            }
            TagEnd::TableRow => self.out.push_str("</tr>"),
            TagEnd::TableCell => {
                self.out.push_str(if self.in_head { "</th>" } else { "</td>" });
                self.cell += 1;
            }
            TagEnd::Emphasis => self.out.push_str("</em>"),
            TagEnd::Strong => self.out.push_str("</strong>"),
            TagEnd::Strikethrough => self.out.push_str("</del>"),
            TagEnd::Superscript => self.out.push_str("</sup>"),
            TagEnd::Subscript => self.out.push_str("</sub>"),
            TagEnd::Link => self.out.push_str("</a>"),
            TagEnd::Image => {}
            TagEnd::MetadataBlock(_) => self.out.push_str("</code></pre></div>"),
        }
    }

    fn finish_heading(&mut self) {
        let Some(h) = self.heading.take() else { return };
        let text = h.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let id = match h.id {
            Some(id) => {
                self.ctx.slugger.reserve(&id);
                id
            }
            None => self.ctx.slugger.slug(&text),
        };
        let mut open = format!("<h{} id=\"", h.level);
        escape_html(&mut open, &id);
        open.push_str(&format!("\" data-l=\"{}\"", h.rel));
        if !h.classes.is_empty() {
            open.push_str(" class=\"");
            escape_html(&mut open, &h.classes.join(" "));
            open.push('"');
        }
        open.push('>');
        self.out.insert_str(h.pos, &open);
        self.out.push_str("<a class=\"anchor\" href=\"#");
        self.esc(&id);
        self.out.push_str(&format!("\" title=\"Copy link to this section\"></a></h{}>", h.level));
        self.ctx.toc.push(TocEntry { level: h.level, text, id, line: h.line });
    }
}
