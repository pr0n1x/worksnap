# worksnap

Incremental tar snapshots of a work directory, built on GNU tar's
`--listed-incremental` mode.

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

Before every run the source tree is walked once, collecting exclusions.
Only one thing is built in: any directory containing a
`.worksnap-ignore-dir` marker file is left out of snapshots entirely
(`touch some/huge/dir/.worksnap-ignore-dir`).

Everything else — build artifacts, dependency caches, the trash dir,
whatever your tree accumulates — is policy, not mechanism, and belongs in
`.worksnap-ignore` files, typically one at the source root.
[worksnap-ignore.example](./worksnap-ignore.example) is a ready-made
starting point, shipped inside the binary:
`worksnap show-ignore-example > .worksnap-ignore`.

```sh
# <source-root>/.worksnap-ignore
*/:Cargo.toml:target/     # every dir with a Cargo.toml: ignore its target/
*/:composer.json:vendor/  # every dir with a composer.json: ignore its vendor/
node_modules              # a bare glob matches anywhere below this directory
.Trash-<uid>              # <uid>/<gid> expand to the current user's ids
/go/pkg                   # a leading / pins the glob to this directory only
```

`worksnap show-ignores` prints the full computed exclude list — built-ins,
globs, and fired rules — without archiving anything.

### `.worksnap-ignore` files

For finer-grained control any directory may contain a `.worksnap-ignore`
file. Every non-blank line that doesn't start with `#` is a tar exclusion
glob (the flavor of GNU tar's `--exclude`: wildcards on, `*` also matches
`/`), scoped to the directory the file lives in:

- a **bare glob** matches anywhere below that directory, like tar's own
  unanchored patterns;
- a glob with a **leading `/`** is pinned to that directory only.

```sh
# proj/.worksnap-ignore
*.log             # proj/deep.log and proj/logs/a.log alike
/data/*.bin       # proj/data/*.bin only, not proj/x/data/*.bin
```

The ignore file itself is archived, so a restored tree keeps its rules.

Besides plain globs, an ignore file may contain **conditional rules**:

```
[<dir-glob>:]<marker>:<path-to-ignore>
```

During the pre-snapshot scan, every directory matching `<dir-glob>` (same
tar-glob flavor, scoped to the ignore file's directory; when omitted, the
ignore file's directory itself) whose `<marker>` path exists gets
`<path-to-ignore>` excluded, resolved relative to that directory; `.` means
the matched directory itself. A rule tests only that `<marker>` exists;
`<path-to-ignore>` is excluded whether it exists yet, so a project's
build artifacts are already covered when the first build creates them:

```sh
Cargo.toml:target/        # this very dir: if Cargo.toml exists, ignore target/
*/:.git:.                 # ignore every git checkout entirely
*/a/b/:c/file.txt:d/      # if any */a/b/c/file.txt exists, ignore that */a/b/d
```

Anything after ` #` (space and hash) on a line is an inline comment. The
literal tokens `<uid>` and `<gid>` in any line expand to the current
user's uid and gid before parsing — the mounted-filesystem trash dir, for
one, is named `.Trash-<uid>`.

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
worksnap show-ignore-example      # print the built-in example .worksnap-ignore
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

## Installation

```sh
cargo install worksnap          # from crates.io
just install                    # from a checkout (cargo install --path .)
```

## Development

```sh
just p    # prepare: cargo check + clippy -D warnings + fmt --check + machete + test
just t    # tests only
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.