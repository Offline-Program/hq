# hq

Query HTML files by CSS selector.  Grep for HTML.  Especially useful for investigating the output of static site generators.

## Install

```sh
cargo install --path hq-cli
```

## Usage

```sh
# Count links in all HTML files current directory and any nested directories
hq "a[href]"

# Count <div> elements in a single file
hq div index.html

# Search a directory tree
hq "a[href]" ./site/

# JSONL output
hq --json ".card" ./templates/

# Hide files with no matches
hq --no-zeros "img" ./pages/

# Sort by match count
hq "div" ./site/ | sort -n
```

## Output

Human-readable (default):
```
12	site/index.html
0	site/about.html
3	site/contact.html
```

JSONL (`--json`):
```
{"path":"site/index.html","matches":12}
{"path":"site/about.html","matches":0}
{"path":"site/contact.html","matches":3}
```

## Supported selectors

hq uses [lol_html's CSS selector engine](https://docs.rs/lol_html/2/lol_html/struct.Selector.html).

Supported:

- Universal selector: `*`
- Type selectors: `div`, `span`, `a`
- Class selectors: `.foo`, `div.bar`
- ID selectors: `#main`
- Attribute selectors: `[href]`, `[data-type="info"]`, `[class~="active"]`, `[href^="https"]`, `[src$=".png"]`, `[title*="hello"]`, `[lang|="en"]`, `[data-x="y" i]` (case-insensitive), `[data-x="y" s]` (case-sensitive)
- Pseudo-classes: `:first-child`, `:nth-child(n)`, `:first-of-type`, `:nth-of-type(n)`, `:not(selector)`
- Descendant combinator: `div span`
- Child combinator: `div > span`

Not supported:

- Sibling combinators (`~`, `+`)
- Selector lists (comma-separated: `div, span`)
- Pseudo-classes beyond the above (`:last-child`, `:has()`, `:is()`, `:where()`, etc.)
- Pseudo-elements (`::before`, `::after`, etc.)

## Exit codes

| Code | Meaning |
|------|---------|
| 0    | At least one file had matches |
| 1    | No files had matches |
| 2    | Error (invalid selector, path not found, etc.) |
