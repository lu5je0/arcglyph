use crate::config::{self, Config, GestureCfg, GroupCfg};
use crate::desktop::{self, Desktop};
use crate::gesture::GestureState;
use crate::i18n::{t, Lang, S};
use iced::{
    alignment, mouse, stream,
    widget::{
        button, canvas, column, container, mouse_area, row, rule, scrollable, space, text,
        text_input, toggler, tooltip, Column, Stack,
    },
    window, Background, Border, Color, Element, Event, Font, Length, Point, Rectangle, Renderer,
    Size, Subscription, Task, Theme,
};
use std::sync::{Mutex, OnceLock};

static TRAY_CHANNEL: OnceLock<Mutex<Option<tokio::sync::mpsc::Sender<ExternalMsg>>>> =
    OnceLock::new();

#[derive(Debug, Clone)]
pub enum ExternalMsg {
    Show,
    Quit,
}

pub fn send(msg: ExternalMsg) {
    let _ = try_send(msg);
}

pub fn try_send(msg: ExternalMsg) -> bool {
    if let Some(cell) = TRAY_CHANNEL.get() {
        if let Ok(guard) = cell.lock() {
            if let Some(tx) = guard.as_ref() {
                return tx.try_send(msg).is_ok();
            }
        }
    }
    false
}

pub fn run() -> iced::Result {
    let mut app = iced::daemon(App::new, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .style(|_, _| iced::theme::Style {
            background_color: bg(),
            text_color: ink(),
        })
        .subscription(App::subscription);
    let font_paths = std::env::var_os("ARCGLYPH_FONT")
        .map(std::path::PathBuf::from)
        .into_iter()
        .chain(
            FONT_CANDIDATES
                .iter()
                .map(|path| std::path::PathBuf::from(*path)),
        );
    for path in font_paths {
        if let Ok(bytes) = std::fs::read(path) {
            app = app.font(bytes);
            break;
        }
    }
    app.default_font(UI_FONT)
        .run()
}

const UI_FONT: Font = Font::with_name("Noto Sans CJK SC");
const EMPHASIS: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..UI_FONT
};

const FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK.ttc",
    "/usr/share/fonts/OTF/NotoSansCJK-Regular.ttc",
];

#[derive(Debug, Clone)]
pub enum Msg {
    // sidebar - group management
    ShowGroups,
    ShowSettings,
    SelectGroup(usize),
    ManageGroup,
    AddGroup,
    OpenGroupMenu(usize),
    DismissGroupMenu,
    CopyGroup(usize),
    RemoveGroup(usize),
    // group editing
    GroupNameChanged(String),
    GroupAppInput(String),
    GroupAttachApp,
    GroupDetachApp(String),
    ToggleGroup(bool),
    ToggleGlobal(bool),
    ToggleAutostart(bool),
    ToggleHideTray(bool),
    SetLang(Lang),
    // gesture list
    SelectGesture(usize),
    AddGesture,
    RemoveGesture(usize),
    KeysChanged(usize, String),
    LabelChanged(usize, String),
    PatternAppend(usize, char),
    PatternBackspace(usize),
    PatternClear(usize),
    DrawPattern(usize),
    EditDirections,
    CancelDrawing,
    PatternDrawn(usize, String),
    ToggleGesture(usize, bool),
    // keyboard shortcut recording
    StartRecordKeys(usize),
    StopRecordKeys,
    KeyRecordedEvent(iced::keyboard::Key, iced::keyboard::Modifiers),
    // top-right
    Save,
    Quit,
    Toast(String),
    ToastClear,
    // permission banner
    FixPerms,
    FixPermsResult(Result<(), String>),
    // window picker
    PickApp,
    PickAppResult(Result<Option<String>, String>),
    // external
    Tray(ExternalMsg),
    WindowOpened(window::Id),
    WindowClosed(window::Id),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Groups,
    Settings,
}

struct App {
    cfg: Config,
    toast: Option<String>,
    dirty: bool,
    window: Option<window::Id>,
    page: Page,
    selected_group: usize,
    group_menu: Option<usize>,
    selected_gesture: usize,
    managing_group: bool,
    editing_directions: bool,
    group_app_input: String,
    picking: Option<iced::task::Handle>,
    recording_keys: Option<usize>,
    drawing_pattern: Option<usize>,
    perms_ok: bool,
    fixing: bool,
    fix_done: bool,
    autostart: bool,
}

impl App {
    fn new() -> (Self, Task<Msg>) {
        let cfg = config::load_raw().unwrap_or_default();
        (
            Self {
                cfg,
                toast: None,
                dirty: false,
                window: None,
                page: Page::Groups,
                selected_group: 0,
                group_menu: None,
                selected_gesture: 0,
                managing_group: false,
                editing_directions: false,
                group_app_input: String::new(),
                picking: None,
                recording_keys: None,
                drawing_pattern: None,
                perms_ok: crate::perms::probe().ok(),
                fixing: false,
                fix_done: false,
                autostart: crate::autostart::is_enabled(),
            },
            Task::none(),
        )
    }

    fn lang(&self) -> Lang {
        self.cfg.lang
    }

    fn title(&self, _: window::Id) -> String {
        "ArcGlyph".to_string()
    }

    fn theme(&self, _: window::Id) -> Theme {
        Theme::Light
    }

