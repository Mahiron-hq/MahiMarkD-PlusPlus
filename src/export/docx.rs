//! Word (.docx) export straight from the Markdown source.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, LinkType, Parser, Tag, TagEnd};

use super::zip::ZipWriter;
use crate::render::slug::Slugger;
use crate::render::{parser_options, urls};

const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_IMAGE_WIDTH_EMU: u64 = 5_943_600;
const EMU_PER_PX: u64 = 9525;
const LIST_INDENT: u32 = 720;

/// Fetches remote image bytes. Returns `None` when unavailable.
pub type Fetch<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

pub fn write_file(markdown: &str, base: Option<&Path>, math: bool, title: &str, target: &Path) -> std::io::Result<()> {
    let bytes = build(markdown, base, math, title, &fetch_remote)?;
    let tmp = target.with_extension("docx.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, target).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(windows)]
fn fetch_remote(url: &str) -> Option<Vec<u8>> {
    use windows::Win32::System::Com::Urlmon::URLDownloadToCacheFileW;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use windows::core::HSTRING;

    let mut buf = [0u16; 1024];
    // SAFETY: COM init for this worker thread; the buffer is valid for its length.
    let path = unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        URLDownloadToCacheFileW(None, &HSTRING::from(url), &mut buf, 0, None).ok()?;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..n])
    };
    let meta = fs::metadata(&path).ok()?;
    if meta.len() as usize > MAX_IMAGE_BYTES {
        return None;
    }
    fs::read(path).ok()
}

#[cfg(not(windows))]
fn fetch_remote(_url: &str) -> Option<Vec<u8>> {
    None
}

fn xml_escape(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    xml_escape(&mut out, s);
    out
}

fn bookmark_name(slug: &str) -> String {
    let mut name = String::from("_");
    for c in slug.chars() {
        name.push(if c.is_alphanumeric() { c } else { '_' });
        if name.chars().count() >= 40 {
            break;
        }
    }
    name
}

fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[derive(Debug, Default, Clone, Copy)]
struct Fmt {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    sup: bool,
    sub: bool,
    link: bool,
    math: bool,
}

#[derive(Debug)]
struct ListCtx {
    num_id: u32,
    first_par_pending: bool,
}

#[derive(Debug)]
struct Image {
    ext: &'static str,
    data: Vec<u8>,
    width: u64,
    height: u64,
}

#[derive(Debug)]
struct Para {
    style: Option<String>,
    num: Option<(u32, u32)>,
    indent: Option<u32>,
    align: Option<&'static str>,
    border_bottom: bool,
    runs: String,
    bookmark_text: Option<String>,
}

struct Doc<'f> {
    base: Option<std::path::PathBuf>,
    fetch: Fetch<'f>,
    targets: Vec<String>,
    par: Option<Para>,
    fmt: Fmt,
    fmt_stack: Vec<Fmt>,
    quote: u32,
    lists: Vec<ListCtx>,
    nums: Vec<(u32, bool, u64)>,
    table_aligns: Vec<Alignment>,
    cell: usize,
    header_row: bool,
    in_cell: bool,
    code: Option<(String, bool)>,
    heading: Option<(HeadingLevel, Option<String>, String)>,
    image: Option<(String, String)>,
    images: Vec<Image>,
    rels: Vec<(String, &'static str, String, bool)>,
    footnote_ids: HashMap<String, u32>,
    footnotes: Vec<(u32, String)>,
    footnote_stack: Vec<u32>,
    slugger: Slugger,
    bookmark_id: u32,
    skip_depth: u32,
    pic_id: u32,
}

const REL_HYPERLINK: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

impl<'f> Doc<'f> {
    fn new(base: Option<&Path>, fetch: Fetch<'f>) -> Self {
        Self {
            base: base.map(Path::to_path_buf),
            fetch,
            targets: vec![String::new()],
            par: None,
            fmt: Fmt::default(),
            fmt_stack: Vec::new(),
            quote: 0,
            lists: Vec::new(),
            nums: Vec::new(),
            table_aligns: Vec::new(),
            cell: 0,
            header_row: false,
            in_cell: false,
            code: None,
            heading: None,
            image: None,
            images: Vec::new(),
            rels: Vec::new(),
            footnote_ids: HashMap::new(),
            footnotes: Vec::new(),
            footnote_stack: Vec::new(),
            slugger: Slugger::default(),
            bookmark_id: 0,
            skip_depth: 0,
            pic_id: 0,
        }
    }

