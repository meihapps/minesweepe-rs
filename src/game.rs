use rand::seq::SliceRandom;
use rand::thread_rng;

use crate::solver::{self, SolveResult};
use crate::types::{Board, CellVisibility, GameConfig, GameState, GameStatus};

// ---------------------------------------------------------------------------
// Mine placement
// ---------------------------------------------------------------------------

/// Places mines on the board after the first click, guaranteeing:
///   - A safe zone around the first click (3×3 when density allows, else 1×1).
///   - The resulting puzzle is solvable without guessing.
///
/// Keeps rerolling until a solvable layout is found.
pub fn place_mines(board: &mut Board, config: GameConfig, first_click: (u16, u16)) {
    generate_mines(board, config, first_click, || false);
}

/// Shared mine generator behind `place_mines` and the pregen worker.
/// Rerolls layouts until one is solvable, checking `cancelled` before each
/// attempt. Returns `false` if cancelled before a solvable layout was found.
pub(crate) fn generate_mines(
    board: &mut Board,
    config: GameConfig,
    first_click: (u16, u16),
    cancelled: impl Fn() -> bool,
) -> bool {
    let (fx, fy) = first_click;
    let total = (config.width * config.height) as usize;
    let mine_count = config.mine_count as usize;
    debug_assert!(mine_count < total, "no room for a safe first click");

    let neighbourhood: Vec<usize> = std::iter::once((fx, fy))
        .chain(board.neighbours(fx, fy))
        .map(|(nx, ny)| board.idx(nx, ny))
        .collect();

    // Fall back to a 1×1 safe zone when the 3×3 doesn't leave room for every mine.
    let excluded: Vec<usize> = if total - neighbourhood.len() >= mine_count {
        neighbourhood
    } else {
        vec![board.idx(fx, fy)]
    };

    let mut candidates: Vec<usize> = (0..total).filter(|i| !excluded.contains(i)).collect();
    let mut rng = thread_rng();

    loop {
        if cancelled() {
            return false;
        }

        // Clear previous layout.
        for cell in &mut board.cells {
            cell.is_mine = false;
            cell.adjacent_mines = 0;
        }

        let (mines, _) = candidates.partial_shuffle(&mut rng, mine_count);
        for &idx in mines.iter() {
            board.cells[idx].is_mine = true;
        }
        compute_adjacency(board);

        if solver::solve(board, first_click) == SolveResult::Solved {
            return true;
        }
    }
}

pub(crate) fn compute_adjacency(board: &mut Board) {
    for y in 0..board.height {
        for x in 0..board.width {
            if board.get(x, y).is_mine {
                continue;
            }
            let count = u8::try_from(
                board
                    .neighbours(x, y)
                    .filter(|&(nx, ny)| board.get(nx, ny).is_mine)
                    .count(),
            )
            .expect("at most 8 neighbours");
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
        place_mines(&mut state.board, state.config, (x, y));
        state.status = GameStatus::Playing;
    }

    let cell = state.board.get(x, y);
    if cell.is_mine {
        state.board.get_mut(x, y).visibility = CellVisibility::Revealed;
        state.status = GameStatus::Lost { detonated: (x, y) };
        reveal_all_mines(&mut state.board);
        return vec![]; // detonation effect triggered separately
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
    for cell in &mut board.cells {
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
/// Updates the `flags_placed` counter accordingly.
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
        (false, true) => state.flags_placed += 1,
        (true, false) => state.flags_placed = state.flags_placed.saturating_sub(1),
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
    let all_clear = state
        .board
        .cells
        .iter()
        .all(|c| c.is_mine || c.visibility == CellVisibility::Revealed);

    if all_clear {
        // Auto-flag any unflagged mines.
        let mut auto_flags = 0u16;
        for cell in &mut state.board.cells {
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
#[must_use]
pub fn move_cursor(cursor: (u16, u16), dx: i16, dy: i16, config: &GameConfig) -> (u16, u16) {
    let new_x = cursor.0.saturating_add_signed(dx).min(config.width - 1);
    let new_y = cursor.1.saturating_add_signed(dy).min(config.height - 1);
    (new_x, new_y)
}

// ---------------------------------------------------------------------------
// Effect helpers
// ---------------------------------------------------------------------------

use tachyonfx::{fx, Effect, Interpolation};

/// Effect played when cells are revealed: cells coalesce (materialise from
/// random scatter) over the cell area. Duration scales with cell count —
/// a single cell is fast, a large flood fill gets more time to read.
#[must_use]
pub fn reveal_effect(cell_count: usize) -> Effect {
    const MIN_MS: u32 = 120;
    const MAX_MS: u32 = 550;
    const SCALE_AT: u32 = 40; // cell count at which duration reaches MAX_MS
    let n = u32::try_from(cell_count).unwrap_or(u32::MAX).min(SCALE_AT);
    let duration = MIN_MS + (MAX_MS - MIN_MS) * n / SCALE_AT;
    fx::coalesce((duration, Interpolation::QuadOut))
}

/// Effect played on mine detonation: red flash that fades out over the board.
/// `fx::fade_from(fg`, bg, timer) starts from the given colours and fades back
/// to the buffer's existing colours — a pure colour overlay, no content removed.
#[must_use]
pub fn detonate_effect() -> Effect {
    // Use RGB tuples — accepted by Into<Color> on both ratatui and ratatui_core.
    // Slam to white-on-red, then fade back to whatever the board was rendering.
    fx::sequence(&[
        fx::fade_to(
            (255u8, 255u8, 255u8),
            (180u8, 20u8, 20u8),
            (60, Interpolation::Linear),
        ),
        fx::fade_from(
            (255u8, 255u8, 255u8),
            (180u8, 20u8, 20u8),
            (500, Interpolation::QuadOut),
        ),
    ])
}
