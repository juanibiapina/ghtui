# Contributing To `ghtui`

## Working on the Code

The app is a Rust crate. Install Rust with [rustup](https://rustup.rs), then
build and test:

```sh
cargo build
cargo test
cargo clippy --all-targets
cargo fmt --all -- --check
make install  # installs ghtui to ~/.cargo/bin
```

Warnings and clippy lints fail the build.

The UI is built on [ratatui](https://ratatui.rs).

### Debugging

- Log with the `crate::debug!`, `crate::info!`, `crate::warn!` and `crate::error!` macros
- Run `ghtui --debug` in one terminal pane; `LOG_LEVEL` sets the level
- Run `tail -f debug.log` in another pane, in the same directory

## Docs

The docs site lives in `docs/` and deploys to <https://juanm.dev/ghtui/> on every push to
`main`. Run it locally with `make docs`, then open `localhost:4321/ghtui/`.

`make media` re-records the videos and screenshots with [vhs](https://github.com/charmbracelet/vhs).
It needs `vhs`, `ffmpeg` and ImageMagick, plus the font in
`docs/src/fonts/CommitMonoNerdFontMono-Regular.otf` installed on your system. The recordings use
your GitHub account, so their filters only show public repositories.
