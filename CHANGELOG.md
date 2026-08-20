# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-08-20

### Added

- Initial Omarchy Quattro bar widget for today's Obsidian daily note todos.
- Vault path from `OBSIDIAN_VAULT_ROOT`; daily note location from
  `.obsidian/daily-notes.json` (`folder`, `format`, optional `template`).
- List, add, and toggle markdown checkbox todos (`- [ ]` / `- [x]`).
- New items insert under a `## Tasks` heading when present, otherwise append
  at the end of the note. Adding creates today's note from the configured
  template when needed.
