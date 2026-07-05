use anyhow::{Context, Result};
use evdev::Key;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use crate::keys;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Config {
    pub gestures: Vec<GestureCfg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GestureCfg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub pattern: String,
    pub keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Gesture {
    pub pattern: String,
    pub keys: Vec<Key>,
    pub apps: Vec<String>,
    pub label: Option<String>,
}

impl Gesture {
    pub fn from_cfg(cfg: &GestureCfg) -> Result<Self> {
        let keys = cfg
            .keys
            .iter()
            .map(|k| keys::parse(k))
            .collect::<Result<Vec<_>>>()?;
        Ok(Gesture {
            pattern: cfg.pattern.clone(),
            keys,
            apps: cfg.apps.clone(),
            label: cfg.label.clone(),
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

pub fn load() -> Result<Vec<Gesture>> {
    let p = path();
    if !p.exists() {
        eprintln!("no config at {}, using defaults", p.display());
        return Ok(defaults());
    }
    let text = fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    let cfg: Config = serde_yaml::from_str(&text).context("parse config")?;
    cfg.gestures
        .iter()
        .map(Gesture::from_cfg)
        .collect::<Result<Vec<_>>>()
}

pub fn load_raw() -> Result<Config> {
    let p = path();
    if !p.exists() {
        return Ok(Config {
            gestures: defaults().into_iter().map(GestureCfg::from_gesture).collect(),
        });
    }
    let text = fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    serde_yaml::from_str(&text).context("parse config")
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
    for g in &cfg.gestures {
        out.push_str(&format_line(g));
        out.push('\n');
    }
    out
}

const HEADER: &str = "\
# arcglyph gesture bindings
#
# pattern: sequence of numpad-style direction digits
#   7 8 9      up-left  up   up-right
#   4 . 6      left     .    right
#   1 2 3      down-left down down-right
# apps:  list of app_id substrings (case-insensitive). empty = every window.
# keys:  evdev key names pressed together as a chord.
# label: free-form description shown in the GUI.
#
";

fn format_line(g: &GestureCfg) -> String {
    let mut fields = Vec::<String>::new();
    if let Some(l) = &g.label {
        fields.push(format!("label: {}", yaml_scalar(l)));
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
    if !g.apps.is_empty() {
        fields.push(format!(
            "apps: [{}]",
            g.apps
                .iter()
                .map(|a| yaml_scalar(a))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
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

impl GestureCfg {
    pub fn from_gesture(g: Gesture) -> Self {
        Self {
            pattern: g.pattern,
            keys: g.keys.iter().copied().map(keys::name).collect(),
            apps: g.apps,
            label: g.label,
        }
    }
}

pub fn defaults() -> Vec<Gesture> {
    let chrome = ["google-chrome", "chromium", "microsoft-edge"];
    vec![
        chrome_gesture("4", &[Key::KEY_LEFTALT, Key::KEY_LEFT], "后退", &chrome),
        chrome_gesture("6", &[Key::KEY_LEFTALT, Key::KEY_RIGHT], "前进", &chrome),
        chrome_gesture("8", &[Key::KEY_HOME], "回到顶部", &chrome),
        chrome_gesture("2", &[Key::KEY_END], "滚动到底部", &chrome),
        chrome_gesture("26", &[Key::KEY_LEFTCTRL, Key::KEY_W], "关闭标签页", &chrome),
        chrome_gesture(
            "24",
            &[Key::KEY_LEFTCTRL, Key::KEY_LEFTSHIFT, Key::KEY_T],
            "恢复关闭的标签页",
            &chrome,
        ),
        chrome_gesture("46", &[Key::KEY_F5], "刷新", &chrome),
    ]
}

fn chrome_gesture(pattern: &str, keys: &[Key], label: &str, apps: &[&str]) -> Gesture {
    Gesture {
        pattern: pattern.into(),
        keys: keys.to_vec(),
        apps: apps.iter().map(|s| s.to_string()).collect(),
        label: Some(label.into()),
    }
}
