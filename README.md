# obsidian-daily-qs

Omarchy Quattro bar widget for today's [Obsidian daily note](https://obsidian.md/help/plugins/daily-notes) todos: view open items, add new checkboxes, and toggle them from a bar panel.

## Requirements

- Omarchy Quattro (Quickshell-based shell)
- An Obsidian vault with the Daily notes core plugin configured
- `OBSIDIAN_VAULT_ROOT` set to the absolute path of that vault for the graphical session — see [Set the vault path](#set-the-vault-path)

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

The backend reads `OBSIDIAN_VAULT_ROOT` from its process environment. The Omarchy shell runs as a child of Hyprland, so an `export` in `~/.bashrc` (or in a terminal) does **not** reach it. Set the variable for the graphical session instead:

**Option A — Hyprland env** (per-session, applies without logging out):

```lua
-- ~/.config/hypr/hyprland.lua
hl.env("OBSIDIAN_VAULT_ROOT", "/home/you/Documents/notizen")
```

```bash
hyprctl reload && omarchy restart shell
```

**Option B — systemd user environment** (applies at next login, also covers apps launched via `uwsm-app`):

```ini
# ~/.config/environment.d/obsidian-vault.conf
OBSIDIAN_VAULT_ROOT=/home/you/Documents/notizen
```

Setting both keeps the current session working until the next login. Use `pgrep -f 'obsidian-daily-qs watch'` and `/proc/<pid>/environ` to verify the backend sees the variable.

## Usage

- **Bar**: shows done/total for today (`☐ 2/5`). Left-click opens the panel with focus in the add field.
- **Panel**:
  - List todos; click a row to toggle.
  - Nested todos (indented with tabs or two spaces per level in the note) render indented; the open-only filter keeps parents of open items visible.
  - Scrolls vertically with the mouse wheel, scrollbar, or Up/Down/j/k (when not typing in the add field).
  - Search: `/` focuses the search field (even from the empty add field); matches filter the list (ancestors stay visible) and combine with open-only. Esc clears, second Esc closes.
  - Add with Enter / `+`.
  - **Open only** / **All todos** toggles completed visibility (default from `openOnly` setting).
  - **◀ / ● / ▶** move to previous day, today, or next day.
  - **Carry over N** (when viewing today) copies yesterday's still-open todos (preserving nesting).
  - Open-in-Obsidian launches `obsidian://open?path=…` via `xdg-open`.
- **Shell**: `omarchy-shell shell summon luca.obsidian-daily '{}'` / `omarchy-shell shell hide luca.obsidian-daily`.

```bash
omarchy bar set luca.obsidian-daily openOnly true
```

## CLI

```bash
export OBSIDIAN_VAULT_ROOT="/path/to/vault"
obsidian-daily-qs status
obsidian-daily-qs status --date 2026-08-19
obsidian-daily-qs watch
obsidian-daily-qs add --text "Ship plugin"
obsidian-daily-qs add --date 2026-08-19 --text "Backfill"
obsidian-daily-qs toggle --line 12
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
