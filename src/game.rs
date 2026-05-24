use rand::seq::SliceRandom;
use rand::thread_rng;

use crate::types::{
    Board, CellVisibility, GameConfig, GameState, GameStatus,
};

// ---------------------------------------------------------------------------
// Mine placement
// ---------------------------------------------------------------------------

/// Places mines on the board after the first click, guaranteeing a safe zone
/// around the clicked cell. The safe zone is 3×3 if the config allows it
/// (mine_count <= width*height - 10), otherwise just the clicked cell.
pub fn place_mines(board: &mut Board, config: &GameConfig, first_click: (u16, u16)) {
    let (fx, fy) = first_click;
    let total = (config.width * config.height) as usize;

    // Build the set of excluded indices.
    let excluded: std::collections::HashSet<usize> = if config.allows_3x3_safe_zone() {
        (-1i32..=1)
            .flat_map(|dy| (-1i32..=1).map(move |dx| (dx, dy)))
            .filter_map(|(dx, dy)| {
                let nx = fx as i32 + dx;
                let ny = fy as i32 + dy;
                if board.in_bounds(nx, ny) {
                    Some(board.idx(nx as u16, ny as u16))
                } else {
                    None
                }
            })
            .collect()
    } else {
        std::iter::once(board.idx(fx, fy)).collect()
    };

    // Collect all candidate indices.
    let mut candidates: Vec<usize> = (0..total).filter(|i| !excluded.contains(i)).collect();
    candidates.shuffle(&mut thread_rng());

    // Place mines.
    for &idx in candidates.iter().take(config.mine_count as usize) {
        board.cells[idx].is_mine = true;
    }

    // Compute adjacent_mines for every cell.
    compute_adjacency(board);
}

fn compute_adjacency(board: &mut Board) {
    for y in 0..board.height {
        for x in 0..board.width {
            if board.get(x, y).is_mine {
                continue;
            }
            let count = board
                .neighbours(x, y)
                .filter(|&(nx, ny)| board.get(nx, ny).is_mine)
                .count() as u8;
            board.get_mut(x, y).adjacent_mines = count;
        }
    }
}

// ---------------------------------------------------------------------------
// Reveal
// ---------------------------------------------------------------------------

/// Attempts to reveal the cell at (x, y).
/// Returns the list of newly revealed cell coordinates (empty if nothing changed or mine).
/// On mine: returns empty (detonation handled separately via game status).
pub fn reveal(state: &mut GameState, x: u16, y: u16) -> Vec<(u16, u16)> {
    match state.board.get(x, y).visibility {
        CellVisibility::Flagged | CellVisibility::Revealed => return vec![],
        _ => {}
    }

    if state.status == GameStatus::PreGame {
        place_mines(&mut state.board, &state.config, (x, y));
        state.status = GameStatus::Playing;
    }

    let cell = state.board.get(x, y);
    if cell.is_mine {
        state.board.get_mut(x, y).visibility = CellVisibility::Revealed;
        state.status = GameStatus::Lost { detonated: (x, y) };
        reveal_all_mines(&mut state.board);
        return vec![];  // detonation effect triggered separately
    }

    let newly_revealed = if cell.adjacent_mines == 0 {
        flood_fill(&mut state.board, x, y)
    } else {
        state.board.get_mut(x, y).visibility = CellVisibility::Revealed;
        vec![(x, y)]
    };

    check_win(state);
    newly_revealed
}

/// Flood-fill reveal for cells with zero adjacent mines.
/// Returns the list of newly revealed cell coordinates.
fn flood_fill(board: &mut Board, start_x: u16, start_y: u16) -> Vec<(u16, u16)> {
    let mut stack = vec![(start_x, start_y)];
    let mut revealed = Vec::new();

    while let Some((x, y)) = stack.pop() {
        let cell = board.get(x, y);
        if cell.visibility == CellVisibility::Revealed {
            continue;
        }
        if cell.visibility == CellVisibility::Flagged {
            continue;
        }

        board.get_mut(x, y).visibility = CellVisibility::Revealed;
        revealed.push((x, y));

        if board.get(x, y).adjacent_mines == 0 {
            for (nx, ny) in board.neighbours(x, y) {
                if board.get(nx, ny).visibility != CellVisibility::Revealed {
                    stack.push((nx, ny));
                }
            }
        }
    }
    revealed
}

/// On loss, reveal all mines (except correctly flagged ones).
fn reveal_all_mines(board: &mut Board) {
    for cell in board.cells.iter_mut() {
        if cell.is_mine && cell.visibility != CellVisibility::Flagged {
            cell.visibility = CellVisibility::Revealed;
        }
    }
}


