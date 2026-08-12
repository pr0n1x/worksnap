use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use eyre::WrapErr as _;

use crate::excludes;
use crate::storage::Storage;
use crate::timestamp;

/// tar flags shared by full and incremental runs: don't cross into other
/// mounted filesystems and store sparse files efficiently.
const COMMON_TAR_FLAGS: &[&str] = &["--one-file-system", "--sparse"];

#[derive(Clone, Copy)]
enum TarVerbosity {
    Verbose,
    Quiet,
}

pub fn create_full(source_dir: &Path, storage: &Storage) -> eyre::Result<()> {
    let stem = timestamp::now();
    // A full run takes long; the verbose file listing shows the progress.
    run_tar(source_dir, storage, &stem, TarVerbosity::Verbose)
}

pub fn create_incremental(source_dir: &Path, storage: &Storage, base: &str) -> eyre::Result<()> {
    let base_snar = storage.find_snar_by_timestamp(base)?;
    let stem = Storage::incremental_stem(&timestamp::now(), base);
    let snar = storage.snar_path(&stem);
    // tar updates the listing in place, so the base listing is copied first:
    // the copy is both the "previous state" input and the new snapshot's snar.
    std::fs::copy(&base_snar, &snar)
        .wrap_err_with(|| format!("cannot copy {} to {}", base_snar.display(), snar.display()))?;
    run_tar(source_dir, storage, &stem, TarVerbosity::Quiet)
}

fn run_tar(
    source_dir: &Path,
    storage: &Storage,
    stem: &str,
    verbosity: TarVerbosity,
) -> eyre::Result<()> {
    let snar = storage.snar_path(stem);
    let archive = storage.archive_path(stem);
    let mut listed_incremental = OsString::from("--listed-incremental=");
    listed_incremental.push(snar.as_os_str());
    let mode_flags = match verbosity {
        TarVerbosity::Verbose => "-czvpf",
        TarVerbosity::Quiet => "-czpf",
    };
    let status = Command::new("tar")
        .arg("-C")
        .arg(source_dir)
        .arg(listed_incremental)
        .arg(mode_flags)
        .arg(&archive)
        .args(COMMON_TAR_FLAGS)
        .args(excludes::tar_args(source_dir))
        .arg(".")
        .status()
        .wrap_err("cannot run tar")?;
    // Reported even when tar failed: a partial archive on disk is something
    // the operator wants to know about.
    report_archive(&archive);
    eyre::ensure!(status.success(), "tar exited with {status}");
    Ok(())
}

fn report_archive(archive: &Path) {
    match std::fs::metadata(archive) {
        Ok(metadata) => tracing::info!(
            "created {} ({})",
            archive.display(),
            human_size(metadata.len())
        ),
        Err(error) => {
            tracing::warn!(%error, "cannot stat {}", archive.display());
        }
    }
}

/// Binary units for the human-readable archive size report.
const SIZE_UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
const SIZE_UNIT_STEP: f64 = 1024.0;

fn human_size(bytes: u64) -> String {
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= SIZE_UNIT_STEP && unit + 1 < SIZE_UNITS.len() {
        size /= SIZE_UNIT_STEP;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", SIZE_UNITS[0])
    } else {
        format!("{size:.1} {}", SIZE_UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_picks_a_readable_unit() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024 / 2), "1.5 GiB");
    }
}
