// Copyright (c) 2026 Red Hat, Inc.
// Licensed under the BSD 3-Clause License. See LICENSE file for details.

use crate::{FileResult, Result, SelectorEngine};
use jwalk::WalkDir;
use rayon::prelude::*;
use std::path::Path;

pub fn count_matches_in_file(
    engine: &dyn SelectorEngine,
    path: &Path,
    selector: &str,
) -> Result<FileResult> {
    let html = std::fs::read(path)?;
    let matches = engine.count_matches(selector, &html)?;
    Ok(FileResult {
        path: path.to_path_buf(),
        matches,
    })
}

fn is_html_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"))
}

pub fn scan(
    engine: &dyn SelectorEngine,
    path: &Path,
    selector: &str,
) -> Result<Vec<FileResult>> {
    if path.is_file() {
        return Ok(vec![count_matches_in_file(engine, path, selector)?]);
    }

    let results: Vec<FileResult> = WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type.is_file() && is_html_file(&e.path()))
        .par_bridge()
        .filter_map(|e| {
            let file = e.path();
            match count_matches_in_file(engine, &file, selector) {
                Ok(result) => Some(result),
                Err(err) => {
                    eprintln!("hq: {}: {err}", file.display());
                    None
                }
            }
        })
        .collect();

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LolHtmlEngine;
    use std::fs;

    #[test]
    fn scan_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.html");
        fs::write(&file, "<div>a</div><div>b</div>").unwrap();

        let engine = LolHtmlEngine;
        let results = scan(&engine, &file, "div").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matches, 2);
    }

    #[test]
    fn scan_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.html"), "<div>1</div>").unwrap();
        fs::write(dir.path().join("b.htm"), "<div>2</div><div>3</div>").unwrap();
        fs::write(dir.path().join("c.txt"), "<div>ignored</div>").unwrap();

        let engine = LolHtmlEngine;
        let mut results = scan(&engine, dir.path(), "div").unwrap();
        results.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn scan_directory_includes_zero_matches() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.html"), "<div>has div</div>").unwrap();
        fs::write(dir.path().join("b.html"), "<p>no div here</p>").unwrap();

        let engine = LolHtmlEngine;
        let results = scan(&engine, dir.path(), "div").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|r| r.matches == 0));
    }
}
