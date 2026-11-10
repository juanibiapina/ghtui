//! ghtui: a rich terminal UI for GitHub.

mod app;
mod bookmarks;
mod complete;
mod config;
mod disk_cache;
mod git;
mod github;
mod help;
mod highlight;
mod icons;
mod input;
mod keys;
mod log;
mod markdown;
mod model;
mod mouse;
mod msg;
mod news;
mod refresh;
mod section;
mod store;
mod subject;
mod tasks;
mod template;
mod text;
mod theme;
mod ui;
mod util;
mod views;

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use clap::Parser;
use ratatui::DefaultTerminal;
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
    name = "ghtui",
    version,
    disable_version_flag = true,
    after_help = "Docs: https://github.com/juanibiapina/ghtui"
)]
struct Args {
    /// Use this configuration file instead of a .ghtui.yml in the git repo,
    /// $GHTUI_CONFIG, or $XDG_CONFIG_HOME/ghtui/config.yml
    #[arg(short, long, value_name = "FILE")]
    config: Option<String>,

    /// Write debug output to debug.log
    #[arg(long)]
    debug: bool,

    /// Print version and exit
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),
}

/// The current repository as gh finds it: $GH_REPO, else the git remotes (upstream, github, origin first).
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

fn enter_terminal(mouse: bool) -> DefaultTerminal {
    let terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableFocusChange);
    if mouse {
        let _ = execute!(std::io::stdout(), EnableMouseCapture);
    }
    terminal
}

fn leave_terminal(mouse: bool) {
    if mouse {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
    }
    let _ = execute!(std::io::stdout(), DisableFocusChange);
    ratatui::restore();
}

fn fatal(prefix: &str, err: impl std::fmt::Display) -> ! {
    crate::error!("{prefix}: failed parsing config file err={err}");
    eprintln!(
        "{} FATA {prefix} failed parsing config file err={err}",
        chrono::Local::now().to_rfc3339()
    );
    std::process::exit(1);
}

/// Reads the signed-in account's cache: lists first, then the large details file.
fn load_cache(tx: std::sync::mpsc::Sender<msg::Msg>) {
    let started = std::time::Instant::now();
    let cache = github::account_key()
        .ok()
        .and_then(|k| disk_cache::DiskCache::for_account(&k));
    let lists = cache.as_ref().map(|c| c.load_lists()).unwrap_or_default();
    crate::info!(
        "cache lists read sections={} ms={}",
        lists.sections.len(),
        started.elapsed().as_millis()
    );
    let _ = tx.send(msg::Msg::CacheLoaded {
        lists: Box::new(lists),
        cache: cache.clone(),
    });
    let details = cache.map(|c| c.load_details()).unwrap_or_default();
    crate::info!(
        "cache details read prs={} ms={}",
        details.prs.len(),
        started.elapsed().as_millis()
    );
    let _ = tx.send(msg::Msg::DetailsLoaded(Box::new(details)));
}

fn main() {
    let launched = std::time::Instant::now();
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

    // The repo view needs the origin URL; a repo without one is fatal.
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
    let mouse = cfg.mouse;
    if mouse {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = execute!(std::io::stdout(), DisableMouseCapture);
            hook(info);
        }));
    }
    // Focus reports refetch on return.
    let mut terminal = enter_terminal(mouse);
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
    let tx = app.pool.msgs.clone();
    std::thread::spawn(move || load_cache(tx));

    let mut dirty = true;
    let mut first_frame = true;
    let mut hits = mouse::HitMap::default();
    loop {
        // A resize before crossterm listens for SIGWINCH sends no event; compare sizes instead.
        if let Ok(size) = terminal.size()
            && (size.width as usize, size.height as usize) != (app.width, app.height)
        {
            app.on_resize(size.width as usize, size.height as usize);
            dirty = true;
        }
        if dirty {
            let started = std::time::Instant::now();
            app.ensure_preview_cache();
            let preview = started.elapsed();
            let _ = terminal.draw(|f| hits = ui::draw(f, &app));
            let frame = started.elapsed();
            if first_frame {
                crate::info!(
                    "first frame since launch ms={}",
                    launched.elapsed().as_millis()
                );
                first_frame = false;
            }
            crate::debug!(
                "frame us={} preview_us={}",
                frame.as_micros(),
                preview.as_micros()
            );
            dirty = false;
        }
        // Poll briefly while background jobs run so their results show without delay.
        let wait = if app.pool.busy() || app.store_busy() {
            10
        } else {
            100
        };
        if event::poll(Duration::from_millis(wait)).unwrap_or(false) {
            match event::read() {
                Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => {
                    app.handle_key(&keys::key_string(&k));
                    dirty = true;
                }
                Ok(Event::Mouse(m)) => {
                    if app.handle_mouse(&m, &hits) {
                        dirty = true;
                    }
                }
                Ok(Event::FocusGained) => {
                    app.on_focus(true);
                    dirty = true;
                }
                Ok(Event::FocusLost) => {
                    app.on_focus(false);
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
            leave_terminal(mouse);
            let _ = std::io::stdout().flush();
            let res = req.cmd.status();
            terminal = enter_terminal(mouse);
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
    leave_terminal(mouse);
    app.save_cache(true);
}
