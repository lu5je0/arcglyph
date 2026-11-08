mod model;

use crate::config::{self, GroupCfg};
use crate::desktop::{self, Desktop};
use crate::gesture::GestureState;
use crate::i18n::{t, Lang, S};
use gpui::{
    canvas, div, fill, point, prelude::*, px, rgb, size, AnyElement, AnyWindowHandle, Bounds,
    Context, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathBuilder, Pixels, Point, SharedString, Subscription, Window, WindowBounds,
    WindowDecorations, WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, PopupMenuItem},
    switch::Switch,
    Theme, ThemeMode, TitleBar,
};
use gpui_kit as gpui;
use gpui_kit::component as gpui_component;
use model::{Action, Model, Page};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::OnceLock,
    time::Duration,
};

static TRAY_CHANNEL: OnceLock<async_channel::Sender<ExternalMsg>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum ExternalMsg {
    Show,
    Quit,
}

pub fn send(message: ExternalMsg) {
    let _ = try_send(message);
}

pub fn try_send(message: ExternalMsg) -> bool {
    TRAY_CHANNEL
        .get()
        .is_some_and(|sender| sender.try_send(message).is_ok())
}

pub fn run() -> anyhow::Result<()> {
    let (sender, receiver) = async_channel::unbounded();
    let _ = TRAY_CHANNEL.set(sender);
    gpui_kit::application()
        .with_quit_mode(gpui::QuitMode::Explicit)
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Light, None, cx);
            Theme::update(cx, |theme| {
                theme.font_family = "Noto Sans CJK SC".into();
                theme.font_size = px(13.);
                theme.primary = rgb(ACCENT).into();
                theme.button_primary = rgb(ACCENT).into();
                theme.button_primary_hover = rgb(0x1b6354).into();
                theme.primary_active = rgb(0x185546).into();
                theme.primary_hover = rgb(0x1b6354).into();
                theme.ring = rgb(ACCENT).into();
                theme.radius = px(8.);
            });
            if let Some(path) = std::env::var_os("ARCGLYPH_FONT") {
                if let Ok(bytes) = std::fs::read(path) {
                    let _ = cx.text_system().add_fonts(vec![bytes.into()]);
                }
            }
            let model = Rc::new(RefCell::new(Model::new(
                config::load_raw().unwrap_or_default(),
            )));
            cx.spawn(async move |cx| {
                let mut handle: Option<AnyWindowHandle> = None;
                while let Ok(message) = receiver.recv().await {
                    let model = model.clone();
                    let _ =
                        cx.update(|cx| match message {
                            ExternalMsg::Quit => cx.quit(),
                            ExternalMsg::Show => {
                                if let Some(existing) = handle {
                                    if existing
                                        .update(cx, |_, window, _| window.activate_window())
                                        .is_ok()
                                    {
                                        return;
                                    }
                                }
                                {
                                    let mut state = model.borrow_mut();
                                    if !state.dirty {
                                        if let Ok(cfg) = config::load_raw() {
                                            state.cfg = cfg;
                                            state.group = state
                                                .group
                                                .min(state.cfg.groups.len().saturating_sub(1));
                                            let count = state
                                                .current_group()
                                                .map_or(0, |group| group.gestures.len());
                                            let index = state.gesture.min(count.saturating_sub(1));
                                            state.select_gesture(index);
                                        }
                                    }
                                    state.drawing = false;
                                    state.recording = false;
                                }
                                match gpui_kit::open_window(
                                    WindowOptions {
                                        window_bounds: Some(WindowBounds::Windowed(
                                            Bounds::centered(None, size(px(1120.), px(720.)), cx),
                                        )),
                                        window_min_size: Some(size(px(820.), px(520.))),
                                        app_id: Some("arcglyph".into()),
                                        window_decorations: Some(WindowDecorations::Client),
                                        ..Default::default()
                                    },
                                    cx,
                                    |window, cx| cx.new(|cx| Preferences::new(model, window, cx)),
                                ) {
                                    Ok((window, _view)) => handle = Some(window),
                                    Err(error) => eprintln!("arcglyph preferences: {error:#}"),
                                }
                            }
                        });
                }
            })
            .detach();
        });
    Ok(())
}

const INK: u32 = 0x213833;
const MUTED: u32 = 0x75847e;
const ACCENT: u32 = 0x247766;
const BORDER: u32 = 0xe0e9e5;
const SOFT: u32 = 0xf3f7f5;
const SELECTED: u32 = 0xe3f0e9;
const BACKGROUND: u32 = 0xf9fbfa;

#[derive(Clone, Copy)]
enum Field {
    Group,
    App,
    Keys,
    Label,
}

