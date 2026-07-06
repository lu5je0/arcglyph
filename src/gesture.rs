use crate::config::Gesture;
use evdev::Key;

pub const MOVE_THRESHOLD: i32 = 60;
pub const MIN_TOTAL_MOVE: i32 = 20;

#[derive(Default)]
pub struct GestureState {
    pub active: bool,
    pub bypass: bool,
    pub overlay_started: bool,
    pub total_dx: i32,
    pub total_dy: i32,
    acc_dx: i32,
    acc_dy: i32,
    pattern: String,
    last_dir: Option<char>,
    pub app_id: Option<String>,
}

impl GestureState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self, app_id: Option<String>) {
        self.active = true;
        self.overlay_started = false;
        self.total_dx = 0;
        self.total_dy = 0;
        self.acc_dx = 0;
        self.acc_dy = 0;
        self.pattern.clear();
        self.last_dir = None;
        self.app_id = app_id;
    }

    pub fn feed(&mut self, dx: i32, dy: i32) {
        self.total_dx += dx;
        self.total_dy += dy;
        self.acc_dx += dx;
        self.acc_dy += dy;
        let mag2 = self.acc_dx.saturating_mul(self.acc_dx)
            + self.acc_dy.saturating_mul(self.acc_dy);
        if mag2 < MOVE_THRESHOLD * MOVE_THRESHOLD {
            return;
        }
        let dir = classify(self.acc_dx as f32, self.acc_dy as f32);
        match self.last_dir {
            None => {
                self.pattern.push(dir);
                self.last_dir = Some(dir);
            }
            Some(prev) if prev == dir => {}
            Some(prev) => {
                if sector_distance(prev, dir) >= 1 {
                    self.pattern.push(dir);
                    self.last_dir = Some(dir);
                }
            }
        }
        self.acc_dx = 0;
        self.acc_dy = 0;
    }

    pub fn finish(&mut self) -> Option<String> {
        self.active = false;
        if self.total_dx.abs() + self.total_dy.abs() < MIN_TOTAL_MOVE {
            return None;
        }
        if self.pattern.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pattern))
        }
    }

    pub fn moved_enough(&self) -> bool {
        self.total_dx.abs() + self.total_dy.abs() >= MIN_TOTAL_MOVE
    }
}

pub fn has_gestures_for_app(gestures: &[Gesture], app_id: Option<&str>) -> bool {
    let app_lower = app_id.map(|a| a.to_ascii_lowercase());
    for g in gestures {
        if !g.enabled {
            continue;
        }
        if g.apps.is_empty() {
            return true;
        }
        if let Some(ref app) = app_lower {
            if g.apps.iter().any(|a| app.contains(&a.to_ascii_lowercase())) {
                return true;
            }
        }
    }
    false
}

pub fn pick(gestures: &[Gesture], pattern: &str, app_id: Option<&str>) -> Option<Vec<Key>> {
    let app_lower = app_id.map(|a| a.to_ascii_lowercase());
    for g in gestures {
        if !g.enabled || g.pattern != pattern || g.apps.is_empty() {
            continue;
        }
        if let Some(ref app) = app_lower {
            if g.apps.iter().any(|a| app.contains(&a.to_ascii_lowercase())) {
                return Some(g.keys.clone());
            }
        }
    }
    for g in gestures {
        if !g.enabled {
            continue;
        }
        if g.pattern == pattern && g.apps.is_empty() {
            return Some(g.keys.clone());
        }
    }
    None
}

/// Quantize a motion vector into a 4-way cardinal direction character.
/// Screen coordinates: y grows downward.
///
/// Mapping (numpad layout):
///     8         up
///   4 . 6  =>  left . right
///     2         down
///
/// Each direction covers a 90° sector, giving ±45° tolerance.
fn classify(dx: f32, dy: f32) -> char {
    if dx.abs() >= dy.abs() {
        if dx >= 0.0 { '6' } else { '4' }
    } else {
        if dy >= 0.0 { '2' } else { '8' }
    }
}

/// Minimum wrap-around distance between two 4-way direction letters.
/// Distances: 0 (same), 1 (adjacent 90°), 2 (opposite 180°).
fn sector_distance(a: char, b: char) -> u32 {
    let ia = sector_index(a);
    let ib = sector_index(b);
    let d = (ia as i32 - ib as i32).rem_euclid(4) as u32;
    d.min(4 - d)
}

fn sector_index(c: char) -> u32 {
    match c {
        '6' => 0, // right
        '2' => 1, // down
        '4' => 2, // left
        '8' => 3, // up
        _ => 0,
    }
}
