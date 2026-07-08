use anyhow::{Context, Result};
use evdev::Key;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use crate::i18n::Lang;
use crate::keys;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub lang: Lang,
    #[serde(default)]
    pub groups: Vec<GroupCfg>,
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            lang: Lang::default(),
            groups: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupCfg {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apps: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub gestures: Vec<GestureCfg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GestureCfg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub pattern: String,
    pub keys: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct Gesture {
    pub pattern: String,
    pub keys: Vec<Key>,
    pub apps: Vec<String>,
    pub label: Option<String>,
    pub enabled: bool,
}

impl Gesture {
    pub fn from_cfg(cfg: &GestureCfg, group_apps: &[String], group_enabled: bool) -> Result<Self> {
        let keys = cfg
            .keys
            .iter()
            .map(|k| keys::parse(k))
            .collect::<Result<Vec<_>>>()?;
        Ok(Gesture {
            pattern: cfg.pattern.clone(),
            keys,
            apps: group_apps.to_vec(),
            label: cfg.label.clone(),
            enabled: cfg.enabled && group_enabled,
        })
    }
}

pub fn path() -> PathBuf {
    if let Ok(p) = std::env::var("ARCGLYPH_CONFIG") {
        return PathBuf::from(p);
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        });
    base.join("arcglyph").join("arcglyph.yaml")
}

pub fn load() -> Result<(bool, Vec<Gesture>)> {
    let p = path();
    if !p.exists() {
        eprintln!("no config at {}, using defaults", p.display());
        let cfg = default_config();
        let gs = flatten_groups(&cfg.groups)?;
        return Ok((cfg.enabled, gs));
    }
    let text = fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    let cfg = parse_config(&text)?;
    let gs = flatten_groups(&cfg.groups)?;
    Ok((cfg.enabled, gs))
}

pub fn load_raw() -> Result<Config> {
    let p = path();
    if !p.exists() {
        return Ok(default_config());
    }
    let text = fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    parse_config(&text)
}

fn flatten_groups(groups: &[GroupCfg]) -> Result<Vec<Gesture>> {
    let mut out = Vec::new();
    for grp in groups {
        for g in &grp.gestures {
            out.push(Gesture::from_cfg(g, &grp.apps, grp.enabled)?);
        }
    }
    Ok(out)
}

fn parse_config(text: &str) -> Result<Config> {
    if let Ok(cfg) = serde_yaml::from_str::<Config>(text) {
        if !cfg.groups.is_empty() {
            return Ok(cfg);
        }
    }
    // Legacy format: try as { enabled, gestures: [...] } with per-gesture apps
    #[derive(Deserialize)]
    struct LegacyConfig {
        #[serde(default = "default_true")]
        enabled: bool,
        #[serde(default)]
        gestures: Vec<LegacyGestureCfg>,
    }
    #[derive(Deserialize)]
    struct LegacyGestureCfg {
        label: Option<String>,
        pattern: String,
        keys: Vec<String>,
        #[serde(default)]
        apps: Vec<String>,
        #[serde(default = "default_true")]
        enabled: bool,
    }

    let legacy: LegacyConfig = match serde_yaml::from_str(text) {
        Ok(c) => c,
        Err(_) => {
            // Bare list of gestures
            let list: Vec<LegacyGestureCfg> =
                serde_yaml::from_str(text).context("parse config")?;
            LegacyConfig { enabled: true, gestures: list }
        }
    };

    // Group legacy gestures by their apps list
    let mut groups: Vec<GroupCfg> = Vec::new();
    for lg in legacy.gestures {
        let apps_key: Vec<String> = {
            let mut sorted = lg.apps.clone();
            sorted.sort();
            sorted
        };
        let grp = groups.iter_mut().find(|g| {
            let mut ga = g.apps.clone();
            ga.sort();
            ga == apps_key
        });
        let gesture_cfg = GestureCfg {
            label: lg.label,
            pattern: lg.pattern,
            keys: lg.keys,
            enabled: lg.enabled,
        };
        if let Some(grp) = grp {
            grp.gestures.push(gesture_cfg);
        } else {
            let name = if apps_key.is_empty() {
                "全局".to_string()
            } else {
                apps_key.join(", ")
            };
            groups.push(GroupCfg {
                name,
                apps: lg.apps,
                enabled: true,
                gestures: vec![gesture_cfg],
            });
        }
    }

    Ok(Config {
        enabled: legacy.enabled,
        lang: Lang::default(),
        groups,
    })
}

