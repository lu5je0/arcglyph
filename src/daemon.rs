use anyhow::{Context, Result};
use evdev::{EventType, InputEvent, Key, RelativeAxisType};
use std::{os::fd::AsRawFd, thread, time::Duration};

use crate::{
    config, devices, focus,
    gesture::{self, GestureState},
    injector,
    overlay::{Cmd as OverlayCmd, Overlay},
};

pub fn run() -> Result<()> {
    let gestures = config::load()?;
    eprintln!("loaded {} gestures", gestures.len());

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

    let mut fds: Vec<nix::poll::PollFd> = devs
        .iter()
        .map(|d| {
            nix::poll::PollFd::new(
                unsafe { std::os::fd::BorrowedFd::borrow_raw(d.as_raw_fd()) },
                nix::poll::PollFlags::POLLIN,
            )
        })
        .collect();

    loop {
        nix::poll::poll(&mut fds, nix::poll::PollTimeout::NONE)?;
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
                    &overlay,
                    &focus_tracker,
                )?;
            }
        }
    }
}

fn handle_event(
    ev: InputEvent,
    state: &mut GestureState,
    mouse: &mut evdev::uinput::VirtualDevice,
    kbd: &mut evdev::uinput::VirtualDevice,
    gestures: &[config::Gesture],
    overlay: &Overlay,
    focus: &focus::FocusTracker,
) -> Result<()> {
    match ev.event_type() {
        EventType::KEY if ev.code() == Key::BTN_RIGHT.code() => {
            if ev.value() == 1 {
                let (app, fullscreen) = focus.snapshot();
                if fullscreen {
                    state.bypass = true;
                    mouse.emit(&[ev])?;
                    return Ok(());
                }
                eprintln!("right down, focused app_id={:?}", app);
                state.start(app);
                overlay.send(OverlayCmd::Start);
            } else if ev.value() == 0 {
                if state.bypass {
                    state.bypass = false;
                    mouse.emit(&[ev])?;
                    return Ok(());
                }
                let moved = state.moved_enough();
                let app = state.app_id.clone();
                let matched = state.finish();
                overlay.send(OverlayCmd::Finish);
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
                        thread::sleep(Duration::from_millis(30));
                        injector::emit_right_click(mouse)?;
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
                overlay.send(OverlayCmd::Move(dx, dy));
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
