//! Vein miner: while the vein key is held, breaking a block also breaks the
//! blocks of the same kind connected to it (singleplayer).
//!
//! UNVERIFIED against the game source (written without a Rust toolchain):
//!   - `game.world.block(x, y, z)` must be comparable with `==`
//!   - `game.break_block(pos)` must be callable from this module
//! Fix whatever `cargo check` reports.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::Game;

/// Vein key held (Shift or the grave-accent key).
static HELD: AtomicBool = AtomicBool::new(false);
/// True while the extra blocks are being broken, so they do not start
/// another expansion themselves.
static BREAKING: AtomicBool = AtomicBool::new(false);
/// Vein mining without any key (for a phone with no keyboard). Off.
static ALWAYS: AtomicBool = AtomicBool::new(false);
/// Most extra blocks broken per block.
static LIMIT: AtomicUsize = AtomicUsize::new(32);
/// Also join blocks that touch only at an edge or a corner.
static DIAGONALS: AtomicBool = AtomicBool::new(false);

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
    let active = ALWAYS.load(Ordering::Relaxed) || HELD.load(Ordering::Relaxed);
    if !active || BREAKING.load(Ordering::Relaxed) {
        return;
    }
    let target = game.world.block(pos[0], pos[1], pos[2]);
    let found = find_vein(
        pos,
        |p| game.world.block(p[0], p[1], p[2]) == target,
        LIMIT.load(Ordering::Relaxed),
        DIAGONALS.load(Ordering::Relaxed),
    );
    BREAKING.store(true, Ordering::Relaxed);
    for p in found {
        let _ = game.break_block(p);
    }
    BREAKING.store(false, Ordering::Relaxed);
}

/// Blocks connected to `start` for which `same` is true, nearest first, at
/// most `limit`. `start` itself is not in the result.
fn find_vein(
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
    use super::find_vein;

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
}
