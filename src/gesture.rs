use crate::config::Gesture;
use evdev::Key;

pub const MOVE_THRESHOLD: i32 = 60;
pub const MIN_TOTAL_MOVE: i32 = 20;

const DIRECTIONS: [char; 8] = ['6', '3', '2', '1', '4', '7', '8', '9'];

#[derive(Debug)]
pub struct GesturePattern {
    exact: String,
    smoothed: String,
    cardinal: String,
}

impl GesturePattern {
    pub fn recorded(self) -> String {
        self.smoothed
    }

    pub fn pick(&self, gestures: &[Gesture], app_id: Option<&str>) -> Option<Vec<Key>> {
        pick(gestures, &self.exact, app_id)
            .or_else(|| {
                if self.smoothed != self.exact {
                    pick(gestures, &self.smoothed, app_id)
                } else {
                    None
                }
            })
            .or_else(|| pick(gestures, &self.cardinal, app_id))
    }
}

struct DirectionRun {
    sector: usize,
    samples: usize,
}

#[derive(Default)]
pub struct GestureState {
    pub active: bool,
    pub bypass: bool,
    pub overlay_started: bool,
    pub total_dx: i32,
    pub total_dy: i32,
    acc_dx: i32,
    acc_dy: i32,
    cardinal: String,
    runs: Vec<DirectionRun>,
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
        self.cardinal.clear();
        self.runs.clear();
        self.app_id = app_id;
    }

    pub fn feed(&mut self, dx: i32, dy: i32) {
        self.total_dx += dx;
        self.total_dy += dy;
        self.acc_dx += dx;
        self.acc_dy += dy;
        let dx = self.acc_dx as f32;
        let dy = self.acc_dy as f32;
        if dx * dx + dy * dy < (MOVE_THRESHOLD * MOVE_THRESHOLD) as f32 {
            return;
        }

        let cardinal = classify_cardinal(dx, dy);
        if !self.cardinal.ends_with(cardinal) {
            self.cardinal.push(cardinal);
        }

        let angle = dy.atan2(dx).to_degrees();
        let sector = ((angle + 22.5).rem_euclid(360.0) / 45.0) as usize % DIRECTIONS.len();
        if let Some(last) = self.runs.last_mut() {
            // Give the current direction a little extra room near a sector boundary.
            let center = last.sector as f32 * 45.0;
            let offset = (angle - center + 180.0).rem_euclid(360.0) - 180.0;
            if sector == last.sector || offset.abs() <= 30.0 {
                last.samples += 1;
            } else {
                self.runs.push(DirectionRun { sector, samples: 1 });
            }
        } else {
            self.runs.push(DirectionRun { sector, samples: 1 });
        }

        self.acc_dx = 0;
        self.acc_dy = 0;
    }

    pub fn finish(&mut self) -> Option<GesturePattern> {
        self.active = false;
        if self.runs.is_empty() || !self.moved_enough() {
            return None;
        }

        let exact: String = self.runs.iter().map(|run| DIRECTIONS[run.sector]).collect();
        let smoothed = self
            .runs
            .iter()
            .enumerate()
            .filter_map(|(i, run)| {
                // A rounded 90° corner can briefly cross the sector between its legs.
                if run.samples == 1 && i > 0 && i + 1 < self.runs.len() {
                    let before = self.runs[i - 1].sector;
                    let after = self.runs[i + 1].sector;
                    if before != after
                        && sector_distance(before, run.sector) == 1
                        && sector_distance(run.sector, after) == 1
                    {
                        return None;
                    }
                }
                Some(DIRECTIONS[run.sector])
            })
            .collect();

        let mut cardinal = std::mem::take(&mut self.cardinal);
        if self.runs.len() == 1 && cardinal.len() > 1 {
            // A straight stroke can wobble across the boundary between two axes.
            cardinal.clear();
            cardinal.push(classify_cardinal(self.total_dx as f32, self.total_dy as f32));
        }
        Some(GesturePattern { exact, smoothed, cardinal })
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

fn pick(gestures: &[Gesture], pattern: &str, app_id: Option<&str>) -> Option<Vec<Key>> {
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

fn classify_cardinal(dx: f32, dy: f32) -> char {
    if dx.abs() >= dy.abs() {
        if dx >= 0.0 { '6' } else { '4' }
    } else {
        if dy >= 0.0 { '2' } else { '8' }
    }
}

fn sector_distance(a: usize, b: usize) -> usize {
    let distance = a.abs_diff(b);
    distance.min(DIRECTIONS.len() - distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(points: &[(i32, i32)]) -> GesturePattern {
        let mut state = GestureState::new();
        state.start(None);
        for &(dx, dy) in points {
            state.feed(dx, dy);
        }
        state.finish().unwrap()
    }

    fn binding(pattern: &str, apps: &[&str], key: Key) -> Gesture {
        Gesture {
            pattern: pattern.into(),
            keys: vec![key],
            apps: apps.iter().map(|app| (*app).into()).collect(),
            label: None,
            enabled: true,
        }
    }

    #[test]
    fn records_all_diagonal_directions() {
        for (dx, dy, expected) in [
            (50, -50, "9"),
            (50, 50, "3"),
            (-50, -50, "7"),
            (-50, 50, "1"),
        ] {
            assert_eq!(draw(&[(dx, dy)]).recorded(), expected);
        }
    }

    #[test]
    fn diagonal_binding_precedes_cardinal_fallback_for_the_current_app() {
        let pattern = draw(&[(65, -40)]);
        let mut gestures = [
            binding("9", &["chrome"], Key::KEY_T),
            binding("6", &[], Key::KEY_RIGHT),
        ];
        assert_eq!(
            pattern.pick(&gestures, Some("Google-Chrome")),
            Some(vec![Key::KEY_T])
        );
        assert_eq!(
            pattern.pick(&gestures, Some("nautilus")),
            Some(vec![Key::KEY_RIGHT])
        );
        gestures[0].enabled = false;
        assert_eq!(
            pattern.pick(&gestures, Some("chrome")),
            Some(vec![Key::KEY_RIGHT])
        );
    }

    #[test]
    fn smooths_rounded_corners_without_overriding_an_exact_binding() {
        let pattern = draw(&[(50, -50), (65, 0), (50, 50)]);
        let rounded = binding("93", &[], Key::KEY_T);
        let exact = binding("963", &[], Key::KEY_W);
        assert_eq!(
            pattern.pick(&[rounded.clone(), exact], None),
            Some(vec![Key::KEY_W])
        );
        assert_eq!(pattern.pick(&[rounded], None), Some(vec![Key::KEY_T]));
        assert_eq!(pattern.recorded(), "93");
    }

    #[test]
    fn keeps_the_direction_when_motion_wobbles_near_a_sector_boundary() {
        assert_eq!(draw(&[(65, -29), (65, -23), (65, -30)]).recorded(), "9");
    }

    #[test]
    fn projects_a_wobbly_diagonal_onto_its_dominant_axis() {
        let pattern = draw(&[(60, -54), (52, -57), (60, -50)]);
        assert_eq!(
            pattern.pick(&[binding("6", &[], Key::KEY_RIGHT)], None),
            Some(vec![Key::KEY_RIGHT])
        );
    }

    #[test]
    fn retains_existing_cardinal_turns() {
        let pattern = draw(&[(15, 65), (65, 25)]);
        assert_eq!(
            pattern.pick(&[binding("26", &[], Key::KEY_W)], None),
            Some(vec![Key::KEY_W])
        );
    }
}
