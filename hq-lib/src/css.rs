// Copyright (c) 2026 Red Hat, Inc.
// Licensed under the BSD 3-Clause License. See LICENSE file for details.

use crate::Result;
use cssparser::{
    AtRuleParser, CowRcStr, ParseError, Parser, ParserInput, ParserState,
    QualifiedRuleParser, StyleSheetParser,
};
use log::{debug, trace};

#[derive(Debug, Clone)]
pub struct CssRule {
    pub selector: String,
    pub start: usize,
    pub end: usize,
}

pub fn extract_rules(css: &str) -> Result<Vec<CssRule>> {
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut rule_parser = RuleExtractor;

    let mut rules = Vec::new();
    let sheet = StyleSheetParser::new(&mut parser, &mut rule_parser);
    for result in sheet {
        match result {
            Ok(Some(rule)) => rules.push(rule),
            Ok(None) => {}
            Err((err, _slice)) => {
                return Err(crate::Error::CssParse(format!("{err:?}")));
            }
        }
    }
    debug!("extracted {} style rules from CSS", rules.len());
    for rule in &rules {
        trace!("rule: '{}' at bytes {}..{}", rule.selector, rule.start, rule.end);
    }
    Ok(rules)
}

struct RuleExtractor;

impl<'i> QualifiedRuleParser<'i> for RuleExtractor {
    type Prelude = String;
    type QualifiedRule = Option<CssRule>;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> std::result::Result<Self::Prelude, ParseError<'i, Self::Error>> {
        let start = input.position();
        while input.next_including_whitespace_and_comments().is_ok() {}
        let selector = input.slice_from(start).trim().to_string();
        Ok(selector)
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> std::result::Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
        // consume the block contents
        while input.next_including_whitespace_and_comments().is_ok() {}
        let end = input.position().byte_index();
        // +1 for the closing }
        Ok(Some(CssRule {
            selector: prelude,
            start: start.position().byte_index(),
            end: end + 1,
        }))
    }
}

impl<'i> AtRuleParser<'i> for RuleExtractor {
    type Prelude = ();
    type AtRule = Option<CssRule>;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> std::result::Result<Self::Prelude, ParseError<'i, Self::Error>> {
        trace!("skipping at-rule @{}", name);
        while input.next_including_whitespace_and_comments().is_ok() {}
        Ok(())
    }

    fn rule_without_block(
        &mut self,
        _prelude: Self::Prelude,
        _start: &ParserState,
    ) -> std::result::Result<Self::AtRule, ()> {
        Ok(None)
    }

    fn parse_block<'t>(
        &mut self,
        _prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> std::result::Result<Self::AtRule, ParseError<'i, Self::Error>> {
        // consume and discard at-rule block
        while input.next_including_whitespace_and_comments().is_ok() {}
        Ok(None)
    }
}

