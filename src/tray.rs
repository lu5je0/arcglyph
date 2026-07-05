use ksni::{menu::StandardItem, Icon, MenuItem, Tray};
use std::sync::mpsc;

pub enum TrayCmd {
    ShowPreferences,
    Quit,
}

pub struct ArcglyphTray {
    pub tx: mpsc::Sender<TrayCmd>,
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
                label: "Preferences…".into(),
                icon_name: "preferences-system".into(),
                activate: Box::new(|t: &mut ArcglyphTray| {
                    let _ = t.tx.send(TrayCmd::ShowPreferences);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
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
