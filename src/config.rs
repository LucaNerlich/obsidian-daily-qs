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
            Self::MissingEnv => write!(f, "{VAULT_ENV} is not set"),
            Self::EmptyEnv => write!(f, "{VAULT_ENV} is empty"),
            Self::NotADirectory(p) => write!(f, "vault root is not a directory: {}", p.display()),
            Self::Io(msg) => write!(f, "{msg}"),
            Self::BadFormat(msg) => write!(f, "daily note format: {msg}"),
        }
    }
}

impl Vault {
    pub fn from_env() -> Result<Self, VaultError> {
        match env::var(VAULT_ENV) {
            Err(env::VarError::NotPresent) => Err(VaultError::MissingEnv),
            Err(env::VarError::NotUnicode(_)) => Err(VaultError::EmptyEnv),
            Ok(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Err(VaultError::EmptyEnv);
                }
                let root = PathBuf::from(trimmed);
                if !root.is_dir() {
                    return Err(VaultError::NotADirectory(root));
                }
                Ok(Self { root })
            }
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
            folder: parsed
                .folder
                .map(|s| s.trim().trim_matches('/').to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_default(),
            format: parsed
                .format
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "YYYY-MM-DD".to_string()),
            template: parsed
                .template
                .map(|s| s.trim().trim_end_matches(".md").to_string())
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
            path.push(&config.folder);
        }
        // Format may include `/` for year/month subfolders.
        for part in relative.split('/') {
            if part.is_empty() || part == "." || part == ".." {
                continue;
            }
            path.push(part);
        }
        if path.extension().is_none() {
            path.set_extension("md");
        }
        Ok(path)
    }

    pub fn template_path(&self, config: &DailyNotesConfig) -> Option<PathBuf> {
        let rel = config.template.as_ref()?;
        let mut path = self.root.join(rel);
        if path.extension().is_none() {
            path.set_extension("md");
        }
        Some(path)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
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
}
