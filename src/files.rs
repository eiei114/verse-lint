use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

pub const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SOURCE_FILES: usize = 10_000;
pub const MAX_VISITED_ENTRIES: usize = 100_000;
pub const MAX_DIRECTORY_DEPTH: usize = 128;
pub const MAX_WALK_ERRORS: usize = 100;
pub const MAX_DIRECTORY_ENTRIES: usize = 100_000;
/// The pattern bound shared by `[files] exclude` and `verse-lint.ignore`. The two
/// sources have separate budgets, so `docs/cli.md` states one number for each.
pub const MAX_EXCLUDE_GLOBS: usize = 256;
pub const MAX_EXCLUDE_GLOB_BYTES: usize = 4_096;
/// `docs/cli.md`: "256 KiB config/ignore file". Both readers below enforce it.
pub const MAX_IGNORE_BYTES: usize = 256 * 1024;
/// The one ignore file `docs/cli.md` documents, read from the configuration root
/// only. No parent search, no nested files, no user-level file.
pub const IGNORE_FILE_NAME: &str = "verse-lint.ignore";

/// `docs/cli.md`: "64 MiB total successfully read input". Kept as a predicate
/// so both sides of the boundary are testable without allocating 64 MiB.
pub fn total_bytes_exceeded(total: usize) -> bool {
    total > MAX_TOTAL_BYTES
}

/// Why `pattern` is not a usable relative glob.
///
/// `[files] exclude` and `verse-lint.ignore` share this check, so the two sources
/// reject exactly the same forms and `docs/cli.md` documents one list of them.
fn rejection(pattern: &str) -> Option<&'static str> {
    if pattern.is_empty() {
        Some("an empty entry")
    } else if pattern.starts_with('!') {
        Some("negation")
    } else if pattern.starts_with('/') {
        Some("an absolute path")
    } else if pattern.contains('\\') {
        Some("a backslash")
    } else if pattern.contains(':') {
        Some("a drive letter or colon")
    } else if pattern.len() > MAX_EXCLUDE_GLOB_BYTES {
        Some("an entry longer than 4,096 bytes")
    } else {
        None
    }
}

fn build_glob(pattern: &str) -> Result<Glob, String> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .case_insensitive(cfg!(windows))
        .build()
        .map_err(|e| e.to_string())
}

pub fn excludes(patterns: &[String]) -> Result<GlobSet, String> {
    if patterns.len() > MAX_EXCLUDE_GLOBS {
        return Err("at most 256 exclude globs are supported".into());
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Some(reason) = rejection(pattern) {
            return Err(format!(
                "invalid exclude glob {pattern:?}: use a relative forward-slash pattern without negation ({reason})"
            ));
        }
        builder.add(build_glob(pattern)?);
    }
    builder.build().map_err(|e| e.to_string())
}

/// A validated `verse-lint.ignore` from the configuration root.
///
/// Every pattern here came from that one file; nothing on the command line or in
/// another directory can add to it. `--no-ignore-file` is handled by the caller
/// never loading it.
#[derive(Debug)]
pub struct IgnoreFile {
    pub path: PathBuf,
    pub patterns: Vec<String>,
}

