//! Resolve the Obsidian vault and daily-notes plugin settings.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::Deserialize;

use crate::format;

pub const VAULT_ENV: &str = "OBSIDIAN_VAULT_ROOT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vault {
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyNotesConfig {
    /// Relative folder under the vault root (may be empty).
    pub folder: String,
    /// Moment-style date format used for the note path/name.
    pub format: String,
    /// Optional template note path relative to the vault root (no `.md`).
    pub template: Option<String>,
}

impl Default for DailyNotesConfig {
    fn default() -> Self {
        Self {
            folder: String::new(),
            format: "YYYY-MM-DD".to_string(),
            template: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct DailyNotesJson {
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    MissingEnv,
    EmptyEnv,
    NotADirectory(PathBuf),
    Io(String),
    BadFormat(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingEnv => write!(
                f,
                "{VAULT_ENV} is not set (or pass --vault / set vaultPath in bar settings)"
            ),
            Self::EmptyEnv => write!(f, "vault path is empty"),
            Self::NotADirectory(p) => write!(f, "vault root is not a directory: {}", p.display()),
            Self::Io(msg) => write!(f, "{msg}"),
            Self::BadFormat(msg) => write!(f, "daily note format: {msg}"),
        }
    }
}

impl VaultError {
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::MissingEnv | Self::EmptyEnv => "missing_vault",
            Self::NotADirectory(_) => "bad_vault",
            Self::BadFormat(_) => "bad_format",
            Self::Io(_) => "io",
        }
    }
}

