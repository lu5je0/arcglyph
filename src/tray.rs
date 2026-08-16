use crate::config::{self, Config};
use crate::i18n::{t, Lang, S};
use ksni::{
    menu::{CheckmarkItem, StandardItem},
    Icon, MenuItem, Tray, TrayMethods,
};
use std::sync::mpsc;
use std::time::Duration;

pub enum TrayCmd {
    ShowPreferences,
    ToggleEnabled,
    Quit,
}

pub struct ArcglyphTray {
    pub tx: mpsc::Sender<TrayCmd>,
    pub enabled: bool,
    pub lang: Lang,
}

impl Tray for ArcglyphTray {
    fn id(&self) -> String {
        "arcglyph".into()
    }

    fn title(&self) -> String {
        "Arcglyph".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Arcglyph".into(),
            description: "Mouse gesture daemon".into(),
            icon_name: "input-mouse".into(),
            icon_pixmap: vec![],
        }
    }

    fn icon_name(&self) -> String {
        "input-mouse".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(TrayCmd::ShowPreferences);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: t(self.lang, S::TrayPreferences).into(),
                icon_name: "preferences-system".into(),
                activate: Box::new(|t: &mut ArcglyphTray| {
                    let _ = t.tx.send(TrayCmd::ShowPreferences);
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: t(
                    self.lang,
                    if self.enabled {
                        S::TrayStatusOn
                    } else {
                        S::TrayStatusOff
                    },
                )
                .into(),
                checked: self.enabled,
                activate: Box::new(|t: &mut ArcglyphTray| {
                    let _ = t.tx.send(TrayCmd::ToggleEnabled);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: t(self.lang, S::TrayQuit).into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut ArcglyphTray| {
                    let _ = t.tx.send(TrayCmd::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Tray lifecycle loop: spawns or shuts down the icon according to
/// `hide_tray`, and keeps the menu state in sync with the config file.
pub async fn run(tx: mpsc::Sender<TrayCmd>) {
    let mut state = config::load_raw().unwrap_or_default();
    let mut handle = if state.hide_tray {
        None
    } else {
        spawn(&state, tx.clone()).await
    };

    // The GUI (or a manual edit) writes the config file; poll it so the
    // tray icon visibility and menu state follow along.
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    loop {
        interval.tick().await;
        let cfg = match config::load_raw() {
            Ok(c) => c,
            Err(_) => continue,
        };
        if cfg.enabled == state.enabled
            && cfg.hide_tray == state.hide_tray
            && cfg.lang == state.lang
        {
            continue;
        }
        state = cfg;
        if state.hide_tray {
            if let Some(h) = handle.take() {
                h.shutdown().await;
            }
        } else if let Some(h) = &handle {
            let (enabled, lang) = (state.enabled, state.lang);
            let _ = h
                .update(|tray| {
                    tray.enabled = enabled;
                    tray.lang = lang;
                })
                .await;
        } else {
            handle = spawn(&state, tx.clone()).await;
        }
    }
}

async fn spawn(cfg: &Config, tx: mpsc::Sender<TrayCmd>) -> Option<ksni::Handle<ArcglyphTray>> {
    let tray = ArcglyphTray {
        tx,
        enabled: cfg.enabled,
        lang: cfg.lang,
    };
    match tray.spawn().await {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("tray spawn error: {:#}", e);
            None
        }
    }
}
