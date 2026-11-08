//! Vein miner: while the vein key is held, breaking a block also breaks the
//! blocks of the same kind connected to it (singleplayer). Options > Mods >
//! Vein Miner turns it on or off and sets how many blocks at most.
//!
//! Written without a Rust toolchain: fix whatever `cargo check` reports.

#![allow(dead_code)]

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};

use super::Game;

/// The mod's id (its settings folder), name and version.
pub const ID: &str = "vein_miner";
pub const NAME: &str = "Vein Miner";
pub const VERSION: &str = "0.2.0";
pub const DESCRIPTION: &str =
    "Hold the vein key (Shift or the ` key) and break a block to break the connected blocks of the same kind.";

/// Fewest and most blocks the setting allows, and the quick choices.
pub const MIN_COUNT: usize = 5;
pub const MAX_COUNT: usize = 128;
pub const PRESETS: [usize; 5] = [5, 16, 32, 64, 128];

/// The settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    /// Most blocks broken per block, MIN_COUNT to MAX_COUNT.
    pub count: usize,
    /// Also join blocks that touch only at an edge or a corner.
    pub diagonals: bool,
}

impl Settings {
    /// From the stored values; on, 32 blocks, no diagonals for the missing ones.
    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let get = |key: &str| get(key).map(|v| v.trim().to_owned());
        Self {
            enabled: get("enabled").as_deref() != Some("false"),
            count: get("count")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(32)
                .clamp(MIN_COUNT, MAX_COUNT),
            diagonals: get("diagonals").as_deref() == Some("true"),
        }
    }
    pub fn labels(&self) -> Vec<String> {
        vec![
            format!("Vein miner: {}", if self.enabled { "ON" } else { "OFF" }),
            format!("Max blocks: {}", self.count),
            "Max blocks -10".to_owned(),
            "Max blocks -1".to_owned(),
            "Max blocks +1".to_owned(),
            "Max blocks +10".to_owned(),
            format!("Diagonals: {}", if self.diagonals { "ON" } else { "OFF" }),
        ]
    }
    pub fn click(&mut self, i: usize) {
        match i {
            0 => self.enabled = !self.enabled,
            // The next quick choice above the current count, or the first.
            1 => {
                self.count = PRESETS
                    .iter()
                    .copied()
                    .find(|p| *p > self.count)
                    .unwrap_or(PRESETS[0]);
            }
            2 => self.count = self.count.saturating_sub(10).max(MIN_COUNT),
            3 => self.count = self.count.saturating_sub(1).max(MIN_COUNT),
            4 => self.count = (self.count + 1).min(MAX_COUNT),
            5 => self.count = (self.count + 10).min(MAX_COUNT),
            6 => self.diagonals = !self.diagonals,
            _ => {}
        }
    }
    pub fn stored(&self) -> [(&'static str, String); 3] {
        [
            ("enabled", if self.enabled { "true" } else { "false" }.to_owned()),
            ("count", self.count.to_string()),
            ("diagonals", if self.diagonals { "true" } else { "false" }.to_owned()),
        ]
    }
}

thread_local! {
    /// The settings, read once from the files, then kept.
    static SETTINGS: std::cell::Cell<Option<Settings>> = const { std::cell::Cell::new(None) };
}

/// The settings, read once through `get` (one file per key).
pub(super) fn settings(get: impl Fn(&str) -> Option<String>) -> Settings {
    SETTINGS.with(|s| {
        let found = s.get().unwrap_or_else(|| Settings::read(get));
        s.set(Some(found));
        found
    })
}

pub(super) fn set_settings(settings: Settings) {
    SETTINGS.with(|s| s.set(Some(settings)));
}

/// Reset profile-scoped state when Android returns from its launcher.
pub(super) fn reset() {
    SETTINGS.set(None);
    HELD.store(false, Ordering::Relaxed);
}

/// The mod's row in Options > Mods.
pub(super) fn list(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    mods.push(crate::game::native_hooks::NativeMod {
        id: ID.into(),
        title: format!("{NAME} {VERSION}"),
        description: DESCRIPTION.into(),
        keys: Vec::new(),
    });
}

/// The mod's settings page.
pub(super) fn settings_page(game: &mut Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id != ID {
        return;
    }
    let game = &*game;
    buttons.extend(settings(|key| game.native_setting(ID, key)).labels());
}

/// A click on the settings page: change the setting and keep it.
pub(super) fn settings_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != ID {
        return;
    }
    let mut s = settings(|key| game.native_setting(ID, key));
    s.click(button);
    set_settings(s);
    for (key, value) in s.stored() {
        game.native_set_setting(ID, key, &value);
    }
}

