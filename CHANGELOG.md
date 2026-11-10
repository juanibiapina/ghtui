# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Add the `quitOnEsc` setting to quit with `esc`.

### Removed

- Remove support for gh-dash config files, `$GH_DASH_CONFIG` and gh-dash bookmarks.
- Remove read and unread notifications: the notifications view shows every notification you haven't marked done. This removes `m` and `M`, the `is:unread`, `is:read` and `is:all` filters, the `includeReadNotifications` setting and the unread markers, and viewing a notification no longer marks it read.
- Remove the `enter` key and the `view` key name from the notifications view. The preview already shows the selected notification.
- Remove the notification details view that `esc` opened in the notifications view, so `quitOnEsc` quits on the first press.

### Changed

- Show the last session's PRs, issues and notifications at launch, then fetch only what changed.
- Load every view in the background, so switching views and moving between rows shows previews without waiting.
- Stop fetching from GitHub while the terminal doesn't have focus, and catch up when it returns.
- Pause background fetching when GitHub rate limits you, instead of retrying.
- Refresh PRs in the branches view every 5 minutes instead of every minute, and right away when the terminal regains focus.
- Show help in a popup that groups keys by category and closes with `esc`.
- Show fetched PRs, issues and notifications up to 100 ms sooner.
- Stop capturing the mouse, so you can select and copy text in the terminal again.
- Mark a notification as done with `d`, and view a PR diff with `D`.
- Remove a notification from the list as soon as you mark it done, and put it back if GitHub rejects the change.
- Count a notification's new comments from when you last marked it done.

### Fixed

- Fix `P` in the branches view moving the preview as well as pushing the branch.
- Fix the branches view running two rows past the bottom of its area.
- Fix the search popup in the branches view sitting one row too low.
- Fix the branches view never refreshing PRs, and finding no PRs for repositories cloned over SSH.
- Fix the branches view clearing its branches every time you switch to it.
- Fix returning to the terminal loading another page of the current section instead of refreshing it.

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
