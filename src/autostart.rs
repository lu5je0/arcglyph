use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// Path to the XDG autostart entry for arcglyph.
fn autostart_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        });
    base.join("autostart").join("arcglyph.desktop")
}

/// Command to launch on login. Prefer the packaged binary; fall back to the
/// running executable so a locally-built binary still autostarts correctly.
fn exec_path() -> String {
    let packaged = PathBuf::from("/usr/bin/arcglyph");
    if packaged.exists() {
        return packaged.display().to_string();
    }
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "arcglyph".into())
}

pub fn is_enabled() -> bool {
    autostart_path().exists()
}

pub fn set(enabled: bool) -> Result<()> {
    let path = autostart_path();
    if enabled {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let contents = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Arcglyph\n\
             GenericName=Mouse Gestures\n\
             Comment=Right-click drag mouse gesture daemon for Wayland\n\
             Exec={}\n\
             Icon=input-mouse\n\
             Terminal=false\n\
             Categories=Utility;Settings;\n\
             X-GNOME-Autostart-enabled=true\n\
             X-KDE-autostart-after=panel\n\
             X-KDE-StartupNotify=false\n",
            exec_path()
        );
        fs::write(&path, contents).with_context(|| format!("write {}", path.display()))?;
    } else if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}
