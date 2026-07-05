use anyhow::{Context, Result};
use evdev::{
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AttributeSet, EventType, InputEvent, Key, RelativeAxisType,
};
use std::{thread, time::Duration};

pub fn build_virtual_mouse() -> Result<VirtualDevice> {
    let mut keys = AttributeSet::<Key>::new();
    for k in [
        Key::BTN_LEFT,
        Key::BTN_RIGHT,
        Key::BTN_MIDDLE,
        Key::BTN_SIDE,
        Key::BTN_EXTRA,
        Key::BTN_FORWARD,
        Key::BTN_BACK,
    ] {
        keys.insert(k);
    }
    let mut rel = AttributeSet::<RelativeAxisType>::new();
    rel.insert(RelativeAxisType::REL_X);
    rel.insert(RelativeAxisType::REL_Y);
    rel.insert(RelativeAxisType::REL_WHEEL);
    rel.insert(RelativeAxisType::REL_HWHEEL);
    rel.insert(RelativeAxisType::REL_WHEEL_HI_RES);
    rel.insert(RelativeAxisType::REL_HWHEEL_HI_RES);

    VirtualDeviceBuilder::new()?
        .name("arcglyph-virtual-mouse")
        .with_keys(&keys)?
        .with_relative_axes(&rel)?
        .build()
        .context("build virtual mouse (need /dev/uinput permission)")
}

pub fn build_virtual_keyboard() -> Result<VirtualDevice> {
    let mut keys = AttributeSet::<Key>::new();
    for code in 1u16..=248 {
        keys.insert(Key::new(code));
    }
    VirtualDeviceBuilder::new()?
        .name("arcglyph-virtual-keyboard")
        .with_keys(&keys)?
        .build()
        .context("build virtual keyboard")
}

pub fn emit_shortcut(kbd: &mut VirtualDevice, keys: &[Key]) -> Result<()> {
    for k in keys {
        kbd.emit(&[InputEvent::new(EventType::KEY, k.code(), 1)])?;
        thread::sleep(Duration::from_millis(5));
    }
    for k in keys.iter().rev() {
        kbd.emit(&[InputEvent::new(EventType::KEY, k.code(), 0)])?;
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

pub fn emit_right_click(mouse: &mut VirtualDevice) -> Result<()> {
    mouse.emit(&[InputEvent::new(EventType::KEY, Key::BTN_RIGHT.code(), 1)])?;
    thread::sleep(Duration::from_millis(15));
    mouse.emit(&[InputEvent::new(EventType::KEY, Key::BTN_RIGHT.code(), 0)])?;
    Ok(())
}
