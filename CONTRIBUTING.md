# Contributing To `gh-dash`

## Working on the Code

### Installing Required Tooling

Our project uses [Devbox](https://github.com/jetpack-io/devbox) to manage its development environment.

Using Devbox will get your dev environment up and running easily and make sure we're all using the same tools with the same versions.

- Clone this repo

```sh
git clone git@github.com:juanibiapina/gh-dash.git && cd gh-dash
```

- Install `devbox`

```sh
curl -fsSL https://get.jetpack.io/devbox | bash
```

- Start the `devbox` shell and run the setup (will take a while on first time)

```sh
devbox shell
```

_This will create a shell where all required tools are installed._

- _(Optional)_ Set up `direnv` so `devbox shell` runs automatically
  - [direnv](https://www.jetify.com/devbox/docs/ide_configuration/direnv/) is a tool that allows setting unique environment variables per directory in your filesystem.
    - Install `direnv` with: `brew install direnv`
    - Add the following line at the end of the `~/.bashrc` file: `eval "$(direnv hook bash)"`
      - See [direnv's installation instructions](https://direnv.net/docs/hook.html) for other shells.
    - Enable `direnv` by running `direnv allow`
- _(Optional)_ Install the VSCode Extension
  - Follow [this guide](https://www.jetify.com/devbox/docs/ide_configuration/vscode/) to set up VSCode to automatically run `devbox shell`.

#### Troubleshooting

- delete the `.devbox` directory at the project's root

### Navigating the Codebase

To navigate our codebase with confidence, familiarize yourself with:

- [Bubbletea](https://github.com/charmbracelet/bubbletea) - the TUI framework we're using
- [The Elm architecture](https://guide.elm-lang.org/architecture/)
- [charmbracelet/glow](https://github.com/charmbracelet/glow) - for parsing and presenting Markdown

#### Code Structure

- `ui/` - this is the code that's responsible for rendering the different parts of the TUI
- `data/` - the code that fetches data from GitHub's GraphQL API
- `config/` - code to parse the user's `config.yml` file
- `utils/` - various utilities

### Debugging

- Write to the log by using Charm's `log` package
- Tail the log by running `task logs`
- Run `dash` in debug mode with `task debug` in another terminal window / pane

```go
import "charm.land/log/v2"

// more code...

log.Debug("some message", "someVariable", someVariable)
```

### Linting

CI runs this check, but for PRs from forks it may require maintainer approval before it starts. Running `task lint` locally saves a round-trip.

```sh
task lint
```

To auto-fix formatting issues (line length, imports, etc.):

```sh
task lint:fix
```

### Running the Docs Locally

- Run the docs site by running `task docs`

* Go to `localhost:4321` to view them
