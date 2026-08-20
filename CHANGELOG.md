# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
