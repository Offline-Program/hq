// Copyright (c) 2026 Red Hat, Inc.
// Licensed under the BSD 3-Clause License. See LICENSE file for details.

use crate::css::{self, CssRule};
use crate::progress::Progress;
use crate::Result;
use jwalk::WalkDir;
use lol_html::Selector as LolSelector;
use log::{debug, info, trace, warn};
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Serialize)]
pub struct SelectorResult {
    pub selector: String,
    pub used: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CssFileResult {
    pub path: PathBuf,
    pub selectors: Vec<SelectorResult>,
}

pub struct PrunedFile {
    pub path: PathBuf,
    pub content: String,
}

struct SelectorEntry {
    selector: String,
    compiled: Option<LolSelector>,
    rule: CssRule,
    used: AtomicBool,
}

struct CssFileEntry {
    path: PathBuf,
    rule_range: std::ops::Range<usize>,
}

fn is_css_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("css"))
}

fn is_html_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"))
}

fn walk_files(path: &Path, filter: fn(&Path) -> bool) -> Box<dyn Iterator<Item = PathBuf> + Send> {
    if path.is_file() {
        return Box::new(std::iter::once(path.to_path_buf()));
    }
    Box::new(
        WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(move |e| e.file_type.is_file() && filter(&e.path()))
            .map(|e| e.path()),
    )
}

struct SelectorTable {
    css_files: Vec<CssFileEntry>,
    entries: Vec<SelectorEntry>,
}

impl SelectorTable {
    fn from_css_path(css_path: &Path, progress: Option<&Progress>) -> Result<Self> {
        let mut css_files = Vec::new();
        let mut entries = Vec::new();

        for css_file in walk_files(css_path, is_css_file) {
            let css_text = std::fs::read_to_string(&css_file)?;
            let rules = css::extract_rules(&css_text)?;
            let start = entries.len();
            let rule_count = rules.len();

            debug!("{}: {} style rules", css_file.display(), rule_count);
            let mut rule_bytes = 0u64;
            for rule in rules {
                rule_bytes += (rule.end - rule.start) as u64;
                let compiled = rule.selector.parse::<LolSelector>().ok();
                if compiled.is_none() {
                    trace!("cannot compile selector '{}', will skip matching", rule.selector);
                }
                entries.push(SelectorEntry {
                    selector: rule.selector.clone(),
                    compiled,
                    rule,
                    used: AtomicBool::new(false),
                });
            }

            css_files.push(CssFileEntry {
                path: css_file,
                rule_range: start..entries.len(),
            });

            if let Some(p) = progress {
                p.css_files.fetch_add(1, Ordering::Relaxed);
                p.selectors.fetch_add(rule_count as u64, Ordering::Relaxed);
                p.unused_bytes.fetch_add(rule_bytes, Ordering::Relaxed);
            }
        }

        info!("parsed {} CSS files, {} selectors total", css_files.len(), entries.len());
        Ok(SelectorTable { css_files, entries })
    }