    fn subscription(&self) -> Subscription<Msg> {
        let mut subs = vec![
            Subscription::run(tray_stream),
            window::close_events().map(Msg::WindowClosed),
        ];
        if self.recording_keys.is_some() {
            subs.push(iced::event::listen_with(|event, status, _| match (event, status) {
                (
                    Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }),
                    iced::event::Status::Ignored,
                ) => Some(Msg::KeyRecordedEvent(key, modifiers)),
                _ => None,
            }));
        }
        if self.group_menu.is_some() {
            subs.push(iced::event::listen_with(|event, status, _| match event {
                Event::Mouse(mouse::Event::ButtonPressed(_))
                    if status == iced::event::Status::Ignored =>
                {
                    Some(Msg::DismissGroupMenu)
                }
                Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(Msg::DismissGroupMenu),
                _ => None,
            }));
        }
        Subscription::batch(subs)
    }

    fn current_group(&self) -> Option<&GroupCfg> {
        self.cfg.groups.get(self.selected_group)
    }

    fn select_gesture(&mut self, index: usize) {
        self.selected_gesture = index;
        self.drawing_pattern = None;
        self.recording_keys = None;
        self.editing_directions = false;
    }

    fn select_group(&mut self, index: usize) {
        self.page = Page::Groups;
        self.selected_group = index;
        self.managing_group = false;
        self.picking = None;
        self.group_app_input.clear();
        self.select_gesture(0);
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        if !matches!(msg, Msg::OpenGroupMenu(_) | Msg::ToastClear) {
            self.group_menu = None;
        }
        match msg {
            Msg::Tray(ExternalMsg::Show) => {
                // Pick up changes made while the window was closed (e.g. the
                // enabled toggle in the tray menu), unless there are unsaved
                // edits that a reload would clobber.
                if self.window.is_none() && !self.dirty {
                    if let Ok(cfg) = config::load_raw() {
                        self.cfg = cfg;
                        if self.selected_group >= self.cfg.groups.len() {
                            self.selected_group = self.cfg.groups.len().saturating_sub(1);
                        }
                        let count = self.current_group().map_or(0, |group| group.gestures.len());
                        self.select_gesture(self.selected_gesture.min(count.saturating_sub(1)));
                    }
                }
                return self.ensure_window();
            }
            Msg::Quit | Msg::Tray(ExternalMsg::Quit) => std::process::exit(0),
            Msg::WindowOpened(id) => {
                self.window = Some(id);
            }
            Msg::WindowClosed(id) => {
                if self.window == Some(id) {
                    self.window = None;
                    self.picking = None;
                    self.select_gesture(self.selected_gesture);
                }
            }
            Msg::ShowGroups => self.page = Page::Groups,
            Msg::ShowSettings => {
                self.page = Page::Settings;
                self.picking = None;
                self.select_gesture(self.selected_gesture);
            }
            Msg::SelectGroup(i) => self.select_group(i),
            Msg::ManageGroup => {
                self.managing_group = !self.managing_group;
                self.picking = None;
                self.select_gesture(self.selected_gesture);
            }
            Msg::AddGroup => {
                let name = t(self.lang(), S::NewGroup).to_string();
                self.cfg.groups.push(GroupCfg {
                    name,
                    apps: Vec::new(),
                    enabled: true,
                    gestures: Vec::new(),
                });
                self.select_group(self.cfg.groups.len() - 1);
                self.managing_group = true;
                self.dirty = true;
            }
            Msg::OpenGroupMenu(index) => {
                self.group_menu = (self.group_menu != Some(index)).then_some(index);
            }
            Msg::DismissGroupMenu => {}
            Msg::CopyGroup(source) => {
                if let Some(mut group) = self.cfg.groups.get(source).cloned() {
                    group.name.push_str(t(self.lang(), S::GroupCopySuffix));
                    let index = source + 1;
                    self.cfg.groups.insert(index, group);
                    self.select_group(index);
                    self.managing_group = true;
                    self.dirty = true;
                }
            }
            Msg::RemoveGroup(i) => {
                if i < self.cfg.groups.len() {
                    self.cfg.groups.remove(i);
                    let selected = self.selected_group - usize::from(i < self.selected_group);
                    self.select_group(selected.min(self.cfg.groups.len().saturating_sub(1)));
                    self.dirty = true;
                }
            }
            Msg::GroupNameChanged(v) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    grp.name = v;
                    self.dirty = true;
                }
            }
            Msg::GroupAppInput(v) => {
                self.group_app_input = v;
            }
            Msg::GroupAttachApp => {
                let val = self.group_app_input.trim().to_string();
                if !val.is_empty() {
                    if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                        if !grp.apps.iter().any(|a| a == &val) {
                            grp.apps.push(val);
                            self.dirty = true;
                        }
                    }
                }
                self.group_app_input.clear();
            }
            Msg::GroupDetachApp(name) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    let before = grp.apps.len();
                    grp.apps.retain(|a| a != &name);
                    if grp.apps.len() != before {
                        self.dirty = true;
                    }
                }
            }
            Msg::ToggleGroup(v) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    if grp.enabled != v {
                        grp.enabled = v;
                        self.dirty = true;
                    }
                }
            }
            Msg::ToggleGlobal(v) => {
                if self.cfg.enabled != v {
                    self.cfg.enabled = v;
                    self.dirty = true;
                }
            }
            Msg::ToggleAutostart(v) => match crate::autostart::set(v) {
                Ok(()) => {
                    self.autostart = v;
                    let label = if v {
                        t(self.lang(), S::AutostartOn)
                    } else {
                        t(self.lang(), S::AutostartOff)
                    };
                    return Task::done(Msg::Toast(label.into()));
                }
                Err(e) => {
                    return Task::done(Msg::Toast(format!(
                        "{}{}",
                        t(self.lang(), S::SaveFailedPrefix),
                        e
                    )))
                }
            },
            Msg::ToggleHideTray(v) => {
                if self.cfg.hide_tray != v {
                    self.cfg.hide_tray = v;
                    self.dirty = true;
                }
            }
            Msg::SetLang(l) => {
                if self.cfg.lang != l {
                    self.cfg.lang = l;
                    self.dirty = true;
                }
            }
            Msg::SelectGesture(i) => self.select_gesture(i),
            Msg::AddGesture => {
                let label = t(self.lang(), S::NewGesture).to_string();
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    grp.gestures.push(GestureCfg {
                        pattern: "6".into(),
                        keys: vec!["LEFTALT".into(), "RIGHT".into()],
                        label: Some(label),
                        enabled: true,
                    });
                    let index = grp.gestures.len() - 1;
                    self.select_gesture(index);
                    self.dirty = true;
                }
            }
            Msg::RemoveGesture(i) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    if i < grp.gestures.len() {
                        grp.gestures.remove(i);
                        let selected = self.selected_gesture - usize::from(i < self.selected_gesture);
                        let selected = selected.min(grp.gestures.len().saturating_sub(1));
                        self.select_gesture(selected);
                        self.dirty = true;
                    }
                }
            }
            Msg::KeysChanged(i, v) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    if let Some(g) = grp.gestures.get_mut(i) {
                        g.keys = split_list(&v);
                        self.dirty = true;
                    }
                }
            }
            Msg::LabelChanged(i, v) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    if let Some(g) = grp.gestures.get_mut(i) {
                        g.label = if v.is_empty() { None } else { Some(v) };
                        self.dirty = true;
                    }
                }
            }
            Msg::PatternAppend(i, c) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    if let Some(g) = grp.gestures.get_mut(i) {
                        if g.pattern.chars().last() != Some(c) {
                            g.pattern.push(c);
                            self.dirty = true;
                        }
                    }
                }
            }
            Msg::PatternBackspace(i) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    if let Some(g) = grp.gestures.get_mut(i) {
                        g.pattern.pop();
                        self.dirty = true;
                    }
                }
            }
            Msg::PatternClear(i) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    if let Some(g) = grp.gestures.get_mut(i) {
                        g.pattern.clear();
                        self.dirty = true;
                    }
                }
            }
            Msg::EditDirections => self.editing_directions = !self.editing_directions,
            Msg::DrawPattern(i) => {
                self.recording_keys = None;
                self.drawing_pattern = Some(i);
            }
            Msg::CancelDrawing => {
                self.drawing_pattern = None;
            }
            Msg::PatternDrawn(i, pattern) => {
                if self.drawing_pattern == Some(i) {
                    self.drawing_pattern = None;
                    if let Some(gesture) = self
                        .cfg
                        .groups
                        .get_mut(self.selected_group)
                        .and_then(|group| group.gestures.get_mut(i))
                    {
                        gesture.pattern = pattern;
                        self.dirty = true;
                    }
                }
            }
            Msg::ToggleGesture(i, v) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    if let Some(g) = grp.gestures.get_mut(i) {
                        if g.enabled != v {
                            g.enabled = v;
                            self.dirty = true;
                        }
                    }
                }
            }
            Msg::StartRecordKeys(i) => {
                self.drawing_pattern = None;
                self.recording_keys = Some(i);
            }
            Msg::StopRecordKeys => {
                self.recording_keys = None;
            }
            Msg::KeyRecordedEvent(key, mods) => {
                if let Some(i) = self.recording_keys {
                    // Escape cancels recording without changing anything.
                    if matches!(
                        key,
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                    ) {
                        self.recording_keys = None;
                        return Task::none();
                    }
                    // Ignore lone modifier presses; wait for a real key.
                    if is_modifier_key(&key) {
                        return Task::none();
                    }
                    if let Some(main) = evdev_name_from_iced(&key) {
                        let mut names = Vec::new();
                        if mods.control() {
                            names.push("LEFTCTRL".to_string());
                        }
                        if mods.shift() {
                            names.push("LEFTSHIFT".to_string());
                        }
                        if mods.alt() {
                            names.push("LEFTALT".to_string());
                        }
                        if mods.logo() {
                            names.push("LEFTMETA".to_string());
                        }
                        names.push(main);
                        if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                            if let Some(g) = grp.gestures.get_mut(i) {
                                g.keys = names;
                                self.dirty = true;
                            }
                        }
                        self.recording_keys = None;
                    }
                }
            }
            Msg::Save => match config::save(&self.cfg) {
                Ok(()) => {
                    self.dirty = false;
                    return Task::done(Msg::Toast(t(self.lang(), S::Saved).into()));
                }
                Err(e) => {
                    return Task::done(Msg::Toast(format!(
                        "{}{}",
                        t(self.lang(), S::SaveFailedPrefix),
                        e
                    )))
                }
            },
            Msg::PickApp => {
                if self.picking.is_some() {
                    return Task::none();
                }
                let gnome = desktop::detect() == Desktop::Gnome;
                let pick = Task::perform(
                    async move {
                        if !gnome {
                            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                        }
                        let app = tokio::task::spawn_blocking(move || {
                            if gnome {
                                crate::gnome::Helper::connect()?.pick_app()
                            } else {
                                crate::focus::query()
                                    .map(|(app, _, _)| (!app.is_empty()).then_some(app))
                            }
                        })
                        .await
                        .map_err(|error| error.to_string())?
                        .map_err(|error| format!("{error:#}"))?;
                        Ok(app)
                    },
                    Msg::PickAppResult,
                );
                // Wayland cannot restore a minimized window through winit.
                let start = match self.window {
                    Some(id) if !gnome => window::minimize(id, true).chain(pick),
                    _ => pick,
                };
                let (task, handle) = start.abortable();
                self.picking = Some(handle.abort_on_drop());
                return task;
            }
            Msg::PickAppResult(result) => {
                if self.picking.take().is_none() {
                    return Task::none();
                }
                let lang = self.lang();
                let message = match result {
                    Ok(Some(app)) => {
                        if let Some(group) = self.cfg.groups.get_mut(self.selected_group) {
                            if !group.apps.contains(&app) {
                                group.apps.push(app.clone());
                                self.dirty = true;
                            }
                        }
                        format!("{}{}", t(lang, S::AddedPrefix), app)
                    }
                    Ok(None) => t(lang, S::NoAppDetected).into(),
                    Err(error) => {
                        eprintln!("arcglyph window picker: {error}");
                        t(lang, S::PickWindowFailed).into()
                    }
                };
                let restore = match self.window {
                    Some(id) if desktop::detect() != Desktop::Gnome => window::minimize(id, false),
                    _ => Task::none(),
                };
                return restore
                    .chain(self.ensure_window())
                    .chain(Task::done(Msg::Toast(message)));
            }
            Msg::Toast(msg) => {
                self.toast = Some(msg);
                return Task::perform(
                    tokio::time::sleep(std::time::Duration::from_millis(1600)),
                    |_| Msg::ToastClear,
                );
            }
            Msg::ToastClear => self.toast = None,
            Msg::FixPerms => {
                self.fixing = true;
                return Task::perform(run_perm_fix(), |r| {
                    Msg::FixPermsResult(r.map_err(|e| format!("{e:#}")))
                });
            }
            Msg::FixPermsResult(result) => {
                self.fixing = false;
                return match result {
                    Ok(()) => {
                        // Group changes only apply to new login sessions, so
                        // keep the banner up until the user logs back in.
                        self.fix_done = true;
                        Task::done(Msg::Toast(t(self.lang(), S::PermFixDone).into()))
                    }
                    Err(e) => Task::done(Msg::Toast(format!(
                        "{}{}",
                        t(self.lang(), S::PermFixFailedPrefix),
                        e
                    ))),
                };
            }
        }
        Task::none()
    }

    fn ensure_window(&mut self) -> Task<Msg> {
        if let Some(id) = self.window {
            return Task::batch([
                window::gain_focus(id),
                window::request_user_attention(id, Some(window::UserAttention::Informational)),
            ]);
        }
        let (id, task) = window::open(window::Settings {
            size: Size::new(1120.0, 720.0),
            min_size: Some(Size::new(820.0, 520.0)),
            ..Default::default()
        });
        self.window = Some(id);
        task.map(Msg::WindowOpened)
    }

    fn view(&self, _id: window::Id) -> Element<'_, Msg> {
        let lang = self.lang();
        let section = match self.page {
            Page::Groups => S::GestureGroups,
            Page::Settings => S::AppSettings,
        };
        let toolbar = row![
            text(t(lang, section)).size(12).font(EMPHASIS).color(muted()),
            space::horizontal(),
            row![
                text("●").size(9).color(if self.dirty { warm() } else { accent() }),
                text(t(lang, if self.dirty { S::Unsaved } else { S::Synced }))
                    .size(12)
                    .color(muted()),
            ]
            .spacing(5)
            .align_y(alignment::Vertical::Center),
            action_button(t(lang, S::Save), Msg::Save, true),
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center);
        let page = match self.page {
            Page::Groups => self.group_page(),
            Page::Settings => self.settings_page(),
        };
        let main = container(column![toolbar, page].spacing(16))
            .padding([16, 20])
            .width(Length::Fill)
            .height(Length::Fill);
        let mut body = column![].height(Length::Fill).spacing(6);
        if !self.perms_ok {
            body = body.push(perm_banner(lang, self.fixing, self.fix_done));
        }
        body = body.push(row![self.sidebar_view(), main].height(Length::Fill));
        let content = container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::background(bg()));
        let mut layers = Stack::new().push(content);
        if let Some(message) = &self.toast {
            layers = layers.push(
                container(toast(message))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(alignment::Horizontal::Center)
                    .align_y(alignment::Vertical::Bottom)
                    .padding(16),
            );
        }
        layers.into()
    }

    fn group_page(&self) -> Element<'_, Msg> {
        let lang = self.lang();
        let Some(group) = self.current_group() else {
            return container(text(t(lang, S::SelectOrAddGroup)).color(muted()))
                .padding(24)
                .into();
        };

        let heading = row![
            column![
                text(&group.name).size(32).font(EMPHASIS),
                text(t(lang, S::GroupScope)).size(12).color(muted()),
            ]
            .spacing(4)
            .width(Length::Fill),
            container(scope_toggle(
                t(lang, S::EnableGroup),
                t(lang, S::GroupOnly),
                group.enabled,
                Msg::ToggleGroup,
            ))
            .padding([8, 10])
            .width(188)
            .style(card_style),
        ]
        .spacing(12)
        .align_y(alignment::Vertical::Center);
        let associated: Element<'_, Msg> = if group.apps.is_empty() {
            tag(t(lang, S::AppliesAll), muted())
        } else {
            let mut badges = iced::widget::Row::new().spacing(5);
            for app in &group.apps {
                badges = badges.push(tag(app, muted()));
            }
            badges.into()
        };
        let apps = row![
            text(t(lang, S::AssociatedApps)).size(12).color(muted()),
            container(associated).width(Length::Fill).clip(true),
            action_button(
                t(lang, if self.managing_group { S::Done } else { S::ManageGroup }),
                Msg::ManageGroup,
                false,
            ),
        ]
        .spacing(9)
        .align_y(alignment::Vertical::Center);

        let editor = if self.managing_group {
            preferences_scroll(self.group_editor(group)).into()
        } else {
            self.gesture_workspace(group)
        };
        column![heading, apps, divider(), editor]
            .spacing(11)
            .height(Length::Fill)
            .into()
    }

    fn group_editor<'a>(&'a self, group: &'a GroupCfg) -> Element<'a, Msg> {
        let lang = self.lang();
        let name = row![
            labeled_input(
                t(lang, S::GroupName),
                group.name.clone(),
                Msg::GroupNameChanged,
                t(lang, S::GroupName),
            ),
            action_button(t(lang, S::CopyGroup), Msg::CopyGroup(self.selected_group), false),
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Bottom);
        let mut chips = iced::widget::Row::new().spacing(6);
        for app in &group.apps {
            chips = chips.push(group_app_chip(app.clone()));
        }
        let add = row![
            text_input(t(lang, S::AppIdPlaceholder), &self.group_app_input)
                .on_input(Msg::GroupAppInput)
                .on_submit(Msg::GroupAttachApp)
                .padding(8)
                .size(13)
                .style(input_style),
            action_button(t(lang, S::Add), Msg::GroupAttachApp, false),
            button(text(t(lang, if self.picking.is_some() {
                S::ClickTargetWindow
            } else {
                S::PickWindow
            })).size(12))
            .on_press_maybe(self.picking.is_none().then_some(Msg::PickApp))
            .padding([6, 10])
            .style(secondary_button_style),
        ]
        .spacing(7)
        .align_y(alignment::Vertical::Center);
        container(column![name, text(t(lang, S::AssociatedApps)).size(12), chips.wrap(), add].spacing(10))
            .padding(14)
            .width(Length::Fill)
            .style(card_style)
            .into()
    }

    fn gesture_workspace<'a>(&'a self, group: &'a GroupCfg) -> Element<'a, Msg> {
        let lang = self.lang();
        let mut list = Column::new().spacing(1);
        for (index, gesture) in group.gestures.iter().enumerate() {
            if index > 0 {
                list = list.push(divider());
            }
            list = list.push(gesture_row(lang, index, gesture, self.selected_gesture == index));
        }
        let rows = preferences_scroll(list);
        let toolbar = row![
            text(t(lang, S::Gestures)).size(15).font(EMPHASIS),
            tag(group.gestures.len().to_string(), muted()),
            space::horizontal(),
            action_button(t(lang, S::AddGesture), Msg::AddGesture, false),
        ]
        .spacing(6)
        .align_y(alignment::Vertical::Center);
        let list = column![toolbar, rows]
            .spacing(7)
            .width(Length::Fill)
            .height(Length::Fill);

        if let Some(gesture) = group.gestures.get(self.selected_gesture) {
            let index = self.selected_gesture;
            let inspector = gesture_inspector(
                lang,
                index,
                gesture,
                self.recording_keys == Some(index),
                self.drawing_pattern == Some(index),
                self.editing_directions,
            );
            row![
                list,
                preferences_scroll(inspector).width(290),
            ]
            .spacing(14)
            .height(Length::Fill)
            .into()
        } else {
            list.into()
        }
    }

    fn settings_page(&self) -> Element<'_, Msg> {
        let lang = self.lang();
        let (language, next_language) = match lang {
            Lang::En => ("简体中文", Lang::Zh),
            Lang::Zh => ("English", Lang::En),
        };
        let recognition = column![
            text(t(lang, S::GestureRecognition)).size(14).font(EMPHASIS),
            divider(),
            scope_toggle(
                t(lang, S::GlobalSwitch),
                t(lang, S::GlobalScope),
                self.cfg.enabled,
                Msg::ToggleGlobal,
            ),
        ]
        .spacing(9);
        let startup = column![
            text(t(lang, S::StartupAndTray)).size(14).font(EMPHASIS),
            divider(),
            scope_toggle(
                t(lang, S::LaunchAtLogin),
                t(lang, S::LoginScope),
                self.autostart,
                Msg::ToggleAutostart,
            ),
            divider(),
            scope_toggle(
                t(lang, S::ShowTray),
                t(lang, S::TrayScope),
                !self.cfg.hide_tray,
                |visible| Msg::ToggleHideTray(!visible),
            ),
        ]
        .spacing(9);
        let interface = column![
            text(t(lang, S::InterfaceAndConfig)).size(14).font(EMPHASIS),
            divider(),
            row![
                text(t(lang, S::Language)).size(13),
                space::horizontal(),
                action_button(language, Msg::SetLang(next_language), false),
            ]
            .align_y(alignment::Vertical::Center),
            text(t(lang, S::ConfigFile)).size(13),
            text(config::path().display().to_string())
                .font(Font::MONOSPACE)
                .size(12)
                .color(muted()),
        ]
        .spacing(8);
        preferences_scroll(
            column![
                text(t(lang, S::AppSettings)).size(32).font(EMPHASIS),
                settings_panel(recognition),
                settings_panel(startup),
                settings_panel(interface),
                column![
                    divider(),
                    row![
                        text(t(lang, S::ExitAppDescription)).size(12).color(muted()),
                        space::horizontal(),
                        action_button(t(lang, S::ExitApp), Msg::Quit, false),
                    ]
                    .spacing(10)
                    .align_y(alignment::Vertical::Center),
                ]
                .spacing(9),
            ]
            .spacing(15)
            .padding([0, 4])
            .width(Length::Fill),
        )
        .into()
    }

    fn sidebar_view(&self) -> Element<'_, Msg> {
        let lang = self.lang();
        let mut groups = Column::new().spacing(4);
        for (index, group) in self.cfg.groups.iter().enumerate() {
            groups = groups.push(sidebar_group_row(
                lang,
                index,
                group,
                self.page == Page::Groups && index == self.selected_group,
                self.group_menu == Some(index),
            ));
        }

        let add = tooltip(
            action_button("＋", Msg::AddGroup, false),
            container(text(t(lang, S::AddGroup)).size(12))
                .padding(6)
                .style(card_style),
            tooltip::Position::Right,
        );
        let group_title = row![
            text(t(lang, S::Groups)).size(11).font(EMPHASIS).color(muted()),
            space::horizontal(),
            add,
        ]
        .align_y(alignment::Vertical::Center);
        let global_status = button(
            row![
                text("●").size(11).color(if self.cfg.enabled { accent() } else { muted() }),
                column![
                    text(t(lang, S::GlobalGestures)).size(13).font(EMPHASIS),
                    text(t(lang, if self.cfg.enabled { S::Running } else { S::Paused }))
                        .size(11)
                        .color(muted()),
                ],
                space::horizontal(),
                text("›").size(16).color(muted()),
            ]
            .spacing(7)
            .align_y(alignment::Vertical::Center),
        )
        .on_press(Msg::ShowSettings)
        .width(Length::Fill)
        .padding(8)
        .style(secondary_button_style);
        let sidebar = column![
            row![
                container(symbol(Symbol::Arc, Color::WHITE))
                    .padding(5)
                    .style(|_| container::background(accent()).border(iced::border::rounded(7))),
                text("ArcGlyph").size(17).font(EMPHASIS),
            ]
            .spacing(7)
            .align_y(alignment::Vertical::Center),
            navigation_button(Symbol::Groups, t(lang, S::GestureGroups), Msg::ShowGroups, self.page == Page::Groups),
            navigation_button(Symbol::Settings, t(lang, S::AppSettings), Msg::ShowSettings, self.page == Page::Settings),
            divider(),
            group_title,
            preferences_scroll(groups),
            global_status,
        ]
        .spacing(9);
        container(sidebar)
            .padding(12)
            .width(166)
            .height(Length::Fill)
            .style(|_| container::background(sidebar_color()).border(Border {
                width: 1.0,
                color: line(),
                ..Border::default()
            }))
            .into()
    }
}

