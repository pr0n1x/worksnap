# worksnap

Incremental tar snapshots of the work directory. A Rust reimplementation of
the original `work-snapshot.sh` (kept in this repo for reference).

## How it works

worksnap archives the *source directory* into the *storage directory* using
GNU tar's `--listed-incremental` mode. Every snapshot consists of two files
sharing one stem:

- `<stem>.tar.gz` — the archive itself;
- `<stem>.snar` — tar's snapshot listing: the file/mtime state the archive
  was taken against, and the input for the next incremental run.

The stem encodes the snapshot's place in the chain:

- **Full snapshot** — `2026-08-03-1400.tar.gz`. Created from scratch with a
  fresh `.snar`; contains everything (minus the excludes below).
- **Incremental snapshot** — `2026-08-12-1200.based-on-2026-08-03-1400.tar.gz`.
  Contains only what changed since the *base* snapshot. The base's `.snar` is
  copied to the new stem first, and tar updates the copy in place, so the new
  snapshot immediately becomes a valid base for the next increment.

Incrementals chain: a base may itself be incremental, so a typical history
looks like `full ← inc ← inc ← …`, with each link named after its
predecessor via `.based-on-<timestamp>`.

Timestamps are `YYYY-MM-DD-HHMM`, chosen so lexicographic order equals
chronological order — "the last snapshot" is simply the greatest file name.
`find last` returns the newest snapshot of any kind; `find full` returns the
newest full one (and verifies its `.snar` still exists).

Tar runs with `--one-file-system` (nested mounts are not descended into) and
`--sparse` (sparse files stored efficiently).

### What gets excluded

Before every run the source tree is walked once and per-project build
artifacts are excluded:

| Excluded directory | Condition |
|---|---|
| `target/` | a `Cargo.toml` sits next to it |
| `vendor/` | a `composer.json` sits next to it |
| `.anchor/expanded-macros/expand-target/` | always (Anchor macro-expansion output) |
| any directory | it contains a `.worksnap-ignore-dir` marker file |

On top of that a static list is always excluded: `node_modules`, `.venv`,
`.pnpm-store`, `test-ledger` (anywhere in the tree), `go/pkg` and
`.cache/uv` (relative to the source root), `dhat.out.*` / `heaptrack.*`
profiler dumps, and the `.Trash-<uid>` directory.

`worksnap show-ignores` prints the full computed list without archiving
anything. To keep a directory out of snapshots, drop an empty
`.worksnap-ignore-dir` file into it:

```sh
touch some/huge/dir/.worksnap-ignore-dir
```

## Configuration

Both directories are required and have no defaults. Set them via flags
(`--source-dir`, `--storage-dir`) or environment variables — empty values
are rejected:

```sh
# e.g. in ~/.profile.env
export WORKSNAP_SOURCE_DIR="/path/to/your/work"
export WORKSNAP_STORAGE_DIR="/path/to/backups/work.snapshots"
```

## Usage

```sh
worksnap full                     # new full snapshot
worksnap                          # incremental on the last snapshot (default)
worksnap inc                      # same, explicitly
worksnap inc --base last-full     # incremental directly on the last full one
worksnap inc --base 2026-08-03-1400   # incremental on an explicit base
worksnap find last                # print the newest snapshot's timestamp
worksnap find full                # print the newest full snapshot's timestamp
worksnap show-ignores             # print the computed exclude patterns
```

`--base` (alias `--from`, short `-b`) accepts `last`, `last-full` (or its
alias `full`), or a concrete `YYYY-MM-DD-HHMM` timestamp.

Progress and results go to stderr; `find`/`show-ignores` output goes to
stdout. A full run lists files as tar processes them; an incremental run is
quiet and just reports the resulting archive and its size.

## Restoring

Extract the full snapshot first, then every incremental along the
`.based-on-` chain in chronological order, each with an empty listing:

```sh
cd /restore/here
tar -xzpf 2026-08-03-1400.tar.gz --listed-incremental=/dev/null
tar -xzpf 2026-08-10-0900.based-on-2026-08-03-1400.tar.gz --listed-incremental=/dev/null
tar -xzpf 2026-08-12-1200.based-on-2026-08-10-0900.tar.gz --listed-incremental=/dev/null
```

`--listed-incremental` on extraction makes tar replay deletions too: files
removed between snapshots are removed from the restored tree.

## Development

```sh
just p    # prepare: cargo check + clippy -D warnings + fmt --check + machete + test
just t    # tests only
```