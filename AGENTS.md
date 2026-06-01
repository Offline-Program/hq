# hq — HTML Query

CSS selector query tool for HTML files. Searches files or directories and reports per-file match counts.

## Workspace

```
hq-lib/    Core library: SelectorEngine trait, LolHtmlEngine, scanning, FileResult
hq-cli/    CLI binary (installed as `hq`)
```

A server crate will be added later.

## Build / Test / Run

```sh
cargo build                              # build all
cargo test                               # test all
cargo run -p hq-cli -- "div.foo" ./path  # run
```

## Architecture

### SelectorEngine trait

All HTML parsing goes through `trait SelectorEngine: Send + Sync`. The trait accepts a selector string and raw HTML bytes, returns a match count. Implementations are swappable.

Current implementation: `LolHtmlEngine` (streaming via lol_html). Supports tag, class, ID, attribute, descendant, and child selectors. Does not support sibling combinators or pseudo-classes.

### FileResult

Shared struct for all output formats — CLI human-readable, CLI JSONL, and eventual server responses:

```rust
pub struct FileResult {
    pub path: PathBuf,
    pub matches: usize,
}
```

### Scanning

`scan()` accepts a file or directory path. Directories are walked for `*.html`/`*.htm` files and processed in parallel via rayon. Errors on individual files are reported to stderr but do not halt processing.

## Dependencies

Prefer crates already in the dependency tree (including transitives) over adding new ones. Check `cargo tree` before adding a dependency.

## CLI usage

```
hq <SELECTOR> <PATH>
    --json        Output JSONL (one FileResult per line)
    --no-zeros    Omit files with zero matches
```

Exit code: 0 if any matches found, 1 if none, 2 on error.