fn preferences_scroll<'a>(content: impl Into<Element<'a, Msg>>) -> scrollable::Scrollable<'a, Msg> {
    scrollable(content)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(5).scroller_width(5).spacing(4),
        ))
        .height(Length::Fill)
}

fn sidebar_group_row<'a>(
    lang: Lang,
    i: usize,
    grp: &'a GroupCfg,
    selected: bool,
    menu_open: bool,
) -> Element<'a, Msg> {
    let subtitle = if grp.apps.is_empty() {
        t(lang, S::Global).to_string()
    } else {
        format!("{} {}", grp.apps.len(), t(lang, S::LinkedCount))
    };
    let label_col = column![
        text(&grp.name).size(13).wrapping(iced::widget::text::Wrapping::None),
        text(subtitle).size(11).color(muted()),
    ]
    .spacing(1);

    let row_inner = row![
        button(container(label_col).clip(true))
            .on_press(Msg::SelectGroup(i))
            .padding([8, 6])
            .width(Length::Fill)
            .style(move |_theme, status| sidebar_button_style(status, selected)),
        button(text("×").size(14).color(muted()))
            .on_press(Msg::RemoveGroup(i))
            .padding([4, 8])
            .style(|_theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered => Color::from_rgba(0.9, 0.3, 0.3, 0.1),
                    _ => Color::TRANSPARENT,
                })),
                text_color: Color::from_rgb(0.6, 0.3, 0.3),
                border: Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
    ]
    .spacing(2)
    .align_y(alignment::Vertical::Center);
    let group_row = mouse_area(row_inner).on_right_press(Msg::OpenGroupMenu(i));
    let mut layers = Stack::new().push(group_row);
    if menu_open {
        layers = layers.push(
            container(
                button(text(t(lang, S::CopyGroup)).size(12))
                    .on_press(Msg::CopyGroup(i))
                    .padding([6, 10])
                    .style(secondary_button_style),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Right)
            .align_y(alignment::Vertical::Center),
        );
    }
    layers.into()
}

