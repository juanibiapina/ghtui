//! Notification bookmarks persisted as a JSON list of thread ids at
//! $XDG_STATE_HOME/gh-dash/bookmarks.json (data/bookmarks.go).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub struct Store {
    ids: Mutex<BTreeSet<String>>,
    path: Option<PathBuf>,
}

fn state_path() -> Option<PathBuf> {
    let dir = match std::env::var("XDG_STATE_HOME") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => dirs::home_dir()?.join(".local/state"),
    };
    Some(dir.join("gh-dash").join("bookmarks.json"))
}

impl Store {
    fn load() -> Store {
        let path = state_path();
        let ids = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default();
        Store {
            ids: Mutex::new(ids.into_iter().collect()),
            path,
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let ids: Vec<String> = self.ids.lock().unwrap().iter().cloned().collect();
        let Ok(data) = serde_json::to_string_pretty(&ids) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
            let tmp = dir.join(format!(".tmp-{}", std::process::id()));
            if std::fs::write(&tmp, data).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    pub fn has(&self, id: &str) -> bool {
        self.ids.lock().unwrap().contains(id)
    }

    pub fn toggle(&self, id: &str) -> bool {
        let now = {
            let mut ids = self.ids.lock().unwrap();
            if ids.remove(id) {
                false
            } else {
                ids.insert(id.to_string());
                true
            }
        };
        self.save();
        now
    }

    pub fn remove(&self, id: &str) {
        let changed = self.ids.lock().unwrap().remove(id);
        if changed {
            self.save();
        }
    }

    pub fn all(&self) -> Vec<String> {
        self.ids.lock().unwrap().iter().cloned().collect()
    }
}

pub fn store() -> &'static Store {
    static STORE: OnceLock<Store> = OnceLock::new();
    STORE.get_or_init(Store::load)
}
