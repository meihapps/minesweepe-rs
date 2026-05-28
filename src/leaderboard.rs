use std::time::Duration;
use chrono::Utc;

use crate::types::{Difficulty, Leaderboard, ScoreEntry};

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

const SAVE_FILE: &str = "minesweepe-rs_leaderboard.json";

/// Returns the path to the leaderboard save file, using the platform data dir.
fn save_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join(SAVE_FILE))
}

/// Loads the leaderboard from disk. Returns an empty leaderboard on any error
/// (missing file, corrupt JSON, etc.) rather than propagating — a missing
/// leaderboard is not a fatal condition.
pub fn load() -> Leaderboard {
    let path = match save_path() {
        Some(p) => p,
        None => return Leaderboard::default(),
    };

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return Leaderboard::default(),
    };

    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Saves the leaderboard to disk. Errors are silently ignored — a failed
/// save shouldn't crash the game.
pub fn save(leaderboard: &Leaderboard) {
    let path = match save_path() {
        Some(p) => p,
        None => return,
    };

    // Ensure the parent directory exists.
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(json) = serde_json::to_vec_pretty(leaderboard) {
        let _ = std::fs::write(path, json);
    }
}

// ---------------------------------------------------------------------------
// Score insertion
// ---------------------------------------------------------------------------

/// Submits a time for a ranked difficulty. Inserts into the leaderboard if it
/// qualifies for the top 10. Returns true if the entry was inserted (i.e. it
/// made the leaderboard).
pub fn submit(leaderboard: &mut Leaderboard, difficulty: Difficulty, time: Duration) -> bool {
    let entry = ScoreEntry {
        difficulty,
        time,
        achieved_at: Utc::now(),
    };

    let entries = leaderboard.entries.entry(difficulty).or_default();

    // Insert in sorted position (ascending by time).
    let pos = entries.partition_point(|e| e.time <= time);
    entries.insert(pos, entry);

    // Trim to top 10.
    entries.truncate(Leaderboard::MAX_ENTRIES);

    // The entry made the board if it's still present after truncation —
    // i.e. it was inserted within the first MAX_ENTRIES slots.
    pos < Leaderboard::MAX_ENTRIES
}

/// Returns whether a given time would qualify for the leaderboard.
pub fn qualifies(leaderboard: &Leaderboard, difficulty: Difficulty, time: Duration) -> bool {
    let entries = leaderboard.get(difficulty);
    entries.len() < Leaderboard::MAX_ENTRIES || entries.last().map_or(true, |e| time < e.time)
}