fn group_app_chip<'a>(name: String) -> Element<'a, Msg> {
    let label = name.clone();
    let remove_name = name;
    container(
        row![
            text(label).size(12),
            button(text("×").size(12).color(muted()))
                .on_press(Msg::GroupDetachApp(remove_name))
                .padding([0, 6])
                .style(|_theme, status| button::Style {
                    background: Some(Background::Color(match status {
                        button::Status::Hovered => Color::from_rgba(0.9, 0.3, 0.3, 0.15),
                        _ => Color::TRANSPARENT,
                    })),
                    text_color: Color::from_rgb(0.5, 0.2, 0.2),
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        ]
        .spacing(2)
        .align_y(alignment::Vertical::Center),
    )
    .padding([4, 8])
    .style(|_theme| container::Style {
        background: Some(Background::Color(hover_color())),
        border: Border {
            radius: 999.0.into(),
            width: 1.0,
            color: line(),
        },
        text_color: Some(accent()),
        ..Default::default()
    })
    .into()
}

fn scope_toggle<'a>(
    title: &'a str,
    scope: &'a str,
    enabled: bool,
    on_toggle: impl Fn(bool) -> Msg + 'a,
) -> Element<'a, Msg> {
    row![
        column![text(title).size(13).font(EMPHASIS), text(scope).size(11).color(muted())].spacing(2),
        space::horizontal(),
        toggler(enabled).on_toggle(on_toggle).size(17).style(switch_style),
    ]
    .spacing(9)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn navigation_button(kind: Symbol, label: &str, action: Msg, active: bool) -> Element<'_, Msg> {
    button(
        row![
            symbol(kind, if active { accent() } else { muted() }),
            text(label).size(13).font(EMPHASIS),
        ]
        .spacing(7)
        .align_y(alignment::Vertical::Center),
    )
    .on_press(action)
    .padding([8, 10])
    .width(Length::Fill)
    .style(move |_, status| sidebar_button_style(status, active))
    .into()
}

