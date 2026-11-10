//! Configuration loading:
//! defaults are overlaid by the global config, then by a user-provided config
//! (flag, $GHTUI_CONFIG or a repo-local .ghtui.yml). Each file's `include`
//! entries load before the file itself. Keybinding groups are unioned by key;
//! section lists are replaced by the last layer that declares them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::format::{Item, StrftimeItems};
use chrono::{DateTime, Local, Utc};
use ratatui::style::Color;
use serde::{Deserialize, Deserializer, Serialize};
use serde_yaml::{Mapping, Value};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewType {
    Notifications,
    #[default]
    Prs,
    Issues,
    Repo,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColumnConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

impl ColumnConfig {
    fn with_width(width: usize) -> Self {
        Self {
            width: Some(width),
            hidden: None,
        }
    }
    fn with_width_and_hidden(width: usize, hidden: bool) -> Self {
        Self {
            width: Some(width),
            hidden: Some(hidden),
        }
    }
    fn with_hidden(hidden: bool) -> Self {
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
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct LayoutConfig {
    pub prs: PrsLayoutConfig,
    pub issues: IssuesLayoutConfig,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct IssuesSectionConfig {
    pub title: String,

    pub filters: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "is_default_issues_layout")]
    pub layout: IssuesLayoutConfig,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NotificationsSectionConfig {
    pub title: String,

    pub filters: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewConfig {
    pub open: bool,
    pub width: f64,
    pub height: f64,
    pub position: PreviewPosition,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewPosition {
    #[default]
    Auto,
    Right,
    Bottom,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            open: true,
            width: 0.45,
            height: 0.60,
            position: PreviewPosition::Auto,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Defaults {
    pub preview: PreviewConfig,
    pub prs_limit: usize,
    pub pr_approve_comment: String,
    pub issues_limit: usize,
    pub notifications_limit: usize,
    pub view: ViewType,
    pub layout: LayoutConfig,
    pub refetch_interval_minutes: u64,
    pub date_format: DateFormat,
}

/// How tables show dates: relative to now (`3d`) or a chrono strftime format.
#[derive(Clone, Debug, Default)]
pub enum DateFormat {
    #[default]
    Relative,
    Strftime {
        format: String,
        items: Vec<Item<'static>>,
    },
}

impl DateFormat {
    pub fn format(&self, t: DateTime<Utc>) -> String {
        match self {
            DateFormat::Relative => crate::util::time_elapsed(t),
            DateFormat::Strftime { items, .. } => t
                .with_timezone(&Local)
                .format_with_items(items.iter())
                .to_string(),
        }
    }
}

impl Serialize for DateFormat {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            DateFormat::Relative => s.serialize_str("relative"),
            DateFormat::Strftime { format, .. } => s.serialize_str(format),
        }
    }
}

impl<'de> Deserialize<'de> for DateFormat {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let format = String::deserialize(d)?;
        if format == "relative" {
            return Ok(DateFormat::Relative);
        }
        let items = StrftimeItems::new(&format).parse_to_owned().map_err(|_| {
            serde::de::Error::custom(format!(
                "'{format}' is not a strftime format like \"%Y-%m-%d %H:%M\""
            ))
        })?;
        Ok(DateFormat::Strftime { format, items })
    }
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
            date_format: DateFormat::Relative,
            layout: LayoutConfig {
                prs: PrsLayoutConfig {
                    updated_at: ColumnConfig::with_width(time_w),
                    created_at: ColumnConfig::with_width(time_w),
                    repo: ColumnConfig::with_width(20),
                    author: ColumnConfig::with_width(15),
                    author_icon: ColumnConfig::with_hidden(false),
                    labels: ColumnConfig::with_width_and_hidden(22, true),
                    assignees: ColumnConfig::with_width_and_hidden(20, true),
                    base: ColumnConfig::with_width_and_hidden(15, true),
                    lines: ColumnConfig::with_width(" +31.4k -31.6k ".len()),
                    ..Default::default()
                },
                issues: IssuesLayoutConfig {
                    updated_at: ColumnConfig::with_width(time_w),
                    created_at: ColumnConfig::with_width(time_w),
                    repo: ColumnConfig::with_width(15),
                    creator: ColumnConfig::with_width(10),
                    creator_icon: ColumnConfig::with_hidden(false),
                    assignees: ColumnConfig::with_width_and_hidden(20, true),
                    ..Default::default()
                },
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
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

/// A key bound to a built-in action or a shell command.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(try_from = "RawKeybinding", into = "RawKeybinding")]
pub struct Keybinding {
    pub key: String,
    pub name: Option<String>,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Builtin(String),
    Command(String),
}

impl Keybinding {
    pub fn command(&self) -> Option<&str> {
        match &self.action {
            Action::Command(c) => Some(c),
            Action::Builtin(_) => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKeybinding {
    key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    builtin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

impl TryFrom<RawKeybinding> for Keybinding {
    type Error = String;
    fn try_from(raw: RawKeybinding) -> Result<Self, String> {
        let action = match (raw.builtin, raw.command) {
            (Some(b), None) => Action::Builtin(b),
            (None, Some(c)) => Action::Command(c),
            (Some(_), Some(_)) => {
                return Err(format!("key '{}' sets both builtin and command", raw.key));
            }
            (None, None) => {
                return Err(format!("key '{}' needs a builtin or a command", raw.key));
            }
        };
        Ok(Keybinding {
            key: raw.key,
            name: raw.name,
            action,
        })
    }
}

impl From<Keybinding> for RawKeybinding {
    fn from(kb: Keybinding) -> Self {
        let (builtin, command) = match kb.action {
            Action::Builtin(b) => (Some(b), None),
            Action::Command(c) => (None, Some(c)),
        };
        RawKeybinding {
            key: kb.key,
            command,
            builtin,
            name: kb.name,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keybindings {
    pub universal: Vec<Keybinding>,
    pub issues: Vec<Keybinding>,
    pub prs: Vec<Keybinding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<Keybinding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notifications: Vec<Keybinding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub completions: Vec<Keybinding>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pager {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// A program that reviews a PR, run with the PR's URL as its last
    /// argument in place of `gh pr diff`, such as `deltoids pr`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<String>,
}

/// A hex color (`#rrggbb` or `#rgb`) or an ANSI 256 color number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConfigColor(pub Color);

impl Serialize for ConfigColor {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Color::Rgb(r, g, b) => s.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}")),
            Color::Indexed(i) => s.serialize_str(&i.to_string()),
            other => s.serialize_str(&other.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for ConfigColor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        crate::theme::parse_color(&text)
            .map(ConfigColor)
            .ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "'{text}' is not a color: use #rrggbb, #rgb or a number from 0 to 255"
                ))
            })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ColorThemeIcon {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_contributor: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contributor: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collaborator: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_role: Option<ConfigColor>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ColorThemeText {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inverted: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faint: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<ConfigColor>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ColorThemeBorder {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary: Option<ConfigColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faint: Option<ConfigColor>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ColorThemeBackground {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<ConfigColor>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorTheme {
    pub icon: ColorThemeIcon,
    pub text: ColorThemeText,
    pub background: ColorThemeBackground,
    pub border: ColorThemeBorder,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct IconTheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_contributor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contributor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collaborator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_role: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    pub ui: UiThemeConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colors: Option<ColorTheme>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icons: Option<IconTheme>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
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
    pub mouse: bool,
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
            mouse: true,
            show_author_icons: true,
            smart_filtering_at_launch: true,
        }
    }
}

impl Config {
    pub fn diff_pager_env(&self) -> Vec<(String, String)> {
        let diff = match self.pager.diff.as_deref() {
            None => "less".to_string(),
            Some("delta") => "delta --paging always".to_string(),
            Some(other) => other.to_string(),
        };
        vec![("LESS".into(), "CRX".into()), ("GH_PAGER".into(), diff)]
    }

    /// The command `pager.pr` sets, unless it is blank.
    pub fn pr_reviewer(&self) -> Option<&str> {
        self.pager
            .pr
            .as_deref()
            .map(str::trim)
            .filter(|cmd| !cmd.is_empty())
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
        other => other,
    })
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

/// Unions keybindings by key, replaces section lists and deep-merges the rest.
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

fn validate(cfg: &Config) -> Result<()> {
    if cfg.defaults.preview.width <= 0.0 {
        anyhow::bail!("defaults.preview.width must be greater than 0");
    }
    if cfg.defaults.preview.height <= 0.0 {
        anyhow::bail!("defaults.preview.height must be greater than 0");
    }
    let filters = [
        (
            "prSections",
            cfg.pr_sections
                .iter()
                .map(|s| &s.filters)
                .collect::<Vec<_>>(),
        ),
        (
            "issuesSections",
            cfg.issues_sections.iter().map(|s| &s.filters).collect(),
        ),
        (
            "notificationsSections",
            cfg.notifications_sections
                .iter()
                .map(|s| &s.filters)
                .collect(),
        ),
    ];
    for (list, filters) in filters {
        for (i, f) in filters.into_iter().enumerate() {
            crate::template::render(f, &HashMap::new())
                .map_err(|e| anyhow!("invalid config at {list}[{i}].filters: {e}"))?;
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
    let mut cfg: Config = serde_path_to_error::deserialize(base).map_err(|e| {
        let path = e.path().to_string();
        anyhow!("invalid config at {path}: {}", e.into_inner())
    })?;
    if cfg.defaults.view == ViewType::Repo && !feature_enabled("FF_REPO_VIEW") {
        cfg.defaults.view = ViewType::Prs;
    }
    validate(&cfg)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(yamls: &[&str]) -> Result<Config> {
        let mut acc = Value::Mapping(Mapping::new());
        for y in yamls {
            merge_layer(&mut acc, serde_yaml::from_str(y).unwrap());
        }
        from_layers(acc)
    }

    fn layers(yamls: &[&str]) -> Config {
        load(yamls).unwrap()
    }

    fn error(yaml: &str) -> String {
        load(&[yaml]).unwrap_err().to_string()
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
        assert_eq!(prs[0].command(), Some("three"));
        assert_eq!(prs[1].command(), Some("two"));
        assert_eq!(cfg.pr_sections.len(), 1);
        assert_eq!(cfg.pr_sections[0].title, "C");
    }

    #[test]
    fn pager_pr_names_the_pr_reviewer() {
        assert_eq!(
            layers(&["pager:\n  pr: ' deltoids pr '\n"]).pr_reviewer(),
            Some("deltoids pr")
        );
        assert_eq!(layers(&["pager:\n  pr: ''\n"]).pr_reviewer(), None);
        assert_eq!(layers(&["pager:\n  diff: delta\n"]).pr_reviewer(), None);
    }

    #[test]
    fn an_unknown_key_names_its_path() {
        let e = error("defaults:\n  layout:\n    prs:\n      updatedAt:\n        widht: 3\n");
        assert!(e.contains("defaults.layout.prs.updatedAt"), "{e}");
        assert!(e.contains("widht"), "{e}");
    }

    #[test]
    fn keys_are_case_sensitive() {
        let e = error("defaults:\n  preview:\n    Open: false\n");
        assert!(e.contains("defaults.preview"), "{e}");
    }

    #[test]
    fn an_invalid_color_names_its_path() {
        let e = error("theme:\n  colors:\n    text:\n      primary: red\n");
        assert!(e.contains("theme.colors.text.primary"), "{e}");
    }

    #[test]
    fn colors_and_icons_use_camel_case_keys() {
        let cfg = layers(&[
            "theme:\n  colors:\n    icon:\n      newContributor: '#ff0000'\n  icons:\n    unknownRole: x\n",
        ]);
        let colors = cfg.theme.colors.unwrap();
        assert_eq!(
            colors.icon.new_contributor,
            Some(ConfigColor(Color::Rgb(255, 0, 0)))
        );
        assert_eq!(cfg.theme.icons.unwrap().unknown_role.as_deref(), Some("x"));
    }

    #[test]
    fn an_unknown_view_is_an_error() {
        assert!(error("defaults:\n  view: pulls\n").contains("defaults.view"));
    }

    #[test]
    fn a_keybinding_needs_exactly_one_action() {
        let both = error("keybindings:\n  prs:\n  - key: a\n    builtin: diff\n    command: x\n");
        assert!(both.contains("both"), "{both}");
        let neither = error("keybindings:\n  prs:\n  - key: a\n    name: nothing\n");
        assert!(
            neither.contains("needs a builtin or a command"),
            "{neither}"
        );
    }

    #[test]
    fn date_format_takes_strftime_and_rejects_invalid_formats() {
        let cfg = layers(&["defaults:\n  dateFormat: '%Y-%m-%d'\n"]);
        let t = DateTime::parse_from_rfc3339("2025-02-02T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(cfg.defaults.date_format.format(t).len(), "2025-02-02".len());
        let e = error("defaults:\n  dateFormat: '%Q'\n");
        assert!(e.contains("defaults.dateFormat"), "{e}");
    }

    #[test]
    fn an_invalid_filter_template_names_its_section() {
        let e = error("prSections:\n- title: A\n  filters: 'updated:>={{ nowModify \"-2w\" }}'\n");
        assert!(e.contains("prSections[0].filters"), "{e}");
    }

    #[test]
    fn the_default_config_file_loads() {
        let yaml = default_config_yaml();
        let cfg = layers(&[&yaml]);
        assert_eq!(cfg.pr_sections.len(), 3);
    }
}