/// Reads `<root>/verse-lint.ignore`, or `None` when it does not exist.
///
/// A missing file is not an error. An unreadable, oversized, non-UTF-8 or
/// invalid file is, and every message names the path (and, per entry, the line)
/// so the problem is locatable without guessing.
pub fn load_ignore_file(root: &Path) -> Result<Option<IgnoreFile>, String> {
    let path = root.join(IGNORE_FILE_NAME);
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    if !metadata.is_file() {
        return Err(format!("{}: expected a regular file", path.display()));
    }
    let oversized = || format!("{}: {IGNORE_FILE_NAME} exceeds 256 KiB", path.display());
    if metadata.len() > MAX_IGNORE_BYTES as u64 {
        return Err(oversized());
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .and_then(|file| {
            file.take((MAX_IGNORE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() > MAX_IGNORE_BYTES {
        return Err(oversized());
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| format!("{}: {IGNORE_FILE_NAME} must be UTF-8", path.display()))?;
    let patterns = parse_ignore_file(text.trim_start_matches('\u{feff}'))
        .map_err(|(line, message)| format!("{}:{line}: {message}", path.display()))?;
    Ok(Some(IgnoreFile { path, patterns }))
}

/// One relative forward-slash glob per line. Blank lines and lines whose first
/// non-blank character is `#` are skipped; each entry is trimmed; a `#` that is
/// not leading belongs to the pattern. Failures carry the offending line number.
fn parse_ignore_file(text: &str) -> Result<Vec<String>, (usize, String)> {
    let mut patterns = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let entry = raw.trim();
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        if let Some(reason) = rejection(entry) {
            return Err((line, format!("invalid ignore pattern {entry:?}: {reason}")));
        }
        build_glob(entry)
            .map_err(|error| (line, format!("invalid ignore pattern {entry:?}: {error}")))?;
        patterns.push(entry.to_string());
        if patterns.len() > MAX_EXCLUDE_GLOBS {
            return Err((
                line,
                format!("at most {MAX_EXCLUDE_GLOBS} ignore patterns are supported"),
            ));
        }
    }
    Ok(patterns)
}

pub fn label(path: &Path, root: &Path) -> Result<String, String> {
    let relative = pathdiff::diff_paths(path, root).unwrap_or_else(|| path.to_path_buf());
    relative
        .to_str()
        .map(|p| p.replace('\\', "/"))
        .ok_or_else(|| "path cannot be represented as Unicode".into())
}

pub fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub fn guard_path(path: &Path) -> Result<(), String> {
    let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
    for part in absolute.ancestors() {
        let metadata =
            fs::symlink_metadata(part).map_err(|e| format!("{}: {e}", part.display()))?;
        if is_link(&metadata) {
            return Err(format!(
                "{}: symlinks/junctions/reparse points are unsupported",
                part.display()
            ));
        }
    }
    Ok(())
}

fn protected(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::Normal(name) if [".git", "Intermediate", "Saved"].iter().any(|p| name.to_string_lossy().eq_ignore_ascii_case(p))))
        || path.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().ends_with(".digest.verse"))
}

fn hidden(path: &Path, metadata: &fs::Metadata) -> bool {
    let dot = path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'));
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        dot || metadata.file_attributes() & 2 != 0
    }
    #[cfg(not(windows))]
    {
        dot
    }
}

fn add_ignore(directory: &Path, ignores: &mut Vec<Gitignore>) -> Result<bool, String> {
    let file = directory.join(".gitignore");
    if !file.try_exists().map_err(|e| e.to_string())? {
        return Ok(false);
    }
    let size = fs::metadata(&file).map_err(|e| e.to_string())?.len();
    if size > MAX_IGNORE_BYTES as u64 {
        return Err(format!("{}: .gitignore exceeds 256 KiB", file.display()));
    }
    let mut builder = GitignoreBuilder::new(directory);
    if let Some(e) = builder.add(&file) {
        return Err(format!("{}: {e}", file.display()));
    }
    ignores.push(builder.build().map_err(|e| e.to_string())?);
    Ok(true)
}

#[derive(Default)]
pub struct Discovery {
    pub paths: Vec<PathBuf>,
    pub errors: Vec<String>,
    pub excluded: Vec<String>,
}

/// The merged exclusion rules.
///
/// `[files] exclude` and `verse-lint.ignore` are a union: a path matching either
/// source is skipped, and neither silently overrides the other. They stay two
/// matchers so `--verbose` can name the source that matched, and so an explicit
/// argument keeps its documented behaviour per source: a config match is still
/// refused, an ignore-file or gitignore match is bypassed.
struct Exclusions {
    config: GlobSet,
    ignore_file: GlobSet,
}