fn tag<'a>(label: impl iced::widget::text::IntoFragment<'a>, color: Color) -> Element<'a, Msg> {
    container(
        text(label).size(11).color(color).wrapping(iced::widget::text::Wrapping::None),
    )
    .padding([2, 6])
    .style(|_| container::background(soft()).border(iced::border::rounded(6)))
    .into()
}

fn settings_panel<'a>(content: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    container(content)
        .padding([9, 12])
        .width(Length::Fill)
        .style(card_style)
        .into()
}

fn divider<'a>() -> rule::Rule<'a, Theme> {
    rule::horizontal(1).style(|_| rule::Style {
        color: line(),
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    })
}

fn gesture_row(lang: Lang, index: usize, gesture: &GestureCfg, selected: bool) -> Element<'_, Msg> {
    let arrows: String = gesture.pattern.chars().map(pattern_arrow).collect();
    let shortcut = gesture
        .keys
        .iter()
        .map(|key| match key.as_str() {
            "LEFTCTRL" | "RIGHTCTRL" => "Ctrl",
            "LEFTSHIFT" | "RIGHTSHIFT" => "Shift",
            "LEFTALT" | "RIGHTALT" => "Alt",
            "LEFTMETA" | "RIGHTMETA" => "Super",
            "LEFT" => "←",
            "RIGHT" => "→",
            "UP" => "↑",
            "DOWN" => "↓",
            "HOME" => "Home",
            "END" => "End",
            key => key,
        })
        .collect::<Vec<_>>()
        .join(" ");
    let state = if gesture.enabled { S::Enabled } else { S::Paused };
    button(
        row![
            container(
                text(arrows)
                    .size(16)
                    .font(EMPHASIS)
                    .color(accent())
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .width(42)
            .height(25)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(|_| container::background(soft()).border(iced::border::rounded(6)))
            .clip(true),
            container(
                text(gesture.label.as_deref().unwrap_or(t(lang, S::Unnamed)))
                    .size(13)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .width(Length::Fill)
            .clip(true),
            container(
                text(shortcut)
                    .size(11)
                    .font(Font::MONOSPACE)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .max_width(102)
            .padding([2, 6])
            .style(preview_style)
            .clip(true),
            row![
                text("●").size(8).color(if gesture.enabled { accent() } else { muted() }),
                text(t(lang, state)).size(11).color(if gesture.enabled { accent() } else { muted() }),
            ]
            .spacing(4)
            .align_y(alignment::Vertical::Center),
        ]
        .spacing(7)
        .align_y(alignment::Vertical::Center),
    )
    .on_press(Msg::SelectGesture(index))
    .width(Length::Fill)
    .padding([6, 8])
    .style(move |_, status| {
        let mut style = sidebar_button_style(status, selected);
        if !selected && status == button::Status::Active {
            style.background = Some(Background::Color(surface()));
        }
        style
    })
    .into()
}

fn gesture_inspector(
    lang: Lang,
    index: usize,
    gesture: &GestureCfg,
    recording: bool,
    drawing: bool,
    editing_directions: bool,
) -> Element<'_, Msg> {
    let title = row![
        text(t(lang, S::GestureDetails)).size(12).font(EMPHASIS).color(muted()),
        space::horizontal(),
        tag(&gesture.pattern, muted()),
    ]
    .align_y(alignment::Vertical::Center);
    let keys = row![
        text_input("LEFTCTRL + W", &gesture.keys.join(" + "))
            .on_input(move |value| Msg::KeysChanged(index, value))
            .padding(8)
            .size(13)
            .style(input_style),
        action_button(
            t(lang, if recording { S::PressKeys } else { S::Record }),
            if recording { Msg::StopRecordKeys } else { Msg::StartRecordKeys(index) },
            false,
        ),
    ]
    .spacing(5)
    .align_y(alignment::Vertical::Center);
    let pattern: Element<'_, Msg> = if drawing {
        column![
            text(t(lang, S::DrawPatternHint)).size(11).color(muted()),
            container(canvas::Canvas::new(GestureDrawing(index)).width(Length::Fill).height(150))
                .style(preview_style)
                .clip(true),
            action_button(t(lang, S::Cancel), Msg::CancelDrawing, false),
        ]
        .spacing(5)
        .into()
    } else {
        let preview = container(
            column![
                canvas::Canvas::new(GesturePreview(&gesture.pattern)).width(Length::Fill).height(82),
                container(text(direction_words(lang, &gesture.pattern)).size(11).color(muted()))
                    .align_x(alignment::Horizontal::Center)
                    .width(Length::Fill),
            ]
            .spacing(3),
        )
        .padding([5, 8])
        .width(Length::Fill)
        .style(preview_style);
        let controls = row![
            action_button(t(lang, S::DrawPattern), Msg::DrawPattern(index), false),
            action_button(
                t(lang, if editing_directions { S::Done } else { S::DragDirection }),
                Msg::EditDirections,
                false,
            ),
        ]
        .spacing(5);
        let mut editor = column![preview, controls].spacing(6);
        if editing_directions {
            editor = editor.push(
                row![
                    pattern_pad(index),
                    column![
                        action_button(t(lang, S::Delete), Msg::PatternBackspace(index), false),
                        action_button(t(lang, S::Clear), Msg::PatternClear(index), false),
                    ]
                    .spacing(4),
                ]
                .spacing(6)
                .align_y(alignment::Vertical::Center),
            );
        }
        editor.into()
    };

    container(
        column![
            title,
            text(gesture.label.as_deref().unwrap_or(t(lang, S::Unnamed))).size(19).font(EMPHASIS),
            pattern,
            column![text(t(lang, S::Shortcut)).size(11).color(muted()), keys].spacing(4),
            labeled_input(
                t(lang, S::Description),
                gesture.label.clone().unwrap_or_default(),
                move |value| Msg::LabelChanged(index, value),
                t(lang, S::OptionalDescription),
            ),
            divider(),
            scope_toggle(
                t(lang, S::EnableGesture),
                t(lang, S::GestureOnly),
                gesture.enabled,
                move |enabled| Msg::ToggleGesture(index, enabled),
            ),
            action_button(t(lang, S::RemoveGesture), Msg::RemoveGesture(index), false),
        ]
        .spacing(8),
    )
    .padding(12)
    .width(Length::Fill)
    .style(card_style)
    .into()
}

