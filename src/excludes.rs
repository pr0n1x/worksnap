use std::ffi::{OsStr, OsString};
use std::path::Path;

use walkdir::WalkDir;

/// Marker file: a directory containing it is left out of snapshots entirely.
const IGNORE_MARKER: &str = ".worksnap-ignore-dir";
/// Per-directory ignore file: every non-blank, non-`#` line is a tar
/// exclusion glob, scoped to the directory the file lives in. The glob
/// flavor is exactly tar's `--exclude` default (wildcards on, `*` matches
/// `/` too); lines are passed through verbatim apart from the scoping
/// prefix and the `<uid>` placeholder.
const IGNORE_FILE: &str = ".worksnap-ignore";
/// Placeholder in ignore-file lines, replaced with the current user's uid
/// (the GNU/KDE trash dir of a mounted filesystem is `.Trash-<uid>`).
const UID_PLACEHOLDER: &str = "<uid>";
/// Placeholder in ignore-file lines, replaced with the current group's gid.
const GID_PLACEHOLDER: &str = "<gid>";

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

/// All exclude patterns for a snapshot of `source_dir`: the
/// `.worksnap-ignore-dir` markers plus the globs and conditional rules of
/// `.worksnap-ignore` files, discovered by walking the tree.
pub fn patterns(source_dir: &Path) -> Vec<OsString> {
    let uid = rustix::process::getuid().as_raw().to_string();
    let gid = rustix::process::getgid().as_raw().to_string();
    let mut patterns = Vec::new();
    let mut rules: Vec<ConditionalRule> = Vec::new();
    let mut walker = WalkDir::new(source_dir).into_iter();
    while let Some(entry) = walker.next() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(%error, "skipping unreadable path");
                continue;
            }
        };
        if !entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        collect_ignore_file(source_dir, path, &uid, &gid, &mut patterns, &mut rules);
        // The root itself can't be excluded, only scan it for an ignore file.
        if entry.depth() == 0 {
            continue;
        }
        if is_covered_by_patterns(source_dir, path, &patterns) {
            // Already excluded wholesale by a collected pattern; nothing
            // inside can change the outcome, so don't push duplicates and
            // don't waste time walking it.
            walker.skip_current_dir();
            continue;
        }
        if apply_rules(source_dir, path, &rules, &mut patterns) {
            walker.skip_current_dir();
            continue;
        }
        if path.join(IGNORE_MARKER).is_file() {
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
        }
    }
    patterns
}

/// A conditional rule from an ignore file, `<dir-glob>:<marker>:<exclude>`
/// or `<marker>:<exclude>`: in a directory where `marker` exists, `exclude`
/// (a path inside it, `.` for the directory itself) is left out.
struct ConditionalRule {
    /// Anchored (`./…`) glob selecting the candidate directories. `None` is
    /// the two-field form: the rule binds to the ignore file's own
    /// directory and is resolved as soon as the file is read.
    dir_glob: Option<String>,
    /// Path inside the candidate directory that must exist to fire.
    marker: String,
    /// What to exclude, relative to the directory; `.` is the dir itself.
    exclude: String,
}

fn collect_ignore_file(
    source_dir: &Path,
    dir: &Path,
    uid: &str,
    gid: &str,
    patterns: &mut Vec<OsString>,
    rules: &mut Vec<ConditionalRule>,
) {
    let ignore_file = dir.join(IGNORE_FILE);
    if !ignore_file.is_file() {
        return;
    }
    let content = match std::fs::read_to_string(&ignore_file) {
        Ok(content) => content,
        Err(error) => {
            tracing::warn!(%error, "cannot read {}", ignore_file.display());
            return;
        }
    };
    let Ok(Some(rel_dir)) = dir.strip_prefix(source_dir).map(Path::to_str) else {
        tracing::warn!(
            path = %dir.display(),
            "skipping the ignore file: cannot express its dir relative to the source dir"
        );
        return;
    };
    let (file_patterns, file_rules) = parse_ignore_file(rel_dir, &content, uid, gid);
    patterns.extend(file_patterns.into_iter().map(OsString::from));
    for rule in file_rules {
        if rule.dir_glob.is_some() {
            rules.push(rule);
            continue;
        }
        // Two-field form: the candidate directory is this one.
        if !dir.join(&rule.marker).exists() {
            continue;
        }
        if rule.exclude == "." {
            if rel_dir.is_empty() {
                tracing::warn!(
                    marker = rule.marker,
                    "a `<marker>:.` rule in the source root would exclude everything; skipping"
                );
                continue;
            }
            patterns.push(OsString::from(format!("./{rel_dir}")));
        } else {
            patterns.push(OsString::from(anchor(rel_dir, &rule.exclude)));
        }
    }
}