pub fn prune_css(css: &str, unused_rules: &[CssRule]) -> String {
    if unused_rules.is_empty() {
        return css.to_string();
    }

    let mut spans: Vec<(usize, usize)> = unused_rules.iter().map(|r| (r.start, r.end)).collect();
    spans.sort_by_key(|s| s.0);

    let mut result = String::with_capacity(css.len());
    let mut pos = 0;
    for (start, end) in &spans {
        if *start > pos {
            result.push_str(&css[pos..*start]);
        }
        pos = *end;
    }
    if pos < css.len() {
        result.push_str(&css[pos..]);
    }

    // collapse runs of blank lines to a single newline
    let mut cleaned = String::with_capacity(result.len());
    let mut blank_run = 0;
    for line in result.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                cleaned.push('\n');
            }
        } else {
            blank_run = 0;
            if !cleaned.is_empty() {
                cleaned.push('\n');
            }
            cleaned.push_str(line);
        }
    }
    if cleaned.ends_with('\n') || css.ends_with('\n') {
        if !cleaned.ends_with('\n') {
            cleaned.push('\n');
        }
    }

    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_rules ---

    #[test]
    fn extracts_simple_rules() {
        let css = ".foo { color: red; }\n.bar { color: blue; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].selector, ".foo");
        assert_eq!(rules[1].selector, ".bar");
        assert_eq!(&css[rules[0].start..rules[0].end], ".foo { color: red; }");
        assert_eq!(&css[rules[1].start..rules[1].end], ".bar { color: blue; }");
    }

    #[test]
    fn extracts_tag_selector() {
        let css = "div { margin: 0; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "div");
        assert_eq!(&css[rules[0].start..rules[0].end], css);
    }

    #[test]
    fn extracts_id_selector() {
        let css = "#main { padding: 10px; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "#main");
    }

    #[test]
    fn extracts_attribute_selector() {
        let css = "a[href] { color: blue; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "a[href]");
    }

    #[test]
    fn extracts_descendant_selector() {
        let css = "nav ul li { list-style: none; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "nav ul li");
    }

    #[test]
    fn extracts_child_combinator() {
        let css = "div.foo > p.bar { color: red; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "div.foo > p.bar");
    }

    #[test]
    fn extracts_multiline_rule() {
        let css = ".card {\n  background: white;\n  border: 1px solid #ccc;\n  padding: 16px;\n}";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, ".card");
        assert_eq!(&css[rules[0].start..rules[0].end], css);
    }

    #[test]
    fn empty_css_returns_no_rules() {
        let rules = extract_rules("").unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn whitespace_only_css_returns_no_rules() {
        let rules = extract_rules("   \n\n  \t  \n").unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn css_with_only_comments_returns_no_rules() {
        let rules = extract_rules("/* nothing here */\n/* still nothing */").unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn css_with_only_at_rules_returns_no_rules() {
        let css = "@charset \"UTF-8\";\n@import url('other.css');\n@media screen { .x { color: red; } }";
        let rules = extract_rules(css).unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn ignores_media_query() {
        let css = "@media (max-width: 600px) { .responsive { display: none; } }\n.plain { color: red; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, ".plain");
    }

    #[test]
    fn ignores_keyframes() {
        let css = "@keyframes fade { from { opacity: 0; } to { opacity: 1; } }\n.thing { color: red; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, ".thing");
    }

    #[test]
    fn ignores_font_face() {
        let css = "@font-face { font-family: 'Custom'; src: url('font.woff2'); }\n.text { font-family: 'Custom'; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, ".text");
    }

    #[test]
    fn ignores_charset_and_import() {
        let css = "@charset \"UTF-8\";\n@import url('reset.css');\nbody { margin: 0; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "body");
    }

    #[test]
    fn ignores_supports() {
        let css = "@supports (display: grid) { .grid { display: grid; } }\n.fallback { display: flex; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, ".fallback");
    }

    #[test]
    fn mixed_at_rules_and_style_rules() {
        let css = "\
@charset \"UTF-8\";
@import url('reset.css');
body { margin: 0; }
@media screen { .m { color: red; } }
.header { background: blue; }
@keyframes spin { to { transform: rotate(360deg); } }
.footer { padding: 20px; }
@font-face { font-family: 'X'; src: url('x.woff2'); }
";
        let rules = extract_rules(css).unwrap();
        let selectors: Vec<&str> = rules.iter().map(|r| r.selector.as_str()).collect();
        assert_eq!(selectors, vec!["body", ".header", ".footer"]);
    }

    #[test]
    fn spans_are_non_overlapping_and_ordered() {
        let css = ".a { x: 1; }\n.b { x: 2; }\n.c { x: 3; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 3);
        for i in 1..rules.len() {
            assert!(
                rules[i].start >= rules[i - 1].end,
                "rule {} start ({}) should be >= rule {} end ({})",
                i,
                rules[i].start,
                i - 1,
                rules[i - 1].end,
            );
        }
    }

    #[test]
    fn spans_capture_exact_rule_text() {
        let css = "  .a { x: 1; }\n\n  .b { x: 2; }  \n";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(css[rules[0].start..rules[0].end].trim(), ".a { x: 1; }");
        assert_eq!(css[rules[1].start..rules[1].end].trim(), ".b { x: 2; }");
    }

    #[test]
    fn rule_with_comments_before_it() {
        let css = "/* header styles */\n.header { color: red; }";
        let rules = extract_rules(css).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, ".header");
    }

    // --- prune_css ---

    #[test]
    fn prune_removes_single_unused_rule() {
        let css = ".used { color: red; }\n.unused { color: blue; }\n.also-used { color: green; }\n";
        let rules = extract_rules(css).unwrap();
        let unused: Vec<_> = rules.into_iter().filter(|r| r.selector == ".unused").collect();
        let result = prune_css(css, &unused);
        assert!(result.contains(".used { color: red; }"));
        assert!(!result.contains(".unused"));
        assert!(result.contains(".also-used { color: green; }"));
    }

    #[test]
    fn prune_removes_multiple_unused_rules() {
        let css = ".a { x: 1; }\n.b { x: 2; }\n.c { x: 3; }\n.d { x: 4; }\n";
        let rules = extract_rules(css).unwrap();
        let unused: Vec<_> = rules
            .into_iter()
            .filter(|r| r.selector == ".b" || r.selector == ".d")
            .collect();
        let result = prune_css(css, &unused);
        assert!(result.contains(".a { x: 1; }"));
        assert!(!result.contains(".b"));
        assert!(result.contains(".c { x: 3; }"));
        assert!(!result.contains(".d"));
    }

    #[test]
    fn prune_all_rules_leaves_only_at_rules() {
        let css = "@media screen { .m { color: red; } }\n.a { x: 1; }\n.b { x: 2; }\n";
        let rules = extract_rules(css).unwrap();
        let result = prune_css(css, &rules);
        assert!(result.contains("@media"));
        assert!(!result.contains(".a"));
        assert!(!result.contains(".b"));
    }

    #[test]
    fn prune_no_rules_returns_original() {
        let css = ".a { x: 1; }\n.b { x: 2; }\n";
        let result = prune_css(css, &[]);
        assert_eq!(result, css);
    }

    #[test]
    fn prune_preserves_at_rules() {
        let css = "@media (max-width: 600px) { .resp { display: none; } }\n.unused { color: blue; }\n";
        let rules = extract_rules(css).unwrap();
        let result = prune_css(css, &rules);
        assert!(result.contains("@media"));
        assert!(!result.contains(".unused"));
    }

    #[test]
    fn prune_preserves_keyframes() {
        let css = ".unused { x: 1; }\n@keyframes fade { from { opacity: 0; } to { opacity: 1; } }\n";
        let rules = extract_rules(css).unwrap();
        let result = prune_css(css, &rules);
        assert!(!result.contains(".unused"));
        assert!(result.contains("@keyframes fade"));
    }

    #[test]
    fn prune_collapses_blank_lines() {
        let css = ".a { x: 1; }\n\n.unused { x: 2; }\n\n.b { x: 3; }\n";
        let rules = extract_rules(css).unwrap();
        let unused: Vec<_> = rules.into_iter().filter(|r| r.selector == ".unused").collect();
        let result = prune_css(css, &unused);
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn prune_empty_input() {
        let result = prune_css("", &[]);
        assert_eq!(result, "");
    }
}
