# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Removed

- Remove support for gh-dash config files, `$GH_DASH_CONFIG` and gh-dash bookmarks.

### Changed

- Show help in a popup that groups keys by category and closes with `esc`.
- Show fetched PRs, issues and notifications up to 100 ms sooner.
- Stop capturing the mouse, so you can select and copy text in the terminal again.

### Fixed

- Fix the branches view running two rows past the bottom of its area.
- Fix the search popup in the branches view sitting one row too low.
- Fix the branches view never refreshing PRs, and finding no PRs for repositories cloned over SSH.

## [0.1.0] - 2026-11-06

### Added

- Show your pull requests, issues and notifications in sections defined by GitHub search filters.
- Preview a PR or issue with its description, activity, checks and changed files.
- Act on PRs and issues from the keyboard: comment, review, merge, label, assign, close and check out.
- Configure sections, layout, theme and keybindings in YAML, globally or per repository.
- Run custom commands from keybindings.
- Browse local branches and their PRs in the branches view, enabled with `FF_REPO_VIEW`.
- Install with Homebrew or a shell installer.

[Unreleased]: https://github.com/juanibiapina/ghtui/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/juanibiapina/ghtui/releases/tag/v0.1.0
