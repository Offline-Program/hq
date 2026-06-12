// Copyright (c) 2026 Red Hat, Inc.
// Licensed under the BSD 3-Clause License. See LICENSE file for details.

pub mod check_css;
pub mod css;
mod engine;
mod error;
mod scan;

pub use engine::{LolHtmlEngine, SelectorEngine};
pub use error::Error;
pub use scan::{count_matches_in_file, scan};

use serde::Serialize;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize)]
pub struct FileResult {
    pub path: PathBuf,
    pub matches: usize,
}
