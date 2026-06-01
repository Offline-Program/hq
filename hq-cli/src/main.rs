// Copyright (c) 2026 Red Hat, Inc.
// Licensed under the BSD 3-Clause License. See LICENSE file for details.

use anstyle::{AnsiColor, Style};
use clap::Parser;
use hq_lib::{FileResult, LolHtmlEngine, scan};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process;

#[derive(Parser)]
#[command(name = "hq", about = "Query HTML files by CSS selector")]
struct Args {
    /// CSS selector to match elements
    selector: String,

    /// File or directory to search
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Output in JSONL format
    #[arg(long)]
    json: bool,

    /// Omit files with zero matches
    #[arg(long)]
    no_zeros: bool,
}

const BLUE: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Blue)));
const LIGHT_GREEN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightGreen)));
const GREY: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightBlack)));
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

fn main() {
    let args = Args::parse();
    let engine = LolHtmlEngine;

    let mut results = match scan(&engine, &args.path, &args.selector) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("hq: {e}");
            process::exit(2);
        }
    };

    results.sort_by(|a, b| a.path.cmp(&b.path));

    if args.json {
        print_jsonl(&results, args.no_zeros);
    } else {
        let use_color = io::stdout().is_terminal();
        print_human(&results, args.no_zeros, use_color);
    }

    let any_matches = results.iter().any(|r| r.matches > 0);
    process::exit(if any_matches { 0 } else { 1 });
}
