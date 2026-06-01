// Copyright (c) 2026 Red Hat, Inc.
// Licensed under the BSD 3-Clause License. See LICENSE file for details.

use crate::Result;

pub trait SelectorEngine: Send + Sync {
    fn count_matches(&self, selector: &str, html: &[u8]) -> Result<usize>;
}

pub struct LolHtmlEngine;

impl SelectorEngine for LolHtmlEngine {
    fn count_matches(&self, selector: &str, html: &[u8]) -> Result<usize> {
        use lol_html::{HtmlRewriter, Selector, Settings, element};
        use std::cell::Cell;

        let _: Selector = selector
            .parse()
            .map_err(|e: lol_html::errors::SelectorError| {
                crate::Error::Selector(e.to_string())
            })?;

        let count = Cell::new(0usize);

        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers: vec![element!(selector, |_el| {
                    count.set(count.get() + 1);
                    Ok(())
                })],
                ..Settings::new()
            },
            |_chunk: &[u8]| {},
        );

        rewriter
            .write(html)
            .map_err(|e| crate::Error::Selector(e.to_string()))?;

        rewriter
            .end()
            .map_err(|e| crate::Error::Selector(e.to_string()))?;

        Ok(count.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_div_elements() {
        let engine = LolHtmlEngine;
        let html = b"<html><body><div>a</div><div>b</div><span>c</span></body></html>";
        assert_eq!(engine.count_matches("div", html).unwrap(), 2);
    }

    #[test]
    fn counts_by_class() {
        let engine = LolHtmlEngine;
        let html = b"<div class=\"foo\">a</div><div class=\"bar\">b</div><div class=\"foo\">c</div>";
        assert_eq!(engine.count_matches(".foo", html).unwrap(), 2);
    }

    #[test]
    fn zero_matches() {
        let engine = LolHtmlEngine;
        let html = b"<html><body><p>hello</p></body></html>";
        assert_eq!(engine.count_matches("div", html).unwrap(), 0);
    }

    #[test]
    fn invalid_selector_returns_error() {
        let engine = LolHtmlEngine;
        let html = b"<div>test</div>";
        assert!(engine.count_matches("[[[invalid", html).is_err());
    }
}
