# gh-dash (Rust)

A port of gh-dash to Rust with [ratatui](https://ratatui.rs). It reads the same
config files, uses the same keybindings, and draws the same screens as the Go
version in the repository root.

## Build

```sh
cd rust
cargo build --release
```

The binary is `target/release/gh-dash`. To use it as the `gh dash` extension,
copy it over the Go build in the repository root and run `gh extension install .`
from there.

It authenticates like `gh`: `$GH_TOKEN` or `$GITHUB_TOKEN` when set, otherwise
`gh auth token`. Mutations (merge, comment, label, checkout, ...) run through the
`gh` CLI, which must be on `PATH`.

## Differences from the Go version

These fix Go bugs:

- A new section shows an empty preview instead of the previous section's PR.
- Switching into the branches view (`FF_REPO_VIEW`) with `s` loads the branches;
  Go shows "No sections defined".
- The branches view renders branches without a PR in the compact table; Go panics.
- The branch preview follows the selection after searching or checking out.
- Custom branch keybindings run their command.
