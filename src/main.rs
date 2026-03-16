use std::time::{Duration, Instant};

use crossterm::event::{self, Event};

use ratatui::layout::Rect;
use minesweepe_rs::game;
use minesweepe_rs::game::{detonate_effect, reveal_effect};
use minesweepe_rs::leaderboard;
use minesweepe_rs::tui;
use minesweepe_rs::tui::{GAME_OVER_ITEMS, MAIN_MENU_ITEMS, NEW_GAME_ITEMS};
use minesweepe_rs::types::{
    App, Difficulty, GameAction, GameState, GameStatus, LeaderboardTab, Screen, UiHover,
    BORDER_COL, BORDER_ROW, CELL_HEIGHT, CELL_WIDTH,
};

const TICK_RATE: Duration = Duration::from_millis(250);
const EFFECT_TICK: Duration = Duration::from_millis(16); // ~60fps when effects running

fn main() -> anyhow::Result<()> {
    let lb = leaderboard::load();
    let mut app = App::new(lb);
    let mut terminal = tui::init()?;

    let result = run(&mut terminal, &mut app);

    tui::restore(&mut terminal)?;
    leaderboard::save(&app.leaderboard);

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }
    Ok(())
}

fn run(terminal: &mut tui::Tui, app: &mut App) -> anyhow::Result<()> {
    let mut last_tick = Instant::now();

    loop {
        tui::draw(terminal, app)?;

        // Use faster tick while effects are running for smooth animation.
        let tick = if !app.effects.is_empty() { EFFECT_TICK } else { TICK_RATE };
        let timeout = tick.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            let raw = event::read()?;
            handle_event(app, &raw);
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

fn handle_event(app: &mut App, event: &Event) {
    let (tw, th) = crossterm::terminal::size().unwrap_or((80, 24));

    let board_origin = app.game.as_ref().map_or((0, 0), |g| {
        tui::board_origin(tw, th, &g.config)
    });
    let board_size = app.game.as_ref().map_or((0, 0), |g| {
        (g.config.width, g.config.height)
    });

    let Some(ev) = tui::translate_event(event, board_origin, board_size, (tw, th), &app.screen) else {
        return;
    };

    // UI hover actions — update ui_hover and return, never dispatch further.
    match ev.action {
        GameAction::HoverBack => { app.ui_hover = Some(UiHover::Back); return; }
        GameAction::ClearUiHover => { app.ui_hover = None; return; }
        GameAction::HoverGameOverItem(i) => {
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

    // Mouse move on board: take over active_cell.
    if ev.action == GameAction::MoveCursor(0, 0) {
        if let Some(pos) = ev.board_pos {
            app.mouse_controlling = true;
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

    dispatch(app, ev.action, ev.board_pos);
}

// ---------------------------------------------------------------------------
// Action dispatch
// ---------------------------------------------------------------------------

fn dispatch(app: &mut App, action: GameAction, board_pos: Option<(u16, u16)>) {
    match app.screen.clone() {
        Screen::MainMenu { selected }    => dispatch_main_menu(app, action, selected),
        Screen::NewGameMenu { selected } => dispatch_new_game_menu(app, action, selected),
        Screen::Playing                  => dispatch_playing(app, action, board_pos),
        Screen::GameOver { selected, .. } => dispatch_game_over(app, action, selected),
        Screen::Leaderboard { tab }      => dispatch_leaderboard(app, action, tab),
    }
}

fn dispatch_main_menu(app: &mut App, action: GameAction, selected: usize) {
    let item_count = MAIN_MENU_ITEMS.len();
    match action {
        GameAction::Quit => app.should_quit = true,
        GameAction::MoveCursor(0, -1) => {
            app.screen = Screen::MainMenu { selected: selected.saturating_sub(1) };
        }
        GameAction::MoveCursor(0, 1) => {
            app.screen = Screen::MainMenu { selected: (selected + 1).min(item_count - 1) };
        }
        GameAction::Reveal => confirm_main_menu(app, selected),
        // Mouse hover: update highlighted item
        GameAction::HoverGameOverItem(idx) => {
            app.screen = Screen::MainMenu { selected: idx };
        }
        // Mouse click: confirm item
        GameAction::SelectGameOverItem(idx) => confirm_main_menu(app, idx),
        _ => {}
    }
}

fn confirm_main_menu(app: &mut App, selected: usize) {
    match selected {
        0 => app.screen = Screen::NewGameMenu { selected: 0 },
        1 => app.screen = Screen::Leaderboard { tab: LeaderboardTab::Beginner },
        _ => app.should_quit = true,
    }
}

fn dispatch_new_game_menu(app: &mut App, action: GameAction, selected: usize) {
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
        GameAction::Reveal => confirm_new_game(app, selected),
        GameAction::HoverGameOverItem(idx) => {
            app.screen = Screen::NewGameMenu { selected: idx };
        }
        GameAction::SelectGameOverItem(idx) => confirm_new_game(app, idx),
        _ => {}
    }
}

fn confirm_new_game(app: &mut App, selected: usize) {
    let difficulty = match selected {
        0 => Difficulty::Beginner,
        1 => Difficulty::Intermediate,
        2 => Difficulty::Expert,
        // Custom config UI not yet implemented; fall back to Beginner.
        _ => Difficulty::Beginner,
    };
    start_game(app, difficulty);
}

fn dispatch_playing(app: &mut App, action: GameAction, board_pos: Option<(u16, u16)>) {
    match action {
        GameAction::Quit | GameAction::OpenMenu => {
            app.screen = Screen::MainMenu { selected: 0 };
            return;
        }
        GameAction::MoveCursor(dx, dy) => {
            if let Some(game) = &app.game {
                let new_pos = game::move_cursor(app.active_cell, dx, dy, &game.config);
                app.active_cell = new_pos;
                app.cell_active = true;
            }
            return;
        }
        _ => {}
    }

    // Mouse actions use the clicked position; keyboard actions use active_cell.
    let target = board_pos.unwrap_or(app.active_cell);

    let Some(game) = &mut app.game else { return };

    let newly_revealed: Vec<(u16, u16)> = match action {
        GameAction::Reveal => {
            let cell = game.board.get(target.0, target.1);
            if cell.visibility == minesweepe_rs::types::CellVisibility::Revealed {
                game::chord(game, target.0, target.1)
            } else {
                game::reveal(game, target.0, target.1)
            }
        }
        GameAction::CycleFlag => { game::cycle_flag(game, target.0, target.1); vec![] }
        GameAction::Chord     => game::chord(game, target.0, target.1),
        _ => vec![],
    };

    let flag_changed = matches!(action, GameAction::CycleFlag);
    let was_lost = matches!(app.game.as_ref().map(|g| &g.status), Some(GameStatus::Lost { .. }));

    if !newly_revealed.is_empty() {
        // Apply coalesce effect to each newly revealed cell individually.
        let (tw, th) = crossterm::terminal::size().unwrap_or((80, 24));
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

    if !newly_revealed.is_empty() || flag_changed || was_lost {
        check_game_over(app);
    }
}

fn dispatch_game_over(app: &mut App, action: GameAction, selected: usize) {
    let item_count = GAME_OVER_ITEMS.len();
    match action {
        GameAction::Quit     => app.should_quit = true,
        GameAction::OpenMenu => app.screen = Screen::MainMenu { selected: 0 },
        // Keyboard navigation
        GameAction::MoveCursor(0, -1) => {
            app.screen = Screen::GameOver {
                won: matches!(app.screen, Screen::GameOver { won: true, .. }),
                selected: selected.saturating_sub(1),
            };
        }
        GameAction::MoveCursor(0, 1) => {
            app.screen = Screen::GameOver {
                won: matches!(app.screen, Screen::GameOver { won: true, .. }),
                selected: (selected + 1).min(item_count - 1),
            };
        }
        GameAction::Reveal => confirm_game_over(app, selected),
        // Mouse: hover updates selected highlight
        GameAction::HoverGameOverItem(idx) => {
            app.screen = Screen::GameOver {
                won: matches!(app.screen, Screen::GameOver { won: true, .. }),
                selected: idx,
            };
        }
        // Mouse: click confirms
        GameAction::SelectGameOverItem(idx) => confirm_game_over(app, idx),
        _ => {}
    }
}

fn confirm_game_over(app: &mut App, selected: usize) {
    match selected {
        0 => {
            // New Game
            if let Some(game) = &app.game {
                let difficulty = game.difficulty;
                start_game(app, difficulty);
            }
        }
        1 => app.screen = Screen::MainMenu { selected: 0 },
        _ => app.should_quit = true,
    }
}

fn dispatch_leaderboard(app: &mut App, action: GameAction, tab: LeaderboardTab) {
    match action {
        GameAction::OpenMenu | GameAction::Quit => app.screen = Screen::MainMenu { selected: 0 },
        GameAction::MoveCursor(1, 0)            => app.screen = Screen::Leaderboard { tab: tab.next() },
        GameAction::MoveCursor(-1, 0)           => app.screen = Screen::Leaderboard { tab: tab.prev() },
        // Mouse click on a tab zone (0=Beginner, 1=Intermediate, 2=Expert)
        GameAction::SelectGameOverItem(idx) => {
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
    app.screen = Screen::Playing;
}

fn check_game_over(app: &mut App) {
    let Some(game) = &app.game else { return };
    match game.status {
        GameStatus::Won => {
            let elapsed = game.elapsed;
            let difficulty = game.difficulty;
            leaderboard::submit(&mut app.leaderboard, difficulty, elapsed);
            app.screen = Screen::GameOver { won: true, selected: 0 };
        }
        GameStatus::Lost { .. } => {
            let (tw, th) = crossterm::terminal::size().unwrap_or((80, 24));
            let det_rect = tui::board_rect(tw, th, &game.config);
            app.effects.push((detonate_effect(), det_rect));
            app.screen = Screen::GameOver { won: false, selected: 0 };
        }
        _ => {}
    }
}