struct GesturePreview<'a>(&'a str);

impl canvas::Program<Msg> for GesturePreview<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut points = vec![Point::ORIGIN];
        for direction in self.0.chars() {
            let (dx, dy) = match direction {
                '1' => (-1.0, 1.0),
                '2' => (0.0, 1.0),
                '3' => (1.0, 1.0),
                '4' => (-1.0, 0.0),
                '6' => (1.0, 0.0),
                '7' => (-1.0, -1.0),
                '8' => (0.0, -1.0),
                '9' => (1.0, -1.0),
                _ => continue,
            };
            let last = points[points.len() - 1];
            points.push(Point::new(last.x + dx, last.y + dy));
        }
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let guides = canvas::Path::new(|path| {
            for x in (9..bounds.width as usize).step_by(12) {
                for y in (9..bounds.height as usize).step_by(12) {
                    path.move_to(Point::new(x as f32, y as f32));
                    path.line_to(Point::new(x as f32 + 0.5, y as f32));
                }
            }
        });
        frame.stroke(
            &guides,
            canvas::Stroke::default()
                .with_width(1.35)
                .with_line_cap(canvas::LineCap::Round)
                .with_color(Color::from_rgb8(182, 205, 198)),
        );
        if points.len() > 1 {
            let mut min = Point::ORIGIN;
            let mut max = Point::ORIGIN;
            for point in &points {
                min.x = min.x.min(point.x);
                min.y = min.y.min(point.y);
                max.x = max.x.max(point.x);
                max.y = max.y.max(point.y);
            }
            let scale = 52.0_f32
                .min((bounds.width - 22.0).max(0.0) / (max.x - min.x).max(1.0))
                .min((bounds.height - 18.0).max(0.0) / (max.y - min.y).max(1.0));
            let center = Point::new((min.x + max.x) / 2.0, (min.y + max.y) / 2.0);
            for point in &mut points {
                point.x = bounds.width / 2.0 + (point.x - center.x) * scale;
                point.y = bounds.height / 2.0 + (point.y - center.y) * scale;
            }
            let trail = canvas::Path::new(|path| {
                path.move_to(points[0]);
                for &point in &points[1..] {
                    path.line_to(point);
                }
            });
            frame.stroke(
                &trail,
                canvas::Stroke::default()
                    .with_width(3.0)
                    .with_line_cap(canvas::LineCap::Round)
                    .with_line_join(canvas::LineJoin::Round)
                    .with_color(accent()),
            );
            frame.fill(&canvas::Path::circle(points[0], 4.0), accent());
            frame.fill(&canvas::Path::circle(points[0], 2.0), bg());
            frame.fill(&canvas::Path::circle(points[points.len() - 1], 4.0), accent());
        }
        vec![frame.into_geometry()]
    }
}

struct GestureDrawing(usize);

#[derive(Default)]
struct DrawingState {
    gesture: GestureState,
    points: Vec<Point>,
}

impl canvas::Program<Msg> for GestureDrawing {
    type State = DrawingState;

    fn update(
        &self,
        state: &mut DrawingState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Msg>> {
        let action = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let point = cursor.position_in(bounds)?;
                state.points.clear();
                state.points.push(point);
                state.gesture.start(None);
                canvas::Action::request_redraw()
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.gesture.active => {
                let point = cursor.position_from(bounds.position())?;
                let point = Point::new(
                    point.x.clamp(0.0, bounds.width),
                    point.y.clamp(0.0, bounds.height),
                );
                let prev = *state.points.last()?;
                let dx = point.x.round() as i32 - prev.x.round() as i32;
                let dy = point.y.round() as i32 - prev.y.round() as i32;
                if dx == 0 && dy == 0 {
                    return Some(canvas::Action::capture());
                }
                state.gesture.feed(dx, dy);
                state.points.push(point);
                canvas::Action::request_redraw()
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.gesture.active => {
                state.points.clear();
                match state.gesture.finish() {
                    Some(pattern) => {
                        canvas::Action::publish(Msg::PatternDrawn(self.0, pattern.recorded()))
                    }
                    None => canvas::Action::request_redraw(),
                }
            }
            Event::Window(window::Event::Unfocused) if state.gesture.active => {
                state.gesture.active = false;
                state.points.clear();
                return Some(canvas::Action::request_redraw());
            }
            _ => return None,
        };
        Some(action.and_capture())
    }

    fn draw(
        &self,
        state: &DrawingState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let Some(&start) = state.points.first() else {
            return Vec::new();
        };
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill(&canvas::Path::circle(start, 2.0), accent());
        let path = canvas::Path::new(|path| {
            path.move_to(start);
            for &point in &state.points[1..] {
                path.line_to(point);
            }
        });
        frame.stroke(
            &path,
            canvas::Stroke::default().with_width(3.0).with_color(accent()),
        );
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &DrawingState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

fn pattern_pad<'a>(i: usize) -> Element<'a, Msg> {
    let cell = |c: char| -> Element<'a, Msg> {
        button(text(pattern_arrow(c).to_string()).size(18))
            .on_press(Msg::PatternAppend(i, c))
            .padding([8, 12])
            .width(Length::Fixed(44.0))
            .style(secondary_button_style)
            .into()
    };
    let blank = || -> Element<'a, Msg> { container(text(" ")).width(Length::Fixed(44.0)).into() };
    let r1 = row![cell('7'), cell('8'), cell('9')].spacing(4);
    let r2 = row![cell('4'), blank(), cell('6')].spacing(4);
    let r3 = row![cell('1'), cell('2'), cell('3')].spacing(4);
    column![r1, r2, r3].spacing(4).into()
}

fn labeled_input<'a, F>(
    label: &'static str,
    value: String,
    on_change: F,
    placeholder: &'static str,
) -> Element<'a, Msg>
where
    F: 'a + Fn(String) -> Msg,
{
    column![
        text(label).size(11).color(muted()),
        text_input(placeholder, &value)
            .on_input(on_change)
            .padding(8)
            .size(13)
            .style(input_style),
    ]
    .spacing(4)
    .width(Length::Fill)
    .into()
}

fn action_button<'a>(label: &'a str, msg: Msg, primary: bool) -> Element<'a, Msg> {
    let icon = match &msg {
        Msg::Save => Some(Symbol::Check),
        Msg::AddGroup | Msg::AddGesture => Some(Symbol::Add),
        Msg::DrawPattern(_) => Some(Symbol::Draw),
        Msg::Quit => Some(Symbol::Quit),
        _ => None,
    };
    let mut content = iced::widget::Row::new()
        .spacing(5)
        .align_y(alignment::Vertical::Center);
    if let Some(icon) = icon {
        content = content.push(symbol(icon, if primary { Color::WHITE } else { accent() }));
    }
    if !matches!(msg, Msg::AddGroup) {
        content = content.push(text(label).size(12).font(EMPHASIS));
    }
    button(content)
        .on_press(msg)
        .padding([6, 9])
        .style(move |theme, status| {
            if primary {
                let background = match status {
                    button::Status::Pressed | button::Status::Hovered => accent_hover(),
                    _ => accent(),
                };
                button::Style {
                    background: Some(Background::Color(background)),
                    text_color: Color::WHITE,
                    border: iced::border::rounded(8),
                    ..button::Style::default()
                }
            } else {
                secondary_button_style(theme, status)
            }
        })
        .into()
}

