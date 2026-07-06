use crate::config::{self, Config, GestureCfg, GroupCfg};
use iced::{
    alignment,
    stream,
    widget::{
        button, checkbox, column, container, horizontal_rule, horizontal_space, row, scrollable,
        text, text_input, toggler, Column, Space,
    },
    window, Background, Border, Color, Element, Length, Shadow, Size, Subscription, Task, Theme,
    Vector,
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
    let mut app = iced::daemon("手势偏好设置", App::update, App::view)
        .theme(App::theme)
        .subscription(App::subscription);
    for path in FONT_CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            app = app.font(bytes);
            break;
        }
    }
    app.default_font(iced::Font::with_name("Noto Sans CJK SC"))
        .run_with(App::new)
}

const FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK.ttc",
    "/usr/share/fonts/OTF/NotoSansCJK-Regular.ttc",
];

#[derive(Debug, Clone)]
pub enum Msg {
    // sidebar - group management
    SelectGroup(usize),
    AddGroup,
    RemoveGroup(usize),
    // group editing
    GroupNameChanged(String),
    GroupAppInput(String),
    GroupAttachApp,
    GroupDetachApp(String),
    ToggleGroup(bool),
    ToggleGlobal(bool),
    // gesture list
    AddGesture,
    RemoveGesture(usize),
    KeysChanged(usize, String),
    LabelChanged(usize, String),
    PatternAppend(usize, char),
    PatternBackspace(usize),
    PatternClear(usize),
    ToggleGesture(usize, bool),
    // top-right
    Save,
    Toast(String),
    ToastClear,
    // window picker
    PickApp,
    PickAppResult(Option<String>),
    // external
    Tray(ExternalMsg),
    WindowOpened(window::Id),
    WindowClosed(window::Id),
}

