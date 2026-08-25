# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `vaultPath` bar setting and global `--vault` CLI flag (env `OBSIDIAN_VAULT_ROOT` remains a fallback).
- Setup empty state in the panel when the vault path is missing or invalid.
- Keyboard list cursor: j/k or arrows move, Enter/Space toggles, `e` edits, `x` deletes, `[`/`]` outdent/indent, `u` undoes.
- Nested add via Shift+Enter (under the selected todo) and `--under-line` on `add`.
- `edit`, `delete`, `indent`, `outdent`, `undo`, and `week` backend commands.
- Week strip in the panel for jumping between days.
- Optional `todoHeading` filter; `hideWhenDone` / `hideWhenEmpty` bar concealment.
- Middle/right-click on the bar opens the note in Obsidian; progress cue on the icon.

## [1.5.0] - 2026-08-25

### Changed

- Bar label uses the Obsidian mark (theme-tinted PathSvg) beside the done/total
  count instead of a checkbox glyph.
- Panel layout matches other Quattro panels more closely: section separators,
  Open-only toggle switch, hoverable todo rows, and custom checkboxes.

## [1.4.2] - 2026-08-22

### Fixed

- Carry-over into a day whose note does not exist yet no longer duplicates
  every carried todo (the note creation re-entered the rollover, appending
  each item twice and leaving the previous day's note already emptied).

## [1.4.1] - 2026-08-21

### Security

- `verify-bundle` checks the toolchain pin's semantic `components` line
  instead of any text match, so removing rustfmt/clippy from the pin is no
  longer masked by comments.
- All GitHub Actions are pinned to full commit SHAs.

### Fixed

- `watch` change detection keys on the serialized snapshot, so same-size
  checkbox toggles and carry-over count changes from edits to yesterday's
  note are emitted immediately.
- A watch backend that starts but crashes repeatedly now engages the PATH
  fallback and marks the bar stale (error state) instead of resetting to a
  healthy zeroed label.
- The panel clears a leftover search filter when it is reopened.
- One-shot commands exit quietly on a closed stdout instead of panicking
  on EPIPE.
- Atomic note writes no longer leave an orphaned temp file when the write
  or sync fails, and preserve the existing note's file permissions.

## [1.4.0] - 2026-08-21

### Changed

- Carry over now **moves** yesterday's still-open todos into the new day
  (preserving nesting) instead of copying them: the previous daily note is
  left with only its done todos. Open todos that already exist in the target
  note are not duplicated and stay in the previous note.
- Creating a new daily note (first write of a new day) automatically rolls
  the previous day's open todos into it.

### Fixed

- Note creation no longer creates directories outside the vault before the
  write check rejects the note: `ensure_note` canonicalizes the nearest
  existing ancestor of the note's parent and verifies it stays inside the
  vault root before running `create_dir_all`, so a symlinked daily-notes
  folder with a nested date format cannot leave stray directories at the
  link target.

## [1.3.1] - 2026-08-20

### Security

- Enforce the vault boundary on resolved paths: a symlinked daily-notes folder
  or note file that resolves outside the (canonicalized) vault root is refused
  for reads and writes. Atomic note writes resolve the parent directory first
  and create the temp file with `O_EXCL` (`create_new`) under an unpredictable
  name, so a pre-created `*.tmp-obsidian-daily-qs` symlink can no longer
  redirect a write outside the vault.

## [1.3.0] - 2026-08-20

### Added

- Panel search: `/` jumps to the search field (also from the empty add-todo
  field); matching is a case-insensitive substring filter that keeps
  ancestors of matches so nested context stays readable, and combines with
  the open-only filter. Esc clears the query and returns focus to the add
  field, a second Esc closes the panel.

## [1.2.1] - 2026-08-20

### Fixed

- Todo text containing `&`, `<`, or `>` (e.g. `Team & Agile Meetings`) was
  dropped entirely and rendered as an empty checkbox. Todo rows are always
  `PlainText`, so such text is now kept (only control characters are
  stripped); error strings remain strictly sanitized.

## [1.2.0] - 2026-08-20

### Added

- Nested todo lists: checkbox indentation (tabs or two-space levels) is parsed
  into `depth` / `parentLine` per item, rendered indented in the panel, and
  preserved by carry-over. The open-only filter keeps ancestors of open items
  so nested context stays readable.
- Scrollable panel: content flicks vertically with a scrollbar when the list
  exceeds the panel height; Up/Down/j/k scroll while the input field is not
  focused.
- New todos insert under a `## Todos` heading as well as `## Tasks`.

### Fixed

- Arrow keys are no longer swallowed while the add-todo field has focus.

## [1.1.0] - 2026-08-20

### Added

- Bar label shows done/total (`☐ 2/5`).
- Panel day navigation (previous / today / next) with the same todo UX.
- Open-only filter in the panel (default via `openOnly` widget setting).
- Carry over yesterday's still-open todos into today (skips duplicates).
- Open the viewed daily note in Obsidian via `obsidian://open?path=…`.
- Left-click opens the panel with focus in the add field (quick capture).

## [1.0.0] - 2026-08-20

### Added

- Initial Omarchy Quattro bar widget for today's Obsidian daily note todos.
- Vault path from `OBSIDIAN_VAULT_ROOT`; daily note location from
  `.obsidian/daily-notes.json` (`folder`, `format`, optional `template`).
- List, add, and toggle markdown checkbox todos (`- [ ]` / `- [x]`).
- New items insert under a `## Tasks` heading when present, otherwise append
  at the end of the note. Adding creates today's note from the configured
  template when needed.
- Marketplace-ready musl x86_64 backend bundle with byte-identical CI
  attestation (`make verify-bundle`).

### Fixed

- Exit `watch` when stdout is broken so a crashed shell cannot leave a
  polling helper behind.
- Reject `..` / null path components from daily-notes settings so notes and
  templates cannot escape the vault root.
- Drop HTML markup from helper JSON before QML display and force
  `Text.PlainText` for note-derived strings.
- Reject multiline todo text so `add` cannot inject extra markdown lines.

## [0.1.0] - 2026-08-20

### Added

- Development scaffold (superseded by 1.0.0).
