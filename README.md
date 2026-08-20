# obsidian-daily-qs

Omarchy Quattro bar widget for today's [Obsidian daily note](https://obsidian.md/help/plugins/daily-notes) todos: view open items, add new checkboxes, and toggle them from a bar panel.

## Requirements

- Omarchy Quattro (Quickshell-based shell)
- An Obsidian vault with the Daily notes core plugin configured
- `OBSIDIAN_VAULT_ROOT` set to the absolute path of that vault (for the shell session that runs `omarchy-shell`)

Daily note location and date format are read from `.obsidian/daily-notes.json` (`folder`, `format`, optional `template`), relative to the vault root.

## Architecture

- **Rust backend** (`obsidian-daily-qs`): resolves today's note, parses markdown checkboxes, adds/toggles items, streams JSON snapshots.
- **QML frontend** (`omarchy/`): `bar-widget` with a details panel. Left-click opens the panel.

```
obsidian-daily-qs watch ──(JSON lines)──▶ BarWidget ─▶ Panel
obsidian-daily-qs add|toggle ──(JSON line)──▶ BarWidget
```

## Install

```bash
export OBSIDIAN_VAULT_ROOT="/path/to/your/vault"
omarchy plugin add https://github.com/LucaNerlich/obsidian-daily-qs.git --enable
```

Update / remove:

```bash
omarchy plugin update luca.obsidian-daily
omarchy plugin remove luca.obsidian-daily
```

The plugin bundles a statically linked x86_64 musl build of its backend (`omarchy/bin/obsidian-daily-qs`). If the bundled binary cannot start, the widget falls back to an `obsidian-daily-qs` binary on `PATH` (`cargo install --path .`).

Ensure `OBSIDIAN_VAULT_ROOT` is available to the Omarchy shell (e.g. in `~/.config/environment.d/` or your Hyprland env).

## Usage

- **Bar**: shows open todo count (`☐ 3`). Left-click opens/closes the panel.
- **Panel**: lists today's checkboxes; click a row to toggle; type and press Enter (or `+`) to add. New items go under `## Tasks` when that heading exists, otherwise at the end of the note. Adding creates today's note (from the configured template when present).
- **Shell**: `omarchy-shell shell summon luca.obsidian-daily '{}'` / `omarchy-shell shell hide luca.obsidian-daily`.

## CLI

```bash
export OBSIDIAN_VAULT_ROOT="/path/to/vault"
obsidian-daily-qs status
obsidian-daily-qs watch
obsidian-daily-qs add --text "Ship plugin"
obsidian-daily-qs toggle --line 12
```

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
node omarchy/model.test.mjs
omarchy plugin validate .
qmllint -I "$OMARCHY_PATH/shell" omarchy/BarWidget.qml omarchy/Panel.qml
```

`make bundle` rebuilds the musl backend into `omarchy/bin/` (Linux). `make verify-bundle` is the marketplace gate. Any edit under `src/`, `Cargo.toml`, `Cargo.lock`, or `rust-toolchain.toml` requires a fresh `make bundle` in the same change.

### Releasing

1. Bump `Cargo.toml`, `Cargo.lock`, `manifest.json`, and `CHANGELOG.md`.
2. Run `make bundle` then `make verify-bundle` on Linux.
3. Open a PR; wait for the **marketplace bundle** CI job to be green.
4. Merge, then tag `vX.Y.Z` matching the crate version.

## License

Apache-2.0.