/// Parses ignore-file lines into plain tar globs and conditional rules,
/// both anchored at `rel_dir` (the file's directory relative to the source
/// root, `""` for the root itself). `uid` and `gid` fill the
/// [`UID_PLACEHOLDER`] and [`GID_PLACEHOLDER`] tokens.
fn parse_ignore_file(
    rel_dir: &str,
    content: &str,
    uid: &str,
    gid: &str,
) -> (Vec<String>, Vec<ConditionalRule>) {
    let mut patterns = Vec::new();
    let mut rules = Vec::new();
    for line in content.lines() {
        // Inline comments start at " #"; a leading `#` comments the line out.
        let line = line.split(" #").next().unwrap_or_default().trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line
            .replace(UID_PLACEHOLDER, uid)
            .replace(GID_PLACEHOLDER, gid);
        let fields: Vec<&str> = line.split(':').collect();
        match *fields.as_slice() {
            [glob] => {
                if let Some(pinned) = glob.strip_prefix('/') {
                    // "/glob" is pinned to the ignore file's own directory.
                    patterns.push(anchor(rel_dir, pinned));
                } else {
                    // A bare glob matches anywhere below the ignore file's
                    // directory, like tar's own unanchored patterns:
                    // directly here, and at any deeper level via `*/`
                    // (tar's exclusion `*` crosses `/`).
                    patterns.push(anchor(rel_dir, glob));
                    patterns.push(anchor(rel_dir, &format!("*/{glob}")));
                }
            }
            [.., marker, exclude] if fields.len() <= 3 => {
                let dir_glob = (fields.len() == 3).then(|| trim_rule_part(fields[0]));
                let marker = trim_rule_part(marker);
                let exclude = trim_rule_part(exclude);
                if marker.is_empty() || exclude.is_empty() || dir_glob == Some("") {
                    tracing::warn!(
                        line,
                        "malformed rule: every [<dir-glob>:]<marker>:<path> part must be non-empty"
                    );
                    continue;
                }
                rules.push(ConditionalRule {
                    dir_glob: dir_glob.map(|dir_glob| anchor(rel_dir, dir_glob)),
                    marker: marker.to_owned(),
                    exclude: exclude.to_owned(),
                });
            }
            _ => tracing::warn!(
                line,
                "malformed ignore line: expected a glob or [<dir-glob>:]<marker>:<path>"
            ),
        }
    }
    (patterns, rules)
}

fn trim_rule_part(part: &str) -> &str {
    if part == "." {
        "."
    } else {
        part.trim_start_matches('/').trim_end_matches('/')
    }
}

fn anchor(rel_dir: &str, glob: &str) -> String {
    if rel_dir.is_empty() {
        format!("./{glob}")
    } else {
        format!("./{rel_dir}/{glob}")
    }
}

/// Applies the conditional rules to one directory, pushing an exclude for
/// every rule that fires. Returns true when the directory itself got
/// excluded, so the caller can prune the walk.
fn apply_rules(
    source_dir: &Path,
    dir: &Path,
    rules: &[ConditionalRule],
    patterns: &mut Vec<OsString>,
) -> bool {
    if rules.is_empty() {
        return false;
    }
    let Ok(Some(rel)) = dir.strip_prefix(source_dir).map(Path::to_str) else {
        return false;
    };
    let rel = format!("./{rel}");
    let mut excluded_self = false;
    for rule in rules {
        let Some(dir_glob) = &rule.dir_glob else {
            // Two-field rules are resolved by `collect_ignore_file` and
            // never reach the walk's rule list.
            continue;
        };
        if !glob_match(dir_glob, &rel) || !dir.join(&rule.marker).exists() {
            continue;
        }
        if rule.exclude == "." {
            patterns.push(OsString::from(rel.clone()));
            excluded_self = true;
        } else {
            patterns.push(OsString::from(format!("{rel}/{}", rule.exclude)));
        }
    }
    excluded_self
}

