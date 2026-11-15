//! GitHub-style heading anchors.

use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct Slugger {
    seen: HashMap<String, usize>,
}

impl Slugger {
    pub fn slug(&mut self, text: &str) -> String {
        let base = slugify(text);
        let base = if base.is_empty() { "section".to_string() } else { base };
        let Some(&start) = self.seen.get(&base) else {
            self.seen.insert(base.clone(), 0);
            return base;
        };
        let mut n = start;
        let candidate = loop {
            n += 1;
            let candidate = format!("{base}-{n}");
            if !self.seen.contains_key(&candidate) {
                break candidate;
            }
        };
        self.seen.insert(base, n);
        self.seen.insert(candidate.clone(), 0);
        candidate
    }

    /// Registers an explicit id so generated ones do not collide with it.
    pub fn reserve(&mut self, id: &str) {
        self.seen.entry(id.to_string()).or_insert(0);
    }
}

pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.trim().chars() {
        if c.is_alphanumeric() || c == '_' || c == '-' {
            out.extend(c.to_lowercase());
        } else if c.is_whitespace() {
            out.push('-');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_style() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("Привет мир"), "привет-мир");
        assert_eq!(slugify("C++ & Rust 2024"), "c--rust-2024");
        assert_eq!(slugify("snake_case-name"), "snake_case-name");
    }

    #[test]
    fn duplicates_get_suffixes() {
        let mut s = Slugger::default();
        assert_eq!(s.slug("Intro"), "intro");
        assert_eq!(s.slug("Intro"), "intro-1");
        assert_eq!(s.slug("Intro"), "intro-2");
        assert_eq!(s.slug("Intro 1"), "intro-1-1");
        assert_eq!(s.slug("!!!"), "section");
    }
}