struct App {
    cfg: Config,
    toast: Option<String>,
    dirty: bool,
    window: Option<window::Id>,
    selected_group: usize,
    group_app_input: String,
    picking: bool,
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
                selected_group: 0,
                group_app_input: String::new(),
                picking: false,
            },
            Task::none(),
        )
    }

    fn theme(&self, _: window::Id) -> Theme {
        Theme::Light
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            Subscription::run(tray_stream),
            window::close_events().map(Msg::WindowClosed),
        ])
    }

    fn current_group(&self) -> Option<&GroupCfg> {
        self.cfg.groups.get(self.selected_group)
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Tray(ExternalMsg::Show) => return self.ensure_window(),
            Msg::Tray(ExternalMsg::Quit) => std::process::exit(0),
            Msg::WindowOpened(id) => {
                self.window = Some(id);
            }
            Msg::WindowClosed(id) => {
                if self.window == Some(id) {
                    self.window = None;
                }
            }
            Msg::SelectGroup(i) => {
                self.selected_group = i;
                self.group_app_input.clear();
            }
            Msg::AddGroup => {
                self.cfg.groups.push(GroupCfg {
                    name: "新分组".to_string(),
                    apps: Vec::new(),
                    enabled: true,
                    gestures: Vec::new(),
                });
                self.selected_group = self.cfg.groups.len() - 1;
                self.dirty = true;
            }
            Msg::RemoveGroup(i) => {
                if i < self.cfg.groups.len() {
                    self.cfg.groups.remove(i);
                    if self.selected_group >= self.cfg.groups.len() && self.selected_group > 0 {
                        self.selected_group -= 1;
                    }
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
            Msg::AddGesture => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    grp.gestures.push(GestureCfg {
                        pattern: "6".into(),
                        keys: vec!["LEFTALT".into(), "RIGHT".into()],
                        label: Some("新手势".into()),
                        enabled: true,
                    });
                    self.dirty = true;
                }
            }
            Msg::RemoveGesture(i) => {
                if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                    if i < grp.gestures.len() {
                        grp.gestures.remove(i);
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
            Msg::Save => match config::save(&self.cfg) {
                Ok(()) => {
                    self.dirty = false;
                    return Task::done(Msg::Toast("已保存".into()));
                }
                Err(e) => return Task::done(Msg::Toast(format!("保存失败: {}", e))),
            },
            Msg::PickApp => {
                self.picking = true;
                let minimize = if let Some(id) = self.window {
                    window::minimize(id, true)
                } else {
                    Task::none()
                };
                let pick_task = Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                        crate::focus::query()
                            .ok()
                            .and_then(|(app, _, _)| if app.is_empty() { None } else { Some(app) })
                    },
                    Msg::PickAppResult,
                );
                return minimize.chain(pick_task);
            }
            Msg::PickAppResult(app_id) => {
                self.picking = false;
                if let Some(id) = self.window {
                    let restore = window::minimize(id, false);
                    let focus = window::gain_focus(id);
                    if let Some(app) = app_id {
                        if let Some(grp) = self.cfg.groups.get_mut(self.selected_group) {
                            if !grp.apps.iter().any(|a| a == &app) {
                                grp.apps.push(app.clone());
                                self.dirty = true;
                            }
                        }
                        return restore
                            .chain(focus)
                            .chain(Task::done(Msg::Toast(format!("已添加: {}", app))));
                    }
                    return restore.chain(focus).chain(Task::done(Msg::Toast(
                        "未检测到应用".into(),
                    )));
                }
            }
            Msg::Toast(t) => {
                self.toast = Some(t);
                return Task::perform(
                    tokio::time::sleep(std::time::Duration::from_millis(1600)),
                    |_| Msg::ToastClear,
                );
            }
            Msg::ToastClear => self.toast = None,
        }
        Task::none()
    }

    fn ensure_window(&mut self) -> Task<Msg> {
        if let Some(id) = self.window {
            return window::gain_focus(id);
        }
        let (id, task) = window::open(window::Settings {
            size: Size::new(940.0, 640.0),
            min_size: Some(Size::new(720.0, 480.0)),
            ..Default::default()
        });
        self.window = Some(id);
        task.map(Msg::WindowOpened)
    }

    fn view(&self, _id: window::Id) -> Element<'_, Msg> {
        let header = row![
            text(if let Some(grp) = self.current_group() {
                format!("{} 手势", grp.name)
            } else {
                "无分组".to_string()
            })
            .size(24),
            horizontal_space(),
            toggler(self.cfg.enabled)
                .label(if self.cfg.enabled {
                    "总开关：开"
                } else {
                    "总开关：关"
                })
                .on_toggle(Msg::ToggleGlobal)
                .size(20),
            action_button("保存", Msg::Save, self.dirty),
        ]
        .spacing(12)
        .align_y(alignment::Vertical::Center);

        let subtitle = text(format!(
            "{} · {}",
            config::path().display(),
            if self.dirty { "未保存" } else { "已同步" }
        ))
        .size(11)
        .color(muted());

        let content = if let Some(grp) = self.current_group() {
            let group_header = self.group_header_view(grp);
            let mut list = Column::new().spacing(10);
            for i in 0..grp.gestures.len() {
                list = list.push(gesture_card(i, &grp.gestures[i]));
            }
            list = list.push(add_card());
            let body = scrollable(
                container(column![group_header, list].spacing(16))
                    .padding([4, 4])
                    .width(Length::Fill),
            )
            .height(Length::Fill);
            column![header, subtitle, horizontal_rule(1), body].spacing(12)
        } else {
            column![
                header,
                subtitle,
                horizontal_rule(1),
                container(text("请从左侧选择或添加分组").size(14).color(muted()))
                    .padding(40)
                    .width(Length::Fill)
                    .align_x(alignment::Horizontal::Center),
            ]
            .spacing(12)
        };

        let mut root = row![
            self.sidebar_view(),
            container(content).padding([20, 24]).width(Length::Fill),
        ]
        .spacing(0);

        let mut wrapper = column![].spacing(0);
        wrapper = wrapper.push(root);
        if let Some(t) = &self.toast {
            wrapper = wrapper.push(container(toast(t)).padding([0, 24]));
            root = row![];
            let _ = root;
        }

        container(wrapper)
            .style(|_theme| container::Style {
                background: Some(Background::Color(bg())),
                ..Default::default()
            })
            .into()
    }

    fn group_header_view<'a>(&'a self, grp: &'a GroupCfg) -> Element<'a, Msg> {
        let name_row = row![
            text("分组名称").size(11).color(muted()),
            Space::with_width(8),
            text_input("分组名称", &grp.name)
                .on_input(Msg::GroupNameChanged)
                .padding(8)
                .style(input_style),
            Space::with_width(12),
            checkbox("启用", grp.enabled).on_toggle(Msg::ToggleGroup),
        ]
        .align_y(alignment::Vertical::Center);

        let apps_label = row![
            text("关联应用").size(11).color(muted()),
            horizontal_space(),
            text(if grp.apps.is_empty() {
                "适用于所有应用".to_string()
            } else {
                format!("已关联 {} 个", grp.apps.len())
            })
            .size(11)
            .color(muted()),
        ]
        .align_y(alignment::Vertical::Center);

        let mut chips_col = Column::new().spacing(6);
        let per_row = 4usize;
        let mut buf: Vec<Element<'a, Msg>> = Vec::new();
        for name in &grp.apps {
            buf.push(group_app_chip(name.clone()));
            if buf.len() == per_row {
                let mut r = iced::widget::Row::new().spacing(6);
                for c in buf.drain(..) {
                    r = r.push(c);
                }
                chips_col = chips_col.push(r);
            }
        }
        if !buf.is_empty() {
            let mut r = iced::widget::Row::new().spacing(6);
            for c in buf.drain(..) {
                r = r.push(c);
            }
            chips_col = chips_col.push(r);
        }

        let add_row = row![
            text_input("app_id 子串（例如 chrome）", &self.group_app_input)
                .on_input(Msg::GroupAppInput)
                .on_submit(Msg::GroupAttachApp)
                .padding(6)
                .size(12)
                .style(input_style),
            button(text("添加").size(12))
                .on_press(Msg::GroupAttachApp)
                .padding([6, 12])
                .style(secondary_button_style),
            button(text(if self.picking { "点击目标窗口…" } else { "拾取窗口" }).size(12))
                .on_press(Msg::PickApp)
                .padding([6, 12])
                .style(secondary_button_style),
        ]
        .spacing(6);

        let mut inner = Column::new().spacing(8).push(name_row).push(apps_label);
        if !grp.apps.is_empty() {
            inner = inner.push(chips_col);
        }
        inner = inner.push(add_row);

        container(inner)
            .padding(16)
            .style(card_style)
            .width(Length::Fill)
            .into()
    }

    fn sidebar_view(&self) -> Element<'_, Msg> {
        let mut col = Column::new()
            .spacing(2)
            .push(text("分组").size(11).color(muted()))
            .push(Space::with_height(4));

        for (i, grp) in self.cfg.groups.iter().enumerate() {
            let selected = i == self.selected_group;
            col = col.push(sidebar_group_row(i, grp, selected));
        }

        col = col.push(Space::with_height(10));
        col = col.push(
            button(
                row![text("+").size(14), text("添加分组").size(12)]
                    .spacing(6)
                    .align_y(alignment::Vertical::Center),
            )
            .on_press(Msg::AddGroup)
            .padding([8, 12])
            .width(Length::Fill)
            .style(|_theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered => hover_color(),
                    _ => Color::TRANSPARENT,
                })),
                text_color: accent(),
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        );

        container(col)
            .padding(18)
            .width(Length::Fixed(220.0))
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Background::Color(sidebar_color())),
                border: Border {
                    width: 1.0,
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.05),
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }
}

