<br />
<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./docs/public/logo.png">
    <img alt="DASH logo" width="600" src="./docs/public/logo-light.png">
  </picture>
</p>

<p align="center">
  A rich terminal UI for GitHub that doesn't break your flow.
</p>

<br />

<img src="./docs/src/assets/overview.gif" />

## 🌟 Features

- User-defined, per-repo, PRs & issues sections
- Overridable vim-style keyboard hotkeys
- Custom actions to perform your specific workflow needs
- Everything you can do on GitHub - diff, comment, checkout, push, update etc.
- Control every setting with a YAML config file

If you like quickly navigating with your keyboard, seeing the PRs and issues you need and you <strong>love the terminal</strong> - <code>DASH</code> is for you! 🫵🏽

## 📦 Install

```sh
git clone https://github.com/juanibiapina/gh-dash
cd gh-dash
cargo install --path rust
```

This installs `gh-dash` to `~/.cargo/bin`. To update, run `git pull` and
`cargo install --path rust` again.

## 📃 Docs

The docs live in [docs/src/content/docs](./docs/src/content/docs).

## 🙏 Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## 🛞 Under the hood

`DASH` uses:

- [ratatui](https://ratatui.rs) for the TUI
- [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark) for parsing markdown
- [Chroma](https://github.com/alecthomas/chroma) lexers for syntax highlighting
- [gh](https://github.com/cli/cli) for the GitHub functionality
- [delta](https://github.com/dandavison/delta) for viewing PR diffs

## Credits

Forked from [dlvhdr/gh-dash](https://github.com/dlvhdr/gh-dash) by Dolev Hadar, MIT licensed, and
ported from Go to Rust.
