use crate::config::Gesture;
use evdev::Key;

pub const MOVE_THRESHOLD: i32 = 40;
pub const MIN_TOTAL_MOVE: i32 = 20;

#[derive(Default)]
pub struct GestureState {
    pub active: bool,
    pub bypass: bool,
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
                if sector_distance(prev, dir) >= 2 {
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

pub fn pick(gestures: &[Gesture], pattern: &str, app_id: Option<&str>) -> Option<Vec<Key>> {
    let app_lower = app_id.map(|a| a.to_ascii_lowercase());
    for g in gestures {
        if g.pattern != pattern || g.apps.is_empty() {
            continue;
        }
        if let Some(ref app) = app_lower {
            if g.apps.iter().any(|a| app.contains(&a.to_ascii_lowercase())) {
                return Some(g.keys.clone());
            }
        }
    }
    for g in gestures {
        if g.pattern == pattern && g.apps.is_empty() {
            return Some(g.keys.clone());
        }
    }
    None
}

/// Quantize a motion vector into a numpad-style 8-way direction character.
/// Screen coordinates: y grows downward.
///
/// Mapping (numpad layout):
///   7 8 9         up-left  up    up-right
///   4 . 6   =>    left     .     right
///   1 2 3         dn-left  down  dn-right
fn classify(dx: f32, dy: f32) -> char {
    let angle = dy.atan2(dx); // -pi..pi, 0 = +x (right)
    let deg = angle.to_degrees();
    // Rotate so that "right" is centered on sector 0 (-22.5..22.5).
    let shifted = ((deg + 22.5).rem_euclid(360.0)) / 45.0;
    match shifted as u32 {
        0 => '6', // right
        1 => '3', // down-right
        2 => '2', // down
        3 => '1', // down-left
        4 => '4', // left
        5 => '7', // up-left
        6 => '8', // up
        7 => '9', // up-right
        _ => '6',
    }
}

/// Minimum wrap-around distance between two 8-way direction letters.
/// Distances: 0 (same), 1 (adjacent 45°), 2 (perpendicular 90°), … 4 (opposite).
fn sector_distance(a: char, b: char) -> u32 {
    let ia = sector_index(a);
    let ib = sector_index(b);
    let d = (ia as i32 - ib as i32).rem_euclid(8) as u32;
    d.min(8 - d)
}

fn sector_index(c: char) -> u32 {
    match c {
        '6' => 0, // right
        '3' => 1, // down-right
        '2' => 2, // down
        '1' => 3, // down-left
        '4' => 4, // left
        '7' => 5, // up-left
        '8' => 6, // up
        '9' => 7, // up-right
        _ => 0,
    }
}
