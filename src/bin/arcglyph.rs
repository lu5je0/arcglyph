use std::thread;

use arcglyph::{
    daemon,
    gui::{self, ExternalMsg},
    tray::{ArcglyphTray, TrayCmd},
};
use ksni::TrayMethods;

fn main() {
    if std::env::args().any(|a| a == "--install-gnome-extension") {
        match arcglyph::gnome::install_extension() {
            Ok(dir) => {
                println!("extension installed to {}", dir.display());
                println!(
                    "enable it with: gnome-extensions enable {}\n(then log out and back in)",
                    arcglyph::gnome::EXTENSION_UUID
                );
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("failed to install gnome extension: {:#}", e);
                std::process::exit(1);
            }
        }
    }

    // On GNOME, keep the bundled helper extension files in sync so upgrades
    // take effect after the next shell restart. Enabling it stays manual.
    if arcglyph::desktop::detect() == arcglyph::desktop::Desktop::Gnome {
        if let Err(e) = arcglyph::gnome::install_extension() {
            eprintln!("gnome extension sync failed: {:#}", e);
        }
    }

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
    // is up. When launched with --hidden (e.g. via autostart at login),
    // skip this so the app stays in the tray without showing the window.
    let start_hidden = std::env::args().any(|a| a == "--hidden");
    if !start_hidden {
        thread::spawn(|| {
            for _ in 0..100 {
                if gui::try_send(ExternalMsg::Show) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
    }

    if let Err(e) = gui::run() {
        eprintln!("arcglyph gui error: {:#}", e);
        std::process::exit(1);
    }
}
