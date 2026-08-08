use anyhow::{Context, Result};
use dbus::blocking::Connection;
use std::{path::PathBuf, time::Duration};

pub const BUS_NAME: &str = "io.github.lu5je0.Arcglyph.Helper";
pub const OBJECT_PATH: &str = "/io/github/lu5je0/Arcglyph/Helper";
pub const IFACE: &str = "io.github.lu5je0.Arcglyph.Helper";
pub const EXTENSION_UUID: &str = "arcglyph@lu5je0.github.io";

/// Client for the Arcglyph Helper GNOME Shell extension.
pub struct Helper {
    conn: Connection,
}

impl Helper {
    pub fn connect() -> Result<Self> {
        Ok(Self {
            conn: Connection::new_session().context("open session bus")?,
        })
    }

    fn proxy(&self) -> dbus::blocking::Proxy<'_, &Connection> {
        self.conn
            .with_proxy(BUS_NAME, OBJECT_PATH, Duration::from_millis(1000))
    }

    pub fn get_focus(&self) -> Result<(String, bool, bool)> {
        let (app_id, fullscreen, cursor_inside): (String, bool, bool) = self
            .proxy()
            .method_call(IFACE, "GetFocus", ())
            .context("GetFocus")?;
        Ok((app_id, fullscreen, cursor_inside))
    }

    pub fn trail_start(&self) -> Result<()> {
        let _: () = self
            .proxy()
            .method_call(IFACE, "TrailStart", ())
            .context("TrailStart")?;
        Ok(())
    }

    pub fn trail_end(&self) -> Result<()> {
        let _: () = self
            .proxy()
            .method_call(IFACE, "TrailEnd", ())
            .context("TrailEnd")?;
        Ok(())
    }
}

/// Copy the bundled GNOME Shell extension into the user's extensions dir.
/// Idempotent; the user still has to enable it once with
/// `gnome-extensions enable arcglyph@lu5je0.github.io` and re-login.
pub fn install_extension() -> Result<PathBuf> {
    let data_home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME").map(|h| PathBuf::from(h).join(".local/share"))
        })
        .context("neither XDG_DATA_HOME nor HOME is set")?;
    let dir = data_home.join("gnome-shell/extensions").join(EXTENSION_UUID);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    std::fs::write(
        dir.join("extension.js"),
        include_str!("../packaging/gnome-extension/arcglyph@lu5je0.github.io/extension.js"),
    )?;
    std::fs::write(
        dir.join("metadata.json"),
        include_str!("../packaging/gnome-extension/arcglyph@lu5je0.github.io/metadata.json"),
    )?;
    Ok(dir)
}