#[derive(Clone, Copy)]
enum Symbol {
    Arc,
    Groups,
    Settings,
    Add,
    Check,
    Draw,
    Quit,
}

fn symbol(kind: Symbol, color: Color) -> canvas::Canvas<Glyph, Msg> {
    canvas::Canvas::new(Glyph { kind, color }).width(16).height(16)
}

struct Glyph {
    kind: Symbol,
    color: Color,
}

impl canvas::Program<Msg> for Glyph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let lines: &[&[(f32, f32)]] = match self.kind {
            Symbol::Arc => &[&[(3.0, 11.5), (7.0, 4.5), (12.5, 9.0)], &[(9.4, 9.0), (12.5, 9.0), (12.5, 5.9)]],
            Symbol::Groups => &[
                &[(2.3, 5.6), (8.0, 2.5), (13.7, 5.6), (8.0, 8.7), (2.3, 5.6)],
                &[(2.3, 9.0), (8.0, 12.1), (13.7, 9.0)],
            ],
            Symbol::Settings => &[
                &[(2.5, 4.0), (13.5, 4.0)],
                &[(2.5, 8.0), (13.5, 8.0)],
                &[(2.5, 12.0), (13.5, 12.0)],
                &[(6.0, 2.5), (6.0, 5.5)],
                &[(10.0, 6.5), (10.0, 9.5)],
                &[(6.0, 10.5), (6.0, 13.5)],
            ],
            Symbol::Add => &[&[(3.5, 8.0), (12.5, 8.0)], &[(8.0, 3.5), (8.0, 12.5)]],
            Symbol::Check => &[&[(3.4, 8.2), (6.5, 11.2), (12.7, 5.0)]],
            Symbol::Draw => &[
                &[(3.2, 12.7), (4.0, 9.5), (10.9, 2.6), (13.4, 5.1), (6.5, 12.0), (3.2, 12.7)],
                &[(9.4, 4.1), (11.9, 6.6)],
            ],
            Symbol::Quit => &[
                &[(7.3, 3.0), (3.3, 3.0), (3.3, 13.0), (7.3, 13.0)],
                &[(6.0, 8.0), (13.2, 8.0)],
                &[(10.7, 5.5), (13.2, 8.0), (10.7, 10.5)],
            ],
        };
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let path = canvas::Path::new(|path| {
            for line in lines {
                path.move_to(Point::new(line[0].0, line[0].1));
                for &(x, y) in &line[1..] {
                    path.line_to(Point::new(x, y));
                }
            }
        });
        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_width(1.45)
                .with_color(self.color)
                .with_line_cap(canvas::LineCap::Round)
                .with_line_join(canvas::LineJoin::Round),
        );
        vec![frame.into_geometry()]
    }
}

fn perm_banner<'a>(lang: Lang, fixing: bool, fix_done: bool) -> Element<'a, Msg> {
    let mut inner = Column::new()
        .spacing(4)
        .push(
            text(t(lang, S::PermTitle))
                .size(14)
                .color(Color::from_rgb(0.55, 0.35, 0.0)),
        )
        .push(
            text(t(lang, S::PermBody))
                .size(12)
                .color(Color::from_rgb(0.4, 0.3, 0.1)),
        );
    if fix_done {
        inner = inner.push(
            text(t(lang, S::PermFixDone))
                .size(12)
                .color(Color::from_rgb(0.2, 0.45, 0.2)),
        );
    } else {
        inner = inner.push(
            text(t(lang, S::PermFixHint))
                .size(12)
                .color(Color::from_rgb(0.3, 0.25, 0.1)),
        );
        let mut fix_btn = button(
            text(if fixing {
                t(lang, S::PermFixRunning)
            } else {
                t(lang, S::PermFixButton)
            })
            .size(12),
        )
        .padding([6, 14])
        .style(secondary_button_style);
        if !fixing {
            fix_btn = fix_btn.on_press(Msg::FixPerms);
        }
        inner = inner.push(fix_btn);
    }

    container(inner)
        .padding([10, 24])
        .width(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Background::Color(Color::from_rgb(1.0, 0.96, 0.85))),
            border: Border {
                width: 1.0,
                color: Color::from_rgba(0.85, 0.6, 0.0, 0.4),
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

async fn run_perm_fix() -> anyhow::Result<()> {
    use anyhow::Context;
    let user = String::from_utf8_lossy(
        &std::process::Command::new("id")
            .arg("-un")
            .output()
            .context("cannot determine current user")?
            .stdout,
    )
    .trim()
    .to_string();
    if user.is_empty() {
        anyhow::bail!("could not determine current username");
    }
    // pkexec asks for authorization through the desktop's polkit agent.
    let out = tokio::process::Command::new("pkexec")
        .args(["usermod", "-aG", "input", &user])
        .output()
        .await
        .context("cannot start pkexec")?;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

fn toast(msg: &str) -> Element<'_, Msg> {
    container(text(msg).size(13).color(Color::WHITE))
        .padding([8, 14])
        .style(|_theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.1, 0.1, 0.12, 0.9))),
            border: Border {
                radius: 10.0.into(),
                ..Default::default()
            },
            text_color: Some(Color::WHITE),
            ..Default::default()
        })
        .max_width(440)
        .into()
}

fn card_style(_theme: &Theme) -> container::Style {
    container::background(surface())
        .border(Border {
            radius: 11.0.into(),
            width: 1.0,
            color: line(),
        })
}

fn preview_style(_theme: &Theme) -> container::Style {
    container::background(soft()).border(Border {
        radius: 7.0.into(),
        width: 1.0,
        color: line(),
    })
}

fn switch_style(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let mut style = toggler::default(theme, status);
    let (enabled, hovered) = match status {
        toggler::Status::Active { is_toggled } | toggler::Status::Disabled { is_toggled } => (is_toggled, false),
        toggler::Status::Hovered { is_toggled } => (is_toggled, true),
    };
    style.background = Background::Color(match (enabled, hovered) {
        (true, true) => accent_hover(),
        (true, false) => accent(),
        (false, true) => Color::from_rgb8(118, 134, 130),
        (false, false) => Color::from_rgb8(145, 161, 157),
    });
    style.foreground = Background::Color(surface());
    style.foreground_border_width = 0.5;
    style.foreground_border_color = Color::from_rgba(0.1, 0.2, 0.19, 0.10);
    style.padding_ratio = 0.12;
    style
}

fn input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused { .. } => accent(),
        text_input::Status::Hovered => Color::from_rgb8(166, 185, 179),
        _ => line(),
    };
    text_input::Style {
        background: Background::Color(soft()),
        border: Border {
            radius: 7.0.into(),
            width: 1.0,
            color: border_color,
        },
        icon: muted(),
        placeholder: muted(),
        value: ink(),
        selection: hover_color(),
    }
}

fn secondary_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (hover_color(), accent()),
        button::Status::Disabled => (soft(), muted()),
        button::Status::Active => (surface(), ink()),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: line(),
        },
        ..Default::default()
    }
}