struct Preferences {
    model: Rc<RefCell<Model>>,
    group_input: Entity<InputState>,
    app_input: Entity<InputState>,
    keys_input: Entity<InputState>,
    label_input: Entity<InputState>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    toast: Option<String>,
    toast_generation: u64,
    picking: bool,
    pick_generation: u64,
    permissions: bool,
    fixing: bool,
    fixed: bool,
    login: bool,
    stroke: Vec<Point<Pixels>>,
    gesture_state: GestureState,
    drawing_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Preferences {
    fn new(model: Rc<RefCell<Model>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let group_input = cx.new(|cx| InputState::new(window, cx));
        let app_input = cx.new(|cx| InputState::new(window, cx).placeholder("chrome, firefox…"));
        let keys_input = cx.new(|cx| InputState::new(window, cx).placeholder("LEFTCTRL + T"));
        let label_input = cx.new(|cx| InputState::new(window, cx));
        let mut subscriptions = vec![];
        for (field, input) in [
            (Field::Group, &group_input),
            (Field::App, &app_input),
            (Field::Keys, &keys_input),
            (Field::Label, &label_input),
        ] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this, input, event, window, cx| match event {
                    InputEvent::Change => this.edit(field, input.read(cx).value().to_string(), cx),
                    InputEvent::PressEnter { .. } if matches!(field, Field::App) => {
                        this.dispatch(Action::AttachApp, window, cx)
                    }
                    _ => {}
                },
            ));
        }
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut view = Self {
            model,
            group_input,
            app_input,
            keys_input,
            label_input,
            focus,
            _subscriptions: subscriptions,
            toast: None,
            toast_generation: 0,
            picking: false,
            pick_generation: 0,
            permissions: crate::perms::probe().ok(),
            fixing: false,
            fixed: false,
            login: crate::autostart::is_enabled(),
            stroke: vec![],
            gesture_state: GestureState::new(),
            drawing_bounds: Rc::new(Cell::new(Bounds::default())),
        };
        view.sync_inputs(window, cx);
        view
    }

    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.model.borrow();
        let values = [
            (
                &self.group_input,
                state
                    .current_group()
                    .map_or(String::new(), |group| group.name.clone()),
            ),
            (&self.app_input, state.app_input.clone()),
            (
                &self.keys_input,
                state
                    .current_gesture()
                    .map_or(String::new(), |gesture| gesture.keys.join(" + ")),
            ),
            (
                &self.label_input,
                state
                    .current_gesture()
                    .and_then(|gesture| gesture.label.clone())
                    .unwrap_or_default(),
            ),
        ];
        for (input, value) in values {
            if input.read(cx).value().as_str() != value {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
    }

    fn edit(&mut self, field: Field, value: String, cx: &mut Context<Self>) {
        let mut state = self.model.borrow_mut();
        let group_index = state.group;
        let changed = match field {
            Field::Group => state.cfg.groups.get_mut(group_index).is_some_and(|group| {
                if group.name == value {
                    return false;
                }
                group.name = value;
                true
            }),
            Field::App => {
                state.app_input = value;
                false
            }
            Field::Keys => state.current_gesture_mut().is_some_and(|gesture| {
                let keys = model::split_keys(&value);
                if gesture.keys == keys {
                    return false;
                }
                gesture.keys = keys;
                true
            }),
            Field::Label => state.current_gesture_mut().is_some_and(|gesture| {
                let label = (!value.is_empty()).then_some(value);
                if gesture.label == label {
                    return false;
                }
                gesture.label = label;
                true
            }),
        };
        state.dirty |= changed;
        cx.notify();
    }

    fn notify(&mut self, message: String, cx: &mut Context<Self>) {
        self.toast = Some(message);
        self.toast_generation += 1;
        let generation = self.toast_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(2000))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.toast_generation == generation {
                    this.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn dispatch(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let lang = self.model.borrow().lang();
        match &action {
            Action::Save => {
                let result = config::save(&self.model.borrow().cfg);
                let message = match result {
                    Ok(()) => {
                        self.model.borrow_mut().dirty = false;
                        t(lang, S::Saved).into()
                    }
                    Err(error) => format!("{}{error:#}", t(lang, S::SaveFailedPrefix)),
                };
                self.notify(message, cx);
            }
            Action::ToggleLogin => {
                let enabled = !self.login;
                match crate::autostart::set(enabled) {
                    Ok(()) => {
                        self.login = enabled;
                        self.notify(
                            t(
                                lang,
                                if enabled {
                                    S::AutostartOn
                                } else {
                                    S::AutostartOff
                                },
                            )
                            .into(),
                            cx,
                        );
                    }
                    Err(error) => {
                        self.notify(format!("{}{error:#}", t(lang, S::SaveFailedPrefix)), cx)
                    }
                }
            }
            Action::Pick => {
                self.pick(window, cx);
                return;
            }
            Action::FixPermissions => {
                self.fix_permissions(window, cx);
                return;
            }
            Action::Quit => {
                cx.quit();
                return;
            }
            _ => {}
        }
        if matches!(
            action,
            Action::AddGroup
                | Action::SelectGroup(_)
                | Action::Settings
                | Action::ManageGroup
                | Action::RemoveGroup(_)
                | Action::CopyGroup(_)
        ) {
            self.pick_generation += 1;
            self.picking = false;
        }
        if matches!(action, Action::Draw) {
            self.stroke.clear();
            self.gesture_state = GestureState::new();
        }
        self.model.borrow_mut().apply(action);
        self.sync_inputs(window, cx);
        cx.notify();
    }

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        self.pick_generation += 1;
        let generation = self.pick_generation;
        let group_index = self.model.borrow().group;
        let (sender, receiver) = async_channel::bounded(1);
        let gnome = desktop::detect() == Desktop::Gnome;
        if !gnome {
            window.minimize_window();
        }
        std::thread::spawn(move || {
            let result = if gnome {
                crate::gnome::Helper::connect().and_then(|helper| helper.pick_app())
            } else {
                std::thread::sleep(Duration::from_millis(1500));
                crate::focus::query().map(|(app, _, _)| (!app.is_empty()).then_some(app))
            };
            let _ = sender.send_blocking(result.map_err(|error| format!("{error:#}")));
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = receiver.recv().await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                if this.pick_generation != generation {
                    return;
                }
                this.picking = false;
                let lang = this.model.borrow().lang();
                let message = match result {
                    Ok(Some(app)) => {
                        let mut model = this.model.borrow_mut();
                        if let Some(group) = model.cfg.groups.get_mut(group_index) {
                            if !group.apps.contains(&app) {
                                group.apps.push(app.clone());
                                model.dirty = true;
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
                window.activate_window();
                this.notify(message, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn fix_permissions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.fixing {
            return;
        }
        self.fixing = true;
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                use anyhow::Context;
                let output = std::process::Command::new("id").arg("-un").output()?;
                let user = String::from_utf8_lossy(&output.stdout).trim().to_string();
                anyhow::ensure!(!user.is_empty(), "could not determine current username");
                let output = std::process::Command::new("pkexec")
                    .args(["usermod", "-aG", "input", &user])
                    .output()
                    .context("cannot start pkexec")?;
                anyhow::ensure!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                Ok(())
            })();
            let _ = sender.send_blocking(result.map_err(|error| format!("{error:#}")));
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = receiver.recv().await else {
                return;
            };
            let _ = this.update_in(cx, |this, _, cx| {
                this.fixing = false;
                let lang = this.model.borrow().lang();
                let message = match result {
                    Ok(()) => {
                        this.fixed = true;
                        t(lang, S::PermFixDone).into()
                    }
                    Err(error) => format!("{}{error}", t(lang, S::PermFixFailedPrefix)),
                };
                this.notify(message, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let recording = self.model.borrow().recording;
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            self.dispatch(Action::Cancel, window, cx);
            cx.stop_propagation();
        } else if recording {
            cx.stop_propagation();
            window.prevent_default();
            if let Some(main) = evdev_key(key) {
                let mods = &event.keystroke.modifiers;
                let mut keys = vec![];
                for (pressed, name) in [
                    (mods.control, "LEFTCTRL"),
                    (mods.shift, "LEFTSHIFT"),
                    (mods.alt, "LEFTALT"),
                    (mods.platform, "LEFTMETA"),
                ] {
                    if pressed {
                        keys.push(name.to_string());
                    }
                }
                keys.push(main);
                {
                    let mut state = self.model.borrow_mut();
                    if let Some(gesture) = state.current_gesture_mut() {
                        gesture.keys = keys;
                        state.dirty = true;
                    }
                    state.recording = false;
                }
                self.sync_inputs(window, cx);
                cx.notify();
            }
        } else if event.keystroke.modifiers.control && key == "s" {
            self.dispatch(Action::Save, window, cx);
            cx.stop_propagation();
            window.prevent_default();
        }
    }

    fn button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        action: Action,
        primary: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        Button::new(id.into())
            .label(label.into())
            .h(px(32.))
            .px(px(12.))
            .rounded(px(7.))
            .text_size(px(12.))
            .when(primary, |button| button.primary())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.dispatch(action.clone(), window, cx);
            }))
            .into_any_element()
    }

    fn toggle(
        &self,
        id: &'static str,
        title: &'static str,
        detail: &'static str,
        enabled: bool,
        action: Action,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(div().font_weight(FontWeight::MEDIUM).child(title))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(detail),
                    ),
            )
            .child(
                Switch::new(id)
                    .checked(enabled)
                    .accessibility_label(title)
                    .on_change(cx.listener(move |this, _, window, cx| {
                        this.dispatch(action.clone(), window, cx)
                    })),
            )
            .into_any_element()
    }

    fn sidebar(&self, state: &Model, cx: &Context<Self>) -> AnyElement {
        let lang = state.lang();
        let mut groups = div()
            .id("group-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(6.));
        for (index, group) in state.cfg.groups.iter().enumerate() {
            let selected = state.page == Page::Groups && state.group == index;
            let subtitle = if group.apps.is_empty() {
                t(lang, S::AppliesAll).to_string()
            } else {
                format!("{} {}", group.apps.len(), t(lang, S::LinkedCount))
            };
            let owner = cx.entity().downgrade();
            let item = div()
                .id(("group", index))
                .flex()
                .items_center()
                .gap(px(8.))
                .p(px(11.))
                .rounded(px(9.))
                .cursor_pointer()
                .bg(rgb(if selected { SELECTED } else { 0xf0f5f2 }))
                .hover(|style| style.bg(rgb(SELECTED)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.dispatch(Action::SelectGroup(index), window, cx)
                }))
                .child(div().size(px(7.)).rounded_full().bg(rgb(if group.enabled {
                    ACCENT
                } else {
                    MUTED
                })))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(if selected { ACCENT } else { INK }))
                                .child(group.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(rgb(MUTED))
                                .truncate()
                                .child(subtitle),
                        ),
                )
                .child(
                    Button::new(("delete-group", index))
                        .label("×")
                        .ghost()
                        .h(px(24.))
                        .w(px(24.))
                        .p_0()
                        .text_size(px(16.))
                        .text_color(rgb(MUTED))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.dispatch(Action::RemoveGroup(index), window, cx);
                        })),
                )
                .context_menu(move |menu, _, _| {
                    let copy = owner.clone();
                    let remove = owner.clone();
                    menu.item(PopupMenuItem::new(t(lang, S::CopyGroup)).on_click(
                        move |_, window, cx| {
                            let _ = copy.update(cx, |this, cx| {
                                this.dispatch(Action::CopyGroup(index), window, cx)
                            });
                        },
                    ))
                    .separator()
                    .item(
                        PopupMenuItem::new(t(lang, S::Delete)).on_click(move |_, window, cx| {
                            let _ = remove.update(cx, |this, cx| {
                                this.dispatch(Action::RemoveGroup(index), window, cx)
                            });
                        }),
                    )
                });
            groups = groups.child(item);
        }

        div()
            .w(px(208.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(0xf0f5f2))
            .border_r_1()
            .border_color(rgb(BORDER))
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(36.))
                    .child(
                        div()
                            .size(px(30.))
                            .rounded(px(9.))
                            .bg(rgb(ACCENT))
                            .text_color(rgb(0xffffff))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(21.))
                            .child("↗"),
                    )
                    .child(
                        div()
                            .text_size(px(21.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("ArcGlyph"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(self.nav(
                        "nav-groups",
                        "▱",
                        t(lang, S::GestureGroups),
                        Action::Groups,
                        state.page == Page::Groups,
                        cx,
                    ))
                    .child(self.nav(
                        "nav-settings",
                        "☷",
                        t(lang, S::AppSettings),
                        Action::Settings,
                        state.page == Page::Settings,
                        cx,
                    )),
            )
            .child(div().h(px(1.)).bg(rgb(BORDER)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t(lang, S::Groups)),
                    )
                    .child(self.button("add-group", "+", Action::AddGroup, false, cx)),
            )
            .child(groups)
            .child(
                div()
                    .id("global-state")
                    .p(px(11.))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(0xf8fbf9))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dispatch(Action::Settings, window, cx)
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .child(
                                div()
                                    .size(px(7.))
                                    .rounded_full()
                                    .bg(rgb(if state.cfg.enabled { ACCENT } else { MUTED })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(
                                        div().text_size(px(12.)).child(t(lang, S::GlobalGestures)),
                                    )
                                    .child(div().text_size(px(10.)).text_color(rgb(MUTED)).child(
                                        t(
                                            lang,
                                            if state.cfg.enabled {
                                                S::Running
                                            } else {
                                                S::Paused
                                            },
                                        ),
                                    )),
                            )
                            .child(
                                div()
                                    .mt(px(8.))
                                    .text_size(px(10.))
                                    .text_color(rgb(MUTED))
                                    .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn nav(
        &self,
        id: &'static str,
        icon: &'static str,
        label: &'static str,
        action: Action,
        selected: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(39.))
            .px(px(11.))
            .rounded(px(8.))
            .cursor_pointer()
            .bg(rgb(if selected { SELECTED } else { 0xf0f5f2 }))
            .text_color(rgb(if selected { ACCENT } else { INK }))
            .hover(|style| style.bg(rgb(SELECTED)))
            .on_click(
                cx.listener(move |this, _, window, cx| this.dispatch(action.clone(), window, cx)),
            )
            .child(div().w(px(16.)).text_size(px(17.)).child(icon))
            .child(label)
            .into_any_element()
    }

    fn group_page(&self, state: &Model, cx: &Context<Self>) -> AnyElement {
        let lang = state.lang();
        let Some(group) = state.current_group() else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(MUTED))
                .child(t(lang, S::SelectOrAddGroup))
                .into_any_element();
        };
        let apps = if group.apps.is_empty() {
            t(lang, S::AppliesAll).to_string()
        } else {
            group.apps.join("   ·   ")
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(14.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(30.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(group.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(MUTED))
                                    .child(t(lang, S::GroupScope)),
                            ),
                    )
                    .child(card().w(px(208.)).p(px(12.)).child(self.toggle(
                        "enable-group",
                        t(lang, S::EnableGroup),
                        t(lang, S::GroupOnly),
                        group.enabled,
                        Action::ToggleGroup,
                        cx,
                    ))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(t(lang, S::AssociatedApps)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .truncate()
                            .child(apps),
                    )
                    .child(self.button(
                        "manage-group",
                        t(
                            lang,
                            if state.managing {
                                S::Done
                            } else {
                                S::ManageGroup
                            },
                        ),
                        Action::ManageGroup,
                        false,
                        cx,
                    )),
            )
            .child(divider())
            .child(if state.managing {
                self.group_editor(state, group, cx)
            } else {
                self.workspace(state, group, cx)
            })
            .into_any_element()
    }

    fn group_editor(&self, state: &Model, group: &GroupCfg, cx: &Context<Self>) -> AnyElement {
        let lang = state.lang();
        let chips = group.apps.iter().enumerate().map(|(index, app)| {
            let app = app.clone();
            div()
                .id(("app-chip", index))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(9.))
                .py(px(5.))
                .bg(rgb(SELECTED))
                .text_color(rgb(ACCENT))
                .rounded(px(6.))
                .child(app.clone())
                .child(
                    div()
                        .id(("detach", index))
                        .cursor_pointer()
                        .text_color(rgb(MUTED))
                        .child("×")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.dispatch(Action::DetachApp(app.clone()), window, cx)
                        })),
                )
        });
        div()
            .id("group-editor-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                card()
                    .p(px(18.))
                    .flex()
                    .flex_col()
                    .gap(px(15.))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(7.))
                                    .child(caption(t(lang, S::GroupName)))
                                    .child(Input::new(&self.group_input).h(px(34.))),
                            )
                            .child(self.button(
                                "copy-group",
                                t(lang, S::CopyGroup),
                                Action::CopyGroup(state.group),
                                false,
                                cx,
                            )),
                    )
                    .child(caption(t(lang, S::AssociatedApps)))
                    .child(div().flex().flex_wrap().gap(px(7.)).children(chips))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&self.app_input).h(px(34.))),
                            )
                            .child(self.button(
                                "attach-app",
                                t(lang, S::Add),
                                Action::AttachApp,
                                false,
                                cx,
                            ))
                            .child(self.button(
                                "pick-app",
                                t(
                                    lang,
                                    if self.picking {
                                        S::ClickTargetWindow
                                    } else {
                                        S::PickWindow
                                    },
                                ),
                                Action::Pick,
                                false,
                                cx,
                            )),
                    ),
            )
            .into_any_element()
    }

    fn workspace(&self, state: &Model, group: &GroupCfg, cx: &Context<Self>) -> AnyElement {
        let lang = state.lang();
        let mut rows = div()
            .id("gesture-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(6.));
        for (index, gesture) in group.gestures.iter().enumerate() {
            let selected = state.gesture == index;
            rows = rows.child(
                div()
                    .id(("gesture", index))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .min_h(px(46.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(rgb(if selected { 0xc9e0d3 } else { BORDER }))
                    .bg(rgb(if selected { SELECTED } else { 0xffffff }))
                    .hover(|style| style.bg(rgb(0xedf5f0)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.dispatch(Action::SelectGesture(index), window, cx)
                    }))
                    .child(
                        div()
                            .w(px(47.))
                            .h(px(28.))
                            .rounded(px(6.))
                            .bg(rgb(if selected { 0xd5e9dc } else { SOFT }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(ACCENT))
                            .text_size(px(17.))
                            .child(gesture.pattern.chars().map(arrow).collect::<String>()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(
                                gesture
                                    .label
                                    .clone()
                                    .unwrap_or_else(|| t(lang, S::Unnamed).into()),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(10.))
                            .px(px(6.))
                            .py(px(3.))
                            .rounded(px(5.))
                            .bg(rgb(if selected { 0xf1f7f3 } else { SOFT }))
                            .text_color(rgb(MUTED))
                            .child(shortcut(&gesture.keys)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(10.))
                            .text_color(rgb(if gesture.enabled { ACCENT } else { MUTED }))
                            .child(t(
                                lang,
                                if gesture.enabled {
                                    S::Enabled
                                } else {
                                    S::Paused
                                },
                            )),
                    ),
            );
        }
        let list = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(14.))
                            .child(t(lang, S::Gestures)),
                    )
                    .child(
                        div()
                            .rounded(px(5.))
                            .bg(rgb(SELECTED))
                            .text_color(rgb(ACCENT))
                            .text_size(px(10.))
                            .px(px(6.))
                            .py(px(2.))
                            .child(group.gestures.len().to_string()),
                    )
                    .child(div().flex_1())
                    .child(self.button(
                        "add-gesture",
                        t(lang, S::AddGesture),
                        Action::AddGesture,
                        false,
                        cx,
                    )),
            )
            .child(rows);
        let mut workspace = div().flex_1().min_h_0().flex().gap(px(16.)).child(list);
        if state.current_gesture().is_some() {
            workspace = workspace.child(self.inspector(state, cx));
        }
        workspace.into_any_element()
    }

    fn inspector(&self, state: &Model, cx: &Context<Self>) -> AnyElement {
        let lang = state.lang();
        let gesture = state.current_gesture().unwrap();
        let preview =
            if state.drawing {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(caption(t(lang, S::DrawPatternHint)))
                    .child(self.gesture_canvas(&gesture.pattern, true, cx))
                    .child(self.button(
                        "cancel-drawing",
                        t(lang, S::Cancel),
                        Action::Cancel,
                        false,
                        cx,
                    ))
                    .into_any_element()
            } else {
                let mut editor = div()
                    .flex()
                    .flex_col()
                    .gap(px(9.))
                    .child(self.gesture_canvas(&gesture.pattern, false, cx))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .text_center()
                            .child(direction_words(lang, &gesture.pattern)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(self.button(
                                "draw-gesture",
                                t(lang, S::DrawPattern),
                                Action::Draw,
                                false,
                                cx,
                            ))
                            .child(self.button(
                                "edit-directions",
                                t(
                                    lang,
                                    if state.directions {
                                        S::Done
                                    } else {
                                        S::DragDirection
                                    },
                                ),
                                Action::Directions,
                                false,
                                cx,
                            )),
                    );
                if state.directions {
                    let mut pad = div().flex().flex_col().gap(px(4.));
                    for directions in [['7', '8', '9'], ['4', ' ', '6'], ['1', '2', '3']] {
                        pad = pad.child(div().flex().gap(px(4.)).children(
                            directions.into_iter().map(|direction| {
                                if direction == ' ' {
                                    div().size(px(30.)).into_any_element()
                                } else {
                                    Button::new(SharedString::from(format!(
                                        "direction-{direction}"
                                    )))
                                    .label(arrow(direction).to_string())
                                    .h(px(30.))
                                    .w(px(30.))
                                    .p_0()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.dispatch(
                                            Action::AppendDirection(direction),
                                            window,
                                            cx,
                                        );
                                    }))
                                    .into_any_element()
                                }
                            }),
                        ));
                    }
                    editor = editor.child(
                        div().flex().items_center().gap(px(8.)).child(pad).child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(5.))
                                .child(self.button(
                                    "backspace-pattern",
                                    t(lang, S::Delete),
                                    Action::BackspaceDirection,
                                    false,
                                    cx,
                                ))
                                .child(self.button(
                                    "clear-pattern",
                                    t(lang, S::Clear),
                                    Action::ClearPattern,
                                    false,
                                    cx,
                                )),
                        ),
                    );
                }
                editor.into_any_element()
            };
        div()
            .id("inspector-scroll")
            .w(px(298.))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .child(
                card()
                    .p(px(15.))
                    .flex()
                    .flex_col()
                    .gap(px(13.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(caption(t(lang, S::GestureDetails)))
                            .child(
                                div()
                                    .px(px(6.))
                                    .py(px(2.))
                                    .bg(rgb(SOFT))
                                    .rounded(px(5.))
                                    .text_size(px(10.))
                                    .text_color(rgb(MUTED))
                                    .child(gesture.pattern.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(
                                gesture
                                    .label
                                    .clone()
                                    .unwrap_or_else(|| t(lang, S::Unnamed).into()),
                            ),
                    )
                    .child(preview)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(7.))
                            .child(caption(t(lang, S::Shortcut)))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            Input::new(&self.keys_input)
                                                .h(px(34.))
                                                .disabled(state.recording),
                                        ),
                                    )
                                    .child(self.button(
                                        "record-keys",
                                        t(
                                            lang,
                                            if state.recording {
                                                S::PressKeys
                                            } else {
                                                S::Record
                                            },
                                        ),
                                        Action::Record,
                                        state.recording,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(7.))
                            .child(caption(t(lang, S::Description)))
                            .child(Input::new(&self.label_input).h(px(34.))),
                    )
                    .child(divider())
                    .child(self.toggle(
                        "enable-gesture",
                        t(lang, S::EnableGesture),
                        t(lang, S::GestureOnly),
                        gesture.enabled,
                        Action::ToggleGesture,
                        cx,
                    ))
                    .child(divider())
                    .child(self.button(
                        "remove-gesture",
                        t(lang, S::RemoveGesture),
                        Action::RemoveGesture,
                        false,
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn settings(&self, state: &Model, cx: &Context<Self>) -> AnyElement {
        let lang = state.lang();
        let login = if crate::autostart::is_managed() {
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(t(lang, S::LaunchAtLogin))
                .child(caption(t(lang, S::LoginManaged)))
                .into_any_element()
        } else {
            self.toggle(
                "login-toggle",
                t(lang, S::LaunchAtLogin),
                t(lang, S::LoginScope),
                self.login,
                Action::ToggleLogin,
                cx,
            )
        };
        div()
            .id("settings-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .max_w(px(760.))
                    .flex()
                    .flex_col()
                    .gap(px(17.))
                    .child(
                        div()
                            .text_size(px(30.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t(lang, S::AppSettings)),
                    )
                    .child(
                        card()
                            .p(px(18.))
                            .flex()
                            .flex_col()
                            .gap(px(15.))
                            .child(section(t(lang, S::GestureRecognition)))
                            .child(self.toggle(
                                "global-toggle",
                                t(lang, S::GlobalSwitch),
                                t(lang, S::GlobalScope),
                                state.cfg.enabled,
                                Action::ToggleGlobal,
                                cx,
                            )),
                    )
                    .child(
                        card()
                            .p(px(18.))
                            .flex()
                            .flex_col()
                            .gap(px(15.))
                            .child(section(t(lang, S::StartupAndTray)))
                            .child(login)
                            .child(divider())
                            .child(self.toggle(
                                "tray-toggle",
                                t(lang, S::ShowTray),
                                t(lang, S::TrayScope),
                                !state.cfg.hide_tray,
                                Action::ToggleTray,
                                cx,
                            )),
                    )
                    .child(
                        card()
                            .p(px(18.))
                            .flex()
                            .flex_col()
                            .gap(px(13.))
                            .child(section(t(lang, S::InterfaceAndConfig)))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(t(lang, S::Language))
                                    .child(self.button(
                                        "language",
                                        if lang == Lang::En {
                                            "简体中文"
                                        } else {
                                            "English"
                                        },
                                        Action::Language,
                                        false,
                                        cx,
                                    )),
                            )
                            .child(divider())
                            .child(caption(t(lang, S::ConfigFile)))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED))
                                    .child(config::path().display().to_string()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .pt(px(5.))
                            .child(caption(t(lang, S::ExitAppDescription)))
                            .child(self.button(
                                "quit",
                                t(lang, S::ExitApp),
                                Action::Quit,
                                false,
                                cx,
                            )),
                    ),
            )
            .into_any_element()
    }

    fn gesture_canvas(&self, pattern: &str, drawing: bool, cx: &Context<Self>) -> AnyElement {
        let path = if drawing {
            self.stroke.clone()
        } else {
            pattern_points(pattern)
        };
        let bounds_cell = self.drawing_bounds.clone();
        let visual = canvas(
            move |bounds, _, _| {
                bounds_cell.set(bounds);
            },
            move |bounds, (), window, _| paint_gesture(bounds, &path, drawing, window),
        )
        .size_full();
        let area = div()
            .id("gesture-canvas")
            .relative()
            .h(px(if drawing { 166. } else { 108. }))
            .w_full()
            .rounded(px(9.))
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SOFT))
            .overflow_hidden()
            .child(visual);
        if !drawing {
            return area.into_any_element();
        }
        area.cursor_crosshair()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.gesture_state.start(None);
                    this.stroke.clear();
                    this.stroke
                        .push(event.position - this.drawing_bounds.get().origin);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.gesture_state.active {
                    let current = event.position - this.drawing_bounds.get().origin;
                    if let Some(previous) = this.stroke.last() {
                        this.gesture_state.feed(
                            f32::from(current.x - previous.x).round() as i32,
                            f32::from(current.y - previous.y).round() as i32,
                        );
                    }
                    this.stroke.push(current);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.finish_drawing(window, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.finish_drawing(window, cx)),
            )
            .into_any_element()
    }

    fn finish_drawing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.gesture_state.active {
            return;
        }
        if let Some(pattern) = self.gesture_state.finish() {
            let mut state = self.model.borrow_mut();
            if let Some(gesture) = state.current_gesture_mut() {
                gesture.pattern = pattern.recorded();
                state.dirty = true;
            }
            state.drawing = false;
        }
        self.sync_inputs(window, cx);
        cx.notify();
    }
}

