# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