    fn out(&mut self) -> &mut String {
        self.targets.last_mut().expect("target stack is never empty")
    }

    fn add_rel(&mut self, kind: &'static str, target: String, external: bool) -> String {
        let id = format!("rId{}", self.rels.len() + 10);
        self.rels.push((id.clone(), kind, target, external));
        id
    }

    fn footnote_id(&mut self, label: &str) -> u32 {
        let next = self.footnote_ids.len() as u32 + 1;
        *self.footnote_ids.entry(label.to_string()).or_insert(next)
    }

    fn context_style(&self) -> Option<String> {
        if self.in_cell {
            Some("TableText".into())
        } else if self.footnote_stack.last().is_some() {
            Some("FootnoteText".into())
        } else if self.quote > 0 {
            Some("Quote".into())
        } else if !self.lists.is_empty() {
            Some("ListParagraph".into())
        } else {
            None
        }
    }

    fn open_par(&mut self, style: Option<String>) {
        self.close_par();
        let style = style.or_else(|| self.context_style());
        let level = self.lists.len().saturating_sub(1) as u32;
        let mut num = None;
        let mut indent = None;
        if let Some(list) = self.lists.last_mut() {
            if list.first_par_pending {
                list.first_par_pending = false;
                num = Some((list.num_id, level.min(8)));
            } else {
                indent = Some(LIST_INDENT * (level + 1));
            }
        }
        if self.quote > 1 && indent.is_none() && num.is_none() {
            indent = Some(360 * self.quote);
        }
        let align = if self.in_cell {
            match self.table_aligns.get(self.cell) {
                Some(Alignment::Center) => Some("center"),
                Some(Alignment::Right) => Some("right"),
                _ => None,
            }
        } else {
            None
        };
        let mut runs = String::new();
        if !self.footnote_stack.is_empty() && self.out().is_empty() {
            runs.push_str("<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> </w:t></w:r>");
        }
        self.par = Some(Para { style, num, indent, align, border_bottom: false, runs, bookmark_text: None });
    }

    fn ensure_par(&mut self) {
        if self.par.is_none() {
            self.open_par(None);
        }
    }

    fn close_par(&mut self) {
        let Some(p) = self.par.take() else { return };
        let mut xml = String::from("<w:p><w:pPr>");
        if let Some(s) = &p.style {
            xml.push_str(&format!("<w:pStyle w:val=\"{s}\"/>"));
        }
        if let Some((num, lvl)) = p.num {
            xml.push_str(&format!("<w:numPr><w:ilvl w:val=\"{lvl}\"/><w:numId w:val=\"{num}\"/></w:numPr>"));
        }
        if p.border_bottom {
            xml.push_str("<w:pBdr><w:bottom w:val=\"single\" w:sz=\"8\" w:space=\"1\" w:color=\"BFBFBF\"/></w:pBdr>");
        }
        if let Some(ind) = p.indent {
            xml.push_str(&format!("<w:ind w:left=\"{ind}\"/>"));
        }
        if let Some(jc) = p.align {
            xml.push_str(&format!("<w:jc w:val=\"{jc}\"/>"));
        }
        xml.push_str("</w:pPr>");
        if let Some(text) = &p.bookmark_text {
            let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let slug = self.slugger.slug(&collapsed);
            self.push_bookmarked(&mut xml, &slug, &p.runs);
        } else {
            xml.push_str(&p.runs);
        }
        xml.push_str("</w:p>");
        self.out().push_str(&xml);
    }

    fn push_bookmarked(&mut self, xml: &mut String, slug: &str, runs: &str) {
        let id = self.bookmark_id;
        self.bookmark_id += 1;
        xml.push_str(&format!("<w:bookmarkStart w:id=\"{id}\" w:name=\"{}\"/>", esc(&bookmark_name(slug))));
        xml.push_str(runs);
        xml.push_str(&format!("<w:bookmarkEnd w:id=\"{id}\"/>"));
    }

