use anyhow::{bail, Context, Result};
use dbus::{
    blocking::{stdintf::org_freedesktop_dbus::RequestNameReply, Connection},
    channel::{MatchingReceiver, Sender},
    message::MatchRule,
};
use std::{thread, time::Duration};

const BUS_NAME: &str = "io.github.lu5je0.ArcGlyph";
const OBJECT_PATH: &str = "/io/github/lu5je0/ArcGlyph";
const SHOW: &str = "ShowPreferences";

pub fn claim(show_existing: bool, show_preferences: fn()) -> Result<bool> {
    let bus = Connection::new_session().context("connect to the session bus")?;
    match bus.request_name(BUS_NAME, false, false, true)? {
        RequestNameReply::PrimaryOwner => {
            let rule = MatchRule::new_method_call()
                .with_path(OBJECT_PATH)
                .with_interface(BUS_NAME)
                .with_member(SHOW);
            bus.start_receive(
                rule,
                Box::new(move |message, bus| {
                    let _ = bus.send(message.method_return());
                    show_preferences();
                    true
                }),
            );

            thread::Builder::new().name("arcglyph-instance".into()).spawn(move || {
                loop {
                    if let Err(err) = bus.process(Duration::from_secs(30)) {
                        eprintln!("arcglyph session bus error: {err}");
                        std::process::exit(1);
                    }
                }
            })?;
            Ok(true)
        }
        RequestNameReply::Exists => {
            if show_existing {
                let proxy = bus.with_proxy(BUS_NAME, OBJECT_PATH, Duration::from_secs(5));
                proxy
                    .method_call::<(), _, _, _>(BUS_NAME, SHOW, ())
                    .context("show preferences in the running ArcGlyph instance")?;
            }
            Ok(false)
        }
        reply => bail!("unexpected session bus reply: {reply:?}"),
    }
}