/// Vein key held (Shift or the grave-accent key).
static HELD: AtomicBool = AtomicBool::new(false);
/// True while the extra blocks are being broken, so they do not start
/// another expansion themselves.
static BREAKING: AtomicBool = AtomicBool::new(false);

/// From `native_key`: names as the game writes them in options.txt.
pub(super) fn on_key(key: &str, pressed: bool) {
    match key {
        "key.keyboard.left.shift" | "key.keyboard.right.shift" | "key.keyboard.grave.accent" => {
            HELD.store(pressed, Ordering::Relaxed);
        }
        _ => {}
    }
}

/// From `native_destroy_block`, before vanilla breaks the block at `pos`.
/// The block itself is left to vanilla; the connected ones are broken here.
pub(super) fn expand(game: &mut Game<'_>, pos: [i32; 3]) {
    if !HELD.load(Ordering::Relaxed) || BREAKING.load(Ordering::Relaxed) {
        return;
    }
    let s = settings(|key| game.native_setting(ID, key));
    if !s.enabled {
        return;
    }
    let target = game.world.block(pos[0], pos[1], pos[2]);
    let found = find_vein(
        pos,
        |p| game.world.block(p[0], p[1], p[2]) == target,
        s.count,
        s.diagonals,
    );
    BREAKING.store(true, Ordering::Relaxed);
    for p in found {
        let _ = game.break_block(p);
    }
    BREAKING.store(false, Ordering::Relaxed);
}

/// Blocks connected to `start` for which `same` is true, nearest first, at
/// most `limit`. `start` itself is not in the result.
pub(super) fn find_vein(
    start: [i32; 3],
    same: impl Fn([i32; 3]) -> bool,
    limit: usize,
    diagonals: bool,
) -> Vec<[i32; 3]> {
    let mut offsets = Vec::new();
    for dx in -1i32..=1 {
        for dy in -1i32..=1 {
            for dz in -1i32..=1 {
                let steps = dx.abs() + dy.abs() + dz.abs();
                if steps == 0 || (!diagonals && steps > 1) {
                    continue;
                }
                offsets.push([dx, dy, dz]);
            }
        }
    }
    let mut seen: HashSet<[i32; 3]> = HashSet::new();
    seen.insert(start);
    let mut queue = VecDeque::new();
    queue.push_back(start);
    let mut found = Vec::new();
    while let Some(at) = queue.pop_front() {
        for o in &offsets {
            if found.len() >= limit {
                return found;
            }
            let next = [at[0] + o[0], at[1] + o[1], at[2] + o[2]];
            if !seen.insert(next) || !same(next) {
                continue;
            }
            found.push(next);
            queue.push_back(next);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_limit() {
        let line = |p: [i32; 3]| p[1] == 0 && p[2] == 0 && (1..=5).contains(&p[0]);
        assert_eq!(find_vein([0, 0, 0], line, 99, false).len(), 5);
        assert_eq!(find_vein([0, 0, 0], line, 3, false).len(), 3);
    }

    #[test]
    fn unconnected_ignored() {
        let b = |p: [i32; 3]| p == [1, 0, 0] || p == [5, 0, 0];
        assert_eq!(find_vein([0, 0, 0], b, 99, false), vec![[1, 0, 0]]);
    }

    #[test]
    fn diagonals_optional() {
        let b = |p: [i32; 3]| p == [1, 1, 0];
        assert!(find_vein([0, 0, 0], b, 99, false).is_empty());
        assert_eq!(find_vein([0, 0, 0], b, 99, true).len(), 1);
    }

    #[test]
    fn settings_defaults_clamp_and_round_trip() {
        let s = Settings::read(|_| None);
        assert_eq!((s.enabled, s.count, s.diagonals), (true, 32, false));
        let big = Settings::read(|k| (k == "count").then(|| "999".to_owned()));
        assert_eq!(big.count, MAX_COUNT);
        let small = Settings::read(|k| (k == "count").then(|| "1".to_owned()));
        assert_eq!(small.count, MIN_COUNT);
        let mut s = Settings::read(|_| None);
        s.click(0);
        s.click(5);
        s.click(6);
        let st = s.stored();
        let back = Settings::read(|k| st.iter().find(|f| f.0 == k).map(|f| f.1.clone()));
        assert_eq!(back, s);
        assert_eq!(s.count, 42);
    }

    #[test]
    fn count_buttons_stay_in_range() {
        let mut s = Settings::read(|_| None);
        for _ in 0..30 {
            s.click(2);
        }
        assert_eq!(s.count, MIN_COUNT);
        for _ in 0..30 {
            s.click(5);
        }
        assert_eq!(s.count, MAX_COUNT);
        s.click(1);
        assert_eq!(s.count, PRESETS[0]);
    }
}