pub fn save(cfg: &Config) -> Result<()> {
    let p = path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).with_context(|| format!("mkdir {}", dir.display()))?;
    }
    fs::write(&p, format_config(cfg)).with_context(|| format!("write {}", p.display()))?;
    Ok(())
}

fn format_config(cfg: &Config) -> String {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push_str(&format!("enabled: {}\n", cfg.enabled));
    out.push_str(&format!(
        "lang: {}\n",
        match cfg.lang {
            Lang::En => "en",
            Lang::Zh => "zh",
        }
    ));
    out.push_str("groups:\n");
    for grp in &cfg.groups {
        out.push_str(&format!("  - name: {}\n", yaml_scalar(&grp.name)));
        if !grp.enabled {
            out.push_str("    enabled: false\n");
        }
        if !grp.apps.is_empty() {
            out.push_str(&format!(
                "    apps: [{}]\n",
                grp.apps.iter().map(|a| yaml_scalar(a)).collect::<Vec<_>>().join(", ")
            ));
        }
        out.push_str("    gestures:\n");
        for g in &grp.gestures {
            out.push_str("      ");
            out.push_str(&format_gesture_line(g));
            out.push('\n');
        }
    }
    out
}

const HEADER: &str = "\
# arcglyph gesture bindings
#
# enabled: global switch. false disables all gestures.
# lang: GUI language. \"en\" (default) or \"zh\".
# groups: each group associates a set of apps with a list of gestures.
#   name: display name for the group
#   apps: list of app_id substrings (case-insensitive). empty = every window.
#   gestures[*].pattern: sequence of numpad-style direction digits
#     7 8 9      up-left  up   up-right
#     4 . 6      left     .    right
#     1 2 3      down-left down down-right
#   gestures[*].keys: evdev key names pressed together as a chord.
#   gestures[*].label: free-form description shown in the GUI.
#
";

fn format_gesture_line(g: &GestureCfg) -> String {
    let mut fields = Vec::<String>::new();
    if let Some(l) = &g.label {
        fields.push(format!("label: {}", yaml_scalar(l)));
    }
    if !g.enabled {
        fields.push("enabled: false".to_string());
    }
    fields.push(format!("pattern: {}", yaml_scalar(&g.pattern)));
    fields.push(format!(
        "keys: [{}]",
        g.keys
            .iter()
            .map(|k| yaml_scalar(k))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    format!("- {{ {} }}", fields.join(", "))
}

fn yaml_scalar(s: &str) -> String {
    let plain_safe = !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | '+' | ':')
                || (c as u32) > 0x7f
        })
        && !s.parse::<f64>().is_ok()
        && !matches!(s, "true" | "false" | "null" | "yes" | "no" | "on" | "off");
    if plain_safe {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

pub fn default_config() -> Config {
    let chrome_apps = vec![
        "google-chrome".to_string(),
        "chromium".to_string(),
        "microsoft-edge".to_string(),
    ];
    Config {
        enabled: true,
        lang: Lang::default(),
        groups: vec![GroupCfg {
            name: "Browser".to_string(),
            apps: chrome_apps,
            enabled: true,
            gestures: vec![
                GestureCfg { label: Some("Back".into()), pattern: "4".into(), keys: vec!["LEFTALT".into(), "LEFT".into()], enabled: true },
                GestureCfg { label: Some("Forward".into()), pattern: "6".into(), keys: vec!["LEFTALT".into(), "RIGHT".into()], enabled: true },
                GestureCfg { label: Some("Scroll to top".into()), pattern: "8".into(), keys: vec!["HOME".into()], enabled: true },
                GestureCfg { label: Some("Scroll to bottom".into()), pattern: "2".into(), keys: vec!["END".into()], enabled: true },
                GestureCfg { label: Some("Close tab".into()), pattern: "26".into(), keys: vec!["LEFTCTRL".into(), "W".into()], enabled: true },
                GestureCfg { label: Some("Reopen closed tab".into()), pattern: "24".into(), keys: vec!["LEFTCTRL".into(), "LEFTSHIFT".into(), "T".into()], enabled: true },
                GestureCfg { label: Some("Reload".into()), pattern: "46".into(), keys: vec!["F5".into()], enabled: true },
            ],
        }],
    }
}
