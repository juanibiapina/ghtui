//! gh-dash: a rich terminal UI for GitHub.

mod app;
mod bookmarks;
mod chroma;
mod complete;
mod config;
mod git;
mod github;
mod glamour;
mod highlight;
mod icons;
mod input;
mod keys;
mod log;
mod markdown;
mod model;
mod msg;
mod section;
mod tasks;
mod template;
mod text;
mod theme;
mod ui;
mod util;
mod util_time;
mod views;

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use clap::Parser;
use ratatui::crossterm::event::{
    self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event,
    KeyEventKind,
};
use ratatui::crossterm::execute;

use crate::app::App;
use crate::config::Location;

/// A rich terminal UI for GitHub that doesn't break your flow.
#[derive(Debug, Parser)]
#[command(
    name = "gh-dash",
    version,
    disable_version_flag = true,
    after_help = "Docs: https://github.com/juanibiapina/gh-dash"
)]
struct Args {
    /// Use this configuration file instead of a .gh-dash.yml in the git repo,
    /// $GH_DASH_CONFIG, or $XDG_CONFIG_HOME/gh-dash/config.yml
    #[arg(short, long, value_name = "FILE")]
    config: Option<String>,

    /// Write debug output to debug.log
    #[arg(long)]
    debug: bool,

    /// Print version and exit
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),
}

/// go-gh repository.Current: $GH_REPO, else the git remotes (upstream, github, origin first).
fn current_gh_repo() -> Option<(String, String)> {
    if let Ok(r) = std::env::var("GH_REPO") {
        let parts: Vec<&str> = r.trim_end_matches('/').split('/').collect();
        if parts.len() >= 2 {
            let n = parts.len();
            return Some((parts[n - 2].to_string(), parts[n - 1].to_string()));
        }
    }
    let out = Command::new("git").args(["remote", "-v"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let host = std::env::var("GH_HOST").unwrap_or_else(|_| "github.com".into());
    let text = String::from_utf8_lossy(&out.stdout);
    let mut remotes: Vec<(String, String)> = vec![];
    for line in text.lines() {
        let mut f = line.split_whitespace();
        let (Some(name), Some(url)) = (f.next(), f.next()) else {
            continue;
        };
        if !remotes.iter().any(|(n, _)| n == name) {
            remotes.push((name.to_string(), url.to_string()));
        }
    }
    let rank = |n: &str| match n {
        "upstream" => 0,
        "github" => 1,
        "origin" => 2,
        _ => 3,
    };
    remotes.sort_by_key(|(n, _)| rank(n));
    for (_, url) in remotes {
        let path = if let Some(rest) = url.strip_prefix(&format!("git@{host}:")) {
            rest.to_string()
        } else if let Some(idx) = url.find(&format!("{host}/")) {
            url[idx + host.len() + 1..].to_string()
        } else {
            continue;
        };
        let path = path.trim_end_matches(".git").trim_end_matches('/');
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() == 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    None
}

fn fatal(prefix: &str, err: impl std::fmt::Display) -> ! {
    crate::error!("{prefix}: failed parsing config file err={err}");
    eprintln!(
        "{} FATA {prefix} failed parsing config file err={err}",
        chrono::Local::now().to_rfc3339()
    );
    std::process::exit(1);
}

fn main() {
    let args = Args::parse();
    log::init(args.debug);
    crate::info!("Logging to debug.log");
    let repo_path: Option<PathBuf> = std::env::current_dir().ok();
    let gh_repo = current_gh_repo();
    match &repo_path {
        Some(p) => crate::info!("found git repo at path path={}", p.display()),
        None => crate::warn!("did not find git repo at current path"),
    }
    match &gh_repo {
        Some((o, n)) => crate::info!("found github repo at current path owner={o} name={n}"),
        None => crate::warn!("did not find github repo at current path"),
    }
    let cfg = match config::parse_config(&Location {
        repo_path: repo_path.clone(),
        config_flag: args.config.clone(),
    }) {
        Ok(c) => c,
        Err(e) => fatal("Reading config file", format!("{e:#}")),
    };
    let mut keys = keys::Keys::default();
    if let Err(e) = keys.rebind(&cfg.keybindings) {
        fatal("Reading config file", e);
    }

    // ui.initScreen: the repo view needs the origin URL; a repo without one is fatal.
    let mut repo_url = String::new();
    if config::feature_enabled("FF_REPO_VIEW")
        && let Some(p) = &repo_path
    {
        match git::origin_url(&p.display().to_string()) {
            Ok(u) => repo_url = u,
            Err(e) => fatal("Reading config file", e),
        }
    }

    let (pool, rx) = msg::Pool::new(12);
    let mut terminal = ratatui::init();
    // Like the Go version: report focus (refetch on return) and capture the mouse.
    let _ = execute!(std::io::stdout(), EnableFocusChange, EnableMouseCapture);
    let size = terminal.size().unwrap_or_default();
    let mut app = App::new(
        cfg,
        keys,
        pool,
        gh_repo,
        repo_path,
        size.width as usize,
        size.height as usize,
    );
    app.repo_url = repo_url;
    app.start();

    let mut dirty = true;
    loop {
        if dirty {
            app.ensure_sidebar_cache();
            let _ = terminal.draw(|f| ui::draw(f, &app));
            dirty = false;
        }
        if event::poll(Duration::from_millis(100)).unwrap_or(false) {
            match event::read() {
                Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => {
                    app.handle_key(&keys::key_string(&k));
                    dirty = true;
                }
                Ok(Event::FocusGained) => {
                    app.after_exec();
                    dirty = true;
                }
                Ok(Event::Resize(w, h)) => {
                    app.on_resize(w as usize, h as usize);
                    dirty = true;
                }
                _ => {}
            }
        }
        while let Ok(m) = rx.try_recv() {
            app.handle_msg(m);
            dirty = true;
        }
        if app.tick() {
            dirty = true;
        }
        if let Some(mut req) = app.exec.take() {
            let _ = execute!(std::io::stdout(), DisableFocusChange, DisableMouseCapture);
            ratatui::restore();
            let _ = std::io::stdout().flush();
            let res = req.cmd.status();
            terminal = ratatui::init();
            let _ = execute!(std::io::stdout(), EnableFocusChange, EnableMouseCapture);
            let _ = terminal.clear();
            for m in (req.on_done)(res) {
                app.handle_msg(m);
            }
            app.after_exec();
            dirty = true;
        }
        if app.quit {
            break;
        }
    }
    let _ = execute!(std::io::stdout(), DisableFocusChange, DisableMouseCapture);
    ratatui::restore();
}
