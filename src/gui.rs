use crate::config::{self, Config, GestureCfg};
use iced::{
    alignment,
    stream,
    widget::{
        button, column, container, horizontal_rule, horizontal_space, row, scrollable, text,
        text_input, Column, Space,
    },
    window, Background, Border, Color, Element, Length, Shadow, Size, Subscription, Task, Theme,
    Vector,
};
use std::sync::{Mutex, OnceLock};

/// Filter selector on the left sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    All,
    App(String),
}

impl Scope {
    fn label(&self) -> String {
        match self {
            Scope::All => "全局".into(),
            Scope::App(a) => a.clone(),
        }
    }
}

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
    // sidebar
    SelectScope(Scope),
    AddApp,
    NewAppInput(String),
    RemoveApp(String),
    // list
    Add,
    Remove(usize),
    KeysChanged(usize, String),
    LabelChanged(usize, String),
    PatternAppend(usize, char),
    PatternBackspace(usize),
    PatternClear(usize),
    // top-right
    Save,
    Reload,
    Toast(String),
    ToastClear,
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
    scope: Scope,
    new_app: String,
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
                scope: Scope::All,
                new_app: String::new(),
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
            Msg::SelectScope(s) => self.scope = s,
            Msg::NewAppInput(v) => self.new_app = v,
            Msg::AddApp => {
                let name = self.new_app.trim().to_string();
                if !name.is_empty() && !self.known_apps().iter().any(|a| a == &name) {
                    self.scope = Scope::App(name);
                }
                self.new_app.clear();
            }
            Msg::RemoveApp(name) => {
                for g in self.cfg.gestures.iter_mut() {
                    g.apps.retain(|a| a != &name);
                }
                if self.scope == Scope::App(name) {
                    self.scope = Scope::All;
                }
                self.dirty = true;
            }
            Msg::Add => {
                let apps = match &self.scope {
                    Scope::All => vec![],
                    Scope::App(a) => vec![a.clone()],
                };
                self.cfg.gestures.push(GestureCfg {
                    pattern: "6".into(),
                    keys: vec!["LEFTALT".into(), "RIGHT".into()],
                    apps,
                    label: Some("新手势".into()),
                });
                self.dirty = true;
            }
            Msg::Remove(i) => {
                if i < self.cfg.gestures.len() {
                    self.cfg.gestures.remove(i);
                    self.dirty = true;
                }
            }
            Msg::KeysChanged(i, v) => {
                if let Some(g) = self.cfg.gestures.get_mut(i) {
                    g.keys = split_list(&v);
                    self.dirty = true;
                }
            }
            Msg::LabelChanged(i, v) => {
                if let Some(g) = self.cfg.gestures.get_mut(i) {
                    g.label = if v.is_empty() { None } else { Some(v) };
                    self.dirty = true;
                }
            }
            Msg::PatternAppend(i, c) => {
                if let Some(g) = self.cfg.gestures.get_mut(i) {
                    if g.pattern.chars().last() != Some(c) {
                        g.pattern.push(c);
                        self.dirty = true;
                    }
                }
            }
            Msg::PatternBackspace(i) => {
                if let Some(g) = self.cfg.gestures.get_mut(i) {
                    g.pattern.pop();
                    self.dirty = true;
                }
            }
            Msg::PatternClear(i) => {
                if let Some(g) = self.cfg.gestures.get_mut(i) {
                    g.pattern.clear();
                    self.dirty = true;
                }
            }
            Msg::Save => match config::save(&self.cfg) {
                Ok(()) => {
                    self.dirty = false;
                    return Task::done(Msg::Toast("已保存".into()));
                }
                Err(e) => return Task::done(Msg::Toast(format!("保存失败: {}", e))),
            },
            Msg::Reload => match config::load_raw() {
                Ok(c) => {
                    self.cfg = c;
                    self.dirty = false;
                    return Task::done(Msg::Toast("已重载".into()));
                }
                Err(e) => return Task::done(Msg::Toast(format!("重载失败: {}", e))),
            },
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

    fn known_apps(&self) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for g in &self.cfg.gestures {
            for a in &g.apps {
                set.insert(a.clone());
            }
        }
        set.into_iter().collect()
    }

    fn visible_gestures(&self) -> Vec<usize> {
        self.cfg
            .gestures
            .iter()
            .enumerate()
            .filter(|(_, g)| match &self.scope {
                Scope::All => g.apps.is_empty(),
                Scope::App(a) => g.apps.iter().any(|x| x == a),
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn view(&self, _id: window::Id) -> Element<'_, Msg> {
        let header = row![
            text(match &self.scope {
                Scope::All => "全局手势".to_string(),
                Scope::App(a) => format!("{} 手势", a),
            })
            .size(24),
            horizontal_space(),
            action_button("重载", Msg::Reload, false),
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

        let mut list = Column::new().spacing(10);
        for i in self.visible_gestures() {
            list = list.push(gesture_card(i, &self.cfg.gestures[i]));
        }
        list = list.push(add_card());

        let body = scrollable(container(list).padding([4, 4]).width(Length::Fill))
            .height(Length::Fill);

        let content = column![header, subtitle, horizontal_rule(1), body,].spacing(12);

        let mut root = row![
            sidebar(&self.scope, &self.known_apps(), &self.new_app),
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
}

fn sidebar<'a>(current: &Scope, apps: &[String], new_app: &str) -> Element<'a, Msg> {
    let mut col = Column::new()
        .spacing(2)
        .push(text("应用").size(11).color(muted()))
        .push(Space::with_height(4))
        .push(sidebar_row(&Scope::All, current == &Scope::All));

    for a in apps {
        let s = Scope::App(a.clone());
        let selected = current == &s;
        col = col.push(sidebar_app_row(a.clone(), selected));
    }

    col = col.push(Space::with_height(10));
    col = col.push(
        row![
            text_input("新应用（app_id 子串）", new_app)
                .on_input(Msg::NewAppInput)
                .on_submit(Msg::AddApp)
                .padding(6)
                .size(12)
                .style(input_style),
            button(text("+").size(14))
                .on_press(Msg::AddApp)
                .padding([4, 10])
                .style(secondary_button_style),
        ]
        .spacing(6),
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

fn sidebar_row<'a>(scope: &Scope, selected: bool) -> Element<'a, Msg> {
    let label = scope.label();
    let s = scope.clone();
    button(text(label).size(13))
        .on_press(Msg::SelectScope(s))
        .padding([8, 12])
        .width(Length::Fill)
        .style(move |_theme, status| sidebar_button_style(status, selected))
        .into()
}

fn sidebar_app_row<'a>(name: String, selected: bool) -> Element<'a, Msg> {
    let label = name.clone();
    let scope = Scope::App(name.clone());
    let remove_name = name;
    let row_inner = row![
        button(text(label).size(13))
            .on_press(Msg::SelectScope(scope))
            .padding([8, 12])
            .width(Length::Fill)
            .style(move |_theme, status| sidebar_button_style(status, selected)),
        button(text("×").size(14).color(muted()))
            .on_press(Msg::RemoveApp(remove_name))
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
        icon_button("×", Msg::Remove(i)),
    ]
    .spacing(12)
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
        .on_press(Msg::Add)
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