impl Render for Preferences {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.model.borrow().clone();
        let lang = state.lang();
        let mut main = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(18.))
            .p(px(22.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(MUTED))
                            .child(t(
                                lang,
                                if state.page == Page::Groups {
                                    S::GestureGroups
                                } else {
                                    S::AppSettings
                                },
                            )),
                    )
                    .child(div().size(px(5.)).rounded_full().bg(rgb(if state.dirty {
                        0xb77b38
                    } else {
                        ACCENT
                    })))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(t(lang, if state.dirty { S::Unsaved } else { S::Synced })),
                    )
                    .child(self.button("save", t(lang, S::Save), Action::Save, true, cx)),
            );
        if !self.permissions {
            main = main.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(rgb(0xfff6e5))
                    .border_1()
                    .border_color(rgb(0xf0d9ac))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t(lang, S::PermTitle)),
                            )
                            .child(div().text_size(px(11.)).child(t(
                                lang,
                                if self.fixed {
                                    S::PermFixDone
                                } else {
                                    S::PermBody
                                },
                            ))),
                    )
                    .child(self.button(
                        "fix-permissions",
                        t(
                            lang,
                            if self.fixing {
                                S::PermFixRunning
                            } else {
                                S::PermFixButton
                            },
                        ),
                        Action::FixPermissions,
                        false,
                        cx,
                    )),
            );
        }
        main = main.child(match state.page {
            Page::Groups => self.group_page(&state, cx),
            Page::Settings => self.settings(&state, cx),
        });
        let mut content = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.sidebar(&state, cx))
            .child(main);
        if let Some(message) = &self.toast {
            content = content.child(
                div()
                    .absolute()
                    .bottom(px(18.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .max_w(px(480.))
                            .px(px(18.))
                            .py(px(10.))
                            .rounded(px(10.))
                            .shadow_lg()
                            .bg(rgb(INK))
                            .text_color(rgb(0xffffff))
                            .text_size(px(12.))
                            .child(message.clone()),
                    ),
            );
        }
        let titlebar = TitleBar::new().bg(rgb(0xf0f5f2)).child(
            div()
                .flex_1()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(MUTED))
                .child("ArcGlyph"),
        );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BACKGROUND))
            .text_color(rgb(INK))
            .font_family("Noto Sans CJK SC")
            .text_size(px(13.))
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::key_down))
            .child(titlebar)
            .child(content)
    }
}

