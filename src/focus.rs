use anyhow::{anyhow, Context, Result};
use dbus::{
    blocking::{Connection, SyncConnection},
    channel::MatchingReceiver,
    message::MatchRule,
};
use std::{
    io::Write,
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Query KWin's scripting API for the currently focused window.
/// Adapted from kdotool (github.com/jinliu/kdotool).
///
/// Returns (app_id, fullscreen). app_id is empty when there is no active
/// window at all.
pub fn query() -> Result<(String, bool, bool)> {
    let ctx = Ctx::new()?;
    let self_conn = SyncConnection::new_session().context("open session bus (self)")?;
    let dbus_addr = self_conn.unique_name().to_string();

    let script = generate_script(&dbus_addr);
    let payload = run_script(&script, &ctx, self_conn)?;
    let info: WindowInfo =
        serde_json::from_str(&payload).with_context(|| format!("parse payload {}", payload))?;
    Ok((info.app_id, info.fullscreen, info.cursor_inside))
}

#[derive(serde::Deserialize)]
struct WindowInfo {
    app_id: String,
    fullscreen: bool,
    #[serde(default = "default_true")]
    cursor_inside: bool,
}

fn default_true() -> bool {
    true
}

struct Ctx {
    script_name: String,
}

impl Ctx {
    fn new() -> Result<Self> {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system time")?
            .as_nanos();
        Ok(Self {
            script_name: format!("arcglyph-focus-{suffix}"),
        })
    }
}

fn generate_script(dbus_addr: &str) -> String {
    format!(
        r#"
function report(payload) {{
    callDBus("{addr}", "/", "", "result", payload.toString());
}}
function report_err(msg) {{
    callDBus("{addr}", "/", "", "error", msg.toString());
}}
try {{
    let w = workspace.activeWindow;
    if (w == null) {{
        report(JSON.stringify({{ app_id: "", fullscreen: false, cursor_inside: false }}));
    }} else {{
        let cur = workspace.cursorPos;
        let geo = w.frameGeometry;
        let inside = cur.x >= geo.x && cur.x < geo.x + geo.width &&
                     cur.y >= geo.y && cur.y < geo.y + geo.height;
        report(JSON.stringify({{
            app_id: (w.resourceClass || "").toString(),
            fullscreen: !!w.fullScreen,
            cursor_inside: inside,
        }}));
    }}
}} catch (e) {{
    report_err(e.toString());
}}
"#,
        addr = dbus_addr,
    )
}

fn run_script(script: &str, ctx: &Ctx, self_conn: SyncConnection) -> Result<String> {
    enum Msg {
        Result(String),
        Error(String),
    }
    let kwin = Connection::new_session().context("open session bus (kwin)")?;
    let proxy = kwin.with_proxy("org.kde.KWin", "/Scripting", Duration::from_millis(3000));

    let (tx, rx) = mpsc::channel();
    let _receiver = self_conn.start_receive(
        MatchRule::new_method_call(),
        Box::new(move |m, _| {
            if let Some(member) = m.member() {
                if let Some(arg) = m.get1::<String>() {
                    match member.as_ref() {
                        "result" => {
                            let _ = tx.send(Msg::Result(arg));
                        }
                        "error" => {
                            let _ = tx.send(Msg::Error(arg));
                        }
                        _ => {}
                    }
                }
            }
            true
        }),
    );

    let mut file = tempfile::NamedTempFile::with_prefix("arcglyph-focus-")?;
    file.write_all(script.as_bytes())?;
    let path = file.into_temp_path();

    let (script_id,): (i32,) = proxy
        .method_call(
            "org.kde.kwin.Scripting",
            "loadScript",
            (path.to_str().unwrap(), &ctx.script_name),
        )
        .context("loadScript")?;
    if script_id < 0 {
        return Err(anyhow!("loadScript returned {}", script_id));
    }

    let script_proxy = kwin.with_proxy(
        "org.kde.KWin",
        format!("/Scripting/Script{script_id}"),
        Duration::from_millis(3000),
    );
    let _: () = script_proxy
        .method_call("org.kde.kwin.Script", "run", ())
        .context("script run")?;
    let _: () = script_proxy
        .method_call("org.kde.kwin.Script", "stop", ())
        .context("script stop")?;

    let start = Instant::now();
    let out = loop {
        self_conn.process(Duration::from_millis(50))?;
        match rx.try_recv() {
            Ok(Msg::Result(p)) => break Ok(p),
            Ok(Msg::Error(e)) => break Err(anyhow!("kwin script error: {}", e)),
            Err(mpsc::TryRecvError::Empty) => {
                if start.elapsed() > Duration::from_millis(500) {
                    break Err(anyhow!("timeout waiting for kwin response"));
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                break Err(anyhow!("channel closed"));
            }
        }
    };

    let _: Result<(), _> = proxy.method_call(
        "org.kde.kwin.Scripting",
        "unloadScript",
        (&ctx.script_name,),
    );

    out
}

// API-compatible facade so daemon.rs doesn't have to change much.
// Snapshot lookups now hit an in-memory cache that a background thread
// refreshes every ~300 ms, so the right-click hot path never blocks on
// KWin's DBus round-trip.
#[derive(Clone, Default)]
pub struct FocusTracker {
    inner: std::sync::Arc<std::sync::Mutex<CacheEntry>>,
}

#[derive(Default, Clone)]
struct CacheEntry {
    app_id: Option<String>,
    fullscreen: bool,
    cursor_inside: bool,
}

impl FocusTracker {
    pub fn snapshot(&self) -> (Option<String>, bool, bool) {
        let g = self.inner.lock().unwrap();
        (g.app_id.clone(), g.fullscreen, g.cursor_inside)
    }

    fn refresh(&self) {
        match query() {
            Ok((app, fs, cursor_inside)) => {
                let mut g = self.inner.lock().unwrap();
                g.app_id = if app.is_empty() { None } else { Some(app) };
                g.fullscreen = fs;
                g.cursor_inside = cursor_inside;
            }
            Err(_) => {
                // keep last-known values on transient errors
            }
        }
    }
}

pub struct FocusHandle;

impl FocusHandle {
    pub fn inert() -> Self {
        Self
    }
}

pub fn spawn() -> Result<(FocusTracker, FocusHandle)> {
    let tracker = FocusTracker::default();
    tracker.refresh(); // seed
    let t2 = tracker.clone();
    std::thread::Builder::new()
        .name("arcglyph-focus".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(300));
            t2.refresh();
        })
        .context("spawn focus polling thread")?;
    Ok((tracker, FocusHandle))
}
