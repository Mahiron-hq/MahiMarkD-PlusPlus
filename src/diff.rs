//! Incremental updates for the preview page.
//!
//! The page keeps one wrapper element per block. A patch replaces only the
//! changed run of blocks and shifts line numbers of the kept ones.

use serde::Serialize;

use crate::render::{Rendered, TocEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockMeta {
    pub hash: u64,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Default)]
pub struct PageState {
    pub blocks: Vec<BlockMeta>,
    pub toc: Vec<TocEntry>,
    pub has_math: bool,
}

#[derive(Serialize)]
struct Full<'a> {
    t: &'static str,
    html: String,
    s: Vec<u32>,
    e: Vec<u32>,
    toc: &'a [TocEntry],
    math: bool,
}

#[derive(Serialize)]
struct Patch<'a> {
    t: &'static str,
    p: usize,
    r: usize,
    html: String,
    s: Vec<u32>,
    e: Vec<u32>,
    shift: Vec<[i64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    toc: Option<&'a [TocEntry]>,
    math: bool,
}

fn wrap(out: &mut String, html: &str) {
    out.push_str("<div class=\"b\">");
    out.push_str(html);
    out.push_str("</div>");
}

/// Builds the message for the page and the new page state. `None` means nothing changed.
pub fn build(prev: Option<&PageState>, doc: &Rendered) -> (Option<String>, PageState) {
    let metas: Vec<BlockMeta> = doc.blocks.iter().map(|b| BlockMeta { hash: b.hash, start: b.start, end: b.end }).collect();
    let state = PageState { blocks: metas, toc: doc.toc.clone(), has_math: doc.has_math };

    let Some(prev) = prev else {
        let mut html = String::with_capacity(doc.blocks.iter().map(|b| b.html.len() + 24).sum());
        for b in &doc.blocks {
            wrap(&mut html, &b.html);
        }
        let msg = Full {
            t: "full",
            html,
            s: doc.blocks.iter().map(|b| b.start).collect(),
            e: doc.blocks.iter().map(|b| b.end).collect(),
            toc: &doc.toc,
            math: doc.has_math,
        };
        return (serde_json::to_string(&msg).ok(), state);
    };

    let old = &prev.blocks;
    let new = &state.blocks;
    let prefix = old.iter().zip(new.iter()).take_while(|(a, b)| a.hash == b.hash).count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old.iter().rev().zip(new.iter().rev()).take(max_suffix).take_while(|(a, b)| a.hash == b.hash).count();

    let removed = old.len() - prefix - suffix;
    let inserted = new.len() - prefix - suffix;

    let mut shift: Vec<[i64; 3]> = Vec::new();
    let mut last: Option<(i64, i64)> = None;
    let kept = (0..prefix).map(|i| (i, i)).chain((0..suffix).map(|k| (new.len() - suffix + k, old.len() - suffix + k)));
    for (ni, oi) in kept {
        let ds = new[ni].start as i64 - old[oi].start as i64;
        let de = new[ni].end as i64 - old[oi].end as i64;
        if last != Some((ds, de)) {
            shift.push([ni as i64, ds, de]);
            last = Some((ds, de));
        }
    }
    if shift.iter().all(|s| s[1] == 0 && s[2] == 0) {
        shift.clear();
    }

    let toc_changed = prev.toc != state.toc;
    if removed == 0 && inserted == 0 && shift.is_empty() && !toc_changed && prev.has_math == state.has_math {
        return (None, state);
    }

    let mid = &doc.blocks[prefix..prefix + inserted];
    let mut html = String::with_capacity(mid.iter().map(|b| b.html.len() + 24).sum());
    for b in mid {
        wrap(&mut html, &b.html);
    }
    let msg = Patch {
        t: "patch",
        p: prefix,
        r: removed,
        html,
        s: mid.iter().map(|b| b.start).collect(),
        e: mid.iter().map(|b| b.end).collect(),
        shift,
        toc: toc_changed.then_some(state.toc.as_slice()),
        math: state.has_math,
    };
    (serde_json::to_string(&msg).ok(), state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Renderer;
    use serde_json::Value;

    fn doc(md: &str) -> Rendered {
        Renderer::new(None).render(md, true)
    }

    fn apply(prev_lines: &[(u32, u32)], msg: &Value) -> Vec<(u32, u32)> {
        let p = msg["p"].as_u64().unwrap() as usize;
        let r = msg["r"].as_u64().unwrap() as usize;
        let s: Vec<u32> = serde_json::from_value(msg["s"].clone()).unwrap();
        let e: Vec<u32> = serde_json::from_value(msg["e"].clone()).unwrap();
        let mut lines = prev_lines.to_vec();
        lines.splice(p..p + r, s.iter().copied().zip(e.iter().copied()));
        let shift: Vec<[i64; 3]> = serde_json::from_value(msg["shift"].clone()).unwrap();
        let mid = p..p + s.len();
        for (k, run) in shift.iter().enumerate() {
            let from = run[0] as usize;
            let to = shift.get(k + 1).map(|n| n[0] as usize).unwrap_or(lines.len());
            for (i, l) in lines.iter_mut().enumerate().take(to).skip(from) {
                if !mid.contains(&i) {
                    l.0 = (l.0 as i64 + run[1]) as u32;
                    l.1 = (l.1 as i64 + run[2]) as u32;
                }
            }
        }
        lines
    }

    fn check(before: &str, after: &str) -> Value {
        let a = doc(before);
        let (_, state) = build(None, &a);
        let b = doc(after);
        let (msg, _) = build(Some(&state), &b);
        let msg: Value = serde_json::from_str(&msg.expect("change expected")).unwrap();
        let old_lines: Vec<_> = a.blocks.iter().map(|x| (x.start, x.end)).collect();
        let want: Vec<_> = b.blocks.iter().map(|x| (x.start, x.end)).collect();
        assert_eq!(apply(&old_lines, &msg), want, "{msg}");
        msg
    }

    #[test]
    fn full_message_has_all_blocks() {
        let (msg, state) = build(None, &doc("# A\n\ntext\n"));
        let v: Value = serde_json::from_str(&msg.unwrap()).unwrap();
        assert_eq!(v["t"], "full");
        assert_eq!(v["s"], serde_json::json!([0, 2]));
        assert_eq!(state.blocks.len(), 2);
    }

    #[test]
    fn edit_in_middle_replaces_one_block() {
        let msg = check("a\n\nb\n\nc\n", "a\n\nbb\n\nc\n");
        assert_eq!(msg["p"], 1);
        assert_eq!(msg["r"], 1);
        assert_eq!(msg["s"], serde_json::json!([2]));
    }

    #[test]
    fn inserted_lines_shift_following_blocks() {
        let msg = check("a\n\nb\n\nc\n\nd\n", "a\n\nnew\n\n\nb\n\nc\n\nd\n");
        assert_eq!(msg["r"], 0);
        assert!(!msg["shift"].as_array().unwrap().is_empty());
    }

    #[test]
    fn blank_line_only_change_is_a_pure_shift() {
        let msg = check("a\n\nb\n\nc\n", "a\n\n\n\nb\n\nc\n");
        assert_eq!(msg["r"], 0);
        assert_eq!(msg["s"], serde_json::json!([]));
    }

    #[test]
    fn deletion_and_toc_change() {
        let msg = check("# A\n\n# B\n\ntext\n", "# A\n\ntext\n");
        assert_eq!(msg["r"], 1);
        assert_eq!(msg["toc"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn identical_doc_sends_nothing() {
        let a = doc("x\n\ny\n");
        let (_, state) = build(None, &a);
        assert!(build(Some(&state), &doc("x\n\ny\n")).0.is_none());
    }

    #[test]
    fn repeated_blocks_are_matched() {
        check("x\n\nx\n\nx\n", "x\n\nx\n\ny\n\nx\n\nx\n");
        check("x\n\ny\n\nx\n", "x\n\nx\n");
    }
}