impl Exclusions {
    fn build(config: &[String], ignore_file: &[String]) -> Result<Self, String> {
        Ok(Self {
            config: excludes(config)?,
            ignore_file: excludes(ignore_file)?,
        })
    }
}

struct Walker<'a> {
    root: &'a Path,
    exclusions: Exclusions,
    seen: HashSet<PathBuf>,
    visited: usize,
    result: Discovery,
}

impl Walker<'_> {
    fn visit(
        &mut self,
        path: &Path,
        explicit: bool,
        ignores: &mut Vec<Gitignore>,
        depth: usize,
    ) -> Result<(), String> {
        self.visited += 1;
        if self.visited > MAX_VISITED_ENTRIES || depth > MAX_DIRECTORY_DEPTH {
            return Err("directory traversal resource limit exceeded".into());
        }
        let metadata =
            fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let shown = label(path, self.root)?;
        let mut reason = None;
        if is_link(&metadata) {
            reason = Some("symlink/junction/reparse point");
        } else if protected(Path::new(&shown)) {
            reason = Some("generated file/directory or Git metadata protection");
        } else if self.exclusions.config.is_match(&shown) {
            reason = Some("verse.toml exclude");
        } else if !explicit {
            if hidden(path, &metadata) {
                reason = Some("hidden entry");
            } else if self.exclusions.ignore_file.is_match(&shown) {
                reason = Some("verse-lint.ignore exclude");
            } else {
                let mut ignored = false;
                for matcher in ignores.iter() {
                    let matched = matcher.matched_path_or_any_parents(path, metadata.is_dir());
                    if matched.is_ignore() {
                        ignored = true;
                    } else if matched.is_whitelist() {
                        ignored = false;
                    }
                }
                if ignored {
                    reason = Some(".gitignore");
                }
            }
        }
        if let Some(reason) = reason {
            let message = format!("{shown}: excluded ({reason})");
            if explicit {
                return Err(message);
            }
            self.result.excluded.push(message);
            return Ok(());
        }
        if metadata.is_dir() {
            let added = add_ignore(path, ignores)?;
            let mut entries = fs::read_dir(path)
                .map_err(|e| format!("{shown}: {e}"))?
                .take(MAX_DIRECTORY_ENTRIES + 1)
                .map(|entry| entry.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("{shown}: {e}"))?;
            if entries.len() > MAX_DIRECTORY_ENTRIES {
                return Err("directory entry resource limit exceeded".into());
            }
            entries.sort();
            for child in entries {
                if let Err(e) = self.visit(&child, false, ignores, depth + 1) {
                    self.result.errors.push(e);
                }
                if self.visited > MAX_VISITED_ENTRIES || self.result.errors.len() >= MAX_WALK_ERRORS
                {
                    break;
                }
            }
            if added {
                ignores.pop();
            }
        } else if metadata.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("verse"))
        {
            if !self.seen.contains(path) {
                if self.result.paths.len() >= MAX_SOURCE_FILES {
                    return Err("at most 10000 source files are supported".into());
                }
                self.seen.insert(path.to_path_buf());
                self.result.paths.push(path.to_path_buf());
            }
        } else if explicit {
            return Err(format!("{shown}: expected a .verse file or directory"));
        }
        Ok(())
    }
}

