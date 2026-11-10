<h1 align="center">ghtui</h1>

<p align="center">
  A rich terminal UI for GitHub that doesn't break your flow.
  <br />
  An AI-friendly fork, rewritten in Rust.
</p>

<br />

<img src="./docs/src/assets/overview.gif" />

## 🌟 Features

- User-defined, per-repo, PRs & issues sections
- Overridable vim-style keyboard hotkeys
- Mouse support: scroll and click
- Custom actions to perform your specific workflow needs
- Everything you can do on GitHub - diff, comment, checkout, push, update etc.
- Control every setting with a YAML config file

If you like quickly navigating with your keyboard, seeing the PRs and issues you need and you <strong>love the terminal</strong> - <code>ghtui</code> is for you! 🫵🏽

## 📦 Install

```sh
brew install juanibiapina/taps/ghtui
```

Or install from source with [cargo](https://rustup.rs):

```sh
cargo install --git https://github.com/juanibiapina/ghtui
```

Then run `ghtui`.

It authenticates like `gh`: `$GH_TOKEN` or `$GITHUB_TOKEN` when set, otherwise
`gh auth token`. Mutations (merge, comment, label, checkout, ...) run through the
`gh` CLI, which must be on `PATH`.

## 📃 Docs

Read the docs at [juanm.dev/ghtui](https://juanm.dev/ghtui/).

## 🙏 Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## 🛞 Under the hood

`ghtui` uses:

- [ratatui](https://ratatui.rs) for the TUI
- [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark) for parsing markdown
- [syntect](https://github.com/trishume/syntect) with [bat](https://github.com/sharkdp/bat)'s syntaxes for syntax highlighting
- [gh](https://github.com/cli/cli) for the GitHub functionality
- [delta](https://github.com/dandavison/delta) for viewing PR diffs
