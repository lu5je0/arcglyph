use std::thread;

use arcglyph::{
    config,
    daemon,
    gui::{self, ExternalMsg},
    instance,
    tray::{self, TrayCmd},
};

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

    if let Err(err) = arcglyph::autostart::migrate_nixos_entry() {
        eprintln!("arcglyph autostart migration: {err:#}");
    }

    let start_hidden = std::env::args().any(|arg| arg == "--hidden");
    match instance::claim(!start_hidden, show_preferences) {
        Ok(true) => {}
        Ok(false) => return,
        Err(err) => {
            eprintln!("arcglyph instance error: {err:#}");
            std::process::exit(1);
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
    // over D-Bus. The loop also watches the config file for hide_tray and
    // enabled-state changes.
    let (tray_tx, tray_rx) = std::sync::mpsc::channel::<TrayCmd>();
    thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("tray runtime error: {:#}", e);
                return;
            }
        };
        rt.block_on(tray::run(tray_tx));
    });

    // Bridge tray commands into the iced runtime.
    thread::spawn(move || {
        while let Ok(cmd) = tray_rx.recv() {
            match cmd {
                TrayCmd::ShowPreferences => gui::send(ExternalMsg::Show),
                TrayCmd::Quit => gui::send(ExternalMsg::Quit),
                TrayCmd::ToggleEnabled => toggle_enabled(),
            }
        }
    });

    if !start_hidden {
        thread::spawn(show_preferences);
    }

    if let Err(e) = gui::run() {
        eprintln!("arcglyph gui error: {:#}", e);
        std::process::exit(1);
    }
}

fn show_preferences() {
    // The first request can arrive before the GUI subscription is ready.
    for _ in 0..100 {
        if gui::try_send(ExternalMsg::Show) {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Flip the global enabled switch directly in the config file. The daemon
/// picks it up via its inotify watch, and the tray loop refreshes the menu
/// state on its next poll.
fn toggle_enabled() {
    match config::load_raw() {
        Ok(mut cfg) => {
            cfg.enabled = !cfg.enabled;
            if let Err(e) = config::save(&cfg) {
                eprintln!("failed to save enabled toggle: {:#}", e);
            }
        }
        Err(e) => eprintln!("failed to load config: {:#}", e),
    }
}
