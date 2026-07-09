use std::fs;
use std::path::Path;

/// Result of the one-shot permission probe shown in the GUI banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermStatus {
    /// Can read at least one /dev/input/event* mouse-like node.
    pub can_read_input: bool,
    /// Can open /dev/uinput for writing (needed to inject events).
    pub can_write_uinput: bool,
}

impl PermStatus {
    pub fn ok(&self) -> bool {
        self.can_read_input && self.can_write_uinput
    }
}

/// Probe access to the input subsystem without grabbing anything.
///
/// This mirrors what the daemon needs at runtime: read access to
/// /dev/input/event* and write access to /dev/uinput. Both are gated by
/// membership in the `input` group plus the udev rule shipped with the deb.
pub fn probe() -> PermStatus {
    PermStatus {
        can_read_input: can_read_any_event(),
        can_write_uinput: can_write_uinput(),
    }
}

fn can_read_any_event() -> bool {
    let dir = match fs::read_dir("/dev/input") {
        Ok(d) => d,
        Err(_) => return false,
    };
    let mut saw_event = false;
    for entry in dir.flatten() {
        let path = entry.path();
        let is_event = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|n| n.starts_with("event"))
            .unwrap_or(false);
        if !is_event {
            continue;
        }
        saw_event = true;
        if fs::File::open(&path).is_ok() {
            return true;
        }
    }
    // No event nodes at all is not a permission problem; treat as OK so we
    // don't nag on unusual setups. A permission problem is: nodes exist but
    // none can be opened.
    !saw_event
}

fn can_write_uinput() -> bool {
    let path = Path::new("/dev/uinput");
    if !path.exists() {
        // Kernel module may be autoloaded on first use; don't nag here.
        return true;
    }
    fs::OpenOptions::new().write(true).open(path).is_ok()
}
