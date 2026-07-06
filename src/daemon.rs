use anyhow::{Context, Result};
use evdev::{EventType, InputEvent, Key, RelativeAxisType};
use std::os::fd::{AsRawFd, AsFd};

use crate::{
    config, devices, focus,
    gesture::{self, GestureState},
    injector,
    overlay::{Cmd as OverlayCmd, Overlay},
};

pub fn run() -> Result<()> {
    let (mut global_enabled, mut gestures) = config::load()?;
    eprintln!("loaded {} gestures (global enabled={})", gestures.len(), global_enabled);

    let mut devs = devices::find_mice()?;
    let mut mouse_out = injector::build_virtual_mouse()?;
    let mut kbd_out = injector::build_virtual_keyboard()?;

    for d in devs.iter_mut() {
        d.grab().context("grab device (need input group)")?;
    }

    let mut state = GestureState::new();
    let overlay = Overlay::spawn().context("spawn overlay thread")?;
    let (focus_tracker, _focus_handle) = focus::spawn().unwrap_or_else(|e| {
        eprintln!("focus tracker disabled: {:#}", e);
        (focus::FocusTracker::default(), focus::FocusHandle::inert())
    });

    let inotify_fd = setup_config_watch();

    let mut fds: Vec<nix::poll::PollFd> = devs
        .iter()
        .map(|d| {
            nix::poll::PollFd::new(
                unsafe { std::os::fd::BorrowedFd::borrow_raw(d.as_raw_fd()) },
                nix::poll::PollFlags::POLLIN,
            )
        })
        .collect();

    if let Some(ref ifd) = inotify_fd {
        fds.push(nix::poll::PollFd::new(
            unsafe { std::os::fd::BorrowedFd::borrow_raw(ifd.as_fd().as_raw_fd()) },
            nix::poll::PollFlags::POLLIN,
        ));
    }

    loop {
        nix::poll::poll(&mut fds, nix::poll::PollTimeout::NONE)?;

        // Check config file change
        if inotify_fd.is_some() {
            let inotify_idx = fds.len() - 1;
            let revents = fds[inotify_idx].revents().unwrap_or(nix::poll::PollFlags::empty());
            if revents.contains(nix::poll::PollFlags::POLLIN) {
                drain_inotify(inotify_fd.as_ref().unwrap());
                match config::load() {
                    Ok((enabled, gs)) => {
                        global_enabled = enabled;
                        gestures = gs;
                        eprintln!("config reloaded: {} gestures (global enabled={})", gestures.len(), global_enabled);
                    }
                    Err(e) => eprintln!("config reload failed: {:#}", e),
                }
            }
        }

        for i in 0..devs.len() {
            let revents = fds[i].revents().unwrap_or(nix::poll::PollFlags::empty());
            if !revents.contains(nix::poll::PollFlags::POLLIN) {
                continue;
            }
            let events: Vec<InputEvent> = devs[i].fetch_events()?.collect();
            for ev in events {
                handle_event(
                    ev,
                    &mut state,
                    &mut mouse_out,
                    &mut kbd_out,
                    &gestures,
                    global_enabled,
                    &overlay,
                    &focus_tracker,
                )?;
            }
        }
    }
}

fn setup_config_watch() -> Option<nix::sys::inotify::Inotify> {
    let p = config::path();
    let dir = p.parent()?;
    if !dir.exists() {
        std::fs::create_dir_all(dir).ok()?;
    }
    let inotify = nix::sys::inotify::Inotify::init(
        nix::sys::inotify::InitFlags::IN_NONBLOCK | nix::sys::inotify::InitFlags::IN_CLOEXEC,
    )
    .ok()?;
    inotify
        .add_watch(
            dir,
            nix::sys::inotify::AddWatchFlags::IN_CLOSE_WRITE
                | nix::sys::inotify::AddWatchFlags::IN_MOVED_TO,
        )
        .ok()?;
    eprintln!("watching config dir: {}", dir.display());
    Some(inotify)
}

fn drain_inotify(inotify: &nix::sys::inotify::Inotify) {
    loop {
        match inotify.read_events() {
            Ok(events) if events.is_empty() => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
}

fn handle_event(
    ev: InputEvent,
    state: &mut GestureState,
    mouse: &mut evdev::uinput::VirtualDevice,
    kbd: &mut evdev::uinput::VirtualDevice,
    gestures: &[config::Gesture],
    global_enabled: bool,
    overlay: &Overlay,
    focus: &focus::FocusTracker,
) -> Result<()> {
    match ev.event_type() {
        EventType::KEY if ev.code() == Key::BTN_RIGHT.code() => {
            if ev.value() == 1 {
                if !global_enabled {
                    state.bypass = true;
                    mouse.emit(&[ev])?;
                    return Ok(());
                }
                let (app, fullscreen, cursor_inside) = focus.snapshot();
                if fullscreen {
                    state.bypass = true;
                    mouse.emit(&[ev])?;
                    return Ok(());
                }
                if !cursor_inside {
                    state.bypass = true;
                    mouse.emit(&[ev])?;
                    return Ok(());
                }
                if !gesture::has_gestures_for_app(gestures, app.as_deref()) {
                    state.bypass = true;
                    mouse.emit(&[ev])?;
                    return Ok(());
                }
                eprintln!("right down, focused app_id={:?}", app);
                state.start(app);
            } else if ev.value() == 0 {
                if state.bypass {
                    state.bypass = false;
                    mouse.emit(&[ev])?;
                    return Ok(());
                }
                let moved = state.overlay_started;
                let app = state.app_id.clone();
                let overlay_was_shown = state.overlay_started;
                let matched = state.finish();
                if overlay_was_shown {
                    overlay.send(OverlayCmd::Finish);
                }
                eprintln!("pattern={:?} app={:?}", matched, app);
                let picked = matched
                    .as_deref()
                    .and_then(|p| gesture::pick(gestures, p, app.as_deref()));
                match picked {
                    Some(keys) => {
                        eprintln!("  -> injecting {} keys: {:?}", keys.len(), keys);
                        injector::emit_shortcut(kbd, &keys)?;
                    }
                    None if !moved => {
                        mouse.emit(&[InputEvent::new(
                            EventType::KEY,
                            Key::BTN_RIGHT.code(),
                            1,
                        )])?;
                        mouse.emit(&[ev])?;
                    }
                    None => {
                        eprintln!("  -> no match");
                    }
                }
            } else {
                mouse.emit(&[ev])?;
            }
        }
        EventType::RELATIVE if state.active => {
            let code = ev.code();
            let (mut dx, mut dy) = (0, 0);
            if code == RelativeAxisType::REL_X.0 {
                dx = ev.value();
                state.feed(dx, 0);
            } else if code == RelativeAxisType::REL_Y.0 {
                dy = ev.value();
                state.feed(0, dy);
            }
            if dx != 0 || dy != 0 {
                if !state.overlay_started && state.moved_enough() {
                    state.overlay_started = true;
                    overlay.send(OverlayCmd::Start);
                }
                if state.overlay_started {
                    overlay.send(OverlayCmd::Move(dx, dy));
                }
            }
            mouse.emit(&[ev])?;
        }
        EventType::SYNCHRONIZATION => {
            mouse.emit(&[ev])?;
        }
        _ => {
            mouse.emit(&[ev])?;
        }
    }
    Ok(())
}
