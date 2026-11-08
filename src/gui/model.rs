use crate::config::{Config, GestureCfg, GroupCfg};
use crate::i18n::{t, Lang, S};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Groups,
    Settings,
}

#[derive(Clone)]
pub enum Action {
    Groups,
    Settings,
    SelectGroup(usize),
    ManageGroup,
    AddGroup,
    CopyGroup(usize),
    RemoveGroup(usize),
    AttachApp,
    DetachApp(String),
    ToggleGroup,
    ToggleGlobal,
    ToggleTray,
    ToggleLogin,
    Language,
    SelectGesture(usize),
    AddGesture,
    RemoveGesture,
    AppendDirection(char),
    BackspaceDirection,
    ClearPattern,
    Draw,
    Directions,
    Cancel,
    Record,
    ToggleGesture,
    Save,
    Pick,
    FixPermissions,
    Quit,
}

#[derive(Clone)]
pub struct Model {
    pub cfg: Config,
    pub dirty: bool,
    pub page: Page,
    pub group: usize,
    pub gesture: usize,
    pub managing: bool,
    pub drawing: bool,
    pub recording: bool,
    pub directions: bool,
    pub app_input: String,
}

impl Model {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            dirty: false,
            page: Page::Groups,
            group: 0,
            gesture: 0,
            managing: false,
            drawing: false,
            recording: false,
            directions: false,
            app_input: String::new(),
        }
    }

    pub fn lang(&self) -> Lang {
        self.cfg.lang
    }

    pub fn current_group(&self) -> Option<&GroupCfg> {
        self.cfg.groups.get(self.group)
    }

    pub fn current_gesture(&self) -> Option<&GestureCfg> {
        self.current_group()?.gestures.get(self.gesture)
    }

    pub fn current_gesture_mut(&mut self) -> Option<&mut GestureCfg> {
        self.cfg
            .groups
            .get_mut(self.group)?
            .gestures
            .get_mut(self.gesture)
    }

    pub fn select_gesture(&mut self, index: usize) {
        self.gesture = index;
        self.drawing = false;
        self.recording = false;
        self.directions = false;
    }

    pub fn select_group(&mut self, index: usize) {
        self.page = Page::Groups;
        self.group = index;
        self.managing = false;
        self.app_input.clear();
        self.select_gesture(0);
    }

    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Groups => self.page = Page::Groups,
            Action::Settings => {
                self.page = Page::Settings;
                self.select_gesture(self.gesture);
            }
            Action::SelectGroup(index) => self.select_group(index),
            Action::SelectGesture(index) => self.select_gesture(index),
            Action::ManageGroup => {
                self.managing = !self.managing;
                self.select_gesture(self.gesture);
            }
            Action::AddGroup => {
                self.cfg.groups.push(GroupCfg {
                    name: t(self.lang(), S::NewGroup).into(),
                    apps: vec![],
                    enabled: true,
                    gestures: vec![],
                });
                self.select_group(self.cfg.groups.len() - 1);
                self.managing = true;
                self.dirty = true;
            }
            Action::CopyGroup(index) => {
                if let Some(mut group) = self.cfg.groups.get(index).cloned() {
                    group.name.push_str(t(self.lang(), S::GroupCopySuffix));
                    self.cfg.groups.insert(index + 1, group);
                    self.select_group(index + 1);
                    self.managing = true;
                    self.dirty = true;
                }
            }
            Action::RemoveGroup(index) => {
                if index < self.cfg.groups.len() {
                    self.cfg.groups.remove(index);
                    let selected = self.group - usize::from(index < self.group);
                    self.select_group(selected.min(self.cfg.groups.len().saturating_sub(1)));
                    self.dirty = true;
                }
            }
            Action::AttachApp => {
                let app = self.app_input.trim().to_string();
                if !app.is_empty() {
                    if let Some(group) = self.cfg.groups.get_mut(self.group) {
                        if !group.apps.contains(&app) {
                            group.apps.push(app);
                            self.dirty = true;
                        }
                    }
                    self.app_input.clear();
                }
            }
            Action::DetachApp(app) => {
                if let Some(group) = self.cfg.groups.get_mut(self.group) {
                    group.apps.retain(|id| id != &app);
                    self.dirty = true;
                }
            }
            Action::ToggleGroup => {
                if let Some(group) = self.cfg.groups.get_mut(self.group) {
                    group.enabled = !group.enabled;
                    self.dirty = true;
                }
            }
            Action::ToggleGlobal => {
                self.cfg.enabled = !self.cfg.enabled;
                self.dirty = true;
            }
            Action::ToggleTray => {
                self.cfg.hide_tray = !self.cfg.hide_tray;
                self.dirty = true;
            }
            Action::Language => {
                self.cfg.lang = match self.cfg.lang {
                    Lang::En => Lang::Zh,
                    Lang::Zh => Lang::En,
                };
                self.dirty = true;
            }
            Action::AddGesture => {
                let label = t(self.lang(), S::NewGesture).to_string();
                if let Some(group) = self.cfg.groups.get_mut(self.group) {
                    group.gestures.push(GestureCfg {
                        pattern: "6".into(),
                        keys: vec!["LEFTALT".into(), "RIGHT".into()],
                        label: Some(label),
                        enabled: true,
                    });
                    let index = group.gestures.len() - 1;
                    self.select_gesture(index);
                    self.dirty = true;
                }
            }
            Action::RemoveGesture => {
                if let Some(group) = self.cfg.groups.get_mut(self.group) {
                    if self.gesture < group.gestures.len() {
                        group.gestures.remove(self.gesture);
                        let index = self.gesture.min(group.gestures.len().saturating_sub(1));
                        self.select_gesture(index);
                        self.dirty = true;
                    }
                }
            }
            Action::AppendDirection(direction) => {
                if let Some(gesture) = self.current_gesture_mut() {
                    if !gesture.pattern.ends_with(direction) {
                        gesture.pattern.push(direction);
                        self.dirty = true;
                    }
                }
            }
            Action::BackspaceDirection => {
                if let Some(gesture) = self.current_gesture_mut() {
                    gesture.pattern.pop();
                    self.dirty = true;
                }
            }
            Action::ClearPattern => {
                if let Some(gesture) = self.current_gesture_mut() {
                    gesture.pattern.clear();
                    self.dirty = true;
                }
            }
            Action::ToggleGesture => {
                if let Some(gesture) = self.current_gesture_mut() {
                    gesture.enabled = !gesture.enabled;
                    self.dirty = true;
                }
            }
            Action::Draw => {
                self.recording = false;
                self.drawing = true;
            }
            Action::Directions => self.directions = !self.directions,
            Action::Record => {
                self.drawing = false;
                self.recording = !self.recording;
            }
            Action::Cancel => {
                self.recording = false;
                self.drawing = false;
            }
            _ => {}
        }
    }
}