pub fn discover(
    inputs: &[PathBuf],
    root: &Path,
    patterns: &[String],
    ignore: Option<&IgnoreFile>,
) -> Result<Discovery, String> {
    let mut walker = Walker {
        root,
        exclusions: Exclusions::build(
            patterns,
            ignore.map(|file| file.patterns.as_slice()).unwrap_or(&[]),
        )?,
        seen: HashSet::new(),
        visited: 0,
        result: Discovery::default(),
    };
    for input in inputs {
        let result = (|| {
            guard_path(input)?;
            let path = fs::canonicalize(input).map_err(|e| format!("{}: {e}", input.display()))?;
            let mut ancestors = Vec::new();
            // A traversal root that itself contains .git must not inherit ignores above that root.
            if !path.is_dir() || !path.join(".git").exists() {
                for ancestor in path.parent().into_iter().flat_map(Path::ancestors) {
                    ancestors.push(ancestor);
                    if ancestor.join(".git").exists() {
                        break;
                    }
                }
            }
            let mut ignores = Vec::new();
            for ancestor in ancestors.into_iter().rev() {
                add_ignore(ancestor, &mut ignores)?;
            }
            walker.visit(&path, true, &mut ignores, 0)
        })();
        if let Err(e) = result {
            walker.result.errors.push(e);
        }
        if walker.visited > MAX_VISITED_ENTRIES || walker.result.errors.len() >= MAX_WALK_ERRORS {
            break;
        }
    }
    walker
        .result
        .paths
        .sort_by_cached_key(|p| label(p, root).unwrap_or_default());
    if walker.result.paths.is_empty() {
        walker
            .result
            .errors
            .push("no eligible .verse files found".into());
    }
    Ok(walker.result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walker(visited: usize) -> Walker<'static> {
        Walker {
            root: Path::new("."),
            exclusions: Exclusions::build(&[], &[]).unwrap(),
            seen: HashSet::new(),
            visited,
            result: Discovery::default(),
        }
    }

    #[test]
    fn visited_entry_limit_fails_before_accessing_another_path() {
        let mut walker = walker(MAX_VISITED_ENTRIES);
        let error = walker
            .visit(
                Path::new("path-is-not-read-after-limit"),
                false,
                &mut Vec::new(),
                0,
            )
            .unwrap_err();

        assert!(error.contains("directory traversal resource limit"));
        assert_eq!(walker.visited, MAX_VISITED_ENTRIES + 1);
    }

    #[test]
    fn directory_depth_limit_fails_before_accessing_another_path() {
        let mut walker = walker(0);
        let error = walker
            .visit(
                Path::new("path-is-not-read-after-limit"),
                false,
                &mut Vec::new(),
                MAX_DIRECTORY_DEPTH + 1,
            )
            .unwrap_err();

        assert!(error.contains("directory traversal resource limit"));
        assert_eq!(walker.visited, 1);
    }

    /// The documented boundaries are inclusive: 100,000 visited entries and
    /// 128 directory levels are inspected, one past either is refused.
    #[test]
    fn visited_and_depth_limits_accept_the_documented_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.verse");
        fs::write(&file, b"A := 1\n").unwrap();

        let mut at_limit = walker(MAX_VISITED_ENTRIES - 1);
        at_limit
            .visit(&file, true, &mut Vec::new(), MAX_DIRECTORY_DEPTH)
            .unwrap();
        assert_eq!(at_limit.result.paths, vec![file.clone()]);
        assert_eq!(at_limit.visited, MAX_VISITED_ENTRIES);

        let mut past_limit = walker(MAX_VISITED_ENTRIES - 1);
        assert!(
            past_limit
                .visit(&file, true, &mut Vec::new(), MAX_DIRECTORY_DEPTH + 1)
                .is_err()
        );
    }

    /// `docs/cli.md` advertises at most 10,000 source files. The walker refuses
    /// the next file only when 10,000 are already discovered.
    #[test]
    fn source_file_limit_accepts_the_10000th_file_and_refuses_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.verse");
        let second = dir.path().join("b.verse");
        fs::write(&first, b"A := 1\n").unwrap();
        fs::write(&second, b"B := 1\n").unwrap();

        let mut walker = walker(0);
        walker.result.paths = (0..MAX_SOURCE_FILES - 1)
            .map(|index| dir.path().join(format!("seed-{index}.verse")))
            .collect();
        walker
            .visit(&first, true, &mut Vec::new(), 0)
            .expect("the 10,000th source file is accepted");
        assert_eq!(walker.result.paths.len(), MAX_SOURCE_FILES);

        let error = walker.visit(&second, true, &mut Vec::new(), 0).unwrap_err();
        assert!(error.contains("at most 10000 source files"));
        assert_eq!(walker.result.paths.len(), MAX_SOURCE_FILES);
    }

    #[test]
    fn total_read_limit_predicate_matches_the_documented_64_mib_boundary() {
        assert!(!total_bytes_exceeded(0));
        assert!(!total_bytes_exceeded(MAX_TOTAL_BYTES - 1));
        assert!(!total_bytes_exceeded(MAX_TOTAL_BYTES));
        assert!(total_bytes_exceeded(MAX_TOTAL_BYTES + 1));
    }

    /// `docs/cli.md` records 256 exclude globs of up to 4,096 bytes each. Both
    /// bounds are inclusive.
    #[test]
    fn exclude_glob_count_and_size_boundaries_are_inclusive() {
        let count = |count: usize| {
            (0..count)
                .map(|index| format!("{index}/**"))
                .collect::<Vec<_>>()
        };
        assert!(excludes(&count(MAX_EXCLUDE_GLOBS)).is_ok());
        let error = excludes(&count(MAX_EXCLUDE_GLOBS + 1)).unwrap_err();
        assert!(error.contains("at most 256 exclude globs"));

        let at_limit = vec!["a".repeat(MAX_EXCLUDE_GLOB_BYTES)];
        assert_eq!(at_limit[0].len(), MAX_EXCLUDE_GLOB_BYTES);
        assert!(excludes(&at_limit).is_ok());

        let past_limit = vec!["a".repeat(MAX_EXCLUDE_GLOB_BYTES + 1)];
        let error = excludes(&past_limit).unwrap_err();
        assert!(error.contains("without negation"));
    }

    /// `docs/cli.md` advertises a 256 KiB ignore-file limit. Exactly 256 KiB is
    /// applied; one byte more is refused rather than silently truncated.
    #[test]
    fn ignore_file_size_limit_is_inclusive_at_256_kib() {
        let dir = tempfile::tempdir().unwrap();
        let ignore = dir.path().join(".gitignore");

        fs::write(&ignore, vec![b'#'; 256 * 1024]).unwrap();
        assert!(add_ignore(dir.path(), &mut Vec::new()).unwrap());

        fs::write(&ignore, vec![b'#'; 256 * 1024 + 1]).unwrap();
        let error = add_ignore(dir.path(), &mut Vec::new()).unwrap_err();
        assert!(error.contains(".gitignore exceeds 256 KiB"));
    }

    /// A walk stops after 100 execution errors; files discovered before the cap
    /// keep their place so they are still inspected.
    #[test]
    fn walk_stops_after_100_errors_but_keeps_files_discovered_before_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("a-good.verse");
        fs::write(&good, b"A := 1\n").unwrap();
        let canonical = fs::canonicalize(&good).unwrap();

        let mut before_cap = vec![good.clone()];
        for index in 0..MAX_WALK_ERRORS {
            before_cap.push(dir.path().join(format!("absent-{index}.verse")));
        }
        let discovered = discover(&before_cap, dir.path(), &[], None).unwrap();
        assert_eq!(discovered.errors.len(), MAX_WALK_ERRORS);
        assert_eq!(discovered.paths, vec![canonical]);

        // Once the cap is reached, later inputs are never visited, so the good
        // file after the errors is neither discovered nor inspected.
        let mut after_cap: Vec<_> = (0..MAX_WALK_ERRORS)
            .map(|index| dir.path().join(format!("absent-{index}.verse")))
            .collect();
        after_cap.push(good);
        let discovered = discover(&after_cap, dir.path(), &[], None).unwrap();
        assert!(discovered.paths.is_empty());
        assert!(discovered.errors.len() > MAX_WALK_ERRORS);
        assert!(
            discovered
                .errors
                .iter()
                .any(|error| error.contains("no eligible .verse files found")),
            "a truncated walk must not claim it inspected everything"
        );
    }

    /// Comment, blank and whitespace handling is exact: only a leading `#` starts
    /// a comment, so a `#` inside a pattern stays part of it, and surrounding
    /// whitespace never reaches the glob.
    #[test]
    fn ignore_file_skips_comments_and_blanks_and_keeps_an_inner_hash() {
        let text = "\n   \n# a comment\n   # indented comment\n  Vendor/**  \nbuild/#cache.verse\n\tContent/Generated/*.verse\t\n";
        assert_eq!(
            parse_ignore_file(text).unwrap(),
            vec![
                "Vendor/**".to_string(),
                "build/#cache.verse".to_string(),
                "Content/Generated/*.verse".to_string(),
            ]
        );
        assert!(parse_ignore_file("").unwrap().is_empty());
        assert!(parse_ignore_file("# only a comment\n").unwrap().is_empty());
    }

    /// Every rejected form names the offending line, so a reader can find it in
    /// a long file without counting.
    #[test]
    fn ignore_file_rejections_name_the_line_number() {
        let cases = [
            ("Vendor/**\n!kept.verse\n", 2, "negation"),
            ("/abs/path/**\n", 1, "an absolute path"),
            ("C:/project/**\n", 1, "a drive letter or colon"),
            ("Vendor\\**\n", 1, "a backslash"),
        ];
        for (text, line, reason) in cases {
            let (reported, message) = parse_ignore_file(text).unwrap_err();
            assert_eq!(reported, line, "{text:?}: {message}");
            assert!(message.contains(reason), "{text:?}: {message}");
        }

        let over_long = format!("a\n{}\n", "b".repeat(MAX_EXCLUDE_GLOB_BYTES + 1));
        let (reported, message) = parse_ignore_file(&over_long).unwrap_err();
        assert_eq!(reported, 2, "{message}");
        assert!(message.contains("4,096 bytes"), "{message}");

        // A malformed glob is rejected at its own line too, not as a glob error
        // from the globset builder without context.
        let (reported, message) = parse_ignore_file("ok/**\n[unclosed\n").unwrap_err();
        assert_eq!(reported, 2, "{message}");
        assert!(message.contains("invalid ignore pattern"), "{message}");
    }

    /// The ignore file holds at most 256 effective patterns; comments and blanks
    /// never consume the budget, and the 257th entry names its own line.
    #[test]
    fn ignore_file_pattern_count_boundary_is_inclusive() {
        let at_limit = (0..MAX_EXCLUDE_GLOBS)
            .map(|index| format!("# comment {index}\n{index}/**\n"))
            .collect::<String>();
        assert_eq!(
            parse_ignore_file(&at_limit).unwrap().len(),
            MAX_EXCLUDE_GLOBS
        );

        let past_limit = format!("{at_limit}{MAX_EXCLUDE_GLOBS}/**\n");
        let (line, message) = parse_ignore_file(&past_limit).unwrap_err();
        assert_eq!(line, MAX_EXCLUDE_GLOBS * 2 + 1, "{message}");
        assert!(message.contains("at most 256 ignore patterns"), "{message}");
    }

    /// A missing file is not an error; an oversized one is, and it is refused
    /// before its contents are parsed.
    #[test]
    fn ignore_file_presence_and_size_are_distinguished() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_ignore_file(dir.path()).unwrap().is_none());

        let path = dir.path().join(IGNORE_FILE_NAME);
        fs::write(&path, "Vendor/**\n").unwrap();
        let loaded = load_ignore_file(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.path, path);
        assert_eq!(loaded.patterns, vec!["Vendor/**".to_string()]);

        fs::write(&path, vec![b'#'; MAX_IGNORE_BYTES + 1]).unwrap();
        let error = load_ignore_file(dir.path()).unwrap_err();
        assert!(error.contains("exceeds 256 KiB"), "{error}");
        assert!(error.contains(IGNORE_FILE_NAME), "{error}");

        fs::write(&path, b"Vendor/**\n!kept.verse\n").unwrap();
        let error = load_ignore_file(dir.path()).unwrap_err();
        assert!(
            error.contains("verse-lint.ignore:2:") || error.contains("verse-lint.ignore:2 "),
            "the line number must be reported: {error}"
        );
    }
}
