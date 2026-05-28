use std::time::{Duration, Instant};

use crossterm::event::{self, Event};

use minesweepe_rs::game;
use minesweepe_rs::game::{detonate_effect, reveal_effect};
use minesweepe_rs::leaderboard;
use minesweepe_rs::pregen::PregenHandle;
use minesweepe_rs::solver;
use minesweepe_rs::tui;
use minesweepe_rs::tui::{GAME_OVER_ITEMS, MAIN_MENU_ITEMS, NEW_GAME_ITEMS};
use minesweepe_rs::types::{
    App, CellVisibility, Difficulty, GameAction, GameState, GameStatus, HintState, LeaderboardTab,
    Screen, UiHover, BORDER_COL, BORDER_ROW, CELL_HEIGHT, CELL_WIDTH,
};
use ratatui::layout::Rect;

const TICK_RATE: Duration = Duration::from_millis(250);
const EFFECT_TICK: Duration = Duration::from_millis(16); // ~60fps when effects running

fn main() -> anyhow::Result<()> {
    let lb = leaderboard::load();
    let mut app = App::new(lb);
    let mut terminal = tui::init()?;
    let pregen = PregenHandle::new();

    let result = run(&mut terminal, &mut app, &pregen);

    tui::restore(&mut terminal)?;
    leaderboard::save(&app.leaderboard);

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }
    Ok(())
}

