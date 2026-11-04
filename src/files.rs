use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

pub const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SOURCE_FILES: usize = 10_000;
pub const MAX_VISITED_ENTRIES: usize = 100_000;
pub const MAX_DIRECTORY_DEPTH: usize = 128;
pub const MAX_WALK_ERRORS: usize = 100;
pub const MAX_DIRECTORY_ENTRIES: usize = 100_000;
pub const MAX_EXCLUDE_GLOBS: usize = 256;
pub const MAX_EXCLUDE_GLOB_BYTES: usize = 4_096;

/// `docs/cli.md`: "64 MiB total successfully read input". Kept as a predicate
/// so both sides of the boundary are testable without allocating 64 MiB.
pub fn total_bytes_exceeded(total: usize) -> bool {
    total > MAX_TOTAL_BYTES
}

pub fn excludes(patterns: &[String]) -> Result<GlobSet, String> {
    if patterns.len() > MAX_EXCLUDE_GLOBS {
        return Err("at most 256 exclude globs are supported".into());
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if pattern.is_empty()
            || pattern.starts_with('!')
            || pattern.starts_with('/')
            || pattern.contains(['\\', ':'])
            || pattern.len() > MAX_EXCLUDE_GLOB_BYTES
        {
            return Err(format!(
                "invalid exclude glob {pattern:?}: use a relative forward-slash pattern without negation"
            ));
        }
        builder.add(
            GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .case_insensitive(cfg!(windows))
                .build()
                .map_err(|e| e.to_string())?,
        );
    }
    builder.build().map_err(|e| e.to_string())
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
    if size > 256 * 1024 {
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

struct Walker<'a> {
    root: &'a Path,
    excludes: GlobSet,
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
        } else if self.excludes.is_match(&shown) {
            reason = Some("verse.toml exclude");
        } else if !explicit {
            if hidden(path, &metadata) {
                reason = Some("hidden entry");
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

pub fn discover(inputs: &[PathBuf], root: &Path, patterns: &[String]) -> Result<Discovery, String> {
    let mut walker = Walker {
        root,
        excludes: excludes(patterns)?,
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
            excludes: GlobSetBuilder::new().build().unwrap(),
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
        let discovered = discover(&before_cap, dir.path(), &[]).unwrap();
        assert_eq!(discovered.errors.len(), MAX_WALK_ERRORS);
        assert_eq!(discovered.paths, vec![canonical]);

        // Once the cap is reached, later inputs are never visited, so the good
        // file after the errors is neither discovered nor inspected.
        let mut after_cap: Vec<_> = (0..MAX_WALK_ERRORS)
            .map(|index| dir.path().join(format!("absent-{index}.verse")))
            .collect();
        after_cap.push(good);
        let discovered = discover(&after_cap, dir.path(), &[]).unwrap();
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
}