impl Vault {
    /// Resolve vault from an optional CLI `--vault` path, else `OBSIDIAN_VAULT_ROOT`.
    pub fn resolve(cli_vault: Option<PathBuf>) -> Result<Self, VaultError> {
        match cli_vault {
            Some(path) => Self::from_path(path),
            None => Self::from_env(),
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, VaultError> {
        let trimmed = path.as_ref().to_string_lossy();
        let trimmed = trimmed.trim();
        if trimmed.is_empty() {
            return Err(VaultError::EmptyEnv);
        }
        let root = PathBuf::from(trimmed);
        if !root.is_dir() {
            return Err(VaultError::NotADirectory(root));
        }
        Ok(Self { root })
    }

    pub fn from_env() -> Result<Self, VaultError> {
        match env::var(VAULT_ENV) {
            Err(env::VarError::NotPresent) => Err(VaultError::MissingEnv),
            Err(env::VarError::NotUnicode(_)) => Err(VaultError::EmptyEnv),
            Ok(value) => Self::from_path(value),
        }
    }

    pub fn daily_notes_config(&self) -> Result<DailyNotesConfig, VaultError> {
        let path = self.root.join(".obsidian").join("daily-notes.json");
        if !path.exists() {
            return Ok(DailyNotesConfig::default());
        }
        let raw = fs::read_to_string(&path)
            .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
        let parsed: DailyNotesJson = serde_json::from_str(&raw)
            .map_err(|e| VaultError::Io(format!("failed to parse {}: {e}", path.display())))?;
        Ok(DailyNotesConfig {
            folder: sanitize_rel(
                parsed
                    .folder
                    .map(|s| s.trim().trim_matches('/').to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_default(),
            ),
            format: parsed
                .format
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "YYYY-MM-DD".to_string()),
            template: parsed
                .template
                .map(|s| s.trim().trim_end_matches(".md").to_string())
                .filter(|s| !s.is_empty())
                .map(sanitize_rel)
                .filter(|s| !s.is_empty()),
        })
    }

    pub fn daily_note_path(
        &self,
        config: &DailyNotesConfig,
        date: NaiveDate,
    ) -> Result<PathBuf, VaultError> {
        let relative =
            format::format_moment(&config.format, date).map_err(VaultError::BadFormat)?;
        let mut path = self.root.clone();
        if !config.folder.is_empty() {
            for part in config.folder.split('/') {
                push_safe_component(&mut path, part)?;
            }
        }
        // Format may include `/` for year/month subfolders.
        for part in relative.split('/') {
            push_safe_component(&mut path, part)?;
        }
        if path.extension().is_none() {
            path.set_extension("md");
        }
        ensure_under_root(&self.root, &path)?;
        Ok(path)
    }

    pub fn template_path(&self, config: &DailyNotesConfig) -> Option<PathBuf> {
        let rel = config.template.as_ref()?;
        let mut path = self.root.clone();
        for part in rel.split('/') {
            push_safe_component(&mut path, part).ok()?;
        }
        if path.extension().is_none() {
            path.set_extension("md");
        }
        ensure_under_root(&self.root, &path).ok()?;
        Some(path)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Drop empty / `.` / `..` path segments so settings cannot escape the vault.
fn sanitize_rel(value: String) -> String {
    value
        .split('/')
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .collect::<Vec<_>>()
        .join("/")
}

fn push_safe_component(path: &mut PathBuf, part: &str) -> Result<(), VaultError> {
    if part.is_empty() || part == "." {
        return Ok(());
    }
    if part == ".." || part.contains('\0') {
        return Err(VaultError::Io(format!(
            "refusing path component that escapes the vault: {part:?}"
        )));
    }
    path.push(part);
    Ok(())
}

fn ensure_under_root(root: &Path, path: &Path) -> Result<(), VaultError> {
    // Construction-time guard: paths are built from the vault root with `..`
    // rejected, so a component-wise prefix check must hold. For paths that
    // already exist this also resolves symlinks (including a symlinked root
    // or daily-notes folder) and verifies the real location, so reads cannot
    // be redirected outside the vault. Write paths re-verify the resolved
    // target at the filesystem operation in `todos.rs`, where it is
    // race-free.
    if !path.starts_with(root) {
        return Err(VaultError::Io(format!(
            "resolved path escapes vault root: {}",
            path.display()
        )));
    }
    if let (Ok(real), Ok(real_root)) = (path.canonicalize(), root.canonicalize())
        && !real.starts_with(&real_root)
    {
        return Err(VaultError::Io(format!(
            "resolved path escapes vault root: {} resolves to {}",
            path.display(),
            real.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn tmp_vault(name: &str) -> PathBuf {
        let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "obsidian-daily-qs-{name}-{}-{}",
            std::process::id(),
            n
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidian")).unwrap();
        dir
    }

    #[test]
    fn defaults_when_settings_missing() {
        let root = tmp_vault("defaults");
        let vault = Vault { root: root.clone() };
        let cfg = vault.daily_notes_config().unwrap();
        assert_eq!(cfg.folder, "");
        assert_eq!(cfg.format, "YYYY-MM-DD");
        assert!(cfg.template.is_none());
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let path = vault.daily_note_path(&cfg, date).unwrap();
        assert_eq!(path, root.join("2026-08-20.md"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_folder_format_and_nested_date() {
        let root = tmp_vault("nested");
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY/MM/YYYY-MM-DD","template":"Templates/Daily"}"#,
        )
        .unwrap();
        let vault = Vault { root: root.clone() };
        let cfg = vault.daily_notes_config().unwrap();
        assert_eq!(cfg.folder, "Daily");
        assert_eq!(cfg.format, "YYYY/MM/YYYY-MM-DD");
        assert_eq!(cfg.template.as_deref(), Some("Templates/Daily"));
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let path = vault.daily_note_path(&cfg, date).unwrap();
        assert_eq!(
            path,
            root.join("Daily")
                .join("2026")
                .join("08")
                .join("2026-08-20.md")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn strips_parent_dir_segments_from_folder() {
        let root = tmp_vault("escape");
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"../outside","format":"YYYY-MM-DD"}"#,
        )
        .unwrap();
        let vault = Vault { root: root.clone() };
        let cfg = vault.daily_notes_config().unwrap();
        assert_eq!(cfg.folder, "outside");
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let path = vault.daily_note_path(&cfg, date).unwrap();
        assert!(path.starts_with(&root));
        assert_eq!(path, root.join("outside").join("2026-08-20.md"));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_note_path_escaping_via_symlink() {
        use std::os::unix::fs::symlink;
        let root = tmp_vault("symlink-escape");
        let outside = std::env::temp_dir().join(format!(
            "obsidian-daily-qs-outside-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&outside).unwrap();
        // Daily folder is a symlink to an outside directory holding the note.
        let daily = root.join("Daily");
        fs::create_dir_all(&daily).unwrap();
        fs::remove_dir(&daily).unwrap();
        symlink(&outside, &daily).unwrap();
        fs::write(outside.join("2026-08-20.md"), "- [ ] x\n").unwrap();
        let vault = Vault { root: root.clone() };
        let cfg = DailyNotesConfig {
            folder: "Daily".into(),
            format: "YYYY-MM-DD".into(),
            template: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let err = vault.daily_note_path(&cfg, date).unwrap_err();
        assert!(err.to_string().contains("escapes vault root"), "err: {err}");
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