fn run(terminal: &mut tui::Tui, app: &mut App, pregen: &PregenHandle) -> anyhow::Result<()> {
    let mut last_tick = Instant::now();

    loop {
        tui::draw(terminal, app)?;

        // Use faster tick while effects are running for smooth animation.
        let tick = if !app.effects.is_empty() {
            EFFECT_TICK
        } else {
            TICK_RATE
        };
        let timeout = tick.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            let raw = event::read()?;
            handle_event(app, &raw, pregen);
        }

        if last_tick.elapsed() >= TICK_RATE {
            tick_timer(app);
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Timer
// ---------------------------------------------------------------------------

fn tick_timer(app: &mut App) {
    if let Some(game) = &mut app.game {
        if game.status == GameStatus::Playing {
            game.elapsed += TICK_RATE;
        }
    }
}

// ---------------------------------------------------------------------------
// Event handling
// ---------------------------------------------------------------------------

fn term_size() -> (u16, u16) {
    crossterm::terminal::size().unwrap_or((80, 24))
}

fn handle_event(app: &mut App, event: &Event, pregen: &PregenHandle) {
    let (tw, th) = term_size();

    let board_origin = app
        .game
        .as_ref()
        .map_or((0, 0), |g| tui::board_origin(tw, th, &g.config));
    let board_size = app
        .game
        .as_ref()
        .map_or((0, 0), |g| (g.config.width, g.config.height));

    let Some(ev) = tui::translate_event(event, board_origin, board_size, (tw, th), &app.screen)
    else {
        return;
    };

    // UI hover actions — update ui_hover and return, never dispatch further.
    match ev.action {
        GameAction::HoverBack => {
            app.ui_hover = Some(UiHover::Back);
            return;
        }
        GameAction::ClearUiHover => {
            app.ui_hover = None;
            return;
        }
        GameAction::MenuHover(i) => {
            // On leaderboard this means tab hover; on other screens it's menu item hover.
            // Only set ui_hover here for leaderboard; other screens handle it in dispatch.
            if matches!(app.screen, Screen::Leaderboard { .. }) {
                app.ui_hover = Some(UiHover::Tab(i));
                return;
            }
            // Fall through for menu screens — handled as before.
        }
        _ => {}
    }

    // Mouse move on board: take over active_cell and submit pregen job if applicable.
    if ev.action == GameAction::MoveCursor(0, 0) {
        if let Some(pos) = ev.board_pos {
            app.mouse_controlling = true;
            if pos != app.active_cell || !app.cell_active {
                maybe_submit_pregen(app, pregen, pos);
            }
            app.active_cell = pos;
            app.cell_active = true;
        }
        return;
    }

    if let Some(pos) = ev.board_pos {
        app.mouse_controlling = true;
        app.active_cell = pos;
        app.cell_active = true;
    } else {
        app.mouse_controlling = false;
    }
    // Clear ui_hover on any confirmed action.
    app.ui_hover = None;

    dispatch(app, ev.action, ev.board_pos, pregen);
}

/// Submits a pregen job for `pos` when the game is in PreGame state.
fn maybe_submit_pregen(app: &App, pregen: &PregenHandle, pos: (u16, u16)) {
    if matches!(app.screen, Screen::Playing) {
        if let Some(game) = &app.game {
            if game.status == GameStatus::PreGame {
                pregen.submit(pos, game.config);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Action dispatch
// ---------------------------------------------------------------------------

fn dispatch(
    app: &mut App,
    action: GameAction,
    board_pos: Option<(u16, u16)>,
    pregen: &PregenHandle,
) {
    match app.screen.clone() {
        Screen::MainMenu { selected } => dispatch_main_menu(app, action, selected),
        Screen::NewGameMenu { selected } => dispatch_new_game_menu(app, action, selected),
        Screen::Playing => dispatch_playing(app, action, board_pos, pregen),
        Screen::GameOver { selected, won } => dispatch_game_over(app, action, selected, won),
        Screen::Leaderboard { tab } => dispatch_leaderboard(app, action, tab),
    }
}

fn dispatch_main_menu(app: &mut App, action: GameAction, selected: usize) {
    fn confirm(app: &mut App, idx: usize) {
        match idx {
            0 => app.screen = Screen::NewGameMenu { selected: 0 },
            1 => {
                app.screen = Screen::Leaderboard {
                    tab: LeaderboardTab::Beginner,
                }
            }
            _ => app.should_quit = true,
        }
    }
    let item_count = MAIN_MENU_ITEMS.len();
    match action {
        GameAction::Quit => app.should_quit = true,
        GameAction::MoveCursor(0, -1) => {
            app.screen = Screen::MainMenu {
                selected: selected.saturating_sub(1),
            };
        }
        GameAction::MoveCursor(0, 1) => {
            app.screen = Screen::MainMenu {
                selected: (selected + 1).min(item_count - 1),
            };
        }
        GameAction::Reveal => confirm(app, selected),
        GameAction::MenuSelect(idx) => confirm(app, idx),
        GameAction::MenuHover(idx) => app.screen = Screen::MainMenu { selected: idx },
        _ => {}
    }
}

fn dispatch_new_game_menu(app: &mut App, action: GameAction, selected: usize) {
    fn confirm(app: &mut App, idx: usize) {
        match idx {
            0 => start_game(app, Difficulty::Beginner),
            1 => start_game(app, Difficulty::Intermediate),
            _ => start_game(app, Difficulty::Expert),
        }
    }
    let item_count = NEW_GAME_ITEMS.len();
    match action {
        GameAction::OpenMenu | GameAction::Quit => {
            app.screen = Screen::MainMenu { selected: 0 };
        }
        GameAction::MoveCursor(0, -1) => {
            app.screen = Screen::NewGameMenu {
                selected: selected.saturating_sub(1),
            };
        }
        GameAction::MoveCursor(0, 1) => {
            app.screen = Screen::NewGameMenu {
                selected: (selected + 1).min(item_count - 1),
            };
        }
        GameAction::Reveal => confirm(app, selected),
        GameAction::MenuSelect(idx) => confirm(app, idx),
        GameAction::MenuHover(idx) => app.screen = Screen::NewGameMenu { selected: idx },
        _ => {}
    }
}

fn dispatch_playing(
    app: &mut App,
    action: GameAction,
    board_pos: Option<(u16, u16)>,
    pregen: &PregenHandle,
) {
    match action {
        GameAction::Quit | GameAction::OpenMenu => {
            app.screen = Screen::MainMenu { selected: 0 };
            return;
        }
        GameAction::MoveCursor(dx, dy) => {
            if let Some(game) = &app.game {
                let new_pos = game::move_cursor(app.active_cell, dx, dy, &game.config);
                let config = game.config;
                let is_pregame = game.status == GameStatus::PreGame;
                if is_pregame && new_pos != app.active_cell {
                    pregen.submit(new_pos, config);
                }
                app.active_cell = new_pos;
                app.cell_active = true;
            }
            return;
        }
        GameAction::Hint => {
            handle_hint(app);
            return;
        }
        _ => {}
    }

    // Any board-modifying action clears the active hint.
    app.hint = None;

    // Mouse actions use the clicked position; keyboard actions use active_cell.
    let target = board_pos.unwrap_or(app.active_cell);

    let Some(game) = &mut app.game else { return };

    let newly_revealed: Vec<(u16, u16)> = match action {
        GameAction::Reveal => {
            let cell = game.board.get(target.0, target.1);
            if cell.visibility == CellVisibility::Revealed {
                game::chord_reveal(game, target.0, target.1)
            } else {
                // Use the pre-generated board if it's ready and matches the target cell.
                if game.status == GameStatus::PreGame {
                    if let Some(pre_board) = pregen.take(target) {
                        game.board = pre_board;
                        game.status = GameStatus::Playing;
                    }
                }
                game::reveal(game, target.0, target.1)
            }
        }
        GameAction::CycleFlag => {
            let cell = game.board.get(target.0, target.1);
            if cell.visibility == CellVisibility::Revealed {
                game::chord_flag(game, target.0, target.1);
                vec![]
            } else {
                game::cycle_flag(game, target.0, target.1);
                vec![]
            }
        }
        _ => vec![],
    };

    if !newly_revealed.is_empty() {
        let (tw, th) = term_size();
        if let Some(game) = &app.game {
            let origin = tui::board_origin(tw, th, &game.config);
            let stride_w = CELL_WIDTH + BORDER_COL;
            let stride_h = CELL_HEIGHT + BORDER_ROW;
            let count = newly_revealed.len();
            for (cx, cy) in &newly_revealed {
                let x = origin.0 + 1 + cx * stride_w;
                let y = origin.1 + 1 + cy * stride_h;
                let rect = Rect::new(x, y, CELL_WIDTH, CELL_HEIGHT);
                app.effects.push((reveal_effect(count), rect));
            }
        }
    }

    check_game_over(app);
}

fn dispatch_game_over(app: &mut App, action: GameAction, selected: usize, won: bool) {
    fn confirm(app: &mut App, idx: usize) {
        match idx {
            0 => {
                if let Some(game) = &app.game {
                    let difficulty = game.difficulty;
                    start_game(app, difficulty);
                }
            }
            1 => app.screen = Screen::MainMenu { selected: 0 },
            _ => app.should_quit = true,
        }
    }
    let item_count = GAME_OVER_ITEMS.len();
    match action {
        GameAction::Quit => app.should_quit = true,
        GameAction::OpenMenu => app.screen = Screen::MainMenu { selected: 0 },
        GameAction::MoveCursor(-1, 0) => {
            app.screen = Screen::GameOver {
                won,
                selected: selected.saturating_sub(1),
            };
        }
        GameAction::MoveCursor(1, 0) => {
            app.screen = Screen::GameOver {
                won,
                selected: (selected + 1).min(item_count - 1),
            };
        }
        GameAction::Reveal => confirm(app, selected),
        GameAction::MenuSelect(idx) => confirm(app, idx),
        GameAction::MenuHover(idx) => app.screen = Screen::GameOver { won, selected: idx },
        _ => {}
    }
}

fn dispatch_leaderboard(app: &mut App, action: GameAction, tab: LeaderboardTab) {
    match action {
        GameAction::OpenMenu | GameAction::Quit => app.screen = Screen::MainMenu { selected: 0 },
        GameAction::MoveCursor(1, 0) => app.screen = Screen::Leaderboard { tab: tab.next() },
        GameAction::MoveCursor(-1, 0) => app.screen = Screen::Leaderboard { tab: tab.prev() },
        GameAction::MenuSelect(idx) => {
            let new_tab = match idx {
                0 => LeaderboardTab::Beginner,
                1 => LeaderboardTab::Intermediate,
                _ => LeaderboardTab::Expert,
            };
            app.screen = Screen::Leaderboard { tab: new_tab };
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Game lifecycle helpers
// ---------------------------------------------------------------------------

fn start_game(app: &mut App, difficulty: Difficulty) {
    app.game = Some(GameState::new(difficulty));
    app.active_cell = (0, 0);
    app.cell_active = false;
    app.mouse_controlling = true;
    app.hint = None;
    app.screen = Screen::Playing;
}

fn handle_hint(app: &mut App) {
    // ── TESTING ONLY: set to true to auto-apply basic hints and loop ─────────
    const AUTO_APPLY_BASIC_HINTS: bool = false;
    // ─────────────────────────────────────────────────────────────────────────

    loop {
        let Some(game) = &app.game else { return };
        if !game.is_active() {
            return;
        }
        if game.status == GameStatus::PreGame {
            return;
        }

        let n = game.board.cells.len();
        let mut revealed = vec![false; n];
        let mut flagged = vec![false; n];
        for (i, cell) in game.board.cells.iter().enumerate() {
            revealed[i] = cell.visibility == CellVisibility::Revealed;
            flagged[i] = cell.visibility == CellVisibility::Flagged;
        }

        let deduction = solver::hint(
            &game.board,
            &revealed,
            &flagged,
            game.flags_placed,
            game.config.mine_count,
        );

        if let Some(game) = &mut app.game {
            game.hint_used = true;
        }

        let Some(d) = deduction else {
            app.hint = None;
            return;
        };

        let is_basic = !d.uses_global;

        if AUTO_APPLY_BASIC_HINTS && is_basic {
            // Auto-apply: flag mines, reveal safe cells, then loop for next hint.
            let mine_targets = d.mine_cells.clone();
            let safe_targets = d.safe_cells.clone();
            app.hint = None;
            for (x, y) in mine_targets {
                let cell = app.game.as_ref().unwrap().board.get(x, y);
                if cell.visibility == CellVisibility::Hidden
                    || cell.visibility == CellVisibility::Question
                {
                    game::cycle_flag(app.game.as_mut().unwrap(), x, y);
                    // cycle once more if it landed on Question instead of Flagged
                    if app.game.as_ref().unwrap().board.get(x, y).visibility
                        == CellVisibility::Question
                    {
                        game::cycle_flag(app.game.as_mut().unwrap(), x, y);
                    }
                }
            }
            for (x, y) in safe_targets {
                game::reveal(app.game.as_mut().unwrap(), x, y);
            }
            check_game_over(app);
            // Continue loop to compute next hint.
        } else {
            app.hint = Some(HintState {
                mine_targets: d.mine_cells,
                safe_targets: d.safe_cells,
                witnesses: d.witnesses,
                uses_global: d.uses_global,
            });
            return;
        }
    }
}

fn check_game_over(app: &mut App) {
    let Some(game) = &app.game else { return };
    match game.status {
        GameStatus::Won => {
            let elapsed = game.elapsed;
            let difficulty = game.difficulty;
            let hint_used = game.hint_used;
            if !hint_used {
                leaderboard::submit(&mut app.leaderboard, difficulty, elapsed);
            }
            app.screen = Screen::GameOver {
                won: true,
                selected: 0,
            };
        }
        GameStatus::Lost { .. } => {
            let (tw, th) = term_size();
            let det_rect = tui::board_rect(tw, th, &game.config);
            app.effects.push((detonate_effect(), det_rect));
            app.screen = Screen::GameOver {
                won: false,
                selected: 0,
            };
        }
        _ => {}
    }
}
