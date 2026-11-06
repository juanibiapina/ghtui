# Contributing To `gh-dash`

## Working on the Code

The app is a Rust crate in [rust/](./rust). Install Rust with
[rustup](https://rustup.rs), then build and test from that directory:

```sh
cd rust
cargo build
cargo test
cargo clippy
```

The UI is built on [ratatui](https://ratatui.rs).

### Debugging

- Log with the `crate::debug!`, `crate::info!`, `crate::warn!` and `crate::error!` macros
- Tail the log by running `task logs`
- Run `gh-dash --debug` in another terminal window / pane; `LOG_LEVEL` sets the level

### Running the Docs Locally

- Run the docs site by running `task docs`

* Go to `localhost:4321` to view them