fn sidebar_button_style(status: button::Status, selected: bool) -> button::Style {
    let (background, text_color) = match (selected, status) {
        (true, _) => (hover_color(), accent()),
        (false, button::Status::Hovered | button::Status::Pressed) => (soft(), ink()),
        _ => (Color::TRANSPARENT, ink()),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: iced::border::rounded(7),
        ..Default::default()
    }
}

fn tray_stream() -> impl iced::futures::Stream<Item = Msg> {
    stream::channel::<Msg>(32, async move |mut output| {
        use iced::futures::SinkExt;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<ExternalMsg>(32);
        let slot = TRAY_CHANNEL.get_or_init(|| Mutex::new(None));
        if let Ok(mut guard) = slot.lock() {
            *guard = Some(tx);
        }
        while let Some(msg) = rx.recv().await {
            if output.send(Msg::Tray(msg)).await.is_err() {
                break;
            }
        }
    })
}

fn is_modifier_key(key: &iced::keyboard::Key) -> bool {
    use iced::keyboard::key::Named;
    matches!(
        key,
        iced::keyboard::Key::Named(Named::Control | Named::Shift | Named::Alt | Named::Super)
    )
}

fn evdev_name_from_iced(key: &iced::keyboard::Key) -> Option<String> {
    use iced::keyboard::key::Named;
    match key {
        iced::keyboard::Key::Character(s) => {
            let c = s.chars().next()?;
            let up = c.to_ascii_uppercase();
            if up.is_ascii_alphanumeric() {
                Some(up.to_string())
            } else {
                // Map common punctuation to evdev names.
                match up {
                    '-' => Some("MINUS".into()),
                    '=' => Some("EQUAL".into()),
                    '[' => Some("LEFTBRACE".into()),
                    ']' => Some("RIGHTBRACE".into()),
                    ';' => Some("SEMICOLON".into()),
                    '\'' => Some("APOSTROPHE".into()),
                    '`' => Some("GRAVE".into()),
                    '\\' => Some("BACKSLASH".into()),
                    ',' => Some("COMMA".into()),
                    '.' => Some("DOT".into()),
                    '/' => Some("SLASH".into()),
                    _ => None,
                }
            }
        }
        iced::keyboard::Key::Named(named) => match named {
            Named::Enter => Some("ENTER".into()),
            Named::Tab => Some("TAB".into()),
            Named::Space => Some("SPACE".into()),
            Named::Backspace => Some("BACKSPACE".into()),
            Named::Delete => Some("DELETE".into()),
            Named::Insert => Some("INSERT".into()),
            Named::Home => Some("HOME".into()),
            Named::End => Some("END".into()),
            Named::PageUp => Some("PAGEUP".into()),
            Named::PageDown => Some("PAGEDOWN".into()),
            Named::ArrowUp => Some("UP".into()),
            Named::ArrowDown => Some("DOWN".into()),
            Named::ArrowLeft => Some("LEFT".into()),
            Named::ArrowRight => Some("RIGHT".into()),
            Named::Escape => Some("ESC".into()),
            Named::F1 => Some("F1".into()),
            Named::F2 => Some("F2".into()),
            Named::F3 => Some("F3".into()),
            Named::F4 => Some("F4".into()),
            Named::F5 => Some("F5".into()),
            Named::F6 => Some("F6".into()),
            Named::F7 => Some("F7".into()),
            Named::F8 => Some("F8".into()),
            Named::F9 => Some("F9".into()),
            Named::F10 => Some("F10".into()),
            Named::F11 => Some("F11".into()),
            Named::F12 => Some("F12".into()),
            _ => None,
        },
        _ => None,
    }
}

fn split_list(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c == '+' || c.is_whitespace())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn pattern_arrow(c: char) -> char {
    match c {
        '8' => '↑',
        '2' => '↓',
        '4' => '←',
        '6' => '→',
        '7' => '↖',
        '9' => '↗',
        '1' => '↙',
        '3' => '↘',
        _ => '?',
    }
}

fn direction_words(lang: Lang, p: &str) -> String {
    p.chars()
        .map(|c| match c {
            '8' => t(lang, S::DirUp),
            '2' => t(lang, S::DirDown),
            '4' => t(lang, S::DirLeft),
            '6' => t(lang, S::DirRight),
            '7' => t(lang, S::DirUpLeft),
            '9' => t(lang, S::DirUpRight),
            '1' => t(lang, S::DirDownLeft),
            '3' => t(lang, S::DirDownRight),
            _ => "?",
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

fn bg() -> Color {
    Color::from_rgb8(247, 249, 248)
}
fn surface() -> Color {
    Color::WHITE
}
fn sidebar_color() -> Color {
    Color::from_rgb8(237, 243, 241)
}
fn soft() -> Color {
    Color::from_rgb8(241, 246, 244)
}
fn hover_color() -> Color {
    Color::from_rgb8(220, 239, 233)
}
fn line() -> Color {
    Color::from_rgb8(218, 229, 225)
}
fn ink() -> Color {
    Color::from_rgb8(26, 43, 42)
}
fn muted() -> Color {
    Color::from_rgb8(80, 101, 98)
}
fn accent() -> Color {
    Color::from_rgb8(22, 106, 90)
}
fn accent_hover() -> Color {
    Color::from_rgb8(15, 85, 72)
}
fn warm() -> Color {
    Color::from_rgb8(169, 105, 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas::Program;

    #[test]
    fn drawing_records_a_diagonal_pattern_on_release() {
        let drawing = GestureDrawing(1);
        let mut state = DrawingState::default();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(240.0, 150.0));
        let left_down = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let left_up = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let start = Point::new(30.0, 110.0);
        drawing.update(&mut state, &left_down, bounds, mouse::Cursor::Available(start));

        for position in [Point::new(90.0, 50.0), Point::new(150.0, 110.0)] {
            let moved = Event::Mouse(mouse::Event::CursorMoved { position });
            drawing.update(&mut state, &moved, bounds, mouse::Cursor::Available(position));
        }

        let (message, _, _) = drawing
            .update(&mut state, &left_up, bounds, mouse::Cursor::Unavailable)
            .unwrap()
            .into_inner();
        assert!(matches!(message, Some(Msg::PatternDrawn(1, pattern)) if pattern == "93"));
        assert!(!state.gesture.active);
    }

    #[test]
    fn adding_a_group_opens_its_editor() {
        let (mut app, _) = App::new();
        app.cfg = Config::default();
        let _ = app.update(Msg::ShowSettings);

        let _ = app.update(Msg::AddGroup);

        assert!(app.page == Page::Groups);
        assert!(app.managing_group);
        assert!(app.dirty);
        assert_eq!(app.current_group().unwrap().name, "New Group");
        assert!(app.current_group().unwrap().gestures.is_empty());
    }

    #[test]
    fn copying_a_group_keeps_its_bindings_and_selects_the_copy() {
        let (mut app, _) = App::new();
        app.cfg = Config::default();
        app.selected_group = 0;
        app.cfg.groups.push(GroupCfg {
            name: "Browser".into(),
            apps: vec!["chrome".into()],
            enabled: false,
            gestures: vec![GestureCfg {
                label: Some("New tab".into()),
                pattern: "93".into(),
                keys: vec!["LEFTCTRL".into(), "T".into()],
                enabled: true,
            }],
        });

        let _ = app.update(Msg::CopyGroup(0));

        assert_eq!(app.selected_group, 1);
        assert!(app.dirty);
        assert_eq!(app.cfg.groups[0].name, "Browser");
        let copy = app.current_group().unwrap();
        assert_eq!(copy.name, "Browser (copy)");
        assert_eq!(copy.apps, ["chrome"]);
        assert!(!copy.enabled);
        assert_eq!(copy.gestures[0].pattern, "93");
        assert_eq!(copy.gestures[0].keys, ["LEFTCTRL", "T"]);
    }
}
