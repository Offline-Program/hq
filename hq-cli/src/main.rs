// Copyright (c) 2026 Red Hat, Inc.
// Licensed under the BSD 3-Clause License. See LICENSE file for details.

use anstyle::{AnsiColor, Style};
use clap::{Parser, Subcommand};
use hq_lib::{FileResult, LolHtmlEngine, scan};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;

#[derive(Parser)]
#[command(name = "hq", about = "Query HTML files by CSS selector")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// CSS selector to match elements (when used without a subcommand)
    selector: Option<String>,

    /// File or directory to search
    #[arg(default_value = ".")]
    path: Option<PathBuf>,

    /// Output in JSONL format
    #[arg(long)]
    json: bool,

    /// Omit files with zero matches
    #[arg(long)]
    no_zeros: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Check which CSS selectors are used in HTML files
    CheckCss(CheckCssArgs),
}

#[derive(Parser)]
struct CheckCssArgs {
    /// CSS file or directory
    #[arg(long)]
    css: PathBuf,

    /// HTML file or directory
    #[arg(long)]
    html: PathBuf,

    /// Prune unused rules from CSS
    #[arg(long)]
    prune: bool,

    /// Output file for pruned CSS (single CSS file input only)
    #[arg(short = 'o', long, conflicts_with = "outdir")]
    output: Option<PathBuf>,

    /// Output directory for pruned CSS (directory input only)
    #[arg(long, conflicts_with = "output")]
    outdir: Option<PathBuf>,

    /// Output in JSONL format
    #[arg(long)]
    json: bool,
}

const BLUE: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Blue)));
const LIGHT_GREEN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightGreen)));
const GREY: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightBlack)));
const RED: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));
const RESET: anstyle::Reset = anstyle::Reset;

fn print_human(results: &[FileResult], no_zeros: bool, use_color: bool) {
    let mut out = io::stdout().lock();
    for r in results {
        if no_zeros && r.matches == 0 {
            continue;
        }
        if use_color {
            let count_style = if r.matches > 0 { LIGHT_GREEN } else { GREY };
            let _ = write!(
                out,
                "{count_style}{}{RESET}\t{BLUE}{}{RESET}\n",
                r.matches,
                r.path.display(),
            );
        } else {
            let _ = writeln!(out, "{}\t{}", r.matches, r.path.display());
        }
    }
    if use_color {
        let _ = write!(out, "{RESET}");
    }
}

fn print_jsonl(results: &[FileResult], no_zeros: bool) {
    let mut out = io::stdout().lock();
    for r in results {
        if no_zeros && r.matches == 0 {
            continue;
        }
        let _ = serde_json::to_writer(&mut out, r);
        let _ = out.write_all(b"\n");
    }
}

fn run_query(selector: &str, path: &PathBuf, json: bool, no_zeros: bool) {
    let engine = LolHtmlEngine;

    let mut results = match scan(&engine, path, selector) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("hq: {e}");
            process::exit(2);
        }
    };

    results.sort_by(|a, b| a.path.cmp(&b.path));

    if json {
        print_jsonl(&results, no_zeros);
    } else {
        let use_color = io::stdout().is_terminal();
        print_human(&results, no_zeros, use_color);
    }

    let any_matches = results.iter().any(|r| r.matches > 0);
    process::exit(if any_matches { 0 } else { 1 });
}

fn run_check_css(args: CheckCssArgs) {
    let engine = LolHtmlEngine;
    let use_color = io::stdout().is_terminal();

    let results = match hq_lib::check_css::check_css(&engine, &args.css, &args.html) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("hq: {e}");
            process::exit(2);
        }
    };

    // Print check results
    let mut out = io::stdout().lock();
    for file_result in &results {
        if args.json {
            for sel in &file_result.selectors {
                let _ = serde_json::to_writer(
                    &mut out,
                    &serde_json::json!({
                        "file": file_result.path,
                        "selector": sel.selector,
                        "used": sel.used,
                    }),
                );
                let _ = out.write_all(b"\n");
            }
        } else if use_color {
            let _ = writeln!(out, "{BLUE}{}{RESET}", file_result.path.display());
            for sel in &file_result.selectors {
                let (label, style) = if sel.used {
                    ("USED  ", LIGHT_GREEN)
                } else {
                    ("UNUSED", RED)
                };
                let _ = writeln!(out, "  {style}{label}{RESET}  {}", sel.selector);
            }
        } else {
            let _ = writeln!(out, "{}", file_result.path.display());
            for sel in &file_result.selectors {
                let label = if sel.used { "USED  " } else { "UNUSED" };
                let _ = writeln!(out, "  {}  {}", label, sel.selector);
            }
        }
    }
    drop(out);

    if args.prune {
        let is_single = hq_lib::check_css::css_input_is_single_file(&args.css);

        if is_single && args.output.is_none() {
            eprintln!("hq: --prune requires -o <FILE> for single CSS file input");
            process::exit(2);
        }
        if !is_single && args.outdir.is_none() {
            eprintln!("hq: --prune requires --outdir <DIR> for directory CSS input");
            process::exit(2);
        }

        let pruned = match hq_lib::check_css::prune(&engine, &args.css, &args.html) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("hq: {e}");
                process::exit(2);
            }
        };

        if is_single {
            let out_path = args.output.unwrap();
            if let Some(parent) = out_path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                        eprintln!("hq: {e}");
                        process::exit(2);
                    });
                }
            }
            std::fs::write(&out_path, &pruned[0].content).unwrap_or_else(|e| {
                eprintln!("hq: {}: {e}", out_path.display());
                process::exit(2);
            });
        } else {
            let outdir = args.outdir.unwrap();
            let css_base = if args.css.is_dir() {
                args.css.clone()
            } else {
                args.css.parent().unwrap_or(Path::new(".")).to_path_buf()
            };

            for pf in &pruned {
                let rel = pf.path.strip_prefix(&css_base).unwrap_or(&pf.path);
                let dest = outdir.join(rel);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                        eprintln!("hq: {e}");
                        process::exit(2);
                    });
                }
                std::fs::write(&dest, &pf.content).unwrap_or_else(|e| {
                    eprintln!("hq: {}: {e}", dest.display());
                    process::exit(2);
                });
            }
        }
    }

    let any_unused = results
        .iter()
        .any(|r| r.selectors.iter().any(|s| !s.used));
    process::exit(if any_unused { 1 } else { 0 });
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::CheckCss(args)) => run_check_css(args),
        None => {
            let selector = match cli.selector {
                Some(s) => s,
                None => {
                    eprintln!("hq: missing selector argument");
                    eprintln!("Usage: hq <SELECTOR> <PATH>");
                    process::exit(2);
                }
            };
            let path = cli.path.unwrap_or_else(|| PathBuf::from("."));
            run_query(&selector, &path, cli.json, cli.no_zeros);
        }
    }
}
