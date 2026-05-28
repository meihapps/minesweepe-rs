use std::collections::HashMap;
use std::time::{Duration, Instant};
use ratatui::layout::Rect;
use tachyonfx::Effect;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Render constants
// ---------------------------------------------------------------------------

/// Terminal columns occupied by one cell's content (the fullwidth char).
pub const CELL_WIDTH: u16 = 2;
/// Terminal rows occupied by one cell's content row.
pub const CELL_HEIGHT: u16 = 1;
/// Width of the border column between cells (and around the board).
pub const BORDER_COL: u16 = 1;
/// Height of the border row between cells (and around the board).
pub const BORDER_ROW: u16 = 1;

/// Returns (columns, rows) required to render a board of the given config,
/// including surrounding box-drawing borders.
/// Width  = width  * (CELL_WIDTH  + BORDER_COL) + BORDER_COL
/// Height = height * (CELL_HEIGHT + BORDER_ROW) + BORDER_ROW
pub fn board_render_size(config: &GameConfig) -> (u16, u16) {
    let w = config.width  * (CELL_WIDTH  + BORDER_COL) + BORDER_COL;
    let h = config.height * (CELL_HEIGHT + BORDER_ROW) + BORDER_ROW;
    (w, h)
}

/// Whether a board config fits within the given terminal dimensions.
/// Accounts for the 3-row status bar above the board.
pub fn fits_on_screen(config: &GameConfig, term_width: u16, term_height: u16) -> bool {
    let (w, h) = board_render_size(config);
    w <= term_width && h + 3 <= term_height
}

// ---------------------------------------------------------------------------
// Cell
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellVisibility {
    Hidden,
    Flagged,
    Question,
    Revealed,
}

impl CellVisibility {
    /// Cycles the flag state for a right-click: Hidden → Flagged → Question → Hidden.
    /// Has no effect on already-revealed cells.
    pub fn cycle_flag(self) -> Self {
        match self {
            CellVisibility::Hidden   => CellVisibility::Flagged,
            CellVisibility::Flagged  => CellVisibility::Question,
            CellVisibility::Question => CellVisibility::Hidden,
            CellVisibility::Revealed => CellVisibility::Revealed,
        }
    }

    pub fn is_hidden(self) -> bool {
        matches!(self, CellVisibility::Hidden | CellVisibility::Question)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub visibility: CellVisibility,
    pub is_mine: bool,
    /// Number of mines in the 8 neighbouring cells. Only meaningful when revealed.
    pub adjacent_mines: u8,
}

impl Cell {
    pub fn new() -> Self {
        Self {
            visibility: CellVisibility::Hidden,
            is_mine: false,
            adjacent_mines: 0,
        }
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Board {
    /// Row-major flat storage: index = y * width + x.
    pub cells: Vec<Cell>,
    pub width: u16,
    pub height: u16,
}

impl Board {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            cells: vec![Cell::new(); (width * height) as usize],
            width,
            height,
        }
    }

    pub fn idx(&self, x: u16, y: u16) -> usize {
        (y * self.width + x) as usize
    }

    pub fn get(&self, x: u16, y: u16) -> &Cell {
        &self.cells[self.idx(x, y)]
    }

    pub fn get_mut(&mut self, x: u16, y: u16) -> &mut Cell {
        let i = self.idx(x, y);
        &mut self.cells[i]
    }

    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32
    }

    /// Returns the coordinates of all valid neighbours of (x, y).
    pub fn neighbours(&self, x: u16, y: u16) -> impl Iterator<Item = (u16, u16)> {
        let (w, h) = (self.width as i32, self.height as i32);
        let (cx, cy) = (x as i32, y as i32);
        let mut result = Vec::with_capacity(8);
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                if dx == 0 && dy == 0 { continue; }
                let (nx, ny) = (cx + dx, cy + dy);
                if nx >= 0 && ny >= 0 && nx < w && ny < h {
                    result.push((nx as u16, ny as u16));
                }
            }
        }
        result.into_iter()
    }
}

// ---------------------------------------------------------------------------
// Difficulty & GameConfig
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Difficulty {
    Beginner,
    Intermediate,
    Expert,
}

impl Difficulty {
    pub fn config(self) -> GameConfig {
        match self {
            Difficulty::Beginner     => GameConfig { width: 9,  height: 9,  mine_count: 10 },
            Difficulty::Intermediate => GameConfig { width: 16, height: 16, mine_count: 40 },
            Difficulty::Expert       => GameConfig { width: 30, height: 16, mine_count: 99 },
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Beginner     => "Beginner",
            Difficulty::Intermediate => "Intermediate",
            Difficulty::Expert       => "Expert",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct GameConfig {
    pub width: u16,
    pub height: u16,
    pub mine_count: u16,
}


// ---------------------------------------------------------------------------
// Game status
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameStatus {
    /// Board exists but mines have not been placed yet (awaiting first click).
    PreGame,
    Playing,
    Won,
    /// Carries the coordinate of the cell that detonated, for highlighting.
    Lost { detonated: (u16, u16) },
}

// ---------------------------------------------------------------------------
// Game state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct GameState {
    pub difficulty: Difficulty,
    pub config: GameConfig,
    pub board: Board,
    pub status: GameStatus,
    /// Frozen on win/loss; ticking while Playing. Does not start until first click.
    pub elapsed: Duration,
    pub flags_placed: u16,
    /// True if the player requested at least one hint during this game.
    pub hint_used: bool,
}

impl GameState {
    pub fn new(difficulty: Difficulty) -> Self {
        let config = difficulty.config();
        Self {
            difficulty,
            config,
            board: Board::new(config.width, config.height),
            status: GameStatus::PreGame,
            elapsed: Duration::ZERO,
            flags_placed: 0,
            hint_used: false,
        }
    }

    pub fn remaining_mines(&self) -> i32 {
        self.config.mine_count as i32 - self.flags_placed as i32
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, GameStatus::PreGame | GameStatus::Playing)
    }
}

// ---------------------------------------------------------------------------
// Input actions
// ---------------------------------------------------------------------------

/// Game and navigation actions translated from input events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameAction {
    // --- Board actions ---
    Reveal,
    CycleFlag,
    /// Move the keyboard cursor by (dx, dy) in board/menu space.
    MoveCursor(i16, i16),
    /// Request a hint for the current board state.
    Hint,

