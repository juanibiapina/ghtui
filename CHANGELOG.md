# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Install with Homebrew (`brew install juanibiapina/taps/gh-dash`) or a shell installer from the GitHub releases.

### Changed

- Rewrite gh-dash in Rust. Config files, keybindings and screens stay the same.
- Run gh-dash as its own `gh-dash` command instead of the `gh dash` extension.
- `gh-dash --version` prints only the name and version.

### Removed

- Remove the `--cpuprofile` flag.

### Fixed

- Fix a new section showing the previous section's PR in the preview.
- Fix the branches view (`FF_REPO_VIEW`) showing "No sections defined" when you switch to it.
- Fix a crash in the branches view with a compact table.
- Fix the branch preview showing the wrong branch after a search or checkout.
- Fix custom branch keybindings doing nothing.
