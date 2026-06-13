// Copyright (c) 2026 Red Hat, Inc.
// Licensed under the BSD 3-Clause License. See LICENSE file for details.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub struct Progress {
    pub css_files: AtomicU64,
    pub selectors: AtomicU64,
    pub html_files: AtomicU64,
    pub selectors_used: AtomicU64,
    pub unused_bytes: AtomicU64,
    pub done: AtomicBool,
}

impl Progress {
    pub fn new() -> Self {
        Self {
            css_files: AtomicU64::new(0),
            selectors: AtomicU64::new(0),
            html_files: AtomicU64::new(0),
            selectors_used: AtomicU64::new(0),
            unused_bytes: AtomicU64::new(0),
            done: AtomicBool::new(false),
        }
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            css_files: self.css_files.load(Ordering::Relaxed),
            selectors: self.selectors.load(Ordering::Relaxed),
            html_files: self.html_files.load(Ordering::Relaxed),
            selectors_used: self.selectors_used.load(Ordering::Relaxed),
            unused_bytes: self.unused_bytes.load(Ordering::Relaxed),
        }
    }
}

pub struct ProgressSnapshot {
    pub css_files: u64,
    pub selectors: u64,
    pub html_files: u64,
    pub selectors_used: u64,
    pub unused_bytes: u64,
}

impl ProgressSnapshot {
    pub fn selectors_unused(&self) -> u64 {
        self.selectors.saturating_sub(self.selectors_used)
    }

    pub fn used_percent(&self) -> f64 {
        if self.selectors == 0 {
            return 0.0;
        }
        self.selectors_used as f64 / self.selectors as f64 * 100.0
    }
}
