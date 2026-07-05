use anyhow::{anyhow, Result};
use evdev::{Device, Key, RelativeAxisType};
use std::fs;

pub fn find_mice() -> Result<Vec<Device>> {
    let mut out = Vec::new();
    for entry in fs::read_dir("/dev/input")? {
        let path = entry?.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.starts_with("event") {
            continue;
        }
        let dev = match Device::open(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let has_right = dev
            .supported_keys()
            .map(|k| k.contains(Key::BTN_RIGHT))
            .unwrap_or(false);
        let has_rel = dev
            .supported_relative_axes()
            .map(|r| r.contains(RelativeAxisType::REL_X) && r.contains(RelativeAxisType::REL_Y))
            .unwrap_or(false);
        if has_right && has_rel {
            eprintln!(
                "grabbing {} ({})",
                path.display(),
                dev.name().unwrap_or("unknown")
            );
            out.push(dev);
        }
    }
    if out.is_empty() {
        return Err(anyhow!("no mouse device with BTN_RIGHT + REL_X/Y found"));
    }
    Ok(out)
}
