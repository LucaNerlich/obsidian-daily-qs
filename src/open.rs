//! Obsidian URI helpers and launching the desktop app.

use std::path::Path;
use std::process::Command;

use crate::config::VaultError;

/// Build an Obsidian "open" URI for `note` inside `vault_root`.
///
/// Prefers the vault-relative form. The absolute `path=` form puts the whole
/// local path — including private client/project folder names — into
/// `xdg-open`'s argv, where any local user can read it from
/// `/proc/<pid>/cmdline`; AGENTS.md forbids private data in argv.
/// `vault=`/`file=` carries only the vault's own folder name and the note's
/// path inside it.
///
/// Falls back to `path=` when the vault folder name is unavailable or the note
/// is not under the vault root, so opening still works from an odd setup.
pub fn open_uri(vault_root: &Path, note: &Path) -> String {
    let relative = note
        .strip_prefix(vault_root)
        .ok()
        .filter(|rel| !rel.as_os_str().is_empty())
        .map(|rel| rel.display().to_string());
    if let (Some(name), Some(relative)) = (vault_root.file_name(), relative) {
        // A vault folder name cannot itself contain '/', so the path encoder
        // produces the same bytes as a component encoder for it.
        return format!(
            "obsidian://open?vault={}&file={}",
            percent_encode_path(&name.to_string_lossy()),
            percent_encode_path(&relative),
        );
    }
    format!(
        "obsidian://open?path={}",
        percent_encode_path(&note.display().to_string())
    )
}

fn percent_encode_path(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(hex(b >> 4));
                out.push(hex(b & 0xf));
            }
        }
    }
    out
}

fn hex(n: u8) -> char {
    char::from(if n < 10 { b'0' + n } else { b'A' + (n - 10) })
}

/// Launch the URI via `xdg-open` (Omarchy / Wayland desktop).
pub fn launch(uri: &str) -> Result<(), VaultError> {
    // Some desktop handlers keep xdg-open alive until the app closes. Do not
    // hold up the frontend's action queue for the lifetime of Obsidian.
    Command::new("xdg-open")
        .arg(uri)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| VaultError::Io(format!("failed to spawn xdg-open: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_vault_relative_form() {
        let uri = open_uri(
            Path::new("/home/u/vault"),
            Path::new("/home/u/vault/Daily Notes/2026-08-20.md"),
        );
        assert_eq!(
            uri,
            "obsidian://open?vault=vault&file=Daily%20Notes/2026-08-20.md"
        );
    }

    #[test]
    fn encodes_spaces_in_the_vault_name() {
        let uri = open_uri(
            Path::new("/home/u/My Vault"),
            Path::new("/home/u/My Vault/2026-08-20.md"),
        );
        assert_eq!(uri, "obsidian://open?vault=My%20Vault&file=2026-08-20.md");
    }

    #[test]
    fn keeps_slashes_in_the_note_path() {
        let uri = open_uri(
            Path::new("/home/u/vault"),
            Path::new("/home/u/vault/a/b.md"),
        );
        assert_eq!(uri, "obsidian://open?vault=vault&file=a/b.md");
    }

    #[test]
    fn falls_back_to_the_absolute_path_outside_the_vault() {
        let uri = open_uri(Path::new("/home/u/vault"), Path::new("/elsewhere/a.md"));
        assert_eq!(uri, "obsidian://open?path=/elsewhere/a.md");
    }
}
