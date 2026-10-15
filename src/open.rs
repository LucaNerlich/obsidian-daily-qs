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
/// Falls back to `path=` when the vault-relative form cannot be trusted to
/// resolve, so opening still works from an odd setup. That fallback does carry
/// an absolute path — see the README note next to the `--stdin` rule.
pub fn open_uri(vault_root: &Path, note: &Path) -> String {
    if let Some((vault, file)) = vault_relative_parts(vault_root, note) {
        return format!(
            "obsidian://open?vault={}&file={}",
            percent_encode_path(&vault),
            percent_encode_path(&file),
        );
    }
    format!(
        "obsidian://open?path={}",
        percent_encode_path(&note.display().to_string())
    )
}

/// `(vault name, note path inside it)` for the vault-relative URI form.
///
/// Obsidian resolves `vault=` against the vault's *registered name*, which is
/// the vault folder's own name, and only that folder resolves. So the form is
/// used only when `vault_root` really is a vault root — the `.obsidian`
/// directory Obsidian keeps there is the marker — and the note lies inside it.
/// The root is canonicalised first, so a root reached through a differently
/// named symlink (or spelled with a trailing `..`) is resolved to the real
/// vault rather than emitting a name Obsidian cannot match. A subfolder of a
/// vault, or anything else, falls back to `path=`, which works for any file in
/// any registered vault.
fn vault_relative_parts(vault_root: &Path, note: &Path) -> Option<(String, String)> {
    let root = vault_root.canonicalize().ok()?;
    if !root.join(".obsidian").is_dir() {
        return None;
    }
    let name = root.file_name()?.to_str()?.to_owned();
    // The note is produced from the configured root, so it exists by now; if it
    // cannot be resolved, keep the configured form and let the prefix check
    // decide.
    let resolved = note.canonicalize().unwrap_or_else(|_| note.to_path_buf());
    let file = resolved.strip_prefix(&root).ok()?.to_str()?.to_owned();
    if file.is_empty() {
        return None;
    }
    Some((name, file))
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
    use std::fs;
    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(name: &str) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("obsidian-open-uri-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// A real vault root, recognised by the `.obsidian` directory Obsidian
    /// keeps there, with one note inside it.
    fn vault_with_note(name: &str, note_rel: &str) -> (TempDir, PathBuf, PathBuf) {
        let dir = temp_dir(name);
        let vault = dir.0.join("My Vault");
        fs::create_dir_all(vault.join(".obsidian")).unwrap();
        let note = vault.join(note_rel);
        fs::create_dir_all(note.parent().unwrap()).unwrap();
        fs::write(&note, "- [ ] x\n").unwrap();
        (dir, vault, note)
    }

    #[test]
    fn uses_the_vault_relative_form() {
        let (_dir, vault, note) = vault_with_note("relative", "Daily Notes/2026-08-20.md");
        assert_eq!(
            open_uri(&vault, &note),
            "obsidian://open?vault=My%20Vault&file=Daily%20Notes/2026-08-20.md"
        );
    }

    #[test]
    fn keeps_slashes_in_the_note_path() {
        let (_dir, vault, note) = vault_with_note("slashes", "a/b.md");
        assert_eq!(
            open_uri(&vault, &note),
            "obsidian://open?vault=My%20Vault&file=a/b.md"
        );
    }

    #[test]
    fn resolves_a_symlinked_root_to_the_real_vault_name() {
        #[cfg(unix)]
        {
            let dir = temp_dir("symlink");
            let real = dir.0.join("private-client").join("My Vault");
            fs::create_dir_all(real.join(".obsidian")).unwrap();
            let note = real.join("2026-08-20.md");
            fs::write(&note, "- [ ] x\n").unwrap();
            let link = dir.0.join("obs");
            std::os::unix::fs::symlink(&real, &link).unwrap();

            // The configured root is the symlink; the emitted name must be the
            // real vault folder Obsidian registered, not "obs".
            let uri = open_uri(&link, &link.join("2026-08-20.md"));
            assert_eq!(uri, "obsidian://open?vault=My%20Vault&file=2026-08-20.md");
        }
    }

    #[test]
    fn falls_back_when_the_root_is_not_a_vault() {
        let dir = temp_dir("not-a-vault");
        let sub = dir.0.join("vault").join("subfolder");
        fs::create_dir_all(&sub).unwrap();
        let note = sub.join("2026-08-20.md");
        fs::write(&note, "- [ ] x\n").unwrap();
        assert_eq!(
            open_uri(&sub, &note),
            format!("obsidian://open?path={}", note.display())
        );
    }

    #[test]
    fn falls_back_when_the_note_is_outside_the_vault() {
        let (_dir, vault, _note) = vault_with_note("outside", "2026-08-20.md");
        let dir = temp_dir("outside-note");
        let elsewhere = dir.0.join("a.md");
        fs::write(&elsewhere, "").unwrap();
        assert_eq!(
            open_uri(&vault, &elsewhere),
            format!("obsidian://open?path={}", elsewhere.display())
        );
    }

    #[test]
    fn falls_back_for_a_root_that_cannot_be_resolved() {
        let uri = open_uri(
            Path::new("/definitely/not/here/vault"),
            Path::new("/definitely/not/here/vault/a.md"),
        );
        assert_eq!(uri, "obsidian://open?path=/definitely/not/here/vault/a.md");
    }
}