    fn run_props(&self, fmt: Fmt) -> String {
        let mut p = String::new();
        if fmt.link {
            p.push_str("<w:rStyle w:val=\"Hyperlink\"/>");
        } else if fmt.code {
            p.push_str("<w:rStyle w:val=\"InlineCode\"/>");
        }
        if fmt.math {
            p.push_str("<w:rFonts w:ascii=\"Cambria Math\" w:hAnsi=\"Cambria Math\"/>");
        }
        if fmt.bold {
            p.push_str("<w:b/>");
        }
        if fmt.italic || fmt.math {
            p.push_str("<w:i/>");
        }
        if fmt.strike {
            p.push_str("<w:strike/>");
        }
        if fmt.sup {
            p.push_str("<w:vertAlign w:val=\"superscript\"/>");
        } else if fmt.sub {
            p.push_str("<w:vertAlign w:val=\"subscript\"/>");
        }
        if p.is_empty() { p } else { format!("<w:rPr>{p}</w:rPr>") }
    }

    fn text_run(&mut self, text: &str, fmt: Fmt) {
        if text.is_empty() {
            return;
        }
        self.ensure_par();
        let props = self.run_props(fmt);
        let mut xml = String::new();
        xml.push_str("<w:r>");
        xml.push_str(&props);
        let mut first = true;
        for line in text.split('\n') {
            if !first {
                xml.push_str("<w:br/>");
            }
            first = false;
            for (i, part) in line.split('\t').enumerate() {
                if i > 0 {
                    xml.push_str("<w:tab/>");
                }
                if !part.is_empty() {
                    xml.push_str("<w:t xml:space=\"preserve\">");
                    xml_escape(&mut xml, part.trim_end_matches('\r'));
                    xml.push_str("</w:t>");
                }
            }
        }
        xml.push_str("</w:r>");
        if let Some(p) = self.par.as_mut() {
            p.runs.push_str(&xml);
        }
    }

    fn raw_run(&mut self, xml: &str) {
        self.ensure_par();
        if let Some(p) = self.par.as_mut() {
            p.runs.push_str(xml);
        }
    }

    fn push_fmt(&mut self, f: impl FnOnce(&mut Fmt)) {
        self.fmt_stack.push(self.fmt);
        f(&mut self.fmt);
    }

    fn pop_fmt(&mut self) {
        if let Some(f) = self.fmt_stack.pop() {
            self.fmt = f;
        }
    }

    fn new_list(&mut self, start: Option<u64>) {
        let num_id = self.nums.len() as u32 + 1;
        let level = self.lists.len() as u32;
        self.nums.push((level.min(8), start.is_some(), start.unwrap_or(1)));
        self.lists.push(ListCtx { num_id, first_par_pending: false });
    }

    fn load_image(&self, dest: &str) -> Option<Image> {
        let bytes = match urls::scheme(dest).as_deref() {
            Some("http" | "https") => (self.fetch)(dest)?,
            Some("data") => {
                let comma = dest.find(',')?;
                let (meta, data) = dest.split_at(comma);
                if !meta.to_ascii_lowercase().contains(";base64") {
                    return None;
                }
                decode_base64(&data[1..])?
            }
            Some("file") | None => {
                let path = urls::resolve_local(dest, self.base.as_deref())?;
                if fs::metadata(&path).ok()?.len() as usize > MAX_IMAGE_BYTES {
                    return None;
                }
                fs::read(path).ok()?
            }
            Some(_) => return None,
        };
        let ext = match imagesize::image_type(&bytes).ok()? {
            imagesize::ImageType::Png => "png",
            imagesize::ImageType::Jpeg => "jpeg",
            imagesize::ImageType::Gif => "gif",
            imagesize::ImageType::Bmp => "bmp",
            imagesize::ImageType::Tiff => "tiff",
            _ => return None,
        };
        let size = imagesize::blob_size(&bytes).ok()?;
        if size.width == 0 || size.height == 0 {
            return None;
        }
        Some(Image { ext, data: bytes, width: size.width as u64, height: size.height as u64 })
    }

    fn image(&mut self, dest: String, alt: String) {
        let Some(img) = self.load_image(&dest) else {
            let label = if alt.is_empty() { dest } else { alt };
            let fmt = Fmt { italic: true, ..self.fmt };
            self.text_run(&format!("[{label}]"), fmt);
            return;
        };
        let index = self.images.len() + 1;
        let name = format!("media/image{index}.{}", img.ext);
        let rel = self.add_rel(REL_IMAGE, name, false);
        let mut cx = img.width * EMU_PER_PX;
        let mut cy = img.height * EMU_PER_PX;
        if cx > MAX_IMAGE_WIDTH_EMU {
            cy = cy * MAX_IMAGE_WIDTH_EMU / cx;
            cx = MAX_IMAGE_WIDTH_EMU;
        }
        self.pic_id += 1;
        let id = self.pic_id;
        let alt = esc(&alt);
        let xml = format!(
            "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\" descr=\"{alt}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"image{index}.{ext}\" descr=\"{alt}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
            ext = img.ext
        );
        self.images.push(img);
        self.raw_run(&xml);
    }

