// Copyright (c) 2026 Red Hat, Inc.
// Licensed under the BSD 3-Clause License. See LICENSE file for details.

use crate::css::{self, CssRule};
use crate::{Result, SelectorEngine};
use jwalk::WalkDir;
use serde::Serialize;
use std::path::{Path, PathBuf};

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

fn is_css_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("css"))
}

fn is_html_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"))
}

fn collect_files(path: &Path, filter: fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type.is_file() && filter(&e.path()))
        .map(|e| e.path())
        .collect();
    files.sort();
    Ok(files)
}

fn read_html_files(html_path: &Path) -> Result<Vec<Vec<u8>>> {
    let html_files = collect_files(html_path, is_html_file)?;
    let mut contents = Vec::with_capacity(html_files.len());
    for f in &html_files {
        contents.push(std::fs::read(f)?);
    }
    Ok(contents)
}

fn selector_is_used(engine: &dyn SelectorEngine, selector: &str, html_contents: &[Vec<u8>]) -> bool {
    for html in html_contents {
        match engine.count_matches(selector, html) {
            Ok(n) if n > 0 => return true,
            _ => {}
        }
    }
    false
}

pub fn check_css(
    engine: &dyn SelectorEngine,
    css_path: &Path,
    html_path: &Path,
) -> Result<Vec<CssFileResult>> {
    let css_files = collect_files(css_path, is_css_file)?;
    let html_contents = read_html_files(html_path)?;

    let mut results = Vec::with_capacity(css_files.len());
    for css_file in &css_files {
        let css_text = std::fs::read_to_string(css_file)?;
        let rules = css::extract_rules(&css_text)?;

        let selectors: Vec<SelectorResult> = rules
            .iter()
            .map(|rule| SelectorResult {
                selector: rule.selector.clone(),
                used: selector_is_used(engine, &rule.selector, &html_contents),
            })
            .collect();

        results.push(CssFileResult {
            path: css_file.clone(),
            selectors,
        });
    }

    Ok(results)
}

pub struct PrunedFile {
    pub path: PathBuf,
    pub content: String,
}

pub fn prune(
    engine: &dyn SelectorEngine,
    css_path: &Path,
    html_path: &Path,
) -> Result<Vec<PrunedFile>> {
    let css_files = collect_files(css_path, is_css_file)?;
    let html_contents = read_html_files(html_path)?;

    let mut pruned = Vec::with_capacity(css_files.len());
    for css_file in &css_files {
        let css_text = std::fs::read_to_string(css_file)?;
        let rules = css::extract_rules(&css_text)?;

        let unused: Vec<CssRule> = rules
            .into_iter()
            .filter(|rule| !selector_is_used(engine, &rule.selector, &html_contents))
            .collect();

        let content = css::prune_css(&css_text, &unused);
        pruned.push(PrunedFile {
            path: css_file.clone(),
            content,
        });
    }

    Ok(pruned)
}

pub fn css_input_is_single_file(css_path: &Path) -> bool {
    css_path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LolHtmlEngine;
    use std::fs;

    fn engine() -> LolHtmlEngine {
        LolHtmlEngine
    }

    // --- check_css ---

    #[test]
    fn check_finds_used_and_unused() {
        let dir = tempfile::tempdir().unwrap();
        let css_file = dir.path().join("style.css");
        let html_file = dir.path().join("page.html");
        fs::write(&css_file, ".used { color: red; }\n.unused { color: blue; }").unwrap();
        fs::write(&html_file, r#"<div class="used">hello</div>"#).unwrap();

        let results = check_css(&engine(), &css_file, &html_file).unwrap();
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

        let results = check_css(&engine(), &dir.path().join("s.css"), &dir.path().join("p.html")).unwrap();
        assert!(results[0].selectors.iter().all(|s| s.used));
    }

    #[test]
    fn check_all_selectors_unused() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), ".ghost { x: 1; }\n#phantom { x: 2; }").unwrap();
        fs::write(dir.path().join("p.html"), "<div>no match</div>").unwrap();

        let results = check_css(&engine(), &dir.path().join("s.css"), &dir.path().join("p.html")).unwrap();
        assert!(results[0].selectors.iter().all(|s| !s.used));
    }

    #[test]
    fn check_tag_selector() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "span { color: red; }").unwrap();
        fs::write(dir.path().join("p.html"), "<span>hi</span>").unwrap();

        let results = check_css(&engine(), &dir.path().join("s.css"), &dir.path().join("p.html")).unwrap();
        assert!(results[0].selectors[0].used);
    }

    #[test]
    fn check_id_selector() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "#main { width: 100%; }\n#missing { width: 50%; }").unwrap();
        fs::write(dir.path().join("p.html"), r#"<div id="main">content</div>"#).unwrap();

        let results = check_css(&engine(), &dir.path().join("s.css"), &dir.path().join("p.html")).unwrap();
        assert!(results[0].selectors[0].used);
        assert!(!results[0].selectors[1].used);
    }

    #[test]
    fn check_attribute_selector() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "a[href] { color: blue; }\ninput[type=\"text\"] { border: 1px; }").unwrap();
        fs::write(dir.path().join("p.html"), r#"<a href="/">link</a><input type="checkbox">"#).unwrap();

        let results = check_css(&engine(), &dir.path().join("s.css"), &dir.path().join("p.html")).unwrap();
        assert!(results[0].selectors[0].used);
        assert!(!results[0].selectors[1].used);
    }

    #[test]
    fn check_descendant_selector() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "nav a { text-decoration: none; }").unwrap();
        fs::write(dir.path().join("p.html"), "<nav><a href='/'>home</a></nav>").unwrap();

        let results = check_css(&engine(), &dir.path().join("s.css"), &dir.path().join("p.html")).unwrap();
        assert!(results[0].selectors[0].used);
    }

    #[test]
    fn check_child_combinator() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("s.css"), "ul > li { margin: 0; }").unwrap();
        fs::write(dir.path().join("p.html"), "<ul><li>item</li></ul>").unwrap();

        let results = check_css(&engine(), &dir.path().join("s.css"), &dir.path().join("p.html")).unwrap();
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

        let results = check_css(&engine(), &dir.path().join("s.css"), &html_dir).unwrap();
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

        let results = check_css(&engine(), &css_dir, &html_dir).unwrap();
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

        let results = check_css(&engine(), dir.path(), dir.path()).unwrap();
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

        let results = check_css(&engine(), &dir.path().join("s.css"), &html_dir).unwrap();
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

        let pruned = prune(&engine(), &dir.path().join("s.css"), &dir.path().join("p.html")).unwrap();
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

        let pruned = prune(&engine(), &dir.path().join("s.css"), &dir.path().join("p.html")).unwrap();
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

        let pruned = prune(&engine(), &dir.path().join("s.css"), &dir.path().join("p.html")).unwrap();
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

        let pruned = prune(&engine(), &css_dir, &dir.path().join("p.html")).unwrap();
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

        let pruned = prune(&engine(), &dir.path().join("s.css"), &html_dir).unwrap();
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
