//! Obsidian URI helpers and launching the desktop app.

use std::path::Path;
use std::process::Command;

use crate::config::VaultError;

/// Build `obsidian://open?path=…` for an absolute note path.
pub fn open_uri(path: &Path) -> String {
    let abs = path.display().to_string();
    let encoded = percent_encode_path(&abs);
    format!("obsidian://open?path={encoded}")
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
    let status = Command::new("xdg-open")
        .arg(uri)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| VaultError::Io(format!("failed to spawn xdg-open: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(VaultError::Io(format!(
            "xdg-open exited with {}",
            status.code().unwrap_or(-1)
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn encodes_spaces_in_path() {
        let uri = open_uri(Path::new("/vault/Daily Notes/2026-08-20.md"));
        assert!(uri.starts_with("obsidian://open?path="));
        assert!(uri.contains("Daily%20Notes"));
        assert!(uri.contains("/2026-08-20.md"));
    }

    #[test]
    fn keeps_slashes() {
        let uri = open_uri(&PathBuf::from("/home/u/vault/a/b.md"));
        assert_eq!(uri, "obsidian://open?path=/home/u/vault/a/b.md");
    }
}
