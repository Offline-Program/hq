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

## CSS usage checking

Find which CSS selectors are actually used in your HTML:

```sh
# Check which selectors in a CSS file are used by HTML files
hq check-css --css styles.css --html ./site/

# Prune unused rules, write to a new file
hq check-css --css styles.css --html ./site/ --prune -o styles-clean.css

# Prune a whole directory of CSS files
hq check-css --css ./css/ --html ./site/ --prune --outdir ./css-clean/

# JSONL output
hq check-css --css styles.css --html ./site/ --json
```

Output:
```
styles.css
  USED    .header
  USED    div.content > p
  UNUSED  .old-widget
  UNUSED  #legacy-nav
```

Only simple style rules (selector + declaration block) are checked. At-rules (`@media`, `@keyframes`, `@font-face`, etc.) are left untouched during pruning.

## Exit codes

| Command | Code | Meaning |
|---------|------|---------|
| `hq <SELECTOR>` | 0 | At least one file had matches |
| `hq <SELECTOR>` | 1 | No files had matches |
| `hq check-css`  | 0 | All selectors are used |
| `hq check-css`  | 1 | Some selectors are unused |
| (any)           | 2 | Error (invalid selector, path not found, etc.) |