fn sidebar_group_row<'a>(i: usize, grp: &'a GroupCfg, selected: bool) -> Element<'a, Msg> {
    let subtitle = if grp.apps.is_empty() {
        "全局".to_string()
    } else {
        grp.apps.join(", ")
    };
    let label_col = column![
        text(&grp.name).size(13),
        text(subtitle).size(10).color(muted()),
    ]
    .spacing(1);

    let row_inner = row![
        button(label_col)
            .on_press(Msg::SelectGroup(i))
            .padding([8, 12])
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
    row_inner.into()
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
        background: Some(Background::Color(Color::from_rgba(0.0, 0.48, 1.0, 0.10))),
        border: Border {
            radius: 999.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.0, 0.48, 1.0, 0.25),
        },
        text_color: Some(Color::from_rgb(0.05, 0.35, 0.75)),
        ..Default::default()
    })
    .into()
}

fn gesture_card<'a>(i: usize, g: &'a GestureCfg) -> Element<'a, Msg> {
    let keys_input = labeled_input(
        "快捷键",
        g.keys.join(" + "),
        move |v| Msg::KeysChanged(i, v),
        "LEFTCTRL + W",
    );
    let label_input = labeled_input(
        "说明",
        g.label.clone().unwrap_or_default(),
        move |v| Msg::LabelChanged(i, v),
        "可选描述",
    );

    let pattern_preview = if g.pattern.is_empty() {
        "（未设置）".to_string()
    } else {
        g.pattern.chars().map(pattern_arrow).collect()
    };

    let title_row = row![
        text(pattern_preview).size(26),
        column![
            text(g.label.clone().unwrap_or_else(|| "未命名".into())).size(15),
            text(direction_words(&g.pattern)).size(11).color(muted()),
        ]
        .spacing(2),
        horizontal_space(),
        checkbox("", g.enabled).on_toggle(move |v| Msg::ToggleGesture(i, v)),
        icon_button("×", Msg::RemoveGesture(i)),
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center);

    let pad = pattern_pad(i);
    let pad_row = row![
        pad,
        column![
            text("拖动方向").size(11).color(muted()),
            Space::with_height(4),
            row![
                button(text("← 删除").size(11))
                    .on_press(Msg::PatternBackspace(i))
                    .padding([4, 10])
                    .style(secondary_button_style),
                button(text("清空").size(11))
                    .on_press(Msg::PatternClear(i))
                    .padding([4, 10])
                    .style(secondary_button_style),
            ]
            .spacing(6),
        ]
        .spacing(6),
    ]
    .spacing(16)
    .align_y(alignment::Vertical::Center);

    let grid = column![pad_row, row![keys_input, label_input].spacing(12),].spacing(10);

    container(column![title_row, horizontal_rule(1), grid].spacing(12))
        .padding(16)
        .style(card_style)
        .width(Length::Fill)
        .into()
}