    fn link_target(&self, dest: &str, is_email: bool) -> Option<(bool, String)> {
        let href = urls::safe_href(dest, is_email)?;
        if let Some(anchor) = href.strip_prefix('#') {
            return Some((true, bookmark_name(anchor)));
        }
        match urls::scheme(&href).as_deref() {
            Some(_) => Some((false, href)),
            None => {
                let path = urls::resolve_local(&href, self.base.as_deref())?;
                let url = format!("file:///{}", path.to_string_lossy().replace('\\', "/"));
                Some((false, url))
            }
        }
    }

    fn event(&mut self, event: Event<'_>) {
        if self.skip_depth > 0 {
            match event {
                Event::Start(_) => self.skip_depth += 1,
                Event::End(_) => self.skip_depth -= 1,
                _ => {}
            }
            return;
        }
        if let Some((_, alt)) = self.image.as_mut() {
            match event {
                Event::Text(t) | Event::Code(t) | Event::InlineMath(t) => alt.push_str(&t),
                Event::End(TagEnd::Image) => {
                    if let Some((dest, alt)) = self.image.take() {
                        self.image(dest, alt);
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some((code, _)) = self.code.as_mut() {
            match event {
                Event::Text(t) => code.push_str(&t),
                Event::End(TagEnd::CodeBlock) => self.finish_code(),
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                if let Some((_, _, text)) = self.heading.as_mut() {
                    text.push_str(&t);
                }
                self.text_run(&t, self.fmt);
            }
            Event::Code(t) => {
                if let Some((_, _, text)) = self.heading.as_mut() {
                    text.push_str(&t);
                }
                self.text_run(&t, Fmt { code: true, ..self.fmt });
            }
            Event::InlineMath(t) => self.text_run(&t, Fmt { math: true, ..self.fmt }),
            Event::DisplayMath(t) => {
                self.close_par();
                self.open_par(None);
                if let Some(p) = self.par.as_mut() {
                    p.align = Some("center");
                }
                self.text_run(&t, Fmt { math: true, ..Fmt::default() });
                self.close_par();
            }
            Event::Html(t) => {
                let text = strip_tags(&t);
                if !text.trim().is_empty() {
                    self.text_run(text.trim_matches('\n'), self.fmt);
                }
            }
            Event::InlineHtml(t) => {
                let lower = t.to_ascii_lowercase();
                if lower.starts_with("<br") {
                    self.raw_run("<w:r><w:br/></w:r>");
                }
            }
            Event::FootnoteReference(label) => {
                let id = self.footnote_id(&label);
                self.raw_run(&format!(
                    "<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteReference w:id=\"{id}\"/></w:r>"
                ));
            }
            Event::SoftBreak => self.text_run(" ", self.fmt),
            Event::HardBreak => self.raw_run("<w:r><w:br/></w:r>"),
            Event::Rule => {
                self.close_par();
                self.open_par(None);
                if let Some(p) = self.par.as_mut() {
                    p.border_bottom = true;
                }
                self.close_par();
            }
            Event::TaskListMarker(done) => self.text_run(if done { "\u{2612} " } else { "\u{2610} " }, Fmt::default()),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.open_par(None),
            Tag::Heading { level, id, .. } => {
                self.close_par();
                let style = format!("Heading{}", level as u8);
                self.open_par(Some(style));
                self.heading = Some((level, id.map(|s| s.to_string()), String::new()));
            }
            Tag::BlockQuote(kind) => {
                self.close_par();
                self.quote += 1;
                if let Some(kind) = kind {
                    let title = match kind {
                        BlockQuoteKind::Note => "Note",
                        BlockQuoteKind::Tip => "Tip",
                        BlockQuoteKind::Important => "Important",
                        BlockQuoteKind::Warning => "Warning",
                        BlockQuoteKind::Caution => "Caution",
                    };
                    self.open_par(None);
                    self.text_run(title, Fmt { bold: true, ..Fmt::default() });
                    self.close_par();
                }
            }
            Tag::CodeBlock(kind) => {
                self.close_par();
                let is_math = matches!(&kind, CodeBlockKind::Fenced(info) if info.trim().eq_ignore_ascii_case("math"));
                self.code = Some((String::new(), is_math));
            }
            Tag::HtmlBlock => self.close_par(),
            Tag::List(start) => {
                self.close_par();
                self.new_list(start);
            }
            Tag::Item => {
                self.close_par();
                if let Some(l) = self.lists.last_mut() {
                    l.first_par_pending = true;
                }
            }
            Tag::FootnoteDefinition(label) => {
                self.close_par();
                let id = self.footnote_id(&label);
                self.footnote_stack.push(id);
                self.targets.push(String::new());
            }
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => self.close_par(),
            Tag::Table(aligns) => {
                self.close_par();
                let cols = aligns.len().max(1);
                let width = 9000 / cols;
                let mut xml = String::from(
                    "<w:tbl><w:tblPr><w:tblStyle w:val=\"MdTable\"/><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblLook w:val=\"04A0\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"0\" w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"1\"/></w:tblPr><w:tblGrid>",
                );
                for _ in 0..cols {
                    xml.push_str(&format!("<w:gridCol w:w=\"{width}\"/>"));
                }
                xml.push_str("</w:tblGrid>");
                self.out().push_str(&xml);
                self.table_aligns = aligns;
            }
            Tag::TableHead => {
                self.header_row = true;
                self.cell = 0;
                self.out().push_str("<w:tr><w:trPr><w:tblHeader/></w:trPr>");
            }
            Tag::TableRow => {
                self.cell = 0;
                self.out().push_str("<w:tr>");
            }
            Tag::TableCell => {
                let shade = if self.header_row { "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F2\"/>" } else { "" };
                self.out().push_str(&format!("<w:tc><w:tcPr><w:tcW w:w=\"0\" w:type=\"auto\"/>{shade}</w:tcPr>"));
                self.in_cell = true;
                if self.header_row {
                    self.push_fmt(|f| f.bold = true);
                }
            }
            Tag::Emphasis => self.push_fmt(|f| f.italic = true),
            Tag::Strong => self.push_fmt(|f| f.bold = true),
            Tag::Strikethrough => self.push_fmt(|f| f.strike = true),
            Tag::Superscript => self.push_fmt(|f| f.sup = true),
            Tag::Subscript => self.push_fmt(|f| f.sub = true),
            Tag::Link { link_type, dest_url, .. } => {
                self.ensure_par();
                match self.link_target(&dest_url, link_type == LinkType::Email) {
                    Some((true, anchor)) => self.raw_run(&format!("<w:hyperlink w:anchor=\"{}\" w:history=\"1\">", esc(&anchor))),
                    Some((false, url)) => {
                        let rel = self.add_rel(REL_HYPERLINK, url, true);
                        self.raw_run(&format!("<w:hyperlink r:id=\"{rel}\" w:history=\"1\">"));
                    }
                    None => self.raw_run("<w:hyperlink w:history=\"1\">"),
                }
                self.push_fmt(|f| f.link = true);
            }
            Tag::Image { dest_url, .. } => {
                self.image = Some((dest_url.to_string(), String::new()));
            }
            Tag::MetadataBlock(_) => self.skip_depth = 1,
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.close_par(),
            TagEnd::Heading(_) => {
                if let Some((_, id, text)) = self.heading.take() {
                    match id {
                        Some(id) => {
                            self.slugger.reserve(&id);
                            if let Some(p) = self.par.take() {
                                let mut runs = String::new();
                                self.push_bookmarked(&mut runs, &id, &p.runs);
                                self.par = Some(Para { runs, ..p });
                            }
                        }
                        None => {
                            if let Some(p) = self.par.as_mut() {
                                p.bookmark_text = Some(text);
                            }
                        }
                    }
                }
                self.close_par();
            }
            TagEnd::BlockQuote(_) => {
                self.close_par();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::CodeBlock => self.finish_code(),
            TagEnd::HtmlBlock => self.close_par(),
            TagEnd::List(_) => {
                self.close_par();
                self.lists.pop();
            }
            TagEnd::Item => self.close_par(),
            TagEnd::FootnoteDefinition => {
                self.close_par();
                let body = self.targets.pop().unwrap_or_default();
                if let Some(id) = self.footnote_stack.pop() {
                    self.footnotes.push((id, body));
                }
            }
            TagEnd::DefinitionList | TagEnd::DefinitionListTitle | TagEnd::DefinitionListDefinition => self.close_par(),
            TagEnd::Table => {
                self.close_par();
                self.out().push_str("</w:tbl>");
                self.out().push_str("<w:p><w:pPr><w:spacing w:after=\"0\"/></w:pPr></w:p>");
            }
            TagEnd::TableHead => {
                self.header_row = false;
                self.out().push_str("</w:tr>");
            }
            TagEnd::TableRow => self.out().push_str("</w:tr>"),
            TagEnd::TableCell => {
                if self.par.is_none() {
                    self.open_par(None);
                }
                self.close_par();
                self.out().push_str("</w:tc>");
                self.in_cell = false;
                self.cell += 1;
                if self.header_row {
                    self.pop_fmt();
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Superscript | TagEnd::Subscript => self.pop_fmt(),
            TagEnd::Link => {
                self.pop_fmt();
                self.raw_run("</w:hyperlink>");
            }
            TagEnd::Image => {}
            TagEnd::MetadataBlock(_) => {}
        }
    }

    fn finish_code(&mut self) {
        let Some((code, is_math)) = self.code.take() else { return };
        let code = code.strip_suffix('\n').unwrap_or(&code).to_string();
        if is_math {
            self.open_par(None);
            if let Some(p) = self.par.as_mut() {
                p.align = Some("center");
            }
            self.text_run(&code, Fmt { math: true, ..Fmt::default() });
        } else {
            self.open_par(Some("CodeBlock".into()));
            if let Some(p) = self.par.as_mut() {
                p.num = None;
            }
            self.text_run(&code, Fmt::default());
        }
        self.close_par();
    }
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"";

fn styles_xml() -> String {
    let mut headings = String::new();
    let sizes = [40, 32, 28, 26, 24, 22];
    for (i, sz) in sizes.iter().enumerate() {
        let n = i + 1;
        headings.push_str(&format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{n}\"><w:name w:val=\"heading {n}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"{}\" w:after=\"120\"/><w:outlineLvl w:val=\"{i}\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"1F2328\"/><w:sz w:val=\"{sz}\"/><w:szCs w:val=\"{sz}\"/></w:rPr></w:style>",
            if n <= 2 { 360 } else { 240 }
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>{headings}<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:pBdr><w:left w:val="single" w:sz="18" w:space="8" w:color="D0D7DE"/></w:pBdr><w:ind w:left="360"/></w:pPr><w:rPr><w:color w:val="59636E"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="60"/><w:contextualSpacing/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="CodeBlock"><w:name w:val="Code Block"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:pBdr><w:top w:val="single" w:sz="4" w:space="4" w:color="E1E4E8"/><w:left w:val="single" w:sz="4" w:space="4" w:color="E1E4E8"/><w:bottom w:val="single" w:sz="4" w:space="4" w:color="E1E4E8"/><w:right w:val="single" w:sz="4" w:space="4" w:color="E1E4E8"/></w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="F6F8FA"/><w:spacing w:after="160" w:line="240" w:lineRule="auto"/><w:ind w:left="113" w:right="113"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:sz w:val="19"/><w:szCs w:val="19"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="TableText"><w:name w:val="Table Text"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="40" w:after="40" w:line="240" w:lineRule="auto"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="FootnoteText"><w:name w:val="footnote text"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr></w:style><w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont"><w:name w:val="Default Paragraph Font"/><w:uiPriority w:val="1"/><w:semiHidden/></w:style><w:style w:type="character" w:styleId="InlineCode"><w:name w:val="Inline Code"/><w:basedOn w:val="DefaultParagraphFont"/><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:color w:val="C7254E"/><w:sz w:val="20"/><w:shd w:val="clear" w:color="auto" w:fill="F3F4F6"/></w:rPr></w:style><w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:basedOn w:val="DefaultParagraphFont"/><w:rPr><w:color w:val="0969DA"/><w:u w:val="single"/></w:rPr></w:style><w:style w:type="character" w:styleId="FootnoteReference"><w:name w:val="footnote reference"/><w:basedOn w:val="DefaultParagraphFont"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style><w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:semiHidden/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style><w:style w:type="table" w:styleId="MdTable"><w:name w:val="Markdown Table"/><w:basedOn w:val="TableNormal"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="D0D7DE"/><w:left w:val="single" w:sz="4" w:space="0" w:color="D0D7DE"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="D0D7DE"/><w:right w:val="single" w:sz="4" w:space="0" w:color="D0D7DE"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="D0D7DE"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="D0D7DE"/></w:tblBorders></w:tblPr></w:style></w:styles>"#
    )
}

fn numbering_xml(nums: &[(u32, bool, u64)]) -> String {
    let bullets = ["\u{2022}", "\u{25E6}", "\u{25AA}"];
    let mut abs = String::new();
    for (abs_id, ordered) in [(0u32, false), (1u32, true)] {
        abs.push_str(&format!("<w:abstractNum w:abstractNumId=\"{abs_id}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>"));
        for lvl in 0..9u32 {
            let left = LIST_INDENT * (lvl + 1);
            if ordered {
                abs.push_str(&format!(
                    "<w:lvl w:ilvl=\"{lvl}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%{}.\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{left}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
                    lvl + 1
                ));
            } else {
                abs.push_str(&format!(
                    "<w:lvl w:ilvl=\"{lvl}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{left}\" w:hanging=\"360\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\"/></w:rPr></w:lvl>",
                    bullets[lvl as usize % bullets.len()]
                ));
            }
        }
        abs.push_str("</w:abstractNum>");
    }
    let mut num = String::new();
    for (i, (lvl, ordered, start)) in nums.iter().enumerate() {
        let id = i + 1;
        num.push_str(&format!(
            "<w:num w:numId=\"{id}\"><w:abstractNumId w:val=\"{}\"/><w:lvlOverride w:ilvl=\"{lvl}\"><w:startOverride w:val=\"{start}\"/></w:lvlOverride></w:num>",
            u32::from(*ordered)
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{abs}{num}</w:numbering>"
    )
}

/// Builds the .docx package in memory.
pub fn build(markdown: &str, base: Option<&Path>, math: bool, title: &str, fetch: Fetch<'_>) -> std::io::Result<Vec<u8>> {
    let mut doc = Doc::new(base, fetch);
    for event in Parser::new_ext(markdown, parser_options(math)) {
        doc.event(event);
    }
    doc.close_par();
    while doc.targets.len() > 1 {
        let body = doc.targets.pop().unwrap_or_default();
        if let Some(id) = doc.footnote_stack.pop() {
            doc.footnotes.push((id, body));
        }
    }
    let body = doc.targets.pop().unwrap_or_default();

    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {NS}><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );

    let mut footnotes = String::from(
        "<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>",
    );
    let mut defined: Vec<u32> = Vec::new();
    for (id, body) in &doc.footnotes {
        if defined.contains(id) {
            continue;
        }
        defined.push(*id);
        let body = if body.is_empty() { "<w:p/>".to_string() } else { body.clone() };
        footnotes.push_str(&format!("<w:footnote w:id=\"{id}\">{body}</w:footnote>"));
    }
    for id in doc.footnote_ids.values() {
        if !defined.contains(id) {
            footnotes.push_str(&format!("<w:footnote w:id=\"{id}\"><w:p><w:pPr><w:pStyle w:val=\"FootnoteText\"/></w:pPr><w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteRef/></w:r></w:p></w:footnote>"));
        }
    }
    let footnotes = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:footnotes {NS}>{footnotes}</w:footnotes>");

    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/><Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/>",
    );
    for (id, kind, target, external) in &doc.rels {
        let mode = if *external { " TargetMode=\"External\"" } else { "" };
        rels.push_str(&format!("<Relationship Id=\"{id}\" Type=\"{kind}\" Target=\"{}\"{mode}/>", esc(target)));
    }
    rels.push_str("</Relationships>");

    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/><Default Extension=\"gif\" ContentType=\"image/gif\"/><Default Extension=\"bmp\" ContentType=\"image/bmp\"/><Default Extension=\"tiff\" ContentType=\"image/tiff\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/><Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/><Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>";
    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>";
    let settings = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:defaultTabStop w:val=\"720\"/><w:characterSpacingControl w:val=\"doNotCompress\"/><w:footnotePr><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr><w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat></w:settings>";
    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title></cp:coreProperties>",
        esc(title)
    );
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>MahiMarkD++</Application></Properties>";

    let mut zip = ZipWriter::default();
    zip.add("[Content_Types].xml", content_types.as_bytes(), true)?;
    zip.add("_rels/.rels", root_rels.as_bytes(), true)?;
    zip.add("docProps/core.xml", core.as_bytes(), true)?;
    zip.add("docProps/app.xml", app.as_bytes(), true)?;
    zip.add("word/document.xml", document.as_bytes(), true)?;
    zip.add("word/styles.xml", styles_xml().as_bytes(), true)?;
    zip.add("word/numbering.xml", numbering_xml(&doc.nums).as_bytes(), true)?;
    zip.add("word/settings.xml", settings.as_bytes(), true)?;
    zip.add("word/footnotes.xml", footnotes.as_bytes(), true)?;
    zip.add("word/_rels/document.xml.rels", rels.as_bytes(), true)?;
    for (i, img) in doc.images.iter().enumerate() {
        zip.add(&format!("word/media/image{}.{}", i + 1, img.ext), &img.data, false)?;
    }
    Ok(zip.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_fetch(_: &str) -> Option<Vec<u8>> {
        None
    }

    fn document_xml(md: &str) -> String {
        let mut doc = Doc::new(None, &no_fetch);
        for e in Parser::new_ext(md, parser_options(true)) {
            doc.event(e);
        }
        doc.close_par();
        doc.targets.swap_remove(0)
    }

    #[test]
    fn headings_get_styles_and_bookmarks() {
        let x = document_xml("# Hello World\n\ntext\n");
        assert!(x.contains("<w:pStyle w:val=\"Heading1\"/>"));
        assert!(x.contains("w:name=\"_hello_world\""));
        assert!(x.contains("<w:t xml:space=\"preserve\">text</w:t>"));
    }

    #[test]
    fn inline_formatting() {
        let x = document_xml("**b** *i* ~~s~~ `c` <x>&\n");
        assert!(x.contains("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">b</w:t>"));
        assert!(x.contains("<w:rPr><w:i/></w:rPr>"));
        assert!(x.contains("<w:rPr><w:strike/></w:rPr>"));
        assert!(x.contains("<w:rStyle w:val=\"InlineCode\"/>"));
        assert!(x.contains("&amp;"));
    }

    #[test]
    fn lists_and_code() {
        let mut doc = Doc::new(None, &no_fetch);
        for e in Parser::new_ext("1. one\n2. two\n   - nested\n\n```\na\tb\n```\n", parser_options(true)) {
            doc.event(e);
        }
        doc.close_par();
        let x = doc.targets.swap_remove(0);
        assert_eq!(doc.nums.len(), 2);
        assert_eq!(doc.nums[0], (0, true, 1));
        assert_eq!(doc.nums[1], (1, false, 1));
        assert!(x.contains("<w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"2\"/></w:numPr>"));
        assert!(x.contains("<w:pStyle w:val=\"CodeBlock\"/>"));
        assert!(x.contains("<w:tab/>"));
    }

    #[test]
    fn tables_links_and_tasks() {
        let x = document_xml("| a | b |\n|---|--:|\n| [l](https://e.com) | 2 |\n\n- [x] done\n\n[in](#hello)\n");
        assert!(x.contains("<w:tbl>"));
        assert!(x.contains("<w:tblHeader/>"));
        assert!(x.contains("<w:jc w:val=\"right\"/>"));
        assert!(x.contains("<w:hyperlink r:id=\"rId10\""));
        assert!(x.contains("<w:hyperlink w:anchor=\"_hello\""));
        assert!(x.contains("\u{2612}"));
    }

    #[test]
    fn builds_a_package() {
        let png: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 2, 0, 0, 0, 3, 8, 6, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let fetch = move |url: &str| if url.ends_with(".png") { Some(png.to_vec()) } else { None };
        let bytes = build("# T\n\nText[^1]\n\n[^1]: Note\n\n![p](https://x/a.png)\n", None, true, "T", &fetch).unwrap();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("word/document.xml"));
        assert!(s.contains("word/media/image1.png"));
        assert!(s.contains("word/footnotes.xml"));
    }

    #[test]
    fn base64() {
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
        assert!(decode_base64("a*b").is_none());
    }

    #[test]
    fn bookmark_names() {
        assert_eq!(bookmark_name("intro-1"), "_intro_1");
        assert!(bookmark_name(&"x".repeat(100)).chars().count() <= 40);
    }
}
