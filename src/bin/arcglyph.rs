use std::thread;

use arcglyph::{
    daemon,
    gui::{self, ExternalMsg},
    tray::{ArcglyphTray, TrayCmd},
};
use ksni::TrayMethods;

fn main() {
    // Background: mouse gesture daemon.
    thread::spawn(|| {
        if let Err(e) = daemon::run() {
            eprintln!("arcglyph daemon error: {:#}", e);
        }
    });

    // Background: system tray. Owns its own tokio runtime, since ksni::spawn
    // is async and ksni 0.3 talks to the compositor's StatusNotifierWatcher
    // over D-Bus.
    let (tray_tx, tray_rx) = std::sync::mpsc::channel::<TrayCmd>();
    thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("tray runtime error: {:#}", e);
                return;
            }
        };
        rt.block_on(async move {
            let tray = ArcglyphTray { tx: tray_tx };
            if let Err(e) = tray.spawn().await {
                eprintln!("tray spawn error: {:#}", e);
                return;
            }
            std::future::pending::<()>().await;
        });
    });

    // Bridge tray commands into the iced runtime.
    thread::spawn(move || {
        while let Ok(cmd) = tray_rx.recv() {
            match cmd {
                TrayCmd::ShowPreferences => gui::send(ExternalMsg::Show),
                TrayCmd::Quit => gui::send(ExternalMsg::Quit),
            }
        }
    });

    // Kick off an initial "show" so the window appears on first launch;
    // afterwards the user goes through the tray. If the outbound channel
    // isn't ready yet the message is dropped and we retry once the runtime
    // is up.
    thread::spawn(|| {
        for _ in 0..100 {
            if gui::try_send(ExternalMsg::Show) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    if let Err(e) = gui::run() {
        eprintln!("arcglyph gui error: {:#}", e);
        std::process::exit(1);
    }
}
