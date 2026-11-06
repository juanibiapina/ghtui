//! `--debug` logging to ./debug.log:
//! kitchen-time stamps, level, caller, and $LOG_LEVEL (debug by default).

use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

struct Sink {
    file: File,
    min: Level,
}

static SINK: OnceLock<Mutex<Option<Sink>>> = OnceLock::new();

pub fn init(debug: bool) {
    let sink = if debug {
        let min = match std::env::var("LOG_LEVEL").unwrap_or_default().as_str() {
            "info" => Level::Info,
            "warn" => Level::Warn,
            "error" => Level::Error,
            _ => Level::Debug,
        };
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("debug.log")
            .ok()
            .map(|file| Sink { file, min })
    } else {
        None
    };
    let _ = SINK.set(Mutex::new(sink));
}

pub fn write(level: Level, caller: &str, msg: &str) {
    let Some(lock) = SINK.get() else { return };
    let mut guard = lock.lock().unwrap();
    let Some(sink) = guard.as_mut() else { return };
    if level < sink.min {
        return;
    }
    let tag = match level {
        Level::Debug => "DEBU",
        Level::Info => "INFO",
        Level::Warn => "WARN",
        Level::Error => "ERRO",
    };
    let time = chrono::Local::now().format("%-I:%M%p");
    let _ = writeln!(sink.file, "{time} {tag} <{caller}> {msg}");
}

#[macro_export]
macro_rules! log_at {
    ($lvl:expr, $($arg:tt)*) => {
        $crate::log::write($lvl, concat!(file!(), ":", line!()), &format!($($arg)*))
    };
}
#[macro_export]
macro_rules! debug { ($($arg:tt)*) => { $crate::log_at!($crate::log::Level::Debug, $($arg)*) }; }
#[macro_export]
macro_rules! info { ($($arg:tt)*) => { $crate::log_at!($crate::log::Level::Info, $($arg)*) }; }
#[macro_export]
macro_rules! warn { ($($arg:tt)*) => { $crate::log_at!($crate::log::Level::Warn, $($arg)*) }; }
#[macro_export]
macro_rules! error { ($($arg:tt)*) => { $crate::log_at!($crate::log::Level::Error, $($arg)*) }; }
