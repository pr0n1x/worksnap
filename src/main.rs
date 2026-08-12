mod excludes;
mod snapshot;
mod storage;
mod timestamp;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::storage::Storage;

const ENV_SOURCE_DIR: &str = "WORKSNAP_SOURCE_DIR";
const ENV_STORAGE_DIR: &str = "WORKSNAP_STORAGE_DIR";
/// The example source-root ignore policy, shipped inside the binary so
/// `show-ignore-example` works without a repo checkout.
const IGNORE_EXAMPLE: &str = include_str!("../worksnap-ignore.example");

/// Incremental tar snapshots of the work directory.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Directory whose contents are snapshotted
    #[arg(long, env = ENV_SOURCE_DIR, value_parser = parse_dir, value_name = "DIR")]
    source_dir: Option<PathBuf>,

    /// Directory keeping the archives and their .snar listings
    #[arg(long, env = ENV_STORAGE_DIR, value_parser = parse_dir, value_name = "DIR")]
    storage_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Create an incremental archive on top of a base snapshot [default]
    #[command(visible_alias = "inc")]
    Incremental {
        /// Base snapshot: "last", "last-full" ("full"), or a timestamp
        #[arg(
            short,
            long,
            visible_alias = "from",
            default_value = "last",
            value_parser = parse_base,
            value_name = "TIMESTAMP"
        )]
        base: BaseSpec,
    },
    /// Create a new full archive
    Full,
    /// Print the timestamp of the last (or the last full) base
    Find {
        #[arg(value_enum)]
        target: FindTarget,
    },
    /// Print the tar exclude patterns computed for the source dir
    ShowIgnores,
    /// Print the built-in example of a source-root .worksnap-ignore
    ShowIgnoreExample,
}

#[derive(Clone)]
enum BaseSpec {
    Last,
    LastFull,
    Timestamp(String),
}

#[derive(Clone, Copy, ValueEnum)]
enum FindTarget {
    Last,
    Full,
}

/// Rejects blank values: without this an env var exported as `VAR=""`
/// silently satisfies the argument and turns into an empty path.
fn parse_dir(text: &str) -> Result<PathBuf, String> {
    if text.trim().is_empty() {
        return Err("directory must not be empty".to_owned());
    }
    Ok(PathBuf::from(text))
}

fn parse_base(text: &str) -> Result<BaseSpec, String> {
    match text {
        "last" => Ok(BaseSpec::Last),
        "last-full" | "full" => Ok(BaseSpec::LastFull),
        timestamp if timestamp::is_valid(timestamp) => {
            Ok(BaseSpec::Timestamp(timestamp.to_owned()))
        }
        _ => Err("expected \"last\", \"last-full\" (or its alias \"full\"), \
             or a YYYY-MM-DD-HHMM timestamp"
            .to_owned()),
    }
}

/// Unwraps a directory option for the commands that can't run without it;
/// `show-ignore-example` works with neither dir configured.
fn require_dir(dir: Option<PathBuf>, flag: &str, env: &str) -> eyre::Result<PathBuf> {
    dir.ok_or_else(|| eyre::eyre!("{flag} is required (or set the {env} env var)"))
}

fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let source_dir = || require_dir(cli.source_dir, "--source-dir", ENV_SOURCE_DIR);
    let storage =
        || require_dir(cli.storage_dir, "--storage-dir", ENV_STORAGE_DIR).map(Storage::new);
    let default_command = Command::Incremental {
        base: BaseSpec::Last,
    };
    match cli.command.unwrap_or(default_command) {
        Command::Incremental { base } => {
            let storage = storage()?;
            let base = match base {
                BaseSpec::Last => storage.find_last_base()?,
                BaseSpec::LastFull => storage.find_last_full_base()?,
                BaseSpec::Timestamp(timestamp) => timestamp,
            };
            snapshot::create_incremental(&source_dir()?, &storage, &base)
        }
        Command::Full => snapshot::create_full(&source_dir()?, &storage()?),
        Command::Find { target } => {
            let storage = storage()?;
            let timestamp = match target {
                FindTarget::Last => storage.find_last_base()?,
                FindTarget::Full => storage.find_last_full_base()?,
            };
            println!("{timestamp}");
            Ok(())
        }
        Command::ShowIgnores => {
            for pattern in excludes::patterns(&source_dir()?) {
                println!("{}", pattern.display());
            }
            Ok(())
        }
        Command::ShowIgnoreExample => {
            print!("{IGNORE_EXAMPLE}");
            Ok(())
        }
    }
}
