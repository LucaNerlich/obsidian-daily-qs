# obsidian-daily-qs

[![GitHub Release](https://img.shields.io/github/v/release/LucaNerlich/obsidian-daily-qs)](https://github.com/LucaNerlich/obsidian-daily-qs/releases)
[![Omarchy marketplace](https://img.shields.io/badge/Omarchy-marketplace-teal)](https://omarchyplugins.com/plugin.html?id=luca.obsidian-daily)

Omarchy Quattro bar widget for today's [Obsidian daily note](https://obsidian.md/help/plugins/daily-notes) todos: view open items, add new checkboxes, and toggle them from a bar panel.

## Requirements

- Omarchy Quattro (Quickshell-based shell)
- An Obsidian vault with the Daily notes core plugin configured
- Vault path via the `vaultPath` bar setting **or** `OBSIDIAN_VAULT_ROOT` for the graphical session — see [Set the vault path](#set-the-vault-path)

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
omarchy plugin add https://github.com/LucaNerlich/obsidian-daily-qs.git --enable
```

Update / remove:

```bash
omarchy plugin update luca.obsidian-daily
omarchy plugin remove luca.obsidian-daily
```

The plugin bundles a statically linked x86_64 musl build of its backend (`omarchy/bin/obsidian-daily-qs`). If the bundled binary cannot start, the widget falls back to an `obsidian-daily-qs` binary on `PATH` (`cargo install --path .`).

### Set the vault path

Prefer the bar setting (no Hyprland env needed):

```bash
omarchy bar set luca.obsidian-daily vaultPath '/home/you/Documents/notizen'
```

Or set it from the panel when the widget shows the setup empty state.

The backend also accepts `--vault <path>` and still reads `OBSIDIAN_VAULT_ROOT` when `vaultPath` is empty. The Omarchy shell runs as a child of Hyprland, so an `export` in `~/.bashrc` does **not** reach it unless you also set the graphical session env:

**Option A — Hyprland env** (per-session, applies without logging out):

```lua
-- ~/.config/hypr/hyprland.lua
hl.env("OBSIDIAN_VAULT_ROOT", "/home/you/Documents/notizen")
```

```bash
hyprctl reload && omarchy restart shell
```

**Option B — systemd user environment** (applies at next login):

```ini
# ~/.config/environment.d/obsidian-vault.conf
OBSIDIAN_VAULT_ROOT=/home/you/Documents/notizen
```

## Usage

- **Bar**: Obsidian mark + done/total for today. Left-click opens the panel; middle/right-click opens the note in Obsidian.
- **Panel**:
  - List todos; click or Enter/Space (with keyboard cursor) to toggle.
  - Nested todos render indented; Shift+Enter adds under the selected row.
  - `e` edits, `x` deletes, `[`/`]` outdent/indent, `u` undoes the last mutation.
  - Week strip jumps between days; ◀ / ● / ▶ also navigate.
  - Search: `/`; open-only toggle; carry over; open in Obsidian.
- **Settings** (`omarchy bar set luca.obsidian-daily …`): `vaultPath`, `openOnly`, `todoHeading`, `hideWhenDone`, `hideWhenEmpty`.

```bash
omarchy bar set luca.obsidian-daily openOnly true
omarchy bar set luca.obsidian-daily todoHeading Todos
```

## CLI

```bash
export OBSIDIAN_VAULT_ROOT="/path/to/vault"
# or: obsidian-daily-qs --vault /path/to/vault …
obsidian-daily-qs status
obsidian-daily-qs status --date 2026-08-19 --heading Todos
obsidian-daily-qs watch
obsidian-daily-qs add --text "Ship plugin"
obsidian-daily-qs add --text "Nested" --under-line 12
obsidian-daily-qs toggle --line 12
obsidian-daily-qs edit --line 12 --text "Renamed" --expect-text "Old"
obsidian-daily-qs delete --line 12
obsidian-daily-qs indent --line 12
obsidian-daily-qs outdent --line 12
obsidian-daily-qs undo
obsidian-daily-qs week
obsidian-daily-qs carry-over
obsidian-daily-qs open
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
