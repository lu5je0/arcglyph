use anyhow::{Context, Result};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

const NIXOS_EXEC: &str = "/run/current-system/sw/bin/arcglyph";

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

/// Prefer a stable system path, then the package wrapper, then the running binary.
fn exec_path() -> String {
    for packaged in [NIXOS_EXEC, "/usr/bin/arcglyph"] {
        if Path::new(packaged).is_file() {
            return packaged.into();
        }
    }
    if let Some(wrapper) = std::env::var_os("ARCGLYPH_AUTOSTART_EXEC").filter(|p| !p.is_empty()) {
        return PathBuf::from(wrapper).display().to_string();
    }
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "arcglyph".into())
}

fn desktop_entry(exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Arcglyph\n\
         GenericName=Mouse Gestures\n\
         Comment=Right-click drag mouse gesture daemon for Wayland\n\
         Exec={} --hidden\n\
         Icon=input-mouse\n\
         Terminal=false\n\
         Categories=Utility;Settings;\n\
         X-GNOME-Autostart-enabled=true\n\
         X-KDE-autostart-after=panel\n\
         X-KDE-StartupNotify=false\n",
        exec
    )
}

pub fn is_managed() -> bool {
    let Ok(user) = std::env::var("USER") else {
        return false;
    };
    fs::read_to_string("/etc/arcglyph/autostart-users")
        .is_ok_and(|users| users.lines().any(|name| name == user))
}

fn is_generated_nix_entry(contents: &str) -> bool {
    let Some(exec) = contents
        .lines()
        .find_map(|line| line.strip_prefix("Exec=")?.strip_suffix(" --hidden"))
    else {
        return false;
    };
    let known_exec = matches!(exec, NIXOS_EXEC | "/usr/bin/arcglyph")
        || exec.strip_prefix("/nix/store/").is_some_and(|relative| {
            relative.split_once("/bin/").is_some_and(|(package, bin)| {
                package.contains("-arcglyph-") && matches!(bin, "arcglyph" | ".arcglyph-wrapped")
            })
        });
    known_exec && contents == desktop_entry(exec)
}

/// Repair entries written by older versions or remove them when NixOS manages login.
pub fn migrate_nixos_entry() -> Result<()> {
    if !Path::new(NIXOS_EXEC).is_file() {
        return Ok(());
    }
    let path = autostart_path();
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    let contents = fs::read_to_string(&path)?;
    if !is_generated_nix_entry(&contents) {
        return Ok(());
    }
    if is_managed() {
        fs::remove_file(path)?;
    } else {
        let updated = desktop_entry(NIXOS_EXEC);
        if contents != updated {
            let dir = path.parent().context("autostart directory is missing")?;
            let mut replacement = tempfile::NamedTempFile::new_in(dir)?;
            replacement.as_file().set_permissions(metadata.permissions())?;
            replacement.write_all(updated.as_bytes())?;
            replacement.persist(path)?;
        }
    }
    Ok(())
}

pub fn is_enabled() -> bool {
    autostart_path().exists()
}

pub fn set(enabled: bool) -> Result<()> {
    anyhow::ensure!(!is_managed(), "login startup is managed by NixOS");
    let path = autostart_path();
    if enabled {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        fs::write(&path, desktop_entry(&exec_path()))
            .with_context(|| format!("write {}", path.display()))?;
    } else if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_legacy_and_current_nix_launchers() {
        for path in [
            "/usr/bin/arcglyph",
            "/nix/store/hash-arcglyph-0.7.0/bin/.arcglyph-wrapped",
            "/nix/store/hash-arcglyph-0.7.6/bin/arcglyph",
            NIXOS_EXEC,
        ] {
            assert!(is_generated_nix_entry(&desktop_entry(path)));
        }
    }

    #[test]
    fn preserves_custom_entries() {
        let original = desktop_entry(NIXOS_EXEC);
        assert!(!is_generated_nix_entry(&(original + "Hidden=true\n")));
        assert!(!is_generated_nix_entry(&desktop_entry("/home/alice/bin/arcglyph")));
    }
}
