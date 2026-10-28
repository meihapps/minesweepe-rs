use chrono::Utc;
use std::time::Duration;

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
#[must_use]
pub fn load() -> Leaderboard {
    let Some(path) = save_path() else {
        return Leaderboard::default();
    };

    let Ok(bytes) = std::fs::read(&path) else {
        return Leaderboard::default();
    };

    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Saves the leaderboard to disk. Errors are silently ignored — a failed
/// save shouldn't crash the game.
pub fn save(leaderboard: &Leaderboard) {
    let Some(path) = save_path() else {
        return;
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
#[must_use]
pub fn qualifies(leaderboard: &Leaderboard, difficulty: Difficulty, time: Duration) -> bool {
    let entries = leaderboard.get(difficulty);
    entries.len() < Leaderboard::MAX_ENTRIES || entries.last().is_none_or(|e| time < e.time)
}
