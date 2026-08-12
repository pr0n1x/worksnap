use std::path::PathBuf;

use eyre::WrapErr as _;

use crate::timestamp;

/// Extension of tar's `--listed-incremental` metadata files.
const SNAR_EXT: &str = ".snar";
/// Extension of the archives themselves.
const ARCHIVE_EXT: &str = ".tar.gz";
/// Infix separating an incremental snapshot's own timestamp from the
/// timestamp of the snapshot it is based on.
const BASED_ON_INFIX: &str = ".based-on-";

/// The flat directory holding archives (`<stem>.tar.gz`) next to their snar
/// listings (`<stem>.snar`). A stem is either a bare timestamp (full
/// snapshot) or `<timestamp>.based-on-<base-timestamp>` (incremental).
pub struct Storage {
    dir: PathBuf,
}

impl Storage {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn snar_path(&self, stem: &str) -> PathBuf {
        self.dir.join(format!("{stem}{SNAR_EXT}"))
    }

    pub fn archive_path(&self, stem: &str) -> PathBuf {
        self.dir.join(format!("{stem}{ARCHIVE_EXT}"))
    }

    pub fn incremental_stem(timestamp: &str, base: &str) -> String {
        format!("{timestamp}{BASED_ON_INFIX}{base}")
    }

    /// Timestamp of the most recent snapshot of any kind, full or incremental.
    pub fn find_last_base(&self) -> eyre::Result<String> {
        let names = self.file_names()?;
        let timestamp = last_snar_timestamp(&names).ok_or_else(|| {
            eyre::eyre!(
                "no snar files found in {} — create a full archive first",
                self.dir.display()
            )
        })?;
        Ok(timestamp.to_owned())
    }

    /// Timestamp of the most recent full archive; verifies its snar exists.
    pub fn find_last_full_base(&self) -> eyre::Result<String> {
        let names = self.file_names()?;
        let stem = last_full_archive_stem(&names)
            .ok_or_else(|| eyre::eyre!("no full archives found in {}", self.dir.display()))?;
        let snar = self.snar_path(stem);
        eyre::ensure!(
            snar.is_file(),
            "full archive {} doesn't have the appropriate snar file {}",
            self.archive_path(stem).display(),
            snar.display()
        );
        Ok(stem.to_owned())
    }

    /// The newest snar file whose name starts with the given timestamp.
    /// A bare `<timestamp>.snar` wins over `<timestamp>.based-on-…` copies
    /// because it sorts after them.
    pub fn find_snar_by_timestamp(&self, base: &str) -> eyre::Result<PathBuf> {
        let names = self.file_names()?;
        let name = last_snar_starting_with(&names, base).ok_or_else(|| {
            eyre::eyre!(
                "snar file has not been found in {}: timestamp={base:?}",
                self.dir.display()
            )
        })?;
        Ok(self.dir.join(name))
    }

    fn file_names(&self) -> eyre::Result<Vec<String>> {
        let read_context = || format!("cannot read storage dir {}", self.dir.display());
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&self.dir).wrap_err_with(read_context)? {
            let entry = entry.wrap_err_with(read_context)?;
            if !entry.file_type().wrap_err_with(read_context)?.is_file() {
                continue;
            }
            // Non-UTF-8 names can't be snapshot files; they are irrelevant here.
            if let Ok(name) = entry.file_name().into_string() {
                names.push(name);
            }
        }
        Ok(names)
    }
}

fn last_snar_timestamp(names: &[String]) -> Option<&str> {
    names
        .iter()
        .filter(|name| name.ends_with(SNAR_EXT))
        .filter_map(|name| timestamp::leading(name))
        .max()
}

fn last_full_archive_stem(names: &[String]) -> Option<&str> {
    names
        .iter()
        .filter_map(|name| name.strip_suffix(ARCHIVE_EXT))
        .filter(|stem| timestamp::is_valid(stem))
        .max()
}

fn last_snar_starting_with<'a>(names: &'a [String], base: &str) -> Option<&'a str> {
    names
        .iter()
        .filter(|name| name.starts_with(base) && name.ends_with(SNAR_EXT))
        .max()
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn last_base_is_the_newest_snar_of_any_kind() {
        let names = names(&[
            "2026-01-01-1800.snar",
            "2026-01-02-0900.based-on-2026-01-01-1800.snar",
            "2026-01-02-0900.based-on-2026-01-01-1800.tar.gz",
            "junk.snar",
        ]);
        assert_eq!(last_snar_timestamp(&names), Some("2026-01-02-0900"));
    }

    #[test]
    fn last_full_ignores_incremental_archives_and_junk() {
        let names = names(&[
            "2026-01-01-1800.tar.gz",
            "2026-01-02-0900.based-on-2026-01-01-1800.tar.gz",
            "zzz-not-a-timestamp.tar.gz",
        ]);
        assert_eq!(last_full_archive_stem(&names), Some("2026-01-01-1800"));
    }

    #[test]
    fn snar_lookup_prefers_the_bare_snar_over_based_on_copies() {
        let names = names(&[
            "2026-01-01-1800.based-on-2025-12-31-0200.snar",
            "2026-01-01-1800.snar",
        ]);
        assert_eq!(
            last_snar_starting_with(&names, "2026-01-01-1800"),
            Some("2026-01-01-1800.snar")
        );
    }
}
