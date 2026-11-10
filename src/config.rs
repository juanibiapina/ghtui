//! Configuration loading:
//! defaults are overlaid by the global config, then by a user-provided config
//! (flag, $GHTUI_CONFIG or a repo-local .ghtui.yml). Each file's `include`
//! entries load before the file itself. Keybinding groups are unioned by key;
//! section lists are replaced by the last layer that declares them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Deserializer, Serialize};
use serde_yaml::{Mapping, Value};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum ViewType {
    Notifications,
    #[default]
    Prs,
    Issues,
    Repo,
}

impl ViewType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ViewType::Notifications => "notifications",
            ViewType::Prs => "prs",
            ViewType::Issues => "issues",
            ViewType::Repo => "repo",
        }
    }
}

impl Serialize for ViewType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ViewType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.to_lowercase().as_str() {
            "notifications" => ViewType::Notifications,
            "issues" => ViewType::Issues,
            "repo" => ViewType::Repo,
            _ => ViewType::Prs,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

impl ColumnConfig {
    fn w(width: usize) -> Self {
        Self {
            width: Some(width),
            hidden: None,
        }
    }
    fn wh(width: usize, hidden: bool) -> Self {
        Self {
            width: Some(width),
            hidden: Some(hidden),
        }
    }
    fn h(hidden: bool) -> Self {
        Self {
            width: None,
            hidden: Some(hidden),
        }
    }
    /// Section values win over defaults when set.
    pub fn merged(&self, section: &ColumnConfig) -> ColumnConfig {
        ColumnConfig {
            width: section.width.or(self.width),
            hidden: section.hidden.or(self.hidden),
        }
    }
    pub fn is_hidden(&self) -> bool {
        self.hidden.unwrap_or(false)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PrsLayoutConfig {
    pub updated_at: ColumnConfig,
    pub created_at: ColumnConfig,
    pub repo: ColumnConfig,
    pub author: ColumnConfig,
    pub author_icon: ColumnConfig,
    pub labels: ColumnConfig,
    pub assignees: ColumnConfig,
    pub title: ColumnConfig,
    pub base: ColumnConfig,
    pub review_status: ColumnConfig,
    pub state: ColumnConfig,
    pub ci: ColumnConfig,
    pub lines: ColumnConfig,
    pub num_comments: ColumnConfig,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IssuesLayoutConfig {
    pub updated_at: ColumnConfig,
    pub created_at: ColumnConfig,
    pub state: ColumnConfig,
    pub repo: ColumnConfig,
    pub title: ColumnConfig,
    pub creator: ColumnConfig,
    pub creator_icon: ColumnConfig,
    pub assignees: ColumnConfig,
    pub comments: ColumnConfig,
    pub reactions: ColumnConfig,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutConfig {
    pub prs: PrsLayoutConfig,
    pub issues: IssuesLayoutConfig,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PrsSectionConfig {
    pub title: String,

    pub filters: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "is_default_prs_layout")]
    pub layout: PrsLayoutConfig,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub view_type: Option<ViewType>,
}

fn is_default_prs_layout(_: &PrsLayoutConfig) -> bool {
    true
}
fn is_default_issues_layout(_: &IssuesLayoutConfig) -> bool {
    true
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IssuesSectionConfig {
    pub title: String,

    pub filters: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "is_default_issues_layout")]
    pub layout: IssuesLayoutConfig,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationsSectionConfig {
    pub title: String,

    pub filters: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewConfig {
    pub open: bool,
    pub width: f64,
    pub height: f64,
    pub position: String,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            open: true,
            width: 0.45,
            height: 0.60,
            position: "auto".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Defaults {
    pub preview: PreviewConfig,
    pub prs_limit: usize,
    pub pr_approve_comment: String,
    pub issues_limit: usize,
    pub notifications_limit: usize,
    pub view: ViewType,
    pub layout: LayoutConfig,
    pub refetch_interval_minutes: u64,
    pub date_format: String,
}

impl Default for Defaults {
    fn default() -> Self {
        let time_w = "2mo  ".len();
        Defaults {
            preview: PreviewConfig::default(),
            prs_limit: 20,
            pr_approve_comment: "LGTM".into(),
            issues_limit: 20,
            notifications_limit: 20,
            view: ViewType::Prs,
            refetch_interval_minutes: 30,
            date_format: String::new(),
            layout: LayoutConfig {
                prs: PrsLayoutConfig {
                    updated_at: ColumnConfig::w(time_w),
                    created_at: ColumnConfig::w(time_w),
                    repo: ColumnConfig::w(20),
                    author: ColumnConfig::w(15),
                    author_icon: ColumnConfig::h(false),
                    labels: ColumnConfig::wh(22, true),
                    assignees: ColumnConfig::wh(20, true),
                    base: ColumnConfig::wh(15, true),
                    lines: ColumnConfig::w(" +31.4k -31.6k ".len()),
                    ..Default::default()
                },
                issues: IssuesLayoutConfig {
                    updated_at: ColumnConfig::w(time_w),
                    created_at: ColumnConfig::w(time_w),
                    repo: ColumnConfig::w(15),
                    creator: ColumnConfig::w(10),
                    creator_icon: ColumnConfig::h(false),
                    assignees: ColumnConfig::wh(20, true),
                    ..Default::default()
                },
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RepoConfig {
    pub branches_refetch_interval_seconds: u64,
    pub prs_refetch_interval_seconds: u64,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            branches_refetch_interval_seconds: 30,
            prs_refetch_interval_seconds: 300,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Keybinding {
    pub key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub builtin: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Keybindings {
    pub universal: Vec<Keybinding>,
    pub issues: Vec<Keybinding>,
    pub prs: Vec<Keybinding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<Keybinding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notifications: Vec<Keybinding>,
    #[serde(rename = "completions", skip_serializing_if = "Vec::is_empty")]
    pub cmp: Vec<Keybinding>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Pager {
    pub diff: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorThemeIcon {
    pub newcontributor: String,
    pub contributor: String,
    pub collaborator: String,
    pub member: String,
    pub owner: String,
    pub unknownrole: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorThemeText {
    pub primary: String,
    pub secondary: String,
    pub inverted: String,
    pub faint: String,
    pub warning: String,
    pub success: String,
    pub error: String,
    pub actor: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorThemeBorder {
    pub primary: String,
    pub secondary: String,
    pub faint: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorThemeBackground {
    pub selected: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorTheme {
    pub icon: ColorThemeIcon,
    pub text: ColorThemeText,
    pub background: ColorThemeBackground,
    pub border: ColorThemeBorder,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IconTheme {
    pub newcontributor: String,
    pub contributor: String,
    pub collaborator: String,
    pub member: String,
    pub owner: String,
    pub unknownrole: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TableUiThemeConfig {
    pub show_separator: bool,
    pub compact: bool,
}

impl Default for TableUiThemeConfig {
    fn default() -> Self {
        Self {
            show_separator: true,
            compact: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiThemeConfig {
    pub sections_show_count: bool,
    pub table: TableUiThemeConfig,
}

impl Default for UiThemeConfig {
    fn default() -> Self {
        Self {
            sections_show_count: true,
            table: TableUiThemeConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub ui: UiThemeConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colors: Option<ColorTheme>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icons: Option<IconTheme>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    pub pr_sections: Vec<PrsSectionConfig>,
    pub issues_sections: Vec<IssuesSectionConfig>,
    pub notifications_sections: Vec<NotificationsSectionConfig>,
    pub repo: RepoConfig,
    pub defaults: Defaults,
    pub keybindings: Keybindings,
    pub repo_paths: HashMap<String, String>,
    pub theme: ThemeConfig,
    pub pager: Pager,
    pub confirm_quit: bool,
    pub quit_on_esc: bool,
    pub show_author_icons: bool,
    pub smart_filtering_at_launch: bool,
}

fn sec<T: Default>(title: &str, filters: &str, f: impl Fn(String, String) -> T) -> T {
    f(title.to_string(), filters.to_string())
}

impl Default for Config {
    fn default() -> Self {
        let pr = |t: String, f: String| PrsSectionConfig {
            title: t,
            filters: f,
            ..Default::default()
        };
        let is = |t: String, f: String| IssuesSectionConfig {
            title: t,
            filters: f,
            ..Default::default()
        };
        let nt = |t: String, f: String| NotificationsSectionConfig {
            title: t,
            filters: f,
            limit: None,
        };
        Config {
            include: vec![],
            pr_sections: vec![
                sec("My Pull Requests", "is:open author:@me", pr),
                sec("Needs My Review", "is:open review-requested:@me", pr),
                sec("Involved", "is:open involves:@me -author:@me", pr),
            ],
            issues_sections: vec![
                sec("My Issues", "is:open author:@me", is),
                sec("Assigned", "is:open assignee:@me", is),
                sec("Involved", "is:open involves:@me -author:@me", is),
            ],
            notifications_sections: vec![
                sec("All", "", nt),
                sec("Created", "reason:author", nt),
                sec("Participating", "reason:participating", nt),
                sec("Mentioned", "reason:mention", nt),
                sec("Review Requested", "reason:review-requested", nt),
                sec("Assigned", "reason:assign", nt),
                sec("Subscribed", "reason:subscribed", nt),
                sec("Team Mentioned", "reason:team-mention", nt),
            ],
            repo: RepoConfig::default(),
            defaults: Defaults::default(),
            keybindings: Keybindings::default(),
            repo_paths: HashMap::new(),
            theme: ThemeConfig::default(),
            pager: Pager::default(),
            confirm_quit: false,
            quit_on_esc: false,
            show_author_icons: true,
            smart_filtering_at_launch: true,
        }
    }
}

impl Config {
    pub fn diff_pager_env(&self) -> Vec<(String, String)> {
        let mut diff = self.pager.diff.clone();
        if diff.is_empty() {
            diff = "less".into();
        }
        if diff == "delta" {
            diff = "delta --paging always".into();
        }
        vec![("LESS".into(), "CRX".into()), ("GH_PAGER".into(), diff)]
    }
}

pub struct Location {
    pub repo_path: Option<PathBuf>,
    pub config_flag: Option<String>,
}

pub fn feature_enabled(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

const KEYBINDING_UNION_TYPES: [&str; 4] = ["universal", "prs", "issues", "completions"];
const SECTION_TYPES: [&str; 3] = ["prSections", "issuesSections", "notificationsSections"];

/// `$XDG_CONFIG_HOME/ghtui/config.yml`
fn global_config_path() -> Result<PathBuf> {
    let dir = match std::env::var("XDG_CONFIG_HOME") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => dirs::home_dir()
            .ok_or_else(|| anyhow!("no home dir"))?
            .join(".config"),
    };
    Ok(dir.join("ghtui").join("config.yml"))
}

pub fn default_config_yaml() -> String {
    serde_yaml::to_string(&Config::default()).unwrap_or_default()
}

fn global_config_path_or_create() -> Result<PathBuf> {
    let path = global_config_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| {
            format!(
                "Couldn't find a config.yml or a config.yaml configuration file.\nCreate one under: {}\n\nExample of a config.yml file:\n{}",
                path.display(),
                default_config_yaml()
            )
        })?;
    }
    if !path.exists() {
        std::fs::write(&path, default_config_yaml())
            .with_context(|| format!("writing default config to {}", path.display()))?;
    }
    Ok(path)
}

fn provided_config_path(loc: &Location) -> Option<PathBuf> {
    if let Some(flag) = loc.config_flag.as_ref().filter(|f| !f.is_empty()) {
        return Some(PathBuf::from(flag));
    }
    if let Ok(env) = std::env::var("GHTUI_CONFIG")
        && !env.is_empty()
    {
        return Some(PathBuf::from(env));
    }
    if let Some(repo) = &loc.repo_path {
        for ext in ["yml", "yaml"] {
            let p = repo.join(format!(".ghtui.{ext}"));
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

fn read_yaml(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed parsing config at path {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Value::Mapping(Mapping::new()));
    }
    let v: Value = serde_yaml::from_str(&text)
        .with_context(|| format!("failed parsing config at path {}", path.display()))?;
    Ok(match v {
        Value::Null => Value::Mapping(Mapping::new()),
        other => normalize_keys(other),
    })
}

/// mapstructure matches struct fields case-insensitively, so `Open:` and `Title:`
/// mean `open` and `title`. repoPaths keys are data.
fn normalize_keys(v: Value) -> Value {
    match v {
        Value::Mapping(m) => Value::Mapping(
            m.into_iter()
                .map(|(k, v)| {
                    let keep = k.as_str() == Some("repoPaths");
                    let k = match k {
                        Value::String(s) => {
                            let mut c = s.chars();
                            let s = match c.next() {
                                Some(f) if f.is_ascii_uppercase() => {
                                    f.to_ascii_lowercase().to_string() + c.as_str()
                                }
                                _ => s,
                            };
                            Value::String(s)
                        }
                        other => other,
                    };
                    (k, if keep { v } else { normalize_keys(v) })
                })
                .collect(),
        ),
        Value::Sequence(s) => Value::Sequence(s.into_iter().map(normalize_keys).collect()),
        other => other,
    }
}

fn resolve_include(including: &Path, include: &str) -> PathBuf {
    if include.starts_with('~') {
        return PathBuf::from(crate::util::expand_home(include));
    }
    let p = Path::new(include);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    including.parent().unwrap_or(Path::new(".")).join(p)
}

fn deep_merge(dest: &mut Value, src: Value) {
    match (dest, src) {
        (Value::Mapping(d), Value::Mapping(s)) => {
            for (k, v) in s {
                match d.get_mut(&k) {
                    Some(existing) if existing.is_mapping() && v.is_mapping() => {
                        deep_merge(existing, v)
                    }
                    _ => {
                        d.insert(k, v);
                    }
                }
            }
        }
        (d, s) => *d = s,
    }
}

fn keybindings_of(layer: &Value, typ: &str) -> Vec<Value> {
    layer
        .get("keybindings")
        .and_then(|k| k.get(typ))
        .and_then(|l| l.as_sequence())
        .cloned()
        .unwrap_or_default()
}

/// Mirrors mergeOption(): union keybindings by key, replace section lists, deep-merge the rest.
fn merge_layer(dest: &mut Value, layer: Value) {
    let mut unioned: Vec<(&str, Vec<Value>)> = vec![];
    for typ in KEYBINDING_UNION_TYPES {
        let mut merged: Vec<Value> = vec![];
        for kb in keybindings_of(dest, typ)
            .into_iter()
            .chain(keybindings_of(&layer, typ))
        {
            let key = kb.get("key").cloned();
            if key.is_none() {
                continue;
            }
            if let Some(pos) = merged.iter().position(|m| m.get("key") == key.as_ref()) {
                merged[pos] = kb;
            } else {
                merged.push(kb);
            }
        }
        unioned.push((typ, merged));
    }
    let sections: Vec<(&str, Value)> = SECTION_TYPES
        .iter()
        .filter_map(|k| layer.get(*k).map(|v| (*k, v.clone())))
        .collect();

    deep_merge(dest, layer);

    if let Value::Mapping(d) = dest {
        let kb = d
            .entry(Value::from("keybindings"))
            .or_insert_with(|| Value::Mapping(Mapping::new()));
        if !kb.is_mapping() {
            *kb = Value::Mapping(Mapping::new());
        }
        if let Value::Mapping(kbm) = kb {
            for (typ, binds) in unioned {
                kbm.insert(Value::from(typ), Value::Sequence(binds));
            }
        }
        for (k, v) in sections {
            d.insert(Value::from(k), v);
        }
    }
}

fn load_with_includes(path: &Path, acc: &mut Value, seen: &mut HashSet<PathBuf>) -> Result<()> {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(abs) {
        return Ok(());
    }
    let layer = read_yaml(path)?;
    let includes: Vec<String> = layer
        .get("include")
        .and_then(|v| v.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    for inc in includes {
        load_with_includes(&resolve_include(path, &inc), acc, seen)?;
    }
    merge_layer(acc, layer);
    Ok(())
}

fn validate_color(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    let hex = regex::Regex::new(r"^#([a-fA-F0-9]{6}|[a-fA-F0-9]{3})$").unwrap();
    hex.is_match(s) || s.parse::<u16>().map(|n| n <= 255).unwrap_or(false)
}

fn validate(cfg: &Config) -> Result<()> {
    if cfg.defaults.preview.width <= 0.0 {
        anyhow::bail!(
            "Key: 'Config.defaults.preview.width' Error:Field validation for 'width' failed on the 'gt' tag"
        );
    }
    if let Some(c) = &cfg.theme.colors {
        let fields = [
            ("icon.newcontributor", &c.icon.newcontributor),
            ("icon.contributor", &c.icon.contributor),
            ("icon.collaborator", &c.icon.collaborator),
            ("icon.member", &c.icon.member),
            ("icon.owner", &c.icon.owner),
            ("icon.unknownrole", &c.icon.unknownrole),
            ("text.primary", &c.text.primary),
            ("text.secondary", &c.text.secondary),
            ("text.inverted", &c.text.inverted),
            ("text.faint", &c.text.faint),
            ("text.warning", &c.text.warning),
            ("text.success", &c.text.success),
            ("text.error", &c.text.error),
            ("text.actor", &c.text.actor),
            ("background.selected", &c.background.selected),
            ("border.primary", &c.border.primary),
            ("border.secondary", &c.border.secondary),
            ("border.faint", &c.border.faint),
        ];
        for (name, v) in fields {
            if !validate_color(v) {
                anyhow::bail!(
                    "Key: 'Config.theme.colors.{name}' Error:Field validation for '{name}' failed on the 'color' tag"
                );
            }
        }
    }
    Ok(())
}

pub fn parse_config(loc: &Location) -> Result<Config> {
    let global = global_config_path_or_create()?;
    let mut acc = Value::Mapping(Mapping::new());
    let mut seen = HashSet::new();
    load_with_includes(&global, &mut acc, &mut seen)?;
    if let Some(user) = provided_config_path(loc) {
        let mut seen = HashSet::new();
        load_with_includes(&user, &mut acc, &mut seen)?;
    }
    from_layers(acc)
}

fn from_layers(acc: Value) -> Result<Config> {
    let mut base = serde_yaml::to_value(Config::default())?;
    // Defaults are an overlay base, not a merge layer, so keybinding union rules don't apply.
    deep_merge(&mut base, acc);
    let mut cfg: Config = serde_yaml::from_value(base).context("failed decoding config")?;
    if cfg.defaults.view == ViewType::Repo && !feature_enabled("FF_REPO_VIEW") {
        cfg.defaults.view = ViewType::Prs;
    }
    validate(&cfg)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layers(yamls: &[&str]) -> Config {
        let mut acc = Value::Mapping(Mapping::new());
        for y in yamls {
            merge_layer(&mut acc, normalize_keys(serde_yaml::from_str(y).unwrap()));
        }
        from_layers(acc).unwrap()
    }

    #[test]
    fn defaults_survive_partial_overrides() {
        let cfg =
            layers(&["defaults:\n  layout:\n    prs:\n      updatedAt:\n        hidden: true\n"]);
        assert_eq!(cfg.defaults.layout.prs.updated_at.width, Some(5));
        assert_eq!(cfg.defaults.layout.prs.updated_at.hidden, Some(true));
        assert_eq!(cfg.pr_sections.len(), 3);
        assert!(cfg.theme.ui.table.show_separator);
    }

    #[test]
    fn keybindings_union_and_sections_replace() {
        let cfg = layers(&[
            "keybindings:\n  prs:\n  - key: a\n    command: one\n  - key: b\n    command: two\nprSections:\n- title: A\n  filters: x\n- title: B\n  filters: y\n",
            "keybindings:\n  prs:\n  - key: a\n    command: three\nprSections:\n- title: C\n  filters: z\n",
        ]);
        let prs = &cfg.keybindings.prs;
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].command, "three");
        assert_eq!(prs[1].command, "two");
        assert_eq!(cfg.pr_sections.len(), 1);
        assert_eq!(cfg.pr_sections[0].title, "C");
    }

    #[test]
    fn capitalized_keys_from_go_marshalled_config() {
        let cfg = layers(&[
            "defaults:\n  preview:\n    Open: false\n    width: 0.5\nrepoPaths:\n  Owner/repo: /x\n",
        ]);
        assert_eq!(
            cfg.repo_paths.get("Owner/repo").map(String::as_str),
            Some("/x")
        );
        assert!(!cfg.defaults.preview.open);
        assert_eq!(cfg.defaults.preview.width, 0.5);
        assert_eq!(cfg.defaults.preview.height, 0.6);
    }

    #[test]
    fn invalid_color_is_rejected() {
        let mut acc = Value::Mapping(Mapping::new());
        merge_layer(
            &mut acc,
            serde_yaml::from_str("theme:\n  colors:\n    text:\n      primary: red\n").unwrap(),
        );
        assert!(from_layers(acc).is_err());
    }
}
