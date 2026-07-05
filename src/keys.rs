use anyhow::{anyhow, Result};
use evdev::Key;
use std::str::FromStr;

pub fn parse(name: &str) -> Result<Key> {
    let upper = name.to_ascii_uppercase();
    let full = if upper.starts_with("KEY_") || upper.starts_with("BTN_") {
        upper
    } else {
        format!("KEY_{}", upper)
    };
    Key::from_str(&full).map_err(|_| anyhow!("unknown key: {}", name))
}

pub fn name(key: Key) -> String {
    let dbg = format!("{:?}", key);
    dbg.strip_prefix("KEY_")
        .or_else(|| dbg.strip_prefix("BTN_"))
        .map(|s| s.to_string())
        .unwrap_or(dbg)
}