fn pattern_pad<'a>(i: usize) -> Element<'a, Msg> {
    let cell = |c: char| -> Element<'a, Msg> {
        button(text(pattern_arrow(c).to_string()).size(18))
            .on_press(Msg::PatternAppend(i, c))
            .padding([8, 12])
            .width(Length::Fixed(44.0))
            .style(pad_button_style)
            .into()
    };
    let blank = || -> Element<'a, Msg> {
        container(text(" ")).width(Length::Fixed(44.0)).into()
    };
    let r1 = row![cell('7'), cell('8'), cell('9')].spacing(4);
    let r2 = row![cell('4'), blank(), cell('6')].spacing(4);
    let r3 = row![cell('1'), cell('2'), cell('3')].spacing(4);
    column![r1, r2, r3].spacing(4).into()
}

fn add_card<'a>() -> Element<'a, Msg> {
    container(
        button(
            row![text("+").size(18), text("添加手势").size(14)]
                .spacing(8)
                .align_y(alignment::Vertical::Center),
        )
        .on_press(Msg::AddGesture)
        .padding([10, 16])
        .style(|_theme, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => hover_color(),
                _ => card_color(),
            })),
            text_color: accent(),
            border: Border {
                radius: 12.0.into(),
                width: 1.0,
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.06),
            },
            ..Default::default()
        }),
    )
    .width(Length::Fill)
    .align_x(alignment::Horizontal::Center)
    .into()
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
            .style(input_style),
    ]
    .spacing(4)
    .width(Length::Fill)
    .into()
}