    fn test_html_file(&self, html: &[u8], progress: Option<&Progress>) {
        use lol_html::{HtmlRewriter, Settings};
        use std::borrow::Cow;
        use std::cell::Cell;

        let unsettled: Vec<usize> = self.entries.iter().enumerate()
            .filter(|(_, e)| !e.used.load(Ordering::Relaxed) && e.compiled.is_some())
            .map(|(i, _)| i)
            .collect();

        if unsettled.is_empty() {
            return;
        }

        let matched: Vec<Cell<bool>> = unsettled.iter().map(|_| Cell::new(false)).collect();

        let handlers: Vec<_> = unsettled.iter().zip(matched.iter())
            .map(|(&idx, flag)| {
                let selector = self.entries[idx].compiled.as_ref().unwrap();
                (
                    Cow::Borrowed(selector),
                    lol_html::ElementContentHandlers::default().element(
                        move |_el: &mut lol_html::html_content::Element| {
                            flag.set(true);
                            Ok(())
                        },
                    ),
                )
            })
            .collect();

        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers: handlers,
                ..Settings::new()
            },
            |_chunk: &[u8]| {},
        );

        if rewriter.write(html).is_err() || rewriter.end().is_err() {
            return;
        }

        for (&idx, flag) in unsettled.iter().zip(matched.iter()) {
            if flag.get() {
                let entry = &self.entries[idx];
                if entry.used.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                    if let Some(p) = progress {
                        p.selectors_used.fetch_add(1, Ordering::Relaxed);
                        let span = (entry.rule.end - entry.rule.start) as u64;
                        p.unused_bytes.fetch_sub(span, Ordering::Relaxed);
                    }
                }
                debug!("selector '{}' matched", entry.selector);
            }
        }
    }

    fn stream_html(&self, html_path: &Path, progress: Option<&Progress>) {
        let html_count = std::sync::atomic::AtomicU64::new(0);
        walk_files(html_path, is_html_file)
            .par_bridge()
            .for_each(|html_file| {
                trace!("testing against {}", html_file.display());
                match std::fs::read(&html_file) {
                    Ok(html) => {
                        self.test_html_file(&html, progress);
                        html_count.fetch_add(1, Ordering::Relaxed);
                        if let Some(p) = progress {
                            p.html_files.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) => warn!("{}: {e}", html_file.display()),
                }
            });
        info!("streamed {} HTML files", html_count.load(Ordering::Relaxed));
    }

    fn to_results(&self) -> Vec<CssFileResult> {
        self.css_files.iter().map(|cf| {
            let selectors: Vec<SelectorResult> = self.entries[cf.rule_range.clone()]
                .iter()
                .map(|e| {
                    let used = e.used.load(Ordering::Relaxed);
                    debug!("{}: '{}' -> {}", cf.path.display(), e.selector, if used { "used" } else { "unused" });
                    SelectorResult {
                        selector: e.selector.clone(),
                        used,
                    }
                })
                .collect();
            CssFileResult {
                path: cf.path.clone(),
                selectors,
            }
        }).collect()
    }

    fn into_pruned(self) -> Result<Vec<PrunedFile>> {
        self.css_files.iter().map(|cf| {
            let unused: Vec<CssRule> = self.entries[cf.rule_range.clone()]
                .iter()
                .filter(|e| !e.used.load(Ordering::Relaxed))
                .map(|e| e.rule.clone())
                .collect();

            debug!("{}: pruning {} unused rules", cf.path.display(), unused.len());
            let css_text = std::fs::read_to_string(&cf.path)?;
            let content = css::prune_css(&css_text, &unused);
            Ok(PrunedFile {
                path: cf.path.clone(),
                content,
            })
        }).collect()
    }
}

pub fn check_css(
    css_path: &Path,
    html_path: &Path,
    progress: Option<&Progress>,
) -> Result<Vec<CssFileResult>> {
    let table = SelectorTable::from_css_path(css_path, progress)?;
    table.stream_html(html_path, progress);
    Ok(table.to_results())
}

pub fn check_and_prune(
    css_path: &Path,
    html_path: &Path,
    progress: Option<&Progress>,
) -> Result<(Vec<CssFileResult>, Vec<PrunedFile>)> {
    let table = SelectorTable::from_css_path(css_path, progress)?;
    table.stream_html(html_path, progress);
    let results = table.to_results();
    let pruned = table.into_pruned()?;
    Ok((results, pruned))
}

