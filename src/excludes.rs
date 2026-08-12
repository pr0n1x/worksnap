use std::ffi::{OsStr, OsString};
use std::path::Path;

use walkdir::WalkDir;

/// Patterns excluded from every snapshot: `./`-anchored entries are relative
/// to the source root, bare names match a directory anywhere in the tree,
/// `*` entries are tar file-name globs.
const STATIC_COMMON_PATTERNS: &[&str] = &[
    "node_modules",
    ".venv",
    ".pnpm-store",
    "test-ledger",
    "./go/pkg",
    "./.cache/uv",
    "dhat.out.*",
    "heaptrack.*",
];

/// Marker file: a directory containing it is left out of snapshots entirely.
const IGNORE_MARKER: &str = ".worksnap-ignore-dir";
/// A `target` directory is a Cargo build artifact only when this manifest
/// sits next to it.
const CARGO_MANIFEST: &str = "Cargo.toml";
const CARGO_TARGET_DIR: &str = "target";
/// A `vendor` directory holds Composer dependencies only when this manifest
/// sits next to it.
const COMPOSER_MANIFEST: &str = "composer.json";
const COMPOSER_VENDOR_DIR: &str = "vendor";
/// Anchor (Solana) macro-expansion output — a build artifact like `target/`.
const ANCHOR_EXPAND_TARGET: &str = ".anchor/expanded-macros/expand-target";

/// All exclude patterns for a snapshot of `source_dir`: per-project build
/// artifacts discovered by walking the tree, then the common patterns.
pub fn patterns(source_dir: &Path) -> Vec<OsString> {
    let common = common_patterns();
    let mut patterns = detect_project_excludes(source_dir, &common);
    patterns.extend(common.into_iter().map(OsString::from));
    patterns
}

/// The same patterns as [`patterns`], interleaved with `--exclude` so they
/// can be passed to tar directly.
pub fn tar_args(source_dir: &Path) -> Vec<OsString> {
    let patterns = patterns(source_dir);
    let mut args = Vec::with_capacity(patterns.len() * 2);
    for pattern in patterns {
        args.push(OsString::from("--exclude"));
        args.push(pattern);
    }
    args
}

fn common_patterns() -> Vec<String> {
    let mut patterns: Vec<String> = STATIC_COMMON_PATTERNS
        .iter()
        .map(ToString::to_string)
        .collect();
    // The GNU/KDE trash directory of a mounted filesystem is named after
    // the owner's uid.
    patterns.push(format!(".Trash-{}", rustix::process::getuid().as_raw()));
    patterns
}

fn detect_project_excludes(source_dir: &Path, common_patterns: &[String]) -> Vec<OsString> {
    let mut patterns = Vec::new();
    let mut walker = WalkDir::new(source_dir).into_iter();
    while let Some(entry) = walker.next() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(%error, "skipping unreadable path");
                continue;
            }
        };
        // The root itself can't be excluded; everything else of interest
        // is a directory.
        if entry.depth() == 0 || !entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if is_excluded_project_dir(path) {
            match path.strip_prefix(source_dir) {
                Ok(rel) => {
                    let mut pattern = OsString::from("./");
                    pattern.push(rel.as_os_str());
                    patterns.push(pattern);
                }
                Err(error) => tracing::warn!(
                    %error,
                    path = %path.display(),
                    "cannot make the path relative to the source dir"
                ),
            }
            walker.skip_current_dir();
        } else if is_covered_by_common(source_dir, path, common_patterns) {
            // Excluded wholesale by a common pattern; nothing inside can
            // contribute new excludes, so don't waste time walking it.
            walker.skip_current_dir();
        }
    }
    patterns
}

fn is_excluded_project_dir(path: &Path) -> bool {
    if path.join(IGNORE_MARKER).is_file() {
        return true;
    }
    if path.ends_with(ANCHOR_EXPAND_TARGET) {
        return true;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    match path.file_name().and_then(OsStr::to_str) {
        Some(CARGO_TARGET_DIR) => parent.join(CARGO_MANIFEST).is_file(),
        Some(COMPOSER_VENDOR_DIR) => parent.join(COMPOSER_MANIFEST).is_file(),
        _ => false,
    }
}

fn is_covered_by_common(source_dir: &Path, path: &Path, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        if let Some(rel) = pattern.strip_prefix("./") {
            path.strip_prefix(source_dir)
                .is_ok_and(|suffix| suffix == Path::new(rel))
        } else {
            !pattern.contains('*') && path.file_name() == Some(OsStr::new(pattern))
        }
    })
}