fn action_button<'a>(label: &'a str, msg: Msg, primary: bool) -> Element<'a, Msg> {
    button(text(label).size(13))
        .on_press(msg)
        .padding([8, 16])
        .style(move |_theme, status| {
            let (bg_color, fg) = if primary {
                (
                    match status {
                        button::Status::Hovered => Color {
                            a: 1.0,
                            ..accent_hover()
                        },
                        _ => accent(),
                    },
                    Color::WHITE,
                )
            } else {
                (
                    match status {
                        button::Status::Hovered => hover_color(),
                        _ => card_color(),
                    },
                    Color::from_rgb(0.15, 0.15, 0.17),
                )
            };
            button::Style {
                background: Some(Background::Color(bg_color)),
                text_color: fg,
                border: Border {
                    radius: 8.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
                ..Default::default()
            }
        })
        .into()
}

fn icon_button<'a>(label: &'a str, msg: Msg) -> Element<'a, Msg> {
    button(text(label).size(18).color(muted()))
        .on_press(msg)
        .padding(4)
        .style(|_theme, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => Color::from_rgba(0.95, 0.3, 0.3, 0.15),
                _ => Color::TRANSPARENT,
            })),
            text_color: Color::from_rgb(0.6, 0.2, 0.2),
            border: Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
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
        .align_x(alignment::Horizontal::Center)
        .width(Length::Fill)
        .into()
}

fn card_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(card_color())),
        border: Border {
            radius: 14.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.05),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.05),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 4.0,
        },
        ..Default::default()
    }
}

fn input_style(_theme: &Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.7)),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.08),
        },
        icon: muted(),
        placeholder: muted(),
        value: Color::from_rgb(0.1, 0.1, 0.12),
        selection: Color::from_rgba(0.0, 0.48, 1.0, 0.25),
    }
}

fn pad_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered => Color::from_rgba(0.0, 0.48, 1.0, 0.12),
            _ => Color::from_rgba(0.0, 0.0, 0.0, 0.04),
        })),
        text_color: Color::from_rgb(0.12, 0.12, 0.14),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.08),
        },
        ..Default::default()
    }
}

fn secondary_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered => hover_color(),
            _ => card_color(),
        })),
        text_color: Color::from_rgb(0.2, 0.2, 0.22),
        border: Border {
            radius: 6.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.08),
        },
        ..Default::default()
    }
}

fn sidebar_button_style(status: button::Status, selected: bool) -> button::Style {
    let bg_color = if selected {
        Color::from_rgba(0.0, 0.48, 1.0, 0.12)
    } else {
        match status {
            button::Status::Hovered => Color::from_rgba(0.0, 0.0, 0.0, 0.04),
            _ => Color::TRANSPARENT,
        }
    };
    button::Style {
        background: Some(Background::Color(bg_color)),
        text_color: if selected {
            accent()
        } else {
            Color::from_rgb(0.15, 0.15, 0.17)
        },
        border: Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn tray_stream() -> impl iced::futures::Stream<Item = Msg> {
    stream::channel(32, |mut output| async move {
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

fn direction_words(p: &str) -> String {
    p.chars()
        .map(|c| match c {
            '8' => "上",
            '2' => "下",
            '4' => "左",
            '6' => "右",
            '7' => "左上",
            '9' => "右上",
            '1' => "左下",
            '3' => "右下",
            _ => "?",
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

fn bg() -> Color {
    Color::from_rgb(0.96, 0.96, 0.97)
}
fn sidebar_color() -> Color {
    Color::from_rgb(0.94, 0.94, 0.96)
}
fn card_color() -> Color {
    Color::from_rgb(1.0, 1.0, 1.0)
}
fn hover_color() -> Color {
    Color::from_rgb(0.93, 0.93, 0.95)
}
fn muted() -> Color {
    Color::from_rgb(0.5, 0.5, 0.55)
}
fn accent() -> Color {
    Color::from_rgb(0.0, 0.48, 1.0)
}
fn accent_hover() -> Color {
    Color::from_rgb(0.0, 0.42, 0.9)
}
