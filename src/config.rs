use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{files, rules};

/// `docs/cli.md`: "256 KiB config/ignore file". The same bound applies to the
/// ignore readers in [`crate::files`].
pub const MAX_CONFIG_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Files {
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Settings {
    pub schema_version: u32,
    pub files: Files,
    pub lint: rules::Settings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<toml::Table>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            files: Files::default(),
            lint: rules::Settings::default(),
            format: None,
        }
    }
}

pub struct Config {
    pub source: Option<PathBuf>,
    pub root: PathBuf,
    pub settings: Settings,
    /// `verse-lint.ignore` at [`Config::root`], when it exists and the caller did
    /// not disable it with `--no-ignore-file`.
    pub ignore_file: Option<files::IgnoreFile>,
    /// Whether `--no-ignore-file` suppressed the file, so `--show-config` can
    /// report "disabled" instead of the indistinguishable "not found".
    pub ignore_file_disabled: bool,
}

/// Resolves the configuration and the one ignore file it owns.
///
/// The ignore file is loaded here, before any discovery, so a rejected entry or
/// an oversized file fails the run without inspecting one source. `disabled`
/// comes from `--no-ignore-file`: the file is then never opened, so a broken
/// file cannot fail a run that asked not to use it.
pub fn resolve(
    explicit: Option<&Path>,
    virtual_path: Option<&Path>,
    disabled: bool,
) -> Result<Config, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let source = if let Some(path) = explicit {
        Some(
            std::fs::canonicalize(path)
                .map_err(|e| format!("configuration {}: {e}", path.display()))?,
        )
    } else {
        let from = virtual_path.map(|p| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd.join(p)
            }
        });
        let start = from.as_ref().and_then(|p| p.parent()).unwrap_or(&cwd);
        let mut found = None;
        for parent in start.ancestors() {
            let candidate = parent.join("verse.toml");
            if candidate
                .try_exists()
                .map_err(|e| format!("configuration {}: {e}", candidate.display()))?
            {
                found = Some(std::fs::canonicalize(&candidate).map_err(|e| e.to_string())?);
                break;
            }
            if parent
                .join(".git")
                .try_exists()
                .map_err(|e| e.to_string())?
            {
                break;
            }
        }
        found
    };
    let root = std::fs::canonicalize(source.as_deref().and_then(Path::parent).unwrap_or(&cwd))
        .map_err(|e| e.to_string())?;
    let settings = if let Some(path) = &source {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|e| format!("configuration {}: {e}", path.display()))?
            .take((MAX_CONFIG_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err("configuration exceeds 256 KiB".into());
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| "configuration must be UTF-8")?
            .trim_start_matches('\u{feff}');
        toml::from_str::<Settings>(text)
            .map_err(|e| format!("configuration {}: {e}", path.display()))?
    } else {
        Settings::default()
    };
    if settings.schema_version != 1 {
        return Err(format!(
            "unsupported schema-version {}; expected 1",
            settings.schema_version
        ));
    }
    // Validate globs even in --show-config mode or when no files would match.
    crate::files::excludes(&settings.files.exclude)?;
    settings.lint.validate()?;
    let ignore_file = if disabled {
        None
    } else {
        files::load_ignore_file(&root)?
    };
    Ok(Config {
        source,
        root,
        settings,
        ignore_file,
        ignore_file_disabled: disabled,
    })
}

impl Config {
    pub fn show(&self) -> Result<String, String> {
        let origin = self
            .source
            .as_ref()
            .map_or_else(|| "<defaults>".into(), |p| p.display().to_string());
        // Render the effective severity of every registered rule, not just the
        // configured overrides, so a user can confirm the result without
        // reconstructing registry defaults. `BTreeMap` ordering keeps the
        // emitted table independent of the order of keys in the source file.
        let mut settings = self.settings.clone();
        settings.lint.severity = settings.lint.effective_severity()?;
        // Print the expanded complete IDs, never a prefix: the point of
        // `--show-config` is to make the effective set unambiguous.
        settings.lint.select = settings.lint.expanded_select()?;
        settings.lint.ignore = settings.lint.expanded_ignore()?;
        // Presence, path and the merged effective count are reported as a header
        // comment so the TOML body keeps parsing as configuration. The count is
        // the union both sources contribute, which is what actually excludes.
        let ignore = self.ignore_report();
        Ok(format!(
            "# source: {origin:?}\n# root: {:?}\n{ignore}\n{}",
            self.root,
            toml::to_string_pretty(&settings).map_err(|e| e.to_string())?
        ))
    }

    /// One deterministic line stating whether an ignore file was found, where it
    /// is and how many patterns exclusion actually applies.
    fn ignore_report(&self) -> String {
        let effective = self.settings.files.exclude.len()
            + self
                .ignore_file
                .as_ref()
                .map_or(0, |file| file.patterns.len());
        match (&self.ignore_file, self.ignore_file_disabled) {
            (_, true) => format!(
                "# ignore file: disabled by --no-ignore-file ({effective} effective pattern(s) from [files] exclude only)"
            ),
            (Some(file), false) => format!(
                "# ignore file: {:?} ({} pattern(s), {effective} effective)",
                file.path,
                file.patterns.len()
            ),
            (None, false) => format!(
                "# ignore file: none at {:?} (0 pattern(s), {effective} effective)",
                self.root.join(files::IGNORE_FILE_NAME)
            ),
        }
    }
}