fn card() -> gpui::Div {
    div()
        .rounded(px(11.))
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(0xffffff))
}

fn divider() -> gpui::Div {
    div().h(px(1.)).w_full().flex_shrink_0().bg(rgb(BORDER))
}

fn caption(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(11.))
        .text_color(rgb(MUTED))
        .child(text.into())
}

fn section(text: &'static str) -> gpui::Div {
    div()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

fn arrow(direction: char) -> char {
    match direction {
        '1' => '↙',
        '2' => '↓',
        '3' => '↘',
        '4' => '←',
        '6' => '→',
        '7' => '↖',
        '8' => '↑',
        '9' => '↗',
        _ => '·',
    }
}

fn direction_words(lang: Lang, pattern: &str) -> String {
    pattern
        .chars()
        .map(|direction| {
            t(
                lang,
                match direction {
                    '1' => S::DirDownLeft,
                    '2' => S::DirDown,
                    '3' => S::DirDownRight,
                    '4' => S::DirLeft,
                    '6' => S::DirRight,
                    '7' => S::DirUpLeft,
                    '8' => S::DirUp,
                    _ => S::DirUpRight,
                },
            )
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn shortcut(keys: &[String]) -> String {
    keys.iter()
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
            other => other,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn evdev_key(key: &str) -> Option<String> {
    let named = match key {
        "enter" => "ENTER",
        "tab" => "TAB",
        "space" => "SPACE",
        "backspace" => "BACKSPACE",
        "delete" => "DELETE",
        "insert" => "INSERT",
        "home" => "HOME",
        "end" => "END",
        "pageup" => "PAGEUP",
        "pagedown" => "PAGEDOWN",
        "up" => "UP",
        "down" => "DOWN",
        "left" => "LEFT",
        "right" => "RIGHT",
        "-" => "MINUS",
        "=" => "EQUAL",
        "[" => "LEFTBRACE",
        "]" => "RIGHTBRACE",
        ";" => "SEMICOLON",
        "'" => "APOSTROPHE",
        "`" => "GRAVE",
        "\\" => "BACKSLASH",
        "," => "COMMA",
        "." => "DOT",
        "/" => "SLASH",
        value if value.len() == 1 && value.as_bytes()[0].is_ascii_alphanumeric() => {
            return Some(value.to_ascii_uppercase())
        }
        value
            if value.starts_with('f')
                && value[1..]
                    .parse::<u8>()
                    .is_ok_and(|n| (1..=24).contains(&n)) =>
        {
            return Some(value.to_ascii_uppercase())
        }
        _ => return None,
    };
    Some(named.into())
}

fn pattern_points(pattern: &str) -> Vec<Point<Pixels>> {
    let mut points = vec![point(px(0.), px(0.))];
    for direction in pattern.chars() {
        let (x, y) = match direction {
            '1' => (-1., 1.),
            '2' => (0., 1.),
            '3' => (1., 1.),
            '4' => (-1., 0.),
            '6' => (1., 0.),
            '7' => (-1., -1.),
            '8' => (0., -1.),
            '9' => (1., -1.),
            _ => continue,
        };
        points.push(*points.last().unwrap() + point(px(x), px(y)));
    }
    points
}

fn paint_gesture(
    bounds: Bounds<Pixels>,
    points: &[Point<Pixels>],
    drawing: bool,
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    for x in (12..width as usize).step_by(14) {
        for y in (12..height as usize).step_by(14) {
            window.paint_quad(fill(
                Bounds::new(
                    bounds.origin + point(px(x as f32), px(y as f32)),
                    size(px(1.), px(1.)),
                ),
                rgb(0xd4e1d9),
            ));
        }
    }
    if points.len() < 2 {
        return;
    }
    let mut points = points.to_vec();
    if !drawing {
        let min_x = points
            .iter()
            .map(|p| f32::from(p.x))
            .fold(f32::INFINITY, f32::min);
        let max_x = points
            .iter()
            .map(|p| f32::from(p.x))
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = points
            .iter()
            .map(|p| f32::from(p.y))
            .fold(f32::INFINITY, f32::min);
        let max_y = points
            .iter()
            .map(|p| f32::from(p.y))
            .fold(f32::NEG_INFINITY, f32::max);
        let scale = 43_f32
            .min((width - 36.) / (max_x - min_x).max(1.))
            .min((height - 30.) / (max_y - min_y).max(1.));
        for p in &mut points {
            p.x = px(width / 2. + (f32::from(p.x) - (min_x + max_x) / 2.) * scale);
            p.y = px(height / 2. + (f32::from(p.y) - (min_y + max_y) / 2.) * scale);
        }
    }
    let mut builder = PathBuilder::stroke(px(if drawing { 3. } else { 2.5 }));
    builder.move_to(bounds.origin + points[0]);
    for point in &points[1..] {
        builder.line_to(bounds.origin + *point);
    }
    if !drawing {
        let end = points[points.len() - 1];
        let previous = points[points.len() - 2];
        let angle = f32::from(end.y - previous.y).atan2(f32::from(end.x - previous.x));
        for offset in [-0.65, 0.65] {
            builder.move_to(bounds.origin + end);
            builder.line_to(
                bounds.origin + end
                    - point(
                        px((angle + offset).cos() * 8.),
                        px((angle + offset).sin() * 8.),
                    ),
            );
        }
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, rgb(ACCENT));
    }
}