pub fn css_input_is_single_file(css_path: &Path) -> bool {
    css_path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // --- check_css ---

    #[test]
    fn check_finds_used_and_unused() {
        let dir = tempfile::tempdir().unwrap();
        let css_file = dir.path().join("style.css");
        let html_file = dir.path().join("page.html");
        fs::write(&css_file, ".used { color: red; }\n.unused { color: blue; }").unwrap();
        fs::write(&html_file, r#"<div class="used">hello</div>"#).unwrap();

        let results = check_css(&css_file, &html_file, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].selectors.len(), 2);
        assert_eq!(results[0].selectors[0].selector, ".used");
        assert!(results[0].selectors[0].used);
        assert_eq!(results[0].selectors[1].selector, ".unused");
        assert!(!results[0].selectors[1].used);
    }

    #[test]
    fn check_all_selectors_used() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "div { margin: 0; }\np { padding: 0; }").unwrap();
        fs::write(dir.path().join("p.html"), "<div><p>text</p></div>").unwrap();

        let results = check_css(&dir.path().join("s.css"), &dir.path().join("p.html"), None).unwrap();
        assert!(results[0].selectors.iter().all(|s| s.used));
    }

    #[test]
    fn check_all_selectors_unused() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), ".ghost { x: 1; }\n#phantom { x: 2; }").unwrap();
        fs::write(dir.path().join("p.html"), "<div>no match</div>").unwrap();

        let results = check_css(&dir.path().join("s.css"), &dir.path().join("p.html"), None).unwrap();
        assert!(results[0].selectors.iter().all(|s| !s.used));
    }

    #[test]
    fn check_tag_selector() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "span { color: red; }").unwrap();
        fs::write(dir.path().join("p.html"), "<span>hi</span>").unwrap();

        let results = check_css(&dir.path().join("s.css"), &dir.path().join("p.html"), None).unwrap();
        assert!(results[0].selectors[0].used);
    }

    #[test]
    fn check_id_selector() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "#main { width: 100%; }\n#missing { width: 50%; }").unwrap();
        fs::write(dir.path().join("p.html"), r#"<div id="main">content</div>"#).unwrap();

        let results = check_css(&dir.path().join("s.css"), &dir.path().join("p.html"), None).unwrap();
        assert!(results[0].selectors[0].used);
        assert!(!results[0].selectors[1].used);
    }

    #[test]
    fn check_attribute_selector() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "a[href] { color: blue; }\ninput[type=\"text\"] { border: 1px; }").unwrap();
        fs::write(dir.path().join("p.html"), r#"<a href="/">link</a><input type="checkbox">"#).unwrap();

        let results = check_css(&dir.path().join("s.css"), &dir.path().join("p.html"), None).unwrap();
        assert!(results[0].selectors[0].used);
        assert!(!results[0].selectors[1].used);
    }

    #[test]
    fn check_descendant_selector() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "nav a { text-decoration: none; }").unwrap();
        fs::write(dir.path().join("p.html"), "<nav><a href='/'>home</a></nav>").unwrap();

        let results = check_css(&dir.path().join("s.css"), &dir.path().join("p.html"), None).unwrap();
        assert!(results[0].selectors[0].used);
    }

    #[test]
    fn check_child_combinator() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "ul > li { margin: 0; }").unwrap();
        fs::write(dir.path().join("p.html"), "<ul><li>item</li></ul>").unwrap();

        let results = check_css(&dir.path().join("s.css"), &dir.path().join("p.html"), None).unwrap();
        assert!(results[0].selectors[0].used);
    }

    #[test]
    fn check_selector_used_in_one_html_file_of_many() {
        let dir = tempfile::tempdir().unwrap();
        let html_dir = dir.path().join("html");
        fs::create_dir(&html_dir).unwrap();

        fs::write(dir.path().join("s.css"), ".rare { color: red; }").unwrap();
        fs::write(html_dir.join("a.html"), "<div>no match</div>").unwrap();
        fs::write(html_dir.join("b.html"), "<div>still no match</div>").unwrap();
        fs::write(html_dir.join("c.html"), r#"<span class="rare">found</span>"#).unwrap();

        let results = check_css(&dir.path().join("s.css"), &html_dir, None).unwrap();
        assert!(results[0].selectors[0].used);
    }

    #[test]
    fn check_walks_css_directory() {
        let dir = tempfile::tempdir().unwrap();
        let css_dir = dir.path().join("css");
        let html_dir = dir.path().join("html");
        fs::create_dir(&css_dir).unwrap();
        fs::create_dir(&html_dir).unwrap();

        fs::write(css_dir.join("a.css"), ".foo { color: red; }").unwrap();
        fs::write(css_dir.join("b.css"), ".bar { color: blue; }").unwrap();
        fs::write(html_dir.join("page.html"), r#"<div class="foo">x</div>"#).unwrap();

        let results = check_css(&css_dir, &html_dir, None).unwrap();
        assert_eq!(results.len(), 2);

        let a = results.iter().find(|r| r.path.ends_with("a.css")).unwrap();
        assert!(a.selectors[0].used);

        let b = results.iter().find(|r| r.path.ends_with("b.css")).unwrap();
        assert!(!b.selectors[0].used);
    }

    #[test]
    fn check_ignores_non_css_files_in_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("style.css"), ".x { color: red; }").unwrap();
        fs::write(dir.path().join("notes.txt"), ".y { color: blue; }").unwrap();
        fs::write(dir.path().join("page.html"), r#"<div class="x">x</div>"#).unwrap();

        let results = check_css(dir.path(), dir.path(), None).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].path.ends_with("style.css"));
    }

    #[test]
    fn check_ignores_non_html_files_in_html_directory() {
        let dir = tempfile::tempdir().unwrap();
        let html_dir = dir.path().join("html");
        fs::create_dir(&html_dir).unwrap();

        fs::write(dir.path().join("s.css"), ".x { color: red; }").unwrap();
        fs::write(html_dir.join("page.html"), r#"<div class="x">x</div>"#).unwrap();
        fs::write(html_dir.join("data.json"), r#"{"class": "x"}"#).unwrap();

        let results = check_css(&dir.path().join("s.css"), &html_dir, None).unwrap();
        assert!(results[0].selectors[0].used);
    }

    // --- prune ---

    #[test]
    fn prune_removes_unused_preserves_at_rules() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("s.css"),
            ".used { color: red; }\n.unused { color: blue; }\n@media screen { .m { display: none; } }\n",
        ).unwrap();
        fs::write(dir.path().join("p.html"), r#"<div class="used">hello</div>"#).unwrap();

        let (_, pruned) = check_and_prune(&dir.path().join("s.css"), &dir.path().join("p.html"), None).unwrap();
        assert_eq!(pruned.len(), 1);
        assert!(pruned[0].content.contains(".used"));
        assert!(!pruned[0].content.contains(".unused"));
        assert!(pruned[0].content.contains("@media"));
    }

    #[test]
    fn prune_all_used_returns_full_content() {
        let dir = tempfile::tempdir().unwrap();
        let css = "div { margin: 0; }\np { padding: 0; }\n";
        fs::write(dir.path().join("s.css"), css).unwrap();
        fs::write(dir.path().join("p.html"), "<div><p>text</p></div>").unwrap();

        let (_, pruned) = check_and_prune(&dir.path().join("s.css"), &dir.path().join("p.html"), None).unwrap();
        assert_eq!(pruned[0].content, css);
    }

    #[test]
    fn prune_all_unused_removes_all_style_rules() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("s.css"),
            "@keyframes x { to { opacity: 1; } }\n.a { x: 1; }\n.b { x: 2; }\n",
        ).unwrap();
        fs::write(dir.path().join("p.html"), "<div>nothing matches</div>").unwrap();

        let (_, pruned) = check_and_prune(&dir.path().join("s.css"), &dir.path().join("p.html"), None).unwrap();
        assert!(!pruned[0].content.contains(".a"));
        assert!(!pruned[0].content.contains(".b"));
        assert!(pruned[0].content.contains("@keyframes"));
    }

    #[test]
    fn prune_multiple_css_files() {
        let dir = tempfile::tempdir().unwrap();
        let css_dir = dir.path().join("css");
        fs::create_dir(&css_dir).unwrap();

        fs::write(css_dir.join("a.css"), ".used { x: 1; }\n.dead { x: 2; }").unwrap();
        fs::write(css_dir.join("b.css"), ".also-dead { x: 3; }").unwrap();
        fs::write(dir.path().join("p.html"), r#"<div class="used">x</div>"#).unwrap();

        let (_, pruned) = check_and_prune(&css_dir, &dir.path().join("p.html"), None).unwrap();
        assert_eq!(pruned.len(), 2);

        let a = pruned.iter().find(|p| p.path.ends_with("a.css")).unwrap();
        assert!(a.content.contains(".used"));
        assert!(!a.content.contains(".dead"));

        let b = pruned.iter().find(|p| p.path.ends_with("b.css")).unwrap();
        assert!(!b.content.contains(".also-dead"));
    }

    #[test]
    fn prune_with_multiple_html_files() {
        let dir = tempfile::tempdir().unwrap();
        let html_dir = dir.path().join("html");
        fs::create_dir(&html_dir).unwrap();

        fs::write(
            dir.path().join("s.css"),
            ".in-a { x: 1; }\n.in-b { x: 2; }\n.nowhere { x: 3; }",
        ).unwrap();
        fs::write(html_dir.join("a.html"), r#"<div class="in-a">a</div>"#).unwrap();
        fs::write(html_dir.join("b.html"), r#"<div class="in-b">b</div>"#).unwrap();

        let (_, pruned) = check_and_prune(&dir.path().join("s.css"), &html_dir, None).unwrap();
        assert!(pruned[0].content.contains(".in-a"));
        assert!(pruned[0].content.contains(".in-b"));
        assert!(!pruned[0].content.contains(".nowhere"));
    }

    // --- css_input_is_single_file ---

    #[test]
    fn single_file_detected() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("style.css");
        fs::write(&f, ".x { }").unwrap();
        assert!(css_input_is_single_file(&f));
    }

    #[test]
    fn directory_is_not_single_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!css_input_is_single_file(dir.path()));
    }
}