    // --- Navigation ---
    OpenMenu,
    Quit,

    // --- UI interaction ---
    /// Click a menu item by index.
    MenuSelect(usize),
    /// Hover over a menu item by index.
    MenuHover(usize),
    /// Hover over the Back button.
    HoverBack,
    /// Mouse moved off all hotspots — clear hover state.
    ClearUiHover,
}

/// A translated input event bundling the logical action with an optional
/// board-space coordinate. Mouse events carry the cell they occurred over;
/// keyboard events carry None and operate on the current cursor position.
#[derive(Clone, Copy, Debug)]
pub struct GameEvent {
    pub action: GameAction,
    /// Board-space coordinate (x, y) for mouse events; None for keyboard.
    pub board_pos: Option<(u16, u16)>,
}

// ---------------------------------------------------------------------------
// Leaderboard & scores
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoreEntry {
    pub difficulty: Difficulty,
    pub time: Duration,
    pub achieved_at: DateTime<Utc>,
}

/// Stores top-10 entries per ranked difficulty, sorted by time ascending.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Leaderboard {
    pub entries: HashMap<Difficulty, Vec<ScoreEntry>>,
}

impl Leaderboard {
    pub const MAX_ENTRIES: usize = 10;

    pub fn get(&self, difficulty: Difficulty) -> &[ScoreEntry] {
        self.entries.get(&difficulty).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

// ---------------------------------------------------------------------------
// UI / App state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Screen {
    MainMenu { selected: usize },
    NewGameMenu { selected: usize },
    Playing,
    GameOver { won: bool, selected: usize },
    /// Which difficulty tab is currently shown.
    Leaderboard { tab: LeaderboardTab },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderboardTab {
    Beginner,
    Intermediate,
    Expert,
}

impl LeaderboardTab {
    pub fn difficulty(self) -> Difficulty {
        match self {
            LeaderboardTab::Beginner     => Difficulty::Beginner,
            LeaderboardTab::Intermediate => Difficulty::Intermediate,
            LeaderboardTab::Expert       => Difficulty::Expert,
        }
    }

    pub fn next(self) -> Self {
        match self {
            LeaderboardTab::Beginner     => LeaderboardTab::Intermediate,
            LeaderboardTab::Intermediate => LeaderboardTab::Expert,
            LeaderboardTab::Expert       => LeaderboardTab::Beginner,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            LeaderboardTab::Beginner     => LeaderboardTab::Expert,
            LeaderboardTab::Intermediate => LeaderboardTab::Beginner,
            LeaderboardTab::Expert       => LeaderboardTab::Intermediate,
        }
    }
}

/// Which UI element the mouse is currently hovering over (on non-board screens).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiHover {
    Back,
    Tab(usize),
}

/// A group of deducible cells sharing the same inference, for hint display.
#[derive(Clone, Debug)]
pub struct HintState {
    pub mine_targets: Vec<(u16, u16)>,
    pub safe_targets: Vec<(u16, u16)>,
    /// Revealed numbered cells whose constraints force the deduction.
    pub witnesses: Vec<(u16, u16)>,
    /// True when the global mine counter was needed (Tier 2).
    pub uses_global: bool,
}

pub struct App {
    pub screen: Screen,
    pub game: Option<GameState>,
    pub leaderboard: Leaderboard,
    /// Active hint, cleared on any board action or game start.
    pub hint: Option<HintState>,
    /// The currently highlighted cell in board space.
    /// Updated by both keyboard navigation and mouse movement.
    /// Only rendered when `cell_active` is true.
    pub active_cell: (u16, u16),
    /// Whether the active cell indicator should be shown.
    /// False on game start; set true by any keyboard or mouse move on the board.
    pub cell_active: bool,
    /// Whether the mouse is currently controlling the active cell.
    /// When true, mouse moves update active_cell. When false (keyboard took over),
    /// mouse moves only resume control once the mouse actually moves again.
    pub mouse_controlling: bool,
    /// Hovered UI element on menu/leaderboard screens. None when mouse is elsewhere.
    pub ui_hover: Option<UiHover>,
    /// Active effects with their target rects. Processed each frame in draw().
    pub effects: Vec<(Effect, Rect)>,
    /// Timestamp of the last rendered frame, used to compute elapsed for effects.
    pub last_frame: Instant,
    pub should_quit: bool,
}

impl App {
    pub fn new(leaderboard: Leaderboard) -> Self {
        Self {
            screen: Screen::MainMenu { selected: 0 },
            game: None,
            leaderboard,
            hint: None,
            active_cell: (0, 0),
            cell_active: false,
            mouse_controlling: true,
            ui_hover: None,
            effects: Vec::new(),
            last_frame: Instant::now(),
            should_quit: false,
        }
    }
}