/// Reveals all hidden (non-flagged) neighbours of a revealed cell, no safety check.
pub fn chord_reveal(state: &mut GameState, x: u16, y: u16) -> Vec<(u16, u16)> {
    let cell = state.board.get(x, y);
    if cell.visibility != CellVisibility::Revealed {
        return vec![];
    }

    let to_reveal: Vec<(u16, u16)> = state
        .board
        .neighbours(x, y)
        .filter(|&(nx, ny)| state.board.get(nx, ny).visibility.is_hidden())
        .collect();

    let mut all_revealed = Vec::new();
    for (nx, ny) in to_reveal {
        let newly = reveal(state, nx, ny);
        all_revealed.extend(newly);
        if matches!(state.status, GameStatus::Lost { .. }) {
            return all_revealed;
        }
    }
    all_revealed
}

/// Flags all hidden neighbours of a revealed cell, no safety check.
pub fn chord_flag(state: &mut GameState, x: u16, y: u16) {
    let cell = state.board.get(x, y);
    if cell.visibility != CellVisibility::Revealed {
        return;
    }

    let to_flag: Vec<(u16, u16)> = state
        .board
        .neighbours(x, y)
        .filter(|&(nx, ny)| state.board.get(nx, ny).visibility.is_hidden())
        .collect();

    for (nx, ny) in to_flag {
        state.board.get_mut(nx, ny).visibility = CellVisibility::Flagged;
        state.flags_placed += 1;
    }
}

// ---------------------------------------------------------------------------
// Flag cycling
// ---------------------------------------------------------------------------

/// Cycles the flag state of a non-revealed cell.
/// Hidden → Flagged → Question → Hidden.
/// Updates the flags_placed counter accordingly.
pub fn cycle_flag(state: &mut GameState, x: u16, y: u16) -> bool {
    let cell = state.board.get(x, y);
    if cell.visibility == CellVisibility::Revealed {
        return false;
    }

    let was_flagged = cell.visibility == CellVisibility::Flagged;
    let new_visibility = cell.visibility.cycle_flag();
    let is_flagged = new_visibility == CellVisibility::Flagged;

    state.board.get_mut(x, y).visibility = new_visibility;

    match (was_flagged, is_flagged) {
        (false, true)  => state.flags_placed += 1,
        (true,  false) => state.flags_placed  = state.flags_placed.saturating_sub(1),
        _ => {}
    }

    true
}

// ---------------------------------------------------------------------------
// Win detection
// ---------------------------------------------------------------------------

/// Checks whether all non-mine cells are revealed. If so, flags all remaining
/// mines and transitions to Won.
fn check_win(state: &mut GameState) {
    let all_clear = state.board.cells.iter().all(|c| {
        c.is_mine || c.visibility == CellVisibility::Revealed
    });

    if all_clear {
        // Auto-flag any unflagged mines.
        let mut auto_flags = 0u16;
        for cell in state.board.cells.iter_mut() {
            if cell.is_mine && cell.visibility != CellVisibility::Flagged {
                cell.visibility = CellVisibility::Flagged;
                auto_flags += 1;
            }
        }
        state.flags_placed += auto_flags;
        state.status = GameStatus::Won;
    }
}

// ---------------------------------------------------------------------------
// Cursor clamping
// ---------------------------------------------------------------------------

/// Moves the cursor by (dx, dy) in board space, clamping to board bounds.
pub fn move_cursor(cursor: (u16, u16), dx: i16, dy: i16, config: &GameConfig) -> (u16, u16) {
    let new_x = (cursor.0 as i16 + dx).clamp(0, config.width as i16 - 1) as u16;
    let new_y = (cursor.1 as i16 + dy).clamp(0, config.height as i16 - 1) as u16;
    (new_x, new_y)
}

// ---------------------------------------------------------------------------
// Effect helpers
// ---------------------------------------------------------------------------

use tachyonfx::{fx, Effect, Interpolation};

/// Effect played when cells are revealed: cells coalesce (materialise from
/// random scatter) over the cell area. Duration scales with cell count —
/// a single cell is fast, a large flood fill gets more time to read.
pub fn reveal_effect(cell_count: usize) -> Effect {
    const MIN_MS: u32 = 120;
    const MAX_MS: u32 = 550;
    const SCALE_AT: usize = 40; // cell count at which duration reaches MAX_MS
    let t = (cell_count as f32 / SCALE_AT as f32).min(1.0);
    let duration = MIN_MS + ((MAX_MS - MIN_MS) as f32 * t) as u32;
    fx::coalesce((duration, Interpolation::QuadOut))
}

/// Effect played on mine detonation: red flash that fades out over the board.
/// fx::fade_from(fg, bg, timer) starts from the given colours and fades back
/// to the buffer's existing colours — a pure colour overlay, no content removed.
pub fn detonate_effect() -> Effect {
    // Use RGB tuples — accepted by Into<Color> on both ratatui and ratatui_core.
    // Slam to white-on-red, then fade back to whatever the board was rendering.
    fx::sequence(&[
        fx::fade_to((255u8, 255u8, 255u8), (180u8, 20u8, 20u8), (60, Interpolation::Linear)),
        fx::fade_from((255u8, 255u8, 255u8), (180u8, 20u8, 20u8), (500, Interpolation::QuadOut)),
    ])
}