/// True when tar will exclude this directory wholesale because one of the
/// already-collected patterns matches it, so walking inside it can't change
/// the outcome. Purely an optimization: a miss here only costs walk time.
fn is_covered_by_patterns(source_dir: &Path, path: &Path, patterns: &[OsString]) -> bool {
    let Ok(rel) = path.strip_prefix(source_dir) else {
        return false;
    };
    let rel = format!("./{}", rel.display());
    let name = path.file_name().map(OsStr::to_string_lossy);
    patterns.iter().any(|pattern| {
        let Some(pattern) = pattern.to_str() else {
            return false;
        };
        if pattern.starts_with("./") {
            glob_match(pattern, &rel)
        } else {
            // An unanchored pattern (like the trash entry) matches any name
            // component; for pruning only the directory's own name matters.
            name.as_deref()
                .is_some_and(|name| glob_match(pattern, name))
        }
    })
}

/// Minimal fnmatch for pruning decisions: `*` (matching `/` too, exactly as
/// tar's exclusion globs do) and `?`. Character classes are not supported —
/// a pattern using them never prunes, it only costs walk time.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0usize, 0usize);
    let mut backtrack: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                backtrack = Some((p, t));
                p += 1;
            }
            Some('?') => {
                p += 1;
                t += 1;
            }
            Some(literal) if *literal == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match backtrack {
                Some((star_p, star_t)) => {
                    backtrack = Some((star_p, star_t + 1));
                    p = star_p + 1;
                    t = star_t + 1;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|tail| *tail == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_globs_match_anywhere_below_and_slashed_ones_pin_to_the_file_dir() {
        let content = "target2\n/build\n*.log\n";
        let (patterns, rules) = parse_ignore_file("proj/sub", content, "1000", "984");
        assert_eq!(
            patterns,
            vec![
                "./proj/sub/target2",
                "./proj/sub/*/target2",
                "./proj/sub/build",
                "./proj/sub/*.log",
                "./proj/sub/*/*.log",
            ]
        );
        assert!(rules.is_empty());
    }

    #[test]
    fn ignore_file_globs_at_the_source_root_anchor_to_dot() {
        let (patterns, _) = parse_ignore_file("", "/big-dir\n", "1000", "984");
        assert_eq!(patterns, vec!["./big-dir"]);
    }

    #[test]
    fn conditional_rules_are_parsed_and_scoped() {
        let content = "\
*/a/b/:c/file.txt:d/ # inline comment
*/:Cargo.toml:.
two-fields:is-ok
";
        let (patterns, rules) = parse_ignore_file("sub", content, "1000", "984");
        assert!(patterns.is_empty());
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].dir_glob.as_deref(), Some("./sub/*/a/b"));
        assert_eq!(rules[0].marker, "c/file.txt");
        assert_eq!(rules[0].exclude, "d");
        assert_eq!(rules[1].dir_glob.as_deref(), Some("./sub/*"));
        assert_eq!(rules[1].marker, "Cargo.toml");
        assert_eq!(rules[1].exclude, ".");
        // Two fields: a rule bound to the ignore file's own directory.
        assert_eq!(rules[2].dir_glob, None);
        assert_eq!(rules[2].marker, "two-fields");
        assert_eq!(rules[2].exclude, "is-ok");
    }

    #[test]
    fn glob_match_star_crosses_slashes_like_tar_exclusion_globs() {
        assert!(glob_match("./*/node_modules", "./a/b/node_modules"));
        assert!(glob_match("./node_modules", "./node_modules"));
        assert!(!glob_match("./node_modules", "./a/node_modules"));
        assert!(glob_match("dhat.out.*", "dhat.out.1234"));
        assert!(glob_match("?env", "venv"));
        assert!(!glob_match("./go/pkg", "./go/pkgx"));
        assert!(glob_match("./go/*", "./go/pkg"));
    }

    #[test]
    fn uid_and_gid_placeholders_expand_in_any_line_kind() {
        let content = ".Trash-<uid>\n*/:marker-<uid>:junk-<gid>/\n";
        let (patterns, rules) = parse_ignore_file("", content, "1000", "984");
        assert_eq!(patterns, vec!["./.Trash-1000", "./*/.Trash-1000"]);
        assert_eq!(rules[0].marker, "marker-1000");
        assert_eq!(rules[0].exclude, "junk-984");
    }

    #[test]
    fn ignore_file_lines_skip_blanks_and_comments() {
        let content = "\n# a comment\n  \n/build\n/keep # trailing comment\n";
        let (patterns, _) = parse_ignore_file("proj", content, "1000", "984");
        assert_eq!(patterns, vec!["./proj/build", "./proj/keep"]);
    }
}