pub fn split_keys(value: &str) -> Vec<String> {
    value
        .split(|ch: char| ch == ',' || ch == '+' || ch.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_a_group_opens_its_editor() {
        let mut model = Model::new(Config::default());
        model.apply(Action::Settings);
        model.apply(Action::AddGroup);
        assert!(model.page == Page::Groups);
        assert!(model.managing && model.dirty);
        assert_eq!(model.current_group().unwrap().name, "New Group");
    }

    #[test]
    fn copying_a_group_keeps_bindings_and_selects_the_copy() {
        let mut model = Model::new(Config::default());
        model.cfg.groups.push(GroupCfg {
            name: "Browser".into(),
            apps: vec!["chrome".into()],
            enabled: false,
            gestures: vec![GestureCfg {
                pattern: "93".into(),
                keys: vec!["LEFTCTRL".into(), "T".into()],
                label: Some("New tab".into()),
                enabled: true,
            }],
        });
        model.apply(Action::CopyGroup(0));
        assert_eq!(model.group, 1);
        assert!(model.managing && model.dirty);
        let copy = model.current_group().unwrap();
        assert_eq!(copy.name, "Browser (copy)");
        assert_eq!(copy.apps, ["chrome"]);
        assert!(!copy.enabled);
        assert_eq!(copy.gestures[0].pattern, "93");
        assert_eq!(copy.gestures[0].keys, ["LEFTCTRL", "T"]);
    }
}
