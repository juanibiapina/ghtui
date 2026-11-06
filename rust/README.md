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

## Not ported

- The branches view behind `FF_REPO_VIEW`.
- `FF_MOCK_DATA`, `--debug` logging and `--cpuprofile` (the flags are accepted and ignored).
- Mouse input, and the "Update available!" release check.
